use crate::spec_fetch::AuthMode;
use crate::spec_registry::{self, SpecSource};
use std::path::PathBuf;

pub fn run(args: &[String]) -> Result<String, String> {
    match parse_command(args) {
        Command::Version => Ok(version_text()),
        Command::Help => Ok(help_text()),
        Command::Spec(spec_command) => run_spec_command(spec_command),
        Command::Unknown(command) => Err(format!("unknown command: {command}\n\n{}", help_text())),
    }
}

enum Command {
    Version,
    Help,
    Spec(SpecCommand),
    Unknown(String),
}

enum SpecCommand {
    Add(SpecAddArgs),
    List,
}

struct SpecAddArgs {
    service_id: String,
    source: SpecAddSource,
    auth_mode: AuthMode,
}

enum SpecAddSource {
    File(PathBuf),
    Url(String),
}

fn parse_command(args: &[String]) -> Command {
    match args.get(1).map(String::as_str) {
        Some("version") | Some("--version") | Some("-V") => Command::Version,
        Some("help") | Some("--help") | Some("-h") | None => Command::Help,
        Some("spec") => match parse_spec_command(args) {
            Ok(command) => Command::Spec(command),
            Err(message) => Command::Unknown(format!("spec {message}")),
        },
        Some(other) => Command::Unknown(other.to_string()),
    }
}

fn parse_spec_command(args: &[String]) -> Result<SpecCommand, String> {
    match args.get(2).map(String::as_str) {
        Some("list") => Ok(SpecCommand::List),
        Some("add") => parse_spec_add(args),
        Some(other) => Err(format!("unknown subcommand: {other}")),
        None => Err("missing subcommand".to_string()),
    }
}

fn parse_spec_add(args: &[String]) -> Result<SpecCommand, String> {
    let mut service_id: Option<String> = None;
    let mut file_path: Option<PathBuf> = None;
    let mut url: Option<String> = None;
    let mut auth_mode = AuthMode::None;
    let mut position = 3;

    while position < args.len() {
        let flag = args[position].as_str();
        let value = args
            .get(position + 1)
            .ok_or_else(|| format!("missing value for flag: {flag}"))?
            .to_string();

        match flag {
            "--service" => service_id = Some(value),
            "--file" => file_path = Some(PathBuf::from(value)),
            "--url" => url = Some(value),
            "--auth-bearer" => auth_mode = AuthMode::Bearer(value),
            "--auth-basic" => {
                let (username, password) = value
                    .split_once(':')
                    .ok_or_else(|| "--auth-basic expects username:password".to_string())?;
                auth_mode = AuthMode::Basic {
                    username: username.to_string(),
                    password: password.to_string(),
                };
            }
            "--auth-apikey-header" => {
                let (name, key_value) = value
                    .split_once(':')
                    .ok_or_else(|| "--auth-apikey-header expects header:value".to_string())?;
                auth_mode = AuthMode::ApiKeyHeader {
                    name: name.to_string(),
                    value: key_value.to_string(),
                };
            }
            "--auth-apikey-query" => {
                let (name, key_value) = value
                    .split_once(':')
                    .ok_or_else(|| "--auth-apikey-query expects key:value".to_string())?;
                auth_mode = AuthMode::ApiKeyQuery {
                    name: name.to_string(),
                    value: key_value.to_string(),
                };
            }
            _ => return Err(format!("unknown flag: {flag}")),
        }

        position += 2;
    }

    let service_id = service_id.ok_or_else(|| "missing required flag: --service".to_string())?;

    let source = match (file_path, url) {
        (Some(path), None) => SpecAddSource::File(path),
        (None, Some(url_value)) => SpecAddSource::Url(url_value),
        (Some(_), Some(_)) => return Err("use either --file or --url, not both".to_string()),
        (None, None) => return Err("missing required source flag: --file or --url".to_string()),
    };

    if matches!(source, SpecAddSource::File(_)) && !matches!(auth_mode, AuthMode::None) {
        return Err("auth flags can only be used with --url".to_string());
    }

    Ok(SpecCommand::Add(SpecAddArgs {
        service_id,
        source,
        auth_mode,
    }))
}

fn run_spec_command(command: SpecCommand) -> Result<String, String> {
    match command {
        SpecCommand::List => {
            let entries = spec_registry::list_specs()?;
            if entries.is_empty() {
                return Ok("no specs registered".to_string());
            }

            let mut output =
                String::from("service\tdigest\tsource_format\tspec_kind\tdeclared_version\tsource\n");
            for entry in entries {
                output.push_str(&format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\n",
                    entry.service_id,
                    entry.digest,
                    entry.source_format,
                    entry.spec_kind,
                    entry.declared_version,
                    entry.source
                ));
            }

            Ok(output.trim_end().to_string())
        }
        SpecCommand::Add(args) => {
            let source = match args.source {
                SpecAddSource::File(path) => SpecSource::File(path),
                SpecAddSource::Url(url) => SpecSource::Url {
                    url,
                    auth: args.auth_mode,
                },
            };

            let stored = spec_registry::add_spec(&args.service_id, source)?;
            Ok(format!(
                "added spec: service={} digest={} kind={} declared_version={} format={}",
                stored.service_id,
                stored.digest,
                stored.spec_kind,
                stored.declared_version,
                stored.source_format
            ))
        }
    }
}

fn version_text() -> String {
    format!("specgate {}", env!("CARGO_PKG_VERSION"))
}

fn help_text() -> String {
    "specgate 0.1.0\n\nUSAGE:\n    specgate <COMMAND>\n\nCOMMANDS:\n    version  Print CLI version\n    help     Print this help text\n    spec     Manage API specs\n\nSPEC COMMANDS:\n    spec add --service <id> --file <path>\n    spec add --service <id> --url <url> [--auth-bearer <token> | --auth-basic <user:pass> | --auth-apikey-header <header:value> | --auth-apikey-query <key:value>]\n    spec list"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn run_version_returns_version() {
        let actual =
            run(&args(&["specgate", "version"])).expect("expected version command to succeed");

        assert_eq!(actual, format!("specgate {}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn run_help_returns_usage_text() {
        let actual = run(&args(&["specgate", "help"])).expect("expected help command to succeed");

        assert!(actual.contains("USAGE:"));
        assert!(actual.contains("version"));
        assert!(actual.contains("spec add"));
    }

    #[test]
    fn unknown_command_returns_error() {
        let error =
            run(&args(&["specgate", "nope"])).expect_err("expected unknown command to fail");

        assert!(error.contains("unknown command: nope"));
        assert!(error.contains("USAGE:"));
    }

    #[test]
    fn spec_without_subcommand_returns_error() {
        let error = run(&args(&["specgate", "spec"])).expect_err("expected spec command to fail");

        assert!(error.contains("unknown command: spec missing subcommand"));
    }
}

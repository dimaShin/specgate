pub fn run(args: &[String]) -> Result<String, String> {
    match parse_command(args) {
        Command::Version => Ok(version_text()),
        Command::Help => Ok(help_text()),
        Command::Unknown(command) => Err(format!("unknown command: {command}\n\n{}", help_text())),
    }
}

enum Command {
    Version,
    Help,
    Unknown(String),
}

fn parse_command(args: &[String]) -> Command {
    match args.get(1).map(String::as_str) {
        Some("version") | Some("--version") | Some("-V") => Command::Version,
        Some("help") | Some("--help") | Some("-h") | None => Command::Help,
        Some(other) => Command::Unknown(other.to_string()),
    }
}

fn version_text() -> String {
    format!("specgate {}", env!("CARGO_PKG_VERSION"))
}

fn help_text() -> String {
    "specgate 0.1.0\n\nUSAGE:\n    specgate <COMMAND>\n\nCOMMANDS:\n    version  Print CLI version\n    help     Print this help text"
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
    }

    #[test]
    fn unknown_command_returns_error() {
        let error =
            run(&args(&["specgate", "nope"])).expect_err("expected unknown command to fail");

        assert!(error.contains("unknown command: nope"));
        assert!(error.contains("USAGE:"));
    }
}

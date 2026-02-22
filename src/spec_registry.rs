use crate::spec_adapters::detect_spec;
use crate::spec_fetch::{fetch_url, AuthMode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub enum SpecSource {
    File(PathBuf),
    Url { url: String, auth: AuthMode },
}

#[derive(Debug, Clone)]
pub struct StoredSpec {
    pub service_id: String,
    pub digest: String,
    pub source_format: String,
    pub spec_kind: String,
    pub declared_version: String,
}

#[derive(Debug, Clone)]
pub struct ActiveSpec {
    pub service_id: String,
    pub digest: String,
    pub spec_kind: String,
    pub declared_version: String,
    pub spec_file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SpecListEntry {
    pub service_id: String,
    pub digest: String,
    pub active: bool,
    pub source_format: String,
    pub spec_kind: String,
    pub declared_version: String,
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SpecMetadata {
    service_id: String,
    digest: String,
    source_format: String,
    spec_kind: String,
    declared_version: String,
    source: String,
    stored_at_epoch_secs: u64,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct ServiceState {
    active_digest: Option<String>,
}

pub fn add_spec(service_id: &str, source: SpecSource) -> Result<StoredSpec, String> {
    validate_service_id(service_id)?;

    let (bytes, source_label) = read_source(source)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let detected = detect_spec(&bytes)?;

    let source_format = detected.format.as_str().to_string();
    let spec_kind = detected.identity.kind.as_str().to_string();
    let declared_version = detected.identity.declared_version;

    let root = registry_root_dir()?;
    let version_dir = root.join(service_id).join(&digest);
    fs::create_dir_all(&version_dir)
        .map_err(|error| format!("failed to create registry directory: {error}"))?;

    let spec_file = version_dir.join(format!("spec.{source_format}"));
    fs::write(&spec_file, bytes).map_err(|error| format!("failed to write spec file: {error}"))?;

    let metadata = SpecMetadata {
        service_id: service_id.to_string(),
        digest: digest.clone(),
        source_format: source_format.clone(),
        spec_kind: spec_kind.clone(),
        declared_version: declared_version.clone(),
        source: source_label,
        stored_at_epoch_secs: current_epoch_seconds(),
    };

    let metadata_file = version_dir.join("metadata.json");
    let metadata_json = serde_json::to_vec_pretty(&metadata)
        .map_err(|error| format!("failed to encode metadata: {error}"))?;
    fs::write(metadata_file, metadata_json)
        .map_err(|error| format!("failed to write metadata file: {error}"))?;

    set_active_digest(service_id, &digest)?;

    Ok(StoredSpec {
        service_id: service_id.to_string(),
        digest,
        source_format,
        spec_kind,
        declared_version,
    })
}

pub fn list_specs() -> Result<Vec<SpecListEntry>, String> {
    let root = registry_root_dir()?;
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    let service_dirs = read_sorted_directories(&root)?;

    for service_dir in service_dirs {
        let service_id = service_dir
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "invalid service directory name".to_string())?
            .to_string();

        let service_state = read_service_state(&service_dir)?;

        let version_dirs = read_sorted_directories(&service_dir)?;
        for version_dir in version_dirs {
            let metadata_file = version_dir.join("metadata.json");
            if !metadata_file.exists() {
                continue;
            }

            let metadata_raw = fs::read(&metadata_file)
                .map_err(|error| format!("failed to read metadata file: {error}"))?;
            let metadata: SpecMetadata = serde_json::from_slice(&metadata_raw)
                .map_err(|error| format!("failed to parse metadata file: {error}"))?;
            let is_active = service_state.active_digest.as_deref() == Some(metadata.digest.as_str());

            entries.push(SpecListEntry {
                service_id: service_id.clone(),
                digest: metadata.digest,
                active: is_active,
                source_format: metadata.source_format,
                spec_kind: metadata.spec_kind,
                declared_version: metadata.declared_version,
                source: metadata.source,
            });
        }
    }

    entries.sort_by(|left, right| {
        left.service_id
            .cmp(&right.service_id)
            .then(left.digest.cmp(&right.digest))
    });

    Ok(entries)
}

pub fn resolve_active_spec(service_id: &str) -> Result<Option<ActiveSpec>, String> {
    validate_service_id(service_id)?;

    let root = registry_root_dir()?;
    let service_dir = root.join(service_id);
    if !service_dir.exists() {
        return Ok(None);
    }

    let service_state = read_service_state(&service_dir)?;
    let digest = match service_state.active_digest {
        Some(value) => value,
        None => return Ok(None),
    };

    let version_dir = service_dir.join(&digest);
    if !version_dir.exists() {
        return Ok(None);
    }

    let metadata = read_metadata(&version_dir)?;
    let spec_file = version_dir.join(format!("spec.{}", metadata.source_format));
    if !spec_file.exists() {
        return Ok(None);
    }

    Ok(Some(ActiveSpec {
        service_id: metadata.service_id,
        digest: metadata.digest,
        spec_kind: metadata.spec_kind,
        declared_version: metadata.declared_version,
        spec_file,
    }))
}

fn registry_root_dir() -> Result<PathBuf, String> {
    if let Ok(override_path) = std::env::var("SPECGATE_REGISTRY_DIR") {
        return Ok(PathBuf::from(override_path).join("specs"));
    }

    let cwd = std::env::current_dir().map_err(|error| format!("failed to get current directory: {error}"))?;
    Ok(cwd.join(".specgate").join("specs"))
}

fn read_source(source: SpecSource) -> Result<(Vec<u8>, String), String> {
    match source {
        SpecSource::File(path) => {
            let bytes = fs::read(&path)
                .map_err(|error| format!("failed to read spec file {}: {error}", path.display()))?;
            Ok((bytes, format!("file:{}", path.display())))
        }
        SpecSource::Url { url, auth } => {
            let bytes = fetch_url(&url, &auth)?;
            Ok((bytes, format!("url:{url}")))
        }
    }
}

fn validate_service_id(service_id: &str) -> Result<(), String> {
    if service_id.is_empty() {
        return Err("service id must not be empty".to_string());
    }

    let allowed = service_id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_');

    if !allowed {
        return Err("service id may contain only letters, numbers, '-' and '_'".to_string());
    }

    Ok(())
}

fn read_sorted_directories(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut dirs = Vec::new();
    for item in fs::read_dir(path).map_err(|error| format!("failed to read directory: {error}"))? {
        let entry = item.map_err(|error| format!("failed to read directory entry: {error}"))?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            dirs.push(entry_path);
        }
    }

    dirs.sort();
    Ok(dirs)
}

fn read_metadata(version_dir: &Path) -> Result<SpecMetadata, String> {
    let metadata_file = version_dir.join("metadata.json");
    let metadata_raw = fs::read(&metadata_file)
        .map_err(|error| format!("failed to read metadata file: {error}"))?;
    serde_json::from_slice(&metadata_raw)
        .map_err(|error| format!("failed to parse metadata file: {error}"))
}

fn read_service_state(service_dir: &Path) -> Result<ServiceState, String> {
    let state_file = service_dir.join("service_state.json");
    if !state_file.exists() {
        return Ok(ServiceState::default());
    }

    let raw = fs::read(&state_file)
        .map_err(|error| format!("failed to read service state file: {error}"))?;
    serde_json::from_slice(&raw)
        .map_err(|error| format!("failed to parse service state file: {error}"))
}

fn set_active_digest(service_id: &str, digest: &str) -> Result<(), String> {
    let root = registry_root_dir()?;
    let service_dir = root.join(service_id);
    fs::create_dir_all(&service_dir)
        .map_err(|error| format!("failed to create service directory: {error}"))?;

    let state = ServiceState {
        active_digest: Some(digest.to_string()),
    };
    let state_json = serde_json::to_vec_pretty(&state)
        .map_err(|error| format!("failed to encode service state: {error}"))?;
    fs::write(service_dir.join("service_state.json"), state_json)
        .map_err(|error| format!("failed to write service state file: {error}"))
}

fn current_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_service_id_rejects_invalid_characters() {
        let error = validate_service_id("pet/api").expect_err("expected invalid service id");

        assert!(error.contains("service id may contain only"));
    }
}

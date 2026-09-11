//! TypeWhisper discovery file parsing.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Discovery {
    pub port: u16,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub version: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error(
        "TypeWhisper discovery file not found at {0} — is TypeWhisper running with the API server enabled?"
    )]
    Missing(PathBuf),
    #[error("cannot determine home directory")]
    NoHome,
    #[error("invalid discovery file: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("discovery io error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn discovery_path() -> Result<PathBuf, DiscoveryError> {
    let home = dirs::home_dir().ok_or(DiscoveryError::NoHome)?;
    Ok(home.join("Library/Application Support/TypeWhisper/api-discovery.json"))
}

pub fn load(path: &Path) -> Result<Discovery, DiscoveryError> {
    if !path.exists() {
        return Err(DiscoveryError::Missing(path.to_path_buf()));
    }
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_discovery() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("api-discovery.json");
        std::fs::write(&p, r#"{"port":8978,"token":"abc","version":1}"#).unwrap();
        let d = load(&p).unwrap();
        assert_eq!(d.port, 8978);
        assert_eq!(d.token.as_deref(), Some("abc"));
    }

    #[test]
    fn missing_file_is_missing_error() {
        assert!(matches!(
            load(std::path::Path::new("/nonexistent/x.json")),
            Err(DiscoveryError::Missing(_))
        ));
    }

    #[test]
    fn corrupt_json_is_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("d.json");
        std::fs::write(&p, "not json").unwrap();
        assert!(matches!(load(&p), Err(DiscoveryError::Parse(_))));
    }

    #[test]
    fn token_is_optional() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("d.json");
        std::fs::write(&p, r#"{"port":9000}"#).unwrap();
        assert_eq!(load(&p).unwrap().token, None);
    }
}

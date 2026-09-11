//! Configuration loading and defaults.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_journal_path")]
    pub journal_path: PathBuf,
}

const DEFAULT_JOURNAL_PATH: &str = "~/Documents/VoiceJournal.md";

fn default_hotkey() -> String {
    "Ctrl+Alt+KeyJ".into()
}

fn default_journal_path() -> PathBuf {
    expand_tilde(DEFAULT_JOURNAL_PATH)
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hotkey: default_hotkey(),
            journal_path: default_journal_path(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("hotkey must not be empty")]
    EmptyHotkey,
    #[error("config io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid config: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("cannot determine home directory")]
    NoHome,
}

pub fn config_path_from(env_value: Option<&str>) -> Result<PathBuf, ConfigError> {
    if let Some(p) = env_value.filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let dir = dirs::config_dir()
        .ok_or(ConfigError::NoHome)?
        .join("voice-journal");
    Ok(dir.join("config.toml"))
}

pub fn config_path() -> Result<PathBuf, ConfigError> {
    config_path_from(std::env::var("VOICE_JOURNAL_CONFIG").ok().as_deref())
}

pub fn expand_tilde(input: &str) -> PathBuf {
    if input == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home;
    }
    if let Some(rest) = input.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(input)
}

pub fn parse(s: &str) -> Result<Config, ConfigError> {
    let mut c: Config = toml::from_str(s)?;
    if c.hotkey.trim().is_empty() {
        return Err(ConfigError::EmptyHotkey);
    }
    c.journal_path = expand_tilde(&c.journal_path.to_string_lossy());
    Ok(c)
}

pub fn load_or_create(path: &Path) -> Result<Config, ConfigError> {
    if path.exists() {
        return parse(&std::fs::read_to_string(path)?);
    }
    let default = Config::default();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = format!(
        "# Global toggle hotkey (global-hotkey key syntax).\nhotkey = \"{}\"\n\n# Journal destination. ~ is expanded.\njournal_path = \"{}\"\n",
        default.hotkey, DEFAULT_JOURNAL_PATH
    );
    std::fs::write(path, body)?;
    Ok(default)
}

pub fn load(config_path: &Path) -> Result<Config, ConfigError> {
    parse(&std::fs::read_to_string(config_path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_toggle_hotkey_and_documents_path() {
        let c = Config::default();
        assert_eq!(c.hotkey, "Ctrl+Alt+KeyJ");
        assert!(c.journal_path.ends_with("Documents/VoiceJournal.md"));
    }

    #[test]
    fn parses_valid_toml() {
        let c = parse(
            r#"hotkey = "Cmd+Shift+Space"
journal_path = "/tmp/j.md""#,
        )
        .unwrap();
        assert_eq!(c.hotkey, "Cmd+Shift+Space");
        assert_eq!(c.journal_path, std::path::PathBuf::from("/tmp/j.md"));
    }

    #[test]
    fn rejects_empty_hotkey() {
        assert!(matches!(
            parse(r#"hotkey = "  ""#).unwrap_err(),
            ConfigError::EmptyHotkey
        ));
    }

    #[test]
    fn expands_tilde() {
        let expanded = expand_tilde("~/x/y.md");
        assert!(expanded.ends_with("x/y.md"));
        assert!(!expanded.to_string_lossy().starts_with('~'));
    }

    #[test]
    fn creates_default_file_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let c = load_or_create(&path).unwrap();
        assert_eq!(c, Config::default());
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("Ctrl+Alt+KeyJ")
        );
    }

    #[test]
    fn explicit_config_path_overrides_default() {
        let path = config_path_from(Some("/tmp/custom.toml")).unwrap();
        assert_eq!(path, std::path::PathBuf::from("/tmp/custom.toml"));
        let default_like = config_path_from(Some("")).unwrap();
        assert!(default_like.ends_with("voice-journal/config.toml"));
    }

    #[test]
    fn created_config_round_trips_with_expanded_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let created = load_or_create(&path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(created, loaded);
        assert!(!loaded.journal_path.to_string_lossy().starts_with('~'));
    }
}

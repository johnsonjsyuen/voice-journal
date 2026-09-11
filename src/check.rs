//! Headless `--check` report.

use crate::{api, config, discovery};
use std::path::Path;

#[derive(Debug)]
pub struct Report {
    pub lines: Vec<String>,
    pub ok: bool,
}

pub fn run(config_path: &Path, discovery_path: &Path) -> Report {
    let mut lines = Vec::new();
    let mut ok = true;

    let cfg = match config::load_or_create(config_path) {
        Ok(cfg) => {
            lines.push(format!("config: ok ({})", config_path.display()));
            cfg
        }
        Err(e) => {
            lines.push(format!("config: error: {e}"));
            return Report { lines, ok: false };
        }
    };

    match check_journal(&cfg.journal_path) {
        Ok(()) => lines.push(format!("journal: ok ({})", cfg.journal_path.display())),
        Err(e) => {
            lines.push(format!(
                "journal: error: cannot append to {}: {e}",
                cfg.journal_path.display()
            ));
            ok = false;
        }
    }

    if cfg.hotkey.trim().is_empty() {
        lines.push("hotkey: error: hotkey must not be empty (example: Ctrl+Alt+KeyJ)".into());
        ok = false;
    } else {
        #[cfg(target_os = "macos")]
        {
            use std::str::FromStr;
            match global_hotkey::hotkey::HotKey::from_str(&cfg.hotkey) {
                Ok(_) => lines.push(format!("hotkey: {}", cfg.hotkey)),
                Err(e) => {
                    lines.push(format!(
                        "hotkey: error: cannot parse {:?}: {e} (accepted syntax example: Ctrl+Alt+KeyJ)",
                        cfg.hotkey
                    ));
                    ok = false;
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        lines.push(format!("hotkey: {}", cfg.hotkey));
    }

    match discovery::load(discovery_path) {
        Ok(d) => {
            let token = if d.token.as_deref().is_some_and(|t| !t.is_empty()) {
                "set"
            } else {
                "not set"
            };
            lines.push(format!("discovery: ok (port {}, token {token})", d.port));
            match api::probe_status(&d) {
                Ok(()) => lines.push(format!("api: ok (port {} /v1/status)", d.port)),
                Err(e) => lines.push(format!("api: warning: {e}")),
            }
        }
        Err(e) => lines.push(format!("discovery: warning: {e}")),
    }

    Report { lines, ok }
}

pub fn run_cli() -> i32 {
    let config_path = match config::config_path() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("config path error: {e}");
            return 1;
        }
    };
    let discovery_path = match discovery::discovery_path() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("discovery path error: {e}");
            return 1;
        }
    };

    let report = run(&config_path, &discovery_path);
    for line in &report.lines {
        println!("{line}");
    }
    if report.ok { 0 } else { 1 }
}

fn check_journal(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        // Validate the actual journal path, not just its parent: it may be a
        // directory or a file we cannot append to.
        std::fs::OpenOptions::new().append(true).open(path)?;
        Ok(())
    } else {
        probe_writable(journal_parent(path))
    }
}

fn journal_parent(journal_path: &Path) -> &Path {
    journal_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn probe_writable(parent: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(parent)?;
    let probe = parent.join(format!(".voice-journal-check-{}.probe", std::process::id()));
    let write = std::fs::write(&probe, b"");
    let remove = std::fs::remove_file(&probe);
    write?;
    remove
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn write_config(dir: &Path, hotkey: &str, journal_path: &Path) -> PathBuf {
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            format!(
                "hotkey = \"{hotkey}\"\njournal_path = \"{}\"\n",
                journal_path.display()
            ),
        )
        .unwrap();
        path
    }

    fn missing_discovery(dir: &Path) -> PathBuf {
        dir.join("no-such-discovery.json")
    }

    #[test]
    fn healthy_config_and_journal_is_ok() {
        let config_dir = tempfile::tempdir().unwrap();
        let journal_dir = tempfile::tempdir().unwrap();
        let journal_path = journal_dir.path().join("journal.md");
        let config_path = write_config(config_dir.path(), "Ctrl+Alt+KeyJ", &journal_path);

        let report = run(&config_path, &missing_discovery(config_dir.path()));

        assert!(report.ok, "unexpected lines: {:?}", report.lines);
        assert!(
            report
                .lines
                .iter()
                .any(|l| l.contains(&journal_path.to_string_lossy().into_owned()))
        );
        assert!(report.lines.iter().any(|l| {
            let lower = l.to_lowercase();
            lower.contains("typewhisper") || lower.contains("discovery")
        }));
        assert!(
            !journal_path.exists(),
            "check must not create the journal file"
        );
        assert_eq!(
            std::fs::read_dir(journal_dir.path()).unwrap().count(),
            0,
            "probe file must be cleaned up"
        );
    }

    #[test]
    fn invalid_toml_is_not_ok() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, "not = valid = toml").unwrap();

        let report = run(&config_path, &missing_discovery(dir.path()));

        assert!(!report.ok, "unexpected lines: {:?}", report.lines);
    }

    #[test]
    fn missing_discovery_is_warning_not_failure() {
        let dir = tempfile::tempdir().unwrap();
        let journal_path = dir.path().join("journal.md");
        let config_path = write_config(dir.path(), "Ctrl+Alt+KeyJ", &journal_path);

        let report = run(&config_path, &missing_discovery(dir.path()));

        assert!(report.ok, "unexpected lines: {:?}", report.lines);
        assert!(report.lines.iter().any(|l| {
            let lower = l.to_lowercase();
            lower.contains("typewhisper") || lower.contains("discovery")
        }));
    }

    #[test]
    fn discovery_present_reports_port() {
        let dir = tempfile::tempdir().unwrap();
        let journal_path = dir.path().join("journal.md");
        let config_path = write_config(dir.path(), "Ctrl+Alt+KeyJ", &journal_path);
        let discovery_path = dir.path().join("api-discovery.json");
        std::fs::write(&discovery_path, r#"{"port":9123,"token":"secret"}"#).unwrap();

        let report = run(&config_path, &discovery_path);

        assert!(report.ok, "unexpected lines: {:?}", report.lines);
        assert!(
            report.lines.iter().any(|l| l.contains("9123")),
            "port missing from lines: {:?}",
            report.lines
        );
        assert!(
            report
                .lines
                .iter()
                .any(|l| l.to_lowercase().contains("token")),
            "token status missing from lines: {:?}",
            report.lines
        );
    }

    #[test]
    fn unwritable_journal_parent_is_not_ok() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "i am a file").unwrap();
        let journal_path = blocker.join("j.md");
        let config_path = write_config(dir.path(), "Ctrl+Alt+KeyJ", &journal_path);

        let report = run(&config_path, &missing_discovery(dir.path()));

        assert!(!report.ok, "unexpected lines: {:?}", report.lines);
    }

    #[test]
    fn journal_path_that_is_a_directory_is_not_ok() {
        let dir = tempfile::tempdir().unwrap();
        let journal_path = dir.path().join("VoiceJournal.md");
        std::fs::create_dir(&journal_path).unwrap();
        let config_path = write_config(dir.path(), "Ctrl+Alt+KeyJ", &journal_path);

        let report = run(&config_path, &missing_discovery(dir.path()));

        assert!(!report.ok, "unexpected lines: {:?}", report.lines);
        assert!(report.lines.iter().any(|l| l.starts_with("journal: error")));
    }

    #[test]
    fn existing_journal_is_checked_in_place_and_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let journal_path = dir.path().join("journal.md");
        std::fs::write(&journal_path, "# Voice Journal\n\n- old\n").unwrap();
        let config_path = write_config(dir.path(), "Ctrl+Alt+KeyJ", &journal_path);

        let report = run(&config_path, &missing_discovery(dir.path()));

        assert!(report.ok, "unexpected lines: {:?}", report.lines);
        assert_eq!(
            std::fs::read_to_string(&journal_path).unwrap(),
            "# Voice Journal\n\n- old\n"
        );
    }

    #[test]
    fn live_api_is_reported_ok() {
        use httpmock::prelude::*;

        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/v1/status");
            then.status(200).body(r#"{"version":1}"#);
        });
        let dir = tempfile::tempdir().unwrap();
        let journal_path = dir.path().join("journal.md");
        let config_path = write_config(dir.path(), "Ctrl+Alt+KeyJ", &journal_path);
        let discovery_path = dir.path().join("api-discovery.json");
        std::fs::write(
            &discovery_path,
            format!(r#"{{"port":{},"token":"secret"}}"#, server.port()),
        )
        .unwrap();

        let report = run(&config_path, &discovery_path);

        assert!(report.ok, "unexpected lines: {:?}", report.lines);
        assert!(
            report.lines.iter().any(|l| l.contains("api: ok")),
            "api line missing: {:?}",
            report.lines
        );
        mock.assert_hits(1);
    }

    #[test]
    fn empty_hotkey_is_not_ok() {
        let dir = tempfile::tempdir().unwrap();
        let journal_path = dir.path().join("journal.md");
        let config_path = write_config(dir.path(), "  ", &journal_path);

        let report = run(&config_path, &missing_discovery(dir.path()));

        assert!(!report.ok, "unexpected lines: {:?}", report.lines);
    }
}

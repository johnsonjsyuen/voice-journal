use chrono::{DateTime, Local};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

const HEADER: &str = "# Voice Journal\n\n";

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("transcript is empty")]
    Empty,
    #[error("journal io error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn format_entry(text: &str, now: DateTime<Local>) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("- {} — {}\n", now.format("%Y-%m-%d %H:%M"), flat)
}

pub fn append_entry(path: &Path, text: &str, now: DateTime<Local>) -> Result<(), JournalError> {
    if text.split_whitespace().next().is_none() {
        return Err(JournalError::Empty);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let exists = path.exists();
    let needs_leading_newline = if exists {
        use std::io::Read;
        let mut f = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        !buf.is_empty() && !buf.ends_with(b"\n")
    } else {
        false
    };
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if !exists {
        file.write_all(HEADER.as_bytes())?;
    } else if needs_leading_newline {
        file.write_all(b"\n")?;
    }
    file.write_all(format_entry(text, now).as_bytes())?;
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts() -> chrono::DateTime<chrono::Local> {
        chrono::Local
            .with_ymd_and_hms(2026, 9, 11, 14, 32, 0)
            .unwrap()
    }

    #[test]
    fn formats_single_line_bullet() {
        assert_eq!(
            format_entry("hello world", ts()),
            "- 2026-09-11 14:32 — hello world\n"
        );
    }

    #[test]
    fn flattens_newlines_and_collapses_whitespace() {
        assert_eq!(
            format_entry("  line one\n\nline   two \t", ts()),
            "- 2026-09-11 14:32 — line one line two\n"
        );
    }

    #[test]
    fn creates_header_for_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.md");
        append_entry(&path, "first", ts()).unwrap();
        let got = std::fs::read_to_string(&path).unwrap();
        assert_eq!(got, "# Voice Journal\n\n- 2026-09-11 14:32 — first\n");
    }

    #[test]
    fn inserts_newline_before_entry_when_file_missing_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.md");
        std::fs::write(&path, "# Voice Journal\n\n- old entry").unwrap();
        append_entry(&path, "new one", ts()).unwrap();
        let got = std::fs::read_to_string(&path).unwrap();
        assert!(got.ends_with("- old entry\n- 2026-09-11 14:32 — new one\n"));
    }

    #[test]
    fn appends_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.md");
        append_entry(&path, "one", ts()).unwrap();
        append_entry(&path, "two", ts()).unwrap();
        let got = std::fs::read_to_string(&path).unwrap();
        assert!(got.ends_with("— one\n- 2026-09-11 14:32 — two\n"));
    }

    #[test]
    fn rejects_empty_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.md");
        assert!(matches!(
            append_entry(&path, "   \n", ts()),
            Err(JournalError::Empty)
        ));
        assert!(!path.exists());
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/deeper/journal.md");
        append_entry(&path, "hi", ts()).unwrap();
        assert!(path.exists());
    }
}

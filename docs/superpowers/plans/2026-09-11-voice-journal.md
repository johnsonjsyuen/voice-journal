# Voice Journal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** A macOS menu-bar Rust binary that toggles TypeWhisper recording on a global hotkey and appends the Granite-transcribed speech as a timestamped bullet to a dedicated Markdown journal file.

**Architecture:** Platform-neutral core (`config`, `discovery`, `api`, `engine`, `journal`, `check`) compiles and tests on Linux; the macOS layer (`src/platform/macos/`) owns the winit event loop, `global-hotkey` registration, `tray-icon` status, and drives the engine on a worker thread over channels.

**Tech Stack:** Rust 2024, `ureq 2` (blocking HTTP), `serde`/`toml`/`chrono`, macOS-only `global-hotkey 0.8`, `tray-icon`, `winit 0.30`.

**Spec:** `docs/superpowers/specs/2026-09-11-voice-journal-design.md`

---

## Anti-patterns (DO NOT)

| ❌ Don't | ✅ Do Instead | Why |
|---|---|---|
| Call TypeWhisper recorder simultaneously from tray, hotkey, and worker | Route every command through the worker's command channel | Recorder sessions are global; concurrent calls produce 409 races |
| Insert transcripts into the focused app | Only append to the journal file | Core product promise; insertion must be structurally impossible |
| Block the winit event loop on HTTP | Worker thread performs HTTP; main thread drains updates | A hung TypeWhisper must never freeze the tray |
| Write journal entries with `fs::write` (truncate) | `OpenOptions::append(true)` | Truncation destroys the journal |
| Parse `HotKey` inside platform-neutral modules | Keep hotkey a `String` in `Config`; parse in `platform/macos` | `global-hotkey` links X11/Cocoa; core must test on Linux |
| Copy transcripts to the clipboard on every success | Clipboard fallback only when the journal write fails | Clipboard churn is a side effect the user did not ask for |
| Append empty/whitespace transcripts | Skip and report "No speech detected" | Empty bullets pollute the journal |
| Hardcode port 8978 | Read `api-discovery.json` | Port and token change on TypeWhisper restart |
| Retry on `409 Already recording` | Surface as transient error, return to Idle | Retrying fights the other recording session |

---

## File structure

| File | Responsibility |
|---|---|
| `Cargo.toml` | Deps; macOS-target-gated GUI crates |
| `src/lib.rs` | Core module exports (no GUI) |
| `src/config.rs` | TOML load/create, defaults, `~` expansion, hotkey string |
| `src/discovery.rs` | Parse `api-discovery.json` (port, token) |
| `src/api.rs` | `Api` trait + `HttpApi` (`ureq`); §6 contract types |
| `src/engine.rs` | Pure state machine over `Api`; returns `Update`s |
| `src/journal.rs` | Bullet formatting + append-only write |
| `src/check.rs` | `--check` headless report (config/journal/discovery) |
| `src/main.rs` | Arg parsing; dispatch to check or macOS daemon |
| `src/platform/macos/mod.rs` | winit event loop, worker thread, channel wiring |
| `src/platform/macos/hotkey.rs` | `global-hotkey` registration + parse |
| `src/platform/macos/tray.rs` | `tray-icon` states, menu, programmatic RGBA icons |
| `tests/recorder_flow.rs` | `httpmock` integration: start→stop→poll→append |
| `scripts/install-launchd.sh` | launchd install/uninstall |
| `.github/workflows/ci.yml` | lint/test (Linux) + build/test/`--check` (macOS) |

---

### Task 1: Scaffold + journal writer (TDD)

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `LICENSE`, `src/lib.rs`, `src/journal.rs`
- Test: in-module `#[cfg(test)]` in `src/journal.rs`

- [x] **Step 1: Scaffold**

```bash
cd /home/johnson/code/voice-journal
cargo init --name voice-journal --vcs none
cargo add ureq@2 --features json
cargo add serde --features derive
cargo add serde_json toml dirs chrono thiserror log env_logger
cargo add --dev tempfile
```

Add to `Cargo.toml` after the dependency block (versions pinned by cargo add):

```toml
[target.'cfg(target_os = "macos")'.dependencies]
global-hotkey = "0.8"
tray-icon = "0.21"
winit = "0.30"

[dev-dependencies]
httpmock = "0.7"
```

`.gitignore`:

```gitignore
/target
.DS_Store
```

`LICENSE`: standard MIT text, copyright `2026 Johnson Yuen`.

- [x] **Step 2: Write failing tests** in `src/journal.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts() -> chrono::DateTime<chrono::Local> {
        chrono::Local.with_ymd_and_hms(2026, 9, 11, 14, 32, 0).unwrap()
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
        assert!(matches!(append_entry(&path, "   \n", ts()), Err(JournalError::Empty)));
        assert!(!path.exists());
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/deeper/journal.md");
        append_entry(&path, "hi", ts()).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn writes_header_into_empty_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.md");
        std::fs::write(&path, "").unwrap();
        append_entry(&path, "first", ts()).unwrap();
        let got = std::fs::read_to_string(&path).unwrap();
        assert_eq!(got, "# Voice Journal\n\n- 2026-09-11 14:32 — first\n");
    }

    #[test]
    fn unwritable_path_returns_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("adir");
        std::fs::create_dir(&path).unwrap();
        assert!(matches!(append_entry(&path, "hi", ts()), Err(JournalError::Io(_))));
    }
}
```

- [x] **Step 3: Run tests, verify failure**

Run: `cargo test journal -- --nocapture`
Expected: compile error, `format_entry` not found.

- [x] **Step 4: Implement `src/journal.rs`**

```rust
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
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    let len = file.metadata()?.len();
    if len == 0 {
        file.write_all(HEADER.as_bytes())?;
    } else {
        use std::io::{Read, Seek, SeekFrom};
        file.seek(SeekFrom::End(-1))?;
        let mut last = [0u8; 1];
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            file.write_all(b"\n")?;
        }
    }
    file.write_all(format_entry(text, now).as_bytes())?;
    file.flush()?;
    Ok(())
}
```

- [x] **Step 5: `src/lib.rs`**

```rust
pub mod api;
pub mod check;
pub mod config;
pub mod discovery;
pub mod engine;
pub mod journal;
```

Create empty stubs (`api.rs`, `check.rs`, `config.rs`, `discovery.rs`, `engine.rs`) with a doc comment so the crate compiles.

- [x] **Step 6: Run tests to verify pass**

Run: `cargo test journal`
Expected: 7 passed.

- [x] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore LICENSE src
git commit -m "feat: scaffold crate and append-only journal writer"
```

---

### Task 2: Config (TDD)

**Files:** Create/replace `src/config.rs`; Test in-module.

- [x] **Step 1: Failing tests**

```rust
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
        let c = parse(r#"hotkey = "Cmd+Shift+Space"
journal_path = "/tmp/j.md""#).unwrap();
        assert_eq!(c.hotkey, "Cmd+Shift+Space");
        assert_eq!(c.journal_path, std::path::PathBuf::from("/tmp/j.md"));
    }

    #[test]
    fn rejects_empty_hotkey() {
        assert!(matches!(parse(r#"hotkey = "  ""#).unwrap_err(), ConfigError::EmptyHotkey));
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
        assert!(std::fs::read_to_string(&path).unwrap().contains("Ctrl+Alt+KeyJ"));
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
```

- [x] **Step 2: Run failing**: `cargo test config` → compile error.
- [x] **Step 3: Implement**

```rust
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

fn default_hotkey() -> String { "Ctrl+Alt+KeyJ".into() }
fn default_journal_path() -> PathBuf { expand_tilde(DEFAULT_JOURNAL_PATH) }

impl Default for Config {
    fn default() -> Self { Config { hotkey: default_hotkey(), journal_path: default_journal_path() } }
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
    let dir = dirs::config_dir().ok_or(ConfigError::NoHome)?.join("voice-journal");
    Ok(dir.join("config.toml"))
}

pub fn config_path() -> Result<PathBuf, ConfigError> {
    config_path_from(std::env::var("VOICE_JOURNAL_CONFIG").ok().as_deref())
}

pub fn expand_tilde(input: &str) -> PathBuf {
    if input == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
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
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
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
```

- [x] **Step 4: `cargo test config`** → 6 passed.
- [x] **Step 5: Commit**: `git commit -am "feat: config load/create with defaults"`

---

### Task 3: Discovery (TDD)

**Files:** `src/discovery.rs`.

- [x] **Step 1: Failing tests** — valid file parses port+token; missing file → `DiscoveryError::Missing`; corrupt JSON → `DiscoveryError::Parse`; omitted token → `None`.

```rust
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
        assert!(matches!(load(std::path::Path::new("/nonexistent/x.json")), Err(DiscoveryError::Missing)));
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
```

- [x] **Step 2: Run failing**: `cargo test discovery`.
- [x] **Step 3: Implement**

```rust
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
    #[error("TypeWhisper discovery file not found at {0} — is TypeWhisper running with the API server enabled?")]
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
    if !path.exists() { return Err(DiscoveryError::Missing(path.to_path_buf())); }
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}
```

- [x] **Step 4: `cargo test discovery`** → 4 passed.
- [x] **Step 5: Commit**: `git commit -am "feat: TypeWhisper discovery file parsing"`

---

### Task 4: API client (TDD with httpmock)

**Files:** `src/api.rs`; integration tests in `tests/recorder_flow.rs` (Task 5 adds flow).

- [x] **Step 1: Implement types + client contract** (agents implement tests first; below is the required interface)

```rust
pub trait Api: Send + Sync {
    fn start(&mut self) -> Result<RecorderSession, ApiError>;
    fn stop(&mut self) -> Result<RecorderSession, ApiError>;
    fn session(&mut self, id: &str) -> Result<RecorderSession, ApiError>;
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RecorderSession {
    pub id: String,
    pub status: SessionStatus,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub output_file: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus { Recording, Finalizing, Completed, Failed }

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("TypeWhisper API unavailable: {0}")]
    Unavailable(String),
    #[error("TypeWhisper API token rejected (401)")]
    Unauthorized,
    #[error("TypeWhisper HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("invalid TypeWhisper response: {0}")]
    InvalidResponse(String),
}

pub struct HttpApi { agent: ureq::Agent, base: String, token: Option<String>, discovery_path: Option<PathBuf> }
impl HttpApi {
    pub fn new(port: u16, token: Option<String>) -> Self;                 // discovery_path: None
    pub fn from_discovery(d: &Discovery, path: PathBuf) -> Self;
}
```

Behavior:
- `start`: `POST {base}/v1/recorder/start?mic=true&system_audio=false`, no body; parse `RecorderSession`.
- `stop`: `POST {base}/v1/recorder/stop`.
- `session`: `GET {base}/v1/recorder/session?id={id}`.
- Always send `Authorization: Bearer <token>` when token is `Some`.
- Timeouts: connect 1 s, overall 5 s (`ureq::AgentBuilder`).
- Map `ureq::Error::Status(code, resp)` to `ApiError::Http` with the body's `error.message` if parseable, else the raw status text; transport errors to `Unavailable`; status 401 to `Unauthorized`.
- **401 recovery (spec §10):** every request goes through a private `call` helper. On `ApiError::Unauthorized` with a `discovery_path` present, re-read the discovery file, replace `base`/`token`, and retry the request exactly once; a second 401 returns `Unauthorized`.

- [x] **Step 2: Required tests** (in `src/api.rs` `#[cfg(test)]` using `httpmock`)

| Test | Mock | Assert |
|---|---|---|
| `start_parses_session` | `POST /v1/recorder/start` query contains `mic=true`, returns `{"id":"a","status":"recording"}` | `id == "a"`, status Recording |
| `stop_parses_finalizing` | `POST /v1/recorder/stop` → `{"id":"a","status":"finalizing"}` | status Finalizing |
| `session_parses_completed_text` | `GET /v1/recorder/session` query `id=a` → `{"id":"a","status":"completed","text":"hi","output_file":"/tmp/r.wav"}` | text `Some("hi")`, output_file `/tmp/r.wav` |
| `session_parses_failed` | → `{"id":"a","status":"failed","error":"finalTranscription: boom"}` | status Failed, error text preserved |
| `auth_header_sent_when_token_present` | any route, assert header `Authorization == Bearer tok` | header matches |
| `status_409_maps_to_http_error` | `{"error":{"code":"bad_request","message":"Already recording"}}`, status 409 | `ApiError::Http { status: 409, message }` contains "Already recording" |
| `status_401_maps_to_unauthorized` | status 401, any body, no discovery file | `ApiError::Unauthorized` |
| `unauthorized_reloads_discovery_and_retries_once` | temp discovery file; first request 401, second 200 with corrected token | success on retry; second request carries the new token |
| `persistent_unauthorized_stops_after_one_retry` | temp discovery file; both requests 401 | `ApiError::Unauthorized`; exactly 2 requests made, second carrying `Bearer new` |
| `invalid_json_response_is_invalid_response` | 200 with non-JSON body | `ApiError::InvalidResponse` |
| `connection_refused_is_unavailable` | port 1 (no listener) | `ApiError::Unavailable` |

- [x] **Step 3: Run** `cargo test api` → pass.
- [x] **Step 4: Commit**: `git commit -am "feat: TypeWhisper recorder HTTP client"`

---

### Task 5: Engine state machine + integration flow (TDD)

**Files:** `src/engine.rs`; `tests/recorder_flow.rs`.

- [x] **Step 1: Required interface**

```rust
pub enum State {
    Idle,
    Recording { id: String, started: Instant },
    Finalizing { id: String, deadline: Instant, output_file: Option<String> },
    Error { message: String },
}

pub enum Update {
    None,
    Transcribed { text: Option<String>, output_file: Option<String> },
    Error(String),
}

pub struct Engine<A: Api> { api: A, state: State, last_error: Option<String> }

impl<A: Api> Engine<A> {
    pub fn new(api: A) -> Self;
    pub fn state(&self) -> &State;
    pub fn toggle(&mut self, now: Instant) -> Update;
    pub fn tick(&mut self, now: Instant) -> Update;
    pub fn started_at(&self) -> Option<Instant>;
}
```

Behavior:
- `toggle` from `Idle | Error`: `api.start()`; Recording response → `Recording{id, started: now}`; error → `Error`.
- `toggle` from `Recording`: `api.stop()`; success → `Finalizing{id: stopped id, deadline: now + max(120s, 3×elapsed), output_file: None}`; error → `Error`.
- `toggle` while `Finalizing` → `Update::Error("transcription still in progress")`, state stays `Finalizing`, no API call (spec §7 transient feedback).
- `tick` when not `Finalizing` → `None`; when `Finalizing`: past deadline → `log::warn!` last-known `output_file` (if any) then `Error("timed out...")`; session `Completed` → `Idle`, `Transcribed{text, output_file}`; `Failed` → `log::warn!` the session `output_file` (spec §10: a recording is never lost silently) then `Error(error or "transcription failed")`; `Recording | Finalizing` status → remember `output_file` in state, return `None`; API error → `Error`.
- `Error` state is sticky only until next `toggle`, which retries from scratch.

- [x] **Step 2: Required unit tests** with a `MockApi` (`RefCell<VecDeque<Result<RecorderSession, ApiError>>>`):

| Test | Behavior verified |
|---|---|
| `happy_path_toggle_stop_poll_complete` | Idle→toggle→Recording→toggle→Finalizing; tick returns None while session says finalizing; final tick returns Transcribed |
| `start_error_enters_error_and_next_toggle_retries` | start Err → Error; next toggle calls start again |
| `failed_session_surfaces_provider_error` | tick → `Update::Error("finalTranscription: boom")` |
| `timeout_produces_error` | tick at `deadline + 1s` → Error containing "timed out" |
| `toggle_during_finalizing_is_ignored` | returns `Update::Error` containing "still in progress", state remains `Finalizing`, no extra API calls |
| `empty_text_is_transcribed_with_none_text` | Transcribed `text: None` (caller skips journal) |
| `deadline_is_at_least_120s` | stop at now; tick just before 120s → None (still polling) |
| `deadline_scales_with_long_recording` | start at t0, stop at t0+100s; deadline = stop + 3×100s = t0+400s; tick at t0+399s → None, tick at t0+401s → Error (proves 3×recorded-duration binds) |
| `stop_error_enters_error` | `api.stop()` Err → `Update::Error` and state Error |
| `session_api_error_enters_error` | `api.session()` Err during Finalizing → `Update::Error` |
| `non_recording_start_response_enters_error` | start returns `Completed` → Error "unexpected start status" |

- [x] **Step 3: Integration test `tests/recorder_flow.rs`**

Use `httpmock` to mock all three endpoints (start → finalizing → completed with text) and a `tempfile` journal path. Drive `Engine<HttpApi>` by hand with synthetic `Instant`s (no sleeps), then `journal::append_entry` on the `Transcribed` update, and assert the journal contains exactly one bullet with the transcript.

- [x] **Step 4: Run** `cargo test engine recorder_flow` → pass.
- [x] **Step 5: Commit**: `git commit -am "feat: recorder engine state machine and flow test"`

---

### Task 6: `--check` report and CLI

**Files:** `src/check.rs`, `src/main.rs`.

- [x] **Step 1: Implement `check.rs`**

```rust
pub struct Report { pub lines: Vec<String>, pub ok: bool }
pub fn run(config_path: &Path, discovery_path: &Path) -> Report;
pub fn run_cli() -> i32; // resolves paths via config::config_path() and discovery::discovery_path(); prints lines; 0/1
```

Checks, in order:
1. Config loads (or is created) — fail → `ok = false`.
2. Journal parent directory exists or is creatable and writable — probe by `create_dir_all(parent)` then creating and deleting a `*.probe` file in it; fail → `ok = false`. Do not create the journal file itself.
3. Journal path reported.
4. Hotkey string non-empty — fail → `ok = false`.
5. Discovery file: present → report port + token present/absent; missing → warning line, **does not** flip `ok`.
On macOS additionally attempt `global_hotkey::hotkey::HotKey::from_str(&cfg.hotkey)` and fail `ok` if unparseable; error message must include the accepted syntax example `Ctrl+Alt+KeyJ`.

- [x] **Step 2: Implement `main.rs`**

```rust
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    match std::env::args().nth(1).as_deref() {
        Some("--check") => std::process::exit(check::run_cli()),
        Some("--version") | Some("-V") => { println!("voice-journal {}", env!("CARGO_PKG_VERSION")); }
        Some("--help") | Some("-h") => print!("{USAGE}"),
        Some(other) => { eprintln!("unknown argument: {other}\n{USAGE}"); std::process::exit(2); }
        None => run_daemon(),
    }
}

#[cfg(target_os = "macos")]
fn run_daemon() { if let Err(e) = voice_journal::platform::macos::run() { eprintln!("fatal: {e}"); std::process::exit(1); } }

#[cfg(not(target_os = "macos"))]
fn run_daemon() { eprintln!("voice-journal daemon runs on macOS only (use --check here)"); std::process::exit(1); }
```

`check::run_cli` prints each line and returns 0/1. `lib.rs` must gate the platform module:

```rust
#[cfg(target_os = "macos")]
pub mod platform;
```

- [x] **Step 3: Tests**: `--check` with `VOICE_JOURNAL_CONFIG` pointing at a temp config and a missing discovery path returns ok=true (discovery absent is a warning); an invalid TOML returns ok=false.
- [x] **Step 4: Manual**: `cargo run -- --check; echo $?` on Linux prints report and `0`.
- [x] **Step 5: Commit**: `git commit -am "feat: headless --check report and CLI dispatch"`

---

### Task 7: macOS platform layer (compile on CI, manual acceptance on Mac)

**Files:** `src/platform/mod.rs`, `src/platform/macos/{mod.rs,hotkey.rs,tray.rs}`.

**Spike requirement:** this task is the design's spike #1/#2. First commit a minimal `hotkey.rs` + event loop that only logs events, run CI; then add tray and worker wiring.

- [x] **Step 0: dependency + single-instance lock**

Run: `cargo add --target 'cfg(target_os = "macos")' fs4` (flock; version pinned by cargo add).

At daemon startup, before registering the hotkey or building the tray:

```rust
let dir = config::config_path()?.parent().unwrap().to_path_buf();
std::fs::create_dir_all(&dir)?;
let lock_file = std::fs::OpenOptions::new().create(true).read(true).write(true).open(dir.join("adapter.lock"))?;
fs4::fs_std::FileExt::try_lock_exclusive(&lock_file).map_err(|_| "voice-journal is already running")?;
// keep `lock_file` alive for the process lifetime
```

- [x] **Step 1: `hotkey.rs`**

```rust
pub struct HotkeyHandle { _manager: GlobalHotKeyManager, pub id: HotKeyId }
pub fn register(spec: &str) -> Result<HotkeyHandle, String>;
pub fn parse(spec: &str) -> Result<HotKey, String>; // wraps FromStr with a helpful error listing "Ctrl+Alt+KeyJ"
```

- [x] **Step 2: `tray.rs`**

- `TrayIconBuilder::new()`, menu via `tray_icon::menu::{Menu, MenuItem}` with disabled status item, `Open Journal…`, `Open Config…`, `Quit`.
- Icons built in code with `tray_icon::Icon::from_rgba` (16×16 and 32×32):
  - idle: black filled circle with alpha (set as template? macOS template icons are monochrome; idle uses template `true`),
  - recording: red filled circle (template `false`),
  - error: orange filled square (template `false`).
- `set_icon`, `set_tooltip`, status menu item text updated from `Engine::state()`.
- Menu event handling via `MenuEvent::receiver()` in `about_to_wait`.
- `Open Journal…` / `Open Config…` shell out to `/usr/bin/open`.

- [x] **Step 3: `mod.rs` — wiring**

```
main thread (winit ApplicationHandler):
  acquire single-instance flock (Step 0)
  create GlobalHotKeyManager + register configured hotkey
  build tray
  spawn worker thread owning Engine<HttpApi> and an mpsc::Receiver<Command>
  about_to_wait():
    drain worker updates FIRST (so a queued error is rendered before a hotkey press clears it)
    drain HotKeyEvent::receiver() -> on Pressed clear notice, send Command::Toggle
    drain tray/menu receivers -> menu actions, Command::Toggle, Command::Quit
    drain mpsc::Receiver<Update>:
      Update::Transcribed { text: Some(t) } if !t.trim().is_empty() ->
        match journal::append_entry(journal_path, &t, Local::now()):
          Ok(())  -> tray idle
          Err(e)  -> transcript copied to clipboard via `pbcopy` (stdin) so speech is not lost,
                     tray error "journal write failed; transcript copied to clipboard: {e}"
      Update::Transcribed { text: None|empty } -> tray tooltip "No speech detected"
      Update::Error(m) -> tray error state
    update tray from engine state snapshot sent by worker
    event_loop.set_control_flow(ControlFlow::WaitUntil(now + 100ms))
worker thread:
  loop { recv_timeout(250ms):
    Command::Toggle ->
      if engine state is Recording|Finalizing -> engine.toggle(Instant::now())   // never swallow a stop; stale API will fail into Error
      else -> discovery::load; on Err send Update::Error (state snapshot None) with spec §10 wording for Missing
              ("TypeWhisper API unavailable — enable API Server in Settings → Advanced"); on Ok rebuild
              Engine::new(HttpApi::from_discovery(&d, path)) and engine.toggle(Instant::now())
    Command::Quit -> if engine state is Recording, best-effort engine.toggle(Instant::now()) to POST stop
                    (ignore result, so TypeWhisper finalizes instead of leaving the mic hot); then break
                    (main thread exits the event loop)
    timeout -> engine.tick(Instant::now()) if an engine exists
    send Update + state snapshot to main
  }
```

Main-thread error notice rules: `Update::Error(m)` sets a sticky notice and renders the error tray state even when the worker sent no state snapshot; periodic state snapshots never override a sticky notice; the notice clears when the next hotkey press begins a new attempt. Quit is executed on the main thread via `event_loop.exit()`; the worker never calls `process::exit`.

Discovery is re-read on every `Toggle` so port/token rotations are picked up. `HttpApi` additionally re-reads discovery and retries once on a 401 (Task 4). On discovery/API error the worker sends `Update::Error` while remaining in `Error` state (next toggle retries).

- [x] **Step 4: Platform gating test**: `cargo build` on Linux must not compile `platform/macos`; `cargo build` on macOS CI must succeed.
- [x] **Step 5: Commit**: `git commit -am "feat: macOS hotkey, tray, and worker wiring"`

---

### Task 8: launchd install script

**Files:** `scripts/install-launchd.sh` (exact content):

```bash
#!/usr/bin/env bash
set -euo pipefail

LABEL="com.johnson.voice-journal"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
BIN="${1:-$(pwd)/target/release/voice-journal}"

if [[ "${1:-}" == "--uninstall" ]]; then
  launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
  rm -f "$PLIST"
  echo "Uninstalled $LABEL"
  exit 0
fi

if [[ ! -x "$BIN" ]]; then
  echo "Binary not found or not executable: $BIN" >&2
  echo "Usage: $0 [path-to-voice-journal]  |  $0 --uninstall" >&2
  exit 1
fi

mkdir -p "$HOME/Library/LaunchAgents" "$HOME/Library/Logs"
cat > "$PLIST" <<PLIST_EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key>
  <array><string>$BIN</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>StandardOutPath</key><string>$HOME/Library/Logs/voice-journal.log</string>
  <key>StandardErrorPath</key><string>$HOME/Library/Logs/voice-journal.log</string>
</dict>
</plist>
PLIST_EOF

launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$PLIST"
echo "Installed and started $LABEL ($BIN)"
```

- [x] **Step 1:** Write file, `chmod +x`, `bash -n scripts/install-launchd.sh` (syntax check on any OS), commit.

---

### Task 9: GitHub Actions CI

**Files:** `.github/workflows/ci.yml` (exact content):

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - run: cargo fmt --all --check
      - run: cargo clippy --all-targets -- -D warnings

  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test

  macos:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --release
      - run: cargo test
      - run: ./target/release/voice-journal --check
```

- [x] **Step 1:** Write file, commit.
- [x] **Step 2:** Push and verify all three jobs green via `gh run watch`.

---

### Task 10: Repo creation, push, CI verification

- [x] **Step 1:** Ensure spec + plan are committed and `docs/superpowers/` is tracked.
- [x] **Step 2:** Create the public repo and push:

```bash
gh repo create voice-journal --public --source=. --remote=origin --description "macOS push-to-talk voice journaling: global hotkey → TypeWhisper Granite STT → append-only markdown" --push
```

- [x] **Step 3:** `gh run list` then `gh run watch <id> --exit-status` for the pushed commit.
- [x] **Step 4:** Fix CI failures (likely candidates: `tray-icon`/`winit` API versions, clippy lints) and push until `lint`, `test`, and `macos` are green. Do not mark complete until all three pass.
- [x] **Step 5:** Report the repo URL and CI run URL. GUI behaviors (tray icon, hotkey, real TypeWhisper round-trip) are listed in the spec §14 manual checklist for the user's Mac.

---

## Self-review notes

- Spec coverage: architecture §5 → Tasks 1–7; API contract §6 → Tasks 4–5; behavior §7 → Task 5; config §8 → Task 2; journal §9 → Task 1; failure handling §10 → Tasks 5–7; CLI/tray/launchd §11 → Tasks 6–8; structure/deps §12–13 → Task 1; testing/CI §14 → Tasks 5, 9, 10; risks §15 → Task 7 spikes.
- Deviations from spec found during planning: tray icons are generated in code (`Icon::from_rgba`) instead of `assets/icons/*.png`. The spec must be updated to match (stream-coding: no divergence).
- Type consistency: `Api` trait methods `start/stop/session` used consistently; `RecorderSession` fields match §6 JSON; `Engine::toggle/tick` return `Update` everywhere.

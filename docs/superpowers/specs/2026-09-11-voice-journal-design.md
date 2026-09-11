# Voice Journal — macOS push-to-talk journaling adapter

- **Date:** 2026-09-11
- **Status:** Approved pending written-spec review
- **Repo:** https://github.com/johnsonjsyuen/voice-journal (public)
- **Target:** macOS 14+, Apple Silicon (Granite Speech constraint)

## 1. Summary

A small Rust menu-bar binary ("the adapter") that turns anywhere on macOS into
a voice-notes surface: press a global hotkey, speak, press again, and the raw
transcript is appended as a timestamped bullet to one Markdown journal file.
The adapter owns the hotkey and the destination file. TypeWhisper (the macOS
app) owns microphone capture and on-device transcription via its bundled
IBM Granite Speech (MLX) engine. The transcript is never inserted into the
focused app.

This spec covers v1 only. `repo-hotwords` biasing is explicitly deferred to v2
(see §16).

## 2. Goals

1. Global toggle hotkey works in any app, with no Accessibility or Input
   Monitoring permission prompts.
2. Speech is transcribed fully on-device by TypeWhisper + Granite Speech.
3. Every completed dictation is appended to one dedicated Markdown file as
   `- <YYYY-MM-DD HH:MM> — <text>`.
4. The transcript is never typed/pasted into any focused app.
5. Menu-bar icon communicates idle / recording / error state.
6. Build and core logic are validated in GitHub Actions on a macOS runner.

## 3. Non-goals (v1)

- No STT implementation in Rust; no audio capture in the adapter.
- No repo-hotwords/Granite dictionary biasing (v2).
- No LLM post-processing, no cloud STT, no Obsidian/Notes integration.
- No sounds or notifications; no windows/dock icon.
- No Windows/Linux runtime support (core logic still compiles/tests on Linux).

## 4. Decision record

| Option | Verdict | Why |
|---|---|---|
| Use TypeWhisper config-only (Obsidian action target, MCP filesystem) | Rejected | Action-target workflows force an LLM step; stock plugins cannot write a raw `~/journal.md`; insertion suppression depends on fragile config. |
| Fully custom Rust app with Granite (ONNX/MLX) | Rejected | Reimplements mic capture, VAD, hotkey, model runtime. TypeWhisper already ships Granite Speech 4.0 1B, dictionary biasing, and a recorder API documented as paste-free. |
| Thin Rust adapter driving TypeWhisper's recorder HTTP API | **Chosen** | Smallest deliverable that meets every goal; keeps the door open to a v2 Granite/hotwords path. |
| TypeWhisper native hotkey + Webhook plugin into an adapter | Rejected | Normal dictation path inserts into the focused app; recorder API path avoids insertion structurally. |

## 5. Architecture

```
┌──────────────────── voice-journal (Rust, menu bar, no dock icon) ─────────────────────┐
│  global-hotkey (toggle, no Accessibility permission)                                  │
│  state machine ──ureq──► 127.0.0.1:8978 (TypeWhisper local API)                       │
│  tray-icon: idle / recording / error        journal writer (O_APPEND)                 │
└───────────────┬────────────────────────────────────────────────┬──────────────────────┘
                │ port + token from api-discovery.json           │ append bullet
                ▼                                                ▼
     TypeWhisper.app (LSUIElement, hidden)              ~/Documents/VoiceJournal.md
     mic capture + Granite Speech (MLX) on device
```

The adapter reads the TypeWhisper discovery file before each session so app
restarts with a new port or token are picked up automatically.

Hard dependency: TypeWhisper 1.6.0+ installed, running, with **Settings →
Advanced → API Server enabled** (off by default) and the Granite model
downloaded and selected for the Recorder. The adapter surfaces a clear error
state if any of these are missing.

## 6. TypeWhisper API contract (verified)

Verified against `TypeWhisper/typewhisper-mac` at v1.6.0 and main
(`a9e84fdf`, 2026-09-11). The adapter pins this contract to 1.6.x and MUST fail
softly if responses deviate.

**Discovery.** `~/Library/Application Support/TypeWhisper/api-discovery.json`,
mode 0600, deleted when the server stops:

```json
{ "port": 8978, "token": "<base64url>", "version": 1 }
```

Plain HTTP on `127.0.0.1` only. Auth is off by default; when on, send
`Authorization: Bearer <token>` (or `X-TypeWhisper-API-Token`). `GET /v1/status`
is always public.

**Start recording.** `POST /v1/recorder/start?mic=true&system_audio=false`
(JSON body is ignored):

```json
200 {"id":"<uuid>","status":"recording"}
409 {"error":{...}}   // "Already recording" | "Recorder is finalizing"
400                   // no audio source enabled
```

**Stop recording.** `POST /v1/recorder/stop` — does not block:

```json
200 {"id":"<uuid>","status":"finalizing"}
409 // "Not recording" (API-started sessions only)
```

**Poll session.** `GET /v1/recorder/session?id=<uuid>`; absent optionals are
omitted, not null. Status flow: `recording → finalizing → completed | failed`.

```json
{"id":"<uuid>","status":"completed","text":"...","output_file":"/Users/.../Recording.wav"}
{"id":"<uuid>","status":"failed","output_file":"...","error":"finalTranscription: <provider error>"}
```

**Engine constraints.** There is no per-session engine/model/language/prompt
control. The Recorder uses the engine/model configured in TypeWhisper, and the
app-level dictionary terms are applied as the Granite prompt automatically.
Granite plugin: providerId `granite`, default model
`granite-1b-speech-4bit` (~2 GB), Apple Silicon only, weights downloaded via
plugin settings. Recordings land in `~/Documents/TypeWhisper Recordings/` as
`.wav` (default) or `.m4a`.

## 7. Adapter behavior

State machine:

```
Idle ──hotkey──► Starting ──200──► Recording(id) ──hotkey──► Finalizing(id)
  ▲                  │                   │                        │
  │                  │ 4xx/5xx            │ app quits/network      │ poll:
  │                  ▼                   ▼                        ▼
  └───────────── Error(shown in tray; auto-clears to Idle on next toggle)
                                                                  │
                       Completed ──► append bullet ──► Idle       │
                       Failed/empty ──► Error ──► Idle            │
                       timeout ──► Error ──► Idle ◄───────────────┘
```

- Toggle always starts from Idle; while Recording it stops; while
  Finalizing/Starting it is ignored (and shown as transient error).
- Poll interval 250 ms. Wait cap `max(120 s, 3 × recorded_duration)`, where
  duration is measured by the adapter between start and stop.
- On `completed` with non-empty `text`: append to the journal. Empty text
  (silence) is reported as "No speech detected" and nothing is appended.
- `output_file` is logged on failure/timeout so a recording is never lost.

## 8. Configuration

`~/Library/Application Support/voice-journal/config.toml`, created with
defaults and comments on first run:

```toml
# Global toggle hotkey (global-hotkey key syntax).
hotkey = "Ctrl+Alt+KeyJ"

# Journal destination. ~ is expanded.
journal_path = "~/Documents/VoiceJournal.md"
```

- `VOICE_JOURNAL_CONFIG` env var overrides the config path (used by tests/CI).
- A single-instance lock (`flock` on `.../voice-journal/adapter.lock`) prevents
  duplicate hotkeys when launchd and a manual run overlap.
- Invalid config or bad hotkey string aborts startup with a clear stderr
  message and exit code 1; the tray is not created.

## 9. Journal format

On first append, the file is created with a `# Voice Journal` header. Each
entry:

```
- 2026-09-11 14:32 — refactored the GranitePlugin prompt handling
```

- Local time, `YYYY-MM-DD HH:MM`.
- Transcript whitespace is collapsed and embedded newlines become spaces, so
  one entry is always exactly one bullet.
- Appends use `OpenOptions::append`; if the existing file does not end with a
  newline, one is inserted first.
- Parent directories are created if missing.

## 10. Failure handling

| Case | Behavior |
|---|---|
| Discovery file missing (app not running / API off) | Tray error: "TypeWhisper API unavailable — enable API Server in Settings → Advanced". Auto-retry on next toggle. |
| `409 Already recording` / `Not recording` / `finalizing` | Transient tray error, state returns to Idle. |
| `401` | Re-read discovery once (token rotation), retry the request once, then error. |
| TypeWhisper quits mid-recording | Poll fails; error state includes last known `output_file`. |
| Transcription `failed` | Error state with provider error text; `output_file` logged. |
| Journal write fails | Error state; transcript is copied to the clipboard as a fallback so speech is not lost. |
| Poll timeout | Error state; `output_file` logged. |

All errors are logged to stderr, which launchd captures in
`~/Library/Logs/voice-journal.log`.

## 11. CLI and process lifecycle

| Command | Behavior |
|---|---|
| `voice-journal` | Runs the menu-bar daemon (macOS only). |
| `voice-journal --check` | Validates config, journal writability, discovery parse; prints a report; exits 0/1. **Does not create tray/hotkey** — used by CI. Exit 0 even if TypeWhisper is absent (reported as a warning). |
| `voice-journal --version` | Prints version and exits. |
| `voice-journal --help` | Usage. |

Tray menu: disabled status row (state/last error), `Open Journal…`,
`Open Config…`, `Quit`. Icons are generated in code as RGBA via
`tray_icon::Icon::from_rgba` (no binary assets): idle (template monochrome
circle, adapts to light/dark menu bar), recording (red circle), error (orange
square).

A `scripts/install-launchd.sh` installs and loads
`~/Library/LaunchAgents/com.johnson.voice-journal.plist` (`RunAtLoad`,
`KeepAlive`, stdout/stderr to `~/Library/Logs/voice-journal.log`); a
`--uninstall` mode removes it.

## 12. Project structure

```
voice-journal/
├── Cargo.toml
├── .gitignore
├── src/
│   ├── main.rs               # arg parsing; macOS: start app; --check works everywhere
│   ├── lib.rs                # core module exports
│   ├── config.rs             # TOML load/create, defaults, ~ expansion
│   ├── discovery.rs          # api-discovery.json parse
│   ├── api.rs                # TypeWhisper HTTP client (ureq)
│   ├── engine.rs             # platform-neutral state machine over an Api trait
│   ├── journal.rs            # bullet formatting + atomic append
│   ├── check.rs              # --check report
│   └── platform/
│       └── macos/
│           ├── mod.rs        # event loop wiring
│           ├── hotkey.rs     # global-hotkey registration
│           └── tray.rs       # tray-icon + menu + programmatic icon states
├── scripts/install-launchd.sh
├── docs/superpowers/specs/2026-09-11-voice-journal-design.md
├── docs/superpowers/plans/…
└── .github/workflows/ci.yml
```

Everything except `platform/macos/` is platform-neutral and compiles/tests on
Linux. The macOS crate dependencies are optional and gated under
`[target.'cfg(target_os = "macos")'.dependencies]`: `global-hotkey = "0.8"`,
`tray-icon`, and the event-loop choice from spike #2 (`winit`, with `tao` or
`objc2` as fallback). Exact `tray-icon`/`winit` versions are pinned at
implementation time once the spike confirms the pairing; they must be
compatible with `global-hotkey 0.8`.

## 13. Dependencies

Core: `ureq` (blocking HTTP; IPv4 literal `127.0.0.1` — listener is IPv4-only),
`serde`, `serde_json`, `toml`, `dirs`, `chrono`, `thiserror`, `log`,
`env_logger`. macOS: `global-hotkey 0.8`, `tray-icon`, `winit` (spike), image
decoding as needed. Dev: `httpmock`, `tempfile`.

## 14. Testing and CI

**Unit tests (run on Linux and macOS):** journal (header creation, bullet
format, newline flattening, missing trailing newline, unwritable path), config
(defaults, round-trip, bad hotkey, `~` expansion), discovery (valid, missing,
corrupt), engine transitions with a mocked `Api` trait (happy path, start 409,
failed session, timeout math, empty text, stop 409).

**Integration tests (mock HTTP server):** exact recorder contract from §6 —
start response, non-blocking stop, session poll to `completed`, append result.

**GitHub Actions — `.github/workflows/ci.yml`:**

| Job | Runner | Steps |
|---|---|---|
| `lint` | ubuntu-latest | `cargo fmt --check`, `cargo clippy -- -D warnings` |
| `test` | ubuntu-latest | `cargo test` (platform-neutral core) |
| `macos` | macos-latest | `cargo build --release`, `cargo test`, `./target/release/voice-journal --check` smoke test |

The macOS job proves the binary compiles, links, and runs its headless check
path on a real Apple environment. Tray/hotkey behavior cannot be exercised
headlessly; those are covered by the manual checklist:
hotkey with zero permission prompts; tray state changes; real TypeWhisper
round-trip; TypeWhisper stopped; API disabled; quit mid-recording; port/token
change after TypeWhisper restart.

## 15. Risks and first spikes

1. **`global-hotkey` in a windowless event loop** — must be spiked first on a
   Mac. Carbon hotkeys need a main-thread event loop; verify a no-window
   binary receives pressed/released events. (CI compiles this, but cannot
   prove runtime behavior.)
2. **`tray-icon` + event loop pairing** — winit is the documented pairing; tao
   is the tauri alternative used by `global-hotkey`. Spike both; fall back to
   raw `objc2` `NSStatusItem` if neither is clean. Tray icons are updated only
   on the main thread; the engine thread communicates state via channel.
   Also set `NSApplication.setActivationPolicy(.accessory)` so the daemon shows
   no Dock icon (verify in the same spike).
3. **Modifier-only hotkeys are impossible** with Carbon (`global-hotkey`);
   config must use a real key. Default `Ctrl+Alt+KeyJ`; exact `FromStr` syntax
   verified during spike.
4. **TypeWhisper version drift** — contract pinned to 1.6.x; main (1.7) adds
   `/v1/models/load` and settings endpoints. Adapter tolerates additions and
   fails softly on removals.
5. **Empty/partial transcript edge cases** (silence, sub-0.5 s clips) — empty
   text skips the append; never write an empty bullet.

## 16. v2 candidates (explicitly out of scope now)

- `repo-hotwords` integration: run the Rust indexer for a configured repo and
  either sync terms via `PUT /v1/dictionary/terms` or use the
  capture-only + `/v1/transcribe/local-file` path for true per-session
  `Keywords:` prompting (requires verifying prompt support on that endpoint).
- Selection-to-journal (append currently selected text), Obsidian support,
  LLM cleanup, sounds/notifications, Windows/Linux.

## 17. Open questions

1. Default hotkey `Ctrl+Alt+KeyJ` acceptable, or prefer another combo
   (e.g. `Cmd+Shift+Space`)?
2. Journal default `~/Documents/VoiceJournal.md` acceptable, or a different
   path/name?
3. Tray menu wording above fine as-is?

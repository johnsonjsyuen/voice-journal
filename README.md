# Voice Journal

Capture notes by voice, anywhere on your Mac. Press a global hotkey, speak, press
it again — the on-device transcript is appended to a single Markdown journal
file. Nothing is inserted into the app you're using, and nothing leaves your
machine.

```
- 2026-09-11 14:32 — refactored the prompt handling, next up is the settings screen
```

Uses [TypeWhisper](https://github.com/TypeWhisper/typewhisper-mac) for local
microphone capture and Granite Speech transcription through its local API.

## Requirements

- macOS 14 or later, Apple Silicon
- [TypeWhisper](https://www.typewhisper.com) 1.6 or later, running, with:
  - **Settings → Advanced → API Server** enabled
  - a Granite Speech model downloaded and selected for the Recorder
- Rust (only to build from source)

## Build

```bash
git clone https://github.com/johnsonjsyuen/voice-journal.git
cd voice-journal
cargo build --release
```

## Run

```bash
./target/release/voice-journal
```

A menu bar icon appears (no Dock icon):

| Icon | Meaning |
|---|---|
| Outline circle | Idle |
| Red circle | Recording |
| Orange square | Error — hover for details |

## Use

- Press **Ctrl+Alt+J** anywhere to start recording; press it again to stop.
- When transcription finishes, the text is appended to `~/Documents/VoiceJournal.md`.
- The tray menu shows the current status, the last transcript and its duration,
  and today's recording count/total time, plus **Open Journal…**,
  **Open Config…**, **Quit**.
- A brand-new journal gets a `# Voice Journal` header automatically.

## Configuration

The config file is created on first run at
`~/Library/Application Support/voice-journal/config.toml`:

```toml
# Global toggle hotkey (global-hotkey key syntax).
hotkey = "Ctrl+Alt+KeyJ"

# Journal destination. ~ is expanded.
journal_path = "~/Documents/VoiceJournal.md"
```

Restart the app after editing.

## Start at login

```bash
scripts/install-launchd.sh                      # install and start
scripts/install-launchd.sh --uninstall          # remove
```

Crash restarts are automatic; quitting from the tray menu stays quit. Logs go to
`~/Library/Logs/voice-journal.log`.

## Troubleshooting

- Run `voice-journal --check` to validate the config, journal writability, and
  TypeWhisper discovery/API.
- Tray error **"TypeWhisper API unavailable"** means TypeWhisper isn't running
  or its API Server is disabled (Settings → Advanced).
- **"No speech detected"** — the recording was silent, so nothing was appended.
- If writing the journal fails, the transcript is copied to the clipboard so
  speech is never lost.
- Journal not updating? Make sure the folder
  `~/Documents` is writable and that the model is finished downloading in
  TypeWhisper.

## License

MIT

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

## Download

Download `voice-journal-aarch64-apple-darwin.dmg` from
[GitHub Releases](https://github.com/johnsonjsyuen/voice-journal/releases).
Each successful build pushed to `main` publishes a release tagged with its commit
SHA. You do not need Rust to use the download.

1. Open the DMG and drag **Voice Journal.app** to **Applications**.
2. Eject the disk image, then open **Voice Journal** from Applications.
3. Look for the menu bar icon; the app has no Dock icon. Press **Ctrl+Alt+J** to
   start recording and again to save the transcript.

The app is ad-hoc signed, but is **not Developer ID signed or notarized**.
If macOS blocks the first launch and you trust this download, open
**System Settings → Privacy & Security → Open Anyway**, then confirm the launch.
See [Apple's instructions](https://support.apple.com/en-gb/102445).
The DMG also includes [INSTALL.txt](docs/INSTALL.txt) with installation and usage
instructions.

To upgrade, quit Voice Journal from its menu bar menu, then drag the new app into
Applications and choose **Replace**. Your journal and configuration are stored
outside the app.

### Archive alternative

The release also includes `voice-journal-aarch64-apple-darwin.tar.gz`.
Extract the archive into a permanent folder, then run:

```bash
tar -xzf voice-journal-aarch64-apple-darwin.tar.gz
./voice-journal
```

To start the archive binary at login, run `./install-launchd.sh ./voice-journal`
from that folder. The archive binary is not Developer ID signed or notarized.

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

The terminal shows a welcome banner, the active config and journal paths,
hotkey, and discovered TypeWhisper API address/port. Voice Journal is an API
client and does not listen on a network port itself. Timestamped logs show
hotkey detection, API start/stop requests and acknowledgements, transcription
text, and successful journal writes (or errors). Transcript text is included
in logs, including the launch-at-login log file below.

Logs default to info for Voice Journal. Use `RUST_LOG=warn` for quieter output,
or `RUST_LOG=voice_journal=debug` to include API polling requests.

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

For the DMG app, open **System Settings → General → Login Items** (called
**Login Items & Extensions** on some macOS versions), then add
`/Applications/Voice Journal.app` under **Open at Login**.

Use only one login method. If switching from the archive or source launchd
installer, remove that service first:

```bash
bash "/Applications/Voice Journal.app/Contents/Resources/install-launchd.sh" --uninstall
```

For a source build, use the launchd installer instead:

```bash
scripts/install-launchd.sh                      # install and start
scripts/install-launchd.sh --uninstall          # remove
```

With the launchd installer, crash restarts are automatic; quitting from the tray
menu stays quit. Logs go to
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

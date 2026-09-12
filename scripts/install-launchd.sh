#!/usr/bin/env bash
set -euo pipefail

LABEL="com.johnson.voice-journal"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
BIN="${1:-$(pwd)/target/release/voice-journal}"

xml_escape() {
  local s="$1"
  s="${s//&/&amp;}"
  s="${s//</&lt;}"
  s="${s//>/&gt;}"
  printf '%s' "$s"
}

if [[ "${1:-}" == "--uninstall" ]]; then
  launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
  rm -f "$PLIST"
  echo "Uninstalled $LABEL"
  exit 0
fi

# launchd runs with cwd /, so a relative ProgramArguments path must be absolute.
if [[ "$BIN" != /* ]]; then
  if bin_dir="$(cd -- "$(dirname -- "$BIN")" 2>/dev/null && pwd)"; then
    BIN="$bin_dir/$(basename -- "$BIN")"
  fi
fi

if [[ ! -x "$BIN" ]]; then
  echo "Binary not found or not executable: $BIN" >&2
  echo "Usage: $0 [path-to-voice-journal]  |  $0 --uninstall" >&2
  exit 1
fi

BIN_XML="$(xml_escape "$BIN")"
HOME_XML="$(xml_escape "$HOME")"

mkdir -p "$HOME/Library/LaunchAgents" "$HOME/Library/Logs"
cat > "$PLIST" <<PLIST_EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key>
  <array><string>$BIN_XML</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>StandardOutPath</key><string>$HOME_XML/Library/Logs/voice-journal.log</string>
  <key>StandardErrorPath</key><string>$HOME_XML/Library/Logs/voice-journal.log</string>
</dict>
</plist>
PLIST_EOF

launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$PLIST"
echo "Installed and started $LABEL ($BIN)"

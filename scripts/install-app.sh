#!/usr/bin/env bash
# Build the release .app and install it to /Applications (replacing the old one).
# The installed app uses the real data dir; its own startup migration backs up data first.
set -euo pipefail
cd "$(dirname "$0")/.."

pnpm tauri build --bundles app

SRC="src-tauri/target/release/bundle/macos/Pomelo OS.app"
DST="/Applications/Pomelo OS.app"

if pgrep -f "$DST/" >/dev/null; then
  osascript -e 'quit app "Pomelo OS"' >/dev/null 2>&1 || true
  sleep 1
fi
# Move (not copy): leaving the build output behind makes macOS list a second, real-data-using app.
rm -rf "$DST"
mv "$SRC" "$DST"
echo "已安装：$DST"

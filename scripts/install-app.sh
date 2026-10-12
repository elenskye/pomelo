#!/usr/bin/env bash
# Build the release .app and install it to /Applications (replacing the old one).
# The installed app uses the real data dir; its own startup migration backs up data first.
set -euo pipefail
cd "$(dirname "$0")/.."

# Signed with the fixed self-signed certificate (tauri.conf.json bundle.macOS.signingIdentity) so macOS
# sees every update as the same app: keychain items (小拾's API keys) stay readable.
if ! security find-identity -v -p codesigning | grep -q "Pomelo Self-Signed"; then
  echo "缺少签名证书 Pomelo Self-Signed（钥匙串访问 → 证书助理 → 创建证书，见 docs/architecture.md）" >&2
  exit 1
fi

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
codesign --verify --strict "$DST"
echo "已安装：$DST"

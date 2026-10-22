#!/usr/bin/env bash
# Rust build output (src-tauri/target) only grows: every profile / feature / toolchain change leaves
# another copy of each crate (deps/*-<hash>.rlib, incremental/, dep-graph.bin). All of it is rebuilt
# on demand, so it is simply wiped:
#   no flag  — now
#   --auto   — only when over POMELO_TARGET_LIMIT_GB (default 20); run by pnpm check / app:dev / app:build /
#              app:install and at the start of every Claude Code session (.claude/settings.json)
set -euo pipefail
cd "$(dirname "$0")/.."

T=src-tauri/target
LIMIT_GB=${POMELO_TARGET_LIMIT_GB:-20}
[ -d "$T" ] || exit 0
GB=$(( $(du -sk "$T" | cut -f1) / 1048576 ))
if [ "${1:-}" = "--auto" ] && [ "$GB" -lt "$LIMIT_GB" ]; then
  exit 0
fi
if pgrep -f "$PWD/$T/" >/dev/null; then
  echo "编译出来的程序正在运行，先不清理编译缓存" >&2
  exit 0
fi
PATH="$HOME/.cargo/bin:$PATH" cargo clean --quiet --manifest-path src-tauri/Cargo.toml
echo "已清理编译缓存 src-tauri/target（约 ${GB} GB），下次编译会从头编一遍"

#!/usr/bin/env bash
# PDFium (engine-pdf's `pdfium` feature, linked statically): an APFS clone (cp -c, no extra space) of
# engine-pdf's own copy into src-tauri/vendor/pdfium (not in Git). .cargo/config.toml points the build there.
set -euo pipefail
cd "$(dirname "$0")/.."
SRC=${ENGINE_PDF_DIR:-$HOME/Documents/GitHub/personal-engine-studio/engine-pdf}/local/pdfium/pdfium-mac-arm64
DST=src-tauri/vendor/pdfium
if [ -f "$DST/lib/libpdfium.a" ]; then
  exit 0
fi
if [ ! -f "$SRC/lib/libpdfium.a" ]; then
  echo "没找到 engine-pdf 的 PDFium（$SRC）：先照 engine-pdf 的 README 准备好" >&2
  exit 1
fi
mkdir -p "$DST/lib"
cp -c "$SRC/lib/libpdfium.a" "$DST/lib/"
cp -c "$SRC/LICENSE" "$DST/"
echo "PDFium 已克隆到 $DST"

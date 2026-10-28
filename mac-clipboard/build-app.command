#!/bin/bash
set -euo pipefail
HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
ROOT="$(dirname "$HERE")"
# Build into a fresh staging bundle so no legacy Python files survive.
STAGE="$(mktemp -d "$HERE/.native-build.XXXXXX")"
APP="$STAGE/JPEG Clipboard.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/mac-clipboard"
export MACOSX_DEPLOYMENT_TARGET=12.0
cargo build --locked --release --manifest-path "$ROOT/mac-rust-service/Cargo.toml"
/usr/bin/swiftc -O -target "$(uname -m)-apple-macosx12.0" -module-cache-path "$HERE/.module-cache" "$HERE/ClientApp.swift" -o "$APP/Contents/MacOS/JPEGClipboard"
/usr/bin/swiftc -O -target "$(uname -m)-apple-macosx12.0" -module-cache-path "$HERE/.module-cache" "$HERE/Clipboard.swift" -o "$APP/Contents/Resources/mac-clipboard/clipboard-helper"
cp "$ROOT/mac-rust-service/target/release/jpeg-clipboard-service" "$APP/Contents/Resources/mac-clipboard/"
cp "$HERE/Info.plist" "$APP/Contents/Info.plist"
/usr/bin/codesign --force --deep --sign - "$APP"
/usr/bin/codesign --verify --deep --strict "$APP"
# Verify the exact replacement target before deleting the old generated bundle.
TARGET="$HERE/JPEG Clipboard.app"
if [[ "$TARGET" != "$ROOT/mac-clipboard/JPEG Clipboard.app" ]]; then echo 'Unexpected bundle destination' >&2; exit 1; fi
if [[ -d "$TARGET" ]]; then rm -rf -- "$TARGET"; fi
/usr/bin/ditto "$APP" "$TARGET"
echo "Built Python-free App: $TARGET"

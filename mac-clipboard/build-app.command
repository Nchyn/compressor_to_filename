#!/bin/bash
set -euo pipefail
HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
ROOT="$(dirname "$HERE")"
APP="$HERE/JPEG Clipboard.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/mac-clipboard" "$APP/Contents/Resources/res"
/usr/bin/swiftc -O -module-cache-path "$HERE/.module-cache" "$HERE/ClientApp.swift" -o "$APP/Contents/MacOS/JPEGClipboard"
/usr/bin/swiftc -O -module-cache-path "$HERE/.module-cache" "$HERE/Clipboard.swift" -o "$APP/Contents/Resources/mac-clipboard/clipboard-helper"
cp "$HERE/pairing.py" "$APP/Contents/Resources/mac-clipboard/pairing.py"
cp "$HERE/server.py" "$APP/Contents/Resources/mac-clipboard/server.py"
cp "$ROOT/indexv2.html" "$APP/Contents/Resources/"
for asset in manifest.json; do cp "$ROOT/$asset" "$APP/Contents/Resources/"; done
for asset in jszip.min.js FileSaver.min.js jpeg-encode-worker.mjs; do cp "$ROOT/res/$asset" "$APP/Contents/Resources/res/"; done
cp "$HERE/Info.plist" "$APP/Contents/Info.plist"
/usr/bin/codesign --force --deep --sign - "$APP"
echo "Built: $APP"

#!/bin/bash
set -euo pipefail
HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
if [[ ! -x "$HERE/clipboard-helper" || "$HERE/Clipboard.swift" -nt "$HERE/clipboard-helper" ]]; then
  /usr/bin/swiftc -O -module-cache-path "$HERE/.module-cache" "$HERE/Clipboard.swift" -o "$HERE/clipboard-helper"
fi
exec /usr/bin/python3 "$HERE/server.py"

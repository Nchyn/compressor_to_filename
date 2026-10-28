#!/bin/bash
set -euo pipefail
set +x
HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
case "${1:---preview}" in
  --dry-run|--preview) MODE=preview ;;
  --deploy) MODE=production ;;
  --build-only) MODE=build-only ;;
  *) echo 'Usage: deploy-eo.command [--preview|--dry-run|--deploy|--build-only]'; exit 2 ;;
esac
exec python3 "$HERE/deploy-eo.py" "$MODE"

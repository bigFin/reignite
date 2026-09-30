#!/usr/bin/env bash
set -euo pipefail
# Compose with upstream (or your existing save wrapper), never replace its logic.
if [ "$#" -lt 2 ]; then echo "usage: save.sh SIDECAR UPSTREAM_SAVE [args...]" >&2; exit 2; fi
sidecar="$1"; upstream="$2"; shift 2
"$upstream" "$@"
python3 "$(dirname "$0")/tmux-sidecar.py" reconcile "$sidecar"

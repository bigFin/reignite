#!/usr/bin/env bash
set -euo pipefail
# Call only after the intended snapshot-specific sidecar has been selected/copied.
if [ "$#" -lt 2 ]; then echo "usage: restore.sh SIDECAR UPSTREAM_RESTORE [args...]" >&2; exit 2; fi
sidecar="$1"; upstream="$2"; shift 2
python3 "$(dirname "$0")/tmux-sidecar.py" prepare "$sidecar"
"$upstream" "$@"

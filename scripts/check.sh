#!/usr/bin/env bash
# Run inside the flake dev shell. Integration uses disposable sessions only.
set -euo pipefail

usage() {
  echo "usage: nix develop --command bash scripts/check.sh [--integration]"
}

integration=false
if [ "$#" -gt 1 ]; then usage >&2; exit 2; fi
case "${1:-}" in
  "") ;;
  --integration) integration=true ;;
  --help|-h) usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac

if "$integration"; then
  : "${PI_PACKAGE_DIR:?--integration requires the installed Pi package directory}"
  : "${TMUX_ASSISTANT_PLUGIN_DIR:?--integration requires the tmux-assistant-resurrect plugin directory}"
fi

cd "$(dirname "$0")/.."
cargo fmt --check
for script in scripts/*.sh integration/*.sh; do
  bash -n "$script"
done
shellcheck scripts/*.sh integration/*.sh .envrc
actionlint .github/workflows/*.yml
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
node --test tests/pi-adapter.test.ts
python3 tests/tmux_sidecar.py
nix flake check --no-update-lock-file

if "$integration"; then
  bash scripts/check-adapter.sh
  python3 scripts/pi-smoke.py
  python3 scripts/tmux-transport-smoke.py
  echo "PASS: local and isolated integration checks (boot changes simulated, no real reboot)."
else
  echo "PASS: local checks. SDK typecheck and real Pi/tmux smokes require --integration."
fi

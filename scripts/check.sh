#!/usr/bin/env bash
# Run inside the flake dev shell. Integration uses disposable sessions only.
set -euo pipefail

usage() {
  echo "usage: nix develop --command bash scripts/check.sh [--native-integration|--integration]"
}

integration=false
native=false
if [ "$#" -gt 1 ]; then usage >&2; exit 2; fi
case "${1:-}" in
  "") ;;
  --native-integration) native=true ;;
  --integration) integration=true; native=true ;;
  --help|-h) usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac

if "$native"; then
  : "${PI_PACKAGE_DIR:?native integration requires the installed Pi package directory}"
fi
if "$integration"; then
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

if "$native"; then
  python3 scripts/pi-rpc-smoke.py
  node scripts/pi-eligibility-smoke.mjs
  python3 scripts/pi-intent-smoke.py
  python3 scripts/pi-owned-launch-smoke.py
  python3 scripts/pi-delivery-smoke.py
  python3 scripts/pi-handoff-smoke.py
fi
if "$integration"; then
  bash scripts/check-adapter.sh
  python3 scripts/pi-smoke.py
  python3 scripts/tmux-transport-smoke.py
  echo "PASS: local and isolated integration checks (boot changes simulated, no real reboot)."
elif "$native"; then
  echo "PASS: local checks and real tmux-free Pi evidence/RPC delivery/operator handoff/process-loss (no native automatic recovery or reboot)."
else
  echo "PASS: local checks. Real Pi RPC needs --native-integration; Pi/tmux prototype smokes need --integration."
fi

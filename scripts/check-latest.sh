#!/usr/bin/env bash
# Run inside the Nix dev shell. Downloads disposable fixtures, never updates Pi.
set -euo pipefail

if [ "$#" -gt 0 ]; then
  echo "usage: nix develop --command bash scripts/check-latest.sh" >&2
  case "$1" in --help|-h) exit 0 ;; *) exit 2 ;; esac
fi

cd "$(dirname "$0")/.."
fixtures="$(mktemp -d)"
trap 'rm -rf "$fixtures"' EXIT
npm install --ignore-scripts --no-audit --no-fund --prefix "$fixtures/pi" \
  @earendil-works/pi-coding-agent@latest @types/node
export PI_PACKAGE_DIR="$fixtures/pi/node_modules/@earendil-works/pi-coding-agent"
export PI_NODE_TYPES_DIR="$fixtures/pi/node_modules/@types/node"
git clone --depth 1 https://github.com/timvw/tmux-assistant-resurrect.git "$fixtures/plugin"
export TMUX_ASSISTANT_PLUGIN_DIR="$fixtures/plugin"
node -e 'console.log("Pi fixture:", JSON.parse(require("node:fs").readFileSync(process.env.PI_PACKAGE_DIR + "/package.json", "utf8")).version)'
printf 'tmux-assistant-resurrect fixture: '
git -C "$fixtures/plugin" rev-parse HEAD
bash scripts/check.sh --integration

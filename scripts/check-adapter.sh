#!/usr/bin/env bash
set -euo pipefail
# Installed Pi is a read-only SDK reference; no npm install or live config edits.
: "${PI_PACKAGE_DIR:?Set PI_PACKAGE_DIR to the installed pi-coding-agent package}"
root="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
cp "$root/adapters/pi.ts" "$tmp/pi.ts"
mkdir -p "$tmp/node_modules/@earendil-works" "$tmp/node_modules/@types"
ln -s "$PI_PACKAGE_DIR" "$tmp/node_modules/@earendil-works/pi-coding-agent"
node_types="${PI_NODE_TYPES_DIR:-$PI_PACKAGE_DIR/node_modules/@types/node}"
if [ ! -d "$node_types" ]; then node_types="$PI_PACKAGE_DIR/../../@types/node"; fi
if [ ! -d "$node_types" ]; then echo "Set PI_NODE_TYPES_DIR to installed @types/node" >&2; exit 1; fi
ln -s "$node_types" "$tmp/node_modules/@types/node"
tsc --noEmit --strict --skipLibCheck --target es2022 --module nodenext --moduleResolution nodenext "$tmp/pi.ts"

#!/usr/bin/env bash
# Runs before `cargo tauri build`: stage the release sidecar, install the
# locked frontend deps, and produce ui/dist for the bundler to embed.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"
"$SCRIPT_DIR/stage-sidecar.sh" release
npm --prefix "$ROOT/ui" ci
npm --prefix "$ROOT/ui" run build

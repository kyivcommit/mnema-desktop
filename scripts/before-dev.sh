#!/usr/bin/env bash
# Runs before `cargo tauri dev`: stage the extraction-worker sidecar (the
# invariant the ci.yml comments guard), then hand off to the long-lived Vite
# dev server. Paths derive from this script's location, not the caller's CWD.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"
"$SCRIPT_DIR/stage-sidecar.sh" debug
exec npm --prefix "$ROOT/ui" run dev

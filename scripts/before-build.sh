#!/usr/bin/env bash
# Runs before `cargo tauri build`: stage the release sidecar, install the
# locked frontend deps, and produce ui/dist for the bundler to embed.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"
# The release ships the real MLX sidecar; stage-sidecar.sh would only write a placeholder.
# MNEMA_MLX_PREBUILT=1 (CI, after its own build step) skips the second build, but only over a
# real binary: a placeholder is a shell script, and a script cannot be a release.
if [ "$(uname -s)" = "Darwin" ]; then
  staged="$ROOT/src-tauri/binaries/mnema-mlx-aarch64-apple-darwin"
  if [ "${MNEMA_MLX_PREBUILT:-}" = "1" ] && [ -f "$staged" ] && [ "$(head -c 2 "$staged")" != "#!" ]; then
    echo "before-build: MLX sidecar already built, reusing $staged"
  else
    "$SCRIPT_DIR/build-mlx-sidecar.sh"
  fi
fi
"$SCRIPT_DIR/stage-sidecar.sh" release
npm --prefix "$ROOT/ui" ci
npm --prefix "$ROOT/ui" run build

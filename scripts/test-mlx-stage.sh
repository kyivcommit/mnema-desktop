#!/usr/bin/env bash
# build-mlx-sidecar.sh --stage-only with a toolchain that has no libswiftCompatibilitySpan.dylib must exit 6
# with its own message — on macOS's bash 3.2 too, where an empty array under `set -u` is an unbound variable.
set -uo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
mkdir -p "$t/bin" "$t/lib/swift-6.2/macosx"
printf '#!/bin/sh\necho "%s/bin/swift"\n' "$t" > "$t/bin/xcrun"
chmod +x "$t/bin/xcrun"
out=$(PATH="$t/bin:$PATH" /bin/bash "$ROOT/scripts/build-mlx-sidecar.sh" --stage-only 2>&1)
code=$?
echo "$out"
if [[ $code -ne 6 ]]; then echo "FAIL: exit $code, expected 6"; exit 1; fi
if ! grep -q 'expected exactly one in the active toolchain, found 0' <<<"$out"; then echo "FAIL: message missing"; exit 1; fi
echo "ok   no Span dylib -> exit 6"

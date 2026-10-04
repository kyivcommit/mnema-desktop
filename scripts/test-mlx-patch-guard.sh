#!/usr/bin/env bash
# The Metal 3.1 patch guard of build-mlx-sidecar.sh, on a throwaway checkout of the four patched files in
# their "before" state, rebuilt from the patch itself (each hunk's ' ' and '-' lines).
set -uo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PATCH=$ROOT/sidecar/mnema-mlx/patches/mlx-metal31.patch
BUILD=$ROOT/scripts/build-mlx-sidecar.sh
fail=0
dirs=()
trap 'rm -rf "${dirs[@]}"' EXIT

checkout() {
    local d
    d=$(mktemp -d)
    git -C "$d" init -q
    awk -v d="$d" '
        /^\+\+\+ b\// { f = d "/" substr($0, 7); n = split(f, p, "/"); dir = substr(f, 1, length(f) - length(p[n]) - 1)
                        system("mkdir -p \"" dir "\""); next }
        /^@@/ { inhunk = 1; next }
        /^diff / { inhunk = 0; next }
        inhunk && /^[ -]/ { print substr($0, 2) > f }
    ' "$PATCH"
    echo "$d"
}

run() { MNEMA_MLX_CHECKOUT=$1 "$BUILD" --patch-only 2>&1; }

check() {  # name, expected exit, actual exit
    if [[ "$2" != "$3" ]]; then echo "FAIL $1: exit $3, expected $2"; fail=1; else echo "ok   $1"; fi
}

files=$(sed -n 's|^+++ b/||p' "$PATCH")

# Clean upstream: patch applies, every file gains the version switch.
d=$(checkout); dirs+=("$d"); out=$(run "$d"); code=$?
check clean 0 "$code"
for f in $files; do
    grep -q '__METAL_VERSION__ >= 320' "$d/$f" || { echo "FAIL clean: $f has no '__METAL_VERSION__ >= 320'"; fail=1; }
done

# Upstream already carries the patch: the build must say so and stop.
d=$(checkout); dirs+=("$d"); git -C "$d" apply "$PATCH"; out=$(run "$d"); code=$?
check already-patched 4 "$code"
grep -q 'латку прийнято в апстрім — видаліть її' <<<"$out" || { echo "FAIL already-patched: message missing: $out"; fail=1; }

# Upstream changed a context line: the patch no longer applies.
d=$(checkout); dirs+=("$d"); f=$(head -1 <<<"$files")
sed -i '' 's|^// Binary Operators on Integral constants$|// Binary operators, upstream reworded|' "$d/$f"
out=$(run "$d"); code=$?
check context-changed 3 "$code"

# Upstream removed one of the files: still "does not apply", not a chmod failure.
d=$(checkout); dirs+=("$d"); rm "$d/$(tail -1 <<<"$files")"
out=$(run "$d"); code=$?
check file-missing 3 "$code"

exit $fail

#!/usr/bin/env bash
# Builds sidecar/mnema-mlx (Release) with the Metal 3.1 patch applied and stages it for Tauri:
#   src-tauri/binaries/mnema-mlx-aarch64-apple-darwin
#   src-tauri/binaries/mlx-swift_Cmlx.bundle/
#   src-tauri/binaries/libswiftCompatibilitySpan.dylib
#
# xcodebuild only: `swift build` does not compile MLX's Metal shaders, and its checkout lives elsewhere
# (.build/checkouts, not .build-xc/checkouts), so a patch applied here would not reach it.
#
# --patch-only   only the guard and `git apply` on the mlx-swift checkout ($MNEMA_MLX_CHECKOUT, default
#                the one xcodebuild resolves into sidecar/mnema-mlx/.build-xc)
#
# Exit codes: 3 the patch does not apply, 4 the patch is already upstream, 5 the patch is not in the
# checkout after the build.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PKG=$ROOT/sidecar/mnema-mlx
XC=$PKG/.build-xc
PATCH=$PKG/patches/mlx-metal31.patch
CHECKOUT=${MNEMA_MLX_CHECKOUT:-$XC/checkouts/mlx-swift}

case "${1:-}" in
    -h | --help) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
esac

apply_patch() {
    # SwiftPM checks packages out read-only; git apply cannot write without this.
    sed -n 's|^+++ b/||p' "$PATCH" | while read -r f; do chmod u+w "$CHECKOUT/$f"; done
    if git -C "$CHECKOUT" apply --reverse --check "$PATCH" 2>/dev/null; then
        echo "mlx-metal31.patch: латку прийнято в апстрім — видаліть її" >&2
        exit 4
    fi
    if ! git -C "$CHECKOUT" apply --check "$PATCH"; then
        echo "mlx-metal31.patch: латка не накладається на $CHECKOUT" >&2
        exit 3
    fi
    git -C "$CHECKOUT" apply "$PATCH"
}

if [[ "${1:-}" == "--patch-only" ]]; then
    apply_patch
    exit 0
fi

cd "$PKG"
# A fresh checkout each time, so the guard always sees upstream and never our own earlier apply.
rm -rf "$XC/checkouts/mlx-swift"
xcodebuild -resolvePackageDependencies -onlyUsePackageVersionsFromResolvedFile \
    -clonedSourcePackagesDirPath "$XC" -quiet
apply_patch
# No resolution during the build: it would reset the patched checkout.
xcodebuild build -scheme mnema-mlx -configuration Release -destination 'platform=macOS,arch=arm64' \
    -clonedSourcePackagesDirPath "$XC" -derivedDataPath "$XC/dd" \
    -disableAutomaticPackageResolution -skipPackageUpdates -skipMacroValidation -quiet
if ! git -C "$CHECKOUT" apply --reverse --check "$PATCH"; then
    echo "mlx-metal31.patch: латки нема в $CHECKOUT після збирання" >&2
    exit 5
fi

OUT=$XC/dd/Build/Products/Release
BIN=$ROOT/src-tauri/binaries
mkdir -p "$BIN"
cp "$OUT/mnema-mlx" "$BIN/mnema-mlx-aarch64-apple-darwin"
rm -rf "$BIN/mlx-swift_Cmlx.bundle"
cp -R "$OUT/mlx-swift_Cmlx.bundle" "$BIN/"
# The binary links @rpath/libswiftCompatibilitySpan.dylib (rpath @executable_path/../lib); the
# toolchain has it, the build products do not.
cp "$(dirname "$(xcrun -f swift)")/../lib/swift-6.2/macosx/libswiftCompatibilitySpan.dylib" "$BIN/"
echo "staged mnema-mlx in $BIN"

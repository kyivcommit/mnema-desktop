#!/usr/bin/env bash
#
# Checks the macOS disk image that `cargo tauri build` produced, by opening it
# and looking at the application a user would actually drag out of it.
#
# It looks inside the .dmg rather than at `target/release/bundle/macos/` because
# that directory is EMPTY after a build: with `targets: ["dmg"]` the bundler
# treats the .app as an intermediate and prints `Cleaning …/Mnema.app` once the
# image is written. A check pointed at the staging path reports "no bundle" on a
# perfectly good build.
#
# Why a script rather than four lines in the workflow: a check that only exists
# inside CI is a check nobody can break on purpose, and this repository has
# already shipped a CI step that passed while running nothing (task 7). Every
# failure branch below was produced deliberately before this file was committed.
#
# Nothing here ends in `|| true` or `|| echo`. A packaging check that reports an
# absence as prose and exits 0 is the failure mode, not the report.
#
# Usage: scripts/verify-bundle.sh [bundle-dir]
#   bundle-dir defaults to target/release/bundle

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bundle_dir="${1:-${repo_root}/target/release/bundle}"

# The binary name comes from `[[bin]] name` in src-tauri/Cargo.toml and is not
# the product name: `Mnema.app/Contents/MacOS/mnema-desktop`. Renaming one does
# not rename the other.
executable_name="mnema-desktop"

fail() {
  echo "verify-bundle: $1" >&2
  exit 1
}

if [ "$(uname -s)" != "Darwin" ]; then
  fail "this opens a macOS .dmg; nothing to check on $(uname -s)."
fi

# Derived, not spelled out: renaming the product in tauri.conf.json renames the
# bundle, and a hard-coded "Mnema.app" here would then fail with "no app inside
# the image" — a true statement pointing at the wrong cause.
product="$(sed -n 's/.*"productName"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
  "${repo_root}/src-tauri/tauri.conf.json")"
[ -n "${product}" ] || fail "src-tauri/tauri.conf.json declares no productName."

# --- the image exists at all --------------------------------------------------
#
# Checked first because everything below reports something misleading when it is
# missing: hdiutil on a nonexistent path says "no such file", which reads as a
# broken script rather than as "you did not build anything".
dmg_dir="${bundle_dir}/dmg"
# The directory before its contents. Without this, `find` on a path that does not
# exist fails, and under `set -euo pipefail` the failing pipeline inside the
# command substitution below ends the script with **no output at all** — the one
# outcome the comment above promises not to produce. An empty directory printed a
# good message; an absent one printed nothing.
[ -d "${dmg_dir}" ] || fail "no ${dmg_dir}. Run: cargo tauri build"
dmg_count="$(find "${dmg_dir}" -maxdepth 1 -type f -name '*.dmg' | wc -l | tr -d ' ')"
case "${dmg_count}" in
  0) fail "no .dmg in ${dmg_dir}. Run: cargo tauri build" ;;
  1) : ;;
  # Two images means one of them is from an older build, and picking either
  # would verify a file nobody is about to ship.
  *) fail "${dmg_count} .dmg files in ${dmg_dir}. Remove the stale ones: a check
  that verifies whichever one sorted first proves nothing about the new build." ;;
esac
dmg="$(find "${dmg_dir}" -maxdepth 1 -type f -name '*.dmg')"
[ -s "${dmg}" ] || fail "${dmg} is empty."

echo "verify-bundle: ${dmg} ($(du -h "${dmg}" | cut -f1))"

# --- open it ------------------------------------------------------------------
#
# `-nobrowse` keeps it out of the Finder sidebar, `-readonly` makes it plain
# that nothing here modifies the artefact under test. The trap detaches on every
# exit path; a left-behind mount survives the job and the next run then attaches
# a second copy of the same volume.
mountpoint="$(mktemp -d "${TMPDIR:-/tmp}/mnema-bundle.XXXXXX")"
trap 'hdiutil detach "${mountpoint}" -quiet >/dev/null 2>&1 || true; rmdir "${mountpoint}" 2>/dev/null || true' EXIT
# No `-noverify`: letting hdiutil check the image's own checksum costs about a
# second here and is the only thing that looks at the bytes a user downloads.
hdiutil attach -readonly -nobrowse -mountpoint "${mountpoint}" "${dmg}" >/dev/null \
  || fail "${dmg} could not be attached. An image that does not open is not a package."

app="${mountpoint}/${product}.app"
[ -d "${app}" ] || fail "${dmg} contains no ${product}.app. It holds: $(ls -A "${mountpoint}" | tr '\n' ' ')"
[ -x "${app}/Contents/MacOS/${executable_name}" ] \
  || fail "${product}.app has no executable at Contents/MacOS/${executable_name}."

echo "verify-bundle: ${product}.app ($(du -sh "${app}" | cut -f1)), $(file -b "${app}/Contents/MacOS/${executable_name}")"

# --- the signature ------------------------------------------------------------
#
# `--deep` is deprecated for *signing* and is still right for verifying: it walks
# nested code instead of stopping at the outer seal, which is the whole question
# once a bundle carries a library. `--strict` refuses the relaxations Gatekeeper
# does not grant either.
#
# This passes only because tauri.conf.json sets `macOS.signingIdentity` to "-".
# Without it the bundler leaves the .app unsealed — the executable still carries
# the ad-hoc signature the linker put on it, but the bundle has no
# `_CodeSignature`, and codesign then answers "code has no resources but
# signature indicates they must be present" and exits 1. Measured on this
# repository's first bundle, before the identity was set.
#
# What it does NOT establish: Gatekeeper acceptance. An ad-hoc signature has no
# Team ID and the image is not notarized, so `spctl -a -t exec` rejects it. That
# is a decision recorded in docs/BUILD.md, not something to paper over here.
if ! codesign --verify --deep --strict --verbose=2 "${app}"; then
  fail "the signature of ${product}.app inside the image does not verify. See
  docs/BUILD.md: the build ad-hoc signs on purpose, so a failure here means the
  seal broke or the identity setting was dropped — not that signing is optional."
fi

# --- what the bundle has to carry ---------------------------------------------
#
# Derived from the dependency graph rather than asserted as a constant. Today the
# shell does not depend on mnema-extract, so no code inside the bundle can load
# Pdfium and shipping the 7.7 MB library would be dead weight. The day somebody
# wires extraction into the shell, this turns red until the library is packaged
# with it — which is the only moment the omission is cheap to fix.
#
# `cargo tree`, not a grep over a manifest: the dependency can arrive through any
# crate in between, and the graph is what decides what is linked.
command -v cargo >/dev/null 2>&1 \
  || fail "cargo is not on PATH, so the dependency check below cannot run. It is
  not optional — skipping it is how a bundle ships without the library it needs."

# The graph is CAPTURED first and searched second, and the two steps must not be
# merged back into one `if`. Inside an `if` condition neither `set -e` nor
# `pipefail` ends anything: a non-zero status is simply "false". So a `cargo tree`
# that failed — a renamed package, an unreadable manifest, an unreachable
# registry, a flag that changed meaning — would be indistinguishable from the
# answer "this dependency is absent", take the `else` branch, print that no
# Pdfium is needed and exit 0. Measured, before this line existed: `-p
# mnema-shell` and `--manifest-path /nonexistent/Cargo.toml` both produced a
# green run with the reassuring message. And the day the check must redden is the
# day someone is moving crates around, which is exactly when a package name is
# most likely to be wrong.
deps="$(cargo tree --manifest-path "${repo_root}/Cargo.toml" \
  -p mnema-desktop -e normal --prefix none)" \
  || fail "cargo tree failed, so whether the bundle needs Pdfium is UNANSWERED —
  which is not the same as answered 'no'. Fix the workspace or the package name
  above; do not let this degrade into a green run."

if printf '%s\n' "${deps}" | cut -d' ' -f1 | sort -u | grep -qx 'pdfium-render'; then
  echo "verify-bundle: the shell links pdfium-render, so the library must be inside the bundle"
  found="$(find "${app}" -name 'libpdfium*.dylib' | head -1)"
  [ -n "${found}" ] || fail "mnema-desktop depends on pdfium-render, but
  ${product}.app carries no libpdfium.dylib. Pdfium is opened by path at run
  time, never linked, so the executable starts fine and fails on the first PDF
  instead of at launch. docs/BUILD.md has where it must go, and why
  Contents/Resources is not that place."
  codesign --verify --strict --verbose=2 "${found}" \
    || fail "${found} is inside the bundle but its own signature does not verify.
  Under the hardened runtime the loader refuses a library it cannot validate."
else
  echo "verify-bundle: the shell does not link pdfium-render, so no Pdfium is bundled"
fi

echo "verify-bundle: OK"

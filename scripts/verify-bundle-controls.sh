#!/usr/bin/env bash
#
# Negative controls for scripts/verify-bundle.sh: every state that check is
# supposed to reject, produced on purpose, each one required to exit non-zero.
#
# Why this is in the repository rather than in whoever's scratch directory, for
# the same reason scripts/mutation-check.sh is: a guard nobody else can run is a
# guard nobody else can check. The count of controls was written into three
# documents by hand once, and the three disagreed — one of them describing a
# control that had never been run. This file is now the list, and the number is
# whatever it prints.
#
# It never touches the working tree. Controls that need a different repository —
# a dependency added, a manifest broken — get a copy of the tracked files under
# $TMPDIR and run the copy's script against the real bundle, which the script
# accepts as an argument.
#
# Every control checks its own setup, and that is not decoration. The first
# version did not: `copy_app_out` returned 1 when the image would not attach,
# nobody looked, and controls 6 to 8 then built an image out of a directory that
# was never created. They still went red — on "no .dmg in …", which is control
# 2's reason — and the run still reported ten. Measured before this paragraph was
# written. A control that reddens for a reason it does not name, counted as
# proof, is the defect this whole file exists to guard against, so setup failures
# are counted separately and loudly as BROKEN.
#
# No `set -e`, deliberately, for the reason mutation-check.sh has none: nearly
# every command below is expected to fail, since that is what a red control is.
#
# Not part of CI: it needs a built bundle and takes minutes. It is what you run
# before believing verify-bundle.sh, and what a reviewer reproduces.
#
# Usage:
#   cargo tauri build
#   scripts/verify-bundle-controls.sh

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE="${REPO}/target/release/bundle"
LAB="$(mktemp -d "${TMPDIR:-/tmp}/mnema-controls.XXXXXX")"
trap 'chmod -R u+w "$LAB" 2>/dev/null; rm -rf "$LAB"' EXIT

if [ "$(uname -s)" != "Darwin" ]; then
  echo "verify-bundle-controls: macOS only." >&2
  exit 1
fi

GOOD="$(find "${BUNDLE}/dmg" -maxdepth 1 -type f -name '*.dmg' 2>/dev/null | head -1)"
if [ -z "${GOOD}" ]; then
  echo "verify-bundle-controls: no .dmg under ${BUNDLE}/dmg. Run: cargo tauri build" >&2
  exit 1
fi

red=0
green=0
broken=0

# Runs the check and requires it to fail. A control that passes is not a smaller
# problem than a broken check — it IS the broken check, undetected.
#
# The line printed is the LAST `verify-bundle:` line, not the first: the first is
# the informational one naming the image, and the last is the failure message,
# which is the only thing that says whether the control reddened for the reason
# it claims. Controls 9 and 10 have two possible red exits between them and are
# indistinguishable without it.
#
# An optional `-m FRAGMENT` before the label asserts that fragment is present in
# that line — mechanizing the "own reason" rule instead of leaving it to a human
# to read the message and judge. Without `-m` a control counts as red on exit
# status alone, exactly as before: controls 1 through 8 call this unchanged and
# stay unchanged. The gap this closes is real, not theoretical — control 11
# (added in this same task) went red on the codesign section's message before
# its own check existed, and nothing before this could tell the difference
# between that and the check it names actually firing.
expect_red() {
  local expect=""
  if [ "$1" = "-m" ]; then
    expect="$2"
    shift 2
  fi
  local label="$1" script="$2"
  shift 2
  local out status full msg
  out=$(bash "$script" "$@" 2>&1)
  status=$?
  printf '%s\n' "-- ${label}"
  if [ $status -ne 0 ]; then
    # Matched against the FULL line, not the 140-column one below it is cut to for
    # display: a long mktemp path pushes the fragment past column 140 on its own,
    # which is not a wrong reason, only a long path. Measured on control 12 before
    # this split existed — its correct message was reported WRONG REASON only
    # because the path ate the columns the fragment needed.
    full="$(printf '%s' "$out" | grep 'verify-bundle:' | tail -1)"
    msg="$(printf '%s' "$full" | cut -c1-140)"
    if [ -n "${expect}" ] && ! printf '%s' "${full}" | grep -qF -- "${expect}"; then
      printf '   red (exit %s), but not for its own reason: %s\n' "$status" "$msg"
      printf '   *** WRONG REASON — this control proves nothing. Expected: %s ***\n' "$expect"
      green=$((green + 1))
    else
      printf '   red (exit %s): %s\n' "$status" "$msg"
      red=$((red + 1))
    fi
  else
    printf '   *** STILL GREEN — this control proves nothing ***\n'
    green=$((green + 1))
  fi
}

# A setup step that must succeed. Anything the control needs in place before the
# check runs goes through this, so that a failure is reported as a broken control
# rather than becoming a red one with somebody else's reason.
must() {
  "$@" && return 0
  printf '   BROKEN CONTROL: setup failed: %s\n' "$*"
  broken=$((broken + 1))
  return 1
}

# The mutation must have happened. Same guard as `git diff --quiet` in
# mutation-check.sh, in the shape a filesystem allows: proof that the state the
# control names was actually produced.
gone() {
  [ ! -e "$1" ] && return 0
  printf '   BROKEN CONTROL: %s was supposed to be removed and is still there\n' "$1"
  broken=$((broken + 1))
  return 1
}

there() {
  [ -e "$1" ] && return 0
  printf '   BROKEN CONTROL: %s was supposed to be created and is not\n' "$1"
  broken=$((broken + 1))
  return 1
}

# A fresh copy of Mnema.app, taken out of the real image. Every structural
# control mutates one of these and rebuilds an image around it, so that what is
# being checked is a disk image and not a directory that resembles one.
copy_app_out() {
  local into="$1" mnt
  mnt=$(mktemp -d "${LAB}/mnt.XXXXXX") || return 1
  hdiutil attach -readonly -nobrowse -mountpoint "$mnt" "${GOOD}" >/dev/null || return 1
  mkdir -p "$into" || return 1
  cp -R "$mnt/Mnema.app" "$into/" || return 1
  hdiutil detach "$mnt" -quiet >/dev/null 2>&1
  rmdir "$mnt" 2>/dev/null
  # The copy comes off a read-only volume, so the seal is read-only too and the
  # tampering controls below cannot write into it.
  chmod -R u+w "$into" || return 1
  # Reports failure rather than an empty directory: everything downstream would
  # otherwise redden on the wrong thing.
  [ -x "$into/Mnema.app/Contents/MacOS/mnema-desktop" ]
}

image_from() {
  local src="$1" dest="$2"
  mkdir -p "$(dirname "$dest")" || return 1
  hdiutil create -quiet -volname Mnema -srcfolder "$src" -ov -format UDZO "$dest" || return 1
  [ -s "$dest" ]
}

# A copy of the repository, tracked files only, taken from the WORKING TREE — so
# it carries uncommitted edits to the script under test. `git ls-files` rather
# than `cp -R` keeps target/ and vendor/ out, which is the difference between a
# copy and a several-gigabyte one.
copy_repo() {
  local into="$1"
  mkdir -p "$into" || return 1
  (cd "$REPO" && git ls-files -z | xargs -0 tar cf -) | tar xf - -C "$into" || return 1
  [ -x "$into/scripts/verify-bundle.sh" ]
}

# Task 2's freshness check runs before the section controls 9 and 10 exist to
# test, and looks at ${into}/target/release and ${into}/src-tauri/binaries —
# both git-ignored, so copy_repo never carries them. Without this, both controls
# would redden on "no built worker" instead of on the reason they name. Stages a
# matching pair, copied from the real build, so that check passes quietly and
# execution reaches the cargo tree logic being tested. The triple is derived the
# same way scripts/stage-sidecar.sh does, rather than hardcoded, so this does not
# print a false arm64 name on an Intel host.
stage_fresh_worker() {
  local into="$1" triple
  triple="$(rustc -vV | sed -n 's/^host: //p')"
  [ -n "${triple}" ] || return 1
  mkdir -p "${into}/target/release" "${into}/src-tauri/binaries" || return 1
  cp "${REPO}/target/release/mnema-extract-worker" "${into}/target/release/mnema-extract-worker" || return 1
  cp "${REPO}/target/release/mnema-extract-worker" \
    "${into}/src-tauri/binaries/mnema-extract-worker-${triple}"
}

echo "### 1. the dmg directory does not exist"
expect_red "no bundle at all" "${REPO}/scripts/verify-bundle.sh" "${LAB}/absent"

echo "### 2. the dmg directory is empty"
if must mkdir -p "${LAB}/empty/dmg"; then
  expect_red "built nothing" "${REPO}/scripts/verify-bundle.sh" "${LAB}/empty"
fi

echo "### 3. two images side by side"
if must mkdir -p "${LAB}/two/dmg" \
  && must cp "${GOOD}" "${LAB}/two/dmg/a.dmg" \
  && must cp "${GOOD}" "${LAB}/two/dmg/b.dmg"; then
  expect_red "one of them is from an older build" "${REPO}/scripts/verify-bundle.sh" "${LAB}/two"
fi

echo "### 4. an image that will not attach"
if must mkdir -p "${LAB}/corrupt/dmg" \
  && must dd if=/dev/urandom of="${LAB}/corrupt/dmg/Mnema_0.0.0_aarch64.dmg" bs=1024 count=200 status=none \
  && there "${LAB}/corrupt/dmg/Mnema_0.0.0_aarch64.dmg"; then
  expect_red "the file is named .dmg and is not one" "${REPO}/scripts/verify-bundle.sh" "${LAB}/corrupt"
fi

echo "### 5. an image with no application in it"
if must mkdir -p "${LAB}/hollow/src" \
  && must touch "${LAB}/hollow/src/README" \
  && must image_from "${LAB}/hollow/src" "${LAB}/hollow/dmg/Mnema_0.0.0_aarch64.dmg"; then
  expect_red "no Mnema.app inside" "${REPO}/scripts/verify-bundle.sh" "${LAB}/hollow"
fi

echo "### 6. an application with no executable"
if must copy_app_out "${LAB}/gutted/src" \
  && must rm -f "${LAB}/gutted/src/Mnema.app/Contents/MacOS/mnema-desktop" \
  && gone "${LAB}/gutted/src/Mnema.app/Contents/MacOS/mnema-desktop" \
  && must image_from "${LAB}/gutted/src" "${LAB}/gutted/dmg/Mnema_0.0.0_aarch64.dmg"; then
  expect_red "Contents/MacOS is empty" "${REPO}/scripts/verify-bundle.sh" "${LAB}/gutted"
fi

echo "### 7. a file added after signing"
if must copy_app_out "${LAB}/tampered/src" \
  && must touch "${LAB}/tampered/src/Mnema.app/Contents/Resources/extra.txt" \
  && there "${LAB}/tampered/src/Mnema.app/Contents/Resources/extra.txt" \
  && must image_from "${LAB}/tampered/src" "${LAB}/tampered/dmg/Mnema_0.0.0_aarch64.dmg"; then
  expect_red "the seal no longer covers the contents" "${REPO}/scripts/verify-bundle.sh" "${LAB}/tampered"
fi

echo "### 8. no seal at all — the state this repository's first build produced"
if must copy_app_out "${LAB}/unsealed/src" \
  && must rm -rf "${LAB}/unsealed/src/Mnema.app/Contents/_CodeSignature" \
  && gone "${LAB}/unsealed/src/Mnema.app/Contents/_CodeSignature" \
  && must image_from "${LAB}/unsealed/src" "${LAB}/unsealed/dmg/Mnema_0.0.0_aarch64.dmg"; then
  expect_red "signingIdentity dropped from tauri.conf.json" "${REPO}/scripts/verify-bundle.sh" "${LAB}/unsealed"
fi

echo "### 11. the bundle carries no extraction worker"
if must copy_app_out "${LAB}/no-worker" \
  && must rm -f "${LAB}/no-worker/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && gone "${LAB}/no-worker/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must image_from "${LAB}/no-worker" "${LAB}/no-worker-img/dmg/Mnema.dmg"; then
  expect_red -m "carries no mnema-extract-worker" \
    "a packaged build with no worker indexes nothing" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/no-worker-img"
fi

echo "### 12. the worker is there and is not executable"
if must copy_app_out "${LAB}/dead-worker" \
  && must chmod a-x "${LAB}/dead-worker/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must image_from "${LAB}/dead-worker" "${LAB}/dead-worker-img/dmg/Mnema.dmg"; then
  expect_red -m "exists and is not executable" \
    "a worker that cannot be executed" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/dead-worker-img"
fi

echo "### 13. the staged worker is not the one this build produced"
# copy_repo carries tracked files only, so target/ — git-ignored — does not come
# along; the built binary is copied in here by hand so the check reaches the
# staged-versus-built comparison instead of reddening on "no built_worker" first,
# which is a different control's reason, not this one's.
if must copy_repo "${LAB}/stale" \
  && must mkdir -p "${LAB}/stale/target/release" \
  && must cp "${REPO}/target/release/mnema-extract-worker" \
       "${LAB}/stale/target/release/mnema-extract-worker" \
  && must mkdir -p "${LAB}/stale/src-tauri/binaries" \
  && must cp "${REPO}/target/release/mnema-extract-worker" \
       "${LAB}/stale/src-tauri/binaries/mnema-extract-worker-stale" \
  && must printf 'not the binary you built\n' \
       >> "${LAB}/stale/src-tauri/binaries/mnema-extract-worker-stale"; then
  expect_red -m "the staged sidecar is not the binary" \
    "a stale sidecar must not ship inside a green build" \
    "${LAB}/stale/scripts/verify-bundle.sh" "${BUNDLE}"
fi

echo "### 13b. two staged sidecars for the same triple"
# Mutates the real src-tauri/binaries/, not a copy: unlike controls 9, 10 and 13
# there is no relocated script here to point at a fake directory instead —
# ${repo_root}/src-tauri/binaries always resolves to this one real directory
# when verify-bundle.sh runs unmutated, and bundle_dir is the only path the
# script takes as an argument. Modeled on control 3 one level down: two
# candidate files where the glob used to let `head -1` pick whichever sorted
# first, and the same "proves nothing about the new build" reason already
# written for ${dmg_dir} above. This directory is git-ignored and nothing else
# prunes it, so a sibling left behind here would poison every later run of
# this suite and of verify-bundle.sh itself — cleaned up unconditionally below,
# and `check_one_staged` runs again afterward to prove the cleanup worked
# rather than assume it.
staging_dir="${REPO}/src-tauri/binaries"
extra_staged="${staging_dir}/mnema-extract-worker-second-triple"
check_one_staged() {
  local n
  n="$(find "${staging_dir}" -maxdepth 1 -type f -name 'mnema-extract-worker-*' | wc -l | tr -d ' ')"
  [ "$n" = "1" ] && return 0
  printf '   %s has %s files matching mnema-extract-worker-*, expected exactly 1\n' "${staging_dir}" "$n"
  return 1
}
if must check_one_staged \
  && must cp "${REPO}/target/release/mnema-extract-worker" "${extra_staged}" \
  && there "${extra_staged}"; then
  expect_red -m "files matching mnema-extract-worker-* in" \
    "two staged sidecars, and the check must not pick whichever sorted first" \
    "${REPO}/scripts/verify-bundle.sh" "${BUNDLE}"
fi
must rm -f "${extra_staged}"
gone "${extra_staged}"
must check_one_staged

echo "### 9. the shell depends on Pdfium and the bundle carries none"
if must copy_repo "${LAB}/needs-pdfium" \
  && must perl -0pi -e 's{\[dependencies\]\n}{[dependencies]\nmnema-extract = { path = "../crates/mnema-extract" }\n}' \
    "${LAB}/needs-pdfium/src-tauri/Cargo.toml" \
  && must grep -q 'mnema-extract' "${LAB}/needs-pdfium/src-tauri/Cargo.toml" \
  && must stage_fresh_worker "${LAB}/needs-pdfium"; then
  expect_red -m "depends on pdfium-render" \
    "extraction wired into the shell, library not packaged" \
    "${LAB}/needs-pdfium/scripts/verify-bundle.sh" "${BUNDLE}"
fi

echo "### 10. cargo tree cannot answer — which is not the same as answering no"
if must copy_repo "${LAB}/broken-manifest" \
  && must cp "${REPO}/scripts/verify-bundle.sh" "${LAB}/broken-manifest/scripts/verify-bundle.sh" \
  && must stage_fresh_worker "${LAB}/broken-manifest" \
  && printf '[workspace]\nresolver = "3"\nmembers = ["crates/nope"]\n' \
    > "${LAB}/broken-manifest/Cargo.toml"; then
  expect_red -m "cargo tree failed" \
    "an unanswered dependency question must not read as absent" \
    "${LAB}/broken-manifest/scripts/verify-bundle.sh" "${BUNDLE}"
fi

echo
echo "### and the real bundle, which must pass"
if bash "${REPO}/scripts/verify-bundle.sh" >/dev/null 2>&1; then
  echo "-- the shipped image"
  echo "   green, as it must be"
else
  echo "-- the shipped image"
  echo "   *** RED — the check rejects the bundle it is supposed to accept ***"
  green=$((green + 1))
fi

echo
echo "red: ${red}   still green: ${green}   broken controls: ${broken}"
[ "${green}" -eq 0 ] && [ "${broken}" -eq 0 ]

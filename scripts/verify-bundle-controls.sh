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
# The third clause is control 13b's, not $LAB's: that control stages a second
# file directly in the real, git-ignored src-tauri/binaries/ (there is no copy
# to mutate instead — see the comment at its call site), and a signal between
# the `cp` and its own `rm -f` would otherwise leave the file behind, where
# nothing prunes it and every later run of this suite and of verify-bundle.sh
# itself reddens on "files matching … in …" for a reason that has nothing to do
# with what it is actually testing. The path is a constant, so it costs nothing
# on the normal exit, where 13b has already removed it and this `rm -f` is a
# no-op.
trap 'chmod -R u+w "$LAB" 2>/dev/null; rm -rf "$LAB"; rm -f "${REPO}/src-tauri/binaries/mnema-extract-worker-second-triple"' EXIT

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
# it claims. Two controls that asked the dependency graph about Pdfium once
# forced this — they had two possible red exits between them and were
# indistinguishable without it. Both are gone now, replaced by the
# worker-verdict controls below, but the rule they forced stayed: any two
# controls can still collide the same way.
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
    #
    # That "line" is `grep 'verify-bundle:' | tail -1`, and only a `fail` message's
    # first physical line carries that prefix — `fail`'s own messages are
    # multi-line, and continuation lines do not. So a fragment must sit on the
    # first line of the `fail` it names. Re-wrapping that first line — an edit
    # that looks purely cosmetic — pushes the fragment onto a continuation `full`
    # never sees, and turns a correct control into a WRONG REASON accusation.
    # Whoever reflows a `fail` message should check what fragment, if any, points
    # at its first line before doing it.
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

# A stand-in worker for control 14c: reads and discards one request line, then
# answers with a header frame and nothing else — no Page, no Block, no Summary.
# `/usr/bin/true` (control 14b) already reaches the header grep at
# verify-bundle.sh:203 by answering nothing at all; this reaches the block grep
# at :206 instead, which needs a worker that answers something.
write_header_only_worker() {
  local dest="$1"
  cat > "$dest" <<'SCRIPT' || return 1
#!/bin/sh
IFS= read -r _request
printf '{"frame":"header","sha256":"fake-header-only-worker","mime":"text/plain","source_kind":"document","pages":1}\n'
SCRIPT
  chmod +x "$dest"
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
# Mutates the real src-tauri/binaries/, not a copy — a deliberate trade, not
# an absence of a lever. Control 13 gets to stage into a fake
# src-tauri/binaries/ because it runs a relocated copy of verify-bundle.sh,
# whose own repo_root resolves to that copy (verify-bundle.sh:25); the same
# lever was available here too, and nothing stops a relocated copy from
# staging two files instead of one. It goes unused because reaching that
# count check through a copy means repeating control 13's own prelude first
# — a target/release holding the built worker, so the check reaches the
# staging directory instead of reddening on "no built_worker" — for a
# control whose entire subject is the staging directory, not the freshness
# comparison control 13 already tests. Mutating the one real directory is
# cheaper than restaging that prelude, not required by any limit of the
# mechanism. Modeled on control 3 one level
# down: two candidate files where the glob used to let `head -1` pick
# whichever sorted first, and the same "proves nothing about the new build"
# reason already written for ${dmg_dir} above. This directory is git-ignored
# and nothing else prunes it, so a sibling left behind here would poison
# every later run of this suite and of verify-bundle.sh itself — which is
# why the EXIT trap above removes it unconditionally on every exit path, and
# `check_one_staged` runs again afterward to prove the cleanup worked rather
# than assume it.
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

echo "### 14. the worker cannot answer — which is not the same as answering no"
# Measured: swapping the worker for /usr/bin/false leaves the outer seal covering
# bytes that are no longer there, and codesign calls that "nested code is modified
# or invalid" — the same reddening control 7 exists for. With the check below not
# yet written, that is the only thing left to redden on, so without this re-sign
# the control can never be seen still-green: it proves control 7 twice and itself
# not at all. Once the check exists it runs first and this step no longer decides
# the colour — it decides whether the red-first observation can be repeated.
# It is also the state a real build produces: the bundler re-signs after staging
# the sidecar (see the freshness comment in verify-bundle.sh), so a bundle whose
# worker cannot answer is properly sealed. That single defect is what is under test.
#
# `--deep` here SIGNS, which verify-bundle.sh itself calls deprecated — for
# *signing*. That deprecation is about a shipped, notarized bundle; resealing a
# lab fixture that is neither is exactly the case it does not cover.
if must copy_app_out "${LAB}/mute-worker" \
  && must cp /usr/bin/false \
       "${LAB}/mute-worker/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must chmod +x "${LAB}/mute-worker/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must codesign --sign - --force --deep "${LAB}/mute-worker/Mnema.app" \
  && must image_from "${LAB}/mute-worker" "${LAB}/mute-worker-img/dmg/Mnema.dmg"; then
  expect_red -m "the bundled worker exited" \
    "a worker that exits non-zero and says nothing" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/mute-worker-img"
fi

echo "### 14b. the worker exits clean and says nothing"
# Same re-sign, same reason as control 14: /usr/bin/true's bytes differ from the
# staged worker's, so the seal needs re-cutting before this control's own
# precondition — still green, with the check below absent — is observable.
# Reaches verify-bundle.sh:203's header grep rather than :200's exit check,
# which is the assertion neither this suite nor a prior run had ever reddened.
if must copy_app_out "${LAB}/silent-worker" \
  && must cp /usr/bin/true \
       "${LAB}/silent-worker/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must chmod +x "${LAB}/silent-worker/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must codesign --sign - --force --deep "${LAB}/silent-worker/Mnema.app" \
  && must image_from "${LAB}/silent-worker" "${LAB}/silent-worker-img/dmg/Mnema.dmg"; then
  expect_red -m "returned no header frame" \
    "a worker that exits 0 and answers nothing — UNANSWERED, not the same reason as 14" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/silent-worker-img"
fi

echo "### 14c. the worker answers a header and no block"
# Same re-sign, same reason again. write_header_only_worker's script is not a
# Mach-O binary, so codesign --deep treats it as a plain resource, hashes it,
# and reseals around it — measured before writing this control, not assumed.
# Reaches verify-bundle.sh:206's block grep, the one assertion in this task's
# headline claim that neither 14 nor 14b exercises.
if must copy_app_out "${LAB}/header-only-worker" \
  && must write_header_only_worker \
       "${LAB}/header-only-worker/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must codesign --sign - --force --deep "${LAB}/header-only-worker/Mnema.app" \
  && must image_from "${LAB}/header-only-worker" "${LAB}/header-only-worker-img/dmg/Mnema.dmg"; then
  expect_red -m "returned no block frame" \
    "a worker that answers a header and never a block" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/header-only-worker-img"
fi

echo "### 15. the worker refuses PDFs and the bundle carries Pdfium anyway"
# Same re-sign, same reason as control 14: adding a file under Resources after
# copy_app_out leaves the outer seal covering bytes that are no longer the whole
# story, and codesign calls that "modified or invalid" — control 7's reason, not
# this control's. With the Pdfium-verdict check below not yet written, that seal
# failure is the only thing left to redden on, so without the re-sign this
# control can never be seen still-green: it proves control 7 twice and itself
# not at all.
if must copy_app_out "${LAB}/dead-weight" \
  && must mkdir -p "${LAB}/dead-weight/Mnema.app/Contents/Resources/pdfium/lib" \
  && must cp "${REPO}/vendor/pdfium/lib/libpdfium.dylib" \
       "${LAB}/dead-weight/Mnema.app/Contents/Resources/pdfium/lib/" \
  && there "${LAB}/dead-weight/Mnema.app/Contents/Resources/pdfium/lib/libpdfium.dylib" \
  && must codesign --sign - --force --deep "${LAB}/dead-weight/Mnema.app" \
  && must image_from "${LAB}/dead-weight" "${LAB}/dead-weight-img/dmg/Mnema.dmg"; then
  expect_red -m "refuses PDFs as unsupported" \
    "7.7 MB nothing in the bundle can load" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/dead-weight-img"
fi

echo "### 16. the worker answers a PDF with something else entirely"
# Same re-sign, same reason again: pdf-says-nothing.sh's bytes are not the staged
# worker's, so the seal needs re-cutting before this control's own precondition is
# observable rather than control 7's.
if must copy_app_out "${LAB}/odd-verdict" \
  && must cp "${REPO}/scripts/mutations/pdf-says-nothing.sh" \
       "${LAB}/odd-verdict/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must chmod +x "${LAB}/odd-verdict/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must codesign --sign - --force --deep "${LAB}/odd-verdict/Mnema.app" \
  && must image_from "${LAB}/odd-verdict" "${LAB}/odd-verdict-img/dmg/Mnema.dmg"; then
  expect_red -m "neither blocks nor rule=unsupported" \
    "an unrecognised verdict must not read as 'no reader'" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/odd-verdict-img"
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

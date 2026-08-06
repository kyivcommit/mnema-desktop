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
# The consequence of that, written down because nothing automatic will ever say it:
# while nothing runs this file, nothing checks that verify-bundle.sh still rejects
# anything. CI builds an image and runs that script, and a check that has stopped
# looking passes exactly like a check that looked and found nothing wrong. Delete a
# branch, leave an assertion any failure satisfies, make a grep unreachable — the run
# is green, and stays green. What that leaves unprotected is not this suite; it is
# verify-bundle.sh, and with it the only claim anyone has that a shipped bundle
# carries a worker which runs. The single thing standing between that claim and a
# silent no-op is a person remembering to type the line below.
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

# Four ways a control can end and one way the suite itself can, each counted apart so
# the printed tally names what it counted. They were two before, and "still green"
# covered a control that reddened on somebody else's message and a real image the check
# rejected — neither of which prints those words anywhere in its own output.
red=0      # red, and the message carried the fragment the control named
wrong=0    # red on something else: proves nothing, and says so
green=0    # the check accepted a state it is supposed to reject
broken=0   # the control never got to run: setup failed, or it asserts nothing
rejected=0 # the shipped image failed the check that must accept it

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
# to read the message and judge. Every control below now carries one. Without it a
# control counts as red on exit status alone, and that is satisfied by a misspelled
# path, a syntax error, or a script that dies before reaching the state under test:
# eight controls stood on nothing else for a review cycle, the two signature ones
# among them. The gap is not theoretical — control 11 went red on the codesign
# section's message before its own check existed, and nothing before this could tell
# the difference between that and the check it names actually firing.
expect_red() {
  local expect="" asserted=0
  if [ "$1" = "-m" ]; then
    expect="$2"
    asserted=1
    shift 2
  fi
  local label="$1" script="$2"
  shift 2
  # An empty fragment is not a weaker assertion, it is none: it matches every message,
  # so the control counts red whatever it reddened on — the exact state `-m` exists to
  # end, wearing the marks of having been fixed. Refused before the check is even run,
  # and counted BROKEN rather than passed, because a control that asserts nothing while
  # claiming to is worse than one that never claimed.
  if [ "${asserted}" -eq 1 ] && [ -z "${expect}" ]; then
    printf '%s\n' "-- ${label}"
    printf '   BROKEN CONTROL: -m was given an empty fragment, which asserts nothing\n'
    broken=$((broken + 1))
    return 1
  fi
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
    if [ "${asserted}" -eq 1 ] && ! printf '%s' "${full}" | grep -qF -- "${expect}"; then
      printf '   red (exit %s), but not for its own reason: %s\n' "$status" "$msg"
      printf '   *** WRONG REASON — this control proves nothing. Expected: %s ***\n' "$expect"
      wrong=$((wrong + 1))
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
#
# ⚠️ What the re-signs below this line do to a copy, measured rather than assumed,
# because it inverts a conclusion anyone will reach by trying it here:
#
#   codesign --sign - --force --deep  →  flags 0x10002(adhoc,runtime) become 0x2(adhoc)
#                                        and the entitlements are gone.
#
# The bundler signs with the hardened runtime and with
# `com.apple.security.cs.disable-library-validation`; a `--deep` re-sign keeps neither.
# So in a re-signed lab copy the worker loads Pdfium happily — and it loads it because
# library validation is no longer being enforced at all, not because the entitlement is
# unnecessary. Take that run as evidence about the shipped product and you will delete
# the entitlement and ship a build that reads no PDFs on any machine. The configuration a
# user actually runs is reproduced by exactly two things here: the shipped-image step at
# the bottom, and control 17.
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
printf '{"frame":"header","sha256":"fake-header-only-worker","mime":"text/plain","source_kind":"document","reader":"text","reader_version":1,"pages":1}\n'
SCRIPT
  chmod +x "$dest"
}

# The first two fragments name the path as well as the wording, because these two
# messages differ by little else and "not found" is precisely the accident `-m` is here
# to catch: a control pointed at a path the script never looked at would otherwise be
# indistinguishable from one that reddened where it meant to.
echo "### 1. the dmg directory does not exist"
expect_red -m "no ${LAB}/absent/dmg" \
  "no bundle at all" "${REPO}/scripts/verify-bundle.sh" "${LAB}/absent"

echo "### 2. the dmg directory is empty"
if must mkdir -p "${LAB}/empty/dmg"; then
  expect_red -m "no .dmg in ${LAB}/empty/dmg" \
    "built nothing" "${REPO}/scripts/verify-bundle.sh" "${LAB}/empty"
fi

echo "### 3. two images side by side"
if must mkdir -p "${LAB}/two/dmg" \
  && must cp "${GOOD}" "${LAB}/two/dmg/a.dmg" \
  && must cp "${GOOD}" "${LAB}/two/dmg/b.dmg"; then
  # The count is part of the fragment: "some .dmg files in" would be satisfied by a
  # check that found one image and miscounted, which is the failure this rejects.
  expect_red -m "2 .dmg files in ${LAB}/two/dmg" \
    "one of them is from an older build" "${REPO}/scripts/verify-bundle.sh" "${LAB}/two"
fi

echo "### 4. an image that will not attach"
if must mkdir -p "${LAB}/corrupt/dmg" \
  && must dd if=/dev/urandom of="${LAB}/corrupt/dmg/Mnema_0.0.0_aarch64.dmg" bs=1024 count=200 status=none \
  && there "${LAB}/corrupt/dmg/Mnema_0.0.0_aarch64.dmg"; then
  expect_red -m "could not be attached" \
    "the file is named .dmg and is not one" "${REPO}/scripts/verify-bundle.sh" "${LAB}/corrupt"
fi

echo "### 5. an image with no application in it"
if must mkdir -p "${LAB}/hollow/src" \
  && must touch "${LAB}/hollow/src/README" \
  && must image_from "${LAB}/hollow/src" "${LAB}/hollow/dmg/Mnema_0.0.0_aarch64.dmg"; then
  expect_red -m "contains no Mnema.app" \
    "no Mnema.app inside" "${REPO}/scripts/verify-bundle.sh" "${LAB}/hollow"
fi

echo "### 6. an application with no executable"
if must copy_app_out "${LAB}/gutted/src" \
  && must rm -f "${LAB}/gutted/src/Mnema.app/Contents/MacOS/mnema-desktop" \
  && gone "${LAB}/gutted/src/Mnema.app/Contents/MacOS/mnema-desktop" \
  && must image_from "${LAB}/gutted/src" "${LAB}/gutted/dmg/Mnema_0.0.0_aarch64.dmg"; then
  expect_red -m "has no executable at Contents/MacOS/mnema-desktop" \
    "Contents/MacOS is empty" "${REPO}/scripts/verify-bundle.sh" "${LAB}/gutted"
fi

# Controls 7 and 8 share a fragment, and that is the honest reading rather than a
# copied line: they produce two different states — a file added under a seal that still
# exists, and no seal at all — of one branch, and verify-bundle.sh answers both with the
# same message. What tells them apart is codesign's own output, which is not prefixed
# `verify-bundle:` and so never reaches the line `expect_red` matches. The fragment
# proves what it can: each of these reddens on the signature check rather than on a
# missing file or a dead script, which is what neither of them proved before.
echo "### 7. a file added after signing"
if must copy_app_out "${LAB}/tampered/src" \
  && must touch "${LAB}/tampered/src/Mnema.app/Contents/Resources/extra.txt" \
  && there "${LAB}/tampered/src/Mnema.app/Contents/Resources/extra.txt" \
  && must image_from "${LAB}/tampered/src" "${LAB}/tampered/dmg/Mnema_0.0.0_aarch64.dmg"; then
  expect_red -m "the signature of Mnema.app inside the image does not verify" \
    "the seal no longer covers the contents" "${REPO}/scripts/verify-bundle.sh" "${LAB}/tampered"
fi

echo "### 8. no seal at all — the state this repository's first build produced"
if must copy_app_out "${LAB}/unsealed/src" \
  && must rm -rf "${LAB}/unsealed/src/Mnema.app/Contents/_CodeSignature" \
  && gone "${LAB}/unsealed/src/Mnema.app/Contents/_CodeSignature" \
  && must image_from "${LAB}/unsealed/src" "${LAB}/unsealed/dmg/Mnema_0.0.0_aarch64.dmg"; then
  expect_red -m "the signature of Mnema.app inside the image does not verify" \
    "signingIdentity dropped from tauri.conf.json" "${REPO}/scripts/verify-bundle.sh" "${LAB}/unsealed"
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
# staged-versus-built comparison instead of reddening on "no built_worker" first.
# That branch belongs to control 16b below, which is the mirror of this prelude:
# the same copied repository with the same file deliberately left out. Until 16b
# was written these lines said "a different control's reason" and no such control
# existed — a comment asserting coverage that was nowhere in the file.
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
  # "on a plain text file" and the status, because control 16c reddens on a message
  # identical up to those words. /usr/bin/false is what exits 1 here, so pinning the
  # number costs nothing and pins which fixture the check was asking about.
  expect_red -m "exited 1 on a plain text file" \
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
# Same re-sign, same reason as control 14: the stand-in's bytes are not the staged
# worker's, so the seal has to be re-cut or this reddens on control 7's message.
#
# It needs a stand-in now, and that is the finding rather than a convenience. Until
# the format-readers cycle, the real worker produced this state by itself — it refused
# every PDF — and `Contents/Resources/pdfium/lib/libpdfium.dylib` was a file this
# control created. Both halves inverted at once: the worker reads PDFs, and
# `bundle.resources` puts the library at exactly that path in every build. Left as it
# was, the `cp` would have overwritten the real library with an identical copy, the
# control would have stopped producing the state it names, and `expect_red` would have
# counted it STILL GREEN. Loudly, to be exact — the suite prints that and adds it to the
# tally, which is the one reason it would not have shipped unnoticed. What was lost was
# the control, not the reporting.
if must copy_app_out "${LAB}/dead-weight" \
  && must cp "${REPO}/scripts/mutations/pdf-refuses-unsupported.sh" \
       "${LAB}/dead-weight/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must chmod +x "${LAB}/dead-weight/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && there "${LAB}/dead-weight/Mnema.app/Contents/Resources/pdfium/lib/libpdfium.dylib" \
  && must codesign --sign - --force --deep "${LAB}/dead-weight/Mnema.app" \
  && must image_from "${LAB}/dead-weight" "${LAB}/dead-weight-img/dmg/Mnema.dmg"; then
  expect_red -m "refuses PDFs as unsupported" \
    "7.7 MB nothing in the bundle can load" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/dead-weight-img"
fi

echo "### 15b. two copies of the library, and no telling which one ran"
# The dead-weight defect in the shape this repository can now actually produce: the
# library is packaged unconditionally, so a second copy left over from another
# placement — `Contents/Frameworks` was measured as a candidate and rejected — makes
# 7.7 MB of the image dead without making the bundle fail at anything.
#
# The real worker, not a stand-in, because the question is what the check does with a
# real reading bundle. Sorting cannot answer which copy loaded, and the check is not
# allowed to guess: control 16f is what happens when it picks the wrong one.
if must copy_app_out "${LAB}/two-libs" \
  && must mkdir -p "${LAB}/two-libs/Mnema.app/Contents/Frameworks" \
  && must cp "${REPO}/vendor/pdfium/lib/libpdfium.dylib" \
       "${LAB}/two-libs/Mnema.app/Contents/Frameworks/" \
  && there "${LAB}/two-libs/Mnema.app/Contents/Frameworks/libpdfium.dylib" \
  && must codesign --sign - --force --deep "${LAB}/two-libs/Mnema.app" \
  && must image_from "${LAB}/two-libs" "${LAB}/two-libs-img/dmg/Mnema.dmg"; then
  expect_red -m "2 copies of libpdfium*.dylib" \
    "one of them is dead weight and sorting does not say which" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/two-libs-img"
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

# The three below cover the branches that answer "the check could not find out" rather
# than "the answer is no". That distinction is the reason those branches were written at
# all, and not one of them had ever been run: a branch nobody has seen fire is a claim,
# not a check. They are numbered after 16 so that nothing above them renumbers, not
# because they belong here — 16b's subject is the freshness section, far above.
#
# None of the three can be seen still green first, and that is a property of these
# controls as built rather than a skipped step.
# In an ordinary run all three redden on their own fragment, exactly like every control
# beside them. WRONG REASON is what they produce only in the experiment described next,
# which deliberately disables the branch under test; it is not the state of the suite.
# The still-green step works while disabling the branch under test leaves the state with
# nothing to redden on. Disable any of these and the state falls into a neighbouring
# branch that reddens anyway, so the observation degrades to WRONG REASON with the
# expected fragment printed: red, and provably not on the message the control names.
# That is the strongest evidence these three admit, not a shortcut past a stricter one.
# Control 16e below did get a real still green, and why it could is the test for whether
# a future control can have one: its branch did not exist until the commit that added
# it, so there was nothing downstream for the state to fall into.
# For 16b and 16d the fall-through is the branch layout and cannot be arranged away; for
# 16c it is this stand-in — one printing an unsupported-shaped frame before exiting
# non-zero would reach the elif and leave the check green. That control is not written.

echo "### 16b. nothing built to compare the bundled worker against"
# The mirror of control 13's prelude. That control copies the built worker into the
# copied repository so the check gets past this branch; this one leaves it out, which
# takes no arranging at all: target/ is git-ignored, so a copy of the tracked files is
# already in that state, and so is a fresh clone next to a downloaded image. The check
# must call freshness UNANSWERED there. Answering "fresh" because nothing contradicted
# it is the failure this whole section is shaped against.
if must copy_repo "${LAB}/no-built"; then
  expect_red -m "so the worker's freshness is UNANSWERED" \
    "no built binary, so freshness is unanswered rather than answered yes" \
    "${LAB}/no-built/scripts/verify-bundle.sh" "${BUNDLE}"
fi

echo "### 16c. the worker cannot answer for a PDF"
# Same re-sign, same reason as control 14: the stand-in's bytes are not the staged
# worker's, so the seal has to be re-cut or this control reddens on control 7's message
# instead of its own. Reaches the PDF exit-status check, whose message is identical to
# control 14's up to the words naming the fixture — which is why both fragments name it.
if must copy_app_out "${LAB}/pdf-mute" \
  && must cp "${REPO}/scripts/mutations/pdf-exits-nonzero.sh" \
       "${LAB}/pdf-mute/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must chmod +x "${LAB}/pdf-mute/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must codesign --sign - --force --deep "${LAB}/pdf-mute/Mnema.app" \
  && must image_from "${LAB}/pdf-mute" "${LAB}/pdf-mute-img/dmg/Mnema.dmg"; then
  expect_red -m "exited 1 on a PDF" \
    "a worker that reads text and then fails on the PDF — UNANSWERED, not 'no reader'" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/pdf-mute-img"
fi

echo "### 16d. the worker reads PDFs and its probe says it cannot load Pdfium"
# This control used to be the whole of the PDF-reader case: the branch was a `fail`
# saying "whoever lands the reader closes this", and any worker answering blocks
# reddened it. A reader landed, so the branch now asks the worker where its library
# came from, and the four controls here cover the four ways that can go wrong. This is
# the first: a worker that reads PDFs over the wire and then cannot say `loaded:true`.
# Two answers from one process disagreeing is not a state to pick a side in.
# Same re-sign, same reason as control 14.
if must copy_app_out "${LAB}/pdf-reader" \
  && must cp "${REPO}/scripts/mutations/pdf-reads-blocks.sh" \
       "${LAB}/pdf-reader/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must chmod +x "${LAB}/pdf-reader/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must codesign --sign - --force --deep "${LAB}/pdf-reader/Mnema.app" \
  && must image_from "${LAB}/pdf-reader" "${LAB}/pdf-reader-img/dmg/Mnema.dmg"; then
  expect_red -m "then said it cannot load Pdfium" \
    "a worker that answers a PDF with blocks and cannot run its own probe" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/pdf-reader-img"
fi

echo "### 16f. the worker reads PDFs with a library that is not in the image"
# The defect this whole branch was written for, and the one a check that stopped at
# "it reads PDFs" passes: the bundled worker loaded the developer's vendor/ tree. It is
# not hypothetical — the first bundle built after the reader landed did exactly that
# (task 16), and the only thing that made it visible was a code-signing refusal, which
# the entitlement now removes. Same re-sign, same reason as control 14.
if must copy_app_out "${LAB}/outside-lib" \
  && must cp "${REPO}/scripts/mutations/pdf-loads-from-outside.sh" \
       "${LAB}/outside-lib/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must chmod +x "${LAB}/outside-lib/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && there "${LAB}/outside-lib/Mnema.app/Contents/Resources/pdfium/lib/libpdfium.dylib" \
  && must codesign --sign - --force --deep "${LAB}/outside-lib/Mnema.app" \
  && must image_from "${LAB}/outside-lib" "${LAB}/outside-lib-img/dmg/Mnema.dmg"; then
  # The library IS in this image — `there` above proves it — and that is the point:
  # a check asserting only "a libpdfium is in the bundle" calls this bundle good.
  expect_red -m "loaded Pdfium from OUTSIDE this image" \
    "the library is in the image and is not the one that ran" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/outside-lib-img"
fi

echo "### 16g. the worker reads PDFs and will not say where its library came from"
# What deleting the `library_dir` field from --probe-pdfium turns every bundle into.
# The answer is then absent rather than wrong, and an absent answer must not be read as
# the good one — the distinction the UNANSWERED branches above exist for, arriving here
# through a field instead of an exit status. Same re-sign, same reason as control 14.
if must copy_app_out "${LAB}/mute-probe" \
  && must cp "${REPO}/scripts/mutations/pdf-hides-its-library.sh" \
       "${LAB}/mute-probe/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must chmod +x "${LAB}/mute-probe/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must codesign --sign - --force --deep "${LAB}/mute-probe/Mnema.app" \
  && must image_from "${LAB}/mute-probe" "${LAB}/mute-probe-img/dmg/Mnema.dmg"; then
  expect_red -m "named no library_dir" \
    "loaded:true with no directory is UNANSWERED, not 'from inside the bundle'" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/mute-probe-img"
fi

echo "### 16h. the worker reads a PDF and then its probe exits non-zero"
# The last of the four, and the one with the strongest pull towards being waved through:
# the wire has already answered with blocks, so a failed probe looks like a formality
# that did not matter. It is the difference between reporting on the run that happened
# and the run one wanted. Exits 3, so the status in the message is a number the stand-in
# put there rather than the 1 that any failure produces. Same re-sign, same reason as 14.
if must copy_app_out "${LAB}/probe-dead" \
  && must cp "${REPO}/scripts/mutations/pdf-probe-fails.sh" \
       "${LAB}/probe-dead/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must chmod +x "${LAB}/probe-dead/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must codesign --sign - --force --deep "${LAB}/probe-dead/Mnema.app" \
  && must image_from "${LAB}/probe-dead" "${LAB}/probe-dead-img/dmg/Mnema.dmg"; then
  expect_red -m "exited 3 on --probe-pdfium" \
    "the two answers disagree, and neither of them is the one to keep" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/probe-dead-img"
fi

echo "### 16i. the worker reads PDFs and nothing packaged the library"
# Not an invented state: it is the bundle this repository built on the day the reader
# landed and before `bundle.resources` existed. It read PDFs perfectly — out of the
# developer's checkout — and every other check in verify-bundle.sh passed on it. Delete
# the `resources` block from tauri.conf.json and this is what comes back.
#
# The stand-in is the one that reads everything, because the real worker in this state
# cannot be made to answer blocks: with no library in the image it falls through to the
# compile-time vendored path, which exists on the machine running this suite and would
# make the control pass for the machine's reason rather than the bundle's.
# Same re-sign, same reason as control 14.
if must copy_app_out "${LAB}/no-lib" \
  && must cp "${REPO}/scripts/mutations/pdf-reads-blocks.sh" \
       "${LAB}/no-lib/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must chmod +x "${LAB}/no-lib/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must rm -f "${LAB}/no-lib/Mnema.app/Contents/Resources/pdfium/lib/libpdfium.dylib" \
  && gone "${LAB}/no-lib/Mnema.app/Contents/Resources/pdfium/lib/libpdfium.dylib" \
  && must codesign --sign - --force --deep "${LAB}/no-lib/Mnema.app" \
  && must image_from "${LAB}/no-lib" "${LAB}/no-lib-img/dmg/Mnema.dmg"; then
  expect_red -m "carries no libpdfium*.dylib" \
    "a reading bundle with no library in it reads from somewhere else" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/no-lib-img"
fi

echo "### 16e. the image is older than the worker this checkout built"
# Control 13's prelude turned to the opposite purpose. That control makes the staged
# file DIFFER from the built one; this one makes them the same bytes copied twice, so
# the sha comparison passes and the timestamp is the only thing left that can redden.
# `touch` is what separates those two facts: it moves mtime and leaves content alone,
# which is exactly the state the check names — a sidecar that is perfectly fresh inside
# an image written before it. The guard below asserts that state was produced, rather
# than trusting that `cp` gave the copies a current mtime.
if must copy_repo "${LAB}/stale-image" \
  && must mkdir -p "${LAB}/stale-image/target/release" \
  && must cp "${REPO}/target/release/mnema-extract-worker" \
       "${LAB}/stale-image/target/release/mnema-extract-worker" \
  && must mkdir -p "${LAB}/stale-image/src-tauri/binaries" \
  && must cp "${REPO}/target/release/mnema-extract-worker" \
       "${LAB}/stale-image/src-tauri/binaries/mnema-extract-worker-aarch64-apple-darwin" \
  && must touch "${LAB}/stale-image/target/release/mnema-extract-worker" \
  && must [ "${LAB}/stale-image/target/release/mnema-extract-worker" -nt "${GOOD}" ]; then
  expect_red -m "is older than the worker this checkout built" \
    "an image written before the worker it is supposed to contain" \
    "${LAB}/stale-image/scripts/verify-bundle.sh" "${BUNDLE}"
fi

echo "### 17. the shipped signing configuration, minus the entitlement"
# Controls 15 and 16 above prove that this suite's own re-signing does NOT reproduce the
# state a user runs — see the note above `copy_app_out`. This one does reproduce it, and
# it is the only control here whose subject is the signature rather than the check's
# logic: hardened runtime on, no entitlement, which is what `src-tauri/entitlements.plist`
# and the `entitlements` line in tauri.conf.json exist to prevent.
#
# The worker is signed on its own and the bundle is re-sealed WITHOUT `--deep`, so the
# worker keeps the signature this control gave it. Measured: the seal still verifies, so
# the run reaches the PDF verdict rather than reddening on control 7's message.
#
# It shares a fragment with control 16, deliberately and for the same reason controls 7
# and 8 share one: two different states, one message. The bundled worker cannot load its
# own library, so it answers `Frame::Failed` — which is neither blocks nor unsupported,
# and the branch that says so is the only one that can speak. What tells the two apart is
# the state each builds, not the line each matches. Without this control the entitlement
# is a line in a config file with nothing in the repository saying what it buys; the
# shipped-image step below fails if it is removed, but only this one names why.
if must copy_app_out "${LAB}/no-entitlement" \
  && must codesign --sign - --force --options runtime \
       "${LAB}/no-entitlement/Mnema.app/Contents/MacOS/mnema-extract-worker" \
  && must codesign --sign - --force "${LAB}/no-entitlement/Mnema.app" \
  && must image_from "${LAB}/no-entitlement" "${LAB}/no-entitlement-img/dmg/Mnema.dmg"; then
  expect_red -m "neither blocks nor rule=unsupported" \
    "hardened runtime with no disable-library-validation: the worker cannot load its own Pdfium" \
    "${REPO}/scripts/verify-bundle.sh" "${LAB}/no-entitlement-img"
fi

echo
echo "### and the real bundle, which must pass"
if bash "${REPO}/scripts/verify-bundle.sh" >/dev/null 2>&1; then
  echo "-- the shipped image"
  echo "   green, as it must be"
else
  echo "-- the shipped image"
  echo "   *** RED — the check rejects the bundle it is supposed to accept ***"
  rejected=$((rejected + 1))
fi

echo
printf 'red for its own reason: %s   WRONG REASON: %s   still green: %s\n' \
  "${red}" "${wrong}" "${green}"
printf 'broken controls: %s   shipped image rejected: %s\n' "${broken}" "${rejected}"
[ "${wrong}" -eq 0 ] && [ "${green}" -eq 0 ] \
  && [ "${broken}" -eq 0 ] && [ "${rejected}" -eq 0 ]

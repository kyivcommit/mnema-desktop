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

# --- the extraction worker ----------------------------------------------------
#
# The application is a shell: it walks a folder and hands every file to a worker
# process it expects to find beside itself (src-tauri/src/paths.rs:32-42). Before
# bundle.externalBin existed, a packaged build had no such file, started fine, and
# failed every walk on its first extract() call — reported as EndReason::Failed,
# with nothing in the bundle to explain it.
worker_name="mnema-extract-worker"
worker="${app}/Contents/MacOS/${worker_name}"

[ -f "${worker}" ] || fail "${product}.app carries no ${worker_name}.
  bundle.externalBin in src-tauri/tauri.conf.json is what puts it there, and
  scripts/stage-sidecar.sh is what builds it. A bundle without it indexes nothing."
[ -x "${worker}" ] || fail "${worker} exists and is not executable. The walk would
  fail on its first extract() call with a permission error, not a missing file."

# Freshness is two questions, and neither can be settled by comparing bytes against the
# image. First, answered here: is the file `externalBin` will copy the one cargo built?
# That much is a property of the repository — `externalBin` copies whatever sits in
# src-tauri/binaries/, so a stale file there ships inside a build that is green in every
# other respect. A missing built binary is an UNANSWERED question rather than the answer
# "fresh", the same distinction the dependency check learned the hard way. Second,
# answered after that comparison: was this image written after that binary existed?
#
# Both paths below come from repo_root, not from bundle_dir: pointing this script
# at a different image still reports on THIS checkout's staged and built files.
# Deliberate, not an oversight — it is what lets a control run this script from a
# mutated copy of the repository against the one real bundle and still get a
# meaningful verdict about that copy's own staged file.
built_worker="${repo_root}/target/release/${worker_name}"
# The word UNANSWERED sits on the first physical line of this message on purpose: that
# is the only line carrying the `verify-bundle:` prefix, and it is what control 16b
# asserts on. Reflowing it would move the fragment onto a line the control never sees.
[ -f "${built_worker}" ] \
  || fail "no ${built_worker}, so the worker's freshness is UNANSWERED. Whether the
  bundled worker is the one this build produced cannot be told from here, and that is
  not the same as answered yes. Run scripts/stage-sidecar.sh, or build with
  cargo tauri build, which calls it."

# The directory before its contents, same reason as ${dmg_dir} above: `find` on a
# path that does not exist fails, and under `set -euo pipefail` the failing
# pipeline inside the command substitution below would end the script with no
# output at all — silent, not merely unhelpful.
staged_dir="${repo_root}/src-tauri/binaries"
[ -d "${staged_dir}" ] || fail "no ${staged_dir}.
  scripts/stage-sidecar.sh creates it; beforeBuildCommand calls that script."

# Counted before it is read, same shape as ${dmg_count} above and for the same
# reason: scripts/stage-sidecar.sh overwrites its own file but never removes a
# sibling left by a different host triple, and this directory is git-ignored, so
# nothing else prunes one either. `head -1` on more than one match would verify
# whichever file sorted first — proving nothing about the one the bundler
# actually staged, and control 13 shows how loose the glob is: it matches
# `mnema-extract-worker-stale`, not just a real triple.
staged_count="$(find "${staged_dir}" -maxdepth 1 -type f \
  -name "${worker_name}-*" | wc -l | tr -d ' ')"
case "${staged_count}" in
  0) fail "nothing staged in ${staged_dir}.
  scripts/stage-sidecar.sh puts it there and beforeBuildCommand calls that script." ;;
  1) : ;;
  *) fail "${staged_count} files matching ${worker_name}-* in ${staged_dir}.
  Remove the stale ones: a check that verifies whichever one sorted first proves
  nothing about the new build, the same reason ${dmg_dir} rejects two images." ;;
esac
staged_worker="$(find "${staged_dir}" -maxdepth 1 -type f -name "${worker_name}-*")"

staged_sha="$(shasum -a 256 "${staged_worker}" | cut -d' ' -f1)"
built_sha="$(shasum -a 256 "${built_worker}" | cut -d' ' -f1)"
[ "${staged_sha}" = "${built_sha}" ] || fail "the staged sidecar is not the binary this
  build produced:
    staged ${staged_worker} ${staged_sha}
    built  ${built_worker} ${built_sha}
  A stale copy in src-tauri/binaries/ ships inside an otherwise green build. That
  directory is git-ignored and scripts/stage-sidecar.sh overwrites it every time."

# What that comparison establishes, and what it does not. Measured in task 1: the
# bundler re-signs the sidecar in place, so the bundled bytes are never equal to the
# built bytes even on a perfectly fresh build (see docs/BUILD.md, "The extraction
# worker inside the bundle"), and a direct sha comparison against ${worker} would
# redden on every good build. So the comparison above is made where it can be made:
# between the file the bundler WOULD copy next time and the one cargo built. That is
# a statement about this checkout. It says nothing about which bytes went into THIS
# image — build an image, edit the worker, re-stage, and staged still equals built
# while the image carries the older worker. Every other check below passes in that
# state: the worker is present, executable, runs, and answers.
#
# The timestamp closes that path and only that path. It is not identity — identity
# cannot be had here, for the reason above — it is the cheapest statement that rules
# out an image written before the worker it is supposed to contain.
[ "${dmg}" -nt "${built_worker}" ] \
  || fail "${dmg} is older than the worker this checkout built, so it cannot be carrying
  that worker. This is NOT the staged sidecar going stale, and it does not duplicate the
  check above: that one just proved the staged file is the built one, and it can be
  perfectly fresh while this image predates both. What is stale here is the image —
  something was rebuilt after it was written. Re-run cargo tauri build."

# --- the signature ------------------------------------------------------------
#
# Before anything out of the image is executed, not after. This script is pointed at
# images it has no reason to trust — its own controls feed it /dev/urandom and shell
# scripts standing in for the worker — and running a binary out of an artefact whose
# seal has not been checked is the wrong order on its own terms.
#
# It cannot move any higher, and the constraint is not obvious. The two checks above
# it — the worker is present, the worker is executable — are proved by controls that
# remove and chmod the worker without re-sealing the copy they mutate. A seal check
# running before those would redden on the seal instead, and both controls would prove
# nothing. The controls that swap the worker for a stand-in re-sign the copy first, on
# purpose, and are unaffected by this order.
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

# --- what the worker says -----------------------------------------------------
#
# The check stops here being about files and starts being about behaviour. Nothing
# else in this repository proves that the binary a user receives can read a file:
# the Rust tests build a worker fresh (src-tauri/tests/support/mod.rs:26) and never
# look inside a bundle.
#
# Run directly off the mounted image: measured in task 1, `hdiutil` mounts it
# `read-only,nodev,nosuid,noowners`, not `noexec`, so a binary runs fine from
# there and nothing is copied out first.
#
# MNEMA_PDFIUM_LIB_DIR is cleared, not merely unset: it is the first branch of
# mnema_extract's library search (crates/mnema-extract/src/pdfium_probe.rs:160-193),
# and leaving it set would let this machine answer a question about the bundle.
#
# The status is captured on its own line. Inside an `if` condition a failed run
# would read as an answer, which is the defect the dependency check below this one
# used to have.
ask_worker() {
  local fixture="$1"
  answer=""
  answer_status=0
  answer="$(printf '{"path":"%s","max_bytes":10485760}\n' "${fixture}" \
    | env -u MNEMA_PDFIUM_LIB_DIR "${worker}" 2>&1)" || answer_status=$?
}

text_fixture="${repo_root}/crates/mnema-extract/tests/fixtures/simple.txt"
[ -f "${text_fixture}" ] || fail "${text_fixture} is missing; the check has nothing to
  ask the worker about."

ask_worker "${text_fixture}"
# "on a plain text file" belongs on the first physical line, and so does "on a PDF" in
# the twin of this check further down: the two messages are otherwise identical up to
# the word that distinguishes them, and only the first line reaches the control that
# asserts it (controls 14 and 16c).
[ "${answer_status}" -eq 0 ] \
  || fail "the bundled worker exited ${answer_status} on a plain text file. It said:
  $(printf '%s' "${answer}" | head -3 | tr '\n' ' ' | cut -c1-200)"

printf '%s\n' "${answer}" | grep -q '"frame":"header"' \
  || fail "the bundled worker returned no header frame for ${text_fixture}. It said:
  $(printf '%s' "${answer}" | head -3 | tr '\n' ' ' | cut -c1-200)"
printf '%s\n' "${answer}" | grep -q '"frame":"block"' \
  || fail "the bundled worker returned no block frame for ${text_fixture}. It said:
  $(printf '%s' "${answer}" | head -3 | tr '\n' ' ' | cut -c1-200)"

echo "verify-bundle: the bundled worker reads a text file and returns blocks"

# --- what has to be in the bundle, decided by the worker's own answer ----------
#
# This used to be derived from `cargo tree -p mnema-desktop`, and that was wrong in
# both directions once a sidecar existed (D54). The shell does not link
# pdfium-render and never will; the worker links it and — with no reader
# implemented for any format but text — never loads it. The day a reader lands, the
# worker will load it with no change to any dependency graph.
#
# So the question is put to the thing that knows: the worker in this bundle, asked
# about a PDF, from the image, with the environment cleared.
pdf_fixture="${repo_root}/crates/mnema-extract/tests/fixtures/one-page-text.pdf"
[ -f "${pdf_fixture}" ] || fail "${pdf_fixture} is missing; the Pdfium question cannot
  be asked and is therefore UNANSWERED."

ask_worker "${pdf_fixture}"
[ "${answer_status}" -eq 0 ] \
  || fail "the bundled worker exited ${answer_status} on a PDF. That is not the answer
  'no reader' — it is no answer at all, and the bundle's Pdfium obligation stays
  UNANSWERED. It said:
  $(printf '%s' "${answer}" | head -3 | tr '\n' ' ' | cut -c1-200)"

packaged_pdfium_count="$(find "${app}" -name 'libpdfium*.dylib' | wc -l | tr -d ' ')"
packaged_pdfium="$(find "${app}" -name 'libpdfium*.dylib' | head -1)"

if printf '%s\n' "${answer}" | grep -q '"frame":"block"'; then
  # The bundle reads PDFs, so the library loaded. Three things then have to hold,
  # and only the last of them is about this image rather than about some image.
  #
  # This branch used to be a `fail` saying it could not be written, because the
  # third branch of the library search is an absolute path into the source
  # checkout baked in at compile time — so a bundle that reached it would read
  # PDFs on the machine that built it and fail everywhere else, and nothing here
  # could tell that apart from a correct bundle. That is not a hypothetical: the
  # first bundle built after the reader landed did exactly that, and the only
  # reason anyone saw it is that code signing refused the load and turned a
  # silent wrong answer into a visible one. What closes it is that the worker
  # now reports the directory it loaded from (`--probe-pdfium`), recorded by the
  # call that loaded it rather than re-derived, and this compares that against
  # the image it has mounted.
  case "${packaged_pdfium_count}" in
    0) fail "the bundled worker reads PDFs and ${product}.app carries no libpdfium*.dylib.
  It is loading one from somewhere else on this machine — the developer's vendor/ tree
  is the usual one — and a user's copy would have nothing to load. bundle.resources in
  src-tauri/tauri.conf.json is what puts it there." ;;
    1) : ;;
    # Two copies is the dead-weight defect of the branch below wearing a
    # different hat: one of them is what loaded and the other is 7.7 MB of
    # nothing. Which is which cannot be told by sorting, so it is not guessed.
    *) fail "${packaged_pdfium_count} copies of libpdfium*.dylib in ${product}.app. One of them
  is dead weight and this check will not pick whichever sorted first, the same reason
  ${dmg_dir} rejects two images. Found: $(find "${app}" -name 'libpdfium*.dylib' | tr '\n' ' ')" ;;
  esac

  probe=""
  probe_status=0
  probe="$(env -u MNEMA_PDFIUM_LIB_DIR "${worker}" --probe-pdfium "${pdf_fixture}" 2>&1)" \
    || probe_status=$?
  [ "${probe_status}" -eq 0 ] \
    || fail "the bundled worker exited ${probe_status} on --probe-pdfium, having just read a PDF
  through the same library. It said:
  $(printf '%s' "${probe}" | head -3 | tr '\n' ' ' | cut -c1-200)"
  # Both halves, because either alone is satisfied by the wrong thing: `loaded`
  # without the directory is the claim this branch existed to refuse, and a
  # directory without `loaded` would be read out of a failure message.
  printf '%s\n' "${probe}" | grep -q '"loaded":true' \
    || fail "the bundled worker read a PDF over the wire and then said it cannot load Pdfium:
  $(printf '%s' "${probe}" | head -3 | tr '\n' ' ' | cut -c1-200)"
  loaded_dir="$(printf '%s\n' "${probe}" | sed -n 's/.*"library_dir":"\([^"]*\)".*/\1/p')"
  [ -n "${loaded_dir}" ] \
    || fail "the bundled worker's --probe-pdfium named no library_dir, so WHERE it loaded
  Pdfium from is UNANSWERED — which is not the answer 'from inside the bundle'. It said:
  $(printf '%s' "${probe}" | head -3 | tr '\n' ' ' | cut -c1-200)"

  # Both paths go through `pwd -P` rather than being compared as strings: the
  # image mounts under /var/folders/… and `current_exe()` reports the same place
  # as /private/var/folders/…, so a string comparison of two correct answers
  # fails. Derived from the image rather than written out, so that moving the
  # library inside the bundle does not need an edit here.
  expected_dir="$(cd "$(dirname "${packaged_pdfium}")" && pwd -P)"
  loaded_real=""
  loaded_real="$(cd "${loaded_dir}" 2>/dev/null && pwd -P)" || loaded_real=""
  # The whole verdict on the first physical line, because that is the only line
  # carrying the `verify-bundle:` prefix and therefore the only one control 16f
  # can assert on — wrapping it after "with a Pdfium" would leave the control
  # matching nothing and reporting a correct check as WRONG REASON.
  [ "${loaded_real}" = "${expected_dir}" ] || fail "the bundled worker loaded Pdfium from OUTSIDE this image.
  It loaded ${loaded_dir}, and the image carries ${packaged_pdfium}. A bundle in this
  state reads PDFs on the machine that built it and on no other, which is what this
  branch exists to refuse. See D54 and the packaging spec §4."

  echo "verify-bundle: the bundled worker reads a PDF, loading Pdfium from inside the image"
elif printf '%s\n' "${answer}" | grep -q '"rule":"unsupported"'; then
  [ -z "${packaged_pdfium}" ] || fail "the bundled worker refuses PDFs as unsupported,
  so nothing in this bundle can load Pdfium — and ${packaged_pdfium} is in it anyway.
  7.7 MB of dead weight is a defect, not a spare part. Either a reader landed and this
  check is looking at a stale build, or the library was packaged by mistake."
  echo "verify-bundle: the bundled worker refuses PDF as unsupported, so no Pdfium is bundled"
else
  fail "the bundled worker answered a PDF with neither blocks nor rule=unsupported.
  An unrecognised verdict is UNANSWERED and must not be read as 'no reader'. It said:
  $(printf '%s' "${answer}" | head -3 | tr '\n' ' ' | cut -c1-200)"
fi

echo "verify-bundle: OK"

#!/usr/bin/env bash
#
# Builds the extraction worker so that the application can find it — in a packaged
# build and in a development one, which are two different places.
#
# `cargo tauri build` never builds this binary on its own: src-tauri deliberately
# does not depend on mnema-extract (src-tauri/Cargo.toml:24), so the worker is not
# in the shell's dependency graph and nothing pulls it in. Neither does
# `cargo tauri dev` — which is why paths.rs used to describe the development case
# as working only after somebody had run a cargo build by hand.
#
# Both profiles then copy that binary under the name bundle.externalBin requires:
# Tauri looks for `src-tauri/binaries/<name>-<triple>` and strips the triple when
# bundling, so the file inside the app is plain `mnema-extract-worker`. This is
# the one place that convention is written down — docs/BUILD.md points here rather
# than repeating it.
#
# `debug` stages too, and that is not symmetry for its own sake. `tauri-build`
# validates the declared external binary while src-tauri COMPILES, in whatever
# profile — not when a bundle is assembled. So a tree without that file cannot
# build the shell at all. Measured: with no `src-tauri/binaries/`, `cargo tauri
# dev` exits 101 on `resource path binaries/mnema-extract-worker-<triple> doesn't
# exist` — after this hook has run and reported success. An earlier version of
# this script returned before the copy on the debug profile, and development
# appeared to work only because some earlier release build had left the file
# behind; remove that leftover and a fresh clone could not run `tauri dev` at all.
#
# One consequence to know rather than discover: whichever profile ran last is what
# sits in `src-tauri/binaries/`. That is harmless, because `cargo tauri build`
# re-stages `release` through beforeBuildCommand before it bundles anything, so a
# debug binary cannot reach a package by being left there.
#
# Why a script rather than four lines in tauri.conf.json: build logic that lives
# only inside the bundler cannot be run by hand, and this repository has already
# paid for a check that only existed inside CI. The two `before*Command` hooks call
# this; a person debugging a bundle calls the same thing.
#
# Nothing here ends in `|| true`. A staging step that fails quietly produces a
# bundle with a stale worker in it, which is the one outcome nothing downstream
# can distinguish from success.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
name="mnema-extract-worker"
profile="${1:-release}"

case "${profile}" in
  release|debug) : ;;
  *) echo "stage-sidecar: profile must be release or debug, not '${profile}'." >&2
     exit 1 ;;
esac

flags=()
[ "${profile}" = "release" ] && flags=(--release)

# `${flags[@]+"${flags[@]}"}` rather than a plain `"${flags[@]}"`: macOS ships bash
# 3.2, where expanding an EMPTY array under `set -u` is an unbound variable, not an
# empty list. Measured — the plain form built the release sidecar and then died on
# `flags[@]: unbound variable` for the debug one, which is the profile the
# development hook uses. bash 5 from Homebrew does not reproduce it, so the trap
# only appears where `/usr/bin/env bash` finds the system shell.
cargo build ${flags[@]+"${flags[@]}"} --manifest-path "${repo_root}/Cargo.toml" \
  -p mnema-extract --bin "${name}"

built="${repo_root}/target/${profile}/${name}"
[ -x "${built}" ] || {
  echo "stage-sidecar: ${built} is missing after a successful build." >&2
  exit 1
}

triple="$(rustc -vV | sed -n 's/^host: //p')"
[ -n "${triple}" ] || {
  echo "stage-sidecar: rustc did not report a host triple." >&2
  exit 1
}

dest_dir="${repo_root}/src-tauri/binaries"
mkdir -p "${dest_dir}"
# Always overwrite. The whole point is that a stale copy cannot survive a build.
cp -f "${built}" "${dest_dir}/${name}-${triple}"
chmod +x "${dest_dir}/${name}-${triple}"

echo "stage-sidecar: ${dest_dir}/${name}-${triple}"

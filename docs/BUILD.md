# Building and packaging

How the macOS package is produced, what is inside it, and which of those facts were
measured rather than assumed. Everything below was run on 2026-07-26 on macOS 26.5.2,
arm64, with the toolchain `rust-toolchain.toml` names (1.97.1) and `tauri-cli` 2.11.4;
the Linux figures come from an `ubuntu:24.04` container, arm64, glibc 2.39.

Where a claim has not been observed, it says so. `.github/workflows/ci.yml` has never
run — this repository has no workflow run at all — so every statement about the runners
is a prediction until the first push.

## The version pair

| | |
|---|---|
| `pdfium-render` | `=0.9.3` |
| Pdfium binary, non-V8 | `chromium/7881` |

Those two rows are checked against the pin they describe by
`crates/mnema-extract/tests/pdfium_binding.rs`, the same way the table in that crate's
README is. The full chain — crate feature, constant, fetch script, vendored binary —
is documented in `crates/mnema-extract/README.md`; this page repeats only the pair a
packager needs and is bound to it by a test so the repetition cannot go stale.

### How it is linked

**It is not linked at all.** `pdfium-render` opens the library at run time by path
(`Pdfium::bind_to_library`), so:

- `otool -L target/release/mnema-desktop` names no Pdfium. That is the expected
  result, not evidence of static linking. Task 9's brief predicted the opposite
  and offered `|| echo "pdfium is statically linked"` as the fallback reading; both
  are wrong, and static linking is not even available — the prebuilt macOS releases
  ship `lib/libpdfium.dylib` and no archive.
- **The build does not need the library.** `scripts/fetch-pdfium.sh` is a prerequisite
  of `cargo test`, not of `cargo build` or `cargo tauri build`. This is why the CI
  bundle job has no vendoring step.

## What a build machine needs

| | |
|---|---|
| macOS | 14 or later. Built and measured on 26.5.2. |
| Architecture | arm64 or x86-64. A package is per architecture — see below. |
| Xcode command line tools | for the linker, `codesign` and `hdiutil`. |
| Rust | whatever `rust-toolchain.toml` names; `rustup toolchain install` provisions it. |
| Tauri CLI | major version 2. |
| Pdfium | **not needed to build.** Needed to run `cargo test`. |

## The commands

```sh
# Once per machine.
cargo install tauri-cli --version "^2" --locked

# Once per clone, before any plain cargo command. `cargo tauri build` and
# `cargo tauri dev` run this themselves; `cargo build`, `cargo clippy` and
# `cargo test` do not, and the shell does not compile without it — see
# "A fresh checkout cannot build the shell" below for what it says when it stops.
scripts/stage-sidecar.sh release

# The package.
cargo tauri build

# What it produced, opened and checked.
scripts/verify-bundle.sh
```

`scripts/verify-bundle.sh` is where the checks live rather than in the workflow, so
that a failure can be reproduced locally.

What it rejects is not written out here as prose, because prose is how the count went
wrong the first time — three documents claimed six, seven and seven-of-a-different-set,
and one of them described a control nobody had run. The list is
`scripts/verify-bundle-controls.sh`: it produces every rejected state on purpose,
requires each to exit non-zero, requires the real image to pass, and prints the tally.

```sh
scripts/verify-bundle-controls.sh
```

It prints the tally itself — `red`, `still green`, `broken controls` — and no number
is repeated here, because an eleventh control would make one stale and nothing would
notice. That is item 4 in miniature, in the paragraph that removed item 4.

A control whose *setup* fails is counted as broken rather than red, and that
distinction is not bookkeeping: before it existed, an image that would not attach left
three controls building a package out of a directory that was never created. All three
went red — on "no .dmg", which is a different control's reason — and the run still
reported the full count.

Two of the controls are worth naming because they are not about the image at all:

- **`cargo tree` cannot answer.** The dependency question is asked with a command that
  can fail, and a failure is not the answer "no". Before this was fixed the pipeline
  sat inside an `if`, where neither `set -e` nor `pipefail` applies, so a broken
  manifest or a renamed package printed *"the shell does not link pdfium-render"* and
  exited 0 — with cargo's own error visible in the log above the green verdict.
- **the shell depends on Pdfium and the bundle carries none.** The check derives what
  must be packaged from the graph rather than from a constant, so it stays quiet today
  and turns red the day extraction is wired into the shell.

## What the build produces

```
target/release/bundle/dmg/Mnema_0.0.0_aarch64.dmg     3,704,157 bytes
    └─ Mnema.app                                         11.5 MiB
         Contents/MacOS/mnema-desktop                 11,966,368 bytes, Mach-O arm64
         Contents/Resources/Mnema.icns
         Contents/_CodeSignature/CodeResources
```

**`target/release/bundle/macos/` is empty when the build finishes.** With
`targets: ["dmg"]` the `.app` is an intermediate: the bundler writes the image and then
prints `Cleaning …/Mnema.app` and removes it. A check pointed at that directory — which
is what the task brief specified — reports "no bundle" on a perfectly good build. This
is why `verify-bundle.sh` attaches the image and looks at the application inside it,
which is also the copy a user actually receives.

The `.icns` is generated from the four committed PNGs; no `.icns` needs to be committed.

## The extraction worker inside the bundle

The application hands each file to a separate short-lived process, `mnema-extract-worker`,
and resolves it as a sibling of its own executable (`src-tauri/src/paths.rs`). Neither
`cargo tauri build` nor `cargo tauri dev` builds that binary on its own: `src-tauri`
deliberately does not depend on `mnema-extract`, so the worker is in no dependency graph
either command walks. `scripts/stage-sidecar.sh` builds it and, for a release build, copies
it to `src-tauri/binaries/mnema-extract-worker-<triple>` — the name `bundle.externalBin`
requires, and the only place that convention is written down. `beforeBuildCommand` and
`beforeDevCommand` each call it; so can a person debugging a bundle.

The five facts below were measured on 2026-08-03, macOS 26.6 (25G72), arm64, rustc 1.97.1,
`tauri-cli` 2.11.4. None of them had been observed here before.

**It lands beside the application's own executable.** Attaching the image and running
`find …/Mnema.app -name 'mnema-extract-worker*'` returns exactly
`Contents/MacOS/mnema-extract-worker`, and `ls -l` on it shows `-rwxr-xr-x`. Both halves
matter: that is the sibling directory `paths.rs` already looks in, so no code had to change,
and the bundler does not drop the executable bit on the way in.

**The seal still verifies with nested code inside.** `scripts/verify-bundle.sh` exits 0.
Its `codesign --verify --deep --strict` walks into the sidecar rather than stopping at the
outer seal — it prints `--prepared:` and `--validated:` lines naming
`Contents/MacOS/mnema-extract-worker` — and still reports `valid on disk` and
`satisfies its Designated Requirement`. The bundler signs the sidecar individually first
(`Signing …/mnema-extract-worker: replacing existing signature`) and seals the bundle after.

**The bundled bytes are not the built bytes.** `shasum -a 256` gives the copy inside the
image `8b9f1f71…`, and gives `target/release/mnema-extract-worker` and
`src-tauri/binaries/mnema-extract-worker-aarch64-apple-darwin` the same `b91c71fb…` as each
other. The sizes differ by 352 bytes, which is the signature the bundler replaces. So the
staged copy and the built copy can be compared to each other by digest; **the copy inside
the image cannot be compared to either that way**, and a freshness check that tries reports
a stale worker on every build.

**It runs off the read-only mount.** Feeding one NDJSON request to
`/Volumes/…/Mnema.app/Contents/MacOS/mnema-extract-worker` on the attached image returns
`header`, `page`, two `block` frames and `summary`, and exits 0. The image mounts
`read-only, nodev, nosuid, noowners` — and not `noexec`, which is the flag that would have
made this fail. Nothing has to be copied out of the image to exercise the worker.

**The development hook lands the debug binary — but it is not the last writer.** With
`target/debug/mnema-extract-worker` deleted, `cargo tauri dev` logs
`Running BeforeDevCommand (../scripts/stage-sidecar.sh debug)` and the binary is back within
four seconds, at the path `worker_path()` resolves to. That closes the gap `paths.rs`
described, where development worked only after somebody had run a `cargo build` by hand and
nothing enforced it. What the hook does not get is the last word. `tauri-build`'s build
script declares `rerun-if-changed` on `tauri.conf.json`, and when it re-runs it copies the
staged **release** sidecar over `target/debug/mnema-extract-worker`. Measured with the
confound removed rather than inferred: with the build script fresh, the debug binary
(`6a698545…`, 5,449,544 B) survived a whole `cargo tauri dev`; with `tauri.conf.json`
touched, a plain `cargo build -p mnema-desktop` replaced it with a byte-identical copy of
`src-tauri/binaries/…` (`b91c71fb…`, 3,054,784 B). **Which worker a development run executes
therefore depends on which of the two wrote last**, and `tauri-build`'s copy is only as fresh
as the last `scripts/stage-sidecar.sh release` — a `cargo tauri dev` after a source change to
`mnema-extract` can run the previous release build of the worker.

### A fresh checkout cannot build the shell until the sidecar is staged

`src-tauri/binaries/` is git-ignored on purpose: committing it would let a stale worker ship
inside a green build. The cost is that `tauri-build` refuses to run without it, and says so
from inside the build script:

```
error: failed to run custom build command for `mnema-desktop v0.0.0 (…/src-tauri)`
  resource path `binaries/mnema-extract-worker-aarch64-apple-darwin` doesn't exist
```

Measured with the directory moved aside: `cargo build -p mnema-desktop` and
`cargo clippy -p mnema-desktop --all-targets` both exit 101 on that error. The two `tauri`
commands are unaffected, because their `before*Command` hooks stage the file first — but
**plain cargo has no hooks**, so `cargo clippy --workspace --all-targets` and
`cargo test --workspace` do not get one. That is what `.github/workflows/ci.yml`'s `check`
job runs, on a checkout where the directory cannot exist, and the job has no staging step
yet. A person needs `scripts/stage-sidecar.sh release` once after cloning; the workflow needs
the same thing, and this is the record that it does not have it.

## Signing

`src-tauri/tauri.conf.json` sets `bundle.macOS.signingIdentity` to `"-"`, which is the
ad-hoc identity. The setting is load-bearing and was measured both ways:

| `signingIdentity` | `Contents/_CodeSignature` | `codesign --verify --deep --strict` |
|---|---|---|
| absent | not created | **exit 1** — `code has no resources but signature indicates they must be present` |
| `"-"` | created | exit 0 — `valid on disk`, `satisfies its Designated Requirement` |

Without it the executable still carries the ad-hoc signature the linker applies to every
arm64 binary, under the identifier `mnema_desktop-<hash>`; what is missing is the seal
over the bundle. With it, the identifier is `com.mnema.desktop`, the flags are
`0x10002 (adhoc, runtime)` — the bundler enables the hardened runtime — and the seal
covers the resources, so adding a file to `Contents/Resources` after the fact makes the
verification fail.

**This is not Gatekeeper acceptance and must not be reported as such.** An ad-hoc
signature carries no Team ID and the image is not notarized:

```
spctl -a -vvv -t exec Mnema.app   →   rejected   (exit 3)
```

A user who downloads this `.dmg` gets the quarantine bit and, with it, "Apple cannot
check it for malicious software". Closing that needs a Developer ID certificate and a
notarization step (`APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID`, or an API key —
the bundler already logs `skipping app notarization` when they are absent). Neither
exists yet; it is a purchase and an account decision, not a build change.

## One package per architecture

`cargo tauri build --target universal-apple-darwin` produces a lipo'd binary carrying
both slices:

| | `.dmg` | `Mnema.app` | executable |
|---|---|---|---|
| arm64 only | 3,704,157 B | 11.5 MiB | 11,966,368 B |
| universal | 7,597,603 B | 23.2 MiB | 24,303,184 B |
| ratio | ×2.05 | ×2.02 | ×2.03 |

So the universal image is a little over twice the size of the one a given user can run,
and every user downloads a slice for a machine they do not own. Packages are therefore
built per architecture. Two further consequences worth knowing before anyone reaches
for it:

- the universal build writes to `target/universal-apple-darwin/release/bundle/dmg/`,
  which the CI artefact glob deliberately does not match;
- it needs both `rustup` targets installed and compiles the whole dependency tree twice.

## When Pdfium has to go into the bundle

Nothing in the shipped application loads Pdfium today: `src-tauri` does not depend on
`mnema-extract`, so `cargo tree -p mnema-desktop -e normal` contains no `pdfium-render`
and the 7.7 MB library would be dead weight. `verify-bundle.sh` derives that from the
graph and turns red the day it stops being true, which is the day the following matters.

Both placements were measured on this repository:

| config | lands at | signed |
|---|---|---|
| `bundle.macOS.frameworks: ["../vendor/pdfium/lib/libpdfium.dylib"]` | `Contents/Frameworks/libpdfium.dylib` | yes, individually, ad-hoc; the bundle seal still verifies |
| `bundle.resources: {"…/libpdfium.dylib": "pdfium/"}` | `Contents/Resources/pdfium` | sealed as a resource |

Note the second row: the map value is a destination **path**, not a directory, so
`"pdfium/"` renamed the library to `pdfium`. Two resource entries sharing a first path
component (`pdfium/lib/` and `pdfium/`) fail the build outright with
`File exists (os error 17)` from `tauri-build`, which creates `target/release/pdfium`
for the first entry and collides on the second.

**Neither placement is reachable by today's loader.**
`mnema_extract::library_dir` looks in `$MNEMA_PDFIUM_LIB_DIR`, then beside the running
executable (`Contents/MacOS`), then in the development `vendor/` tree. Neither
`Contents/Frameworks` nor `Contents/Resources` is on that list, and `verify_build`
additionally wants a `VERSION` manifest in the library's directory or its parent —
`frameworks` accepts only `.dylib` and `.framework` entries, so the manifest cannot go
there at all. Packaging Pdfium is therefore a change to `mnema-extract`'s search order
plus a decision about where the manifest lives, and it belongs to the packaging or
extraction spec rather than to a bundler setting. The hardened runtime adds a third
question on top: a `dlopen`'d library must satisfy library validation, which an ad-hoc
signature with no Team ID does not obviously do.

## Linux

Linux is in the CI matrix and is not a delivery target. It is there because the crate
list was assembled from documentation and Tauri's weakest seam is the packaged
WebKitGTK it builds against. Measured in an `ubuntu:24.04` container (arm64, glibc 2.39,
webkit2gtk-4.1 2.52.3): the workspace builds and the full test suite passes, with the
package list the workflow installs. Removing `libwebkit2gtk-4.1-dev` stops the build in
`webkit2gtk-sys`, on `pkg-config` failing to find `webkit2gtk-4.1` — so the step is
load-bearing.

`scripts/fetch-pdfium.sh` gained Linux x86-64 and arm64 pins for this, recorded from the
same release as the macOS pair; both archives carry `lib/libpdfium.so` and the same
`BUILD=` manifest. The number is written once in this page, in the table at the top,
where a test reads it — repeating it here is how it would go stale, and it did: the
first version of this paragraph carried a literal that survived a bump in review and
came to contradict the sentence around it.

A Windows x86-64 pin was added on the same reasoning and is worth no more than the
Linux ones: it lets a Windows machine run the suite, not ship a product. The full
platform list lives in `crates/mnema-extract/README.md`, where a test holds it against
the script; it is not repeated here for the reason the paragraph above gives about the
number. What is worth recording here is what the Windows archive does differently,
because a packager meets it: the library arrives in `bin/pdfium.dll` rather than
`lib/`, and the script renames the directory on install so that `vendor/pdfium/lib/`
means the same thing everywhere.

Two gaps stay open and are not closed by the container run: the runner is **x86-64**
while the container was arm64, so the C build of `sqlite-vec` on x86-64 Linux is still
unobserved, and the D-Bus Secret Service store that `mnema-secrets` compiles on Linux
has no session on a runner and stays unexercised.

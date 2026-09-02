# Building and packaging

How the macOS package is produced, what is inside it, and which of those facts were
measured rather than assumed. This page was not written in one sitting, so no single
date covers every claim on it: a section that names its own date and toolchain was
measured under those, not under the ones in this paragraph. Everything without a
date of its own was run on 2026-07-26 on macOS 26.5.2, arm64, with the toolchain
`rust-toolchain.toml` names (1.97.1) and `tauri-cli` 2.11.4; the Linux figures come
from an `ubuntu:24.04` container, arm64, glibc 2.39.

Where a claim has not been observed, it says so. `.github/workflows/ci.yml` has run on
`main`, `refuse-by-content` and `watched-folder` since 2026-07-30, with failures among
them. `packaging-and-delivery` has never been pushed and has no runs of its own, so
every statement about the runners is still a prediction until this branch's first push.

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
- **The build did not need the library, and now it does.** `scripts/fetch-pdfium.sh`
  was a prerequisite of `cargo test` alone, which is why the CI bundle job had no
  vendoring step. `bundle.resources` changed that: `cargo tauri build` reads the file
  out of `vendor/` and packages it, so the vendoring step is now in both jobs. Nothing
  is *linked* against it either way — that part of this section still holds.

## What a build machine needs

| | |
|---|---|
| macOS | 14 or later. Built and measured on 26.5.2. |
| Architecture | arm64 or x86-64. A package is per architecture — see below. |
| Xcode command line tools | for the linker, `codesign` and `hdiutil`. |
| Rust | whatever `rust-toolchain.toml` names; `rustup toolchain install` provisions it. |
| Tauri CLI | 2.11.4 — the version CI installs as a release binary (`.github/workflows/ci.yml`, the `bundle` job, pinned there by version and checksum). A newer one is bumped in both places at once. |
| Pdfium | `scripts/fetch-pdfium.sh`, before `cargo test` **and** before `cargo tauri build` — the bundle ships it. |

## The commands

```sh
# Once per machine.
cargo install tauri-cli --version 2.11.4 --locked

# Once per clone, before anything else. `src-tauri` declares both of these as
# files that must exist, and `tauri-build` checks them from inside the build
# script — so nothing about `src-tauri` compiles until they do, in any profile.
# `cargo build`, `cargo clippy` and `cargo test` have no hooks that would create
# either, and `cargo tauri dev` runs its hook without waiting for it to finish.
# (`cargo tauri build` does wait; only the dev hook is unawaited.) See "A fresh
# checkout cannot build the shell" below for what it says when it stops, and for
# why the dev hook is not a substitute.
scripts/stage-sidecar.sh debug     # or release; either satisfies the declaration

# Lint with this rather than `cargo clippy` directly. It is the same lint, with
# `-D warnings`, in a target directory of its own — because clippy and
# `cargo test` compile the same crates with different rustc invocations, so
# sharing one `CARGO_TARGET_DIR` makes each invalidate what the other just
# built. Measured on a fully built tree: clippy 172s -> 2s, and the
# `cargo test --workspace` that follows it 1783s -> 492s. The script's own
# header carries the numbers and what was measured and rejected.
scripts/lint.sh
scripts/fetch-pdfium.sh            # bundle.resources names it; see "Pdfium in the bundle"

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

**Nothing runs it automatically, and that leaves the check itself unguarded.** The CI
`bundle` job builds the image and runs `scripts/verify-bundle.sh`; no workflow runs the
controls. So an edit that makes `verify-bundle.sh` stop rejecting things — a branch that
can no longer be reached, an assertion that any failure satisfies, a check deleted along
with the state it named — passes CI green and keeps passing. The only thing that would
notice is a person who remembers to run this script. That is deliberate rather than
overlooked: the suite needs a built bundle and takes minutes. It is also this
repository's own signature defect, a step that passes while proving nothing, standing
one level above the check written to catch it.

It prints the tally itself — `red for its own reason`, `WRONG REASON`, `still green`,
`broken controls`, `shipped image rejected` — and no number is repeated here, because
an eleventh control would make one stale and nothing would notice. That is item 4 in
miniature, in the paragraph that removed item 4. Every control asserts a fragment of
the message it expects, so a control that exits non-zero for somebody else's reason is
counted apart from one that proved what it names, and prints which fragment it wanted.

A control whose *setup* fails is counted as broken rather than red, and that
distinction is not bookkeeping: before it existed, an image that would not attach left
three controls building a package out of a directory that was never created. All three
went red — on "no .dmg", which is a different control's reason — and the run still
reported the full count. A control whose asserted fragment is empty is broken for the
same reason: it would match every message and prove nothing while looking proved.

The Pdfium check is worth naming on its own, because what it checks is not the image
at all, and because it used to work differently:

- **The verdict comes from the worker, not from a dependency graph.** This check used
  to ask `cargo tree -p mnema-desktop`, and that answered a question about the wrong
  binary once a sidecar existed: `src-tauri` never depended on `mnema-extract`, so the
  graph said nothing was needed even after extraction moved into a bundled worker that
  does link Pdfium — the day that happened, a graph-based check would have stayed
  quiet rather than turned red. `scripts/verify-bundle.sh` now feeds the bundled
  worker, run from inside the mounted image, one PDF and reads its verdict.
  `refused`/`unsupported` means the library must **not** be in the bundle — present
  anyway is dead weight, a defect rather than a spare part. That branch is no longer
  reachable from this repository's own configuration, which packages the library
  unconditionally; it is kept because the rule outlives the configuration, and
  `scripts/mutations/pdf-refuses-unsupported.sh` is what still reaches it. `blocks`
  means the library must be present exactly once, and must have been loaded **from
  inside this image** — established by asking the worker where it loaded from, not by
  finding a `.dylib` somewhere in the bundle. Any other answer is unanswered, and
  unanswered reads as red, not as a pass.

## The acceptance run

Run once per packaging change, by a person. It is the only part of the criterion
no script covers, and that is deliberate: driving the window automatically is
expensive and brittle, and this is the one link a human checks anyway.

1. Download the `mnema-macos-arm64` artefact from a green CI run — **with `gh run download`,
   not through a browser.** How you fetch it decides whether step 3 works at all; see below.
2. Mount the image; drag `Mnema.app` into `/Applications`; eject the image.
3. Launch it from `/Applications`, not from the mounted volume.
4. Add a folder holding several `.txt` and `.md` files.
5. The walk finishes without `EndReason::Failed`.
6. Ask a question whose answer is in one of those files; a citation comes back,
   and its highlight covers text that is actually in the file.

Add a `.pdf` to that folder too, and expect it to be **read** rather than refused.
That is the one step of this list that changed when the format readers landed, and it
is worth doing by hand: it is the only place where the packaged library, the
entitlement and the loader meet outside a script. A PDF that comes back refused as
unsupported now means the bundle lost its reader.

### Why step 1 names the tool

The image is ad-hoc signed and unnotarized on purpose, and `spctl` rejects it either way
— that verdict is not new and is measured above. What changes with the download is
whether the system **acts** on the verdict, and Gatekeeper acts only on a file carrying
`com.apple.quarantine`.

Measured on both paths, same artefact:

| | `gh run download` | downloaded through a browser |
|---|---|---|
| `com.apple.quarantine` on the `.dmg` | absent | `0081;…;<agent>;<UUID>` |
| propagates to the app copied out of the image | — | yes, as `0281;…` |
| `spctl -a -t exec` | rejected | rejected |
| `codesign --verify --deep --strict` | valid | valid |
| the bundled worker reads a file | yes | yes |

So the browser path blocks at launch while the bundle is intact — the seal verifies and
the worker inside it works. The failure a person meets says the application is damaged,
which is false, and it is the one step of this list that fails for a reason having
nothing to do with what was packaged.

One half of this is now observed rather than inferred. The acceptance run was performed
on 2026-08-03 with an artefact fetched by `gh run download`, and the application launched
from `/Applications` with **no Gatekeeper prompt at all** — so the left column is measured
end to end, and the tool named in step 1 is a measured instruction rather than a cautious
one.

Two limits remain, so nobody re-derives them: the browser case was reproduced by writing
the attribute LaunchServices writes, not by clicking a download, so the Gatekeeper
behaviour and the propagation are measured while the exact attribute a given browser
writes is not; and no launch was attempted from that path, so the block itself is still
inferred from the quarantine mechanism rather than seen.

Closing this properly needs a Developer ID certificate and notarization, which is a
purchase and an account decision, not a build change.

## What the build produces

```
target/release/bundle/dmg/Mnema_0.0.0_aarch64.dmg      5,815,668 bytes
    └─ Mnema.app                                          17.7 MiB
         Contents/MacOS/mnema-desktop                  15,391,264 bytes, Mach-O arm64
         Contents/MacOS/mnema-extract-worker            3,055,136 bytes, Mach-O arm64
         Contents/Resources/Mnema.icns
         Contents/_CodeSignature/CodeResources
```

Measured 2026-08-03 on macOS 26.6 (25G72), arm64, rustc 1.97.1, `tauri-cli` 2.11.4.
The bundle carries one more binary than it used to: `mnema-extract-worker` did not
exist in this table before `bundle.externalBin` was wired up, and none of the other
rows above were adjusted by arithmetic — a fresh `cargo tauri build` and a fresh
`du` produced every figure in this table.

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
either command walks. `scripts/stage-sidecar.sh` builds it in the profile it is given and
copies it to `src-tauri/binaries/mnema-extract-worker-<triple>`, the name
`bundle.externalBin` requires; that script's header is where the naming convention is
written down, and this page does not repeat it. `beforeBuildCommand` and `beforeDevCommand`
each call it; so can a person debugging a bundle, and — see the end of this section — so
must a person with a fresh clone.

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
a stale worker on every build. `scripts/verify-bundle.sh` therefore proves freshness at the
staged file in `src-tauri/binaries/` against `target/release/mnema-extract-worker`, and does
not compare either of them to the copy inside the image.

**It runs off the read-only mount.** The image was attached with
`hdiutil attach -readonly -nobrowse -mountpoint /tmp/mnema-m4`, and feeding one NDJSON
request to `/tmp/mnema-m4/Mnema.app/Contents/MacOS/mnema-extract-worker` returns
`header`, `page`, two `block` frames and `summary`, and exits 0. The image mounts
`read-only, nodev, nosuid, noowners` — and not `noexec`, which is the flag that would have
made this fail. Nothing has to be copied out of the image to exercise the worker.

**The development hook runs, and is not the last writer — nor a guarantee.** With
`target/debug/mnema-extract-worker` deleted, `cargo tauri dev` logs
`Running BeforeDevCommand (../scripts/stage-sidecar.sh debug)` and the binary is back within
four seconds, at the path `worker_path()` resolves to. That closes the gap `paths.rs`
described, where development worked only after somebody had run a `cargo build` by hand and
nothing enforced it. Two things qualify it, both measured.

*The dev hook is not awaited.* In every `cargo tauri dev` log on this repository,
`Running DevCommand` is the line immediately after `Running BeforeDevCommand`, and the hook's
own output arrives later, interleaved with the dev build — sometimes behind
`Blocking waiting for file lock on build directory`. Tauri spawns that hook and does not wait,
which is right for a dev server and wrong for a staging step whose output the build needs. So
on a clone with no `src-tauri/binaries/` at all, `cargo tauri dev` is a race between the
hook's copy and `tauri-build`'s validation of the same path: **one failure in ten runs**, on
`resource path … doesn't exist`, in the run where the dev build's cargo took the
build-directory lock first and left the hook still compiling when the build script asked for
the file.

That rate is worth exactly its conditions, so here they are: every run was on this machine
with a **warm** `target/` and `src-tauri/binaries/` deleted beforehand, three of them also
with the worker unlinked and three of those also with `src-tauri/src/lib.rs` and
`crates/mnema-extract/src/lib.rs` touched so the shell itself recompiled. A genuinely cold
clone compiles for minutes on both sides and shifts the contention this measures; nobody has
run that. **Since the staged file survives `cargo clean`, the coin is flipped once per clone
and never again** — which is why one hand-run command retires it.

**The dev hook is therefore a convenience, not the mechanism.** Run `scripts/stage-sidecar.sh`
once yourself after cloning; that is what makes it dependable, and what the CI `check` job
does as an explicit step rather than relying on a hook.

*`tauri-build` writes to the same path.* Its build script declares `rerun-if-changed` on
`tauri.conf.json`, and when it re-runs it copies whatever is in `src-tauri/binaries/` over
`target/debug/mnema-extract-worker`. Since both profiles now stage, the common path is
self-consistent: after `scripts/stage-sidecar.sh debug`, the staged file and
`target/debug/mnema-extract-worker` are the same 5,449,544-byte debug binary, digest
`5eb4fc08…` for both. **The hazard is narrowed, not gone.** Stage `release` — which
`cargo tauri build` does on every package — and then run a plain `cargo build -p mnema-desktop`
with no hook in between, and `target/debug/mnema-extract-worker` becomes the 3,054,784-byte
release binary, `b91c71fb…`, for both. A development run started after a packaging run
executes a release worker, and if `mnema-extract`'s sources have moved since, a stale one.

### A fresh checkout cannot build the shell until the sidecar is staged

`src-tauri/binaries/` is git-ignored on purpose: committing it would let a stale worker ship
inside a green build. The cost is that `tauri-build` refuses to run without it, and says so
from inside the build script:

```
error: failed to run custom build command for `mnema-desktop v0.0.0 (…/src-tauri)`
  resource path `binaries/mnema-extract-worker-aarch64-apple-darwin` doesn't exist
```

Measured with the directory moved aside: `cargo build -p mnema-desktop`,
`cargo check -p mnema-desktop` and `cargo clippy -p mnema-desktop --all-targets` all exit 101
on that error. **Plain cargo has no hooks**, so `cargo clippy --workspace --all-targets` and
`cargo test --workspace` get nothing to save them, and that is what
`.github/workflows/ci.yml`'s `check` job runs on a checkout where the directory cannot exist.
The job therefore stages explicitly, before `fmt`, `clippy` and `test`.

The two `tauri` commands are **not** equivalent to that step, and the difference is not
symmetry:

- `cargo tauri build` is safe, and the evidence is the log ordering rather than the exit
  status: from an absent-directory state it prints `Running beforeBuildCommand`, then the
  hook's own `stage-sidecar: …/src-tauri/binaries/…` line, and only then the build output.
  `beforeBuildCommand` is awaited. (Two green runs would not have shown that; they cannot
  separate "awaited" from "won the race" — which is exactly what went wrong with the dev hook
  above.) So the `bundle` job needs no staging step and does not have one.
- `cargo tauri dev` is not, because `beforeDevCommand` is *not* awaited. On a clone with
  nothing staged the hook races `tauri-build`'s validation of the file it is still writing.
  The hook removes the *certain* failure, not the *possible* one; the rate and the conditions
  it was measured under are in the section above, and are not repeated here.

Hence the once-per-clone line in *The commands* above. **The requirement belongs to
`externalBin`, not to the staging script:** `tauri-build` validates the declared path while
`src-tauri` compiles, in any profile, so it holds however the file arrives and no change to
how staging works removes it.

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

Both rows and the ratio belong to a build made before `bundle.externalBin` put a worker
in the bundle. **When is not recorded.** No build log names the day the universal image
was produced; the 2026-07-26 at the top of this page is the default this section falls
under, not a measurement taken for this table, and writing it here as one would state
more than is known. The current arm64 figures, worker included, are in
"What the build produces" above; there is no current universal counterpart to compare
them to, since producing one compiles the dependency tree twice and nobody has run that
since the worker landed.

So the universal image is a little over twice the size of the one a given user can run,
and every user downloads a slice for a machine they do not own. Packages are therefore
built per architecture. Two further consequences worth knowing before anyone reaches
for it:

- the universal build writes to `target/universal-apple-darwin/release/bundle/dmg/`,
  which the CI artefact glob deliberately does not match;
- it needs both `rustup` targets installed and compiles the whole dependency tree twice.

## Pdfium in the bundle

The bundle ships Pdfium, and `scripts/fetch-pdfium.sh` is therefore a prerequisite of
`cargo tauri build` and not only of `cargo test`. Three things had to be settled to get
there, and each of them was measured wrong first.

**Where it lands.** `bundle.resources` in `src-tauri/tauri.conf.json`:

```
Mnema.app/Contents/Resources/pdfium/VERSION
Mnema.app/Contents/Resources/pdfium/lib/libpdfium.dylib
```

The vendored layout reproduced, not flattened, because `verify_build` requires the
`VERSION` manifest in the library's directory or its parent — so `pdfium/lib/…` puts the
manifest at `pdfium/VERSION` with nothing else in that directory to collide with. The
alternative, `bundle.macOS.frameworks`, lands the library at
`Contents/Frameworks/libpdfium.dylib` and is unusable for exactly that reason: it accepts
only `.dylib` and `.framework` entries, so the manifest cannot go there at all.

One trap in the map syntax: the value is a destination **path**, not a directory, so
`{"…/libpdfium.dylib": "pdfium/"}` renames the library to `pdfium`. Two entries sharing a
first path component in that shortened form fail the build outright with
`File exists (os error 17)` from `tauri-build`. Three entries whose values are complete
file paths — which is what is configured — do not.

**And it lands under a different name on every platform**, which is the second trap and
cost a whole CI leg. `bundle.resources` is not a delivery-time setting: `tauri-build`
validates every declared path from inside the build script, in every profile, so a
resource named for one platform stops `cargo check` on the others. The base file names
macOS's `.dylib` while `scripts/fetch-pdfium.sh` stages `lib/libpdfium.so` on Linux and
`lib/pdfium.dll` under Git Bash, and `src-tauri` therefore did not compile on either:

```
resource path `../vendor/pdfium/lib/libpdfium.dylib` doesn't exist
```

The fix is Tauri's platform-config merge, and the mechanism is worth stating because the
usual reading of it is wrong. `tauri-build` reads the configuration through
`tauri_utils::config::parse::read_from(Target::from_triple(&triple), …)`, which merges
`tauri.<platform>.conf.json` over `tauri.conf.json` as an **RFC 7396 merge patch** — so
the platform file *adds* keys rather than replacing the map, and the only way to drop the
`.dylib` entry is to give it the value `null`. That is what `src-tauri/tauri.linux.conf.json`
and `src-tauri/tauri.windows.conf.json` each do, before adding their own library. Neither
platform is bundled; both have to compile.

`tauri.conf.json` is deliberately left alone rather than emptied and split three ways: it
is the file the signed image was verified against, and a `tauri.macos.conf.json` beside it
would move macOS onto a merged configuration nothing has measured.
`src-tauri/tests/vendored_library_resource.rs` fails if one appears, and is also what
holds the other two in step with the fetch script — it reads every platform's effective
configuration through the same `read_from`, from whatever host it runs on, so this class
of defect no longer waits for a CI leg to be reached. It was found by the first Linux job
that ever completed on the branch that introduced it, three commits late.

**How it is found.** `mnema_extract::library_dir` looks in `$MNEMA_PDFIUM_LIB_DIR`, then
beside the running executable, then in `Contents/Resources/pdfium/lib` derived from the
executable's directory, then in the development `vendor/` tree. The last branch is an
absolute path baked in at compile time, and it is why the bundle check does not stop at
"the worker read a PDF": the first bundle built after the PDF reader landed reached that
branch and read the *developer's checkout*. It would have read PDFs on one machine on
earth. `--probe-pdfium` reports the directory the library was actually loaded from and
`scripts/verify-bundle.sh` compares it against the mounted image.

**Why it loads at all.** `src-tauri/entitlements.plist` grants
`com.apple.security.cs.disable-library-validation`, and without it the product reads no
PDFs anywhere. The bundler signs every binary ad-hoc with the hardened runtime
(`flags=0x10002(adhoc,runtime)`, `TeamIdentifier=not set`), and library validation then
refuses the `dlopen`:

```
code signature in <…> '…/libpdfium.dylib' not valid for use in process:
mapping process and mapped file (non-platform) have different Team IDs
```

Measured on this repository's own image, twice: refused without the entitlement, and
`{"loaded":true,"pages":1,"stage":"ok"}` with it. A Developer ID would sign worker and
library under one Team ID and need no entitlement; that was declined for v1 (D54), so this
is the cost of ad-hoc signing rather than a preference.

⚠️ **`bundle.macOS.entitlements` does reach `externalBin`** — measured, because it is the
kind of thing that would be reasonable either way. The worker is the process that loads
the library, and `codesign -d --entitlements -` on the bundled worker shows the key. No
separate `codesign` call is needed in `scripts/stage-sidecar.sh`, and one there would be
overwritten anyway: the bundler re-signs the sidecar in place.

⚠️ And the trap next to it: `codesign --sign - --force --deep` on a copy of the .app
**drops both the hardened runtime and the entitlements** (`0x10002(adhoc,runtime)` becomes
`0x2(adhoc)`). A re-signed copy then loads Pdfium happily — because library validation is
no longer being enforced, not because the entitlement is spare. Every lab bundle in
`scripts/verify-bundle-controls.sh` is in that state; the shipped configuration is
reproduced only by the real image and by control 17.

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

That container run predates `bundle.resources`, and the gap showed: it is what made the
Linux arm look covered while the arm that would have caught the `.dylib` declaration had
not run. See "Pdfium in the bundle" above for what the declaration now looks like per
platform.

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

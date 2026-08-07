//! The bundled Pdfium is declared under one platform's filename, and a script
//! installs it under another's.
//!
//! `bundle.resources` in `tauri.conf.json` made `vendor/pdfium` a **compile-time**
//! requirement of this crate rather than a run-time one: `tauri-build` validates
//! every declared resource from inside the build script, in every profile, and a
//! path that does not exist stops the build:
//!
//! ```text
//! error: failed to run custom build command for `mnema-desktop v0.0.0`
//!   resource path `../vendor/pdfium/lib/libpdfium.dylib` doesn't exist
//! ```
//!
//! `scripts/fetch-pdfium.sh` installs `lib/libpdfium.dylib` on macOS,
//! `lib/libpdfium.so` on Linux and `lib/pdfium.dll` under Git Bash. The
//! declaration named the first of the three, so `src-tauri` could not compile on
//! the other two at all — not the tests, not clippy, not `cargo check`.
//!
//! # Why a test, when CI builds on Linux anyway
//!
//! Because CI reaching that leg is a coincidence, and here it did not happen for
//! three commits: the first Linux run died in GitHub infrastructure, the second
//! was cancelled in a queue during an Actions outage, and the branch collected
//! sixteen tasks' worth of green macOS runs in between. The defect was found by
//! the first Linux job that ever executed on this branch.
//!
//! Windows is the sharper half of the same point, and it is why this file reads
//! all three platforms rather than the one that broke. `fetch-pdfium.sh` pins a
//! Windows archive on purpose — "*this pin exists so a Windows machine can run
//! the test suite*" — and **no** workflow builds Windows. A fix that repaired
//! Linux alone would have left the identical defect on the one platform where
//! nothing but a person can find it.
//!
//! # What it does, and why that is host-independent
//!
//! It asks `tauri_utils` for the configuration of each target *by name*, which is
//! the same call `tauri-build` makes — `read_from(Target::from_triple(&triple),
//! …)` at tauri-build-2.6.3/src/lib.rs:483 — with the target chosen explicitly
//! instead of taken from the triple. So a macOS machine can see exactly what the
//! Linux build script will see, including the platform-file merge and the
//! `deny_unknown_fields` deserialisation that would otherwise only ever run on a
//! Linux runner.
//!
//! It then compares that against what `fetch-pdfium.sh` installs on the same
//! platform, read out of the script's own `case` arms. Neither side is written
//! down twice: the check is that the two agree.
//!
//! # What it does not do
//!
//! It does not build anything on Linux or Windows, and it is not evidence that
//! either builds. It closes one specific hole — a declaration naming a file the
//! installer never produces — which is the hole that was open.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tauri_utils::config::parse::read_from;
use tauri_utils::config::{BundleResources, Config};
use tauri_utils::platform::Target;

/// Where the vendored library lands inside the bundle, minus the file name.
///
/// `crates/mnema-extract/src/pdfium_probe.rs` resolves the packaged library by
/// walking to `Contents/Resources/pdfium/lib`, and prefers it over a copy beside
/// the executable — the ordering task 16 verified on a signed image, and the one
/// thing this file must not disturb. The destination is therefore derived from
/// the source below rather than restated, so a platform config cannot quietly
/// file its library somewhere the probe does not look.
const BUNDLE_PREFIX: &str = "pdfium/";

/// Where the vendored tree sits, as `tauri.conf.json` spells it: relative to
/// `src-tauri/`, which is the directory the build script runs in.
const VENDOR_PREFIX: &str = "../vendor/pdfium/";

/// The three platforms `scripts/fetch-pdfium.sh` pins an archive for, spelled the
/// way `Target`'s own `Display` spells them.
///
/// Enumerated rather than derived from the script, and that is the point: a
/// platform added to the script and not here would otherwise be checked by
/// nothing, which is the shape of the defect this file exists for. Adding a
/// platform to the script must therefore mean adding it here, and the assertion
/// in [`the_script_pins_a_library_for_every_platform_this_file_checks`] fails
/// loudly rather than passing over an arm it does not recognise.
const PLATFORMS: [(&str, Target); 3] = [
    ("macOS", Target::MacOS),
    ("linux", Target::Linux),
    ("windows", Target::Windows),
];

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri sits inside the workspace root")
}

fn src_tauri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// The `uname -s` family a `case` pattern selects, as a `Target` display name.
///
/// `None` is not "some other platform" — it is "this file does not know", and
/// every caller treats it as a failure. The Windows arm carries three patterns
/// because bash reaches that script on Windows only through Git Bash, MSYS2 or
/// Cygwin, each of which appends a version to its family name.
fn family(pattern: &str) -> Option<&'static str> {
    let os = pattern.trim().trim_matches('"').split('/').next()?.trim();
    match os {
        "Darwin" => Some("macOS"),
        "Linux" => Some("linux"),
        _ if os.starts_with("MINGW") || os.starts_with("MSYS") || os.starts_with("CYGWIN") => {
            Some("windows")
        }
        _ => None,
    }
}

/// Every `(platform, library)` pair `scripts/fetch-pdfium.sh` installs, read out
/// of the `case` arms that set `library=`.
///
/// Read rather than restated. A copy of these three file names kept here would
/// drift from the script exactly the way the configuration drifted from it, and
/// a test that agrees with its own copy of the answer is the thing being
/// replaced, not a check on it.
fn installed_libraries() -> Vec<(&'static str, String)> {
    let path = repo_root().join("scripts/fetch-pdfium.sh");
    let script = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));

    let mut pairs = Vec::new();
    // The patterns of the `case` arm currently open. `None` between arms and for
    // the `*)` fallback, which sets no library — so a stale arm cannot lend its
    // platform to a line that is no longer inside it.
    let mut arm: Option<Vec<String>> = None;

    for line in script.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(patterns) = line.strip_suffix(')') {
            arm = patterns
                .contains('/')
                .then(|| patterns.split('|').map(|p| p.trim().to_string()).collect());
            continue;
        }
        let Some(value) = line.strip_prefix("library=") else {
            continue;
        };
        let library = value.trim().trim_matches('"').to_string();
        let patterns = arm.as_ref().unwrap_or_else(|| {
            panic!(
                "{} sets `library={library}` outside any `case` arm, so which \
                 platform installs it is unanswered — and an unanswered platform \
                 is what this file exists to refuse.",
                path.display()
            )
        });
        for pattern in patterns {
            let platform = family(pattern).unwrap_or_else(|| {
                panic!(
                    "{} pins `library={library}` for `{pattern}`, a platform \
                     PLATFORMS in {} does not name. Skipping it would leave that \
                     platform's `bundle.resources` checked by nothing, which is \
                     precisely how `libpdfium.dylib` came to be declared for \
                     Linux. Add it to both, or say here why it needs no bundle \
                     resource.",
                    path.display(),
                    file!()
                )
            });
            pairs.push((platform, library.clone()));
        }
    }
    pairs
}

/// The one library `scripts/fetch-pdfium.sh` installs on this platform.
///
/// Panics when a platform pins two different file names, rather than picking
/// one. That would make "the declaration matches the script" ambiguous, and an
/// ambiguous invariant satisfied by whichever value sorted first is the failure
/// mode, not the report.
fn installed_library(platform: &str) -> String {
    let pairs = installed_libraries();
    let mut names: Vec<&str> = pairs
        .iter()
        .filter(|(p, _)| *p == platform)
        .map(|(_, library)| library.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    match names.as_slice() {
        [one] => (*one).to_string(),
        [] => panic!(
            "scripts/fetch-pdfium.sh pins no library for {platform}, so there is \
             nothing for its `bundle.resources` to agree with. Either the script \
             stopped supporting it or the arm no longer parses — and the second \
             would make every assertion below vacuous."
        ),
        many => panic!(
            "scripts/fetch-pdfium.sh installs more than one library name on \
             {platform}: {many:?}. `bundle.resources` can only name one, so which \
             one it should name is now a decision rather than a lookup."
        ),
    }
}

/// The configuration `tauri-build` will see for this target, obtained the way it
/// obtains it.
///
/// Deserialising into `Config` is half the value and is easy to skip: it is
/// `deny_unknown_fields`, so a mistyped key in `tauri.linux.conf.json` is a hard
/// error here — on a macOS developer's machine — instead of a Linux-only build
/// failure nobody sees until a runner reaches it. That is the same class of hole
/// as the one being fixed.
fn effective_config(target: Target) -> (Config, Vec<PathBuf>) {
    let (value, paths) = read_from(target, &src_tauri())
        .unwrap_or_else(|e| panic!("reading the {target} configuration failed: {e}"));
    let config = serde_json::from_value(value)
        .unwrap_or_else(|e| panic!("the merged {target} configuration is not a valid Config: {e}"));
    (config, paths)
}

fn resources(config: &Config, target: Target) -> &HashMap<String, String> {
    match config.bundle.resources.as_ref() {
        Some(BundleResources::Map(map)) => map,
        Some(BundleResources::List(list)) => panic!(
            "the {target} configuration declares `bundle.resources` as a list \
             ({list:?}). The list form copies each file to a path derived from \
             its own, so it cannot put the library under `{BUNDLE_PREFIX}lib/` — \
             where `pdfium_probe.rs` looks, and where the bundle task verified it \
             on a signed image."
        ),
        None => panic!(
            "the {target} configuration declares no `bundle.resources` at all. \
             The vendored Pdfium then ships with nothing, and the worker falls \
             back to a copy beside the executable or to the development \
             checkout — which is the state a signed image was measured in before \
             the library was bundled."
        ),
    }
}

#[test]
fn every_platform_declares_the_library_its_installer_installs() {
    for (platform, target) in PLATFORMS {
        let library = installed_library(platform);
        let (config, _) = effective_config(target);
        let declared = resources(&config, target);

        let under_lib: Vec<(&String, &String)> = declared
            .iter()
            .filter(|(source, _)| source.starts_with(&format!("{VENDOR_PREFIX}lib/")))
            .collect();

        let (source, destination) = match under_lib.as_slice() {
            [one] => *one,
            [] => panic!(
                "the {target} configuration declares no file under \
                 `{VENDOR_PREFIX}lib/`, but scripts/fetch-pdfium.sh installs \
                 `{library}` there for {platform}. A bundle built from this \
                 configuration carries no Pdfium.\nDeclared: {declared:?}"
            ),
            many => panic!(
                "the {target} configuration declares {} files under \
                 `{VENDOR_PREFIX}lib/`: {many:?}. scripts/verify-bundle.sh \
                 refuses an image containing two, so this cannot be built into a \
                 bundle that passes.",
                many.len()
            ),
        };

        assert_eq!(
            source,
            &format!("{VENDOR_PREFIX}{library}"),
            "the {target} configuration declares `{source}`, but \
             scripts/fetch-pdfium.sh installs `{VENDOR_PREFIX}{library}` on \
             {platform}. `tauri-build` validates declared resources from inside \
             the build script, so this is not a packaging defect that shows up at \
             bundle time — src-tauri does not compile on {platform} at all:\n  \
             resource path `{source}` doesn't exist"
        );

        let expected_destination = format!(
            "{BUNDLE_PREFIX}{}",
            source
                .strip_prefix(VENDOR_PREFIX)
                .expect("the source was matched on this prefix above")
        );
        assert_eq!(
            destination, &expected_destination,
            "the {target} configuration installs the library into the bundle at \
             `{destination}` rather than `{expected_destination}`. \
             `pdfium_probe.rs` resolves the packaged library by walking to \
             `Contents/Resources/{BUNDLE_PREFIX}lib`, and prefers that copy over \
             one beside the executable — the ordering a signed image was measured \
             against. A library filed elsewhere ships without being found, and \
             the search falls through to the branch that ignores the bundle."
        );

        for manifest in ["VERSION", "LICENSE"] {
            assert_eq!(
                declared.get(&format!("{VENDOR_PREFIX}{manifest}")),
                Some(&format!("{BUNDLE_PREFIX}{manifest}")),
                "the {target} configuration does not ship \
                 `{VENDOR_PREFIX}{manifest}` at `{BUNDLE_PREFIX}{manifest}`. The \
                 worker verifies the build number in `VERSION` against the API \
                 revision its bindings were compiled for; without the file it \
                 cannot tell a matching library from one whose struct layouts \
                 have drifted.\nDeclared: {declared:?}"
            );
        }
    }
}

#[test]
fn the_script_pins_a_library_for_every_platform_this_file_checks() {
    // Without this, the loop above is satisfied by a parser that returns nothing:
    // `installed_libraries` failing to recognise a single `case` arm would make
    // every platform report "no library", and `installed_library` would panic —
    // but only if it is called, and only for platforms PLATFORMS happens to name.
    // This asserts the other direction, that the script's arms and this file's
    // list cover each other, so neither can shrink quietly.
    let pairs = installed_libraries();
    assert!(
        !pairs.is_empty(),
        "no `case` arm in scripts/fetch-pdfium.sh sets a library. Either the \
         script changed shape or the parser above stopped working — and in both \
         cases every check in this file is being satisfied by an empty list."
    );

    for (platform, _) in PLATFORMS {
        let library = installed_library(platform);
        assert!(
            library.starts_with("lib/") && library.contains("pdfium"),
            "scripts/fetch-pdfium.sh installs `{library}` on {platform}, which is \
             not a Pdfium library under `lib/`. The vendored layout is one shape \
             for every platform — the Windows archive's `bin/` is renamed to \
             `lib/` on install for exactly this reason — and the configuration \
             checked above assumes it."
        );
    }
}

#[test]
fn macos_is_configured_by_the_base_file_alone() {
    // The macOS bundle is the one that was verified end to end: a signed image,
    // opened, with the worker asked where it loaded Pdfium from. That evidence
    // is about the configuration in `tauri.conf.json`, and a `tauri.macos.conf.json`
    // appearing beside it would silently move macOS onto a merged configuration
    // nothing has measured. This does not forbid that file — it requires that
    // adding it comes with a rebuilt image and a fresh run of
    // scripts/verify-bundle.sh, by failing until someone reads this.
    let (_, paths) = effective_config(Target::MacOS);
    let names: Vec<String> = paths
        .iter()
        .map(|p| p.file_name().unwrap_or_default().to_string_lossy().into())
        .collect();
    assert_eq!(
        names,
        vec!["tauri.conf.json".to_string()],
        "macOS is no longer configured by tauri.conf.json alone; it now merges \
         {names:?}. The bundle guarantee — Pdfium loaded from inside the image, \
         preferred over a copy beside the executable — was measured against the \
         unmerged file. Rebuild the image, run scripts/verify-bundle.sh, and \
         then update this test."
    );

    // The mirror image, and it is what makes the assertion above a statement
    // about macOS rather than about `read_from` returning one path for everyone.
    let (_, linux_paths) = effective_config(Target::Linux);
    assert!(
        linux_paths.len() > 1,
        "the Linux configuration is not merging a platform file, so \
         `read_from` is returning the base configuration for every target and \
         the check above proves nothing. Linux would then be declaring macOS's \
         `.dylib` again."
    );
}

//! Proves the pdfium binding, not the PDF extractor.
//!
//! Three things have to hold before any extraction spec is worth writing: the
//! library loads, the binary that loads is the one the compiled bindings were
//! generated against, and a page of text comes back with the characters intact.
//!
//! The fixtures are built by `tests/fixtures/make_fixtures.py`. Their content is
//! invented; the character counts asserted below come from that script's output
//! and change only when the script changes.

use std::path::{Path, PathBuf};

use mnema_extract::{PDFIUM_API_BUILD, TEXT_LAYER_MIN_CHARS, probe_text_layer};

/// Non-whitespace characters drawn on the body page of both fixtures.
const BODY_PAGE_CHARS: usize = 119;

/// Non-whitespace characters in the stamp on page 2 of `text-then-stamp.pdf`.
/// Above zero and far below the threshold — that gap is the whole point.
const STAMP_PAGE_CHARS: usize = 8;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn a_page_with_a_text_layer_is_detected() {
    let probes = probe_text_layer(&fixture("one-page-text.pdf"))
        .expect("pdfium binds and the fixture parses");

    assert_eq!(probes.len(), 1);
    // An exact count, not `> 0`: a binding that loads but reads the wrong struct
    // layout still returns *something*, and "non-empty" would accept it.
    assert_eq!(probes[0].char_count, BODY_PAGE_CHARS);
    assert!(probes[0].char_count > TEXT_LAYER_MIN_CHARS);
    assert!(probes[0].has_text_layer);
    assert_eq!(probes[0].page_no, 1);
}

#[test]
fn the_text_layer_test_is_a_threshold_not_a_non_zero_check() {
    // A scanned page routinely carries a stamp or a scanner footer. Counting
    // characters > 0 would index it as though it had content, which the
    // requirements call the worst available behaviour. G7.0 §8.1.
    //
    // A `const` block, because the claim is about a constant and clippy is right
    // that a runtime assertion on one is theatre. This way lowering the threshold
    // fails the build rather than a test run.
    const {
        assert!(
            TEXT_LAYER_MIN_CHARS >= 40,
            "a handful of characters is a stamp, not a text layer"
        )
    };
}

#[test]
fn a_page_carrying_only_a_stamp_has_no_text_layer() {
    // The behavioural half of the test above. The constant being large is not the
    // same claim as the code comparing against it: this fails if the comparison
    // ever becomes `char_count > 0`.
    let probes = probe_text_layer(&fixture("text-then-stamp.pdf")).expect("the fixture parses");

    let stamp = &probes[1];
    assert_eq!(stamp.char_count, STAMP_PAGE_CHARS);
    assert!(
        stamp.char_count > 0,
        "the stamp page must carry text, or this test proves nothing"
    );
    assert!(!stamp.has_text_layer);
}

#[test]
fn pages_are_reported_in_document_order_and_numbered_from_one() {
    let probes = probe_text_layer(&fixture("text-then-stamp.pdf")).expect("the fixture parses");

    assert_eq!(probes.len(), 2);
    assert_eq!(
        probes.iter().map(|p| p.page_no).collect::<Vec<_>>(),
        vec![1, 2]
    );
    // The two pages carry different amounts of text, so a reversed or repeated
    // iteration cannot pass this.
    assert_eq!(
        probes.iter().map(|p| p.char_count).collect::<Vec<_>>(),
        vec![BODY_PAGE_CHARS, STAMP_PAGE_CHARS]
    );
    assert_eq!(
        probes.iter().map(|p| p.has_text_layer).collect::<Vec<_>>(),
        vec![true, false]
    );
}

#[test]
fn a_corrupt_file_returns_an_error_rather_than_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("broken.pdf");
    std::fs::write(&bad, b"%PDF-1.4\nthis is not a pdf").unwrap();

    let err = probe_text_layer(&bad).expect_err("a truncated file is not a document");

    // Naming the reason, not just demanding an error. Pdfium's bindings are a
    // process-wide singleton, and an implementation that binds per call returns
    // `PdfiumLibraryBindingsAlreadyInitialized` on every call after the first —
    // an error, so `is_err()` is satisfied, for a reason that has nothing to do
    // with the file being broken. Asserting the cause is what separates the two.
    //
    // `PdfiumError`'s `Display` is its `Debug`, so these are variant names. That
    // is only a safe thing to match on because the crate version is pinned exactly.
    let message = err.to_string();
    assert!(
        message.contains("FormatError"),
        "expected pdfium to reject the file's format, got: {message}"
    );

    // And the failure must not have poisoned the library for the next document.
    let probes = probe_text_layer(&fixture("one-page-text.pdf"))
        .expect("a good document still probes after a bad one");
    assert!(probes[0].has_text_layer);
}

#[test]
fn the_crate_root_error_type_is_the_one_probe_text_layer_actually_returns() {
    // Regression: a prior commit re-exported `text::Error` (then a zero-variant
    // enum) as the crate-root `Error`, while `probe_text_layer` returns
    // `pdfium_probe::Error` — a value whose type nothing outside this crate
    // could name or match on. This test would fail to *compile* under that
    // commit: `mnema_extract::Error` and the type `err` actually holds would
    // be two distinct enums, and a `match` naming this one's variants against
    // a value of the other would not type-check.
    let err = probe_text_layer(Path::new("/nonexistent/mnema/absent.pdf"))
        .expect_err("a path that does not exist is not a document");

    match err {
        mnema_extract::Error::Pdfium(_) | mnema_extract::Error::Library(_) => {}
        mnema_extract::Error::BuildMismatch { .. } => {
            panic!("a missing file is not a build mismatch")
        }
    }
}

#[test]
fn a_missing_file_returns_an_error() {
    let err = probe_text_layer(Path::new("/nonexistent/mnema/absent.pdf"))
        .expect_err("a path that does not exist is not a document");
    let message = err.to_string();
    assert!(
        message.contains("NotFound"),
        "expected the missing path to surface as an I/O error, got: {message}"
    );
}

/// The worker binary, not `probe_text_layer` directly: the question this
/// answers is whether the *bundled* worker — running under whatever code
/// signature and library placement packaging gives it — can load Pdfium at
/// all, which the wire protocol has no way to ask (D53, D54).
#[test]
fn the_worker_reports_whether_pdfium_loaded() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mnema-extract-worker"))
        .arg("--probe-pdfium")
        .arg("tests/fixtures/one-page-text.pdf")
        .output()
        .expect("the worker binary starts");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line = String::from_utf8(out.stdout).expect("stdout is UTF-8");
    let v: serde_json::Value = serde_json::from_str(line.trim()).expect("one JSON line");
    // Both directions: it must say it loaded AND report pages. A probe that
    // answers `{"loaded":true}` with no page count proves nothing about the
    // binding — it proves the flag was parsed.
    assert_eq!(v["loaded"], serde_json::json!(true));
    assert!(v["pages"].as_u64().expect("a page count") > 0);
}

#[test]
fn concurrent_probes_do_not_crash_the_process() {
    // Pdfium is not thread-safe, and `pdfium-render`'s `thread_safe` feature does
    // not make it so — in 0.9.3 that feature adds `Send`/`Sync` impls and no
    // serialisation, despite a README that describes a mutex. Before
    // `mnema-extract` added its own lock, this crate's own test binary died with
    // SIGSEGV, because the harness runs tests on several threads.
    //
    // This test fails by killing the process rather than by failing an assertion.
    // That is the honest shape of the failure it guards against.
    let threads: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                for _ in 0..10 {
                    let probes = probe_text_layer(&fixture("text-then-stamp.pdf"))
                        .expect("the fixture parses on every thread");
                    assert_eq!(
                        probes.iter().map(|p| p.char_count).collect::<Vec<_>>(),
                        vec![BODY_PAGE_CHARS, STAMP_PAGE_CHARS],
                        "a concurrent probe read a different page than it loaded"
                    );
                }
            })
        })
        .collect();

    for thread in threads {
        thread.join().expect("no probing thread panicked");
    }
}

// The build number has to be the same in four places, and each pair of them is
// held together by something that fails when they separate:
//
//   Cargo.toml feature  ─┐
//   PDFIUM_API_BUILD    ─┼─ the two tests below, which read the files
//   fetch-pdfium.sh     ─┘
//   the vendored binary ─── verify_build() at load time, against VERSION
//
// The feature link is the one that decides struct layout and the one that no code
// can observe: a feature is a compile-time choice inside a dependency, invisible
// to `cfg!` here. Asserting the constant against a literal, which is what this
// file used to do, checks none of that — it catches an edit to the constant alone
// and nothing about the feature or the script, while its name claims the chain.
// Reading the two files is what actually closes it.
//
// Nothing here asserts what the number *is*. Moving all four together is a version
// bump, and a bump should not have to defeat a test; `PDFIUM_API_BUILD` is the one
// place the number is written, and everything else is derived from it.
//
// Both tests match a whole line, not a substring. Searching for the number
// anywhere in the file is satisfied by a comment that merely mentions it — and a
// comment mentioning the old number is exactly what a person writes while moving
// a pin, so a substring guard goes quiet in the one situation it exists for.

fn repo_file(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{relative} is part of this crate's version pin: {e}"))
}

#[test]
fn the_fetch_script_pins_the_build_the_bindings_declare() {
    let script = repo_file("scripts/fetch-pdfium.sh");
    let pin = format!("PDFIUM_BUILD={PDFIUM_API_BUILD}");
    assert!(
        script.lines().any(|line| line.trim() == pin),
        "scripts/fetch-pdfium.sh has no line `{pin}`. It would vendor a Pdfium build \
         other than the one these bindings are compiled for, which reads moved \
         struct layouts and returns plausible nonsense instead of failing. \
         (The assignment must stand on its own line; a mention inside a comment \
         does not count, and is not meant to.)"
    );
}

/// Asserts the document contains this exact line, trimmed.
///
/// A whole row, not a cell: the row is what a reader sees, and half a correct row
/// is not a correct answer to "which release do I fetch". Reword a label here and
/// the test says so, with the row it wants.
///
/// `derived_from` names where the expected text came from, and the caller passes it
/// because the rows do not share an answer: two derive from a constant in this
/// crate, one from the dependency pin in `Cargo.toml`. A single sentence covering
/// both would be wrong for one of them — and sending someone who just edited the
/// manifest off to look at a constant is exactly the misdirection these tests
/// exist to prevent, committed inside the guard against it.
///
/// `document` is a parameter for the same reason. Two files now carry this table —
/// this crate's README and `docs/BUILD.md` — and a message that names the wrong
/// file is the same defect one step coarser: it sends someone to edit a document
/// that was already right.
fn assert_table_row(document: &str, contents: &str, expected: &str, derived_from: &str) {
    assert!(
        contents.lines().any(|line| line.trim() == expected),
        "{document} has no row\n\n    {expected}\n\n\
         That table is among the first things anyone reads to learn which Pdfium \
         release to fetch, so a stale value in it sends people to the wrong binary \
         while every other check stays green. The expected row is derived from \
         {derived_from} — if that moved, move this row; if a label was reworded, \
         reword it here too."
    );
}

/// The `pdfium-render` version `crates/mnema-extract/Cargo.toml` pins, read from the
/// manifest rather than written down here: this file is not one of the places the
/// pin is allowed to live.
fn pinned_pdfium_render() -> String {
    repo_file("crates/mnema-extract/Cargo.toml")
        .lines()
        .find_map(|line| line.trim().strip_prefix("pdfium-render = { version = \""))
        .and_then(|rest| rest.split('"').next())
        .expect(
            "Cargo.toml should pin pdfium-render as `pdfium-render = { version = \"…\"` \
             with the version on that line",
        )
        .to_string()
}

#[test]
fn the_readme_table_names_the_build_the_bindings_declare() {
    const README: &str = "crates/mnema-extract/README.md";
    let readme = repo_file(README);
    let source = "PDFIUM_API_BUILD in src/pdfium_probe.rs";
    assert_table_row(
        README,
        &readme,
        &format!("| crate feature selecting the C API | `pdfium_{PDFIUM_API_BUILD}` |"),
        source,
    );
    assert_table_row(
        README,
        &readme,
        &format!("| Pdfium binary, non-V8 | `chromium/{PDFIUM_API_BUILD}` |"),
        source,
    );
}

#[test]
fn the_readme_table_names_the_crate_version_the_manifest_pins() {
    // Not part of the build-number chain, but the same defect in the same table:
    // the row a reader trusts for "which pdfium-render" can go stale on its own.
    const README: &str = "crates/mnema-extract/README.md";
    assert_table_row(
        README,
        &repo_file(README),
        &format!("| `pdfium-render` | `{}` |", pinned_pdfium_render()),
        "the pdfium-render version pinned in crates/mnema-extract/Cargo.toml",
    );
}

#[test]
fn the_build_document_names_the_same_pair() {
    // docs/BUILD.md is the sixth place this number is written down and the one a
    // packager reads first — it answers "which release do I fetch" for someone who
    // never opens this crate. Writing the pair there without binding it here would
    // add exactly the kind of prose the rest of this file exists to stop: correct
    // on the day it was written, silently wrong after the next bump, and believed
    // because every other check is green.
    //
    // It repeats only two of the five rows. The chain itself is documented in this
    // crate's README, and duplicating the other three would be adding places for a
    // stale number to hide in order to guard against stale numbers.
    const BUILD_DOC: &str = "docs/BUILD.md";
    let doc = repo_file(BUILD_DOC);
    assert_table_row(
        BUILD_DOC,
        &doc,
        &format!("| `pdfium-render` | `{}` |", pinned_pdfium_render()),
        "the pdfium-render version pinned in crates/mnema-extract/Cargo.toml",
    );
    assert_table_row(
        BUILD_DOC,
        &doc,
        &format!("| Pdfium binary, non-V8 | `chromium/{PDFIUM_API_BUILD}` |"),
        "PDFIUM_API_BUILD in crates/mnema-extract/src/pdfium_probe.rs",
    );
}

/// Every archive the fetch script is willing to install, read from the script.
///
/// The `asset=` assignments are the list. Nothing else in the file assigns that
/// name, and taking it from the script rather than from a literal here is the
/// same rule the build-number tests follow: this file is not one of the places
/// the platform list is allowed to live.
fn pinned_assets() -> Vec<String> {
    let mut assets: Vec<String> = repo_file("scripts/fetch-pdfium.sh")
        .lines()
        .filter_map(|line| line.trim().strip_prefix("asset=\"").map(str::to_string))
        .filter_map(|rest| rest.split('"').next().map(str::to_string))
        .collect();
    assets.sort();
    assets.dedup();
    assert!(
        !assets.is_empty(),
        "scripts/fetch-pdfium.sh has no `asset=\"…\"` assignment. Either every \
         platform pin was removed, or the assignments were reshaped and this \
         reader now sees nothing — which would make the test below pass by \
         finding nothing to disagree about."
    );
    assets
}

#[test]
fn the_readme_names_exactly_the_platforms_the_script_pins() {
    // A prose sentence naming platforms is the one place in this chain nothing
    // was reading, and it drifted: the README said "Only macOS arm64 and x86-64
    // have pinned checksums" for as long as the two Linux pins existed beside
    // it, while docs/BUILD.md described those same Linux pins on the next page.
    // Two documents disagreeing about which platforms work is worse than either
    // being silent — a reader on a third platform believes the wrong one and
    // concludes the archive does not exist.
    //
    // Set equality, not containment, and deliberately: the failure that actually
    // happened is a document falling *behind* the script, and a containment
    // check in the forgiving direction is exactly the check that stays green
    // through it.
    const README: &str = "crates/mnema-extract/README.md";
    let readme = repo_file(README);

    let mut documented: Vec<String> = readme
        .split('`')
        .filter(|token| token.starts_with("pdfium-") && token.ends_with(".tgz"))
        .map(str::to_string)
        .collect();
    documented.sort();
    documented.dedup();

    assert_eq!(
        documented,
        pinned_assets(),
        "{README} and scripts/fetch-pdfium.sh disagree about which platforms have \
         pinned archives. The README is what someone reads before concluding \
         their platform is unsupported, so an archive missing from it is an \
         archive nobody fetches, and one listed but unpinned is a script that \
         refuses to run after the page promised it would. Adding a platform \
         means adding its row here as well as its `asset=` and checksum there."
    );
}

#[test]
fn the_manifest_selects_the_api_feature_the_bindings_declare() {
    let manifest = repo_file("crates/mnema-extract/Cargo.toml");
    let feature = format!("\"pdfium_{PDFIUM_API_BUILD}\"");
    assert!(
        manifest
            .lines()
            .any(|line| line.trim().trim_end_matches(',') == feature),
        "Cargo.toml has no feature-list entry {feature}. That feature is what fixes \
         the C API the bindings are generated from, and nothing at run time can \
         observe it — if it drifts from PDFIUM_API_BUILD, the binary that loads is \
         not the one the bindings describe and no check downstream notices. \
         (The entry must stand on its own line, one feature per line, which is how \
         the dependency is written; a mention inside a comment does not count.)"
    );
}

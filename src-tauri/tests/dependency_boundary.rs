//! One dependency the application must not have, checked rather than promised.
//!
//! D40 and D35 place the Pdfium binding inside the extraction worker and nowhere
//! else: *"`mnema-extract` is linked only into the worker binary, never into the
//! application, which satisfies D35's demand **structurally** rather than by
//! convention — if the crate is absent from the application there is no FFI to
//! reach around the mutex."* Structurally is the load-bearing word. Until this
//! file existed the claim was made in three doc comments and enforced by nothing.
//!
//! Nothing would go red the day it broke, either. The one place in the repository
//! that reasons about Pdfium and the bundle, `scripts/verify-bundle.sh`, has no
//! opinion on this graph at all: it derives whether the library belongs in the
//! bundle from the worker's own stated verdict when asked to read a PDF, never
//! from a dependency tree (D54). A commit that gave the application a path to the
//! PDF library would not move that verdict — the worker is a separate binary —
//! so the check would go on passing, oblivious to exactly the violation this
//! test exists to catch.
//!
//! The mistake is easy to make and invisible afterwards. `mnema-extract` holds
//! the file-typing table, the text decoder and the wire format's original home;
//! any of them is a plausible thing for the application to want, and adding one
//! line to `src-tauri/Cargo.toml` gets it, along with the FFI.

use std::path::Path;
use std::process::Command;

/// The crate that binds Pdfium, the crate that links it, and the parsers that
/// crate pulls in. None may appear in the application's dependency graph.
///
/// `comrak` is on the list although it arrives only through `mnema-extract`
/// today, which the entry above already forbids. That is true of the route it
/// takes now and not of the route someone takes next: a markdown helper added
/// to `mnema-core` would put a parser of untrusted input into the application
/// with this test still green. Naming it costs a string and makes the question
/// asked rather than inferred.
const FORBIDDEN: [&str; 3] = ["pdfium-render", "mnema-extract", "comrak"];

#[test]
fn the_application_does_not_link_the_pdf_library() {
    // `-e normal` excludes dev- and build-dependencies, which is the right
    // question: a dev-dependency is not linked into the shipped binary. `--prefix
    // none` gives one package per line, the same form `scripts/verify-bundle.sh`
    // reads.
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri sits inside the workspace root");
    let output = Command::new(env!("CARGO"))
        .args([
            "tree",
            "-p",
            "mnema-desktop",
            "-e",
            "normal",
            "--prefix",
            "none",
        ])
        .arg("--manifest-path")
        .arg(repo_root.join("Cargo.toml"))
        .output()
        .expect("cargo runs");

    // A failure to answer is not the answer "no". This is the same discipline
    // `verify-bundle.sh` states at length and for the same reason: a renamed
    // package or an unreadable manifest must not read as an absent dependency,
    // and the day this check matters most is the day someone is moving crates
    // around, which is exactly when a package name is most likely to be wrong.
    assert!(
        output.status.success(),
        "cargo tree failed, so whether the application links Pdfium is UNANSWERED, \
         which is not the same as answered no:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tree = String::from_utf8(output.stdout).expect("cargo tree prints UTF-8");
    let packages: Vec<&str> = tree
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert!(
        packages.contains(&"mnema-index"),
        "the tree does not contain a dependency known to be present, so it is not the \
         tree this test means to read:\n{tree}"
    );

    for forbidden in FORBIDDEN {
        assert!(
            !packages.contains(&forbidden),
            "{forbidden} is in the application's dependency graph. D40 puts the Pdfium \
             binding in the worker binary and nowhere else, so that no code in the \
             application can reach the FFI at all. Whatever the application needed from \
             it belongs in mnema-core, the way the wire format does — and note that \
             scripts/verify-bundle.sh will not catch this either: it has no opinion on \
             this graph, only on the worker's own stated verdict about a PDF.\n{tree}"
        );
    }
}

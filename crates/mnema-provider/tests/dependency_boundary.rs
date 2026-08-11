//! Two dependencies this crate must not have, checked rather than promised.
//!
//! `mnema-provider` speaks to a model provider over HTTP and parses what comes
//! back; the design that lets it be exercised without a real network call also
//! means it must be runnable, and testable, without a database and without a
//! credential store. The key arrives as a parameter to whatever Task 2 adds,
//! and the database never arrives at all — the same rule `mnema-chunk` keeps
//! for the chunker (see `crates/mnema-provider/Cargo.toml`). That was a comment
//! and nothing else enforced it: Task 2 legitimately adds a dependency to this
//! crate, and nothing would go red if it added the wrong one.
//!
//! Modelled on `src-tauri/tests/dependency_boundary.rs`: `cargo tree -e normal`
//! so a dev-dependency (this test's own fixtures, for instance) cannot hide a
//! real one or be mistaken for one.

use std::path::Path;
use std::process::Command;

/// Neither may appear in this crate's normal dependency graph.
const FORBIDDEN: [&str; 2] = ["mnema-index", "mnema-secrets"];

#[test]
fn the_provider_crate_reaches_neither_the_database_nor_the_keychain() {
    // `-e normal` excludes dev- and build-dependencies, which is the right
    // question here: a dev-dependency — something this crate's own tests
    // reasonably want, that neither `models_from_json` nor whatever Task 2
    // adds ever compiles against — is not linked into anything that ships, so
    // it must not trip this check. `--prefix none` gives one package per
    // line, the same form the sibling test in `src-tauri` reads.
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/mnema-provider sits two levels under the workspace root");

    let output = Command::new(env!("CARGO"))
        .args([
            "tree",
            "-p",
            "mnema-provider",
            "-e",
            "normal",
            "--prefix",
            "none",
        ])
        .arg("--manifest-path")
        .arg(repo_root.join("Cargo.toml"))
        .output()
        .expect("cargo runs");

    // A failure to answer is not the answer "no" — the same discipline
    // `src-tauri/tests/dependency_boundary.rs` states at length: the day this
    // check matters most is the day someone is moving crates around, which is
    // exactly when a package name or a manifest path is most likely to be
    // wrong.
    assert!(
        output.status.success(),
        "cargo tree failed, so whether mnema-provider reaches the database or the keychain \
         is UNANSWERED, which is not the same as answered no:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tree = String::from_utf8(output.stdout).expect("cargo tree prints UTF-8");
    let packages: Vec<&str> = tree
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();

    assert!(
        packages.contains(&"serde_json"),
        "the tree does not contain a dependency known to be present, so it is not the tree \
         this test means to read:\n{tree}"
    );

    for forbidden in FORBIDDEN {
        assert!(
            !packages.contains(&forbidden),
            "{forbidden} is in mnema-provider's dependency graph. This crate must be \
             runnable, and testable, without a database or a credential store: the key \
             arrives as a parameter and the database never arrives at all. Whatever this \
             crate needed from {forbidden} belongs elsewhere, or belongs to a decision that \
             changes this comment along with the manifest.\n{tree}"
        );
    }
}

//! What the shipped dependency graph must and must not contain, checked rather
//! than promised.
//!
//! Three manifest decisions live here. One is a dependency the application must
//! not have; the other two are things it must, and both of those were enforced
//! by a comment until the graph was asked directly.
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

/// The shipped dependency graph, one package per line as `<package>|<features>`.
///
/// `-e normal` excludes dev- and build-dependencies, which is the right
/// question for every caller here: a dev-dependency is not linked into the
/// shipped binary. `--prefix none` gives one package per line, the same form
/// `scripts/verify-bundle.sh` reads. `{p}` never contains a `|`, so the
/// separator splits the name-and-version from the feature list cleanly, and
/// `split_whitespace().next()` still reads the package name out of the first
/// half exactly as it did before the format gained a second field.
fn shipped_graph() -> String {
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
            "-f",
            "{p}|{f}",
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
        "cargo tree failed, so every question this file asks is UNANSWERED, which is \
         not the same as answered the way it wanted:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("cargo tree prints UTF-8")
}

fn packages_in(tree: &str) -> Vec<&str> {
    tree.lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect()
}

/// The features `cargo` resolved for one package, as they appear in the tree.
///
/// Every occurrence, not the first: a package can appear on many lines, and a
/// helper that read one of them would answer about whichever the tree happened
/// to print first.
fn features_of<'a>(tree: &'a str, package: &str) -> Vec<&'a str> {
    tree.lines()
        .filter(|line| line.split_whitespace().next() == Some(package))
        .filter_map(|line| line.split_once('|'))
        .flat_map(|(_, features)| features.split(',').filter(|f| !f.is_empty()))
        .collect()
}

#[test]
fn the_application_does_not_link_the_pdf_library() {
    let tree = shipped_graph();
    let packages = packages_in(&tree);
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

/// Two things the shipped graph must *have*, both of them decisions that live
/// in a manifest and were, until this test, enforced by nothing.
///
/// This file's older half asks what must be absent. These ask what must be
/// present, and they are here rather than beside the code they are about
/// because a manifest line is not reachable from a unit test — the graph is the
/// only place the answer exists.
#[test]
fn the_shipped_graph_takes_tls_roots_from_the_machine_and_carries_no_test_store() {
    let tree = shipped_graph();
    let packages = packages_in(&tree);
    assert!(
        packages.contains(&"mnema-index"),
        "the tree does not contain a dependency known to be present, so it is not the \
         tree this test means to read:\n{tree}"
    );

    // The stated fact, and then the thing that follows from it — in that order,
    // and both, because only the first decides which code compiles.
    //
    // `crates/mnema-provider/src/http.rs` asks for `RootCerts::PlatformVerifier`.
    // Under `#[cfg(not(feature = "platform-verifier"))]` that arm is a `panic!`
    // (`ureq-3.3.0/src/tls/rustls.rs:183-185`), which fires when a real
    // connection is opened — the first key check, on every installation, and
    // never here, because nothing in this workspace performs a TLS handshake.
    // Loudness that first sounds in front of a user is silence to a gate.
    assert!(
        features_of(&tree, "ureq").contains(&"platform-verifier"),
        "ureq's `platform-verifier` feature is off, so asking for \
         `RootCerts::PlatformVerifier` compiles to a panic on the first real \
         connection, in front of a user:\n{tree}"
    );
    // The consequence: `platform-verifier = [\"dep:rustls-platform-verifier\"]`
    // (`ureq-3.3.0/Cargo.toml:100`), so the crate is in the graph only under the
    // feature. Measured 2026-08-09: `cargo tree -i` gives it exactly one route
    // into this graph, through ureq. That is what makes its presence a faithful
    // reading of the line above rather than a coincidence — and what would stop
    // being true if a second dependency ever brought it in, which is why the
    // feature itself is asserted first and this is the corroboration.
    assert!(
        packages.contains(&"rustls-platform-verifier"),
        "the platform verifier is not in the shipped graph, so certificates would be \
         validated against roots compiled into this binary rather than the ones the \
         machine trusts:\n{tree}"
    );

    // And what the graph must not carry. `mnema-secrets`' `test-store` feature
    // compiles a store that claims durability and keeps nothing; enabling it
    // does not lose keys on its own — `register()` has no caller in product
    // code — but it is the half of that pair which a manifest edit can supply
    // silently. `[dev-dependencies]` asking for it is what `cargo build` is
    // supposed to ignore; this is the assertion that says it did.
    let leaked: Vec<&str> = features_of(&tree, "mnema-secrets");
    assert!(
        !leaked.contains(&"test-store"),
        "mnema-secrets is in the SHIPPED graph with `test-store` enabled. That feature \
         exists for other crates' tests and compiles a credential store which reports \
         durability and stores nothing, so a key handed to it is gone at the next \
         launch. Features enabled from [dev-dependencies] must not reach a normal \
         build:\n{tree}"
    );
    assert!(
        packages.contains(&"mnema-secrets"),
        "mnema-secrets is not in the graph at all, so the assertion above passed by \
         asking about nothing:\n{tree}"
    );
}

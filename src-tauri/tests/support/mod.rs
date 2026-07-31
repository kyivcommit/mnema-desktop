//! Fixtures shared by more than one integration test binary under `tests/`.
//!
//! `tests/support/mod.rs` — a directory, not `tests/support.rs` — keeps this
//! from becoming a third, empty test suite of its own: Cargo turns every file
//! that sits directly inside `tests/` into its own binary, and a module
//! nested one directory down is not one of those files.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The extraction worker binary, built fresh so a walk job under test has
/// something real behind `Pool::extract` to call.
///
/// This is `crates/mnema-ingest/tests/support/mod.rs::worker` copied rather
/// than shared: Cargo's `tests/` directory is private to the crate it
/// belongs to, so a module under it cannot be `use`d from another crate's
/// integration tests, dev-dependency or not — the only two mechanisms that
/// *do* cross a crate boundary are a library item (which would put this
/// test-only code in the shipped binary) and `CARGO_BIN_EXE_*` (which Cargo
/// sets only for a package's own binaries, and `mnema-desktop` does not own
/// `mnema-extract-worker`). Both are worse than the copy. What must not
/// happen is a *second, different* resolver drifting from this one, which is
/// why this file's logic is the other file's, unchanged apart from how many
/// directories separate this crate from the workspace root.
pub fn worker() -> &'static Path {
    static WORKER: OnceLock<PathBuf> = OnceLock::new();
    WORKER.get_or_init(|| {
        let exe = std::env::current_exe().expect("a test binary knows its own path");
        // …/target/<profile>/deps/<binary>-<hash>
        let profile_dir = exe
            .parent()
            .and_then(Path::parent)
            .expect("a test binary sits in <target>/<profile>/deps");
        let target_dir = profile_dir
            .parent()
            .expect("<target>/<profile> sits inside <target>");
        let profile = profile_dir
            .file_name()
            .and_then(|name| name.to_str())
            .expect("the profile directory is named");
        // `src-tauri` sits ONE level below the workspace root — unlike
        // `crates/mnema-ingest`, which sits two — so this is the one line
        // that differs from the file this was copied from.
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(1)
            .expect("src-tauri sits one level below the workspace root");

        let mut cargo = Command::new(env!("CARGO"));
        cargo
            .args([
                "build",
                "-p",
                "mnema-extract",
                "--bin",
                "mnema-extract-worker",
            ])
            .arg("--manifest-path")
            .arg(workspace.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(target_dir);
        // `debug` is what the dev profile is called on disk, and naming it
        // explicitly is an error; every other profile is passed through.
        if profile != "debug" {
            cargo.args(["--profile", profile]);
        }
        let status = cargo.status().expect("cargo runs");
        assert!(
            status.success(),
            "the extraction worker did not build, so this whole file is unanswered \
             rather than passing"
        );

        let path = profile_dir.join(format!(
            "mnema-extract-worker{}",
            std::env::consts::EXE_SUFFIX
        ));
        assert!(
            path.exists(),
            "cargo reported success but {} is not there",
            path.display()
        );
        path
    })
}

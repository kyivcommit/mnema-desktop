//! Fixtures shared by the files under `tests/`.
//!
//! `tests/support/mod.rs` — a directory, not `tests/support.rs` — is how a
//! function is shared between integration test binaries without either one
//! owning the other. Cargo turns every file that sits directly inside
//! `tests/` into its own test binary; a *module* nested one directory down is
//! not one of those files.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The extraction worker binary.
///
/// **A deliberate copy of `crates/mnema-ingest/tests/support/mod.rs:61-118`.**
/// `cargo` sets `CARGO_BIN_EXE_*` only for binaries of the package being
/// tested, and the worker belongs to `mnema-extract`, a separate crate; a
/// dev-dependency would not help either, since cargo builds a dependency's
/// library and not its binaries.
///
/// So the path is derived from this test binary's own, and the worker is built
/// before it is named — a clean tree would otherwise fail or use a stale one.
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
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("crates/mnema-eval sits two levels below the workspace root");

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

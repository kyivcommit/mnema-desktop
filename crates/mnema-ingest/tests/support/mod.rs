//! Fixtures shared by more than one file under `tests/`.
//!
//! `tests/support/mod.rs` — a directory, not `tests/support.rs` — is how a
//! function is shared between integration test binaries without either one
//! owning the other. Cargo turns every file that sits directly inside
//! `tests/` into its own test binary; a *module* nested one directory down is
//! not one of those files, so `mod support;` in two different top-level test
//! files pulls in the same code without running it as a third, empty suite of
//! its own.
//!
//! `worker` used to be defined once inside `slice.rs`, and `walk.rs` needed
//! the same binary for the same reason. Writing a second resolver there — a
//! `walk.rs`-only copy that could drift from this one — is exactly the
//! divergence this module exists to close off: two answers to "where is the
//! worker" are a standing invitation for one of them to end up wrong while
//! the other stays green.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

// ------------------------------------------------------------ the real worker

/// The extraction worker binary.
///
/// `crates/mnema-pool/tests/` names its stand-in with
/// `env!("CARGO_BIN_EXE_mnema-pool-test-worker")`, and that mechanism is not
/// available here: cargo sets `CARGO_BIN_EXE_*` only for binaries of the
/// package being tested, and this package has none — the worker belongs to
/// `mnema-extract`, which this crate must never depend on. Nor does declaring
/// a dev-dependency help; cargo builds a dependency's library, not its
/// binaries.
///
/// So the path is derived from where this test binary itself was put, and the
/// worker is built before it is named — otherwise `cargo test -p mnema-ingest`
/// on a clean tree would either fail or, worse, silently use a stale binary
/// from a previous build. Running cargo from inside a test is already how
/// `src-tauri/tests/dependency_boundary.rs` asks its question.
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
            .expect("crates/mnema-ingest sits two levels below the workspace root");

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

// ---------------------------------------------------------- a worker that is not

/// Writes an executable stand-in worker that answers every request with
/// `body`, and returns where it is.
///
/// A shell script rather than a Rust binary, and not
/// `crates/mnema-pool/src/bin/test_worker.rs` either. That one selects its
/// behaviour from a prefix on the requested path, which cannot survive being
/// joined to a temporary directory, and it is task 8's scaffolding besides.
/// What these tests need is simpler and closer to the thing being modelled: a
/// sidecar that is not the worker this parent speaks to — a half-finished
/// install, a mismatched release — which is a file, not a mock.
///
/// `dir` should never be a watched root: `enumerate` would then list the
/// script itself as a found file, and every caller of this function passes a
/// scratch directory that is not walked for exactly that reason.
#[cfg(unix)]
pub fn wrong_worker(dir: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("wrong-worker");
    std::fs::write(
        &path,
        format!("#!/bin/sh\nwhile read -r _line; do\n{body}\ndone\n"),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

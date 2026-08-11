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

// -------------------------------------------------- a format with no reader

/// A file the worker refuses under `SkipRule::Unsupported`: a zip archive with
/// no member any reader recognises. `typing::identify` answers
/// `Reader::Unrecognized` for it, and the worker's `unsupported` branch is
/// "no reader implemented yet" — a promise that one is coming.
///
/// Twenty-two bytes: an end-of-central-directory record and nothing else,
/// which is what an empty zip is. Written out rather than produced with the
/// `zip` crate so that this crate does not take a dependency to make a file
/// whose whole content is a constant.
///
/// **It used to be `%PDF-1.7…`, and every test using it silently changed
/// subject when the PDF reader landed.** A `%PDF-` stub is now refused as
/// `malformed` — a verdict about *damage*, which is remembered until
/// `INDEX_FORMAT_VERSION` moves — where these tests need `unsupported`, whose
/// whole point is that it is the least stable verdict this product gives.
/// Seven tests went red on the rule and were fixed here rather than by
/// lowering each assertion to whatever the new answer happened to be.
pub const NO_READER_FOR_THIS: &[u8] = b"PK\x05\x06\
    \x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";

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
///
/// **It states the real worker's readers, and only its reading is wrong.**
/// `--manifest` is delegated to the binary `worker()` names rather than
/// answered from a literal here, for two reasons. The narrow one: a literal
/// would be a second copy of the product's manifest, green until the day a
/// reader is added and then wrong in a file nobody would think to look in.
/// The load-bearing one: a parent asks its worker for the manifest before it
/// sends a single file (`Pool::manifest`), so a stand-in that cannot answer
/// stops the walk *there* — and every test whose subject is what happens to
/// files afterwards would be measuring the handshake instead. The shape this
/// models is the one D44 was written for and is the commoner one anyway: the
/// binary is the right binary, and the library it loads is not.
///
/// Use [`worker_from_before_the_manifest`] for the other shape — a binary that
/// reads files perfectly and cannot answer the handshake.
#[cfg(unix)]
pub fn wrong_worker(dir: &Path, body: &str) -> PathBuf {
    // Quoted, not interpolated bare: a target directory can hold a space. It
    // cannot hold a double quote or a `$` in any environment this repository
    // builds in, which is the assumption `sh` leaves standing here.
    let manifest = format!(
        "if [ \"$1\" = \"--manifest\" ]; then\n  exec \"{}\" --manifest\nfi\n",
        worker().display()
    );
    write_script(
        dir,
        "wrong-worker",
        &format!("{manifest}while read -r _line; do\n{body}\ndone\n"),
    )
}

/// A release from before `--manifest` existed: it reads every file exactly as
/// the current worker does, and answers the manifest question with nothing.
///
/// The exact inverse of [`wrong_worker`], and deliberately so. **A stand-in
/// that fails the handshake *and* the frames cannot show which of the two
/// stopped a walk** — measured, and it is why this function is shaped the way
/// it is: an earlier version of it printed rubbish for every request, and a
/// walk that invented its own manifest instead of asking then stopped anyway,
/// one file later, with the same `PoolError::Protocol` and the same empty
/// index. Both mutation cases stayed green against a test that read as though
/// it asserted the handshake. Delegating the files with `exec` — so the real
/// worker inherits this process's stdin and stdout and the protocol runs
/// untouched — leaves the handshake as the only thing that can fail, and a
/// parent that skipped it indexes the folder instead.
///
/// `dead_code` is allowed because this module is compiled into **every** test
/// binary that declares `mod support;`, and only `walk.rs` has a use for this
/// one — the same shape that would make any fixture added here for one file
/// warn in the others.
#[cfg(unix)]
#[allow(dead_code)]
pub fn worker_from_before_the_manifest(dir: &Path) -> PathBuf {
    write_script(
        dir,
        "worker-without-a-manifest",
        &format!(
            "if [ \"$1\" = \"--manifest\" ]; then\n  exit 0\nfi\nexec \"{}\" \"$@\"\n",
            worker().display()
        ),
    )
}

#[cfg(unix)]
fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    wait_until_it_will_run(&path);
    path
}

/// Runs the script once before handing it over, retrying while the kernel says
/// the file is still open for writing somewhere.
///
/// **`ETXTBSY` here is a state of the process, not of this thread.** `fs::write`
/// above has closed its own descriptor; what has not necessarily happened is
/// somebody else's `exec`. These tests run in parallel threads and every one of
/// them spawns workers, and between a `fork` and the `exec` that follows it the
/// child holds a duplicate of every descriptor the parent had open — including
/// this file's, for as long as that window lasts. Execute it in that window and
/// Linux refuses with "Text file busy".
///
/// Measured 2026-08-11 rather than reasoned about: one run in six of
/// `cargo test -p mnema-ingest --test slice` failed this way on a four-core
/// Linux box, and both CI runs of a branch that changes nothing in this crate
/// failed it — **on a different test each time**, which is what a race looks
/// like when a total is the only thing you write down.
///
/// This closes it rather than making it rarer: nothing opens the file for
/// writing again after the line above, so once every child that forked during
/// that one window has reached its `exec`, the file is free for good. Waiting
/// for one successful run is waiting for exactly that.
///
/// `--manifest` is the argument both stand-ins answer without reading standard
/// input and without touching an index, and stdin is closed as well, so a body
/// that loops on `read` cannot be left waiting for a request that is not coming.
///
/// It does not belong in `mnema_pool`, which has no such problem to solve: the
/// worker it spawns was not written to disk seconds earlier by the process that
/// runs it.
#[cfg(unix)]
fn wait_until_it_will_run(path: &Path) {
    use std::process::{Command, Stdio};

    for _ in 0..400 {
        match Command::new(path)
            .arg("--manifest")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                let _ = child.wait();
                return;
            }
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            // Anything else is this stand-in being wrong rather than busy, and
            // is worth failing on here — where the script is written and its
            // text is in hand — instead of several layers down as a pool error.
            Err(e) => panic!("the stand-in at {} will not run at all: {e}", path.display()),
        }
    }
    panic!(
        "{} was still reported as busy after two seconds of retrying, which is far longer \
         than a fork-to-exec window: something holds it open for writing",
        path.display()
    );
}

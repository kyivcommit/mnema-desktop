//! `Pool::manifest`, against binaries that answer `--manifest` in the ways a
//! binary can answer it wrongly.
//!
//! Shell stand-ins rather than `mnema-pool-test-worker`: that one selects its
//! behaviour from a prefix on the requested *path*, which is a field of the
//! NDJSON protocol, and this branch is outside the protocol entirely — it is an
//! argument and an exit. What these tests need is a file that prints one line,
//! which is what a mismatched sidecar is.
//!
//! The real worker cannot stand in here either, for the reason
//! `crates/mnema-ingest/tests/support/mod.rs` gives at length: cargo does not
//! build a dependency's binaries, and `mnema-pool` must never depend on
//! `mnema-extract` (D40).

#![cfg(unix)]

use std::path::{Path, PathBuf};

use mnema_pool::{Pool, PoolConfig, PoolError};

/// An executable that prints `line` and exits, whatever it is asked.
fn worker_answering(dir: &Path, name: &str, line: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\nprintf '%s\\n' '{line}'\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    wait_until_it_will_run(&path);
    path
}

/// Runs the script once before handing it over, retrying while Linux says the
/// file is still open for writing somewhere.
///
/// The same function as `crates/mnema-ingest/tests/support/mod.rs` has, for the
/// same reason: `fs::write` has closed its own descriptor, but a child that
/// another test's thread forked during that write still holds a copy until its
/// `exec`, and running the file inside that window fails with "Text file busy".
/// Seen once in CI here (`an_extension_naming_no_reader_is_refused_too`,
/// ubuntu-24.04, run 35088290024) and measured 2026-09-16 on the Ubuntu stand:
/// 105 and 122 refusals in 2,000 write-then-exec rounds against three threads
/// spawning `/bin/true`, 0 without them. Nothing opens the file for writing
/// again after this, so one successful run means every such child has passed
/// its `exec` and the file is free for good.
///
/// The pre-run is itself a spawn outside the pool's lock, so its child can hold
/// the pipe ends of a `Pool::manifest` spawned at the same moment — for the
/// millisecond `/bin/sh` takes to print one line and exit, which `manifest`
/// then waits out as part of `wait_with_output`. Accepted: it delays, it does
/// not lose, and nothing in this binary writes to a departed worker.
fn wait_until_it_will_run(path: &Path) {
    use std::process::{Command, Stdio};

    for _ in 0..400 {
        match Command::new(path)
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
            Err(e) => panic!(
                "the stand-in at {} will not run at all: {e}",
                path.display()
            ),
        }
    }
    panic!(
        "{} was still reported as busy after two seconds of retrying, which is far longer \
         than a fork-to-exec window: something holds it open for writing",
        path.display()
    );
}

fn pool_over(worker: PathBuf) -> Pool {
    Pool::new(PoolConfig::new(worker)).unwrap()
}

/// A manifest that names no reader is refused, and one that names readers is
/// not.
///
/// `run_one` refuses a *header* naming no reader, and the reason it gives is
/// that no manifest ever names the empty reader. Nothing held that sentence to
/// anything until this test: `NOT NULL` is satisfied by `""`, serde is happy
/// with it, and a `Manifest { default: ReaderId { reader: "", version: 1 } }`
/// parses. What it would cost is not a bad name in a row — every `path` row
/// would then mismatch a value nothing can equal, so the entire index would be
/// handed to workers on every walk, for ever, silently.
///
/// **Both directions**, because a `Pool::manifest` that refused everything
/// would pass the first half alone, and one that refused nothing would pass the
/// second.
#[test]
fn a_manifest_naming_no_reader_is_refused_and_a_named_one_is_not() {
    let dir = tempfile::tempdir().unwrap();

    let empty = pool_over(worker_answering(
        dir.path(),
        "empty-reader",
        r#"{"default":{"reader":"","version":1},"by_extension":{}}"#,
    ));
    let outcome = empty.manifest();
    assert!(
        matches!(outcome, Err(PoolError::Protocol { .. })),
        "a manifest whose default names no reader must be refused: {outcome:?}"
    );

    let named = pool_over(worker_answering(
        dir.path(),
        "named-reader",
        r#"{"default":{"reader":"text","version":1},"by_extension":{}}"#,
    ));
    let manifest = named
        .manifest()
        .expect("a manifest that names its reader is an ordinary answer");
    assert_eq!(manifest.default.reader, "text");
}

/// The same rule under an extension rather than under the default.
///
/// Its own test, because the default and the map are two fields and a check
/// written for one of them leaves the other open — an empty name under a single
/// extension re-reads only that extension's files on every walk, which is the
/// same defect at a size nobody would go looking for.
#[test]
fn an_extension_naming_no_reader_is_refused_too() {
    let dir = tempfile::tempdir().unwrap();
    let pool = pool_over(worker_answering(
        dir.path(),
        "empty-under-md",
        r#"{"default":{"reader":"text","version":1},"by_extension":{"md":{"reader":" ","version":1}}}"#,
    ));

    let outcome = pool.manifest();
    assert!(
        matches!(outcome, Err(PoolError::Protocol { .. })),
        "the map is published too, and a blank name in it is the same fault: {outcome:?}"
    );
}

//! A worker that cannot be handed even its first request.
//!
//! Its own binary, with one test, on purpose. The request below is written into
//! a pipe whose only reader closes it without reading; the write fails only once
//! EVERY read end is closed, and a child spawned outside the pool's lock by a
//! sibling test in the same process can inherit that end — on macOS keeping it
//! for its whole life, on Linux only until its own exec — `tests/pipes.rs` says
//! how that was measured. With no sibling there is nothing to inherit it, and
//! no concurrent fork to make the freshly written stand-in "Text file busy"
//! either.

#![cfg(unix)]

use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use mnema_pool::{Pool, PoolConfig, PoolError};

#[allow(dead_code)]
mod support;
use support::Watchdog;

/// The pair of states this separates: a pool that gives up on a worker it has
/// only just started, against one that retries it as though it had aged out.
///
/// The retry is not visible in the variant — both arms end in
/// `WorkerUnreachable` — so the assertions are on what only the retry changes:
/// how many processes were started, and which error is carried. The retrying
/// arm spawns a second worker and falls out of the loop with an error of its
/// own making (`ErrorKind::Other`); the refusing arm carries the write's own
/// broken pipe.
///
/// Why the request is a megabyte: a pipe holds 64 KiB, so the write blocks
/// until the reader either reads or closes its end, and a writer blocked on a
/// pipe whose last reader closes gets `EPIPE`. A short request would race the
/// stand-in's own startup — it would land in the buffer before `/bin/sh` got
/// round to closing its stdin, and the pool would wait out `timeout` for an
/// answer instead.
#[test]
fn a_fresh_worker_that_cannot_be_handed_its_request_is_not_retried() {
    let _watchdog = Watchdog::new(
        "deaf-from-birth worker: the write never failed, so something else holds the read end",
        Duration::from_secs(30),
    );

    let dir = tempfile::tempdir().unwrap();
    let worker = dir.path().join("deaf-from-birth");
    // Closes its stdin before reading a byte, then lives — bounded, so that a
    // pool which fails to kill it does not leave it behind for long.
    std::fs::write(&worker, "#!/bin/sh\nexec 0<&-\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o755)).unwrap();

    let pool = Pool::new(PoolConfig {
        workers: 1,
        timeout: Duration::from_secs(5),
        ..PoolConfig::new(worker.clone())
    })
    .unwrap();

    let path = format!("/{}", "x".repeat(1 << 20));
    let outcome = pool.extract(path.as_ref());

    match outcome {
        Err(PoolError::WorkerUnreachable { source }) => assert_eq!(
            source.kind(),
            ErrorKind::BrokenPipe,
            "the error is not the write's own: a second worker was started and the \
             pool fell out of its retry loop: {source}"
        ),
        other => panic!(
            "a worker that closed its stdin before its first request should end the \
             call as WorkerUnreachable; got {other:?}"
        ),
    }
    assert_eq!(
        pool.worker_generation(),
        1,
        "a freshly spawned worker that could not be written to was retried"
    );
}

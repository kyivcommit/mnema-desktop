//! A worker's pipe ends must never reach a sibling's child.
//!
//! Its own binary, on purpose. The property is that no two spawns **in this
//! process** overlap, and `tests/outside_the_pool.rs` spawns `ps` and `kill`
//! outside the pool; a child of those, between its fork and its exec, holds a
//! copy of every descriptor the process has — which for a few microseconds
//! includes the read end of a worker being spawned at that moment — and the
//! test below hit exactly that once in twenty runs on a four-core Ubuntu
//! stand, back when it and those two spawns shared `supervision.rs`. Here every spawn is the pool's, so every spawn is under its lock.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mnema_pool::{Outcome, Pool, PoolConfig};

fn config() -> PoolConfig {
    PoolConfig::new(env!("CARGO_BIN_EXE_mnema-pool-test-worker"))
}

/// A departed worker is detected by the write into its request pipe failing.
/// That holds only while nobody else holds the pipe's read end — and on macOS
/// the standard library creates a pipe with `pipe()` and sets `FD_CLOEXEC` in a
/// second call, so a `spawn` on another thread in between inherits both ends
/// into a child that then keeps them for as long as it lives. Measured
/// 2026-09-16 with a standalone probe on Apple M2 Max: 6 and 10 writes in 2000
/// succeeded into a pipe whose only reader had closed it, against 0 in 2000
/// without a concurrent spawner; with every spawn under one process-wide lock,
/// 0 in 2000 twice over 140,000 background spawns.
///
/// Here the spawners are pools too — a `batch` of one retires a worker after
/// every file, so each `extract` is a spawn — and the deaf worker's second
/// request must still be answered on a fresh worker, not time out. A hit costs
/// `timeout` and arrives as a skip, which is what the assertion names. Without
/// the lock this failed 4 of 4 runs at the default rounds on the machine above.
#[cfg(unix)]
#[test]
fn a_concurrent_spawn_does_not_keep_a_departed_workers_pipe_open() {
    let rounds: usize = std::env::var("MNEMA_PIPE_RACE_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(800);
    let stop = Arc::new(AtomicBool::new(false));

    let spawners: Vec<_> = (0..4)
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let pool = Pool::new(PoolConfig {
                    workers: 1,
                    batch: 1,
                    timeout: Duration::from_secs(1),
                    ..config()
                })
                .unwrap();
                while !stop.load(Ordering::SeqCst) {
                    let outcome = pool.extract("ok:churn.txt".as_ref()).unwrap();
                    assert!(matches!(outcome, Outcome::Extracted(_)), "{outcome:?}");
                }
            })
        })
        .collect();

    let probes: Vec<_> = (0..4)
        .map(|_| {
            std::thread::spawn(move || {
                for round in 0..rounds {
                    let pool = Pool::new(PoolConfig {
                        workers: 1,
                        batch: 100,
                        timeout: Duration::from_secs(1),
                        ..config()
                    })
                    .unwrap();
                    let first = pool.extract("deaf:first.txt".as_ref()).unwrap();
                    assert!(matches!(first, Outcome::Extracted(_)), "{first:?}");
                    match pool.extract("ok:second.txt".as_ref()).unwrap() {
                        Outcome::Extracted(_) => {}
                        Outcome::Skipped(skip) => panic!(
                            "round {round}, worker generation {}: the request to a departed \
                             worker was buffered instead of refused — its pipe was inherited \
                             by a sibling's child — and the file timed out: {skip:?}",
                            pool.worker_generation()
                        ),
                    }
                    assert_eq!(pool.worker_generation(), 2);
                }
            })
        })
        .collect();

    let outcomes: Vec<_> = probes.into_iter().map(|probe| probe.join()).collect();
    stop.store(true, Ordering::SeqCst);
    for spawner in spawners {
        spawner.join().unwrap();
    }
    for outcome in outcomes {
        outcome.expect("a probe thread panicked");
    }
}

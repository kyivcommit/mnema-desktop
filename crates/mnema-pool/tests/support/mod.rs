//! Shared by the test binaries of this crate. Each binary is its own process,
//! and which spawns share a process is the whole point of there being several:
//! see `pipes.rs` and `outside_the_pool.rs`.

#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mnema_pool::{Document, Pool, PoolConfig, PoolError};

/// The stand-in worker (`src/bin/test_worker.rs`), whose behaviour is selected
/// by the prefix on the requested path.
pub fn config() -> PoolConfig {
    PoolConfig::new(env!("CARGO_BIN_EXE_mnema-pool-test-worker"))
}

pub fn extract(pool: &Pool, path: &str) -> Result<mnema_pool::Outcome, PoolError> {
    pool.extract(Path::new(path))
}

pub fn document(outcome: mnema_pool::Outcome) -> Document {
    match outcome {
        mnema_pool::Outcome::Extracted(document) => document,
        mnema_pool::Outcome::Skipped(skip) => panic!("expected a document, got {skip:?}"),
    }
}

/// `cargo test` has no per-test timeout, so this is one. It aborts the whole
/// run rather than letting a hang wedge CI; the stand-in workers left behind
/// self-destruct on their own timer, which is the only reason exiting this
/// abruptly is acceptable.
pub struct Watchdog(Arc<AtomicBool>);

impl Watchdog {
    pub fn new(label: &'static str, bound: Duration) -> Self {
        let finished = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&finished);
        std::thread::spawn(move || {
            let deadline = Instant::now() + bound;
            while Instant::now() < deadline {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            eprintln!(
                "watchdog: {label} did not finish within {bound:?}. Failing loudly \
                 rather than hanging: the supervisor's own deadline is broken."
            );
            std::process::exit(103);
        });
        Watchdog(finished)
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

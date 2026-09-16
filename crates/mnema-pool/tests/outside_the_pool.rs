//! An idle worker's death, and the `ps` and `kill` that observing it takes —
//! in a binary of its own.
//!
//! The property every binary in this crate has to keep is this: a process that
//! writes to a departed worker and expects the write to fail must never let a
//! spawn outside the pool overlap a spawn inside it. A child of any spawn holds
//! a copy of every descriptor the process has between its fork and its exec,
//! the read end of a worker's request pipe included if that worker is being
//! spawned at the moment; the pool's lock covers the pool's own spawns and
//! nothing else. Measured 2026-09-16 on a four-core Ubuntu stand: the test in
//! `pipes.rs` hit exactly this once in twenty runs while it shared a process
//! with the `ps` loop below.
//!
//! The test here expects a failed write too — into the worker it has just
//! killed — and holds the property by order rather than by a lock: it is the
//! only test in this process, its `kill` and `ps` run after that worker was
//! spawned and before the next one is, so no child of theirs can ever hold a
//! pipe end of either. The macOS rlimit probe used to live here as well and
//! now has `rlimit.rs`, because its `pre_exec` fork could overlap this test's
//! first spawn.

use std::time::{Duration, Instant};

use mnema_pool::{Pool, PoolConfig};

mod support;
use support::{Watchdog, config, document, extract};

/// Waits until process `pid` has terminated, reaped or not. A child the pool has
/// not waited for stays visible as a zombie, and `kill -0` still succeeds on one,
/// so the state column is what tells the truth.
#[cfg(unix)]
fn wait_until_terminated(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .expect("ps runs");
        let state = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if state.is_empty() || state.starts_with('Z') {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "worker {pid} never terminated; ps says {state:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn a_worker_that_died_while_idle_costs_the_next_file_nothing() {
    let _watchdog = Watchdog::new("idle worker died", Duration::from_secs(30));
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let pool = Pool::new(PoolConfig {
        workers: 1,
        batch: 100,
        ..config()
    })
    .unwrap();

    document(extract(&pool, &format!("pid:{}", pid_file.display())).unwrap());
    let pid: u32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();

    // The worker is now idle, between documents, and something outside this pool
    // ends it — which is exactly what the out-of-memory killer does on a
    // platform where no ceiling can be imposed, since it chooses by size and not
    // by what a process is doing.
    assert!(
        std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    wait_until_terminated(pid);

    // One idle worker's death must cost this file nothing at all: not a skip
    // recorded against an innocent document, and certainly not the job. A pool
    // of two workers over forty thousand files would otherwise abort because a
    // process died doing nothing.
    document(extract(&pool, "ok:next.txt").unwrap());
    assert_eq!(
        pool.worker_generation(),
        2,
        "the dead worker was replaced, not written to"
    );
}

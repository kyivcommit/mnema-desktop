//! Tests that spawn processes **outside** the pool — `ps`, `kill`, and on macOS
//! `/usr/bin/true` under `pre_exec` — in a binary of their own.
//!
//! A child of such a spawn holds a copy of every descriptor this process has
//! between its fork and its exec, the read end of a worker's request pipe
//! included if a worker is being spawned at that moment; the pool's lock covers
//! only the pool's own spawns. `supervision.rs` is where a departed worker must
//! be detected by a failed write, so nothing in that process may spawn behind
//! the lock's back. Measured 2026-09-16 on a four-core Ubuntu stand: the pipe
//! test hit exactly this once in twenty runs while these lived beside it.

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

#[cfg(target_os = "macos")]
#[test]
fn this_macos_still_refuses_an_address_space_rlimit() {
    // Measured 2026-07-26 on Darwin 25.5.0/arm64: setrlimit(RLIMIT_AS) fails
    // with EINVAL, and `ulimit -v` agrees. The pool's Linux-only ceiling rests
    // on that, so the fact is pinned here rather than trusted to a comment: if
    // a future macOS starts honouring the call, this test goes red and the
    // ceiling can be switched on for a platform that has one.
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let mut command = Command::new("/usr/bin/true");
    command.stdout(Stdio::null()).stderr(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: 512 << 20,
                rlim_max: 512 << 20,
            };
            if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let error = command
        .status()
        .expect_err("macOS is expected to reject an address-space limit");
    assert_eq!(
        error.raw_os_error(),
        Some(libc::EINVAL),
        "expected EINVAL from setrlimit(RLIMIT_AS), got {error:?}"
    );
}

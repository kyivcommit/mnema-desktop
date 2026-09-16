//! The macOS answer to `memory_ceiling`, pinned by running the call. Its own
//! binary because the probe forks under `pre_exec`, and `outside_the_pool.rs`
//! says why a fork must not share a process with a test that writes to a
//! departed worker.

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

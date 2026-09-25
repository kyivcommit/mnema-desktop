//! The watchdog's own sentence must reach the log.
//!
//! libtest captures what a test prints, and a thread the test spawns inherits
//! that capture; `std::process::exit` then throws the captured bytes away. A
//! watchdog that printed its label through the capture left CI with nothing but
//! "exit code 103" three times on `macos-14` — which of 28 tests had hung was
//! never in the log (D160). This runs the watchdog in a child copy of this very
//! binary, under the default capture, and reads the child's real stderr.

use std::process::Command;
use std::time::Duration;

#[allow(dead_code)]
mod support;
use support::Watchdog;

/// The fixture the test below runs in a child process. Ignored so a plain
/// `cargo test` never waits on it; the child asks for it by exact name.
#[test]
#[ignore = "runs only as the child of the test below"]
fn hangs_under_a_watchdog() {
    let _watchdog = Watchdog::new("the hanging fixture", Duration::from_millis(200));
    std::thread::sleep(Duration::from_secs(30));
}

#[test]
fn a_watchdog_that_fires_names_the_test_in_the_log() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "hangs_under_a_watchdog", "--ignored"])
        // The capture is the condition under test: a parent run with
        // `RUST_TEST_NOCAPTURE` set must not switch it off in the child too.
        .env_remove("RUST_TEST_NOCAPTURE")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(103), "stderr: {stderr}");
    assert!(
        stderr.contains("watchdog: the hanging fixture did not finish"),
        "the label never reached the log; stderr was: {stderr:?}"
    );
}

//! Start-up smoke test — the guard the unit suite structurally cannot be.
//!
//! The application menu is built with `muda`, which must run on the real main
//! thread, so a `#[test]` (a libtest worker thread) cannot build the real app
//! in-process. That is exactly why a start-up panic — `build_app_menu` calling
//! `app.path()` before Tauri had managed the path resolver — shipped green
//! through 1190 workspace tests, 16 UI tests, and every review: nothing in the
//! suite launches the app. The only faithful guard is to run the real binary
//! and confirm it survives start-up (build -> menu -> setup) without dying.
//!
//! `#[ignore]` BY DESIGN: it launches a GUI and needs a display, so it must NOT
//! run in the default `cargo test --workspace` (a headless or Linux leg would
//! false-fail). CI runs it explicitly on macOS, and it is runnable locally:
//!
//!   cargo test -p mnema-desktop --test launch_smoke -- --include-ignored
//!
//! `--include-ignored`, NOT `--ignored`: `--ignored` runs ONLY ignored tests,
//! so the day someone drops the `#[ignore]` the step would select zero tests and
//! pass green, certifying nothing — the trap `mnema-secrets`' keychain step
//! documents (ci.yml). This mirrors how that `roundtrip` test is invoked from
//! its own CI step rather than the default suite.

use std::process::Command;
use std::time::{Duration, Instant};

/// How long start-up (build -> menu -> `.setup`) is given to complete. A panic
/// kills the process in a fraction of a second; this window only needs to be
/// comfortably longer than a real, healthy start-up.
const STARTUP_WINDOW: Duration = Duration::from_secs(8);

#[test]
#[ignore = "launches a GUI; run explicitly where a display exists (macOS CI / locally) — see module docs"]
fn the_app_survives_startup_without_panicking() {
    // Cargo builds the crate's binary for its integration tests and hands us its
    // path here — no `cargo tauri dev`/Vite needed; the frontend is embedded.
    let exe = env!("CARGO_BIN_EXE_mnema-desktop");
    let mut child = Command::new(exe)
        .spawn()
        .unwrap_or_else(|e| panic!("could not spawn {exe}: {e}"));

    // Poll for an early exit. A tray-resident that started cleanly runs until it
    // is quit, so ANY exit inside the start-up window is the failure — a panic
    // (exit 101 / SIGABRT), the class this test guards, or a launch that could
    // not come up. The process's own output is inherited, so a Rust panic
    // message appears above this test's failure in the log.
    let deadline = Instant::now() + STARTUP_WINDOW;
    loop {
        match child.try_wait().expect("poll the child process") {
            Some(status) => {
                panic!(
                    "mnema-desktop exited during start-up with {status} instead of \
                     running. A start-up panic (for example the build_app_menu / \
                     app.path() timing this test was written for) is the regression \
                     it exists to catch — see the process output above."
                );
            }
            None if Instant::now() >= deadline => break, // survived → healthy
            None => std::thread::sleep(Duration::from_millis(200)),
        }
    }

    // Started cleanly. The resident has no self-exit path here, so shut it down.
    let _ = child.kill();
    let _ = child.wait();
}

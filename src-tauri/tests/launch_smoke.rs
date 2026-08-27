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

use std::path::{Path, PathBuf};
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

/// The boot opens the index — proved from a launched binary, not from a
/// function call.
///
/// `the_boot_opens_the_index` in `tests/commands.rs` proves `boot_index` opens
/// what it is handed. Nothing in it proves `.setup` ever calls it, and a gate
/// reached by accident is not a gate: the call is one line, deleting it reddens
/// no in-process test, and the P0 this whole task exists for was precisely a
/// function nobody called.
///
/// So this one launches the real binary with its data directory redirected to a
/// fresh temporary tree and looks for the database file appearing in it. Both
/// directions: nothing there before, something there after.
///
/// ⚠️ **Three limitations, stated rather than left to be discovered.** (1) The
/// redirect is a hypothesis about how Tauri resolves `app_local_data_dir()` on
/// this platform; if it does not take, the launch touches the developer's own
/// index and this test fails without having measured the thing it names — which
/// is why its failure message names both hypotheses and the disambiguator.
/// (2) CI runs this file on macOS only, so a green run says nothing about Linux
/// or Windows. (3) It is the weaker of the two guards by construction: it
/// cannot say *which* line was lost, only that the file did not appear.
#[test]
#[ignore = "launches a GUI; run explicitly where a display exists (macOS CI / locally) — see module docs"]
fn a_real_launch_creates_the_index_in_its_data_directory() {
    let home = tempfile::tempdir().expect("a temporary home");

    // The half a fixture forgets. Without it the test passes on a file it did
    // not cause, and would go on passing after the boot stopped opening
    // anything.
    assert_eq!(
        find_index(home.path()),
        None,
        "the temporary home must start with no index"
    );

    let exe = env!("CARGO_BIN_EXE_mnema-desktop");
    let mut child = Command::new(exe)
        // One redirect per platform's own resolver rather than a guess at which
        // one is in play: macOS and Linux read `HOME` (Linux prefers
        // `XDG_DATA_HOME` when set), Windows reads `LOCALAPPDATA`. Setting all
        // of them is what makes this test's *premise* portable even where its
        // execution is not.
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", home.path())
        .env("LOCALAPPDATA", home.path())
        .env("APPDATA", home.path())
        .spawn()
        .unwrap_or_else(|e| panic!("could not spawn {exe}: {e}"));

    let deadline = Instant::now() + STARTUP_WINDOW;
    let found = loop {
        if let Some(path) = find_index(home.path()) {
            break Some(path);
        }
        if let Some(status) = child.try_wait().expect("poll the child process") {
            panic!(
                "mnema-desktop exited during start-up with {status} before any index                  appeared — see the process output above."
            );
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(200));
    };

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        found.is_some(),
        "No index under the temp home ({}). Either `.setup` does not call \
         `boot_index`, or the environment redirect did not take on this platform \
         — the in-process guard `the_boot_opens_the_index` in `tests/commands.rs` \
         decides the first half: if it is green, what failed here is the call site \
         or the redirect, not the function.",
        home.path().display()
    );
}

/// The first `index.sqlite` anywhere under `dir`, or `None`.
///
/// A search rather than the exact path, because the exact path is the thing
/// under test's business: it is composed from the bundle identifier by Tauri's
/// own resolver, and hard-coding this test's guess at it would mean asserting
/// against a second implementation of the code being measured. Any index under
/// a home that started empty was put there by this launch.
fn find_index(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_index(&path) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|n| n == "index.sqlite") {
            return Some(path);
        }
    }
    None
}

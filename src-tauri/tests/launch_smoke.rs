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
//!
//! **One test, one spawn — not two.** An earlier version of this file spawned
//! two real binaries, each `#[ignore]`d and each proving one fact. CI runs the
//! whole file with one command and no thread limit (`ci.yml:316`), so libtest
//! ran both concurrently, and `tauri_plugin_single_instance` (`lib.rs:413`) is
//! registered before anything else the app does: its second-instance callback
//! calls `focus_launcher` and lets the process exit, keyed on a socket path
//! under `/tmp` derived from the bundle identifier — a path that does **not**
//! depend on `HOME`, so this file's own `HOME` redirect never separated the two
//! spawns from each other. Whichever one lost the race exited cleanly during
//! its own start-up window, and the test watching it reported a start-up panic
//! that never happened. One spawn removes the second process there is to race.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// How long start-up (build -> menu -> `.setup`) is given to complete. A panic
/// kills the process in a fraction of a second; this window only needs to be
/// comfortably longer than a real, healthy start-up.
const STARTUP_WINDOW: Duration = Duration::from_secs(8);

/// The real binary survives start-up, and its boot opens the index — both
/// proved from one launched process.
///
/// **Why one launch proves both.** The panic this file guards against (the
/// `build_app_menu` / `app.path()` timing named above) and the P0 `boot_index`
/// closes (nothing in the shipped app ever opened the index) are two different
/// claims about the same window: the interval between spawning the process and
/// `.setup()` finishing. Watching that interval once, with two things to look
/// for, needs one process; watching it twice needed two, and two is what
/// raced.
///
/// `the_boot_opens_the_index` in `tests/commands.rs` proves `boot_index` opens
/// what it is handed, in-process. Nothing in it proves `.setup` ever calls it,
/// and a gate reached by accident is not a gate: the call is one line, deleting
/// it reddens no in-process test, and the P0 this whole task exists for was
/// precisely a function nobody called. So this test launches the real binary
/// with its data directory redirected to a fresh temporary tree, and the same
/// process is watched for two things over the same window: it must not exit
/// early (the panic this file was written for), and `index.sqlite` must appear
/// under the redirected home before the window closes (the call-site proof).
///
/// Both directions on the index claim: nothing there before, something there
/// after. The panic claim only has one direction — there is no "exited on
/// purpose" for a tray-resident mid start-up.
///
/// ⚠️ **Three limitations, stated rather than left to be discovered.** (1) The
/// redirect is a hypothesis about how Tauri resolves `app_local_data_dir()` on
/// this platform; if it does not take, the launch touches the developer's own
/// index, and a run that then finds an index is not proof of anything this
/// test claims. (2) CI runs this file on macOS only, so a green run says
/// nothing about Linux or Windows. (3) The index half is the weaker of the two
/// claims by construction: it cannot say *which* line was lost, only that the
/// file did not appear.
#[test]
#[ignore = "launches a GUI; run explicitly where a display exists (macOS CI / locally) — see module docs"]
fn a_real_launch_survives_startup_and_opens_its_index() {
    let home = tempfile::tempdir().expect("a temporary home");

    // The half a fixture forgets. Without it, finding an index after the
    // launch does not prove the launch created it, and the assertion at the
    // end would go on passing after `boot_index` stopped running.
    assert_eq!(
        find_index(home.path()),
        None,
        "the temporary home must start with no index"
    );

    // Cargo builds the crate's binary for its integration tests and hands us
    // its path here — no `cargo tauri dev`/Vite needed; the frontend is
    // embedded.
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

    // Poll for the index and for an early exit over the whole window, rather
    // than returning as soon as the index appears: a panic guarded by this
    // file can land after `boot_index` runs (menu rebuild, tray construction,
    // window show all come later in `.setup`), so stopping early on the first
    // fact would stop watching for the second. `found` only ever moves from
    // `None` to `Some` — once the file exists it stays found regardless of
    // what the process does afterwards.
    let deadline = Instant::now() + STARTUP_WINDOW;
    let mut found: Option<PathBuf> = None;
    loop {
        if found.is_none() {
            found = find_index(home.path());
        }
        match child.try_wait().expect("poll the child process") {
            Some(status) => {
                panic!(
                    "mnema-desktop exited during start-up with {status} instead \
                     of running to the deadline. A start-up panic (for example \
                     the build_app_menu / app.path() timing this test was \
                     written for) is the regression this half catches — see the \
                     process output above. (Index seen before the exit: \
                     {found:?}.)"
                );
            }
            None if Instant::now() >= deadline => break, // survived → healthy
            None => std::thread::sleep(Duration::from_millis(200)),
        }
    }

    // Started cleanly and stayed up for the whole window. A tray-resident that
    // started cleanly runs until it is quit, so it has no self-exit path here;
    // shut it down before asserting on the filesystem.
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        found.is_some(),
        "No index appeared under the temp home ({}) during a start-up that \
         otherwise ran cleanly for the whole {STARTUP_WINDOW:?} window — so \
         this is not the panic the loop above already ruled out. Either \
         `.setup` does not call `boot_index`, or the environment redirect did \
         not take on this platform — the in-process guard \
         `the_boot_opens_the_index` in `tests/commands.rs` decides which: if \
         it is green, this is the call site or the redirect, not the \
         function.",
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

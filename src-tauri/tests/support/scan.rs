//! The scanning job's own test driver: a fixture no `Channel` answers, so
//! every binary that wants a scan to run to completion under a real
//! `ScanState` shares this ONE driver rather than three copies drifting apart.
//!
//! Declared in `support/mod.rs` — unlike `support/app.rs` and
//! `support/fixture.rs`, which sit beside it and are pulled in with `#[path]`
//! by only the binary that wants them — because all three binaries under
//! `tests/` that drive a job want this: `commands.rs` and `mask_differential.rs`
//! through a real `tauri::App`, `model_commands.rs` through `support::fixture::
//! Fixture`. Both expose an `AppHandle<MockRuntime>` (`App::handle`, `Fixture::
//! handle`), which is what the driver is built on: `AppHandle` is the one of
//! the two that is `Clone` and can be moved into the job observer's closure,
//! which runs on whichever thread the scan announces from — see
//! `run_scan_watching`'s own doc comment.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mnema_desktop::job::EndReason;
use mnema_desktop::scan_job;
use mnema_desktop::scan_state::{Entry, ReadingOutcome, ScanReport, ScanSnapshot, ScanState};
use mnema_desktop::state::AppState;
use tauri::test::MockRuntime;
use tauri::{AppHandle, Manager};

/// How long a scan under test is given before the driver gives up and fails
/// loudly rather than hanging the suite. Generous: `mnema-extract-worker` may
/// still be building on the first test that needs it (`support::worker`'s own
/// lock), and a real walk over a fixture folder is not instant.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Runs a whole scan and answers with what an observer FOUND each time the job
/// slot changed hands, followed by the state the application settled in.
///
/// An observer rather than a channel, because a scan has no channel: the job
/// writes one value and announces, and every surface reads
/// `AppState::scan_state` for itself — `state::JobObserver`'s own doc comment
/// has why nothing is passed to the observer. Recording what was READ at each
/// announcement is therefore recording exactly what a tray or a reopened window
/// would have drawn.
pub fn run_scan_capturing_snapshots(
    handle: &AppHandle<MockRuntime>,
    entry: Entry,
    within: Duration,
) -> (Vec<ScanState>, ScanState) {
    run_scan_watching(handle, entry, within, |_, _| {})
}

/// [`run_scan_capturing_snapshots`] with a hand at the announcement: `watcher`
/// runs on whichever thread announced, which for a progress report is the
/// job's own thread INSIDE `walk_root`.
///
/// That is what makes an interleaving built rather than waited for: there is
/// no sleep anywhere in this function.
pub fn run_scan_watching(
    handle: &AppHandle<MockRuntime>,
    entry: Entry,
    within: Duration,
    watcher: impl Fn(&AppState, &ScanState) + Send + Sync + 'static,
) -> (Vec<ScanState>, ScanState) {
    let handle = handle.clone();
    let seen: Arc<Mutex<Vec<ScanState>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);

    let state = handle.state::<AppState>();
    let observed_handle = handle.clone();
    state.set_job_observer(Box::new(move || {
        let state = observed_handle.state::<AppState>();
        let now = state.scan_state();
        recorder
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(now.clone());
        watcher(state.inner(), &now);
    }));
    scan_job::start_scan_job(state, entry).expect("the scan would not start");

    let deadline = Instant::now() + within;
    while handle.state::<AppState>().job_is_running() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let settled = handle.state::<AppState>().scan_state();
    assert!(
        !handle.state::<AppState>().job_is_running(),
        "the scan never released the job slot: {settled:?}"
    );
    let snapshots = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
    (snapshots, settled)
}

/// The report a finished scan settled on, or a failure naming what was found
/// instead.
pub fn report_of(settled: &ScanState) -> ScanReport {
    match &settled.snapshot {
        ScanSnapshot::Ended { report } => report.clone(),
        other => panic!("the scan did not end with a report: {other:?}"),
    }
}

/// Drives `start_scan_job(Entry::Full)`, under the production deps
/// (`scan_job::start_scan_job` always builds `ScanDeps::production` — there is
/// no seam here to inject a fake one, which is the point: this is the fixture
/// every OTHER test's `run_walk_to_completion` used to be, and a real scan is
/// what actually populates the index the rest of the test reads).
///
/// Asserts the snapshot is `Ended` and `last_reading.reason == Completed`,
/// the same promise `run_walk_to_completion` made — a fixture that used the
/// walk only to get files into the index is not the test that wants to know
/// HOW the scan ended, so this is where that assertion belongs rather than at
/// every call site.
///
/// `#[allow(dead_code)]` for the reason this module's own header gives: this
/// file is compiled into every binary that declares `mod support;`, and
/// `model_commands.rs` drives `Entry::EmbedOnly` exclusively, so it never
/// wants a helper that always asks for `Entry::Full`.
#[allow(dead_code)]
pub fn scan_to_completion(handle: &AppHandle<MockRuntime>) -> ScanState {
    let settled = scan_with(handle, Entry::Full);
    let report = report_of(&settled);
    assert_eq!(
        report.reason,
        EndReason::Completed,
        "the scan meant only to populate the index did not complete: {report:?}"
    );
    let reading = reading_of(&settled);
    assert_eq!(
        reading.reason,
        EndReason::Completed,
        "the reading pass meant only to populate the index did not complete: {reading:?}"
    );
    settled
}

/// [`scan_to_completion`] without the assertion on how it ended — for a test
/// that starts the scan job with `entry` and means to inspect the ending
/// itself, or that drives `Entry::EmbedOnly` the way `start_embed_job` used to
/// be driven directly.
pub fn scan_with(handle: &AppHandle<MockRuntime>, entry: Entry) -> ScanState {
    let (_, settled) = run_scan_capturing_snapshots(handle, entry, DEFAULT_TIMEOUT);
    settled
}

/// The counts the old `Ended` JSON carried — `indexed`, `unchanged`, `skipped`,
/// `removed`, `complete` — off the state a scan settled in.
///
/// Panics rather than answering `None`: every caller already knows a reading
/// pass ran (it just drove one), so a missing `last_reading` here is this
/// fixture's own bug, not a state a test should have to handle.
pub fn reading_of(state: &ScanState) -> &ReadingOutcome {
    state
        .last_reading
        .as_ref()
        .expect("no reading pass has ended in this process yet")
}

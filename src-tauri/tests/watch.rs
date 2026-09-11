//! The folder watcher: measurement (Task 1) and integration (Task 6).
mod support;

// Beside `support/mod.rs` rather than inside it — see that file's own header.
#[path = "support/app.rs"]
mod app;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use app::{app_in, call, main_webview};
use mnema_desktop::job::EndReason;
use mnema_desktop::scan_state::{Entry, ScanSnapshot};
use mnema_desktop::state::AppState;
use mnema_desktop::watch::{MAX_WAIT, QUIET};
use serde_json::json;
use support::scan::{reading_of, run_scan_capturing_snapshots};
use tauri::Manager;

/// §6 of the spec: what one trigger on an unchanged corpus costs. Release
/// profile, so the number is the product's and not the debug build's:
///
///   MNEMA_MEASURE_FILES=100000 cargo test --release -p mnema-desktop --test watch \
///     measure_full_scan_on_unchanged_corpus -- --ignored --nocapture
#[test]
#[ignore]
fn measure_full_scan_on_unchanged_corpus() {
    let n: usize = std::env::var("MNEMA_MEASURE_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);
    // The fixture's 60 s default would cut a 100 000-file first ingest short;
    // the first scan is not the number being measured, so give it an hour.
    let generous = Duration::from_secs(3600);
    let dir = tempfile::tempdir().unwrap();
    let corpus = tempfile::tempdir().unwrap();
    for i in 0..n {
        let sub = corpus.path().join(format!("d{:03}", i % 500));
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join(format!("f{i}.txt")),
            format!("file number {i} says hello"),
        )
        .unwrap();
    }
    let app = app_in(dir.path());
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).unwrap();
    call(
        &webview,
        "add_watched_folder",
        json!({ "path": corpus.path().display().to_string() }),
    )
    .unwrap();

    let t0 = Instant::now();
    run_scan_capturing_snapshots(app.handle(), Entry::Full, generous);
    let first = t0.elapsed();
    let t1 = Instant::now();
    run_scan_capturing_snapshots(app.handle(), Entry::Full, generous);
    let second = t1.elapsed();

    // The synchronous cost of the `watch()` call returning, not time-to-first-event:
    // FSEvents (and the other backends) register asynchronously, so this does not
    // say when the watch is actually live.
    let t2 = Instant::now();
    let mut watcher = notify::recommended_watcher(|_| {}).unwrap();
    notify::Watcher::watch(
        &mut watcher,
        corpus.path(),
        notify::RecursiveMode::Recursive,
    )
    .unwrap();
    let watch_call_return = t2.elapsed();

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    // `first_scan` is walk + extract only: `app_in` builds `AppState` with
    // `NO_PROVIDER`, so `Entry::Full` never reaches an embedding pass.
    println!(
        "MEASURE files={n} profile={profile} provider=none first_scan={first:?} unchanged_scan={second:?} watch_call_return={watch_call_return:?}"
    );
}

/// Counts `Ended` snapshots as the observer sees them.
fn count_ended(app: &tauri::App<tauri::test::MockRuntime>) -> Arc<AtomicUsize> {
    let n = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&n);
    let handle = app.handle().clone();
    app.state::<AppState>().set_job_observer(Box::new(move || {
        if matches!(
            handle.state::<AppState>().scan_state().snapshot,
            ScanSnapshot::Ended { .. }
        ) {
            counter.fetch_add(1, Ordering::SeqCst);
        }
    }));
    n
}

fn wait_until(within: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    done()
}

fn hits(webview: &tauri::WebviewWindow<tauri::test::MockRuntime>, q: &str) -> bool {
    call(webview, "search", json!({ "query": q }))
        .map(|v| !v["hits"].as_array().unwrap().is_empty())
        .unwrap_or(false)
}

/// Index open, roots added, observer set, THEN the watcher — and its startup
/// scan waited for, so the test's own counting starts from a known state.
fn app_watching(
    data: &std::path::Path,
    roots: &[&std::path::Path],
) -> (
    tauri::App<tauri::test::MockRuntime>,
    tauri::WebviewWindow<tauri::test::MockRuntime>,
    Arc<AtomicUsize>,
) {
    let app = app_in(data);
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).unwrap();
    for r in roots {
        call(
            &webview,
            "add_watched_folder",
            json!({ "path": r.display().to_string() }),
        )
        .unwrap();
    }
    let ended = count_ended(&app);
    mnema_desktop::watch::install(app.handle());
    assert!(
        wait_until(Duration::from_secs(60), || ended.load(Ordering::SeqCst)
            >= 1),
        "the startup scan never ended"
    );
    ended.store(0, Ordering::SeqCst);
    (app, webview, ended)
}

struct CloseOnDrop(tauri::AppHandle<tauri::test::MockRuntime>);
impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        self.0.state::<AppState>().watch().close();
    }
}

#[test]
fn a_change_in_a_watched_folder_is_indexed_without_a_button() {
    let dir = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (app, webview, _) = app_watching(dir.path(), &[root.path()]);
    let _close = CloseOnDrop(app.handle().clone());
    std::fs::write(root.path().join("late.txt"), "the quick brown fox").unwrap();
    assert!(
        wait_until(Duration::from_secs(60), || hits(&webview, "fox")),
        "the file written after install was never indexed by the watcher"
    );
}

#[test]
fn the_index_inside_a_watched_root_does_not_retrigger_itself() {
    // Spec review P1-1: WAL/SHM/prefs temp files beside index.sqlite; the
    // scan writes SCAN_INCOMPLETE before it walks anything.
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("app-data");
    std::fs::create_dir(&data).unwrap();
    let (app, webview, ended) = app_watching(&data, &[root.path()]);
    let _close = CloseOnDrop(app.handle().clone());
    std::fs::write(root.path().join("doc2.txt"), "the quick brown fox").unwrap();
    assert!(
        wait_until(Duration::from_secs(60), || hits(&webview, "fox")),
        "positive control: doc2 must be indexed by the triggered scan"
    );
    assert!(wait_until(Duration::from_secs(10), || ended
        .load(Ordering::SeqCst)
        >= 1));
    let after = ended.load(Ordering::SeqCst);
    std::thread::sleep(MAX_WAIT + QUIET + Duration::from_secs(2));
    assert_eq!(
        ended.load(Ordering::SeqCst),
        after,
        "the scan's own writes under app-data re-triggered a scan"
    );
}

#[test]
fn a_root_that_vanishes_ends_the_auto_scan_as_root_unavailable_and_keeps_its_documents() {
    let dir = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.txt"), "the quick brown fox").unwrap();
    let (app, webview, ended) = app_watching(dir.path(), &[&root]);
    let _close = CloseOnDrop(app.handle().clone());
    assert!(
        hits(&webview, "fox"),
        "the startup scan must have indexed it, or there is nothing to protect"
    );
    std::fs::remove_dir_all(&root).unwrap();
    assert!(
        wait_until(Duration::from_secs(60), || ended.load(Ordering::SeqCst)
            >= 1),
        "the removal never triggered a scan"
    );
    // The pass's OWN `reason` stays `Completed` here by design (`scan_state.rs`'s
    // doc on `ReadingOutcome::reason`: a folder that is not there does not stop
    // the pass) — the per-folder `RootUnavailable` is on its own row instead.
    let state = app.state::<AppState>().scan_state();
    let reading = reading_of(&state);
    assert_eq!(reading.roots.len(), 1, "{reading:?}");
    assert_eq!(
        reading.roots[0].reason,
        EndReason::RootUnavailable,
        "{reading:?}"
    );
    assert!(
        hits(&webview, "fox"),
        "a vanished root must not delete its documents"
    );
}

#[test]
fn a_manual_stop_is_not_undone_by_a_change_that_came_before_it() {
    // Decision 3 through the real slot: a write, then Stop before the quiet
    // window ends, then nothing — no scan may start.
    let dir = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (app, _webview, ended) = app_watching(dir.path(), &[root.path()]);
    let _close = CloseOnDrop(app.handle().clone());
    // 2 000 small files so the watcher's own auto-triggered scan below is
    // still `Running` when the write and the Stop land — a one-file corpus
    // finishes before either lands and proves nothing (brief's own timing
    // note). Letting the WATCHER start this scan, rather than starting one
    // by hand, is what keeps the burst's own 2 000 wakes from racing a
    // hand-started scan for the job slot — by the time the loop below
    // observes `Running`, every one of those wakes has already done its one
    // job (triggering this scan) and cannot trigger a second one.
    for i in 0..2000 {
        std::fs::write(root.path().join(format!("f{i}.txt")), "filler").unwrap();
    }
    assert!(
        wait_until(Duration::from_secs(60), || matches!(
            app.state::<AppState>().scan_state().snapshot,
            ScanSnapshot::Running { .. }
        )),
        "the 2 000-file burst never triggered a scan"
    );
    // Write, then Stop, while that scan is still running. The Stop rule
    // compares wake ARRIVAL time against `stopped_at`, not the write's own
    // timestamp — 200ms gives the watcher's backend time to have actually
    // delivered this write's event before Stop is recorded, the same gap any
    // real person clicking Stop right after saving a file would leave, well
    // inside `QUIET` (2 s) so no natural trigger fires first.
    std::fs::write(root.path().join("b.txt"), "b").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    app.state::<AppState>().cancel_job();
    assert!(
        wait_until(Duration::from_secs(60), || ended.load(Ordering::SeqCst)
            >= 1),
        "the triggered scan never ended"
    );
    std::thread::sleep(MAX_WAIT + QUIET + Duration::from_secs(2));
    assert_eq!(
        ended.load(Ordering::SeqCst),
        1,
        "only the stopped scan ended; the pre-Stop change must not restart it"
    );
    std::fs::write(root.path().join("c.txt"), "c").unwrap();
    assert!(
        wait_until(Duration::from_secs(60), || ended.load(Ordering::SeqCst)
            >= 2),
        "positive control: a change after Stop scans"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn reading_every_file_without_changing_one_starts_no_scan() {
    let dir = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "a").unwrap();
    let (app, _webview, ended) = app_watching(dir.path(), &[root.path()]);
    let _close = CloseOnDrop(app.handle().clone());
    for entry in std::fs::read_dir(root.path()).unwrap() {
        let _ = std::fs::read(entry.unwrap().path());
    }
    std::thread::sleep(MAX_WAIT + QUIET + Duration::from_secs(2));
    assert_eq!(ended.load(Ordering::SeqCst), 0, "reads triggered a scan");
    std::fs::write(root.path().join("c.txt"), "c").unwrap();
    assert!(
        wait_until(Duration::from_secs(60), || ended.load(Ordering::SeqCst)
            >= 1),
        "positive control"
    );
}

/// §6, third number: how many scans a minute of saves every 5 s produces.
#[test]
#[ignore]
fn measure_scans_under_periodic_saves() {
    let dir = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (app, _webview, ended) = app_watching(dir.path(), &[root.path()]);
    let _close = CloseOnDrop(app.handle().clone());
    for i in 0..12 {
        std::fs::write(root.path().join("live.txt"), format!("save {i}")).unwrap();
        std::thread::sleep(Duration::from_secs(5));
    }
    std::thread::sleep(MAX_WAIT + QUIET);
    println!(
        "MEASURE saves=12 over=60s scans={}",
        ended.load(Ordering::SeqCst)
    );
}

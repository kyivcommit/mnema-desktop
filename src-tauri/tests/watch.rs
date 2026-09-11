//! The folder watcher: measurement (Task 1) and integration (Task 6).
mod support;

// Beside `support/mod.rs` rather than inside it — see that file's own header.
#[path = "support/app.rs"]
mod app;

use std::time::{Duration, Instant};

use app::{app_in, call, main_webview};
use mnema_desktop::scan_state::Entry;
use serde_json::json;
use support::scan::run_scan_capturing_snapshots;

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

    let t2 = Instant::now();
    let mut watcher = notify::recommended_watcher(|_| {}).unwrap();
    notify::Watcher::watch(
        &mut watcher,
        corpus.path(),
        notify::RecursiveMode::Recursive,
    )
    .unwrap();
    let register = t2.elapsed();

    println!(
        "MEASURE files={n} profile=release first_scan={first:?} unchanged_scan={second:?} watch_register={register:?}"
    );
}

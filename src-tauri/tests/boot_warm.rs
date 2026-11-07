//! The boot path's OCR warm-up (spec R6), through `boot_index` itself: the
//! decision has its own tests beside `warm_ocr_if_scanned`; this one is about
//! the line that calls it. A stand-in worker records the arguments it was
//! started with, then outlives the 10 s page deadline (`RENDER_TIMEOUT`) and says
//! so: finishing proves the boot gave it the scan deadline.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::time::{Duration, Instant};

use mnema_core::SourceKind;
use mnema_desktop::state::AppState;
use tauri::test::{mock_builder, mock_context, noop_assets};

#[test]
fn the_boot_warms_the_ocr_engine_in_the_background_when_the_index_holds_a_scan() {
    mnema_secrets::test_store::register();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("args");
    let done = dir.path().join("done");
    let worker = dir.path().join("worker");
    std::fs::write(
        &worker,
        format!(
            "#!/bin/sh\n[ -n \"$1\" ] || exit 0\necho \"$*\" > '{}'\nsleep 11\necho done > '{}'\n",
            log.display(),
            done.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o755)).unwrap();
    // Run once with no argument (it does nothing), retrying while Linux still
    // says the file is open for writing, so the boot's own start cannot meet that.
    for _ in 0..400 {
        match std::process::Command::new(&worker).status() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(5))
            }
            other => {
                other.expect("the stand-in runs");
                break;
            }
        }
    }
    assert!(!log.exists(), "the pre-run wrote the log");
    {
        let db = mnema_index::open(&mnema_desktop::paths::index_path(dir.path())).unwrap();
        let doc = db
            .insert_document(&"a".repeat(64), "application/pdf", 1, SourceKind::Document)
            .unwrap();
        db.insert_page(&doc, 1, "ocr:vision", None).unwrap();
    }
    let app = mock_builder()
        .manage(AppState::new(
            dir.path().to_path_buf(),
            worker,
            "http://127.0.0.1:1".to_string(),
            format!("mnema-desktop-boot-warm-test-{}", dir.path().display()),
        ))
        .build(mock_context(noop_assets()))
        .expect("the mock application");

    drop(mnema_desktop::boot_index(app.handle()));

    // The boot returns without waiting for the worker; the file appears when
    // the background thread has run it. A generous bound, only for a failure.
    let began = Instant::now();
    while !log.exists() {
        assert!(
            began.elapsed() < Duration::from_secs(60),
            "the boot never started the worker"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // The script writes the file before it exits, and the file is complete
    // once `echo` returns; a short read would only mean the line is not there yet.
    let mut said = String::new();
    while !said.ends_with('\n') {
        assert!(began.elapsed() < Duration::from_secs(60), "{said:?}");
        said = std::fs::read_to_string(&log).unwrap_or_default();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(said, "--warm-ocr\n");
    // Longer than the 10 s a native page is given, shorter than the scan's 300 s.
    while !done.exists() {
        assert!(
            began.elapsed() < Duration::from_secs(40),
            "the worker was killed before it finished: the boot gave it too short a deadline"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

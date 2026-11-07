//! The worker reading a scanned PDF with `--ocr` helpers (D196): the same
//! frames as reading it alone, whatever a helper does.
//!
//! macOS only: elsewhere there is no engine and the worker starts no helper.
#![cfg(target_os = "macos")]

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mnema_extract::OCR_UNAVAILABLE_ENV;
use mnema_extract::ocr_parallel::OCR_PROCESSES_ENV;
use mnema_extract::ocr_wire::{OCR_CRASH_AFTER_ENV, OCR_STALE_AFTER_ENV, OCR_STALL_MS_ENV};

const WORKER: &str = env!("CARGO_BIN_EXE_mnema-extract-worker");

fn request(fixture: &str) -> String {
    serde_json::json!({
        "path": format!("tests/fixtures/{fixture}"),
        "max_bytes": 4_194_304,
    })
    .to_string()
}

/// Everything the worker writes for `fixture`, with `env` set for it (and so
/// for its helpers).
fn frames(fixture: &str, env: &[(&str, &str)]) -> String {
    let mut child = Command::new(WORKER)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the worker starts");
    writeln!(child.stdin.as_mut().unwrap(), "{}", request(fixture)).unwrap();
    let out = child.wait_with_output().expect("the worker exits");
    assert!(
        out.status.success(),
        "{:?}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn three_helpers_give_the_frames_one_process_gives() {
    for fixture in [
        "scan-six-pages.pdf",
        "mixed-native-scan.pdf",
        "interleaved-native-scan.pdf",
    ] {
        let alone = frames(fixture, &[(OCR_PROCESSES_ENV, "1")]);
        let three = frames(fixture, &[(OCR_PROCESSES_ENV, "3")]);
        assert_eq!(three, alone, "{fixture}");
    }
    // And the frames are a document whose scanned pages were read: each page
    // of the six carries its own number.
    let six = frames("scan-six-pages.pdf", &[(OCR_PROCESSES_ENV, "3")]);
    for n in 1..=6 {
        assert!(six.contains(&format!("Page {n} of six")), "page {n}: {six}");
    }
}

/// A helper that dies after one reply loses no page: the one it held is read
/// by the worker, and the rest by the helpers left.
#[test]
fn a_helper_that_crashes_loses_no_page() {
    let alone = frames("scan-six-pages.pdf", &[(OCR_PROCESSES_ENV, "1")]);
    let crashing = frames(
        "scan-six-pages.pdf",
        &[(OCR_PROCESSES_ENV, "3"), (OCR_CRASH_AFTER_ENV, "1")],
    );
    assert_eq!(crashing, alone);
}

/// A helper that answers with the page before the one it was sent is caught
/// by the page number it names: trusted, it would put that page's text under
/// this page's number, a citation of the wrong page. Caught, the page is read
/// in the worker and nothing differs.
#[test]
fn a_helper_answering_for_another_page_is_not_believed() {
    let alone = frames("scan-six-pages.pdf", &[(OCR_PROCESSES_ENV, "1")]);
    let stale = frames(
        "scan-six-pages.pdf",
        &[(OCR_PROCESSES_ENV, "3"), (OCR_STALE_AFTER_ENV, "1")],
    );
    assert_eq!(stale, alone);
}

#[test]
fn an_unavailable_engine_refuses_the_same_way_with_helpers() {
    let env = [(OCR_UNAVAILABLE_ENV, "1")];
    let alone = frames("scan-six-pages.pdf", &[env[0], (OCR_PROCESSES_ENV, "1")]);
    let three = frames("scan-six-pages.pdf", &[env[0], (OCR_PROCESSES_ENV, "3")]);
    assert_eq!(three, alone);
    // One `Failed` frame and nothing else: not a document whose scanned
    // pages are journalled as textless.
    assert_eq!(alone.lines().count(), 1, "{alone}");
    assert!(alone.starts_with(r#"{"frame":"failed""#), "{alone}");
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn children_of(pid: u32) -> Vec<u32> {
    let out = Command::new("pgrep")
        .args(["-P", &pid.to_string()])
        .output()
        .expect("pgrep runs");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect()
}

/// The pool kills a worker that overruns its deadline with SIGKILL, so no
/// `Drop` runs and the helpers must go on their own. Each page here takes a
/// helper 30 s — a cold start's worth — so EOF on stdin, which a helper reads
/// only between pages, cannot be what ends them.
#[test]
fn helpers_do_not_outlive_a_killed_worker() {
    let mut child = Command::new(WORKER)
        .env(OCR_PROCESSES_ENV, "3")
        .env(OCR_STALL_MS_ENV, "30000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{}", request("scan-six-pages.pdf")).unwrap();
    // No frame comes while the first helper stalls (and the rest start only
    // after it answers), so wait for it to appear.
    let started = Instant::now() + Duration::from_secs(10);
    let helpers = loop {
        let helpers = children_of(pid);
        if !helpers.is_empty() {
            break helpers;
        }
        assert!(Instant::now() < started, "a helper starts: {helpers:?}");
        std::thread::sleep(Duration::from_millis(50));
    };

    child.kill().unwrap();
    child.wait().unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while helpers.iter().any(|&h| alive(h)) {
        if Instant::now() > until {
            // Not left behind for the next run to trip over.
            for h in &helpers {
                let _ = Command::new("kill").args(["-9", &h.to_string()]).status();
            }
            panic!("helpers {helpers:?} outlived the worker");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The second helper starts only after the first has answered: helpers
/// starting cold together pay Vision's cold start one after another (eight
/// together: the first page after 295 s, against the pool's 300 s).
#[test]
fn more_helpers_start_only_after_the_first_has_answered() {
    let mut child = Command::new(WORKER)
        .env(OCR_PROCESSES_ENV, "3")
        .env(OCR_STALL_MS_ENV, "1500")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    writeln!(
        child.stdin.as_mut().unwrap(),
        "{}",
        request("scan-six-pages.pdf")
    )
    .unwrap();

    // Inside the first page's 1.5 s: one helper and no more.
    std::thread::sleep(Duration::from_millis(1000));
    let early = children_of(pid);
    // After it: the rest have started. Generous, because the first answer
    // can wait for a cold start, and that is not what is under test.
    let until = Instant::now() + Duration::from_secs(240);
    let later = loop {
        let now = children_of(pid);
        if now.len() == 3 || Instant::now() > until {
            break now;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    drop(child.stdin.take());
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(early.len(), 1, "before any answer: {early:?}");
    assert_eq!(later.len(), 3, "after the first answer: {later:?}");
}

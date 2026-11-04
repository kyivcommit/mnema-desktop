//! `render_page`, the one-shot call, against the stand-in worker's
//! `--render-page` branch: one answer it can give, and the ways it ends wrongly.

#![cfg(unix)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mnema_core::render::{RenderReply, RenderRequest};
use mnema_pool::{PoolError, render_page};

fn worker() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mnema-pool-test-worker"))
}

fn request(path: &str, hash: Option<&str>) -> RenderRequest {
    RenderRequest {
        path: path.to_string(),
        page_no: 2,
        width_px: 640,
        text_sha256: hash.map(str::to_string),
        ranges: Vec::new(),
    }
}

const SECOND: Duration = Duration::from_secs(5);

#[test]
fn the_reply_line_comes_back_with_the_request_it_answered() {
    let reply = render_page(&worker(), &request("ok:/a.pdf", Some("ab")), SECOND).unwrap();
    let RenderReply::Rendered {
        pages,
        width_px,
        png_base64,
        ..
    } = reply
    else {
        panic!("{reply:?}");
    };
    // `pages` is the stand-in's own number; the other two show the request's
    // fields reached it, with and without a hash.
    assert_eq!((pages, width_px, png_base64.as_str()), (7, 640, "hashed"));
    let bare = render_page(&worker(), &request("ok:/a.pdf", None), SECOND).unwrap();
    assert!(matches!(bare, RenderReply::Rendered { png_base64, .. } if png_base64 == "bare"));
}

#[test]
fn a_worker_that_outlives_the_timeout_is_killed_and_gone() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    // One answered call first: the 200 ms below must be spent on the sleeping
    // worker, not on the first start of a cold binary, or the pid is never written.
    render_page(&worker(), &request("ok:/a.pdf", None), SECOND).unwrap();
    let began = Instant::now();
    let error = render_page(
        &worker(),
        &request(&format!("sleep:{}", pid_file.display()), None),
        Duration::from_millis(200),
    )
    .unwrap_err();
    assert!(
        matches!(error, PoolError::RenderTimeout { .. }),
        "{error:?}"
    );
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "{:?}",
        began.elapsed()
    );
    let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
    // Reaped by the call, so the pid names nothing: signal 0 finds no process.
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    assert!(!alive, "worker {pid} is still there after the error");
}

#[test]
fn a_crash_and_a_garbled_line_are_protocol_errors_with_what_the_worker_said() {
    let crashed = render_page(&worker(), &request("exit:3", None), SECOND).unwrap_err();
    let PoolError::Protocol { line, detail } = &crashed else {
        panic!("{crashed:?}");
    };
    assert!(line.contains("pdfium crashed"), "{line}");
    assert!(detail.contains("exit"), "{detail}");

    let garbled = render_page(&worker(), &request("garbage:x", None), SECOND).unwrap_err();
    let PoolError::Protocol { line, .. } = &garbled else {
        panic!("{garbled:?}");
    };
    assert!(line.contains("this is not a reply"), "{line}");
}

#[test]
fn a_worker_that_is_not_there_is_a_spawn_error() {
    let error = render_page(
        std::path::Path::new("/nonexistent/worker"),
        &request("ok:/a.pdf", None),
        SECOND,
    )
    .unwrap_err();
    assert!(matches!(error, PoolError::Spawn { .. }), "{error:?}");
}

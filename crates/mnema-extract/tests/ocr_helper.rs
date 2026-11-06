//! `mnema-extract-worker --ocr`: the helper process a worker hands rendered
//! pages to (D196). A file of its own so that the one test here that renders a
//! page binds Pdfium in a process of its own.
//!
//! Everything except the real scan runs on every platform: the seam
//! `MNEMA_OCR_UNAVAILABLE` gives an answer without Vision.

use std::io::{BufReader, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use mnema_extract::ocr_wire::{OCR_CRASH_AFTER_ENV, Reply, read_reply, write_request};
use mnema_extract::{OCR_UNAVAILABLE_ENV, OcrError, OwnedPageImage};

fn helper(env: &[(&str, &str)]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_mnema-extract-worker"))
        .arg("--ocr")
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the worker binary starts")
}

fn blank(width: u32, height: u32) -> OwnedPageImage {
    OwnedPageImage {
        bgra: vec![255; (width * height * 4) as usize],
        width,
        height,
        stride: width as usize * 4,
        dpi: 300,
    }
}

/// Sends `frames`, closes stdin, and returns every reply and the exit status.
fn exchange(
    mut child: Child,
    frames: &[(u32, &OwnedPageImage)],
) -> (Vec<(u32, Reply)>, ExitStatus) {
    {
        let mut stdin = child.stdin.take().expect("stdin is piped");
        for (seq, image) in frames {
            // A helper that has already exited closes the pipe; the replies
            // and the status below are what the test is about.
            if write_request(&mut stdin, *seq, &image.as_image()).is_err() {
                break;
            }
        }
    }
    let mut out = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let mut replies = Vec::new();
    while let Ok(Some(reply)) = read_reply(&mut out) {
        replies.push(reply);
    }
    (replies, wait_within(&mut child, Duration::from_secs(120)))
}

fn wait_within(child: &mut Child, limit: Duration) -> ExitStatus {
    let until = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return status;
        }
        if Instant::now() > until {
            let _ = child.kill();
            panic!("the helper did not exit within {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_helper_whose_stdin_closes_before_any_frame_exits_cleanly_and_says_nothing() {
    let (replies, status) = exchange(helper(&[]), &[]);
    assert!(replies.is_empty());
    assert!(status.success(), "{status:?}");
}

#[test]
fn each_frame_gets_one_reply_under_its_own_seq_in_order() {
    let (a, b) = (blank(8, 8), blank(4, 6));
    let (replies, status) = exchange(helper(&[(OCR_UNAVAILABLE_ENV, "1")]), &[(5, &a), (6, &b)]);
    assert!(status.success(), "{status:?}");
    let seqs: Vec<u32> = replies.iter().map(|(s, _)| *s).collect();
    assert_eq!(seqs, [5, 6]);
    for (_, reply) in &replies {
        assert!(
            matches!(reply, Err(OcrError::Unavailable(_))),
            "the seam's answer is carried as itself: {reply:?}"
        );
    }
}

#[test]
fn a_helper_given_garbage_exits_with_failure_instead_of_waiting() {
    let mut child = helper(&[]);
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(b"this is not a header\n").unwrap();
        // stdin stays open until here: a helper that waited for more would
        // still be waiting when the pipe closes, and exit 0 — not this.
        let status = wait_within(&mut child, Duration::from_secs(10));
        assert!(!status.success(), "{status:?}");
    }
}

/// The test hook the worker's fallback is tested with (task 6): after `k`
/// replies the helper dies the way a crashed process does.
#[test]
fn the_crash_hook_kills_the_helper_after_k_replies() {
    let image = blank(8, 8);
    let (replies, status) = exchange(
        helper(&[(OCR_UNAVAILABLE_ENV, "1"), (OCR_CRASH_AFTER_ENV, "1")]),
        &[(1, &image), (2, &image), (3, &image)],
    );
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(!status.success(), "{status:?}");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn without_vision_the_helper_answers_no_engine() {
    let (replies, status) = exchange(helper(&[]), &[(9, &blank(8, 8))]);
    assert!(status.success());
    assert_eq!(replies, [(9, Ok(None))]);
}

#[cfg(target_os = "macos")]
#[test]
fn a_scanned_page_sent_to_the_helper_comes_back_read() {
    use std::path::Path;

    use pdfium_render::prelude::*;

    let dir = std::env::var_os(mnema_extract::PDFIUM_LIB_DIR_ENV)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/pdfium/lib"));
    let pdfium = Pdfium::new(
        Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&dir))
            .expect("pdfium binds"),
    );
    let doc = pdfium
        .load_pdf_from_file(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan-uk.pdf"),
            None,
        )
        .expect("fixture opens");
    let page = doc.pages().get(0).expect("page 1");
    let bitmap = page
        .render_with_config(
            &PdfRenderConfig::new()
                .scale_page_by_factor(300.0 / 72.0)
                .set_format(PdfBitmapFormat::BGRA),
        )
        .expect("renders");
    let bgra = bitmap.as_raw_bytes();
    let (width, height) = (bitmap.width() as u32, bitmap.height() as u32);
    let image = OwnedPageImage {
        stride: bgra.len() / height as usize,
        bgra,
        width,
        height,
        dpi: 300,
    };

    let (replies, status) = exchange(helper(&[]), &[(17, &image)]);
    assert!(status.success(), "{status:?}");
    let [(seq, Ok(Some(read)))] = replies.as_slice() else {
        panic!("one recognised reply expected: {replies:?}");
    };
    assert_eq!(*seq, 17);
    // The words `tests/ocr.rs` measured to survive Vision verbatim.
    for word in ["їжак", "єнот", "мʼясо", "памʼятна", "collegium"] {
        assert!(
            read.text.contains(word),
            "{word:?} missing from {:?}",
            read.text
        );
    }
    assert_eq!(
        (read.width_px, read.height_px, read.dpi),
        (width, height, 300)
    );
}

/// A helper whose worker dies while its stdin is still open elsewhere — or,
/// the case this is for, while it is inside Vision and will not read stdin
/// for a whole cold start — exits on its own: orphaned, it has no one to
/// answer.
#[cfg(unix)]
#[test]
fn a_helper_whose_parent_dies_exits_without_waiting_for_eof() {
    use std::io::BufRead;

    // `sh` starts the helper with sh's own stdin (the pipe this test holds
    // open, so EOF never comes), prints its pid, and is then killed.
    let mut sh = Command::new("sh")
        .args([
            "-c",
            r#"exec 3<&0; "$0" --ocr <&3 & echo $!; sleep 60"#,
            env!("CARGO_BIN_EXE_mnema-extract-worker"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let _held_open = sh.stdin.take().unwrap();
    let mut line = String::new();
    BufReader::new(sh.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let helper: u32 = line.trim().parse().expect("the helper's pid");
    sh.kill().unwrap();
    sh.wait().unwrap();

    let alive = || {
        Command::new("kill")
            .args(["-0", &helper.to_string()])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let until = Instant::now() + Duration::from_secs(2);
    while alive() {
        if Instant::now() > until {
            let _ = Command::new("kill")
                .args(["-9", &helper.to_string()])
                .status();
            panic!("the orphaned helper {helper} is still running");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

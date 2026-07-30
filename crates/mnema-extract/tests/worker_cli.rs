//! The two tests task-7-brief.md specifies for the worker binary, verbatim,
//! plus the edge cases its "worth thinking about" section calls for: an
//! empty file, a missing path, a directory, an unreadable file (standing in
//! for the harder-to-reproduce race where a file vanishes between the
//! request and the read — see the report for why), a size exactly at the
//! ceiling, and a recognised-but-unimplemented reader.

use std::io::Write;
use std::process::{Command, Stdio};

use mnema_extract::wire::Frame;

/// Runs the compiled worker binary against `lines`, one request per line on
/// its stdin, and returns everything it wrote to stdout. Asserts the process
/// exited cleanly: none of the scenarios this file exercises should ever
/// crash the worker, only report a frame.
fn run_worker(lines: &[&str]) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mnema-extract-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the worker binary starts");

    {
        let stdin = child.stdin.as_mut().expect("stdin is piped");
        for line in lines {
            writeln!(stdin, "{line}").expect("the worker keeps reading stdin");
        }
    }

    let output = child.wait_with_output().expect("the worker exits");
    assert!(
        output.status.success(),
        "worker exited with {:?}, stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout is UTF-8")
}

fn frames_of(out: &str) -> Vec<Frame> {
    out.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn the_worker_answers_one_file_with_a_header_blocks_and_a_summary() {
    let out = run_worker(&["{\"path\":\"tests/fixtures/simple.txt\",\"max_bytes\":1048576}"]);
    let frames: Vec<Frame> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(matches!(frames.first(), Some(Frame::Header { .. })));
    assert!(matches!(frames.last(), Some(Frame::Summary { .. })));
}

#[test]
fn a_file_over_the_ceiling_is_refused_without_being_read() {
    let out = run_worker(&["{\"path\":\"tests/fixtures/big.txt\",\"max_bytes\":16}"]);
    // `too_large`, and not `unsupported`, which is what this branch reported
    // until the two were found to want opposite things of the index. The
    // parent removes what it holds under a path when a worker read a file and
    // declined its content; this branch decides from `stat` on a configured
    // number, so lowering `max_bytes` must not delete anything. The rule string
    // is the only thing that carries the distinction across the wire —
    // `mnema-extract` may not depend on `mnema-index` — so it is asserted here
    // rather than left to the parent's mapping.
    match frames_of(&out).as_slice() {
        [Frame::Refused { rule, reason }] => {
            assert_eq!(rule, "too_large", "a size ceiling must have its own rule");
            assert!(reason.contains("ceiling"), "{reason}");
        }
        other => panic!("expected exactly one refusal, got {other:?}"),
    }
}

// --- Supplementary: the edges the brief asks to reason through ---

#[test]
fn the_header_hashes_the_bytes_actually_on_disk() {
    use sha2::{Digest, Sha256};

    let out = run_worker(&["{\"path\":\"tests/fixtures/simple.txt\",\"max_bytes\":1048576}"]);
    let frames = frames_of(&out);
    let Some(Frame::Header { sha256, .. }) = frames.first() else {
        panic!("expected a Header frame, got {:?}", frames.first());
    };

    let bytes = std::fs::read("tests/fixtures/simple.txt").unwrap();
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let expected = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();

    assert_eq!(
        *sha256, expected,
        "the worker must hash the same bytes it reads, not a placeholder"
    );
}

#[test]
fn a_text_file_opens_exactly_one_untitled_page_before_its_blocks() {
    let out = run_worker(&["{\"path\":\"tests/fixtures/simple.txt\",\"max_bytes\":1048576}"]);
    let frames = frames_of(&out);
    assert!(
        frames.len() >= 4,
        "the fixture has two paragraphs: header, page, >=1 block, summary"
    );
    // The page frame comes before the first block and there is exactly one of
    // them: plain text is one page (D37), and the pool checks that count
    // against the header rather than trusting the blocks to imply it.
    assert!(
        matches!(
            frames[1],
            Frame::Page {
                page_no: 1,
                section_title: None
            }
        ),
        "the frame after the header must open page 1: {:?}",
        frames[1]
    );
    for frame in &frames[2..frames.len() - 1] {
        assert!(
            matches!(frame, Frame::Block(_)),
            "unexpected frame: {frame:?}"
        );
    }
}

/// Markdown is the first format to send more than one page, and the header's
/// count is what the pool checks the page frames against — so the two are
/// asserted together here, at the only place they are produced.
#[test]
fn a_markdown_file_announces_one_page_per_section_and_sends_that_many() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("звіт.md");
    std::fs::write(
        &path,
        "вступ\n\n# Розділ перший\n\nтекст\n\n# Розділ другий\n\nінший текст\n",
    )
    .unwrap();

    let request = format!(
        "{{\"path\":{:?},\"max_bytes\":1048576}}",
        path.display().to_string()
    );
    let frames = frames_of(&run_worker(&[&request]));

    let Some(Frame::Header { pages, mime, .. }) = frames.first() else {
        panic!("expected a header, got {:?}", frames.first());
    };
    assert_eq!(mime, "text/markdown");
    assert_eq!(*pages, 3, "the content before the first heading is page 1");

    let sent: Vec<&Frame> = frames
        .iter()
        .filter(|f| matches!(f, Frame::Page { .. }))
        .collect();
    assert_eq!(
        sent.len(),
        3,
        "the header's count must be the frames' count"
    );
    assert!(matches!(
        sent[1],
        Frame::Page {
            page_no: 2,
            section_title: Some(title)
        } if title == "Розділ перший"
    ));

    let Some(Frame::Summary { text_source, .. }) = frames.last() else {
        panic!("expected a summary, got {:?}", frames.last());
    };
    // `native:md`, not `native:txt`: `page.text_source` records which reader
    // produced the text, and the same bytes read two ways are not the same
    // evidence.
    assert_eq!(text_source, "native:md");
}

#[test]
fn a_file_exactly_at_the_ceiling_is_still_read() {
    let exact = std::fs::metadata("tests/fixtures/simple.txt")
        .unwrap()
        .len();
    let request = format!("{{\"path\":\"tests/fixtures/simple.txt\",\"max_bytes\":{exact}}}");
    let out = run_worker(&[&request]);
    let frames = frames_of(&out);
    assert!(matches!(frames.first(), Some(Frame::Header { .. })));
    assert!(matches!(frames.last(), Some(Frame::Summary { .. })));
}

#[test]
fn an_empty_file_still_gets_a_header_a_page_and_a_summary_with_no_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.txt");
    std::fs::write(&path, b"").unwrap();

    let request = format!(
        "{{\"path\":{:?},\"max_bytes\":1048576}}",
        path.display().to_string()
    );
    let out = run_worker(&[&request]);
    let frames = frames_of(&out);
    assert_eq!(
        frames.len(),
        3,
        "no lines, no blocks — but still a clean header, its page and a summary, not silence"
    );
    assert!(matches!(frames[0], Frame::Header { pages: 1, .. }));
    // The page still opens, with nothing under it. `page.text_source` is NOT
    // NULL and the header promised one page, so an empty file that sent no
    // page frame would be a document the pool rejects as truncated.
    assert!(matches!(frames[1], Frame::Page { page_no: 1, .. }));
    assert!(matches!(frames[2], Frame::Summary { .. }));
}

#[test]
fn a_missing_path_is_reported_not_crashed() {
    let out =
        run_worker(&["{\"path\":\"tests/fixtures/does-not-exist.txt\",\"max_bytes\":1048576}"]);
    let frames = frames_of(&out);
    assert_eq!(frames.len(), 1);
    assert!(
        matches!(frames[0], Frame::Failed { .. }),
        "a missing file is an I/O failure, not a content refusal: {:?}",
        frames[0]
    );
}

#[test]
fn a_directory_is_refused_as_not_a_regular_file() {
    let out = run_worker(&["{\"path\":\"tests/fixtures\",\"max_bytes\":1048576}"]);
    let frames = frames_of(&out);
    assert_eq!(frames.len(), 1);
    assert!(matches!(frames[0], Frame::Failed { .. }));
}

#[cfg(unix)]
#[test]
fn a_file_without_read_permission_is_reported_not_crashed() {
    // Stands in for the harder-to-reproduce race where a file vanishes
    // between the request and the read: `fs::metadata` succeeds (stat does
    // not require read permission), and the failure only surfaces at
    // `fs::read`, exercising the same code path a genuine TOCTOU vanish
    // would hit.
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unreadable.txt");
    std::fs::write(&path, b"invented content nobody may read").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

    let request = format!(
        "{{\"path\":{:?},\"max_bytes\":1048576}}",
        path.display().to_string()
    );
    let out = run_worker(&[&request]);

    // Restore permissions so the tempdir can clean itself up regardless of
    // the assertion outcome.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    let frames = frames_of(&out);
    assert_eq!(frames.len(), 1);
    assert!(matches!(frames[0], Frame::Failed { .. }));
}

#[cfg(unix)]
#[test]
fn a_named_pipe_is_refused_promptly_not_read() {
    // `fs::read` does not error on a FIFO the way it does on a directory:
    // with no writer, the read blocks forever. If the `is_file()` guard
    // ahead of it were ever removed, this worker would hang holding the
    // request instead of reporting an outcome — and once task 8's pool can
    // kill a hung worker on a timeout, that would look like "a slow file",
    // not the defect it is. This test bounds its own wait so a hang fails
    // loudly here instead of wedging CI, and kills the child if it does
    // hang so a regression does not also leak a process.
    use std::sync::mpsc;
    use std::time::Duration;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pipe.fifo");
    let status = Command::new("mkfifo").arg(&path).status().unwrap();
    assert!(
        status.success(),
        "mkfifo must be available to build this fixture"
    );

    let request = format!(
        "{{\"path\":{:?},\"max_bytes\":1048576}}",
        path.display().to_string()
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_mnema-extract-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the worker binary starts");

    {
        let stdin = child.stdin.as_mut().expect("stdin is piped");
        writeln!(stdin, "{request}").expect("the worker keeps reading stdin");
    }

    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    let output = match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(result) => result.expect("waiting on the worker does not itself fail"),
        Err(_) => {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
            panic!(
                "the worker did not exit within 5s — it is blocked reading the FIFO, \
                 meaning fs::read ran before the is_file() guard rejected it"
            );
        }
    };

    assert!(
        output.status.success(),
        "worker exited with {:?}, stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let out = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let frames = frames_of(&out);
    assert_eq!(frames.len(), 1);
    assert!(
        matches!(frames[0], Frame::Failed { .. }),
        "a FIFO is not a regular file: {:?}",
        frames[0]
    );
}

#[test]
fn a_bare_zip_with_no_recognizable_member_is_refused_as_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("not-really-a-docx.docx");
    {
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
        writer.start_file("readme.txt", opts).unwrap();
        writer.write_all(b"not an office document").unwrap();
        writer.finish().unwrap();
    }

    let request = format!(
        "{{\"path\":{:?},\"max_bytes\":1048576}}",
        path.display().to_string()
    );
    let out = run_worker(&[&request]);
    let frames = frames_of(&out);
    assert_eq!(frames.len(), 1);
    match &frames[0] {
        Frame::Refused { rule, .. } => assert_eq!(rule, "unsupported"),
        other => panic!("expected Refused, got {other:?}"),
    }
}

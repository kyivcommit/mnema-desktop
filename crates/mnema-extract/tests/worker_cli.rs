//! The two tests task-7-brief.md specifies for the worker binary, verbatim,
//! plus the edge cases its "worth thinking about" section calls for: an
//! empty file, a missing path, a directory, an unreadable file (standing in
//! for the harder-to-reproduce race where a file vanishes between the
//! request and the read — see the report for why), a size exactly at the
//! ceiling, and a recognised-but-unimplemented reader.

use std::fmt::Write as _;
use std::io::Write;
use std::process::{Command, Stdio};

use mnema_extract::wire::Frame;
use sha2::{Digest, Sha256};

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
        [
            Frame::Refused {
                rule,
                reason,
                sha256,
            },
        ] => {
            assert_eq!(rule, "too_large", "a size ceiling must have its own rule");
            assert!(reason.contains("ceiling"), "{reason}");
            // "Refused without being read" is the name of this test and a real
            // property of the branch, and this is what pins it: a digest here
            // could only have come from reading the file the ceiling exists to
            // avoid reading.
            assert_eq!(
                *sha256, None,
                "the ceiling decides from stat, so there is nothing it could have hashed"
            );
        }
        other => panic!("expected exactly one refusal, got {other:?}"),
    }
}

/// D51, end to end through the real binary — the same way the defect was
/// found rather than read. Before this, a genuine photo came back as
/// `mime: text/plain`, `source_kind: document`, one block holding the file's
/// bytes as Latin-1 mojibake: no skip, no journal row, no refusal.
#[test]
fn a_photo_is_refused_by_the_real_worker() {
    let out = run_worker(&["{\"path\":\"tests/fixtures/solid.png\",\"max_bytes\":1048576}"]);
    let frames = frames_of(&out);

    assert_eq!(frames.len(), 1, "a refusal is the whole answer: {frames:?}");
    match &frames[0] {
        Frame::Refused {
            rule,
            reason,
            sha256,
        } => {
            assert_eq!(rule, "not_text");
            assert!(
                reason.contains("not text"),
                "the reason is what the window shows a person: {reason:?}"
            );
            // The digest of the bytes this verdict was reached on, and the
            // parent cannot do without it: it is the only thing that tells a
            // file which *became* unindexable from a file that never changed
            // while the rule under it did. The second of those loses a
            // document if this field is missing, because a parent that cannot
            // see the bytes assumes they moved.
            //
            // Asserted against the fixture's own digest rather than merely
            // `is_some()`: a worker that sent a constant, or the digest of the
            // wrong buffer, would satisfy the weaker check.
            let mut hasher = Sha256::new();
            hasher.update(std::fs::read("tests/fixtures/solid.png").unwrap());
            let want = hasher
                .finalize()
                .iter()
                .fold(String::with_capacity(64), |mut s, b| {
                    let _ = write!(s, "{b:02x}");
                    s
                });
            assert_eq!(sha256.as_deref(), Some(want.as_str()));
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Every refusal this worker reaches **by reading the file** carries the digest
/// of what it read — all five of them, not the one that happened to get a test.
///
/// The digest was pinned on the `not_text` branch alone, and the blindness that
/// left behind is structural rather than an oversight. Both of the parent's
/// deterministic witnesses for it (`crates/mnema-ingest/tests/slice.rs`, the two
/// tests that stage a rule change) put a shell script where the worker goes and
/// have it print a digest they chose. They prove `displaces` **consumes** the
/// field correctly; not one of them proves anything **produces** it. Measured:
/// dropping `sha256` from the `unsupported` branch left the whole workspace
/// green — 0 failed, `--no-fail-fast`.
///
/// What that costs if it is ever dropped is the defect the branch before this
/// one was written to close, arriving from the other end. `displaces` reads a
/// missing digest as "the bytes are unknown, so displace", so a folder of PDFs
/// indexed by a build that has the reader and walked by a build that does not
/// would lose a document per file, with the bytes never having moved.
///
/// `too_large` is deliberately not in this table and has the opposite assertion
/// of its own, above: that branch answers from `stat` without opening the file,
/// so a digest there could only have come from reading what the ceiling exists
/// not to read.
///
/// The table is written out by hand, which is the one thing this test cannot
/// fix: a reader refusing under a new rule has to be added here by whoever adds
/// it. What it does close is that no branch reachable today is judged by
/// nothing. The three PDF rows arrived that way — the pdf reader turned one row
/// (`one-page-text.pdf`, then `unsupported`) into three refusals by content,
/// and a row left behind would have been a branch nothing measured.
#[test]
fn every_refusal_that_read_the_file_carries_the_digest_it_read() {
    let dir = tempfile::tempdir().unwrap();
    // An interrupted append: prose well past `HEAD_BYTES`, then a zeroed tail.
    // Built here rather than checked in — it is derived from a rule this
    // repository already states, and a blob in `tests/fixtures` would be one
    // more thing that has to be believed.
    let interrupted = dir.path().join("note.txt");
    let mut bytes = "Нотатка про засідання: ухвалили перенести терміни.\n"
        .repeat(20)
        .into_bytes();
    assert!(bytes.len() > 512, "the prose must outlast the head window");
    bytes.extend(std::iter::repeat_n(0u8, 64));
    std::fs::write(&interrupted, &bytes).unwrap();

    // A PDF by its magic bytes and nothing else after them. Built here for the
    // same reason as the file above: it is derived from a rule already stated
    // (`typing::identify` decides a PDF on `%PDF-`), and a blob in `fixtures`
    // would be one more thing to believe.
    let damaged = dir.path().join("zvit.pdf");
    std::fs::write(&damaged, b"%PDF-1.4\nthis document ends mid-object").unwrap();

    for (path, want_rule) in [
        ("tests/fixtures/solid.png", "not_text"),
        // `one-page-text.pdf` used to be this row, under `unsupported`. It is
        // not refused at all any more — the pdf reader reads it — so the row
        // moved to the PDFs that *are* refused after being read, which is
        // three rules rather than one. Each is a verdict about content, so
        // each owes the digest it was reached on: without it `displaces`
        // reads a missing digest as "the bytes are unknown, displace", and a
        // folder of scans walked by a build whose Pdfium is a version behind
        // loses a document per file with the bytes never having moved.
        ("tests/fixtures/all-scanned.pdf", "no_text_layer"),
        ("tests/fixtures/password-locked.pdf", "encrypted"),
        (damaged.to_str().expect("a temp path is UTF-8"), "malformed"),
        (
            interrupted.to_str().expect("a temp path is UTF-8"),
            "binary_tail",
        ),
    ] {
        let request = serde_json::json!({ "path": path, "max_bytes": 1_048_576 });
        let out = run_worker(&[&request.to_string()]);
        match frames_of(&out).as_slice() {
            [
                Frame::Refused {
                    rule,
                    sha256,
                    reason: _,
                },
            ] => {
                assert_eq!(rule, want_rule, "wrong rule for {path}");
                // Against the file's own digest, not `is_some()`: a worker
                // sending a constant, or the digest of some other buffer, would
                // satisfy the weaker check and lose exactly the documents this
                // field exists to keep.
                assert_eq!(
                    sha256.as_deref(),
                    Some(digest_of(path).as_str()),
                    "{path} was refused as {want_rule} without the digest it was refused on"
                );
            }
            other => panic!("expected exactly one refusal for {path}, got {other:?}"),
        }
    }
}

/// Lower-case hex of a file's sha256 — what `document.id` is, read off the disk
/// rather than out of anything the worker said.
fn digest_of(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(std::fs::read(path).unwrap());
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// The other direction, in the same shape: a text file still comes back with
/// a header and blocks. Without this, a worker that refused everything would
/// pass the test above — the shape that went unnoticed nine times in the
/// previous branch.
#[test]
fn a_text_file_is_still_read_by_the_real_worker() {
    let out = run_worker(&["{\"path\":\"tests/fixtures/simple.txt\",\"max_bytes\":1048576}"]);
    let frames = frames_of(&out);

    assert!(
        matches!(frames.first(), Some(Frame::Header { .. })),
        "expected a header, got {frames:?}"
    );
    assert!(!frames.iter().any(|f| matches!(f, Frame::Refused { .. })));
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

/// The PDF branch's whole wire shape, at the only place it is produced.
///
/// `reader` is the assertion that matters most here and the one nothing else
/// in the workspace can make. `mnema_ingest::pages_of` picks a PDF chunk's
/// coordinate by matching this exact string, across a process boundary and
/// across D40 — so there is no compiler between the two, and a header saying
/// `"pdf-2"` would send every PDF citation to `PageContext::Lines`, which
/// answers `Coordinate::None` for a block with no line numbers. Plausible,
/// silent, and green everywhere else.
///
/// The literal `"pdf"` rather than `manifest::READER_PDF`: a test that asks
/// the code under test what it says and agrees is not a test. The constant is
/// the mechanism, this is the value, and `mnema-ingest/tests/slice.rs` states
/// the same literal from the other side.
#[test]
fn a_pdf_is_read_and_its_header_names_the_pdf_reader() {
    let request = serde_json::json!({
        "path": "tests/fixtures/one-page-text.pdf",
        "max_bytes": 1_048_576,
    });
    let frames = frames_of(&run_worker(&[&request.to_string()]));

    let Some(Frame::Header {
        reader,
        reader_version,
        pages,
        mime,
        ..
    }) = frames.first()
    else {
        panic!("expected a header, got {:?}", frames.first());
    };
    assert_eq!(reader, "pdf");
    assert_eq!(*reader_version, 1);
    assert_eq!(*pages, 1);
    assert_eq!(mime, "application/pdf");

    match &frames[1] {
        Frame::Page {
            page_no,
            section_title,
        } => {
            assert_eq!(*page_no, 1);
            // A PDF page is not a section: `pages_of` cites `Coordinate::Page`
            // for this reader, and a title here would be furniture nobody
            // asked for.
            assert_eq!(*section_title, None);
        }
        other => panic!("expected a page frame, got {other:?}"),
    }

    match &frames[2] {
        Frame::Block(block) => {
            assert!(
                block.text.contains("Northwind Depot"),
                "the fixture's own words must survive the wire: {:?}",
                block.text
            );
            // Not `is_none()` on one of them: a reader that filled in a line
            // range would be cited as rows of a page that has none.
            assert_eq!((block.line_start, block.line_end), (None, None));
        }
        other => panic!("expected a block frame, got {other:?}"),
    }

    let Some(Frame::Summary {
        skipped_pages,
        text_source,
    }) = frames.last()
    else {
        panic!("expected a summary, got {:?}", frames.last());
    };
    assert_eq!(*skipped_pages, 0);
    // `native:pdf`, satisfying `page.text_source`'s CHECK and naming the
    // reader rather than the file — the same rule `native:md` follows.
    assert_eq!(text_source, "native:pdf");
}

/// A PDF that lost a page in the middle: the gap reaches the wire, the header
/// counts what arrived, and the summary counts what did not.
///
/// This is the pool's own integrity check exercised at its producer: it
/// requires `Header::pages` to equal the number of `Page` frames, and it does
/// **not** look at the largest `page_no`. A reader that announced 3 because the
/// document has three pages would stop the job.
#[test]
fn a_skipped_pdf_page_leaves_a_gap_and_is_counted_rather_than_announced() {
    let request = serde_json::json!({
        "path": "tests/fixtures/text-stamp-text.pdf",
        "max_bytes": 1_048_576,
    });
    let frames = frames_of(&run_worker(&[&request.to_string()]));

    let Some(Frame::Header { pages, .. }) = frames.first() else {
        panic!("expected a header, got {:?}", frames.first());
    };
    let sent: Vec<u32> = frames
        .iter()
        .filter_map(|f| match f {
            Frame::Page { page_no, .. } => Some(*page_no),
            _ => None,
        })
        .collect();

    assert_eq!(
        sent,
        vec![1, 3],
        "the skipped page leaves a gap, which `Frame::Page`'s doc calls the honest record"
    );
    assert_eq!(
        *pages,
        sent.len() as u32,
        "the header counts the page frames that arrive, not the document's pages"
    );

    let Some(Frame::Summary { skipped_pages, .. }) = frames.last() else {
        panic!("expected a summary, got {:?}", frames.last());
    };
    assert_eq!(*skipped_pages, 1);
}

/// A scan of a paper document is refused after being read, under a rule about
/// content — not `unsupported`, which promises a reader that is coming when
/// the reader is already here and found no text.
#[test]
fn a_pdf_with_no_text_layer_on_any_page_is_refused_under_its_own_rule() {
    let request = serde_json::json!({
        "path": "tests/fixtures/all-scanned.pdf",
        "max_bytes": 1_048_576,
    });
    let frames = frames_of(&run_worker(&[&request.to_string()]));

    match frames.as_slice() {
        [Frame::Refused { rule, reason, .. }] => {
            assert_eq!(rule, "no_text_layer");
            // The threshold it failed, in the sentence a person reads: "no
            // text" alone does not distinguish a scan from an empty file, and
            // the number is the product decision they may want to argue with.
            assert!(
                reason.contains(&mnema_extract::TEXT_LAYER_MIN_CHARS.to_string()),
                "the refusal must name the threshold it applied: {reason}"
            );
        }
        other => panic!("expected exactly one refusal, got {other:?}"),
    }
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

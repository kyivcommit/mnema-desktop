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
/// of what it read — all six of them, not the one that happened to get a test.
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

    // Three books, for the three ways this reader refuses one after reading it.
    // Built here for the same reason as the two files above: each is derived
    // from a rule this repository already states.
    let pictures = dir.path().join("albom.epub");
    std::fs::write(
        &pictures,
        epub_bytes(&[(
            "cover.xhtml",
            Some("<html><head><title>Обкладинка</title></head><body><img src=\"c.jpg\"/></body></html>"),
        )]),
    )
    .unwrap();

    // A zip carrying the `mimetype` entry `is_epub` requires and nothing that
    // makes it a book — no container, so no spine, so nothing to read.
    let not_a_book = dir.path().join("nedokniga.epub");
    std::fs::write(&not_a_book, epub_bytes(&[])).unwrap();

    // A chapter that inflates past `zip_part::MEMBER_MAX_BYTES` out of an
    // archive small enough to sail through the request's own ceiling.
    let bomb = dir.path().join("bomba.epub");
    let huge = "a".repeat(20 << 20);
    std::fs::write(&bomb, epub_bytes(&[("ch1.xhtml", Some(&huge))])).unwrap();
    assert!(
        std::fs::metadata(&bomb).unwrap().len() < 1_048_576,
        "the archive itself must pass the ceiling, or this row measures the wrong branch"
    );

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
        // The second way into `malformed`, and it is not the first one over
        // again: this file *is* a document — pdfium loaded it, read page 1 and
        // declined page 2. It reached the refusal from the middle of a page
        // loop rather than from the load, which is a different branch and owes
        // the same digest.
        ("tests/fixtures/unloadable-middle-page.pdf", "malformed"),
        (
            interrupted.to_str().expect("a temp path is UTF-8"),
            "binary_tail",
        ),
        // **The three rows the epub reader owes**, because this table is
        // written out by hand and a branch left out of it is a branch nothing
        // measures. All three are verdicts about content — the file was opened
        // — so all three owe the digest they were reached on.
        (
            pictures.to_str().expect("a temp path is UTF-8"),
            "no_text_layer",
        ),
        (
            not_a_book.to_str().expect("a temp path is UTF-8"),
            "malformed",
        ),
        // **`too_large` reached a second way, and this is the row that says so.**
        // The branch above `not_text` decides from `stat` without opening the
        // file, and carries no digest precisely because of it. This one is a cap
        // on what a *member* inflates to: the archive is a few kilobytes and
        // passes the request's ceiling comfortably, one chapter inside it does
        // not, and the file really was read. Same rule string, because it is the
        // same answer to the person holding it — and harmless to carry a digest
        // under, because `displaces` decides `TooLarge` on size and mtime and
        // never looks at the digest (`crates/mnema-ingest/src/lib.rs:1200-1203`).
        (bomb.to_str().expect("a temp path is UTF-8"), "too_large"),
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
    assert!(skipped_pages.is_empty());
    // `native:pdf`, satisfying `page.text_source`'s CHECK and naming the
    // reader rather than the file — the same rule `native:md` follows.
    assert_eq!(text_source, "native:pdf");
}

/// A PDF that lost a page in the middle: the gap reaches the wire, the header
/// counts what arrived, and the summary **names** what did not.
///
/// This is the pool's own integrity check exercised at its producer: it
/// requires `Header::pages` to equal the number of `Page` frames, and it does
/// **not** look at the largest `page_no`. A reader that announced 3 because the
/// document has three pages would stop the job.
///
/// The summary assertion is the whole vector, and it constrains both
/// directions: the page that was skipped is named, and the two that were not
/// are absent. `skipped_pages.len() == 1` is satisfied by `[1]`, which would be
/// a journal row against a page the index holds and cites.
#[test]
fn a_skipped_pdf_page_leaves_a_gap_and_is_named_rather_than_announced() {
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
    assert_eq!(
        skipped_pages,
        &vec![2],
        "the middle page is the one without a text layer, and the summary is \
         the only place its number can leave this process"
    );
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

/// The HTML branch's whole wire shape, at the only place it is produced.
///
/// **The one format that was answered wrongly rather than refused.** Before
/// this branch existed, `.html` reached `Reader::PlainText` and the worker sent
/// `reader: "text"`, `mime: "text/plain"`, `native:txt` and one block holding
/// the file's markup — measured in spec §2.1 against a shipped build. Every
/// field below is one of the four things that changed, and each is checked at
/// its value rather than against the code that produces it.
///
/// `reader` is the assertion that matters most and the one nothing else in the
/// workspace can make. `mnema_ingest::pages_of` matches this exact string to
/// cite an HTML chunk as `Coordinate::Section`, across a process boundary and
/// across D40 — a header saying `"html-2"` falls to `PageContext::Lines`, which
/// asks blocks that carry no line numbers for a line range and answers
/// `Coordinate::None`. The literal `"html"` rather than `manifest::READER_HTML`
/// on purpose: a test that asks the code under test what it says and then
/// agrees is not a test. The constant is the mechanism, this is the value, and
/// `mnema-ingest/tests/slice.rs` states the same literal from the other side.
#[test]
fn an_html_file_is_read_as_prose_and_its_header_names_the_html_reader() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("звіт.html");
    std::fs::write(
        &path,
        "<html><head><title>Річний звіт</title><style>.a{color:red}</style></head>\
         <body><p>Вступ до звіту.</p><h1>Розділ перший</h1><p>Виторг зріс.</p>\
         <script>var x=1;</script></body></html>",
    )
    .unwrap();

    let request = format!(
        "{{\"path\":{:?},\"max_bytes\":1048576}}",
        path.display().to_string()
    );
    let frames = frames_of(&run_worker(&[&request]));

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
    assert_eq!(reader, "html");
    assert_eq!(*reader_version, 1);
    // Not `text/plain`, which is what this file used to be called.
    assert_eq!(mime, "text/html");
    // Two sections: the document's title names the first, the heading the
    // second. The pool checks this count against the page frames that arrive.
    assert_eq!(*pages, 2);

    let sent: Vec<&Frame> = frames
        .iter()
        .filter(|f| matches!(f, Frame::Page { .. }))
        .collect();
    assert_eq!(
        sent.len(),
        2,
        "the header's count must be the frames' count"
    );
    assert!(
        matches!(
            sent[1],
            Frame::Page {
                page_no: 2,
                section_title: Some(title),
            } if title == "Розділ перший"
        ),
        // Unlike a PDF's, an HTML page carries a section title: it is the whole
        // of what a citation into this format points at.
        "{:?}",
        sent[1]
    );

    let prose: String = frames
        .iter()
        .filter_map(|f| match f {
            Frame::Block(block) => Some(block.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    // Both directions across the wire: the markup is gone and the prose is not.
    assert!(!prose.contains("color:red"), "{prose:?}");
    assert!(!prose.contains("var x"), "{prose:?}");
    assert!(prose.contains("Виторг зріс."), "{prose:?}");
    assert!(prose.contains("Вступ до звіту."), "{prose:?}");

    let Some(Frame::Summary {
        skipped_pages,
        text_source,
    }) = frames.last()
    else {
        panic!("expected a summary, got {:?}", frames.last());
    };
    // Empty rather than absent: this reader cannot skip a page, and a number
    // here naming a page that was also sent stops the whole job.
    assert!(skipped_pages.is_empty(), "{skipped_pages:?}");
    // `native:html`, satisfying `page.text_source`'s CHECK and naming the
    // reader rather than the file — the same rule `native:md` follows.
    assert_eq!(text_source, "native:html");
}

/// A book of the shape every book has: `mimetype` first and uncompressed, a
/// container, a package document, and `chapters` as `(member name, body)`
/// pairs — the spine naming every id in order, whether the archive holds the
/// member or not.
fn epub_bytes(chapters: &[(&str, Option<&str>)]) -> Vec<u8> {
    use std::io::Cursor;

    let manifest: String = chapters
        .iter()
        .enumerate()
        .map(|(n, (href, _))| {
            format!("<item id=\"c{n}\" href=\"{href}\" media-type=\"application/xhtml+xml\"/>")
        })
        .collect();
    let spine: String = (0..chapters.len())
        .map(|n| format!("<itemref idref=\"c{n}\"/>"))
        .collect();
    let opf = format!(
        "<package xmlns=\"http://www.idpf.org/2007/opf\">\
         <manifest>{manifest}</manifest><spine>{spine}</spine></package>"
    );

    let mut buf = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let stored: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        w.start_file("mimetype", stored).unwrap();
        w.write_all(b"application/epub+zip").unwrap();
        w.start_file("META-INF/container.xml", deflated).unwrap();
        w.write_all(
            b"<container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
              <rootfiles><rootfile full-path=\"content.opf\" \
              media-type=\"application/oebps-package+xml\"/></rootfiles></container>",
        )
        .unwrap();
        w.start_file("content.opf", deflated).unwrap();
        w.write_all(opf.as_bytes()).unwrap();
        for (href, body) in chapters {
            if let Some(body) = body {
                w.start_file(*href, deflated).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
        }
        w.finish().unwrap();
    }
    buf.into_inner()
}

/// The EPUB branch's whole wire shape, at the only place it is produced.
///
/// **The frame this test exists for is the summary, not the header.** A book is
/// the first format on this wire that both sends pages and names pages it did
/// not send, and the pool stops the entire job — `PoolError::Protocol`, which
/// accuses the worker binary of being from another release — when one number is
/// in both lists (`crates/mnema-pool/src/lib.rs:1324`). The natural way to write
/// "skip this chapter" is to send an empty page for it and count it as well,
/// and that shape passes every assertion about prose in this file.
///
/// The literal `"epub"` rather than `manifest::READER_EPUB` on purpose: a test
/// that asks the code under test what it says and then agrees is not a test.
/// The constant is the mechanism, this is the value, and `mnema-ingest` matches
/// the same constant from the other side of D40 to cite a chapter by its
/// section.
#[test]
fn an_epub_is_read_chapter_by_chapter_and_its_summary_names_what_it_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("книжка.epub");
    std::fs::write(
        &path,
        epub_bytes(&[
            (
                "ch1.xhtml",
                Some(
                    "<html><head><title>Розділ перший</title></head>\
                     <body><p>Виторг зріс.</p></body></html>",
                ),
            ),
            // In the spine, in the manifest, and not in the archive.
            ("ch2.xhtml", None),
            (
                "ch3.xhtml",
                Some(
                    "<html><head><title>Розділ третій</title></head>\
                     <body><p>А потім впав.</p></body></html>",
                ),
            ),
        ]),
    )
    .unwrap();

    let request = format!(
        "{{\"path\":{:?},\"max_bytes\":1048576}}",
        path.display().to_string()
    );
    let frames = frames_of(&run_worker(&[&request]));

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
    assert_eq!(reader, "epub");
    assert_eq!(*reader_version, 1);
    assert_eq!(mime, "application/epub+zip");
    // Two, not three: a chapter that was skipped produces no page frame, and
    // the pool checks this count against the frames that arrive.
    assert_eq!(*pages, 2);

    let sent: Vec<(u32, Option<String>)> = frames
        .iter()
        .filter_map(|f| match f {
            Frame::Page {
                page_no,
                section_title,
            } => Some((*page_no, section_title.clone())),
            _ => None,
        })
        .collect();
    // The numbers are the spine's, so the gap where chapter 2 was is kept
    // rather than closed — the same honest record a PDF's skipped page leaves.
    assert_eq!(
        sent,
        vec![
            (1, Some("Розділ перший".to_string())),
            (3, Some("Розділ третій".to_string())),
        ]
    );

    let prose: Vec<&str> = frames
        .iter()
        .filter_map(|f| match f {
            Frame::Block(block) => Some(block.text.as_str()),
            _ => None,
        })
        .collect();
    // Both directions across the wire: the chapters' prose is there, and the
    // tab labels their `<title>` elements carry are not.
    assert_eq!(prose, vec!["Виторг зріс.", "А потім впав."]);

    let Some(Frame::Summary {
        skipped_pages,
        text_source,
    }) = frames.last()
    else {
        panic!("expected a summary, got {:?}", frames.last());
    };
    assert_eq!(skipped_pages, &vec![2]);
    // And the pair the pool stops the whole job over, asserted here because
    // nothing downstream ever sees the two lists side by side again.
    assert!(
        !sent
            .iter()
            .any(|(page_no, _)| skipped_pages.contains(page_no)),
        "a chapter was both sent as a page and reported skipped: {sent:?} / {skipped_pages:?}"
    );
    // `native:epub` satisfies `page.text_source`'s CHECK
    // (`crates/mnema-index/src/schema.sql:101-102`) and names the reader rather
    // than the file.
    assert_eq!(text_source, "native:epub");
}

/// A book with nothing readable in it is refused under a rule about content —
/// and not under `unsupported`, which is what an EPUB got until this branch
/// existed and which promises a reader that is coming.
///
/// `no_text_layer` rather than `malformed`: the archive is intact and this
/// reader has nothing to say about what is in it, which is the same sentence
/// `pdf.rs` says about a scan.
#[test]
fn a_book_with_no_readable_chapter_is_refused_by_content_rather_than_as_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("самі-картинки.epub");
    let bytes = epub_bytes(&[(
        "cover.xhtml",
        Some(
            "<html><head><title>Обкладинка</title></head><body><img src=\"c.jpg\"/></body></html>",
        ),
    )]);
    std::fs::write(&path, &bytes).unwrap();

    let request = format!(
        "{{\"path\":{:?},\"max_bytes\":1048576}}",
        path.display().to_string()
    );
    let frames = frames_of(&run_worker(&[&request]));
    assert_eq!(frames.len(), 1);
    let Frame::Refused {
        rule,
        sha256,
        reason,
    } = &frames[0]
    else {
        panic!("expected Refused, got {:?}", frames[0]);
    };
    assert_eq!(rule, "no_text_layer");
    assert_ne!(rule, "unsupported");
    // The digest of the bytes this verdict was reached on: the file *was* read,
    // unlike the `too_large` branch that decides from `stat`, so the parent can
    // tell whether the file changed or only the rule did.
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let expected = hasher.finalize();
    assert_eq!(
        sha256.as_deref(),
        Some(
            expected
                .iter()
                .fold(String::new(), |mut s, b| {
                    let _ = write!(s, "{b:02x}");
                    s
                })
                .as_str()
        )
    );
    assert!(reason.contains("chapter"), "{reason}");
}

/// A book whose structure is broken is refused as damaged, which is a different
/// rule and a different sentence to the person holding it than "no text here".
#[test]
fn a_book_with_no_container_is_refused_as_malformed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("побита.epub");
    {
        use std::io::Cursor;
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let stored: zip::write::FileOptions<()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            w.start_file("mimetype", stored).unwrap();
            w.write_all(b"application/epub+zip").unwrap();
            w.finish().unwrap();
        }
        std::fs::write(&path, buf.into_inner()).unwrap();
    }

    let request = format!(
        "{{\"path\":{:?},\"max_bytes\":1048576}}",
        path.display().to_string()
    );
    let frames = frames_of(&run_worker(&[&request]));
    assert_eq!(frames.len(), 1);
    match &frames[0] {
        Frame::Refused { rule, .. } => assert_eq!(rule, "malformed"),
        other => panic!("expected Refused, got {other:?}"),
    }
}

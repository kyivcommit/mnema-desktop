//! The PDF reader, at the library boundary: which pages become text, which are
//! named as having none, and which failures are about the file.
//!
//! The wire half — what the worker binary writes for the same fixtures — is in
//! `tests/worker_cli.rs`, because that is where every other reader's frames are
//! asserted and because the two halves fail differently: this file can be wrong
//! about a page, that one about a rule string nothing in Rust checks.
//!
//! Fixtures come from `tests/fixtures/make_fixtures.py`; the character counts
//! below are that script's own output. `text-stamp-text.pdf` puts the
//! unreadable page in the **middle** on purpose — on a two-page fixture,
//! "dropped page 2", "dropped the last page" and "kept only the first page" are
//! one observation, and a reader doing any of the three would pass.

use std::path::{Path, PathBuf};

use mnema_core::BlockType;
use mnema_extract::{PdfError, TEXT_LAYER_MIN_CHARS, extract_pdf, probe_text_layer};

/// The text pdfium reads back from page 1 of every body fixture, exactly.
///
/// `\r\n` because that is what the fixture's content stream draws, and
/// `pdfium_binding.rs` already pins the same two sentences at the level below
/// this one. Asserted whole rather than with `contains`: a reader that returned
/// the first line only, or that joined the two lines with a space it invented,
/// satisfies a substring check and stores text the page does not contain.
const PAGE_ONE_TEXT: &str = "Invented contract 4417 between Northwind Depot and Ravella Freight,\r\n\
     signed 2026-07-25, covering pallet haulage for one calendar quarter.";

/// Page 3 of `text-stamp-text.pdf`. Shares no words with page 1, so a reader
/// that emitted page 1 twice cannot pass the test that reads this.
const PAGE_THREE_TEXT: &str = "Schedule B lists forty pallets of dried barley, collected weekly\r\n\
     from the Ravella yard, each delivery note countersigned on arrival.";

/// Non-whitespace characters on a stamp page, from `make_fixtures.py`'s output.
/// Above zero and far below the threshold, which is the entire point of the
/// fixture.
const STAMP_PAGE_CHARS: usize = 8;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn a_page_with_a_text_layer_becomes_a_block_of_that_page() {
    let doc =
        extract_pdf(include_bytes!("fixtures/one-page-text.pdf")).expect("the fixture parses");

    assert_eq!(doc.skipped, Vec::<u32>::new());
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(doc.pages[0].page_no, 1);

    let blocks = &doc.pages[0].blocks;
    assert_eq!(blocks.len(), 1, "one block per page: {blocks:?}");
    assert_eq!(blocks[0].text, PAGE_ONE_TEXT);
    assert_eq!(blocks[0].block_type, BlockType::Paragraph);
    assert_eq!(blocks[0].reading_order, 0);
    // **A PDF block carries no line numbers**, and that is load-bearing rather
    // than unfinished: `mnema_ingest::pages_of` gives this reader
    // `PageContext::Fixed(Coordinate::Page)` *because* a line range computed
    // from these blocks would be `Coordinate::None`. A number invented here
    // would be cited as rows of a page that has none.
    assert_eq!(blocks[0].line_start, None);
    assert_eq!(blocks[0].line_end, None);
}

/// The skipped page is named, its neighbours are not, and the survivors keep
/// their own numbering.
///
/// Three assertions and none is redundant. `skipped == [2]` alone is satisfied
/// by a reader that also dropped page 3; `page_no == [1, 3]` alone is satisfied
/// by a reader that skipped nothing and mislabelled; and the text assertion is
/// what separates "page 3 came through" from "page 1 came through twice".
#[test]
fn a_page_under_the_threshold_is_skipped_by_number_and_its_neighbours_are_not() {
    // The premise, stated rather than assumed: page 2 carries text. Without
    // this, everything below is also satisfied by a reader whose rule is
    // "drop empty pages", which is the rule `TEXT_LAYER_MIN_CHARS` exists to
    // replace — a scanned page in a real archive is never empty.
    let probes = probe_text_layer(&fixture("text-stamp-text.pdf")).expect("the fixture parses");
    assert_eq!(probes.len(), 3);
    assert_eq!(probes[1].char_count, STAMP_PAGE_CHARS);
    assert!(
        probes[1].char_count > 0 && probes[1].char_count < TEXT_LAYER_MIN_CHARS,
        "page 2 must carry text and still be under the threshold, or this test proves nothing"
    );

    let doc =
        extract_pdf(include_bytes!("fixtures/text-stamp-text.pdf")).expect("the fixture parses");

    assert_eq!(
        doc.skipped,
        vec![2],
        "the scanned page is named, not merely counted, and only that page"
    );
    assert_eq!(
        doc.pages.iter().map(|p| p.page_no).collect::<Vec<_>>(),
        vec![1, 3],
        "the gap is the honest record of the skip: the survivors are not renumbered"
    );
    assert_eq!(doc.pages[0].blocks[0].text, PAGE_ONE_TEXT);
    assert_eq!(
        doc.pages[1].blocks[0].text, PAGE_THREE_TEXT,
        "the page numbered 3 must carry page 3's own words"
    );
}

/// A scan of a paper document: every page exists, none has text.
///
/// Both halves, because either alone is satisfied by a mistake. A reader that
/// returned nothing at all would pass the first assertion; one that skipped
/// only what it could not read would pass the second.
#[test]
fn a_pdf_whose_every_page_is_scanned_yields_no_pages_and_names_them_all() {
    let doc = extract_pdf(include_bytes!("fixtures/all-scanned.pdf")).expect("the fixture parses");

    assert!(doc.pages.is_empty(), "{:?}", doc.pages);
    assert_eq!(
        doc.skipped,
        vec![1, 2, 3],
        "all three pages are named, in order — a count alone could not say which"
    );
}

/// Damage is about the file; a library that will not load is not.
///
/// **Both directions in one test, deliberately.** The expensive failure is not
/// "the malformed arm is missing" but "the two collapsed onto one", and each
/// half alone is satisfied by that collapse: an implementation reporting
/// everything as `Malformed` passes the first, one reporting everything as
/// `Library` passes the second.
///
/// The second half runs the **worker binary** rather than calling `extract_pdf`
/// here, and it has to: `pdfium()` caches its answer in a `OnceLock` for the
/// life of the process, so once any test in this binary has loaded the library
/// successfully no in-process call can ever see it fail again. A subprocess
/// with `MNEMA_PDFIUM_LIB_DIR` pointed at an empty directory is the same shape
/// `an_empty_library_directory_fails_at_the_verify_build_stage` uses one layer
/// down, and the same shape a quarantined `libpdfium.dylib` takes in the field.
#[test]
fn a_damaged_pdf_is_the_files_fault_and_an_unloadable_library_is_not() {
    let damaged = extract_pdf(b"%PDF-1.4\nthis is not a pdf")
        .expect_err("a truncated document is not a document");
    match &damaged {
        PdfError::Malformed(message) => assert!(
            // pdfium's own word for it, so the assertion cannot pass on an
            // error this crate invented without asking the library.
            message.contains("FormatError"),
            "the reader must say what pdfium said: {message}"
        ),
        other => panic!("a damaged file is not {other:?}"),
    }

    // And a good document still reads after a bad one — the library must not
    // have been poisoned by the failure above.
    assert_eq!(
        extract_pdf(include_bytes!("fixtures/one-page-text.pdf"))
            .expect("a good document still reads")
            .pages
            .len(),
        1
    );

    let dir = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mnema-extract-worker"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .env(mnema_extract::PDFIUM_LIB_DIR_ENV, dir.path())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            let request = serde_json::json!({
                "path": fixture("one-page-text.pdf").display().to_string(),
                "max_bytes": 1_048_576,
            });
            writeln!(child.stdin.as_mut().unwrap(), "{request}")?;
            child.wait_with_output()
        })
        .expect("the worker runs");
    let line = String::from_utf8(out.stdout).expect("stdout is UTF-8");
    let frames: Vec<mnema_extract::wire::Frame> = line
        .lines()
        .map(|l| serde_json::from_str(l).expect("one frame per line"))
        .collect();

    match frames.as_slice() {
        [mnema_extract::wire::Frame::Failed { message }] => {
            assert!(
                message.contains("pdfium"),
                "the message is the only place this reason survives: {message}"
            );
            assert!(
                message.contains("VERSION"),
                "and it must name what was actually missing: {message}"
            );
        }
        // Named rather than left to a generic panic: `Refused { rule:
        // "malformed" }` here is precisely the failure this test exists for,
        // and it should read as that rather than as "wrong frame".
        other => panic!(
            "a library that will not load must be Failed, not a verdict about the file. \
             Under `malformed` the walk would finish green with every PDF in the folder \
             journalled as damaged, and repairing the install would return nothing. Got: \
             {other:?}"
        ),
    }
}

/// A locked document is not a damaged one.
///
/// The fixture's body is byte-for-byte the body of `one-page-text.pdf`, which
/// the first test in this file reads without complaint — so the refusal cannot
/// be blamed on its content, and an implementation that reported every
/// unreadable PDF as `Malformed` fails here rather than passing on a file that
/// happened to be broken as well as locked.
#[test]
fn a_password_protected_pdf_is_locked_rather_than_damaged() {
    let locked = extract_pdf(include_bytes!("fixtures/password-locked.pdf"))
        .expect_err("a document behind a password is not one this reader can open");

    assert!(
        matches!(locked, PdfError::Encrypted),
        "a password is its own answer to the person holding the file: {locked:?}"
    );
}

/// The `PdfDocument` a caller gets back partitions the document: every page is
/// either read or named, never both and never neither.
///
/// A property rather than a case, and it is the one thing no single fixture
/// above can state — each of them knows its own page count by hand, so a
/// reader that quietly lost a page from *both* lists would pass all three.
#[test]
fn every_page_of_a_document_is_either_read_or_named() {
    for name in [
        "one-page-text.pdf",
        "text-stamp-text.pdf",
        "all-scanned.pdf",
    ] {
        let bytes = std::fs::read(fixture(name)).expect("the fixture is on disk");
        let doc = extract_pdf(&bytes).expect("the fixture parses");
        let total = probe_text_layer(&fixture(name))
            .expect("the fixture parses")
            .len();

        let mut seen: Vec<u32> = doc
            .pages
            .iter()
            .map(|p| p.page_no)
            .chain(doc.skipped.iter().copied())
            .collect();
        seen.sort_unstable();
        assert_eq!(
            seen,
            (1..=total as u32).collect::<Vec<_>>(),
            "{name}: the read pages and the skipped ones must be every page, once each"
        );
    }
}

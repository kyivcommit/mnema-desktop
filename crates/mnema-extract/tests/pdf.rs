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

/// Page 1 of `replacement-chars.pdf` as it must be stored: a contents line whose
/// leader dots pdfium returned as twenty U+FFFD, then a body line.
const CONTENTS_PAGE_TEXT: &str = "About the Authors xxi\r\n\
     Invented contract 4417 between Northwind Depot and Ravella Freight,";

/// Page 3 of `replacement-chars.pdf`, the neighbour after the skipped page.
const ANNEX_PAGE_TEXT: &str = "Annex C records the arbitration venue and the notice period agreed,\r\n\
     together with the schedule of penalties for a late collection.";

/// Page 2 of `offset-cropbox.pdf`, exactly as the page shows it.
///
/// `all()` reads `page_size()` — width×height from `(0,0)` — and this page's
/// CropBox starts at `x 89`, so the rectangle `all()` reads is 433 wide and stops
/// in the middle of both lines: measured, it returns `…to the Rave` and `…the
/// wagon leav`. An assertion on the whole line therefore fails there and only
/// there, which is what makes it worth writing out rather than checking a prefix.
const CROPPED_PAGE_TEXT: &str = "Invented ledger entry 4417 lists every pallet hauled to the Ravella yard,\r\n\
     and the depot clerk signs each page of it before the wagon leaves.";

/// The slug drawn on page 2 of `offset-cropbox.pdf`, between the CropBox top
/// (720) and the MediaBox top (792). It is in the file and on nobody's screen.
const CROPPED_PAGE_SLUG: &str = "INVENTED PRESS SLUG";

/// Page 2 of `broken-utf16.pdf`: one word of it drawn in a font whose
/// `/ToUnicode` sends a single code to U+D800, and a third line that runs past
/// the page's CropBox (to `x 543`) with a fourth line after it.
///
/// The single space is one assertion. pdfium answers a lone surrogate by
/// failing to decode the page's whole buffer, so `all()` returns `""` and
/// `inside_rect` of any rectangle returns `""`; the page is rebuilt from its
/// characters, the one that is not a `char` becomes U+FFFD, and
/// `replacement_runs_to_spaces` — D186's rule, unchanged — turns it into the
/// space between `word` and `from`. A reader that dropped the character instead
/// would store `wordfrom`, and one that kept the surrogate could not store the
/// page at all.
///
/// The `\r\n` after `that we` is the other assertion. The line is cut by the
/// box at `x 522`, and the line break pdfium writes at the end of it lies
/// outside, so the fallback has to write that break itself — one word shorter
/// and the last visible word of the cut line fuses with the first word of the
/// next (`…that weand the depot…`), which is two words lost from search.
///
/// Every line here is the string pdfium's own bounded read gives for the same
/// geometry with the broken glyph taken out (measured on a copy of this page
/// before the test was written), so the fallback is held to the fast path's
/// answer and not to its own.
const BROKEN_PAGE_TEXT: &str = "A damaged mapping in the middle of this line\r\n\
     drops the word from the index silently.\r\n\
     nvented ledger entry 4417 lists every pallet hauled to the Ravella yard gate that we\r\n\
     and the depot clerk signs each page of it before the wagon leaves.";

/// The slug drawn on page 3 of `page-boxes.pdf`, above its MediaBox and inside
/// its raw CropBox: the part of a page a viewer clips.
const OUTSIDE_MEDIA_SLUG: &str = "INVENTED COVER SPREAD";

/// A page whose every glyph came back as U+FFFD is a page without text, the
/// same as a scan, however many of them it holds.
///
/// Measured on a real library: a book whose prose font has a wrong
/// `/ToUnicode` gave pages of two thousand U+FFFD each, every one of them over
/// the threshold, and those pages won searches they had no words for.
#[test]
fn a_page_drawn_only_in_replacement_characters_is_skipped_like_a_scan() {
    let doc =
        extract_pdf(include_bytes!("fixtures/replacement-chars.pdf")).expect("the fixture parses");

    assert_eq!(doc.skipped, vec![2], "only the page of U+FFFD is skipped");
    assert_eq!(
        doc.pages.iter().map(|p| p.page_no).collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert_eq!(
        doc.pages[1].blocks[0].text, ANNEX_PAGE_TEXT,
        "the page numbered 3 must carry page 3's own words"
    );
}

/// A run of U+FFFD between words is one space: the words on either side stay
/// two words, and nothing the file does not show is stored.
#[test]
fn a_run_of_replacement_characters_becomes_one_space() {
    let doc =
        extract_pdf(include_bytes!("fixtures/replacement-chars.pdf")).expect("the fixture parses");

    assert_eq!(doc.pages[0].page_no, 1);
    assert_eq!(doc.pages[0].blocks[0].text, CONTENTS_PAGE_TEXT);
}

/// The diagnostic and the reader agree on which page has text, and the
/// diagnostic is handed the raw page — U+FFFD and all — so it is the counting
/// itself that has to leave them out.
#[test]
fn the_probe_does_not_count_replacement_characters_as_text() {
    let probes = probe_text_layer(&fixture("replacement-chars.pdf")).expect("the fixture parses");

    assert_eq!(
        probes.iter().map(|p| p.has_text_layer).collect::<Vec<_>>(),
        vec![true, false, true]
    );
    assert_eq!(probes[1].char_count, 0, "not one U+FFFD is text");
}

/// A page whose box starts away from the origin is a page of text, not a scan.
///
/// `all()` is `inside_rect(page_size())`, and `page_size()` is a rectangle from
/// `(0,0)` — width×height, with the box's own origin thrown away. Every fixture
/// written before this one has its MediaBox at `[0 0 612 792]`, where the two
/// rectangles are identical and the defect cannot be seen. Page 1 here carries
/// a box at `130.5 279 661.5 945` (D187) and its text above
/// `y 666`, so `all()` returns `""` — measured on the worker before the fix:
/// `skipped_pages: [1, 3]` — and the page is journalled as a scan.
///
/// Page 3 is the same box with **no `/CropBox` at all**, so `crop()` answers
/// `Err` there (measured); a reader that fell back to `page_size()` instead
/// would lose this page exactly as it lost page 1.
#[test]
fn a_page_whose_box_starts_away_from_the_origin_is_read_whole() {
    let doc =
        extract_pdf(include_bytes!("fixtures/offset-cropbox.pdf")).expect("the fixture parses");

    assert_eq!(
        doc.pages.iter().map(|p| p.page_no).collect::<Vec<_>>(),
        vec![1, 2, 3],
        "every page of this document carries text; none of them is a scan"
    );
    assert_eq!(doc.skipped, Vec::<u32>::new());
    assert_eq!(
        doc.pages[0].blocks[0].text, PAGE_ONE_TEXT,
        "page 1's text lies above the rectangle all() reads, and it is the page's own text"
    );
    assert_eq!(
        doc.pages[2].blocks[0].text, PAGE_THREE_TEXT,
        "a page with a MediaBox and no CropBox is read within its MediaBox: {:?}",
        doc.pages[2].blocks[0].text
    );
}

/// A line that runs past the CropBox is read to its end.
///
/// The page is 433 wide by its CropBox and its lines end at `x≈478`, inside the
/// visible page and outside `all()`'s rectangle. The assertion is the whole two
/// lines rather than their beginning: `…to the Rave` is what `all()` returns,
/// and a reader that keeps half a line has the same shape of defect as one that
/// keeps none.
#[test]
fn a_line_that_runs_past_the_crop_box_is_read_to_its_end() {
    let doc =
        extract_pdf(include_bytes!("fixtures/offset-cropbox.pdf")).expect("the fixture parses");

    assert_eq!(
        doc.pages
            .iter()
            .find(|p| p.page_no == 2)
            .map(|p| p.blocks[0].text.as_str()),
        Some(CROPPED_PAGE_TEXT),
        "the ends of both lines are part of what the person reads"
    );
}

/// A page whose boxes live on `/Pages` is read inside them.
///
/// `page-boxes.pdf` states both boxes on the page tree, the way a real book
/// often does (D187): `PdfPageBoundaries::crop`/`media` read only the page
/// dictionary, so measured on this fixture, both answer `Err`. A bound built
/// from them therefore falls back to `page_size()` — width×height from `(0,0)`,
/// the rectangle `all()` used — and this page, whose text sits above that
/// rectangle, is journalled as a scan that is not one. `bounding()` is the box
/// that resolves inheritance.
#[test]
fn a_page_whose_boxes_are_inherited_is_read_whole() {
    let doc = extract_pdf(include_bytes!("fixtures/page-boxes.pdf")).expect("the fixture parses");

    assert_eq!(
        doc.pages
            .iter()
            .find(|p| p.page_no == 1)
            .map(|p| p.blocks[0].text.as_str()),
        Some(PAGE_ONE_TEXT),
        "page 1 inherits its CropBox and its MediaBox; neither is on the page"
    );
}

/// A page whose own `/CropBox` is empty shows its MediaBox, and is read.
///
/// `/CropBox [0 0 0 0]` is what broken producers write. pdfium treats an empty
/// box as absent — measured: `bounding()` for this page answers its MediaBox —
/// so the page is read. A reader that took the raw `/CropBox` would find no
/// character inside it, skip the page, and journal a page of prose as a scan.
///
/// **"Read" is the whole of what this asserts, and the name says so.** All of
/// this page's text lies inside the MediaBox, so a reader with no bound at all
/// passes it too; the MediaBox clipping itself is
/// [`text_outside_the_media_box_is_not_indexed`], which is the neighbouring test
/// on page 3 of this fixture.
#[test]
fn a_page_with_a_degenerate_crop_box_is_read() {
    let doc = extract_pdf(include_bytes!("fixtures/page-boxes.pdf")).expect("the fixture parses");

    assert_eq!(
        doc.pages
            .iter()
            .find(|p| p.page_no == 2)
            .map(|p| p.blocks[0].text.as_str()),
        Some(PAGE_THREE_TEXT),
        "an empty /CropBox means the MediaBox, not an empty page"
    );
}

/// …and a `/CropBox` wider than the `/MediaBox` clips at the MediaBox.
///
/// Page 3 declares a CropBox that reaches past its MediaBox on every side, with
/// a cover slug in the part a viewer clips. The raw CropBox contains that slug,
/// so a reader that took it would index text nobody can see in the file; the
/// box a page is read within is MediaBox ∩ CropBox. The body is asserted whole
/// as well, so a reader that emptied the page cannot pass this.
#[test]
fn text_outside_the_media_box_is_not_indexed() {
    let doc = extract_pdf(include_bytes!("fixtures/page-boxes.pdf")).expect("the fixture parses");

    assert_eq!(
        doc.pages
            .iter()
            .find(|p| p.page_no == 3)
            .map(|p| p.blocks[0].text.as_str()),
        Some(ANNEX_PAGE_TEXT),
        "the visible text, and not the cover spread above it"
    );
    for page in &doc.pages {
        assert!(
            !page.blocks[0].text.contains(OUTSIDE_MEDIA_SLUG),
            "page {} is cited by what it shows: {:?}",
            page.page_no,
            page.blocks[0].text
        );
    }
}

/// …and text outside the CropBox is not indexed, even though the file has it.
///
/// This is the other half of "read what the person sees", and it is the reason
/// the bound is the box a viewer clips to rather than every character on the
/// page: page 2 carries a press slug above the CropBox top, where a book's
/// press slug sits (D187). A reader that took the page's characters
/// without asking where they are would store the slug, and a search would find
/// text nobody can see in the file.
///
/// The premise that the slug is really in the file is the **exact** line in the
/// test above: its last words are `leaves.`, and an appended slug would redden
/// that string. Nothing here reads a compressed stream to prove it a second time.
#[test]
fn text_outside_the_crop_box_is_not_indexed() {
    let doc =
        extract_pdf(include_bytes!("fixtures/offset-cropbox.pdf")).expect("the fixture parses");

    for page in &doc.pages {
        assert!(
            !page.blocks[0].text.contains(CROPPED_PAGE_SLUG),
            "page {} is cited by what it shows, and it does not show the slug: {:?}",
            page.page_no,
            page.blocks[0].text
        );
    }
}

/// The diagnostic reads the same text the reader stores, on the pages where the
/// two used to disagree.
///
/// `probe_text_layer` answers `--probe-pdfium`, and it walked `text.all()` too:
/// on this fixture it reported all three pages as scans while the reader — after
/// the fix — reads three pages of prose. A packaging probe and a reader that
/// disagree about which pages of a file have text are two answers to one
/// question, and the probe's is the one a person sees first.
#[test]
fn the_probe_reads_the_pages_the_reader_reads() {
    let probes = probe_text_layer(&fixture("offset-cropbox.pdf")).expect("the fixture parses");

    assert_eq!(
        probes.iter().map(|p| p.has_text_layer).collect::<Vec<_>>(),
        vec![true, true, true],
        "every page of this document carries a text layer: {probes:?}"
    );
}

/// A glyph that is not a Unicode character loses its word, not its page.
///
/// Page 2 of `broken-utf16.pdf` is 70 characters of ordinary text with one word
/// drawn in `F2`, whose `/ToUnicode` sends that word's code to U+D800. pdfium
/// decodes the bounded text of the whole page with that one bad unit in it and
/// fails, so `all()` answers `""` — measured on the worker before the fix:
/// `pages: 1, skipped_pages: [2, 3]`, and the page the person can read is
/// journalled as a scan. Page 3 is nothing but the broken code and stays
/// skipped, which is what keeps the fallback from turning garbage into text.
#[test]
fn a_glyph_that_is_not_a_character_loses_its_word_and_not_its_page() {
    let doc = extract_pdf(include_bytes!("fixtures/broken-utf16.pdf")).expect("the fixture parses");

    assert_eq!(
        doc.pages.iter().map(|p| p.page_no).collect::<Vec<_>>(),
        vec![1, 2],
        "page 1 has no broken glyph and page 2 is readable apart from one word"
    );
    assert_eq!(
        doc.skipped,
        vec![3],
        "a page of nothing but the broken code is a page without text"
    );
}

/// …and the word it loses leaves exactly one space, with its neighbours whole.
///
/// Asserted as the one string and not as `contains` of either half: a reader
/// that dropped the character would store `wordfrom`, a reader that inserted
/// two spaces would store `word  from`, and both satisfy a check for `word` on
/// its own.
#[test]
fn a_broken_glyph_between_two_words_leaves_one_space() {
    let doc = extract_pdf(include_bytes!("fixtures/broken-utf16.pdf")).expect("the fixture parses");

    assert_eq!(
        doc.pages
            .iter()
            .find(|p| p.page_no == 2)
            .map(|p| p.blocks[0].text.as_str()),
        Some(BROKEN_PAGE_TEXT),
        "the broken word is gone and the words around it are not"
    );
}

/// A line cut by the box keeps its line break when the page is rebuilt from
/// characters.
///
/// `broken-utf16.pdf` page 2's third line runs to `x 543`, past its CropBox's
/// right edge at 522, and the break pdfium puts at the end of it lies outside
/// the box — so the break is one of the things `inside_rect` *synthesises* from
/// the y position rather than reads. A fallback that only filtered characters
/// would drop it and store `…that weand the depot…`: the last visible word of
/// one line fused with the first word of the next, and both unfindable.
///
/// Asserted from both sides: the visible tail of the cut line is the tail, and
/// the next line starts at a break rather than in the middle of that word.
#[test]
fn a_line_cut_by_the_box_keeps_its_break_when_the_page_is_rebuilt() {
    let doc = extract_pdf(include_bytes!("fixtures/broken-utf16.pdf")).expect("the fixture parses");
    let text = &doc
        .pages
        .iter()
        .find(|p| p.page_no == 2)
        .expect("page 2 is read")
        .blocks[0]
        .text;

    assert!(
        text.contains("to the Ravella yard gate that we"),
        "the cut line is read up to the box's edge: {text:?}"
    );
    assert!(
        text.ends_with("\r\nand the depot clerk signs each page of it before the wagon leaves."),
        "the line after the cut begins a line, and is not glued to the cut word: {text:?}"
    );
    assert!(
        !text.contains("weand"),
        "the cut word and the next one are two words: {text:?}"
    );
}

/// The fixtures written before this change read the same text they always did.
///
/// The bound moved from `page_size()` to the page's box, and the fallback added
/// a second road to a page's text; both are changes to what every PDF in the
/// index answers with. For a page whose box is `[0 0 612 792]` the two
/// rectangles coincide, so the text must not move by a character — and
/// `tests/pdfium_binding.rs` pins the same sentences one layer below this one,
/// which is what makes a difference here a change in the reader rather than in
/// the expectation.
#[test]
fn the_fixtures_written_before_this_change_read_the_same_text() {
    for (name, expected) in [
        ("one-page-text.pdf", vec![PAGE_ONE_TEXT]),
        ("text-then-stamp.pdf", vec![PAGE_ONE_TEXT]),
        ("text-stamp-text.pdf", vec![PAGE_ONE_TEXT, PAGE_THREE_TEXT]),
        (
            "replacement-chars.pdf",
            vec![CONTENTS_PAGE_TEXT, ANNEX_PAGE_TEXT],
        ),
    ] {
        let bytes = std::fs::read(fixture(name)).expect("the fixture is on disk");
        let doc = extract_pdf(&bytes).expect("the fixture parses");

        assert_eq!(
            doc.pages
                .iter()
                .map(|p| p.blocks[0].text.as_str())
                .collect::<Vec<_>>(),
            expected,
            "{name}: the text a page stores must not move with the reader"
        );
    }
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

/// A page pdfium refuses to load is not a page without a text layer — and,
/// above all, is not the end of the document.
///
/// **This is the shape `pdfium-render`'s page iterator gives that failure away
/// in.** `PdfPagesIterator::next` is `self.pages.get(i).ok()`
/// (`pdfium-render-0.9.3/src/pdf/document/pages.rs:608-613`), so the first page
/// `FPDF_LoadPage` declines ends the iteration. Read with that iterator, the
/// three-page fixture below came back as a **one-page document**: header
/// `pages: 1`, `skipped_pages: 0`, no gap in `page_no`, the pool's integrity
/// check satisfied because the header agreed with the frames that arrived, and
/// the walk green. Nothing anywhere recorded that two pages of a contract had
/// gone. Measured on this fixture before the fix, not reasoned about.
///
/// Refusing the document is the same policy a failing `page.text()` already
/// had, and it is chosen over `skipped` for the reason `pdf.rs` states there:
/// `no_text_layer` is a verdict about *content* that the journal keeps until
/// `INDEX_FORMAT_VERSION` moves, so a reader that put this page there would
/// remember its own failure as a fact about the scan.
#[test]
fn a_page_that_will_not_load_refuses_the_document_rather_than_ending_it() {
    let err = extract_pdf(include_bytes!("fixtures/unloadable-middle-page.pdf"))
        .expect_err("a document with a page that will not load is not one this reader finished");

    match &err {
        PdfError::Malformed(message) => assert!(
            message.contains("page 2"),
            "the message must name the page that failed, since nothing downstream can: {message}"
        ),
        // Both neighbours named, because either is a plausible wrong answer and
        // each is silent in its own way.
        other => panic!(
            "a page that will not load is neither a scan nor a locked file: {other:?}. \
             Reported as `no_text_layer` it would be journalled as content and outlive \
             the fix; dropped from the page list it would vanish with no record at all."
        ),
    }
}

/// A PDF with no pages is not a scan.
///
/// The third meaning that used to share one arm with the other two. Before the
/// fix, `pages.is_empty()` was reached by a scan, by a document with no pages,
/// and by a document whose pages would not load — and the worker answered all
/// three "no page of this PDF carries a text layer of at least 48 characters",
/// a sentence about pages the file does not have, remembered as a verdict about
/// content. Now only the scan reaches it.
#[test]
fn a_pdf_with_no_pages_at_all_is_not_reported_as_having_no_text() {
    let err = extract_pdf(include_bytes!("fixtures/no-pages.pdf"))
        .expect_err("a document with no pages has nothing this reader can return");

    match &err {
        PdfError::Malformed(message) => assert!(
            message.contains("no pages"),
            "the sentence is the only place this differs from a scan: {message}"
        ),
        other => panic!("a document with no pages is not {other:?}"),
    }
}

/// The `PdfDocument` a caller gets back partitions the document: every page is
/// either read or named, never both and never neither.
///
/// A property rather than a case, and it is the one thing no single fixture
/// above can state — each of them knows its own page count by hand, so a
/// reader that quietly lost a page from *both* lists would pass all three.
///
/// **The page counts are literals from `make_fixtures.py`, and that is the
/// point.** This test took `total` from `probe_text_layer(…).len()`, which
/// reaches the same truncating iterator the reader did: both sides of the
/// assertion moved together, so a reader that lost a page lost it from the
/// yardstick as well and the invariant named in this docstring could not go
/// red. It stayed green through the defect it exists to catch. A number the
/// generator printed cannot move with the reader.
#[test]
fn every_page_of_a_document_is_either_read_or_named() {
    for (name, pages_in_the_file) in [
        ("one-page-text.pdf", 1u32),
        ("text-stamp-text.pdf", 3),
        ("all-scanned.pdf", 3),
        ("replacement-chars.pdf", 3),
        ("offset-cropbox.pdf", 3),
        ("broken-utf16.pdf", 3),
        ("page-boxes.pdf", 3),
    ] {
        let bytes = std::fs::read(fixture(name)).expect("the fixture is on disk");
        let doc = extract_pdf(&bytes).expect("the fixture parses");

        let mut seen: Vec<u32> = doc
            .pages
            .iter()
            .map(|p| p.page_no)
            .chain(doc.skipped.iter().copied())
            .collect();
        seen.sort_unstable();
        assert_eq!(
            seen,
            (1..=pages_in_the_file).collect::<Vec<_>>(),
            "{name}: the read pages and the skipped ones must be every page, once each"
        );
    }
}

/// …and the same yardstick applied to the diagnostic, which shares the defect
/// and the fix.
///
/// `probe_text_layer` is what `--probe-pdfium` answers a packaging question
/// with, and it walked the same iterator: on the fixture above it reported one
/// page for a three-page file. A probe that under-reports pages is a probe that
/// says a bundle is fine when it is not, so the two are held to one answer here
/// rather than left to agree by habit.
#[test]
fn the_probe_and_the_reader_see_the_same_pages() {
    for (name, pages_in_the_file) in [
        ("one-page-text.pdf", 1usize),
        ("text-stamp-text.pdf", 3),
        ("all-scanned.pdf", 3),
        ("replacement-chars.pdf", 3),
        ("offset-cropbox.pdf", 3),
        ("broken-utf16.pdf", 3),
        ("page-boxes.pdf", 3),
    ] {
        let probes = probe_text_layer(&fixture(name)).expect("the fixture parses");
        assert_eq!(probes.len(), pages_in_the_file, "{name}");
        assert_eq!(
            probes.iter().map(|p| p.page_no).collect::<Vec<_>>(),
            (1..=pages_in_the_file as u32).collect::<Vec<_>>(),
            "{name}: the probe numbers pages from one, in document order"
        );
    }

    // And it stops rather than truncating on the page it cannot load, which is
    // the direction that matters: a shorter list is the answer nobody can tell
    // from a shorter document.
    probe_text_layer(&fixture("unloadable-middle-page.pdf"))
        .expect_err("a page that will not load is not a page the probe may leave out");
}

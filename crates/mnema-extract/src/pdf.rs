//! The PDF reader: a page's text layer becomes a block, and a page that has no
//! text layer is named rather than dropped.
//!
//! **The one decision this module exists to make is which pages count.** A
//! scanned page in a real archive is not blank — it carries a Bates number, a
//! scanner footer, a stamp — so "has any text at all" would index such a page
//! as though its content were the word `Page`. `TEXT_LAYER_MIN_CHARS`
//! (`pdfium_probe.rs`) is the product's answer, and a page below it goes into
//! [`PdfDocument::skipped`] by number. It is dropped from the text and kept in
//! the record: `Summary::skipped_pages` carries the number out of this process,
//! the parent journals a row for it, and the gap it leaves in `page_no` is what
//! `Frame::Page`'s own doc comment calls the honest record of a reader that
//! skipped something.
//!
//! **Three failures, and the difference between them is worth more than the
//! code that makes it.**
//!
//! - [`PdfError::Malformed`] — pdfium had the bytes and could not read them.
//!   A statement about the file.
//! - [`PdfError::Encrypted`] — pdfium had the bytes, they are a document, and
//!   it is locked. Also a statement about the file, and a different one to the
//!   person holding it.
//! - [`PdfError::Library`] — **not a statement about the file at all.** The
//!   shared library is missing, is the wrong build, or was refused by code
//!   signing. Reported as a content rule it would journal every PDF in a folder
//!   as damaged, with the walk finishing green, and the verdicts would outlive
//!   the repair because `SkipRule::Malformed::is_about_content()` is true and
//!   the journal's cheap arm answers from it until `INDEX_FORMAT_VERSION`
//!   moves. `crates/mnema-pool/src/lib.rs:300-303` names that outcome in prose
//!   — "ten thousand files as damaged when the real fault is a half-finished
//!   install" — and a quarantined `libpdfium.dylib` has already happened on
//!   this machine. `src/bin/worker.rs` sends it as `Frame::Failed`.
//!
//! The split is structural rather than careful: this variant can only be
//! produced by [`pdfium_probe::pdfium`], which names no file and opens none.
//!
//! A fourth, [`PdfError::Ocr`], is the same kind as `Library`: the engine that
//! reads a page without a text layer did not run (D189). See its doc comment.
//!
//! **A page without a text layer is rendered and recognised** (D189,
//! [`crate::ocr`]) before it is skipped: BGRA at 300 dpi, the long side capped
//! at [`OCR_MAX_SIDE_PX`]. What Vision reads is held to the same
//! `TEXT_LAYER_MIN_CHARS` as a native page — one rule for "is this a page of
//! text" — so a blank scan still lands in `skipped`, and so does every
//! textless page on a platform with no engine (`recognise` answers `Ok(None)`).
//!
//! **One block per page, and no attempt at paragraphs.** pdfium hands back a
//! page's text layer as one string in its own extraction order; splitting it
//! would mean inventing a paragraph rule out of line breaks that a PDF's text
//! layer does not reliably carry. What that costs is nothing downstream: a
//! chunk's coordinate for a PDF is the page (`mnema_ingest::pages_of`), which
//! is identical for every chunk on it however the blocks are cut, and
//! `mnema_chunk` splits within a block anyway. What it would cost to get wrong
//! is a block whose text is not a slice of the page.

use std::borrow::Cow;

use mnema_core::{Block, BlockType, nfc};
use pdfium_render::prelude::{
    PdfBitmapFormat, PdfPage as PdfiumPage, PdfRenderConfig, PdfiumError, PdfiumInternalError,
};

use crate::ocr::{self, OcrError, PageImage};
use crate::pdfium_probe::{self, has_text_layer, lock_pdfium, page_text, text_layer_chars};

/// `PdfPage::text_source` of a page read from its own text layer — and the
/// document-level value `Summary::text_source` carries for a PDF.
pub const TEXT_SOURCE_NATIVE: &str = "native:pdf";
/// `PdfPage::text_source` of a page Vision read from its rendering.
pub const TEXT_SOURCE_OCR: &str = "ocr:vision";

/// Set to a number, this **lowers** the most pages a document may declare
/// below [`mnema_core::wire::MAX_PAGES`] — never raises it — so a test can
/// show the refusal on a three-page fixture instead of a 100,001-page one.
/// Costs one environment lookup per document.
pub const MAX_PAGES_ENV: &str = "MNEMA_MAX_PAGES";

/// The most pages one document may declare here: D1's ceiling, or lower under
/// [`MAX_PAGES_ENV`].
pub fn page_cap() -> u32 {
    let cap = mnema_core::wire::MAX_PAGES;
    std::env::var(MAX_PAGES_ENV)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .map_or(cap, |lower| lower.min(cap))
}

/// The resolution a textless page is rendered at for recognition.
const OCR_DPI: f32 = 300.0;
/// The longest side of that rendering, in pixels. A page large enough to
/// exceed it at 300 dpi is rendered smaller instead, aspect kept, and the dpi
/// it was actually rendered at is what [`OcrPage::dpi`] records.
pub const OCR_MAX_SIDE_PX: f32 = 4000.0;

/// One page of a PDF that had a text layer worth keeping.
///
/// No `section_title`: a PDF page is not a section, and `mnema_ingest::pages_of`
/// gives a pdf page `PageContext::Fixed(Coordinate::Page)` — the page number is
/// the whole of what a citation into this format points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfPage {
    /// The page's own 1-based number in the document, **not** its position
    /// among the pages that came back. Pages 1 and 3 arriving with page 2
    /// skipped is the intended shape, and renumbering them 1 and 2 would cite
    /// the reader's bookkeeping instead of the document.
    pub page_no: u32,
    pub blocks: Vec<Block>,
    /// [`TEXT_SOURCE_NATIVE`] or [`TEXT_SOURCE_OCR`]; `ocr` is `Some` exactly
    /// for the second.
    pub text_source: &'static str,
    pub ocr: Option<OcrPage>,
}

/// What recognition of a page knew beyond its text: the bitmap it read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OcrPage {
    pub confidence: f32,
    pub width_px: u32,
    pub height_px: u32,
    pub dpi: u32,
}

// `f32` in `OcrPage` rules out a derived `Eq`; no confidence here is NaN
// (Vision's are in 0..=1, and an empty page is 0), so equality is total.
impl Eq for OcrPage {}

/// What a PDF read produced: the pages that had text, and the numbers of the
/// pages that did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfDocument {
    pub pages: Vec<PdfPage>,
    /// Ascending, 1-based, and disjoint from `pages` — every page the document
    /// declares appears in exactly one of the two.
    ///
    /// That is a promise the loop keeps by construction, not by care: it runs
    /// over `0..FPDF_GetPageCount` and any page it cannot handle takes the
    /// whole document out through `Err`. It was **not** true of the first
    /// version of this reader, which walked `pages().iter()` and stopped at the
    /// first page pdfium declined — pages that appeared in neither list and in
    /// no journal row. `every_page_of_a_document_is_either_read_or_named`
    /// checks it against page counts the fixture generator printed, rather than
    /// against anything that reads the file.
    ///
    /// Numbers rather than a count, and `Summary::skipped_pages` now carries
    /// them out whole: a count cannot answer "which page of this contract did
    /// the scanner miss", and this reader is the only thing that ever knows.
    pub skipped: Vec<u32>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum PdfError {
    /// The reader could not be brought up: no library, the wrong build, or a
    /// load the dynamic loader refused. **Not a fact about the file** — see the
    /// module doc for what reporting it as one costs, and
    /// `mnema_core::wire::Frame::Failed` for where it goes instead.
    ///
    /// It carries `pdfium_probe::Error` whole rather than a string, so the
    /// `stage` that failed (`library_dir` / `verify_build` / `bind`) survives
    /// for anyone who wants to act on it rather than print it.
    #[error("{0}")]
    Library(pdfium_probe::Error),

    /// The document is encrypted and this reader has no key for it.
    ///
    /// pdfium reports two internal errors here and both land in this variant:
    /// a failed password check, and a security handler it does not implement.
    /// The second is not damage — the file is intact and the reader is short a
    /// scheme — and `Malformed` would tell the person holding it to go looking
    /// for a corrupt file that does not exist.
    #[error("this PDF is password-protected")]
    Encrypted,

    /// pdfium had the bytes and could not make a document of them.
    #[error("pdfium could not read this document: {0}")]
    Malformed(String),

    /// A page without a text layer could not be recognised: the engine is
    /// missing (no Ukrainian model, the test seam) or failed on the image.
    /// **Not a fact about the file**, the same as [`PdfError::Library`], and
    /// for the same reason it must never become a content verdict (D57).
    /// `no_text_layer` or a skipped page would be journalled and remembered
    /// until the reader's version moves — no repair of the engine reaches that
    /// row — so a library of scans would be recorded as textless for good over
    /// an engine that did not run. The worker sends `Frame::Failed`: nothing is
    /// journalled as content, nothing the index holds is displaced (D44), and
    /// the next walk tries again.
    ///
    /// For the whole document, not the page: a skipped page is exactly the
    /// remembered verdict above, at page scale.
    #[error("{0}")]
    Ocr(OcrError),
}

/// Reads a PDF's text layer into pages of blocks.
///
/// Takes bytes rather than a path, and that is what keeps the worker's one
/// promise: `handle_request` reads the file once and hashes the same `Vec<u8>`
/// it hands here, so there is no window in which the file could change between
/// the digest and the reading.
///
/// **Serialised through [`lock_pdfium`] for the whole life of the document**,
/// not per call. Pdfium is not thread-safe and `pdfium-render`'s `thread_safe`
/// feature does not make it so; a document handle is the thing that must not
/// be interleaved. This crate's test binary died with SIGSEGV before that lock
/// existed.
///
/// An empty `pages` is a real answer, not an error, and it now means exactly
/// one thing: a scan of a paper contract, whose pages exist and hold no text —
/// in their text layer, and (since D189, where there is an engine) in what
/// Vision read from them.
/// The caller refuses such a file under `no_text_layer` (`src/bin/worker.rs`),
/// which is a rule about content and is remembered as one — so the two other
/// ways to produce no pages must not arrive here. A document with no pages at
/// all, and a document with a page that would not load, are both `Malformed`
/// below.
pub fn extract_pdf(bytes: &[u8]) -> Result<PdfDocument, PdfError> {
    let mut pages = Vec::new();
    let skipped = extract_pdf_pages(bytes, |_, page| pages.push(page))?;
    Ok(PdfDocument { pages, skipped })
}

/// [`extract_pdf`] as a stream: each page that has text is handed to `on_page`
/// as soon as it is read, together with the number of pages the document
/// declares, and the skipped page numbers come back at the end.
///
/// The declared count rides with every page rather than through a callback of
/// its own because the one caller that streams (the worker) writes its
/// `Header` lazily, before the first page: a document none of whose pages has
/// text must answer `no_text_layer` *instead of* a header, and it is only known
/// to be one at the end. Every page is still either handed over or returned by
/// number — the loop is the same loop — and an `Err` after some pages were
/// handed over means the pages handed over are not a document: the caller must
/// discard them (the pool does, `run_one`).
///
/// OCR runs on the calling thread, under the same lock, between pages; on
/// a 356-page scanned volume that is minutes, which is why the caller writes
/// each page as it arrives and the pool's deadline is per page (D189).
pub fn extract_pdf_pages(
    bytes: &[u8],
    mut on_page: impl FnMut(u32, PdfPage),
) -> Result<Vec<u32>, PdfError> {
    // First, and on its own line, because everything after it is about the
    // document and this is the only thing here that is not. `pdfium()` has not
    // been shown a file at this point and cannot fail for one.
    let pdfium = pdfium_probe::pdfium().map_err(PdfError::Library)?;
    let _serialised = lock_pdfium();

    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(load_failure)?;

    // **By index over `FPDF_GetPageCount`, never `pages().iter()`.**
    // `PdfPagesIterator::next` is `self.pages.get(i).ok()`
    // (`pdfium-render-0.9.3/src/pdf/document/pages.rs:608-613`), so the first
    // page `FPDF_LoadPage` declines silently becomes the end of the document.
    // Measured on `tests/fixtures/unloadable-middle-page.pdf`: a three-page
    // contract came back as a one-page one, with `skipped_pages: 0`, no gap in
    // `page_no`, the pool's header/page-frame check satisfied because both
    // numbers came from the truncated read, and the walk green. Nothing
    // recorded that two pages had gone, and content-addressing meant no later
    // pass would look again.
    let all_pages = document.pages();
    let count = all_pages.len();

    // A document with no pages at all, kept apart from a document whose pages
    // hold no text. Both used to leave `pages` empty, and the worker answers an
    // empty `pages` with "no page of this PDF carries a text layer of at least
    // N characters" — a sentence about pages this file does not have, recorded
    // as a verdict about content and remembered until `INDEX_FORMAT_VERSION`
    // moves. After this, that arm means one thing: every page was read and
    // every one was below the threshold.
    if count <= 0 {
        return Err(PdfError::Malformed(
            "this PDF declares no pages, so there is nothing in it to read".to_string(),
        ));
    }

    // Before any page is handed over, so before the worker writes a header:
    // the count is the file's own claim, and the pool bounds a document's time
    // by it. A statement about these bytes, like "declares no pages" above.
    let cap = page_cap();
    if count as i64 > i64::from(cap) {
        return Err(PdfError::Malformed(format!(
            "this PDF declares {count} pages, over the {cap} one document may have (D1)"
        )));
    }

    let declared = count as u32;
    let mut skipped = Vec::new();

    for index in 0..count {
        let page_no = (index + 1) as u32;
        // A page that will not load, and a page whose text cannot be read, are
        // the same answer here and it is deliberately **not** `skipped`.
        // `no_text_layer` is a verdict about content that the journal keeps
        // until `INDEX_FORMAT_VERSION` moves, so a reader recording either one
        // there would remember its own failure as a fact about the scan — and
        // by D57 no reader upgrade reaches that journal row. The whole document
        // is refused instead. What that costs is real and is the smaller cost:
        // one bad page refuses a document whose other pages were fine, and
        // `SkipRule::Malformed` does not displace what the index already holds
        // unless the bytes moved.
        //
        // The page number is in the message because it is the last place it can
        // be: `Frame::Refused` carries a rule and a sentence, and nothing after
        // the worker ever learns which page it was.
        let page = all_pages.get(index).map_err(|e| {
            PdfError::Malformed(format!(
                "page {page_no} of this PDF could not be loaded: {e}"
            ))
        })?;
        // Not `all()`: it reads `page_size()`, a rectangle from `(0,0)` that
        // throws the page's own origin away. `page_text` reads the box the page
        // shows and rebuilds a page pdfium failed to decode.
        let raw = page_text(&page).map_err(|e| {
            PdfError::Malformed(format!(
                "page {page_no} of this PDF has no readable text: {e}"
            ))
        })?;
        // Before the threshold and before the text is kept, so the page is
        // admitted and stored from the same string. A space and not nothing:
        // a contents line whose leader dots came back as U+FFFD would otherwise
        // store `About the Authorsxxi`, one word the page does not show.
        let raw = replacement_runs_to_spaces(&raw);

        let (raw, text_source, ocr) = if has_text_layer(text_layer_chars(&raw)) {
            (raw, TEXT_SOURCE_NATIVE, None)
        } else {
            match recognise(&page, page_no)? {
                // The same threshold as a native page, and no second one.
                // `replacement_runs_to_spaces` is not applied: Vision returns
                // characters it recognised, and U+FFFD is not among the shapes
                // it reads (none in any page measured for D189).
                Some(read) if has_text_layer(text_layer_chars(&read.text)) => {
                    let ocr = OcrPage {
                        confidence: read.confidence,
                        width_px: read.width_px,
                        height_px: read.height_px,
                        dpi: read.dpi,
                    };
                    (Cow::Owned(read.text), TEXT_SOURCE_OCR, Some(ocr))
                }
                // Vision found nothing worth a page, or there is no engine on
                // this platform: a scan without text, as before OCR existed.
                _ => {
                    skipped.push(page_no);
                    continue;
                }
            }
        };

        // Once, before the text is stored and after the threshold has been
        // taken from the same normalisation (D32, D38): `text_layer_chars`
        // normalises to count, so the string admitted and the string kept are
        // the same string.
        let text = nfc::normalise(&raw).into_owned();
        on_page(
            declared,
            PdfPage {
                page_no,
                text_source,
                ocr,
                blocks: vec![Block {
                    block_type: BlockType::Paragraph,
                    // Restarts at 0 on every page: uniqueness in the schema is on
                    // `(page_id, reading_order)`.
                    reading_order: 0,
                    // Nothing here detects language, as in every other reader:
                    // a per-block guess is the extraction spec's subject.
                    language: None,
                    text,
                    // **A PDF has no line to name.** Not a gap left to fill later:
                    // `mnema_ingest::pages_of` gives this reader
                    // `PageContext::Fixed(Coordinate::Page)` precisely because a
                    // line range computed from these blocks would be
                    // `Coordinate::None`, and a number invented here would be
                    // cited as "рядки 1–1" of a page that has no rows.
                    line_start: None,
                    line_end: None,
                }],
            },
        );
    }

    Ok(skipped)
}

/// Renders `page` to BGRA — the only format Vision reads (`ocr` module doc) —
/// at [`OCR_DPI`], or smaller if its long side would pass
/// [`OCR_MAX_SIDE_PX`], and recognises it.
///
/// A render pdfium refuses is the page's fault and refuses the document as
/// `Malformed`, like a page that will not load; an engine failure is
/// [`PdfError::Ocr`].
fn recognise(page: &PdfiumPage, page_no: u32) -> Result<Option<ocr::Recognised>, PdfError> {
    let scale = render_scale(page.width().value, page.height().value);
    let config = PdfRenderConfig::new()
        .scale_page_by_factor(scale)
        .set_format(PdfBitmapFormat::BGRA);
    let bitmap = page.render_with_config(&config).map_err(|e| {
        PdfError::Malformed(format!(
            "page {page_no} of this PDF could not be rendered: {e}"
        ))
    })?;
    let bgra = bitmap.as_raw_bytes();
    let (width, height) = (bitmap.width() as u32, bitmap.height() as u32);
    let image = PageImage {
        bgra: &bgra,
        width,
        height,
        // pdfium's buffer is `stride * height`; the stride is what is left.
        stride: bgra.len() / (height.max(1) as usize),
        dpi: (scale * 72.0).round() as u32,
    };
    ocr::recognise(&image).map_err(PdfError::Ocr)
}

/// Pixels per point for a page of `width` × `height` points: [`OCR_DPI`], or
/// whatever brings its long side down to [`OCR_MAX_SIDE_PX`].
fn render_scale(width: f32, height: f32) -> f32 {
    (OCR_DPI / 72.0).min(OCR_MAX_SIDE_PX / width.max(height).max(1.0))
}

/// Every run of U+FFFD, however long, as one space — and as none where a
/// space would stand beside whitespace or at either end of the page.
///
/// pdfium returns U+FFFD for a glyph its font maps to nothing usable — a
/// book's leader dots, or every letter of a prose font with a wrong
/// `/ToUnicode`. Kept, a run of them is a chunk whose vector sits near every
/// question the library has no answer to, and an excerpt of nothing. The space
/// exists only to keep the words on either side apart, so it is written only
/// where there are two words to keep apart: the chunker modifies no character
/// (`mnema_chunk`), and a doubled or leading space would reach the stored text.
fn replacement_runs_to_spaces(text: &str) -> Cow<'_, str> {
    if !text.contains('\u{FFFD}') {
        return Cow::Borrowed(text);
    }
    let chars = text.chars().map(|c| (c, ())).collect();
    Cow::Owned(
        replacement_runs(chars)
            .into_iter()
            .map(|(c, _)| c)
            .collect(),
    )
}

/// [`replacement_runs_to_spaces`] over characters that carry a tag (the pdfium
/// char index, for the page view): the one implementation, so the reader and
/// the highlight cannot disagree about where the spaces go. The space it
/// inserts has no source character and gets `T::default()`.
pub(crate) fn replacement_runs<T: Default>(chars: Vec<(char, T)>) -> Vec<(char, T)> {
    let mut out: Vec<(char, T)> = Vec::with_capacity(chars.len());
    let mut after_run = false;
    for (c, tag) in chars {
        if c == '\u{FFFD}' {
            after_run = true;
            continue;
        }
        if after_run && !c.is_whitespace() && out.last().is_some_and(|(p, _)| !p.is_whitespace()) {
            out.push((' ', T::default()));
        }
        after_run = false;
        out.push((c, tag));
    }
    out
}

/// Which of the three a failed `load_pdf_from_byte_slice` is.
///
/// Everything that is not a lock is damage. That direction is deliberate: the
/// arm that must never absorb a neighbour is [`PdfError::Library`], and it
/// cannot be reached from here at all — the library was already up before this
/// function had bytes to be handed.
fn load_failure(e: PdfiumError) -> PdfError {
    match e {
        PdfiumError::PdfiumLibraryInternalError(
            PdfiumInternalError::PasswordError | PdfiumInternalError::SecurityError,
        ) => PdfError::Encrypted,
        other => PdfError::Malformed(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{render_scale, replacement_runs_to_spaces};

    #[test]
    fn a_page_is_rendered_at_300_dpi_unless_its_long_side_would_pass_the_cap() {
        // A4, and the measured volume page (631×866 pt → 2630×3609 px).
        assert_eq!(render_scale(595.0, 842.0), 300.0 / 72.0);
        assert_eq!((866.0 * render_scale(631.0, 866.0)).round(), 3608.0);
        // A3 portrait or landscape: 1191 pt × 300/72 = 4963 px, over the cap.
        for (w, h) in [(842.0, 1191.0), (1191.0, 842.0)] {
            let scale = render_scale(w, h);
            assert_eq!((1191.0 * scale).round(), 4000.0, "{w}x{h}");
            assert_eq!(
                (scale * 72.0).round(),
                242.0,
                "the dpi recorded is the one used"
            );
        }
    }

    #[test]
    fn a_run_between_two_words_is_one_space_and_nowhere_else_is_there_any() {
        for (raw, kept) in [
            (
                "About the Authors\u{FFFD}\u{FFFD}\u{FFFD}xxi",
                "About the Authors xxi",
            ),
            ("a \u{FFFD}\u{FFFD}b", "a b"),
            ("a\u{FFFD}\u{FFFD} b", "a b"),
            ("a\u{FFFD}\r\nb", "a\r\nb"),
            ("\u{FFFD}\u{FFFD}ab", "ab"),
            ("ab\u{FFFD}\u{FFFD}", "ab"),
            ("\u{FFFD}\u{FFFD}\r\n\u{FFFD}", "\r\n"),
        ] {
            assert_eq!(replacement_runs_to_spaces(raw), kept, "{raw:?}");
        }
    }
}

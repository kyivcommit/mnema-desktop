//! The PDF reader: a page's text layer becomes a block, and a page that has no
//! text layer is named rather than dropped.
//!
//! **The one decision this module exists to make is which pages count.** A
//! scanned page in a real archive is not blank — it carries a Bates number, a
//! scanner footer, a stamp — so "has any text at all" would index such a page
//! as though its content were the word `Page`. `TEXT_LAYER_MIN_CHARS`
//! (`pdfium_probe.rs`) is the product's answer, and a page below it goes into
//! [`PdfDocument::skipped`] by number. It is dropped from the text and kept in
//! the record: `Summary::skipped_pages` counts it, and the gap it leaves in
//! `page_no` is what `Frame::Page`'s own doc comment calls the honest record of
//! a reader that skipped something.
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
//! **One block per page, and no attempt at paragraphs.** pdfium hands back a
//! page's text layer as one string in its own extraction order; splitting it
//! would mean inventing a paragraph rule out of line breaks that a PDF's text
//! layer does not reliably carry. What that costs is nothing downstream: a
//! chunk's coordinate for a PDF is the page (`mnema_ingest::pages_of`), which
//! is identical for every chunk on it however the blocks are cut, and
//! `mnema_chunk` splits within a block anyway. What it would cost to get wrong
//! is a block whose text is not a slice of the page.

use mnema_core::{Block, BlockType, nfc};
use pdfium_render::prelude::{PdfiumError, PdfiumInternalError};

use crate::pdfium_probe::{self, has_text_layer, lock_pdfium, text_layer_chars};

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
}

/// What a PDF read produced: the pages that had text, and the numbers of the
/// pages that did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfDocument {
    pub pages: Vec<PdfPage>,
    /// Ascending, 1-based, and disjoint from `pages` — every page of the
    /// document appears in exactly one of the two.
    ///
    /// Numbers rather than a count, although `Summary::skipped_pages` sends
    /// only the count today: the count cannot answer "which page of this
    /// contract did the scanner miss", and the reader is the only thing that
    /// ever knows.
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
/// An empty `pages` is a real answer, not an error: a scan of a paper contract
/// has pages, and none of them has text. The caller decides what to do with it
/// — `src/bin/worker.rs` refuses the file under `no_text_layer`, which is a
/// rule about content and is remembered as one.
pub fn extract_pdf(bytes: &[u8]) -> Result<PdfDocument, PdfError> {
    // First, and on its own line, because everything after it is about the
    // document and this is the only thing here that is not. `pdfium()` has not
    // been shown a file at this point and cannot fail for one.
    let pdfium = pdfium_probe::pdfium().map_err(PdfError::Library)?;
    let _serialised = lock_pdfium();

    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(load_failure)?;

    let mut pages = Vec::new();
    let mut skipped = Vec::new();

    for (i, page) in document.pages().iter().enumerate() {
        let page_no = (i + 1) as u32;
        // A page whose text cannot be read is **not** a page without a text
        // layer, and must not be recorded as one: `no_text_layer` is a verdict
        // about content that the journal keeps until `INDEX_FORMAT_VERSION`
        // moves, so a reader that guessed here would remember its own failure
        // as a fact about the scan. The whole document is refused instead,
        // which is what `page_texts` already does on the same failure — the
        // probe and the reader must not disagree about whether a file is
        // readable. What that costs is real and is the smaller cost: one
        // damaged page refuses a document whose other pages were fine, and
        // `SkipRule::Malformed` does not displace what the index already holds
        // unless the bytes moved.
        let raw = page
            .text()
            .map_err(|e| PdfError::Malformed(e.to_string()))?
            .all();

        if !has_text_layer(text_layer_chars(&raw)) {
            skipped.push(page_no);
            continue;
        }

        // Once, before the text is stored and after the threshold has been
        // taken from the same normalisation (D32, D38): `text_layer_chars`
        // normalises to count, so the string admitted and the string kept are
        // the same string.
        let text = nfc::normalise(&raw).into_owned();
        pages.push(PdfPage {
            page_no,
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
        });
    }

    Ok(PdfDocument { pages, skipped })
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

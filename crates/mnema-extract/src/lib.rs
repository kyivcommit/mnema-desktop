//! Document extraction.
//!
//! Plain text, markdown, PDF and HTML, plus the Pdfium binding probe that came
//! before any of them — enough to prove the library loads and that the binary
//! matches the bindings compiled against it, which `--probe-pdfium` still asks
//! of a packaged build (D53, D54).
//!
//! What a PDF page's text *is* beyond its text layer — reading order across
//! columns, hyphenation, tables, OCR for the pages `pdf::extract_pdf` skips —
//! is the extraction spec's subject and is deliberately still not decided here.

mod html;
pub mod manifest;
mod markdown;
mod pdf;
mod pdfium_probe;
mod text;
pub mod typing;
pub mod zip_part;

// The wire format moved to `mnema-core` and is re-exported here under the name
// task 7 committed to. It had to move: `mnema-pool` parses these frames and
// runs inside the application, and D40 requires that this crate — the one that
// links Pdfium — never enter the application's dependency graph. See
// `mnema_core::wire`'s module doc.
pub use mnema_core::wire;

// `Error` names `pdfium_probe::Error`, the only fallible reader this crate has.
// `text::extract_text` cannot fail (see its doc comment) and so defines no
// error type of its own — there is no second candidate competing for this
// name. A prior commit re-exported the wrong one: it kept `Error` bound to
// `text`'s (then non-empty) error and dropped `pdfium_probe::Error` from the
// public API entirely, so `probe_text_layer`'s actual return type became
// unnameable outside this crate.
//
// The second fallible reader has since arrived and took its own name, as that
// warning asked: `PdfError` (`pdf.rs`), which carries this one in its `Library`
// variant rather than replacing it. Keeping them separate is what lets the
// worker route "the library would not load" and "the document is damaged" to
// two different frames — one `Error` for both would have made that a decision
// about a string.
pub use html::{HtmlPage, extract_html};
pub use markdown::{MarkdownPage, SECTION_TITLE_MAX_CHARS, extract_markdown};
pub use pdf::{PdfDocument, PdfError, PdfPage, extract_pdf};
pub use pdfium_probe::{
    Error, PDFIUM_API_BUILD, PDFIUM_LIB_DIR_ENV, PageProbe, Stage, TEXT_LAYER_MIN_CHARS,
    probe_text_layer,
};
pub use text::extract_text;

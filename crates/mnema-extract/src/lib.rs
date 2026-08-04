//! Document extraction.
//!
//! At present this holds only the Pdfium binding probe: enough to prove the
//! library loads, that the binary matches the bindings compiled against it, and
//! that a page of text comes back. What a page's text *is* — reading order,
//! hyphenation, tables, OCR fallback — is the extraction spec's subject and is
//! deliberately not decided here.

pub mod manifest;
mod markdown;
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
// unnameable outside this crate. Should a second fallible reader arrive
// (pdf: crash, timeout; a zip-based format: a corrupt archive), it needs its
// own name rather than silently reusing this one.
pub use markdown::{MarkdownPage, SECTION_TITLE_MAX_CHARS, extract_markdown};
pub use pdfium_probe::{
    Error, PDFIUM_API_BUILD, PDFIUM_LIB_DIR_ENV, PageProbe, Stage, TEXT_LAYER_MIN_CHARS,
    probe_text_layer,
};
pub use text::extract_text;

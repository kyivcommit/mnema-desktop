//! This build's answer to "which reader takes which extension, and at what
//! version".
//!
//! The types are in `mnema-core` (see `mnema_core::manifest` for why); what
//! lives here is the part that is only true of *this* crate's readers, and that
//! changes when they do.

// Re-exported so a caller of `manifest()` can name what it got back without a
// second `use` line pointing at another crate — the same convenience `wire`
// already provides (`lib.rs`). The types are still `mnema-core`'s.
//
// `READER_PDF` comes through here for a sharper reason than convenience. The
// worker writes it into `Frame::Header::reader`, and `mnema_ingest::pages_of`
// matches on that string to give a PDF chunk a page number instead of a line
// range — across a process boundary and across D40, with no compiler between
// the two. A literal `"pdf"` in the worker would be a second spelling of a
// name that has to be one symbol: mistype it and the reader falls to
// `PageContext::Lines`, every PDF citation loses its page, and nothing goes
// red. The constant is `mnema-core`'s because neither side of that boundary
// owns it.
pub use mnema_core::manifest::{Manifest, READER_EPUB, READER_HTML, READER_PDF, ReaderId};

/// Bumped by whoever changes what that reader produces from the same bytes.
///
/// Not a build number and not a crate version: it is the thing the parent
/// compares to decide whether a document already in the index was made by
/// today's code. Leaving it alone after changing a reader means every document
/// that reader touched keeps the old reading for ever; bumping it without
/// changing anything costs one re-read of every such file.
pub const TEXT_READER_VERSION: u32 = 1;
pub const MARKDOWN_READER_VERSION: u32 = 1;
pub const PDF_READER_VERSION: u32 = 1;
pub const HTML_READER_VERSION: u32 = 1;
pub const EPUB_READER_VERSION: u32 = 1;

/// Keyed on extension rather than on reader name, and `html` is the entry the
/// whole mechanism was built for: `.html` was read by the *text* reader and is
/// recorded as `text@1` in every index written before this build, so a map of
/// reader versions alone would answer `text@1 == text@1` and never read those
/// files again. What the parent has to be able to see is that the extension
/// changed hands, and this is where it sees it.
///
/// **`pdf` is deliberately absent, and it is the first reader for which that
/// absence costs something.** This map is a claim about `typing::identify`, and
/// `identify` decides a PDF by its magic bytes, not by its name — so an entry
/// `pdf → pdf@1` would be a claim that a *text* file called `notes.pdf` is read
/// by the pdf reader, which it is not
/// (`the_manifest_names_the_reader_that_identify_actually_picks` is what holds
/// this map to that).
///
/// What the absence costs is measured and is not zero: the parent's cheap arm
/// compares the reader recorded on a path against this prediction
/// (`crates/mnema-ingest/src/lib.rs:274-280`), so every real `.pdf` records
/// `pdf@1`, is predicted `text@1`, and is handed to a worker on **every** walk.
/// The re-read is content-addressed and rebuilds nothing — that bill is
/// itemised at `mnema-ingest`'s own comment on the arm and measured by
/// `a_reader_no_build_agrees_on_is_re_read_every_pass_and_costs_only_that` —
/// but for this format the work per pass is a full pdfium parse, serialised
/// process-wide, rather than a text read. Closing it needs what that comment
/// names: a stored prediction per path, or a manifest that can say "this reader
/// is chosen by content". Both are decisions of their own; neither is a line
/// in this function.
pub fn manifest() -> Manifest {
    let mut by_extension = std::collections::BTreeMap::new();
    by_extension.insert(
        "md".to_string(),
        ReaderId::new("markdown", MARKDOWN_READER_VERSION),
    );
    // **Both spellings, and neither is optional.** `identify_plain_text`
    // matches `Some("html") | Some("htm")`, and this map is a claim about that
    // function: listing only one would predict the text reader for the other
    // and hand every `.htm` in the folder to a worker on every walk, for ever.
    // `the_manifest_names_the_reader_that_identify_actually_picks` holds the
    // two together.
    for extension in ["html", "htm"] {
        by_extension.insert(
            extension.to_string(),
            ReaderId::new(READER_HTML, HTML_READER_VERSION),
        );
    }
    Manifest {
        default: ReaderId::new("text", TEXT_READER_VERSION),
        by_extension,
    }
}

//! This build's answer to "which reader takes which extension, and at what
//! version".
//!
//! The types are in `mnema-core` (see `mnema_core::manifest` for why); what
//! lives here is the part that is only true of *this* crate's readers, and that
//! changes when they do.

// Re-exported so a caller of `manifest()` can name what it got back without a
// second `use` line pointing at another crate — the same convenience `wire`
// already provides (`lib.rs`). The types are still `mnema-core`'s.
pub use mnema_core::manifest::{Manifest, ReaderId};

/// Bumped by whoever changes what that reader produces from the same bytes.
///
/// Not a build number and not a crate version: it is the thing the parent
/// compares to decide whether a document already in the index was made by
/// today's code. Leaving it alone after changing a reader means every document
/// that reader touched keeps the old reading for ever; bumping it without
/// changing anything costs one re-read of every such file.
pub const TEXT_READER_VERSION: u32 = 1;
pub const MARKDOWN_READER_VERSION: u32 = 1;

/// Keyed on extension rather than on reader name, and the empty-looking map is
/// the point: `.html` has no entry today because the text reader takes it, and
/// the parent needs to see that entry *appear* to know the file must be read
/// again. A map of reader versions alone would answer `text@1 == text@1` and
/// never re-read it.
pub fn manifest() -> Manifest {
    let mut by_extension = std::collections::BTreeMap::new();
    by_extension.insert(
        "md".to_string(),
        ReaderId::new("markdown", MARKDOWN_READER_VERSION),
    );
    Manifest {
        default: ReaderId::new("text", TEXT_READER_VERSION),
        by_extension,
    }
}

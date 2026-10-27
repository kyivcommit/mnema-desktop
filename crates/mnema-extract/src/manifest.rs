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
pub use mnema_core::manifest::{
    Manifest, READER_DOCX, READER_EPUB, READER_HTML, READER_MARKDOWN, READER_PDF, READER_TEXT,
    READER_XLSX, ReaderId,
};

/// Bumped by whoever changes what that reader produces from the same bytes.
///
/// Not a build number and not a crate version: it is the thing the parent
/// compares to decide whether a document already in the index was made by
/// today's code. Leaving it alone after changing a reader means every document
/// that reader touched keeps the old reading for ever; bumping it without
/// changing anything costs one re-read of every such file.
pub const TEXT_READER_VERSION: u32 = 1;
pub const MARKDOWN_READER_VERSION: u32 = 1;
pub const PDF_READER_VERSION: u32 = 3;
pub const HTML_READER_VERSION: u32 = 1;
pub const EPUB_READER_VERSION: u32 = 1;
pub const DOCX_READER_VERSION: u32 = 1;
pub const XLSX_READER_VERSION: u32 = 1;

/// Every reader this build ships, by the name it answers to on the wire, and
/// the version of it this build has.
///
/// One table rather than a `match` beside the map `manifest()` builds: those
/// would be the same fact written twice, and the day a reader is added to one
/// and not the other is the day the parent stops re-reading it — silently, for
/// the life of the index.
const READERS: [(&str, u32); 7] = [
    (READER_TEXT, TEXT_READER_VERSION),
    (READER_MARKDOWN, MARKDOWN_READER_VERSION),
    (READER_PDF, PDF_READER_VERSION),
    (READER_HTML, HTML_READER_VERSION),
    (READER_EPUB, EPUB_READER_VERSION),
    (READER_DOCX, DOCX_READER_VERSION),
    (READER_XLSX, XLSX_READER_VERSION),
];

/// Today's version of the reader that answers to `reader`, or `None` for a
/// name this build does not ship.
///
/// **By name, because that is the only fact a `path` row carries about who
/// made it.** The parent may not link this crate (D40), so it cannot ask this
/// function; `manifest()` ships the same table in `Manifest::by_reader` and the
/// parent calls [`Manifest::version_of`] there. What it wants it for: a reader
/// picking files by magic bytes has no extension entry, so the extension the
/// parent can see says nothing about the version of the reader that actually
/// read the file.
pub fn reader_version(reader: &str) -> Option<u32> {
    READERS
        .iter()
        .find(|(name, _)| *name == reader)
        .map(|(_, version)| *version)
}

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
/// (`crates/mnema-ingest/src/lib.rs:293-298`), so every real `.pdf` records the
/// `pdf` reader, is predicted `text`, and is handed to a worker on **every** walk.
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
        ReaderId::new(READER_MARKDOWN, MARKDOWN_READER_VERSION),
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
        default: ReaderId::new(READER_TEXT, TEXT_READER_VERSION),
        by_extension,
        by_reader: READERS
            .iter()
            .map(|(reader, version)| ((*reader).to_string(), *version))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No reader this build ships may be missing from the table, and the
    /// second half is the one that can go stale without a red build anywhere
    /// else in the repository.
    ///
    /// The manifest an extension names has its version checked against the
    /// table, so a reader added to `by_extension` without a row here fails. The
    /// readers decided by magic bytes have no extension to walk, so they are
    /// listed here — and the list is not the guard that matters: `worker_cli`'s
    /// `every_reader_the_worker_emits_is_one_the_manifest_can_version` runs the
    /// binary and compares what it *emitted* against this table, which catches a
    /// reader without extending any list (review M1).
    #[test]
    fn every_reader_this_build_ships_has_a_version() {
        let manifest = manifest();
        for id in std::iter::once(&manifest.default).chain(manifest.by_extension.values()) {
            assert_eq!(
                reader_version(&id.reader),
                Some(id.version),
                "the manifest gives .{} to a reader, so the table has to state \
                 its version",
                id.reader
            );
        }
        for reader in [
            READER_PDF,
            READER_HTML,
            READER_EPUB,
            READER_DOCX,
            READER_XLSX,
        ] {
            assert!(
                reader_version(reader).is_some(),
                "{reader} may pick a file by its magic bytes, so no extension \
                 would reveal that its version is missing from the table"
            );
        }
        // And the two lists are the same set, in both directions. A name without
        // a row is a reader the parent cannot version — the state `worker_cli`'s
        // M1 test could only see if a fixture happened to reach it; a row without
        // a name is a version nothing can ask for (review N2).
        let named: std::collections::BTreeSet<&str> =
            mnema_core::manifest::READER_NAMES.into_iter().collect();
        let tabled: std::collections::BTreeSet<&str> =
            READERS.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            named, tabled,
            "a reader this build names must be one this table can version, and \
             the reverse: the two lists are one fact"
        );
        assert_eq!(
            reader_version("no-such-reader"),
            None,
            "a name this build does not ship is not a version anything can judge"
        );
    }

    /// The table reaches the parent, or the whole lookup is local.
    ///
    /// `Manifest::by_reader` is the only road out of this crate (D40), so a
    /// table that is not in the manifest the worker prints is a version no
    /// parent can compare against.
    #[test]
    fn the_table_is_in_the_manifest_this_build_prints() {
        let manifest = manifest();
        for (reader, version) in READERS {
            assert_eq!(manifest.version_of(reader), Some(version), "{reader}");
        }
    }
}

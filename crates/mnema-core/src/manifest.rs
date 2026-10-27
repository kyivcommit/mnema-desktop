//! What a worker's readers are, stated rather than inferred.
//!
//! `mnema-ingest` may not depend on `mnema-extract` (D40), and it decides
//! whether a file needs re-reading *before* a worker runs — so the versions
//! cannot arrive as a constant it links. They arrive as this, once per run,
//! from the worker's `--manifest` branch. The types live here for the same
//! reason `wire` does: what crosses the boundary belongs to neither side of it.
//!
//! A map of versions alone would not be enough, and the case that shows why is
//! `.html`: indexed today it is recorded as `text@1`, and `text@1 == text@1`
//! answers "unchanged" for ever — a file whose reader changed hands would never
//! be re-read. What the parent has to be able to see is that the *extension*
//! changed hands, so the manifest is keyed on extension.
//!
//! Only the plain-text branch is here. Everything decided by magic
//! (`typing::identify`) lands in the skip journal instead, where
//! `INDEX_FORMAT_VERSION` is the lever — see the spec, §2.4.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The names the format readers answer to, on the wire and in the index.
///
/// A string is what actually crosses the boundary — `Frame::Header::reader`,
/// [`ReaderId::reader`] and the `path.reader` column all carry one — and until
/// these constants existed the same name was written out as an independent
/// literal on each side. `mnema_ingest::pages_of` picks a chunk's coordinate by
/// that name, so a spreadsheet reader that called itself `"spreadsheet"` would
/// fall to the line-number default and cite "рядки 10–20" with no sheet on
/// them: plausible, silent, and green, because the only test naming the other
/// literal would have been written by whoever changed it.
///
/// **What they are is one symbol, not a check.** Nothing makes a reader use
/// them: `mnema-extract` is free to print any string it likes, and D40 keeps
/// `mnema-ingest` from ever looking. What they buy is that the two sides can
/// name the same thing rather than agree by coincidence — and the values below
/// are pinned, because `mnema-ingest/tests/slice.rs` states these literals to a
/// stand-in worker and asserts the coordinate that comes back.
///
/// The five formats of G7.1, and the two the default arm used to stand in for.
///
/// **`text` and `markdown` joined when they gained a second and third user.**
/// They were literals in four places at once — the worker's headers,
/// `manifest()`'s `default`, its `md` entry, and the version table
/// `mnema-extract` builds — and drift between those spellings is silent in the
/// worst direction: the parent's lookup answers `None` and keeps a document it
/// should have re-read. A constant is not a check (nothing makes a reader use
/// it), but it is one spelling rather than four.
pub const READER_TEXT: &str = "text";
pub const READER_MARKDOWN: &str = "markdown";
pub const READER_PDF: &str = "pdf";
pub const READER_HTML: &str = "html";
pub const READER_DOCX: &str = "docx";
pub const READER_EPUB: &str = "epub";
pub const READER_XLSX: &str = "xlsx";

/// Every reader name above, in one list.
///
/// **What a name in this list buys is a red build without a fixture.**
/// `mnema-extract`'s `READERS` pairs each of these names with the version this
/// build has, and its unit test requires the two lists to be the same set — so a
/// reader named here whose version row is missing is caught by that test, not by
/// whether some file in `crates/mnema-extract/tests/fixtures/` happens to be read
/// by it. That is the direction `worker_cli`'s
/// `every_reader_the_worker_emits_is_one_the_manifest_can_version` cannot see:
/// it runs a file per reader, and a reader no fixture reaches emits nothing to
/// compare (review M1, and its own N2).
///
/// What no list can catch: a reader added to the worker whose name is written as
/// a literal there and never listed here. Rust cannot enumerate a string that
/// does not exist anywhere; the rule is that a reader this build ships is named
/// in this file, and this is the list it is named in.
pub const READER_NAMES: [&str; 7] = [
    READER_TEXT,
    READER_MARKDOWN,
    READER_PDF,
    READER_HTML,
    READER_DOCX,
    READER_EPUB,
    READER_XLSX,
];

/// A reader and the version of it that produced (or would produce) a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderId {
    pub reader: String,
    pub version: u32,
}

impl ReaderId {
    /// `pub` because the crate that builds the manifest is not this one:
    /// the versions are `mnema-extract`'s to state (they change when its
    /// readers change), and the type is here only so that the parent can name
    /// it without linking that crate.
    pub fn new(reader: &str, version: u32) -> Self {
        Self {
            reader: reader.to_string(),
            version,
        }
    }
}

/// What every reader in a build is, by the signal the parent can see without
/// opening the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// The answer for an extension not in the map, and for no extension at
    /// all — `identify_plain_text`'s `_ =>` arm, stated. Not a gap and not a
    /// "don't know": a file with no extension really is read, by the text
    /// reader, and the parent has to be able to version that reading.
    pub default: ReaderId,
    pub by_extension: BTreeMap<String, ReaderId>,
    /// Every reader this build ships, by the name it answers to on the wire,
    /// and the version of it this build has.
    ///
    /// **`by_extension` cannot stand in for this, which is why it exists.** A
    /// parent asking "is the reader that made this row older than the one this
    /// build has?" may not link `mnema-extract` (D40), so the answer arrives
    /// over `--manifest` with everything else. Keyed on an extension, that map
    /// has no entry for a reader the worker picks by magic bytes, and `pdf` is
    /// deliberately absent from it — so the rows written by exactly those
    /// readers are the ones the extension map cannot judge. `text` and
    /// `markdown` are here too: a `path` row can name any reader this build
    /// ships, and the lookup is by that name rather than by what the extension
    /// predicts.
    ///
    /// `#[serde(default)]` because a worker from before this field answers a
    /// manifest without it: the parent then knows no versions and keeps every
    /// document, which is the side that loses nothing.
    #[serde(default)]
    pub by_reader: BTreeMap<String, u32>,
}

impl Manifest {
    /// Which reader takes a file with this extension.
    ///
    /// The lookup is **exact and case-sensitive**, and that is not an
    /// oversight to fix later: it mirrors `identify_plain_text`
    /// (`crates/mnema-extract/src/typing.rs:336-343`), which matches
    /// `Some("md")` against the extension exactly as it comes off the path.
    /// A manifest that lowercased would answer "markdown" for `NOTES.MD`
    /// while the worker read it as text — and the parent would then compare a
    /// document against the version of a reader that never touched it. The
    /// worker and this map have to be wrong in the same way or not at all;
    /// `crates/mnema-extract/tests/manifest.rs` holds them to it.
    ///
    /// `ext` is `Option` because a path may have no extension, and that case
    /// shares the default arm with an unrecognised one — again mirroring
    /// `identify_plain_text`, where both fall to the same `_ =>`.
    pub fn for_extension(&self, ext: Option<&str>) -> &ReaderId {
        ext.and_then(|ext| self.by_extension.get(ext))
            .unwrap_or(&self.default)
    }

    /// Today's version of the reader that answers to `reader`, or `None` for a
    /// name this build's worker does not state.
    ///
    /// `None` is a row written by a build that had a reader this one does not —
    /// a sidecar of another release, a reader removed — and nothing here can
    /// judge it, so callers have to answer from the name they see.
    pub fn version_of(&self, reader: &str) -> Option<u32> {
        self.by_reader.get(reader).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        let mut by_extension = BTreeMap::new();
        by_extension.insert("md".to_string(), ReaderId::new("markdown", 3));
        let mut by_reader = BTreeMap::new();
        by_reader.insert("text".to_string(), 1);
        by_reader.insert("markdown".to_string(), 3);
        by_reader.insert("pdf".to_string(), 2);
        Manifest {
            default: ReaderId::new("text", 1),
            by_extension,
            by_reader,
        }
    }

    /// Both directions in one test on purpose: an implementation that always
    /// returned the default would pass the miss cases alone, and one that
    /// returned the first entry of the map would pass the hit case alone.
    #[test]
    fn an_extension_in_the_map_wins_and_everything_else_falls_to_the_default() {
        let manifest = sample();
        assert_eq!(
            manifest.for_extension(Some("md")),
            &ReaderId::new("markdown", 3)
        );
        // Not in the map — the text reader takes it, and says so.
        assert_eq!(
            manifest.for_extension(Some("txt")),
            &ReaderId::new("text", 1)
        );
        // No extension at all is the same answer, not a missing one.
        assert_eq!(manifest.for_extension(None), &ReaderId::new("text", 1));
    }

    /// The case rule, pinned where it is decided. `identify_plain_text` matches
    /// `Some("md")` and nothing else, so `MD` is read by the text reader — and
    /// this map has to say the same thing rather than the more helpful one.
    #[test]
    fn a_differently_cased_extension_is_not_the_same_extension() {
        assert_eq!(
            sample().for_extension(Some("MD")),
            &ReaderId::new("text", 1)
        );
    }

    /// The parent parses this out of a worker's stdout, so the shape it reads
    /// — an object keyed by extension, each value carrying `reader` and
    /// `version` — is part of the interface and not a serde detail.
    #[test]
    fn a_manifest_round_trips_as_the_object_the_parent_reads() {
        let json = serde_json::to_string(&sample()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            v["by_extension"]["md"]["reader"],
            serde_json::json!("markdown")
        );
        assert_eq!(v["by_extension"]["md"]["version"], serde_json::json!(3));
        assert_eq!(v["default"]["reader"], serde_json::json!("text"));
        assert_eq!(serde_json::from_str::<Manifest>(&json).unwrap(), sample());
    }

    /// **The lookup is by the reader's own name, and that is the whole
    /// reason the map exists.** `.pdf` is deliberately absent from
    /// `by_extension` (`mnema-extract`'s manifest carries why), so an
    /// extension lookup answers `text@1` for every path the pdf reader made. A
    /// `path` row saying `pdf@1` has to be judged against the pdf reader's own
    /// version, and this is the map that can.
    #[test]
    fn a_reader_no_extension_names_still_has_a_version() {
        let manifest = sample();
        assert_eq!(manifest.for_extension(Some("pdf")).reader, "text");
        assert_eq!(manifest.version_of("pdf"), Some(2));
        assert_eq!(manifest.version_of("no such reader"), None);
    }

    /// A worker from before this field answers a manifest without it, and the
    /// parent still has to parse it — knowing no versions, which is the side
    /// that keeps every document.
    #[test]
    fn a_manifest_without_versions_is_read_as_one_that_knows_none() {
        let older = r#"{"default":{"reader":"text","version":1},"by_extension":{}}"#;
        let manifest: Manifest =
            serde_json::from_str(older).expect("the older manifest still parses");
        assert!(manifest.by_reader.is_empty());
        assert_eq!(manifest.version_of("text"), None);
    }
}

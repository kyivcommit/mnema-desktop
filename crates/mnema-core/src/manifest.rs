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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        let mut by_extension = BTreeMap::new();
        by_extension.insert("md".to_string(), ReaderId::new("markdown", 3));
        Manifest {
            default: ReaderId::new("text", 1),
            by_extension,
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
}

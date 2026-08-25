//! Read-only enumeration for the launcher's left card (§7): watched folders,
//! their indexed files, and recently-indexed documents. Read-only, so it adds
//! no data-loss surface (spec §12); it simply reflects the index at query time.
//! PR 5's `source_around` will join this module.

use crate::error::Error;
use crate::state::AppState;
use mnema_core::Segment;
use mnema_index::Db;
use serde::Serialize;
use tauri::State;

/// The "Recent" tab cap. Tunable; the launcher shows a bounded list, not the
/// whole corpus.
const RECENTS_LIMIT: i64 = 50;

/// The widest window `source_around` will read, in blocks either side.
///
/// The radius is the caller's choice — the card shows one paragraph either
/// side, a scrolling card wants more — but a client must not be able to ask
/// for a whole book. **Clamped, not rejected:** a bad radius is not a reason
/// to show the user nothing.
const MAX_RADIUS: u32 = 20;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeListing {
    pub roots: Vec<TreeRoot>,
    pub recents: Vec<RecentDoc>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeRoot {
    pub root_id: i64,
    pub absolute_path: String,
    pub name: String,
    pub files: Vec<TreeFile>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeFile {
    pub relative_path: String,
    pub document_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentDoc {
    pub document_id: String,
    pub root_id: i64,
    pub relative_path: String,
    pub indexed_at: i64,
}

/// Display name for a watched root: the final path component, or the whole path
/// when there is none (e.g. `/`).
fn basename(absolute_path: &str) -> String {
    std::path::Path::new(absolute_path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| absolute_path.to_string())
}

/// The whole catalogue, assembled from the `mnema-index` reads. A private helper
/// kept out of the command body for readability; `list_tree` is a thin
/// `with_index` wrapper over it. The command is exercised through the IPC in
/// Task 4.2's test, per the `commands.rs` doctrine (a direct call would not prove
/// it is registered or that its fields survive the camelCase rename).
fn build_tree_listing(db: &Db) -> Result<TreeListing, mnema_index::Error> {
    let roots = db
        .list_watched_roots()?
        .into_iter()
        .map(|r| {
            let files = db
                .indexed_files_under_root(r.id)?
                .into_iter()
                .map(|f| TreeFile {
                    relative_path: f.relative_path,
                    document_id: f.document_id,
                })
                .collect();
            Ok(TreeRoot {
                name: basename(&r.absolute_path),
                root_id: r.id,
                absolute_path: r.absolute_path,
                files,
            })
        })
        .collect::<Result<Vec<_>, mnema_index::Error>>()?;

    let recents = db
        .recent_indexed_documents(RECENTS_LIMIT)?
        .into_iter()
        .map(|d| RecentDoc {
            document_id: d.document_id,
            root_id: d.watched_root_id,
            relative_path: d.relative_path,
            indexed_at: d.indexed_at,
        })
        .collect();

    Ok(TreeListing { roots, recents })
}

/// Off the main thread for the reason given on [`crate::bridge::open_index`].
///
/// The whole listing is read inside one [`mnema_index::Db::read_snapshot`], not
/// as three autocommit reads. The roots-and-files reads and the recents read
/// would otherwise be able to straddle the indexing job's `pending → indexed`
/// commit — landing on its own connection, outside the window's mutex — and the
/// returned listing could then carry a recent whose `(rootId, relativePath)` is
/// absent from every `roots[].files`, a torn read the selection consumer (PR 6)
/// cannot resolve. `build_tree_listing` only reads, so it is a safe closure for
/// `read_snapshot`.
#[tauri::command(async)]
pub fn list_tree(state: State<'_, AppState>) -> Result<TreeListing, Error> {
    state.with_index(|db| db.read_snapshot(build_tree_listing))
}

/// What the launcher's right card paints around a cited passage, or the
/// refusal that says the passage is no longer there.
///
/// ⚠️ `rename_all_fields`, and it is **not** decoration. `rename_all` on an
/// *enum* renames the variants only; the fields inside a struct variant keep
/// their snake_case names without this second attribute. Measured, not
/// reasoned: without it `Excerpt` ships `document_id` and `has_more_before`
/// inside a camelCase payload. The `AskAnswer` precedent (`bridge.rs:448-451`)
/// does not warn, because every field of its struct variants is one word.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum SourceAround {
    // Defined in full here, constructed by the arm Task 5.3 puts where
    // `build_source_around`'s `todo!()` stands. The allow goes when the
    // `todo!()` does; the two are one temporary state, not two.
    #[allow(dead_code)]
    Excerpt {
        /// The blocks in document reading order: `radius` before the passage's
        /// own block(s), those blocks, then `radius` after.
        blocks: Vec<SourceBlock>,
        /// Where to paint, measured into `SourceBlock::text` by `blockStart`.
        spans: Vec<WireSegment>,
        /// The excerpt's own provenance — the content hash of the document the
        /// blocks actually came from, so a caller can see it disagreeing with
        /// the citation it came from.
        ///
        /// ⚠️ **No `relative_path` here, deliberately.** No read method can
        /// produce one for the excerpt: `ChunkAnchor` carries no path, and
        /// `PathOccupant::relative_path` is the *cited* path — the query key —
        /// so the field could only ever echo the caller's own input back and
        /// disagree with nothing. The card's header uses the citation's
        /// `relativePath` (`bridge.rs:433`); the provenance check is
        /// `documentId`.
        document_id: String,
        section_title: Option<String>,
        has_more_before: bool,
        has_more_after: bool,
        freshness: Freshness,
    },
    /// The passage is not at that id any more. **No text is returned**: the
    /// alternative is another chunk's neighbourhood under the user's citation.
    Gone { reason: GoneReason },
}

/// Why the passage is not at that id any more. Two causes, split for the same
/// reason `RefusalKind` splits `ask`'s (`bridge.rs:439-446`): a caller may
/// render one sentence for both, and the split is what makes both directions
/// assertable here.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum GoneReason {
    /// No chunk carries that id. Three causes, and the third is the common
    /// one: the document was deleted; a rebuild has not re-inserted yet; or
    /// **the file was edited and re-walked, so `repoint` gave the path to a new
    /// `document_id` and `forget_if_unnamed` deleted the displaced document
    /// outright**, cascading its chunks away. That last one is why `Reindexed`
    /// is rare and this is not.
    NoSuchChunk,
    /// A chunk carries that id, and it is not this passage: the id was reused
    /// by a rebuild or by another document. `chunk.id` is `INTEGER PRIMARY
    /// KEY` without `AUTOINCREMENT`, so SQLite hands the ids of deleted rows
    /// out again — and `ask` and `source_around` are two IPC calls seconds
    /// apart, which no snapshot can span.
    IdReused,
}

/// How far the indexed text has drifted from the file it was read out of.
// Every variant is decided by Task 5.3, in the same arm the `Excerpt` above
// waits on; the allow goes with that one.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Freshness {
    /// The path still names this document and the file matches the size and
    /// mtime recorded at index time.
    Current,
    /// The path now names a different document: a walk has already re-indexed
    /// this file. What is shown is what was indexed.
    Reindexed,
    /// The path still names this document, but the bytes on disk have moved:
    /// no walk has reached it yet. What is shown is what was indexed.
    FileChanged,
    /// Nothing at that path any more (or it cannot be measured).
    FileMissing,
    /// The document has no `path` row — indexed from inside an archive, or its
    /// last copy on disk was deleted. Nothing to compare against.
    NoPath,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceBlock {
    pub block_id: i64,
    /// `block.type` verbatim, so a caller can drop page headers and footers
    /// without the backend baking that display choice in.
    pub kind: String,
    pub text: String,
    pub page_no: i64,
    pub reading_order: i64,
}

/// A camelCase mirror of [`mnema_core::Segment`] for the wire.
///
/// ⚠️ **Do not "fix" this by putting `rename_all` on `Segment` itself.** That
/// type is *persisted*: `chunk.char_span` stores `serde_json::to_string` of a
/// `Vec<Segment>` (`write.rs:840`) and the schema reads the stored JSON by key
/// — the `CHECK` extracts `$[0].block_id` and the `chunk_span_blocks_bi`
/// trigger extracts `$.block_id`. Renaming the field would make every stored
/// row unreadable and both guards blind. The same "local mirror at the seam"
/// move `Hit` already makes for `Citation` (`bridge.rs:88-92`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireSegment {
    pub block_id: i64,
    pub start: u32,
    pub end: u32,
    pub block_start: u32,
}

impl From<Segment> for WireSegment {
    fn from(s: Segment) -> Self {
        WireSegment {
            block_id: s.block_id,
            start: s.start,
            end: s.end,
            block_start: s.block_start,
        }
    }
}

/// The identity pin, and nothing else yet.
///
/// `chunk_id` alone cannot say whether it still names the passage the user
/// clicked, so the caller echoes the passage text back and this compares it
/// against `chunk.text` **exactly**. Not `contains`, not trimmed, not
/// normalised: the text on the wire is `chunk.text` verbatim — `AskCitation.
/// text` (`bridge.rs:432`) is `h.text.clone()` (`bridge.rs:556`), which is
/// `Citation::text`, which is the `chunk.text` column (`write.rs:894`) — so
/// any loosening only widens what a reused id can pass as. A `contains`
/// comparison would accept `""` and match every chunk in the index.
///
/// ⚠️ Signature note: Task 5.3 changes the return type to an intermediate the
/// command finishes, because `Excerpt` carries a `freshness` that cannot be
/// decided without a `stat`, and no filesystem call may happen inside
/// `read_snapshot`. Today every arm this function reaches is settled inside
/// the snapshot, so it returns the answer directly.
fn build_source_around(
    db: &Db,
    chunk_id: i64,
    passage_text: &str,
    _cited_relative_path: Option<&str>,
    _radius: i64,
) -> Result<SourceAround, mnema_index::Error> {
    let Some(anchor) = db.chunk_anchor(chunk_id)? else {
        return Ok(SourceAround::Gone {
            reason: GoneReason::NoSuchChunk,
        });
    };
    if anchor.text != passage_text {
        return Ok(SourceAround::Gone {
            reason: GoneReason::IdReused,
        });
    }
    todo!("Task 5.3: the excerpt and its freshness")
}

/// The paragraphs around a cited passage, read out of the index — never off
/// the disk (spec §12), which the webview could not read anyway under
/// `default-src 'self'`.
///
/// Off the main thread for the reason given on [`crate::bridge::open_index`],
/// and every index read inside one [`mnema_index::Db::read_snapshot`].
#[tauri::command(async)]
pub fn source_around(
    state: State<'_, AppState>,
    chunk_id: i64,
    passage_text: String,
    cited_relative_path: Option<String>,
    radius: u32,
) -> Result<SourceAround, Error> {
    let radius = radius.clamp(1, MAX_RADIUS) as i64;
    state.with_index(|db| {
        db.read_snapshot(|db| {
            build_source_around(
                db,
                chunk_id,
                &passage_text,
                cited_relative_path.as_deref(),
                radius,
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TreeListing {
        TreeListing {
            roots: vec![TreeRoot {
                root_id: 7,
                absolute_path: "/tmp/alpha".into(),
                name: "alpha".into(),
                files: vec![TreeFile {
                    relative_path: "a.txt".into(),
                    document_id: "d".repeat(64),
                }],
            }],
            recents: vec![RecentDoc {
                document_id: "d".repeat(64),
                root_id: 7,
                relative_path: "a.txt".into(),
                indexed_at: 2000,
            }],
        }
    }

    #[test]
    fn wire_shape_is_camel_case() {
        let v = serde_json::to_value(sample()).unwrap();
        let root = &v["roots"][0];
        let recent = &v["recents"][0];

        // Present, camelCase.
        assert!(root["rootId"].is_i64());
        assert!(root["absolutePath"].is_string());
        assert!(root["files"][0]["relativePath"].is_string());
        assert!(root["files"][0]["documentId"].is_string());
        assert!(recent["indexedAt"].is_i64());
        assert!(recent["documentId"].is_string());

        // Absent — the snake_case names must not leak (guards rename_all).
        assert!(root.get("root_id").is_none());
        assert!(root.get("absolute_path").is_none());
        assert!(root["files"][0].get("relative_path").is_none());
        assert!(recent.get("indexed_at").is_none());
        assert!(recent.get("document_id").is_none());
    }
}

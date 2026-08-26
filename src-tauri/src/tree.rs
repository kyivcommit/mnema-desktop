//! Read-only reads for the launcher's two side cards: the left card's watched
//! folders, their indexed files, and recently-indexed documents (§7,
//! [`list_tree`]); the right card's paragraphs around a cited passage (§7.1,
//! [`source_around`]). Read-only, so neither command adds a data-loss surface
//! (spec §12) — they only reflect the index at query time, and `source_around`
//! refuses rather than answer with text a rebuild has since replaced.

use crate::error::Error;
use crate::state::AppState;
use mnema_core::Segment;
use mnema_index::{Db, PathOccupant};
use serde::Serialize;
use tauri::State;

/// The "Recent" tab cap. Tunable; the launcher shows a bounded list, not the
/// whole corpus.
const RECENTS_LIMIT: i64 = 50;

/// The widest window `source_around` will read, in blocks either side.
///
/// The radius is the caller's choice — the card shows one paragraph either
/// side, a scrolling card wants more — but a client must not be able to ask
/// for a whole book. **Clamped, not rejected:** `0` is answered as `1` (the
/// passage's own block plus one either side), and anything above this ceiling
/// is answered at the ceiling. Neither is an error.
///
/// ⚠️ That promise covers the values that reach the command. The parameter is
/// `u32`, so a negative or fractional radius is refused one layer earlier, by
/// deserialisation, and the caller gets an error rather than a clamped
/// answer. PR 6 must always send a non-negative integer.
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
/// does not warn (its variants are at `bridge.rs:455-475`), because every
/// field of its struct variants is one word.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum SourceAround {
    Excerpt {
        /// The blocks in document reading order: `radius` before the passage's
        /// own block(s), those blocks, then `radius` after.
        blocks: Vec<SourceBlock>,
        /// Where to paint. See [`WireSegment`] for the unit and the arithmetic
        /// — both are easy to get wrong from the payload alone, and getting
        /// them wrong moves the highlight silently.
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
///
/// Five variants, and every one of them has a test that produced it: a card
/// with a default branch drawing `Current` for an unmatched variant is how a
/// stale excerpt gets shown as fresh.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Freshness {
    /// The path still names this document and the file matches the size and
    /// mtime recorded at index time.
    Current,
    /// The cited path now names a **different document**. What is shown is
    /// what was indexed.
    ///
    /// ⚠️ **Two causes, not one**, and the index cannot tell them apart: a
    /// walk re-indexed the cited copy and `repoint` gave its row to the new
    /// content hash — or that location was never this document's, and some
    /// other root simply holds a file of the same relative name. The verdict
    /// is true either way (the cited path does name another document); only
    /// the explanation would differ, and this cannot supply it.
    ///
    /// Reached when the single `path` row at the cited location names some
    /// other document. (It used to be described in terms of `cited_occupant`'s
    /// two branches; there is one path through that function now.)
    Reindexed,
    /// The path still names this document, but the bytes on disk have moved:
    /// no walk has reached it yet. What is shown is what was indexed.
    FileChanged,
    /// Nothing at that path any more (or it cannot be measured).
    FileMissing,
    /// **The cited location cannot be pinned down** — nothing to compare
    /// against. Three states reach it, and a caller rendering this tag must
    /// not say "the file is gone", because two of them are not that:
    ///
    /// 1. the citation carried no `relativePath` at all (a document indexed
    ///    from inside an archive, or whose last copy on disk was deleted);
    /// 2. no `path` row holds that relative path under any root — the row
    ///    vanished between the two IPC calls;
    /// 3. more than one root holds that relative path — **whether or not
    ///    anything distinguishes them**, see `cited_occupant` — so a verdict
    ///    would be about a file the user may not have cited.
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
/// `Vec<Segment>` (`Db::insert_chunk_in`) and the schema reads the stored JSON by key
/// — the `CHECK` extracts `$[0].block_id` and the `chunk_span_blocks_bi`
/// trigger extracts `$.block_id`. Renaming the field would make every stored
/// row unreadable and both guards blind. The same "local mirror at the seam"
/// move `Hit` already makes for `Citation` (`bridge.rs:93-101`).
/// 🔴 **How to paint from this, because the payload alone does not say.**
///
/// - `blockStart` is the offset **into the text of the block `blockId` names**,
///   which is in `blocks`. That is where the highlight begins.
/// - `start`/`end` are offsets into the **chunk's own text**, and the chunk's
///   text is *not* on the wire. Their only use here is the length:
///   `len = end - start`. Do not index anything with them.
/// - 🔴 **The unit is Unicode scalar values, not UTF-16 code units.** Every
///   offset this pipeline emits comes from `text.chars().count()`. For Cyrillic
///   and Latin the two agree; one emoji or any character outside the BMP
///   earlier in the paragraph and they diverge, moving the highlight with no
///   error anywhere. In JavaScript that means `[...text].slice(a, b).join("")`,
///   **never** `text.slice(a, b)`. `ui/src/launcher/state.ts` already counts
///   code points for the same reason (D131).
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

/// [`SourceAround::Excerpt`] minus its `freshness` — everything one snapshot
/// can know about a passage that is still there.
///
/// A struct rather than six fields repeated at the seam: `Excerpt` is built
/// from it by [`ExcerptFields::with`], so the two cannot drift apart field by
/// field. It deliberately holds `document_id` **once**; the freshness
/// comparison reads it from here rather than from a second copy carried
/// alongside, because two copies of one fact is the class this project has
/// paid most for and a later refactor comparing the wrong one would be silent.
struct ExcerptFields {
    blocks: Vec<SourceBlock>,
    spans: Vec<WireSegment>,
    document_id: String,
    section_title: Option<String>,
    has_more_before: bool,
    has_more_after: bool,
}

impl ExcerptFields {
    fn with(self, freshness: Freshness) -> SourceAround {
        SourceAround::Excerpt {
            blocks: self.blocks,
            spans: self.spans,
            document_id: self.document_id,
            section_title: self.section_title,
            has_more_before: self.has_more_before,
            has_more_after: self.has_more_after,
            freshness,
        }
    }
}

/// What one snapshot can know. The command turns this into a
/// [`SourceAround`] after the `stat`, which is the only step that needs the
/// filesystem and the one step that must not happen inside the snapshot.
enum Composed {
    /// Nothing further to decide — the pin already refused.
    Settled(SourceAround),
    /// An excerpt whose `freshness` is still open, plus the `path` row
    /// deciding it needs: the recorded size and mtime, the absolute root, and
    /// which document that location names now. `None` when the citation
    /// carried no path, when no row is at that location, or when the root
    /// could not be resolved to exactly one candidate — all three are
    /// [`Freshness::NoPath`], because a guessed root is a confident verdict
    /// about the wrong file.
    Pending {
        excerpt: ExcerptFields,
        occupant: Option<PathOccupant>,
    },
}

/// The `path` row the freshness verdict is read off, or `None` when there is
/// no single honest one.
///
/// **Keyed on the cited location — now the cited *root*, when the citation
/// carries one — and on nothing else.** A citation carries `rootId`
/// (`AskCitation::root_id`) whenever its chunk's document sits under exactly
/// one watched root; `citedRootId: null` is not a stale client but the
/// legitimate answer for zero or several roots (`Citation::root_id`,
/// `write.rs:95-106`), and falls back to the location-only rule this
/// function used before that field existed. Keying on the document instead of
/// the location is wrong in a way that took two reviews to see — after a walk
/// repoints an edited copy, `WHERE document_id = ?` no longer sees that copy at
/// all and returns some other, untouched one, reporting `Current` for a
/// passage whose cited copy is stale.
///
/// 🔴 **Narrowing the candidates by document is deliberately gone, and this is
/// the second decision, not a restatement of the first.** An earlier version
/// kept both: resolve by document, fall back to the location. Owner review on
/// PR #22 reproduced what that costs — two roots holding the *same* document at
/// one path answer `None`, and then editing one copy leaves a single survivor,
/// so the same citation starts answering `Current`, growing confident at the
/// moment it should grow careful. The two situations are shape-identical from
/// the index (two rows at the path, one naming this document, in both), so no
/// rule over those counts separates them — which is the argument *for* the
/// blunt rule, not against it.
///
/// **PR 6 closes the cost the doc comment above used to book here.** Two
/// watched roots that share a relative path — two note folders each holding a
/// `README.md` — no longer lose the verdict permanently: a citation minted
/// with `rootId` names which of the two copies was cited, and `path_occupant`
/// is asked about that one directly, skipping the ambiguity scan below
/// entirely. Only a citation with no root to name — `citedRootId: null` — still
/// falls back to the blunt "exactly one candidate, or `None`" rule, which is
/// unchanged from before this PR.
fn cited_occupant(
    db: &Db,
    cited_root_id: Option<i64>,
    cited_relative_path: Option<&str>,
) -> Result<Option<PathOccupant>, mnema_index::Error> {
    let Some(relative_path) = cited_relative_path else {
        return Ok(None);
    };
    match cited_root_id {
        Some(root) => db.path_occupant(root, relative_path),
        // No root on the citation — its chunk's document names zero or
        // several distinct watched roots, so `Citation::root_id` was `None`
        // when this citation was minted (`write.rs:95-106`), not a citation
        // from before the field existed. Unchanged fallback: exactly one root
        // holds the path, or no verdict.
        None => {
            let roots = db.roots_holding_path(relative_path)?;
            let [root] = roots.as_slice() else {
                return Ok(None);
            };
            db.path_occupant(*root, relative_path)
        }
    }
}

/// The identity pin, then everything one snapshot can know about the passage
/// it let through.
///
/// **The pin has two halves, and both must hold.** `chunk_id` alone cannot say
/// whether it still names the passage the user clicked, so the caller echoes
/// the passage's occurrence identity — `documentId` and `ord` (Task 1) — and
/// its text, and this compares each against the chunk the id names now.
///
/// - **The text** is compared against `chunk.text` **exactly**. Not
///   `contains`, not trimmed, not normalised: the text on the wire is
///   `chunk.text` verbatim — `AskCitation::text` is `Hit::text` cloned when
///   `ask` builds each citation, which is `Citation::text`, which is
///   the `chunk.text` column (read back by `Db::citation`) — so any loosening
///   only widens what a reused id can pass as. A `contains` comparison would
///   accept `""` and match every chunk in the index.
/// - **The identity** — `document_id` and `ord` — catches what byte-identical
///   text cannot: `chunk.id` is reused across documents just as readily as
///   within one (`schema.sql:149`, no `AUTOINCREMENT`), and two documents
///   whose middle paragraph happens to be identical make the text pin alone
///   powerless. `ord` is the other half — the same paragraph repeated *inside*
///   one document reuses `document_id` but not `ord`
///   (`UNIQUE(document_id, ord)`, `schema.sql:168`). Neither pin is redundant
///   with the other: identity says *which* chunk this claims to be, the text
///   says its content did not change under a re-index that kept the same
///   `(document_id, ord)`.
///
/// **Why it cannot return a finished answer.** `Excerpt` carries a
/// `freshness`, and `Current`/`FileChanged`/`FileMissing` are undecidable
/// without a `stat` — which must not happen in here: this whole function runs
/// inside [`mnema_index::Db::read_snapshot`], holding a deferred read
/// transaction, inside `with_index`, holding the state mutex. A `stat` on a
/// sleeping network drive inside both would block every other command on a
/// filesystem round-trip. So the ingredients come back and the command
/// decides.
#[allow(clippy::too_many_arguments)]
fn build_source_around(
    db: &Db,
    chunk_id: i64,
    passage_text: &str,
    cited_document_id: &str,
    cited_ord: i64,
    cited_root_id: Option<i64>,
    cited_relative_path: Option<&str>,
    radius: i64,
) -> Result<Composed, mnema_index::Error> {
    let Some(anchor) = db.chunk_anchor(chunk_id)? else {
        return Ok(Composed::Settled(SourceAround::Gone {
            reason: GoneReason::NoSuchChunk,
        }));
    };
    if anchor.text != passage_text {
        return Ok(Composed::Settled(SourceAround::Gone {
            reason: GoneReason::IdReused,
        }));
    }
    if anchor.document_id != cited_document_id || anchor.ord != cited_ord {
        return Ok(Composed::Settled(SourceAround::Gone {
            reason: GoneReason::IdReused,
        }));
    }

    let window = db.reading_window(
        &anchor.document_id,
        anchor.page_no,
        anchor.first_reading_order,
        anchor.last_reading_order,
        radius,
    )?;
    let occupant = cited_occupant(db, cited_root_id, cited_relative_path)?;

    Ok(Composed::Pending {
        excerpt: ExcerptFields {
            blocks: window
                .blocks
                .into_iter()
                .map(|b| SourceBlock {
                    block_id: b.block_id,
                    kind: b.kind,
                    text: b.text,
                    page_no: b.page_no,
                    reading_order: b.reading_order,
                })
                .collect(),
            spans: anchor.spans.into_iter().map(WireSegment::from).collect(),
            document_id: anchor.document_id,
            section_title: anchor.section_title,
            has_more_before: window.has_more_before,
            has_more_after: window.has_more_after,
        },
        occupant,
    })
}

/// How far the indexed text has drifted from the file it was read out of.
///
/// The only filesystem call `source_around` makes, and it is here rather than
/// in the composer for the reason given there. `mnema_walk::stat`
/// (`mnema-walk/src/lib.rs:371`) rather than a hand-rolled `SystemTime`
/// conversion: `mtime` is **nanoseconds**, and the one place that number is
/// derived is the one place it can be got wrong.
fn decide_freshness(occupant: Option<&PathOccupant>, document_id: &str) -> Freshness {
    let Some(occupant) = occupant else {
        return Freshness::NoPath;
    };
    // Before the filesystem, because the index already knows the answer here
    // and it is a different answer. A walk that has re-indexed this location
    // has repointed the row at a new document; the file on disk then matches
    // *that* document's recorded numbers perfectly, and a `stat` would report
    // `Current` for a passage whose cited copy has already been replaced.
    if occupant.current_document_id != document_id {
        return Freshness::Reindexed;
    }
    let full = std::path::Path::new(&occupant.root_absolute_path).join(&occupant.relative_path);
    let Some(disk) = mnema_walk::stat(&full) else {
        return Freshness::FileMissing;
    };
    // The cheap arm's own comparison, negated, not a new one:
    // `recorded.size_bytes == disk.size_bytes && recorded.mtime == disk.mtime`
    // is two of the five conditions `mnema-ingest` already asks
    // (`mnema-ingest/src/lib.rs:295-296`), and the negated form is what
    // `displaces()` uses (`:1196`). Both halves, because an edit that keeps the
    // length moves only the mtime — dropping either operand loses a whole
    // class of edit silently, which is why the two are separate tests.
    if disk.size_bytes != occupant.size_bytes || disk.mtime != occupant.mtime {
        return Freshness::FileChanged;
    }
    Freshness::Current
}

/// The paragraphs around a cited passage, read out of the index — never off
/// the disk (spec §12), which the webview could not read anyway under
/// `default-src 'self'`.
///
/// Off the main thread for the reason given on [`crate::bridge::open_index`],
/// and every **index** read inside one
/// [`mnema_index::Db::read_snapshot`] — but the `stat` deliberately outside
/// it. The snapshot holds a deferred read transaction for its whole body and
/// `with_index` holds the state mutex for the whole call; a filesystem
/// round-trip inside both would block every other command on a volume that
/// happens to be asleep. So the closure returns the ingredients and the
/// verdict is decided out here.
///
/// `cited_document_id`/`cited_ord` are mandatory — the occurrence identity
/// `Hit`/`AskCitation` mint for every citation since Task 1 — and are compared
/// exactly, never defaulted or skipped when absent: a client that could omit
/// them would turn the pin off for itself, silently. `cited_root_id` is the
/// odd one out and stays `Option`: it feeds `Freshness` only (via
/// `cited_occupant`), never the refusal above, because it is legitimately
/// absent whenever the chunk's document sits under zero or several distinct
/// watched roots (`Citation::root_id`, `write.rs:95-106`) — a citation for
/// such a document can still ask and still get a verdict, a degraded one,
/// through the fallback `cited_occupant` keeps for exactly that case.
///
/// Eight parameters mirrors the wire contract (§10) one field at a time —
/// splitting them into a struct would be a seam no caller needs yet. **There
/// is no caller yet**: `ui/src/` carries no `sourceAround`
/// (`grep -rn "sourceAround" ui/src/` is empty) — PR 6b adds `ipc.ts`'s
/// wrapper, and it is what must send the three parameters this task added
/// (`citedDocumentId`, `citedOrd`, `citedRootId`), not something already
/// relying on them today.
#[allow(clippy::too_many_arguments)]
#[tauri::command(async)]
pub fn source_around(
    state: State<'_, AppState>,
    chunk_id: i64,
    passage_text: String,
    cited_document_id: String,
    cited_ord: i64,
    cited_root_id: Option<i64>,
    cited_relative_path: Option<String>,
    radius: u32,
) -> Result<SourceAround, Error> {
    let radius = radius.clamp(1, MAX_RADIUS) as i64;
    let composed = state.with_index(|db| {
        db.read_snapshot(|db| {
            build_source_around(
                db,
                chunk_id,
                &passage_text,
                &cited_document_id,
                cited_ord,
                cited_root_id,
                cited_relative_path.as_deref(),
                radius,
            )
        })
    })?;
    Ok(match composed {
        Composed::Settled(answer) => answer,
        Composed::Pending { excerpt, occupant } => {
            let freshness = decide_freshness(occupant.as_ref(), &excerpt.document_id);
            excerpt.with(freshness)
        }
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

    /// A `Segment` as the index stores one: four values, all different from
    /// each other, so a mirror that dropped or permuted a field cannot pass by
    /// coincidence.
    fn sample_segment() -> Segment {
        Segment {
            block_id: 41,
            start: 3,
            end: 16,
            block_start: 17,
        }
    }

    /// One `Excerpt` carrying every camelCase field this PR puts on the wire.
    ///
    /// ⚠️ `spans` is built with `.into()` rather than `WireSegment::from(…)`
    /// **on purpose**, and it is what makes Step 3's red-proof possible: the
    /// reflexive `impl From<T> for T` means this line still compiles if
    /// `spans` is changed to a bare `Vec<Segment>` — the mutation "delete the
    /// conversion and send `Segment` straight" — so that mutation shows up as
    /// a failing assertion about `block_start`, not as a compile error the
    /// harness would report as a broken case.
    fn sample_excerpt() -> SourceAround {
        SourceAround::Excerpt {
            blocks: vec![SourceBlock {
                block_id: 41,
                kind: "paragraph".into(),
                text: "Ціна оцифрування одного аркуша становить дві гривні.".into(),
                page_no: 3,
                reading_order: 2,
            }],
            spans: vec![sample_segment().into()],
            document_id: "d".repeat(64),
            section_title: Some("Розділ перший".into()),
            has_more_before: true,
            has_more_after: false,
            freshness: Freshness::FileChanged,
        }
    }

    /// The wire contract PR 6 matches on (§10), asserted in both directions for
    /// each of the three shapes this PR adds.
    ///
    /// The three are not one rule. `SourceAround` is an **enum**, where
    /// `rename_all` renames the variants only and `rename_all_fields` is what
    /// renames the fields of a struct variant — the trap this plan measured
    /// rather than reasoned from the `AskAnswer` precedent, whose struct
    /// variants are all one-word fields. `SourceBlock` and `WireSegment` are
    /// plain **structs**, where `rename_all` does rename fields. And `Segment`
    /// itself carries no rename at all, which is why `WireSegment` exists.
    #[test]
    fn source_wire_shape_is_camel_case() {
        let v = serde_json::to_value(sample_excerpt()).unwrap();

        // The variant tag, which is what a caller switches on.
        assert_eq!(v["kind"], "excerpt");

        // Present, camelCase — the struct-variant fields `rename_all` alone
        // would have left in snake_case.
        assert!(v["documentId"].is_string());
        assert!(v["sectionTitle"].is_string());
        assert!(v["hasMoreBefore"].is_boolean());
        assert!(v["hasMoreAfter"].is_boolean());
        // Absent, the other direction: a payload carrying both spellings would
        // satisfy the six assertions above while PR 6 read the wrong one.
        assert!(v.get("document_id").is_none());
        assert!(v.get("section_title").is_none());
        assert!(v.get("has_more_before").is_none());
        assert!(v.get("has_more_after").is_none());

        // ⚠️ `relativePath` is **not** a field of `Excerpt` and must not become
        // one: no read method can produce a path for the excerpt (`ChunkAnchor`
        // carries none, and `PathOccupant::relative_path` is the caller's own
        // query key), so it could only ever echo the input back and disagree
        // with nothing. Asserted in both spellings, because reinstating it in
        // either would be the regression.
        assert!(v.get("relativePath").is_none());
        assert!(v.get("relative_path").is_none());

        // `SourceBlock`.
        let block = &v["blocks"][0];
        assert!(block["blockId"].is_i64());
        assert!(block["pageNo"].is_i64());
        assert!(block["readingOrder"].is_i64());
        assert!(block["kind"].is_string());
        assert!(block["text"].is_string());
        assert!(block.get("block_id").is_none());
        assert!(block.get("page_no").is_none());
        assert!(block.get("reading_order").is_none());

        // `WireSegment`.
        let span = &v["spans"][0];
        assert!(span["blockId"].is_i64());
        assert!(span["blockStart"].is_u64());
        assert!(span.get("block_id").is_none());
        assert!(span.get("block_start").is_none());

        // The refusal variant and its cause, both tags.
        let gone = serde_json::to_value(SourceAround::Gone {
            reason: GoneReason::IdReused,
        })
        .unwrap();
        assert_eq!(gone["kind"], "gone");
        assert_eq!(gone["reason"]["kind"], "idReused");
        assert!(gone.get("blocks").is_none());
        let missing = serde_json::to_value(SourceAround::Gone {
            reason: GoneReason::NoSuchChunk,
        })
        .unwrap();
        assert_eq!(missing["reason"]["kind"], "noSuchChunk");

        // Every `Freshness` tag, because the set is closed (the outcomes of two
        // comparisons) and a card must render each: a default branch drawing
        // `Current` for a tag it does not recognise is how a stale excerpt gets
        // shown as fresh.
        for (variant, tag) in [
            (Freshness::Current, "current"),
            (Freshness::Reindexed, "reindexed"),
            (Freshness::FileChanged, "fileChanged"),
            (Freshness::FileMissing, "fileMissing"),
            (Freshness::NoPath, "noPath"),
        ] {
            let f = serde_json::to_value(&variant).unwrap();
            assert_eq!(f["kind"], tag, "{variant:?} crossed as {f}");
        }
        assert_eq!(v["freshness"]["kind"], "fileChanged");

        // A missing section title crosses as `null`, **present**, not as an
        // absent key. The distinction is this crate's standing one — `Citation`
        // makes the argument for `relative_path`, and PR 4's `list_tree` test
        // pins it the same way: "we do not know" and "the value is empty" must
        // not render as the same thing. Measured, not assumed: marking the
        // field `skip_serializing_if = "Option::is_none"` passed every other
        // assertion in this file.
        let SourceAround::Excerpt {
            blocks,
            spans,
            document_id,
            has_more_before,
            has_more_after,
            freshness,
            ..
        } = sample_excerpt()
        else {
            unreachable!("sample_excerpt is an Excerpt")
        };
        let untitled = serde_json::to_value(SourceAround::Excerpt {
            blocks,
            spans,
            document_id,
            section_title: None,
            has_more_before,
            has_more_after,
            freshness,
        })
        .unwrap();
        assert!(
            untitled.get("sectionTitle").is_some(),
            "a page with no section title must still carry the key: {untitled}"
        );
        assert!(untitled["sectionTitle"].is_null(), "{untitled}");
    }

    /// The `WireSegment` conversion is load-bearing, so this proves it rather
    /// than assuming it.
    ///
    /// [`mnema_core::Segment`] is **persisted** — `chunk.char_span` stores a
    /// `Vec<Segment>` as JSON and the schema's `CHECK` and its
    /// `chunk_span_blocks_bi` trigger both read that JSON *by key* — so the
    /// type cannot be given a `rename_all` and crosses as `block_start`.
    /// Sending it straight is the mutation this test is written against: delete
    /// the conversion, make `spans` a `Vec<Segment>`, and the two assertions
    /// below go red while everything else still compiles.
    ///
    /// Both directions: the mirror must be camelCase **and** must carry the
    /// same four values, in the same roles — a conversion that swapped `start`
    /// for `block_start` would ship a perfectly camelCase, perfectly wrong
    /// highlight.
    #[test]
    fn a_real_segment_crosses_the_wire_through_its_camel_case_mirror() {
        let segment = sample_segment();
        let v = serde_json::to_value(sample_excerpt()).unwrap();
        let span = &v["spans"][0];

        assert_eq!(span["blockId"], segment.block_id);
        assert_eq!(span["start"], segment.start);
        assert_eq!(span["end"], segment.end);
        assert_eq!(span["blockStart"], segment.block_start);

        assert!(span.get("block_start").is_none());
        assert!(span.get("block_id").is_none());
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

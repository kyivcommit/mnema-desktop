//! Read-only reads for the launcher's two side cards: the left card's watched
//! folders, their indexed files, and recently-indexed documents (§7,
//! [`list_tree`]); the right card's paragraphs around a cited passage (§7.1,
//! [`source_around`]). Read-only, so neither command adds a data-loss surface
//! (spec §12) — they only reflect the index at query time, and `source_around`
//! refuses rather than answer with text a rebuild has since replaced.
//!
//! And, since PR 8a, the exclusion screen's own folder tree
//! ([`list_subfolders`]) — the one read in this file that goes to the **disk**
//! rather than to the index, for the reason its own doc comment gives. Still
//! read-only: it writes nothing and refuses rather than answer short.

use crate::error::Error;
use crate::state::AppState;
use mnema_core::Segment;
use mnema_index::{Db, PathOccupant};
use mnema_walk::WalkRules;
use serde::Serialize;
use std::path::{Path, PathBuf};
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

/// One level of a watched folder's tree as the exclusion screen needs it.
///
/// **A struct rather than a bare `Vec`, because of `unnameable`.** Entries
/// whose names are not valid UTF-8 are omitted rather than rendered lossily
/// (see [`read_subfolders`]), and dropping them silently would make a folder
/// look emptier than it is — so the count travels with the list, where the
/// window can say the folder holds entries it cannot name.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubfolderListing {
    pub entries: Vec<Subfolder>,
    /// How many directory entries were omitted because their names are not
    /// valid UTF-8. Counted, never named.
    pub unnameable: u64,
}

/// One subfolder, and what holds it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subfolder {
    /// The final component, exactly as the filesystem spells it.
    pub name: String,
    /// What to send back to `exclude_subfolder` — the path relative to the
    /// watched root, built by joining the caller's own `relative_path` with
    /// [`Subfolder::name`]. Never normalised: a prefix is a path the user
    /// chose from disk, not a pattern (`rules.rs:315-332`).
    pub relative_path: String,
    pub state: SubfolderState,
}

/// What holds a subfolder, as one tagged state rather than three booleans.
///
/// ⚠️ **`Open` does not mean "will be indexed".** It means *no rule this
/// command can see* holds the folder. The third rule layer is the in-tree
/// `.gitignore` stack (§5), and deciding it means compiling the same ignore
/// stack the walk builds, per directory — which this read-only listing does
/// not do. A folder shown as `Open` may still be skipped by a `.gitignore`
/// inside the tree, so a window's wording here must be about the rules this
/// screen can see, not about what the walk will do.
///
/// **Precedence, in the order [`subfolder_state`] applies it**, and every
/// step of it is the same rule: *never show a control that does nothing.*
///
/// 1. `BuiltIn` — the name is on `WalkRules::BUILTIN_DIRS`. Pruned by the
///    built-in layer whatever any user rule says (`rules.rs:283-290`), so an
///    `Excluded` badge with a working "include" toggle would announce one
///    thing and do another.
/// 2. `Symlink` — the walk runs `follow_links(false)` (`rules.rs:229`), so
///    nothing under this name is ever enumerated; a rule naming it excludes
///    nothing.
/// 3. `ExcludedByAncestor` — a stored rule names an ancestor. Checked
///    **before** `Excluded`, so a folder that has both its own rule and an
///    excluded ancestor names the ancestor: removing its own rule would leave
///    the ancestor's, and the row would not open. Its own rule is not hidden
///    from the person — `list_exclusions` lists every stored prefix.
/// 4. `Excluded` — a stored rule names this folder itself, and nothing above
///    it. The one state whose control does what it says.
/// 5. `Open` — none of the above.
///
/// ⚠️ `rename_all_fields`, for the reason [`SourceAround`] measured: on an
/// *enum*, `rename_all` renames the variants only, and the fields inside a
/// struct variant keep their snake_case names without this second attribute.
/// `prefix` is one word, so today it changes nothing — which is exactly how a
/// two-word field added later would ship snake_case inside a camelCase
/// payload with every test still green.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum SubfolderState {
    /// No rule this command can see. **Not** a promise that the folder will
    /// be indexed — see this type's own doc comment.
    Open,
    /// A stored prefix names this folder itself, and no ancestor of it.
    Excluded,
    /// A stored prefix names an ancestor. Carries **which** one, because a
    /// row that says "held by a rule" without naming it leaves the person
    /// nothing to go and remove.
    ///
    /// The **outermost** ancestor when several hold it. `list_path_exclusions`
    /// sorts (`write.rs:576`), and the ancestors of one path form a chain in
    /// which each is a string prefix of the next, so the first match in that
    /// sorted list is the shallowest one — the rule whose removal has to come
    /// first.
    ExcludedByAncestor { prefix: String },
    /// `WalkRules::BUILTIN_DIRS` names it (`rules.rs:134-147`), so the walk
    /// prunes it regardless of any user rule.
    BuiltIn,
    /// A symlink to a directory. Listed, so the folder does not look emptier
    /// than it is, but never as an ordinary excludable folder: the walk runs
    /// `follow_links(false)` (`rules.rs:229`), so nothing under it is indexed
    /// and an exclusion rule naming it would exclude nothing.
    Symlink,
}

/// Whether `prefix` is a **strict ancestor** of `path`.
///
/// Byte-exact and boundary-aware in one condition, and both halves are
/// load-bearing: `path.starts_with(prefix)` alone makes `ab` an ancestor of
/// `abc/d`, and the `/` test alone would need the length check anyway. Strict,
/// so a prefix equal to the path is not its own ancestor — that is
/// [`SubfolderState::Excluded`], one state down.
///
/// No normalisation of either side, deliberately: a prefix is a path the user
/// chose from disk (`rules.rs:315-332`), and the walk's own matcher compares
/// it byte for byte.
fn is_ancestor_of(prefix: &str, path: &str) -> bool {
    path.len() > prefix.len() && path.as_bytes()[prefix.len()] == b'/' && path.starts_with(prefix)
}

/// What holds one entry. The precedence is [`SubfolderState`]'s own, and the
/// argument for each step is there rather than repeated here.
fn subfolder_state(
    name: &str,
    relative_path: &str,
    is_symlink: bool,
    prefixes: &[String],
) -> SubfolderState {
    if WalkRules::BUILTIN_DIRS.contains(&name) {
        return SubfolderState::BuiltIn;
    }
    if is_symlink {
        return SubfolderState::Symlink;
    }
    if let Some(prefix) = prefixes
        .iter()
        .find(|prefix| is_ancestor_of(prefix, relative_path))
    {
        return SubfolderState::ExcludedByAncestor {
            prefix: prefix.clone(),
        };
    }
    if prefixes.iter().any(|prefix| prefix == relative_path) {
        return SubfolderState::Excluded;
    }
    SubfolderState::Open
}

/// One filesystem error about a **path**, split by
/// [`crate::bridge::path_error_is_an_answer`]: an error that says the path is
/// not there in the shape asked for becomes [`Error::NoSuchSubfolder`], and
/// everything else — a permission this process lacks, a share that dropped —
/// becomes [`Error::SubfolderUnreadable`], because it says something about the
/// observer and nothing about the folder.
///
/// Both sides refuse the call. The classifier is still what decides between
/// them, because the two sentences are different claims and a window will act
/// on them differently: "there is no such folder" invites removing a stale
/// rule, and under D29 acting on that when the folder is merely unreachable
/// sends its contents to the provider on the next scan.
fn refusal(root_id: i64, relative_path: &str, source: std::io::Error) -> Error {
    if crate::bridge::path_error_is_an_answer(source.kind()) {
        Error::NoSuchSubfolder {
            root_id,
            relative_path: relative_path.to_string(),
        }
    } else {
        Error::SubfolderUnreadable {
            root_id,
            relative_path: relative_path.to_string(),
            source,
        }
    }
}

/// The directory `relative_path` names under the watched root, or a refusal.
///
/// **Containment is decided on the canonicalised path, and it is two
/// conditions, neither of which the other covers.** Each catches a shape the
/// other lets straight through, which is why they are two tests and two tests:
///
/// - `resolved != expected` — the path must resolve to exactly where its own
///   spelling says it is. This is what catches `..` (which canonicalises to
///   somewhere the literal join does not name) and every path reaching its
///   target **through a symlink**, in or out of the root. A lexical check
///   cannot see a symlink at all: the string `in/inner` looks like an ordinary
///   relative path and resolves wherever the link points.
/// - `resolved.starts_with(root_canonical)` — the path must be under the root.
///   This is what catches an **absolute** `relative_path`, which the first
///   condition cannot: `Path::join` given an absolute path throws the root
///   away and answers the absolute path itself, so `expected` and `resolved`
///   agree perfectly and both are outside the watched folder.
///
/// The root is canonicalised by the caller, once, so that a watched folder
/// reached through a symlink (`/var` → `/private/var` on macOS, where every
/// temporary directory lives) is compared against its own real path rather
/// than refusing everything.
///
/// 🔴 **The spelling half decides the CASE of the path too, and that is the
/// reason to prefer it over `starts_with` alone.** `std::fs::canonicalize`
/// corrects case on a case-insensitive filesystem — measured on macOS 26.6.2 /
/// APFS: `canonicalize(root/"PRIVATE")` answers `…/Private` for a folder
/// actually named `Private` — so `resolved != expected` refuses a wrong-case
/// spelling that `stat` would happily have opened. It has to be refused:
/// `ignore`'s override matcher, which is what the walk applies, is
/// case-sensitive, so every `relativePath` this command built under a wrong-case
/// ancestor would save a rule that excludes nothing. That is the same defect
/// `bridge::prefix_exists_on_disk` (`bridge.rs:186`) resolves components
/// byte-exactly to avoid, one command over, and the reason
/// `a_wrong_case_relative_path_is_refused` is a test rather than a note.
///
/// ⚠️ The bound on that: it is `canonicalize`'s behaviour, not this function's.
/// A case-insensitive volume whose `realpath` did **not** correct case would let
/// the spelling through — and even then nothing silently over-shares, because
/// the rule such a path saves is dead, and a dead rule is exactly what
/// `list_exclusions` reports as `existsOnDisk: false` for the person to remove.
fn subfolder_dir(
    root_canonical: &Path,
    root_id: i64,
    relative_path: &str,
) -> Result<PathBuf, Error> {
    if relative_path.is_empty() {
        return Ok(root_canonical.to_path_buf());
    }
    let expected = root_canonical.join(relative_path);
    let resolved =
        std::fs::canonicalize(&expected).map_err(|e| refusal(root_id, relative_path, e))?;
    if resolved != expected || !resolved.starts_with(root_canonical) {
        return Err(Error::SubfolderNotUnderRoot {
            root_id,
            relative_path: relative_path.to_string(),
        });
    }
    Ok(resolved)
}

/// One directory entry, reduced to the three things the listing decides on.
///
/// **The reduction is what makes [`read_subfolders`] testable at all**, and
/// that is its whole purpose. A `std::fs::DirEntry` cannot be constructed by
/// hand, so a listing written directly against `ReadDir` can only be exercised
/// through a real directory — and the state that matters most here, a name
/// that is not valid UTF-8, **cannot be put on a real directory on APFS**:
/// `create_dir` refuses it outright with `EILSEQ`, "Illegal byte sequence"
/// (measured on macOS 26.6.2 / APFS, first on the fixture the brief asked for
/// and again on a standalone probe). A guard whose only test needs a filesystem
/// that cannot host it is a guard half this project's machines never check.
struct Entry {
    name: std::ffi::OsString,
    /// Whether the entry is a directory **after following**, which a
    /// `DirEntry`'s own `file_type` does not say for a symlink.
    is_dir: bool,
    /// Whether the entry itself is a symlink, which the follow above erases.
    is_symlink: bool,
}

/// One directory's entries, reduced to [`Entry`], or the first filesystem
/// error that stopped the reading.
///
/// **It takes the iterator rather than the directory path**, the same seam
/// `bridge::entry_named` takes and for the same reason: `ReadDir` yields
/// `io::Result<DirEntry>`, a per-entry error cannot be forced deterministically
/// out of a real filesystem, and `.flatten()` on it would drop every `Err`
/// silently — leaving a folder that looks like it holds fewer subfolders than
/// it does.
///
/// **Two of the three filesystem outcomes below are `?`, and those two are the
/// sites in this crate that deliberately do NOT consult
/// [`crate::bridge::path_error_is_an_answer`]** — the per-entry `io::Result` and
/// the per-entry `file_type`. The classifier exists to keep an error about the
/// observer from being read as an answer about a path, and there is no answer to
/// be read at either: the alternative to refusing is not a boolean but a shorter
/// list, which is a claim about the folder's contents that no error kind
/// supports. The same argument `list_exclusions`'s root guard makes one command
/// over.
///
/// **The third does consult it**, because there it really is a boolean: a
/// symlink's own `file_type` says `symlink` and nothing about what it points at,
/// so a `metadata` through it decides "is this a directory". A dangling link and
/// a link to a file are answers — not a directory, not listed. Anything else is
/// the observer's condition and refuses.
fn directory_entries(
    entries: impl Iterator<Item = std::io::Result<std::fs::DirEntry>>,
) -> std::io::Result<Vec<Entry>> {
    let mut reduced = Vec::new();
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let is_symlink = file_type.is_symlink();
        let is_dir = if is_symlink {
            match std::fs::metadata(entry.path()) {
                Ok(metadata) => metadata.is_dir(),
                Err(e) if crate::bridge::path_error_is_an_answer(e.kind()) => false,
                Err(e) => return Err(e),
            }
        } else {
            file_type.is_dir()
        };
        reduced.push(Entry {
            name: entry.file_name(),
            is_dir,
            is_symlink,
        });
    }
    Ok(reduced)
}

/// The directories among `entries`, sorted by name, plus how many of them had
/// no name this wire type can carry.
///
/// Sorted by name for the reason the walk sorts (`rules.rs:228`): `read_dir`
/// order is the filesystem's, and a window that redraws in a different order on
/// another machine is a window nobody can point at over the phone.
///
/// Only directories are counted in `unnameable`, because only directories would
/// have been listed: an unnameable *file* is no more missing from this answer
/// than a nameable one.
fn read_subfolders(entries: &[Entry], parent: &str, prefixes: &[String]) -> SubfolderListing {
    let mut listed: Vec<Subfolder> = Vec::new();
    let mut unnameable: u64 = 0;
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        // Omitted and counted, never `to_string_lossy`: a lossy name no longer
        // opens the folder it came from, and a rule saved from one would
        // exclude nothing. The walk refuses such entries for the same reason
        // (`PreSkipRule::UnrepresentableName`, `mnema-walk/src/lib.rs:52-54`).
        let Some(name) = entry.name.to_str() else {
            unnameable += 1;
            continue;
        };
        let relative_path = if parent.is_empty() {
            name.to_string()
        } else {
            format!("{parent}/{name}")
        };
        let state = subfolder_state(name, &relative_path, entry.is_symlink, prefixes);
        listed.push(Subfolder {
            name: name.to_string(),
            relative_path,
            state,
        });
    }
    listed.sort_by(|a, b| a.name.cmp(&b.name));
    SubfolderListing {
        entries: listed,
        unnameable,
    }
}

/// One level of a watched folder's own tree, read off the **disk**.
///
/// Off the main thread for the reason given on [`crate::bridge::open_index`].
///
/// **Why not [`list_tree`].** That command reports indexed *documents*
/// (`d.status = 'indexed'`, `write.rs:242-249`), so a folder just added and
/// never scanned lists nothing — and the exclusion step comes *before* the
/// first scan (D136). The tree the person excludes from therefore has to come
/// from the directory itself. Read-only, one level per call, so an expand
/// costs one `read_dir`.
///
/// **Nothing here answers short.** Every failure is a refusal with a sentence:
/// an unknown id, a root that is not there, a path that leaves the root, a
/// folder that is not there, a folder that cannot be read. `entries: []` is
/// reserved for its one true meaning — this folder really holds no subfolders
/// — because a window draws anything else as a tree with nothing left to
/// exclude, and under D29 a folder nobody excludes is a folder whose text goes
/// to a third-party provider.
#[tauri::command(async)]
pub fn list_subfolders(
    state: State<'_, AppState>,
    root_id: i64,
    relative_path: String,
) -> Result<SubfolderListing, Error> {
    let root = state
        .with_index(|db| db.watched_root_path(root_id))?
        .ok_or(Error::UnknownWatchedRoot(root_id))?;
    let root_path = Path::new(&root);
    // The walk's own predicate for "is this root usable"
    // (`mnema-ingest/src/walk.rs:288`), matched here for the reason
    // `bridge::list_exclusions` gives at length: an unmounted drive would
    // otherwise answer "this folder has no subfolders" for every level of it.
    if !root_path.is_dir() {
        return Err(Error::RootUnavailable(root_id));
    }
    let root_canonical = root_path
        .canonicalize()
        .map_err(|_| Error::RootUnavailable(root_id))?;
    let dir = subfolder_dir(&root_canonical, root_id, &relative_path)?;
    let prefixes = state.with_index(|db| db.list_path_exclusions(root_id))?;
    let read = std::fs::read_dir(&dir).map_err(|e| refusal(root_id, &relative_path, e))?;
    let entries = directory_entries(read).map_err(|source| Error::SubfolderUnreadable {
        root_id,
        relative_path: relative_path.clone(),
        source,
    })?;
    Ok(read_subfolders(&entries, &relative_path, &prefixes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One [`Entry`] as `read_dir` would have produced it, from raw bytes.
    ///
    /// `#[cfg(unix)]` because it is the byte-level `OsString` construction that
    /// is platform-specific, not the property: `read_subfolders` decides on
    /// `OsStr::to_str` on every platform, and Windows reaches the same branch
    /// through an unpaired surrogate. This is the seam that lets the
    /// unnameable-name branch be tested on a machine whose own filesystem
    /// refuses to hold such a name — see [`Entry`].
    #[cfg(unix)]
    fn entry(name: &[u8], is_dir: bool, is_symlink: bool) -> Entry {
        use std::os::unix::ffi::OsStrExt;
        Entry {
            name: std::ffi::OsStr::from_bytes(name).to_os_string(),
            is_dir,
            is_symlink,
        }
    }

    /// A directory whose name is not valid UTF-8 is omitted from the entries
    /// and counted in `unnameable` — never rendered lossily, which would put a
    /// name on screen that no longer opens the folder and would save a rule
    /// that excludes nothing.
    ///
    /// **Two of them, and one unnameable FILE beside them.** `unnameable` is a
    /// count: a field answering `1` unconditionally passes any assertion a
    /// single bad entry can make, and a count that included files would answer
    /// `3` for a listing that is missing only two folders.
    #[cfg(unix)]
    #[test]
    fn a_directory_whose_name_is_not_utf8_is_counted_and_never_named() {
        let entries = vec![
            entry(b"beta", true, false),
            entry(b"bad\xffone", true, false),
            entry(b"alpha", true, false),
            entry(b"bad\xfetwo", true, false),
            entry(b"bad\xfdfile.txt", false, false),
        ];

        let listing = read_subfolders(&entries, "", &[]);

        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["alpha", "beta"],
            "only the two nameable folders may be listed, sorted: {listing:?}"
        );
        assert!(
            !names.iter().any(|name| name.contains('\u{FFFD}')),
            "a name was rendered lossily instead of being omitted: {listing:?}"
        );
        assert_eq!(
            listing.unnameable, 2,
            "both unnameable FOLDERS are counted and the unnameable file is not: {listing:?}"
        );
    }

    /// One listing carrying every state this PR puts on the wire.
    fn sample_subfolders() -> SubfolderListing {
        let row = |name: &str, state| Subfolder {
            name: name.to_string(),
            relative_path: format!("Work/{name}"),
            state,
        };
        SubfolderListing {
            entries: vec![
                row("open", SubfolderState::Open),
                row("excluded", SubfolderState::Excluded),
                row(
                    "held",
                    SubfolderState::ExcludedByAncestor {
                        prefix: "Work".to_string(),
                    },
                ),
                row("node_modules", SubfolderState::BuiltIn),
                row("link", SubfolderState::Symlink),
            ],
            unnameable: 2,
        }
    }

    /// The wire contract task 5's UI matches on, both directions.
    ///
    /// `SubfolderState` is an **enum**, so `rename_all` renames its variants
    /// and `rename_all_fields` is what would rename a struct variant's fields —
    /// the trap `SourceAround` measured one type up. `prefix` is one word, so
    /// the second attribute changes nothing today; the tags are what a caller
    /// switches on, and every one of the five is asserted because a card with a
    /// default branch is how an unrecognised state gets drawn as an ordinary
    /// excludable folder.
    #[test]
    fn the_subfolder_wire_shape_is_camel_case() {
        let v = serde_json::to_value(sample_subfolders()).unwrap();

        assert_eq!(v["unnameable"], 2, "{v}");
        let rows = v["entries"].as_array().expect("entries is an array");

        // Present, camelCase — and absent in snake_case, which is the direction
        // that catches a payload carrying both spellings.
        assert!(rows[0]["relativePath"].is_string(), "{v}");
        assert!(rows[0]["name"].is_string(), "{v}");
        assert!(rows[0].get("relative_path").is_none(), "{v}");

        for (row, tag) in rows.iter().zip([
            "open",
            "excluded",
            "excludedByAncestor",
            "builtIn",
            "symlink",
        ]) {
            assert_eq!(row["state"]["kind"], tag, "{row}");
        }

        // The prefix the row promises to name, and the value of it.
        assert_eq!(rows[2]["state"]["prefix"], "Work", "{v}");
        // A unit variant carries its tag and nothing else: a state that grew a
        // field nobody meant to ship would show up here.
        assert_eq!(
            rows[0]["state"]
                .as_object()
                .expect("a state is an object")
                .len(),
            1,
            "{v}"
        );
    }

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

//! Read-only reads for the launcher's two side cards: the tree's watched
//! folders and their indexed files (§7, [`list_tree`]); the file pane's whole
//! document (§7.1, [`open_document`]), with the cited passage highlighted.
//! Read-only, so neither command adds a data-loss surface (spec §12) — they
//! only reflect the index at query time, and `open_document` refuses rather
//! than answer with text a rebuild has since replaced.
//!
//! And, since PR 8a, the exclusion screen's own folder tree
//! ([`list_subfolders`]) — the one read in this file that goes to the **disk**
//! rather than to the index, for the reason its own doc comment gives. Still
//! read-only: it writes nothing and refuses rather than answer short.

use crate::error::Error;
use crate::state::AppState;
use mnema_core::Segment;
use mnema_index::{Db, DocumentStatus, PathOccupant};
use mnema_walk::{BuiltinLayers, WalkRules};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeListing {
    pub roots: Vec<TreeRoot>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeRoot {
    pub root_id: i64,
    pub absolute_path: String,
    pub name: String,
    pub files: Vec<TreeFile>,
    /// The root's folder search entries, by folder path ascending; a folder
    /// with no entry follows its nearest entered ancestor (the spec's §3).
    pub searched: Vec<SearchedFolder>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchedFolder {
    pub folder_path: String,
    pub searched: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeFile {
    pub relative_path: String,
    pub document_id: String,
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
            let searched = db
                .search_folders(r.id)?
                .into_iter()
                .map(|(folder_path, searched)| SearchedFolder {
                    folder_path,
                    searched,
                })
                .collect();
            Ok(TreeRoot {
                name: basename(&r.absolute_path),
                root_id: r.id,
                absolute_path: r.absolute_path,
                files,
                searched,
            })
        })
        .collect::<Result<Vec<_>, mnema_index::Error>>()?;

    Ok(TreeListing { roots })
}

/// Off the main thread for the reason given on [`crate::bridge::open_index`].
///
/// The roots and every root's files are read inside one
/// [`mnema_index::Db::read_snapshot`], not as autocommit reads, so each root
/// and the files listed under it come from the same state of the index. The
/// indexing job commits on its own connection, outside this window's mutex.
/// `build_tree_listing` only reads, so it is a safe closure for `read_snapshot`.
#[tauri::command(async)]
pub fn list_tree(state: State<'_, AppState>) -> Result<TreeListing, Error> {
    state.with_index(|db| db.read_snapshot(build_tree_listing))
}

/// Turns one folder's search on or off for its whole subtree: the entries
/// below it are cleared and its own is written, in one transaction
/// (`Db::set_folder_searched`). Off the main thread, like [`list_tree`].
#[tauri::command(async)]
pub fn set_folder_searched(
    state: State<'_, AppState>,
    root_id: i64,
    folder_path: String,
    searched: bool,
) -> Result<(), Error> {
    state.with_index(|db| db.set_folder_searched(root_id, &folder_path, searched))
}

/// What a mask would take **beyond the rules the next scan already applies** —
/// the built-in layers, each root's stored path exclusions and every stored
/// mask, not the stored ones alone — as of now, in two numbers with
/// **different subjects** (D-d). [`mask_preview`] defines that set; the
/// built-in layers are in it and are not stored, which is why this sentence
/// does not say "already stored" (fix round 6).
///
/// `paths` and `documents` are not two ways of saying the same thing, and
/// conflating them overstates the loss. A document is destroyed only when its
/// **last** path goes (`forget_if_unnamed`, inside phase 3's transaction;
/// `deleting_one_copy_keeps_the_document` in `crates/mnema-ingest/tests/walk.rs`
/// pins it), so a mask that takes one of two copies of a file removes a path
/// and destroys nothing. Reporting that as a lost document is a claim a person
/// acts on — and an overstated disclosure is worse than none.
///
/// 🔴 **Both numbers are a difference between two rule sets, never a property
/// of one mask.** [`mask_preview`] defines the current set and the new set and
/// carries the reason; these field docs quote that definition and must not
/// outlive it. Fix round 4 is what it costs when they do: the definition moved
/// there and these four sentences stayed behind, still describing a number
/// that had stopped existing.
///
/// 🔴 **`documents == 0` is a statement about this rule, and the editor says
/// nothing about documents at all when it reads one** (fix round 7, owner's
/// ruling; `settings_masks_add_cost`'s `=0` arm is empty, and the sentence is
/// the file count and the two-way hedge). A zero means no document has its last
/// surviving path taken *by this rule*; it does **not** mean no document stops
/// being findable, and three rounds of trying to say the narrow version on
/// screen each shipped the wide one. Two mechanisms falsify it, and neither is
/// visible to this count:
///
/// - the in-tree `.gitignore` stack, outside both rule sets
///   ([`mnema_walk::WalkRules::applied_to`] names it as what `AppliedRules` does
///   not cover) while the walk applies it with no git repository present — so a
///   path it covers counts as SURVIVING and not taken, and the document is
///   suppressed. Measured rather than reasoned, in
///   `mask_preview_understates_documents_behind_an_in_tree_gitignore`
///   (`tests/commands.rs`): a two-path document, `documents: 0`, and a next scan
///   that leaves it with no path at all;
/// - **status.** `Db::indexed_files_under_root`
///   (`crates/mnema-index/src/write.rs`) selects `WHERE … d.status = 'indexed'`
///   while `document.status` defaults to `'pending'`
///   (`crates/mnema-index/src/schema.sql`) and `path` rows are written
///   independently, so a `pending`, `failed` or `skipped` document's only path
///   is outside this population entirely and the next walk removes it.
///
/// `mask_preview` is where the direction of the first residue is written down:
/// **larger for `paths`, smaller for `documents`**.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskPreview {
    /// Paths of `status = 'indexed'` DOCUMENTS that survive the current rule
    /// set and do **not** survive the new one, across **all** watched roots.
    ///
    /// Not "indexed paths": the population is `Db::indexed_files_under_root`
    /// (`crates/mnema-index/src/write.rs`, `WHERE … d.status = 'indexed'`), and
    /// status is a property of the document — a `pending` or `failed`
    /// document's paths are in the `path` table and are **not** here, while the
    /// walk's reconcile set does not ask about status at all. That is one of
    /// the two reasons the next scan can remove more than this number says, and
    /// the editor's sentence carries it. [`mask_preview`] holds the definition;
    /// this quotes it (fix round 6).
    pub paths: i64,
    /// The `documentId`s findable under the current rule set — at least one
    /// surviving path — that are **not** findable under the new one: the ones
    /// whose last surviving path this rule takes.
    ///
    /// "Findable" is meant against those two rule sets and **nothing wider**,
    /// which is the whole of the arm above: a document this number does not
    /// count can still stop being findable, through a mechanism neither set
    /// represents. It used to say "the ones that stop being findable at all"
    /// (fix round 6).
    pub documents: i64,
}

/// What adding `pattern` would remove **that your rules do not already
/// remove** — the difference this press makes, counted over the index.
/// Read-only.
///
/// 🔴 **A difference between two rule sets, never a property of one mask, and
/// this is the whole of what the command is for.** Until fix round 4 it built
/// `WalkRules::none().with_masks(vec![pattern])` and counted with that, while
/// `walk_job.rs:149` builds `WalkRules::new(true, true, user_prefixes)?
/// .with_masks(masks)?` — so the preview answered a question the next scan was
/// not going to be asked. The reproduction is ordinary rather than contrived:
/// `*.pdf` stored, no scan run yet, a document indexed at both `copy.pdf` and
/// `copy.txt`. Previewing `*.txt` answered `documents: 0`, and the editor said
/// *"no document stops being findable — each is also indexed under another
/// path"*; the next scan applied both masks and the document was gone. The
/// direction of the error is the worst available — it **understates** the loss,
/// on the one screen whose job is to state it.
///
/// So the two numbers are defined against two sets:
///
/// - the **current set** is what the next scan would apply today: the built-in
///   layers, each root's own stored path exclusions, and every stored mask;
/// - the **new set** is the current set plus the candidate;
/// - `paths` counts indexed paths that survive the current set and do not
///   survive the new one;
/// - `documents` counts documents findable under the current set — at least one
///   surviving path — that are not findable under the new one.
///
/// A path a stored rule already takes is therefore charged to that rule and not
/// to this press: it is going on the next scan whether this question is
/// confirmed or cancelled, and telling a person that cancelling saves it would
/// be a second false promise in the other direction.
///
/// 🔴 **Through [`mnema_walk::AppliedRules`], never a second matcher.** The
/// window has no glob library (`ui/package.json`), and a count derived there
/// would be a second implementation of the rule that disagrees with the walk at
/// exactly the edges Task 9 spent its cases pinning: the `.gitignore` parser
/// edges, the case folding, the normalisation form. This is that plan's review
/// round 1, P1 — and fix round 4 is what happens when a preview stands not on a
/// second copy of the rule but on a *subset* of it.
///
/// 🔴 **Counted over indexed relative paths, never over a disk listing.**
/// [`mnema_walk::MaskLayer::matches`] answers about a *name* and nothing about
/// the entry, while the walk asks `is_file() && matches(name)` — so on a
/// symlink, a FIFO, or an entry whose `file_type()` cannot be read, the
/// predicate says "removed" where the walk removes nothing. The skew is
/// one-directional: everything the walk takes, the predicate also calls taken,
/// never the reverse. Over `path` rows it is unreachable, because those are
/// regular files the walk already indexed; over a `read_dir` it is reachable,
/// and the preview would show a person more files than the next walk takes.
///
/// **It validates the candidate first, alone, before the index is touched** —
/// so previewing a malformed mask gives the same sentence saving it would, and
/// so that sentence is about the mask being typed rather than about some other
/// rule already in storage. Not a count of zero, which reads as "this rule
/// would remove nothing" and is the opposite of true for a rule that cannot be
/// stored at all. A **stored** rule that no longer validates is a different
/// answer and a deliberate one: it refuses here, exactly as it refuses the walk
/// (`walk_job.rs`), because a preview cannot honestly put a number on a scan
/// that is going to stop before it starts.
///
/// **The literal empty string is not malformed and previews as two zeros.** It
/// is `validate_mask`'s one non-error, meaning "no rule" — the blank row in an
/// editor before anything is typed — and zero is the honest answer for it.
/// [`crate::bridge::add_mask`] is where it is refused, because storing it is
/// the harm.
///
/// **Grouping, and why it is complete.** `documents` counts the ids for which
/// every surviving path is taken, over all roots at once — a mask is global, so
/// a document with a copy under a second watched folder must be seen whole.
/// `Db::indexed_files_under_root` returns the paths of `status = 'indexed'`
/// documents only, and status is a property of the *document*: either all of a
/// document's paths are in this set or none are. So a document that appears
/// here appears with **all** of its paths, and "every surviving path is taken"
/// is asked of the whole set rather than of a visible fragment of it.
///
/// ⚠️ **The lock is held across the READS and nothing else.** The rule sets are
/// compiled and the paths matched **after** `with_index` returns, so evaluating
/// four layers per path instead of one costs the mutex nothing — this holds it
/// for strictly less than the previous version did, which matched inside it.
/// The cost that moved is memory: every root's indexed paths are in hand at
/// once rather than one root at a time. That is [`list_tree`]'s own shape
/// (`build_tree_listing` holds the same rows), so it is a cost this command
/// already pays elsewhere and not a new one.
///
/// **"As of now".** The disk can change between this reply and the scan that
/// acts on it, and the removal happens on each root's own next scan — which is
/// two halves of one sentence the editor owes a person (Task 11), not something
/// this count can carry.
#[tauri::command(async)]
pub fn mask_preview(state: State<'_, AppState>, pattern: String) -> Result<MaskPreview, Error> {
    // The candidate, alone and first: before the index is touched, and before
    // any stored rule can answer in its place. A refusal here is about the
    // pattern and is the same whether the index holds one file or a million.
    WalkRules::none().with_masks(vec![pattern.clone()])?;

    // One snapshot, for `list_tree`'s reason: an indexing job commits on its
    // own connection, outside this mutex, so roots read before it and files
    // read after it would be two different states of the index — and this
    // reply is a single claim about one of them. The masks and each root's
    // prefixes join the same snapshot for `walk_job.rs`'s reason: they are all
    // "the rules this walk runs under", and a set assembled from two moments
    // describes no scan that will ever run.
    let (stored, roots) = state.with_index(|db| {
        db.read_snapshot(|db| {
            let stored = db.list_masks()?;
            let mut roots = Vec::new();
            for root in db.list_watched_roots()? {
                roots.push((
                    root.absolute_path,
                    db.list_path_exclusions(root.id)?,
                    db.indexed_files_under_root(root.id)?,
                ));
            }
            Ok((stored, roots))
        })
    })?;

    let mut proposed = stored.clone();
    proposed.push(pattern);

    let mut paths = 0i64;
    // documentId -> (paths surviving the current set, of those, taken by the new one)
    let mut per_document: std::collections::HashMap<String, (i64, i64)> =
        std::collections::HashMap::new();

    for (absolute_path, prefixes, files) in roots {
        let root_path = Path::new(&absolute_path);
        // Both flags `true`, because both production callers pass `true`
        // (`walk_job.rs`, `bridge.rs`) and `AppliedRules` is documented as
        // assuming the built-in layers are on. The `.gitignore` half is not
        // representable here and is disclosed rather than approximated: a path
        // an in-tree `.gitignore` covers counts as surviving on BOTH sides.
        //
        // 🔴 That residue runs in two directions, not one. It makes `paths`
        // LARGER — a path already going is charged to this press — and it makes
        // `documents` SMALLER, because a document is only counted when EVERY
        // surviving path of it is taken, and an extra surviving path that
        // nothing here takes suppresses it. Smaller is the understating
        // direction, the one this whole entry point exists to close — which
        // is why the editor's zero arm was deleted outright rather than
        // narrowed: at `documents == 0` the screen now says nothing about
        // documents at all. Measured in
        // `mask_preview_understates_documents_behind_an_in_tree_gitignore`
        // (`tests/commands.rs`); until fix round 6 these lines said "can only
        // make this difference larger", which is true of `paths` alone.
        let current = WalkRules::new(true, true, prefixes.clone())?
            .with_masks(stored.clone())?
            .applied_to(root_path);
        let with_candidate = WalkRules::new(true, true, prefixes)?
            .with_masks(proposed.clone())?
            .applied_to(root_path);

        for file in files {
            // Already going on the next scan, whatever this press decides. Not
            // this mask's cost, and not a path cancelling would save.
            if current.removes_file(&file.relative_path) {
                continue;
            }
            let entry = per_document.entry(file.document_id).or_insert((0, 0));
            entry.0 += 1;
            if with_candidate.removes_file(&file.relative_path) {
                entry.1 += 1;
                paths += 1;
            }
        }
    }

    // No `&& *taken > 0`: every entry is created immediately before
    // `entry.0 += 1` two lines up, so `surviving >= 1` always, and
    // `surviving == taken` already implies `taken >= 1`. The second conjunct
    // could never be false where the first was true — reviewed and confirmed
    // unreachable rather than assumed (fix round 1, M3) — so it read as a check
    // that could not fire.
    let documents = per_document
        .values()
        .filter(|(surviving, taken)| surviving == taken)
        .count() as i64;

    Ok(MaskPreview { paths, documents })
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

/// A camelCase mirror of [`mnema_core::Segment`] for the wire.
///
/// ⚠️ **Do not "fix" this by putting `rename_all` on `Segment` itself.** That
/// type is *persisted*: `chunk.char_span` stores `serde_json::to_string` of a
/// `Vec<Segment>` (`Db::insert_chunk_in`) and the schema reads the stored JSON by key
/// — the `CHECK` extracts `$[0].block_id` and the `chunk_span_blocks_bi`
/// trigger extracts `$.block_id`. Renaming the field would make every stored
/// row unreadable and both guards blind. The same "local mirror at the seam"
/// move `Hit` already makes for `Citation` (`bridge.rs:479-492`).
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

/// How far the indexed text has drifted from the file it was read out of.
///
/// The only filesystem call either command makes, and it is here rather than
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

/// The most blocks `open_document` will return.
///
/// At the ceiling the command answers `unavailable { tooLarge }` rather than a
/// shortened list: the citation the pane was opened for can sit past the cut,
/// and a list missing it reads as "nothing found" instead of "incomplete".
/// 50 000 blocks is hundreds of thousands of pages and well past any real
/// document — the number is a guess, not a measurement.
///
/// Pinned by `tests::the_block_ceiling_is_fifty_thousand` in this file: the
/// shell test that pins the refusal would pass for any smaller number, so the
/// number itself needs its own assertion.
///
/// shortcut: paging, when a document this size actually turns up; v1 sends the
/// whole file in one payload.
const MAX_BLOCKS: usize = 50_000;

/// The whole of one document's indexed text, or the reason it cannot be shown.
///
/// `blocks` is the file, not a window around a citation — the pane scrolls the
/// file itself. `kind` on each block is `block.type` verbatim, so the pane can
/// drop page headers or draw headings without the backend baking that display
/// choice in.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum DocumentView {
    File {
        blocks: Vec<ViewBlock>,
        /// Every block the focused citation covers, in reading order — a chunk
        /// can span several. Empty when no focus was accepted.
        focus_block_ids: Vec<i64>,
        /// The exact slice to paint inside those blocks.
        spans: Vec<PaneSpan>,
        /// The path this view was resolved for: the caller's own when it named
        /// one, the document's sole `path` row when it named none. `None` when
        /// neither exists — the same case `Freshness::NoPath` reports, and the
        /// pane titles itself from it rather than staying blank.
        relative_path: Option<String>,
        /// The tree's name for the watched root that `relative_path` belongs to
        /// (the same [`basename`] `list_tree` prints), so the head of the right
        /// panel can show the path from its root. `None` exactly when
        /// `relative_path` is.
        root_name: Option<String>,
        freshness: Freshness,
    },
    /// The document cannot be shown whole, and a partial one never is.
    Unavailable { reason: UnavailableReason },
}

/// One block of [`DocumentView::File`], section title included from its page.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewBlock {
    pub block_id: i64,
    pub kind: String,
    pub text: String,
    pub page_no: i64,
    pub section_title: Option<String>,
}

/// A highlight measured inside one block, flattened for a whole-file pane.
///
/// The unit is [`WireSegment`]'s: `block_start` is the offset into the text of
/// block `block_id`, in **Unicode scalar values**, not UTF-16 code units.
/// `start`/`end` from the stored span are not shipped: they index the chunk's
/// own text, and that text is not on this wire — only their difference, as
/// `length`, is useful. `primary` mirrors `Source.svelte`'s own spans; v1
/// highlights one citation, so every span here is primary.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneSpan {
    pub block_id: i64,
    pub block_start: u32,
    pub length: u32,
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UnavailableReason {
    NoSuchDocument,
    NoBlocks,
    /// The document is being written right now (`document.status = 'pending'`):
    /// ingest clears and rewrites it a slice at a time, so what the index holds
    /// is a prefix of the file, not the file. A fourth reason rather than
    /// `noBlocks`, which would say the document is empty when it is unfinished.
    Rebuilding,
    TooLarge,
}

/// [`DocumentView::File`] minus the verdict only a `stat` can give: the index
/// half is read inside one snapshot, and the `stat` happens after it.
struct FileFields {
    blocks: Vec<ViewBlock>,
    focus_block_ids: Vec<i64>,
    spans: Vec<PaneSpan>,
    /// The path the view was resolved for, already picked by
    /// [`build_document_view`] — the pane's title, and the only place a
    /// citation that named no place on disk can get one.
    relative_path: Option<String>,
    /// The tree's name for that path's root, read off the same occupant.
    root_name: Option<String>,
}

impl FileFields {
    fn with(self, freshness: Freshness) -> DocumentView {
        DocumentView::File {
            blocks: self.blocks,
            focus_block_ids: self.focus_block_ids,
            spans: self.spans,
            relative_path: self.relative_path,
            root_name: self.root_name,
            freshness,
        }
    }
}

/// What one snapshot can know about a document view; the freshness verdict is
/// decided after it, outside the snapshot and outside `with_index`.
enum ComposedView {
    Settled(DocumentView),
    Pending {
        file: FileFields,
        occupant: Option<PathOccupant>,
    },
}

/// One document's blocks, its focus and the `path` row freshness needs — no
/// `stat`, for the reason [`decide_freshness`] gives.
///
/// The fallback for a caller that named no location: when the document has
/// exactly one `path` row, that row is the location; when it has several or
/// none, the verdict is [`Freshness::NoPath`] rather than a guess about one of
/// them. Half a location (a root with no path) is left to [`cited_occupant`],
/// which already says what it can and cannot resolve.
fn build_document_view(
    db: &Db,
    document_id: &str,
    root_id: Option<i64>,
    relative_path: Option<&str>,
    focus: Option<(i64, i64, &str)>,
    limit: usize,
) -> Result<ComposedView, mnema_index::Error> {
    let Some(view) = db.document_view(document_id, limit)? else {
        return Ok(ComposedView::Settled(DocumentView::Unavailable {
            reason: UnavailableReason::NoSuchDocument,
        }));
    };

    // The status gate, and why it is not optional: ingest records a rebuild and
    // then writes the document back **a slice at a time** (`STATUS_REBUILDING`,
    // `mnema-ingest/src/lib.rs:530`, written through before the first slice),
    // while `clear_document_content_in` deletes the pages and then writes
    // `pending`, both in one transaction (`write.rs:1220-1221`). A `pending`
    // document that still holds blocks is therefore a file being rewritten, and
    // answering with those blocks would be a partial file that claims to be
    // whole — the one failure this pane exists to avoid. `failed`/`skipped`
    // have no corpus text to show.
    match db.document_status(document_id)? {
        DocumentStatus::Pending => {
            return Ok(ComposedView::Settled(DocumentView::Unavailable {
                reason: UnavailableReason::Rebuilding,
            }));
        }
        DocumentStatus::Failed | DocumentStatus::Skipped => {
            return Ok(ComposedView::Settled(DocumentView::Unavailable {
                reason: UnavailableReason::NoBlocks,
            }));
        }
        DocumentStatus::Indexed => {}
    }

    if view.truncated {
        return Ok(ComposedView::Settled(DocumentView::Unavailable {
            reason: UnavailableReason::TooLarge,
        }));
    }
    if view.blocks.is_empty() {
        return Ok(ComposedView::Settled(DocumentView::Unavailable {
            reason: UnavailableReason::NoBlocks,
        }));
    }

    // The focus is accepted only with its full occurrence identity:
    // `chunk.id` is reused, so the id alone could highlight another passage
    // inside the right file.
    let (focus_block_ids, spans) = match focus {
        Some((chunk_id, ord, text)) => match db.chunk_anchor(chunk_id)? {
            Some(anchor)
                if anchor.document_id == document_id
                    && anchor.ord == ord
                    && anchor.text == text =>
            {
                (
                    anchor.spans.iter().map(|s| s.block_id).collect(),
                    anchor
                        .spans
                        .into_iter()
                        // Through the one `WireSegment` conversion, not a second
                        // copy of it: this is the payload the pane paints, and
                        // the unit it documents is the same one.
                        .map(|s| {
                            let wire = WireSegment::from(s);
                            PaneSpan {
                                block_id: wire.block_id,
                                block_start: wire.block_start,
                                length: wire.end - wire.start,
                                primary: true,
                            }
                        })
                        .collect(),
                )
            }
            _ => (Vec::new(), Vec::new()),
        },
        None => (Vec::new(), Vec::new()),
    };

    let occupant = match (root_id, relative_path) {
        (None, None) => match db.document_path(document_id)? {
            Some((root, path)) => cited_occupant(db, Some(root), Some(&path))?,
            None => None,
        },
        (root_id, relative_path) => cited_occupant(db, root_id, relative_path)?,
    };

    Ok(ComposedView::Pending {
        file: FileFields {
            blocks: view
                .blocks
                .into_iter()
                .map(|b| ViewBlock {
                    block_id: b.block_id,
                    kind: b.kind,
                    text: b.text,
                    page_no: b.page_no,
                    section_title: b.section_title,
                })
                .collect(),
            focus_block_ids,
            spans,
            // Read off the occupant freshness is decided for, so the title and
            // the badge can never name two different places.
            relative_path: occupant.as_ref().map(|o| o.relative_path.clone()),
            // The same occupant, through the rule `list_tree` names roots by.
            root_name: occupant.as_ref().map(|o| basename(&o.root_absolute_path)),
        },
        occupant,
    })
}

/// The whole of one document, read out of the index — what the right pane
/// paints.
///
/// Everything the index can answer comes back from one
/// [`mnema_index::Db::read_snapshot`]; the `stat` that decides `freshness`
/// happens outside it and outside [`AppState::with_index`], for the reason
/// [`decide_freshness`] gives.
///
/// A `chunk_id` without its `ord` and `text` is not refused. It is ignored:
/// the file still opens, unhighlighted, and a client that sent half the pair
/// gets no highlight rather than one on a passage it may not have meant.
#[allow(clippy::too_many_arguments)]
#[tauri::command(async)]
pub fn open_document(
    state: State<'_, AppState>,
    document_id: String,
    root_id: Option<i64>,
    relative_path: Option<String>,
    chunk_id: Option<i64>,
    cited_ord: Option<i64>,
    cited_text: Option<String>,
) -> Result<DocumentView, Error> {
    let focus = match (chunk_id, cited_ord, cited_text.as_deref()) {
        (Some(chunk_id), Some(ord), Some(text)) => Some((chunk_id, ord, text)),
        _ => None,
    };
    let composed = state.with_index(|db| {
        db.read_snapshot(|db| {
            build_document_view(
                db,
                &document_id,
                root_id,
                relative_path.as_deref(),
                focus,
                MAX_BLOCKS,
            )
        })
    })?;
    Ok(match composed {
        ComposedView::Settled(view) => view,
        ComposedView::Pending { file, occupant } => {
            let freshness = decide_freshness(occupant.as_ref(), &document_id);
            file.with(freshness)
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
    /// chose from disk, not a pattern (`rules.rs:505-522`).
    pub relative_path: String,
    pub state: SubfolderState,
}

/// What holds a subfolder, as one tagged state rather than three booleans.
///
/// 🔴 **The invariant every state but `Open` exists to keep: a row this
/// listing shows must not offer an exclusion control that cannot succeed or
/// that changes nothing.** Fix round 1 found the invariant held at the top
/// level and nowhere else, so the enumeration below was re-derived from
/// `crates/mnema-walk/src/rules.rs` — from `builder()` and `validate_prefix`,
/// not from the states that already existed. **Every way this listing can show
/// a folder whose exclusion is a no-op or a refusal:**
///
/// | # | the layer | asked by | state |
/// |---|---|---|---|
/// | 1 | **any override-based built-in layer** prunes the folder **or an ancestor** — today `BUILTIN_DIRS` and `BUILTIN_FILES`, and tomorrow whatever else `builder()` adds through `OverrideBuilder` | `BuiltinLayers::prunes` | `BuiltIn` |
/// | 2 | `ANCHORED_DIRS` names the folder **or an ancestor** *and* a marker file sits in that one's own parent — the one built-in layer that is not an override | the same call | `BuiltIn` |
/// | 3 | `follow_links(false)` — the folder is a symlink | `Entry::is_symlink` | `Symlink` |
/// | 4 | `validate_prefix` refuses the path as a rule — whitespace at a component's edge, a backslash, a control character, `C:` or `~` as the first component, or a prefix too large to compile alone | `WalkRules::check_prefix` | `UnusableName` |
/// | 5 | the in-tree `.gitignore` stack | **nothing — see below** | `Open` |
///
/// Rows 1, 2 and 4 are asked through **one call each into `mnema-walk`**,
/// never re-implemented here: the whitelist in row 4 grew across three review
/// rounds, the last of which found four forms the round before had let
/// through, and a copy of it in this file would be the fourth round waiting to
/// happen. Row 3 is the one this listing can see for itself, because the link
/// is the entry.
///
/// 🔴 **Row 1 is a shape, not a list, and fix round 2 is why.** It read
/// "`BUILTIN_DIRS` names the folder or an ancestor" for a round, and the
/// predicate behind it enumerated that constant by hand — while `builder()`
/// also compiles `BUILTIN_FILES` into `!**/.DS_Store`, which carries no
/// trailing `/` and prunes a *directory* of that name at any depth. A folder
/// called `.DS_Store` was therefore offered as ordinary and excludable, and
/// the predicate's own doc argued that could not happen. `BuiltinLayers` now
/// asks the compiled `Override` — built from the same function `builder()`
/// adds from — so no reading of gitignore semantics happens on this side and a
/// future override layer is covered without an edit here. Row 2 stays a named
/// layer because `filter_entry` is not a glob and no matcher can answer it.
///
/// 🔴 **Rows 1 and 2 are a claim about `exclude_subfolder`, and that command
/// now honours it.** `BuiltIn` says the walk prunes the folder and there is
/// nothing to toggle; until fix round 2 the toggle worked anyway one command
/// over, wrote a row, and `list_exclusions` rendered it live. `bridge.rs`
/// refuses those paths through the same `builtin_layers` call this file makes.
/// Row 4 was already cross-checked that way — `exclude_subfolder` runs the
/// same validator — which is why it was the one row where the two agreed.
///
/// ⚠️ **Row 5 is deliberately not closable here, and no claim above covers
/// it.** Deciding the in-tree `.gitignore` stack means compiling the same
/// ignore stack the walk builds, per directory, out of files inside the tree —
/// which this read-only listing does not do. **So `Open` does not mean "will
/// be indexed"**: it means *no layer this command can see* holds the folder. A
/// window's wording here must be about the rules this screen can see, never
/// about what the walk will do.
///
/// **Precedence, in the order [`subfolder_state`] applies it.** The first two
/// are walk layers no rule can lift; the next two report a rule that exists
/// and whose removal does something; `UnusableName` is what stands between "no
/// rule holds it" and "you may add one".
///
/// 1. `BuiltIn` — rows 1 and 2 above. Pruned whatever any user rule says
///    (`builder()` adds the overrides at `rules.rs:446-447`, `builtin_layers`
///    compiles the same patterns at `:361-362`, and the anchored layer is
///    `filter_entry` at `:411-438`), so an
///    `Excluded` badge with a working "include" toggle would announce one
///    thing and do another.
///
///    ⚠️ A symlink named like an anchored directory, beside that name's
///    marker, reports `BuiltIn` rather than `Symlink` — `BuiltinLayers` does
///    not carry `filter_entry`'s own `is_dir()` guard, so the two sides reach
///    the same outcome by different routes. The row is true either way (the
///    walk does not descend and nothing under it is indexed) and both states
///    are non-toggleable, so the precedence is left as it is rather than
///    reordered for a case where both answers are correct.
/// 2. `Symlink` — row 3. The walk runs `follow_links(false)`
///    (`rules.rs:388`), so nothing under this name is ever enumerated.
/// 3. `ExcludedByAncestor` — a stored rule names an ancestor. Checked
///    **before** `Excluded`, so a folder that has both its own rule and an
///    excluded ancestor names the ancestor: removing its own rule would leave
///    the ancestor's, and the row would not open. Its own rule is not hidden
///    from the person — `list_exclusions` lists every stored prefix.
/// 4. `Excluded` — a stored rule names this folder itself, and nothing above
///    it. The one state whose control does what it says.
/// 5. `UnusableName` — row 4. Below the two rule states deliberately: a rule
///    that already exists is a fact worth reporting, and its removal works.
///    (Reaching both at once takes a rule this build's `exclude_subfolder`
///    would not write, since it runs the same validator — but that is not the
///    same as impossible: a prefix stored **by an older build whose whitelist
///    was narrower** reaches it, which is the path `Error::InvalidRule`
///    already names for the walk, and a row written straight into the database
///    reaches it too.)
/// 6. `Open` — none of the above.
///
/// ⚠️ `rename_all_fields`, for the reason [`DocumentView`] needed it: on an
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
    /// sorts (`write.rs:580`), and the ancestors of one path form a chain in
    /// which each is a string prefix of the next, so the first match in that
    /// sorted list is the shallowest one — the rule whose removal has to come
    /// first.
    ExcludedByAncestor { prefix: String },
    /// One of the walk's unconditional layers prunes this folder **or an
    /// ancestor of it**, so no user rule can change whether anything under it
    /// is indexed: `WalkRules::BUILTIN_DIRS` names it or an ancestor
    /// (`rules.rs:134-147`), or `ANCHORED_DIRS` does and that one's marker
    /// file sits beside it (`rules.rs:156-178`).
    ///
    /// **One state for both layers, and for ancestors as well as the folder
    /// itself**, because the row says the same thing in every case: the walk
    /// prunes it, it is visible here, and there is nothing to toggle. Fix
    /// round 1: before it, this was decided from the entry's own name only, so
    /// `.git/hooks` was offered as an ordinary excludable folder and
    /// `exclude_subfolder` accepted the rule.
    BuiltIn,
    /// A symlink to a directory. Listed, so the folder does not look emptier
    /// than it is, but never as an ordinary excludable folder: the walk runs
    /// `follow_links(false)` (`rules.rs:388`), so nothing under it is indexed
    /// and an exclusion rule naming it would exclude nothing.
    Symlink,
    /// The folder **is** walked, and its path is not one an exclusion rule can
    /// name: `WalkRules::check_prefix` refuses it. A directory called
    /// `Scans 2019 ` is easy to produce by paste and invisible in Finder, and
    /// a row for it that offered "exclude" would be answered with a message
    /// about path components for a path the person never typed.
    ///
    /// **Its own state rather than `BuiltIn`, because the two are opposite
    /// facts about indexing.** A `BuiltIn` folder's contents never reach a
    /// provider; this one's do, and the window has to be able to say so —
    /// under D29 that is the difference between "protected" and "you cannot
    /// protect this from here".
    UnusableName,
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
/// chose from disk (`rules.rs:505-522`), and the walk's own matcher compares
/// it byte for byte.
fn is_ancestor_of(prefix: &str, path: &str) -> bool {
    path.len() > prefix.len() && path.as_bytes()[prefix.len()] == b'/' && path.starts_with(prefix)
}

/// What holds one entry. The enumeration and the precedence are
/// [`SubfolderState`]'s own, and the argument for each step is there rather
/// than repeated here.
///
/// `layers` is compiled once per listing rather than per row — it holds a
/// globset — and it carries the watched folder's canonical path, which the
/// anchored layer needs: `ANCHORED_DIRS` prunes a name only when a marker file
/// sits in that directory's own parent, a question about the disk rather than
/// about the path.
fn subfolder_state(
    layers: &BuiltinLayers,
    relative_path: &str,
    is_symlink: bool,
    prefixes: &[String],
) -> SubfolderState {
    if layers.prunes(relative_path) {
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
    // The whole path, not the entry's name: `validate_component` applies the
    // drive-letter and `~` rules to the FIRST component only, so `a/~` is a
    // rule `exclude_subfolder` accepts and `~` at the top level is not. Asking
    // about the name alone would refuse the first of those.
    if WalkRules::check_prefix(relative_path).is_err() {
        return SubfolderState::UnusableName;
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
/// Sorted by name for the reason the walk sorts (`rules.rs:387`): `read_dir`
/// order is the filesystem's, and a window that redraws in a different order on
/// another machine is a window nobody can point at over the phone.
///
/// Only directories are counted in `unnameable`, because only directories would
/// have been listed: an unnameable *file* is no more missing from this answer
/// than a nameable one.
fn read_subfolders(
    layers: &BuiltinLayers,
    entries: &[Entry],
    parent: &str,
    prefixes: &[String],
) -> SubfolderListing {
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
        let state = subfolder_state(layers, &relative_path, entry.is_symlink, prefixes);
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
    // After containment, never before: `..` and an absolute path are refused
    // by the rule above with a sentence about where they resolve, which is the
    // more specific answer and the one three tests assert. What is left for
    // this to catch is a path that stays inside the root and still cannot be a
    // rule — `a/.`, `a//b`, a component with a backslash — and it is refused
    // with `WalkRules`'s own sentence, the same one `exclude_subfolder` would
    // give, because every `relativePath` this call would emit has this path as
    // its ancestor and would inherit the refusal.
    WalkRules::check_prefix(&relative_path)?;
    let prefixes = state.with_index(|db| db.list_path_exclusions(root_id))?;
    let read = std::fs::read_dir(&dir).map_err(|e| refusal(root_id, &relative_path, e))?;
    let entries = directory_entries(read).map_err(|source| Error::SubfolderUnreadable {
        root_id,
        relative_path: relative_path.clone(),
        source,
    })?;
    // Compiled once for the whole listing, not once a row: it holds a globset,
    // and `subfolder_state` asks it for every entry.
    let layers = WalkRules::builtin_layers(&root_canonical);
    Ok(read_subfolders(
        &layers,
        &entries,
        &relative_path,
        &prefixes,
    ))
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

        // A root that is not there: nothing in this test reaches the disk —
        // the anchored arm of `BuiltinLayers::prunes` is the only part that
        // would, and none of these names is `target`, `build` or `dist`.
        let layers = WalkRules::builtin_layers(Path::new("/nonexistent-root"));
        let listing = read_subfolders(&layers, &entries, "", &[]);

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
                row("trailing ", SubfolderState::UnusableName),
            ],
            unnameable: 2,
        }
    }

    /// The entries come back sorted by name whatever order the filesystem
    /// handed them out in.
    ///
    /// **A `--lib` test over the pure function rather than only the IPC one**,
    /// because through a real directory the input order is the filesystem's and
    /// a listing that never sorted could come back already sorted by luck —
    /// which would report a mutation case STILL GREEN and send someone to
    /// investigate a case that is fine (fix round 1, M2). Here the input order
    /// is chosen, so the case cannot flake in either direction.
    #[cfg(unix)]
    #[test]
    fn the_entries_are_sorted_by_name_whatever_order_they_arrive_in() {
        let entries = vec![
            entry(b"zulu", true, false),
            entry(b"alpha", true, false),
            entry(b"mike", true, false),
            entry(b"bravo", true, false),
            entry(b"kilo", true, false),
        ];

        let layers = WalkRules::builtin_layers(Path::new("/nonexistent-root"));
        let listing = read_subfolders(&layers, &entries, "", &[]);

        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["alpha", "bravo", "kilo", "mike", "zulu"],
            "the entries must come back sorted, not in the order they arrived: {listing:?}"
        );
    }

    /// The wire contract task 5's UI matches on, both directions.
    ///
    /// `SubfolderState` is an **enum**, so `rename_all` renames its variants
    /// and `rename_all_fields` is what would rename a struct variant's fields —
    /// the trap `DocumentView` carries one type up. `prefix` is one word, so
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
        //
        // 🔴 The VALUES, not `is_string()` (fix round 1). A type is satisfied
        // by any string, so a `name`/`relativePath` swap — the exact confusion
        // this pair exists to keep apart, and the one a caller sends back to
        // `exclude_subfolder` — passed it. `sample_subfolders` builds them
        // differently on purpose: the name is the last component, the relative
        // path carries `Work/` in front of it.
        assert_eq!(rows[0]["relativePath"], "Work/open", "{v}");
        assert_eq!(rows[0]["name"], "open", "{v}");
        assert!(rows[0].get("relative_path").is_none(), "{v}");

        let tags = [
            "open",
            "excluded",
            "excludedByAncestor",
            "builtIn",
            "symlink",
            "unusableName",
        ];
        assert_eq!(
            rows.len(),
            tags.len(),
            "every state has to be in the sample, or a tag crosses untested: {v}"
        );
        for (row, tag) in rows.iter().zip(tags) {
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
                searched: vec![SearchedFolder {
                    folder_path: "notes".into(),
                    searched: false,
                }],
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

    /// One `DocumentView::File` carrying every camelCase field the pane reads.
    fn sample_view() -> DocumentView {
        DocumentView::File {
            blocks: vec![ViewBlock {
                block_id: 41,
                kind: "paragraph".into(),
                text: "Ціна оцифрування одного аркуша становить дві гривні.".into(),
                page_no: 3,
                section_title: Some("Розділ перший".into()),
            }],
            focus_block_ids: vec![41],
            spans: vec![PaneSpan {
                block_id: 41,
                block_start: 2,
                length: 5,
                primary: true,
            }],
            relative_path: Some("notes/a.md".into()),
            root_name: Some("docs".into()),
            freshness: Freshness::FileChanged,
        }
    }

    /// The wire contract the pane matches on (§10), asserted in both
    /// directions.
    ///
    /// `DocumentView` is an **enum**, where `rename_all` renames the variants
    /// only and `rename_all_fields` renames a struct variant's fields — without
    /// the second attribute this ships `focus_block_ids` inside a camelCase
    /// payload. `ViewBlock` and `PaneSpan` are plain structs, where
    /// `rename_all` does rename the fields.
    #[test]
    fn document_view_wire_shape_is_camel_case() {
        let v = serde_json::to_value(sample_view()).unwrap();

        assert_eq!(v["kind"], "file");
        // Present, camelCase — the struct-variant fields `rename_all` alone
        // would have left in snake_case.
        assert!(v["focusBlockIds"].is_array());
        // Absent, the other direction: a payload carrying both spellings would
        // satisfy the assertions above while the pane read the wrong one.
        assert!(v.get("focus_block_ids").is_none());

        let block = &v["blocks"][0];
        assert!(block["blockId"].is_i64());
        assert!(block["pageNo"].is_i64());
        assert!(block["sectionTitle"].is_string());
        assert_eq!(block["kind"], "paragraph");
        assert!(block.get("block_id").is_none());
        assert!(block.get("page_no").is_none());
        assert!(block.get("section_title").is_none());

        let span = &v["spans"][0];
        assert!(span["blockId"].is_i64());
        assert!(span["blockStart"].is_u64());
        assert_eq!(span["primary"], true);
        assert!(span.get("block_id").is_none());
        assert!(span.get("block_start").is_none());

        // The pane's title, when the citation named no path on disk.
        assert_eq!(v["relativePath"], "notes/a.md");
        assert!(v.get("relative_path").is_none());
        assert_eq!(v["rootName"], "docs");
        assert!(v.get("root_name").is_none());

        // The refusal carries its reason as a bare tag — the window switches on
        // the string, not on an object.
        let refused = serde_json::to_value(DocumentView::Unavailable {
            reason: UnavailableReason::TooLarge,
        })
        .unwrap();
        assert_eq!(refused["kind"], "unavailable");
        assert_eq!(refused["reason"], "tooLarge");
        assert!(refused.get("blocks").is_none());
    }

    /// A missing section title crosses as `null`, **present**, not as an absent
    /// key: "we do not know" and "the value is empty" must not render as the
    /// same thing (`list_tree`'s wire test pins the same rule).
    #[test]
    fn a_block_with_no_section_title_still_carries_the_key() {
        let DocumentView::File {
            blocks,
            focus_block_ids,
            spans,
            relative_path,
            root_name,
            freshness,
        } = sample_view()
        else {
            unreachable!("sample_view is a File")
        };
        let untitled = serde_json::to_value(DocumentView::File {
            blocks: blocks
                .into_iter()
                .map(|b| ViewBlock {
                    section_title: None,
                    ..b
                })
                .collect(),
            focus_block_ids,
            spans,
            relative_path,
            root_name,
            freshness,
        })
        .unwrap();

        assert!(untitled["blocks"][0].get("sectionTitle").is_some());
        assert!(untitled["blocks"][0]["sectionTitle"].is_null());
    }

    /// Every `Freshness` tag, because the set is closed (the outcomes of two
    /// comparisons) and the pane must render each: a default branch drawing
    /// `Current` for a tag it does not recognise is how a stale file gets shown
    /// as fresh.
    #[test]
    fn every_freshness_tag_crosses_camel_case() {
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
    }

    /// The `WireSegment` conversion is load-bearing, so this proves it rather
    /// than assuming it.
    ///
    /// [`mnema_core::Segment`] is **persisted** — `chunk.char_span` stores a
    /// `Vec<Segment>` as JSON and the schema's `CHECK` and its
    /// `chunk_span_blocks_bi` trigger both read that JSON *by key* — so the
    /// type cannot be given a `rename_all` and crosses as `block_start`. The
    /// pane's `PaneSpan` is built through this conversion, so a swapped field
    /// would ship a perfectly camelCase, perfectly wrong highlight.
    ///
    /// Both directions: the mirror must be camelCase **and** must carry the
    /// same four values, in the same roles.
    #[test]
    fn a_real_segment_crosses_the_wire_through_its_camel_case_mirror() {
        let segment = sample_segment();
        let v = serde_json::to_value(WireSegment::from(segment)).unwrap();

        assert_eq!(v["blockId"], segment.block_id);
        assert_eq!(v["start"], segment.start);
        assert_eq!(v["end"], segment.end);
        assert_eq!(v["blockStart"], segment.block_start);

        assert!(v.get("block_start").is_none());
        assert!(v.get("block_id").is_none());
    }

    #[test]
    fn wire_shape_is_camel_case() {
        let v = serde_json::to_value(sample()).unwrap();
        let root = &v["roots"][0];

        // Present, camelCase.
        assert!(root["rootId"].is_i64());
        assert!(root["absolutePath"].is_string());
        assert!(root["files"][0]["relativePath"].is_string());
        assert!(root["files"][0]["documentId"].is_string());

        // Absent — the snake_case names must not leak (guards rename_all).
        assert!(root.get("root_id").is_none());
        assert!(root.get("absolute_path").is_none());
        assert!(root["files"][0].get("relative_path").is_none());

        // The listing carries no recents: the launcher's history is read by its
        // own commands (`list_history`), not from here.
        assert!(
            v.get("recents").is_none(),
            "list_tree still carries recents: {v}"
        );
    }

    /// The ceiling is part of the wire contract, not a tuning knob: any smaller
    /// number still refuses a 50 001-block document, so the shell test that
    /// pins the refusal cannot see a downward drift. This is that half.
    #[test]
    fn the_block_ceiling_is_fifty_thousand() {
        assert_eq!(MAX_BLOCKS, 50_000);
    }
}

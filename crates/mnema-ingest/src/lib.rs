//! One file on disk becoming a citation someone can read.
//!
//! Everything before this crate built a piece: a worker process that turns
//! bytes into blocks, a supervisor that survives every way that process can
//! die, a schema with the guards a wrong chunk trips over, and a chunker that
//! is a pure function. This is the only place that knows the **order** they go
//! in, and the order is the whole content of the crate.
//!
//! It lives apart from all four for a reason cargo enforces rather than a
//! preference: `mnema-pool` depends on `mnema-index` (for `SkipRule`), so the
//! orchestration cannot sit inside `mnema-index` without a dependency cycle.
//! Keeping it separate also keeps each of the four honest about what it is —
//! storage, supervision, a pure function — and gives the one component that is
//! none of those a home.
//!
//! Two distinctions run through everything here and must not blur:
//!
//! * **A per-file problem is never an `Err`.** A folder nobody curated
//!   contains files that cannot be read, formats with no reader, and documents
//!   that kill a parser. Each of those is recorded and the walk continues.
//!   [`IngestError`] means the job should stop — the pool is broken, or the
//!   database is. Collapsing the two either abandons a multi-hour run over one
//!   bad document, or records forty thousand files as damaged when the real
//!   fault is a half-finished install.
//! * **Characters, never bytes.** Every offset that reaches the database is a
//!   character offset, and the chunker is what produces them; nothing here
//!   recomputes one.

use std::path::Path;

use mnema_chunk::{Chunk, PageContext, chunk_blocks};
use mnema_core::{Block, BlockType, OnDisk, SourceKind};
use mnema_index::{Db, DocumentStatus, INDEX_FORMAT_VERSION, PathEntry, SkipRule};
use mnema_pool::{Document, Outcome, Pool, PoolError};

mod walk;
pub use walk::{StopReason, WalkProgress, WalkReport, walk_root};

/// The stage `ingest_stage` records once a document's chunks are written.
///
/// Named here rather than in `mnema-index` because the stages belong to
/// whoever runs the pipeline: the column has no CHECK
/// (`crates/mnema-index/src/schema.sql:219-224`) precisely so that adding one
/// costs no migration.
pub const STAGE_CHUNK: &str = "chunk";

/// What `ingest_stage.status` says about a stage that finished.
pub const STATUS_DONE: &str = "done";

/// How many pages are written under one transaction.
///
/// **Provisional, and now exercised.** A `.txt` is exactly one page (D37), so
/// until markdown arrived the loop below ran once and this constant never bit;
/// `tests/slice.rs::ord_rises_across_pages_and_across_slices` builds a file of
/// `PAGES_PER_TRANSACTION + 2` sections precisely to cross a boundary. The
/// reason for slicing is still the one PDF will bring: a thousand-page
/// document under one transaction is a write lock held for the length of the
/// whole document, and a job killed half-way through loses all of it. The
/// number 20 is a guess at the balance between that and the per-transaction
/// cost; nothing has measured it, and a markdown file's pages are far smaller
/// than the PDF pages it was guessed for.
///
/// It is `pub` and a `const`: a test can read it — and does, to size its
/// fixture — but cannot lower it. Making it configurable to let a test use a
/// smaller number would be a shape the product carries for the tests' sake.
pub const PAGES_PER_TRANSACTION: usize = 20;

/// What one call to [`ingest_file`] settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ingested {
    /// Read, chunked and written. `chunks` is how many rows landed.
    Indexed { document_id: String, chunks: usize },
    /// The `path` row matched on size and mtime — the file was never opened.
    Unchanged { document_id: String },
    /// A different path with content already indexed; only the `path` row is
    /// new.
    AlreadyIndexed { document_id: String },
    /// Recorded in `skipped`; the walk continues.
    ///
    /// When the skip means what the index held under this name is a previous
    /// version of a file that has since become unreadable, it is removed with
    /// it — see `displaces`, which decides that per rule and, for the size
    /// ceiling, on whether the file's size changed.
    Skipped { rule: SkipRule },
}

/// What stops the job, as opposed to what costs one file.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("the extraction pool cannot continue: {0}")]
    Pool(#[from] PoolError),
    #[error("the index cannot be written: {0}")]
    Index(mnema_index::Error),
    /// Another connection held the write lock for longer than the index's
    /// busy timeout. **This file should be retried; the walk should not stop.**
    ///
    /// Every other variant here means the job cannot continue. This one means
    /// it can, in a moment: the shape that produces it is the window adding a
    /// folder while a walk runs, which is an ordinary thing for a user to do
    /// and which measured at 5.19 s of waiting before `SQLITE_BUSY` came back.
    /// Ending a multi-hour walk over it would be the wrong answer to the one
    /// error that says "wait".
    ///
    /// It is a variant rather than a longer timeout because the timeout's value
    /// is the *window's* contract — how long a user may see a frozen search —
    /// and a walk that can afford to wait far longer must not be the reason
    /// that number grows. Nothing retries yet: `ingest_file` is handed one file
    /// and the walker owns the decision to come back to it, so this exists to
    /// make that decision expressible.
    #[error("the index is busy: {0}")]
    Busy(mnema_index::Error),
}

/// Sorts a database error into "stop" and "come back to this file".
///
/// Written out rather than derived with `#[from]` so that every `?` on an
/// index call in this crate makes the distinction automatically — the
/// alternative is remembering it at each of the fifteen call sites, which is
/// the kind of thing that is remembered fourteen times.
impl From<mnema_index::Error> for IngestError {
    fn from(error: mnema_index::Error) -> Self {
        if error.is_contention() {
            IngestError::Busy(error)
        } else {
            IngestError::Index(error)
        }
    }
}

/// Reads one file and writes everything it becomes: a document, a page, its
/// blocks, its chunks and their search rows.
///
/// `relative` is passed rather than derived from `absolute`, because the caller
/// owns the watched root and this function must not guess where the root ends.
///
/// The order of operations is the design, and each step exists because the
/// step before it is expensive:
///
/// 1. The `path` row, on `(root_id, relative)`. Size and mtime both matching
///    the file on disk, **and** its document's chunking recorded as finished,
///    means the file was not touched and no worker process is started at all.
/// 2. The pool. A skip is recorded and returned; only a broken pool is an
///    `Err`.
/// 3. The content. `document.id` **is** the sha256, so two copies of one file
///    are one document — and if that document's chunking already finished, all
///    that is new is where the file sits.
/// 4. The write, one transaction per slice of pages.
/// 5. The checkpoint, so that step 3 can answer next time.
///
/// `on_disk` is the walk's own stat, handed in rather than taken here. `None`
/// means the walk could not stat the file at all, which is the same state the
/// local `stat` used to express by returning `None`.
///
/// One reading, not two: between a stat taken by the walk and a stat taken
/// here the file can change, and then the walk counted one size while this
/// compared another. The difference never surfaces as an error — only as an
/// index holding a previous version of a file it believes it re-read (§5).
///
/// The freshness this buys is only as good as the measurement's age: `on_disk`
/// must come from the walk pass this call belongs to, and must never be carried
/// across passes or cached between one pass and the next. The enumeration
/// measures a whole root before anything is ingested from it, so "immediately
/// before" is not the rule and never was — the rule is one pass, one
/// measurement. A caller handing in a measurement from an earlier pass
/// reintroduces exactly the defect this parameter exists to close, one level
/// up: the cheap arm answers `Unchanged` for a file that has since changed, and
/// the index keeps text the file no longer contains, with nothing logged.
pub fn ingest_file(
    pool: &Pool,
    db: &Db,
    root_id: i64,
    absolute: &Path,
    relative: &str,
    on_disk: Option<OnDisk>,
) -> Result<Ingested, IngestError> {
    // 1. The cheap arm. A failure to stat is not decided here: the pool names
    //    an unreadable file properly, with the rule and the reason the journal
    //    wants, and duplicating that judgement is how the two would drift.
    //
    //    The stage is part of the question, not an extra check. The path row is
    //    written inside slice 0's transaction and the checkpoint only after
    //    every slice, so a job killed in between leaves a path row that matches
    //    the disk perfectly over a document nothing calls finished. On
    //    `(size, mtime)` alone this arm answers `Unchanged` to that state on
    //    every future walk, for the life of the index, and the rebuild in step 3
    //    is never reached.
    //
    //    That is the whole of what this clause buys, and two things it was once
    //    said to buy are not true. It does not make the rebuild reachable in
    //    *every* case a document ends up unfinished: a `done` stage left behind
    //    by a deleted document fools this check rather than being caught by it,
    //    which is why `ingest_stage` now cascades from `document` — the check
    //    can only be as good as the row it reads. And the damage is not confined
    //    to two columns: that was true while a document was one page in one
    //    transaction, and markdown made it false, because a walk interrupted
    //    after slice 0 leaves pages 21..n missing with nothing anywhere saying
    //    so.
    //
    //    It costs one lookup on a `WITHOUT ROWID` primary key per unchanged
    //    file, against the worker process it is here to avoid.
    let recorded = db.path_entry(root_id, relative)?;
    if let Some(disk) = on_disk
        && let Some(recorded) = &recorded
        && recorded.size_bytes == disk.size_bytes
        && recorded.mtime == disk.mtime
        && db
            .stage_status(&recorded.document_id, STAGE_CHUNK)?
            .as_deref()
            == Some(STATUS_DONE)
    {
        return Ok(Ingested::Unchanged {
            document_id: recorded.document_id.clone(),
        });
    }
    // The second cheap arm: the journal's remembered verdict, when it is
    // still current. `skip_entry` only ever carries bytes to compare for a
    // rule where `SkipRule::is_about_content()` is true — a reproducible
    // reading of the file itself, not of the machine or of a setting that can
    // change underneath it (its docstring has the case that taught this the
    // hard way: `TooLarge` looks like a content fact and is not one). Without
    // this arm, a folder of scans costs one worker process per file per walk
    // forever, which is the debt §16 recorded on 2026-07-27.
    if let Some(disk) = on_disk
        && let Some(skip) = db.skip_entry(root_id, relative)?
        && skip.format_version == INDEX_FORMAT_VERSION
        && skip.size_bytes == Some(disk.size_bytes)
        && skip.mtime == Some(disk.mtime)
    {
        return Ok(Ingested::Skipped { rule: skip.rule });
    }
    // `document.size_bytes` is NOT NULL, so a document with no measurement at
    // all cannot be written regardless of what the pool finds — and unlike
    // before this crate stopped statting for itself, there is no second,
    // different-in-kind read left to try. Checked here, ahead of the pool,
    // rather than after a successful extraction: the outcome does not depend
    // on anything the pool learns, and a file this crate already knows it
    // cannot measure does not need a worker cycle spent reading it first.
    let Some(disk) = on_disk else {
        let rule = SkipRule::Unreadable;
        record_skip(
            db,
            root_id,
            relative,
            "the walk could not measure this file, so its size and mtime are unknown",
            rule,
            &recorded,
            on_disk,
        )?;
        return Ok(Ingested::Skipped { rule });
    };

    // 2. The pool.
    let document = match pool.extract(absolute)? {
        Outcome::Skipped(skip) => {
            let rule = SkipRule::from(skip.failure);
            record_skip(
                db,
                root_id,
                relative,
                &skip.reason,
                rule,
                &recorded,
                on_disk,
            )?;
            return Ok(Ingested::Skipped { rule });
        }
        Outcome::Extracted(document) => document,
    };

    // What this path used to hold, if anything. Needed after the write, not
    // before it — see `repoint`.
    let displaced = recorded.as_ref().map(|entry| entry.document_id.clone());

    // 3. The content. Two copies of one file are one document (D33), so a
    //    second path to content already chunked costs one row.
    let id = document.sha256.clone();
    // Whether the document row is already there, which decides whether slice 0
    // creates it or empties it. Read here and used inside the transaction
    // below, so the whole recovery is one atomic unit — see `rebuild` there.
    let mut rebuild = false;
    if db.document_exists(&id)? {
        if db.stage_status(&id, STAGE_CHUNK)?.as_deref() == Some(STATUS_DONE) {
            db.transaction(|_| repoint(db, root_id, relative, &id, disk, displaced.as_deref()))?;
            return Ok(Ingested::AlreadyIndexed { document_id: id });
        }
        rebuild = true;
    }

    // 4. The write.
    let pages = pages_of(&document);
    let mut next_ord = 0i64;
    let mut chunks = 0usize;
    for (slice_no, slice) in slices(&pages).enumerate() {
        let written = db.transaction(|tx| {
            if slice_no == 0 {
                if rebuild {
                    // The document is here and its chunking is not finished: a
                    // job that stopped part-way, or a stage that failed. The
                    // rows below it cannot be written beside —
                    // `UNIQUE(document_id, ord)` collides — and cannot be left,
                    // because blocks 2..n of a chunk live inside `char_span`
                    // where no foreign key reaches them. So they are cleared,
                    // and only they.
                    //
                    // The `document` row itself stays. Its id is the sha256 of
                    // the bytes just re-read, so it is not stale; and deleting
                    // it would cascade to `path` (`schema.sql:82`) and take
                    // every *other* copy of this file out of the index with it,
                    // to come back only when a walk reaches that copy again.
                    // Inside this transaction rather than before it, so a
                    // second interruption during the recovery rolls the clear
                    // back instead of leaving the document empty.
                    db.clear_document_content(&id)?;
                } else {
                    db.insert_document(&id, &document.mime, disk.size_bytes, document.source_kind)?;
                }
                repoint(db, root_id, relative, &id, disk, displaced.as_deref())?;
            }
            let mut written = 0usize;
            for page in slice {
                let page_id =
                    db.insert_page(&id, page.page_no, &document.text_source, page.section_title)?;
                // The rowids the chunker cannot invent: `Block` carries no id
                // by design, so the caller that wrote the rows passes them in.
                let mut placed: Vec<(i64, &Block)> = Vec::with_capacity(page.blocks.len());
                for block in page.blocks {
                    placed.push((db.insert_block(page_id, block)?, block));
                }
                for chunk in chunk_blocks(&placed, next_ord, &page.context) {
                    db.insert_chunk_in(
                        tx,
                        &id,
                        chunk.ord,
                        &chunk.text,
                        &chunk.locator,
                        chunk_kind(&chunk, &placed, document.source_kind),
                    )?;
                    // Carried across pages and across slices: `ord` is unique
                    // per document, not per page.
                    //
                    // This used to be untested and was recorded as such: with
                    // one page there is nothing to carry to, so `next_ord = 0`
                    // left every test in the crate green. Markdown is the
                    // reader that made it reachable, and
                    // `ord_rises_across_pages_and_across_slices` is the test —
                    // `next_ord = 0` now collides on `UNIQUE(document_id,
                    // ord)` at the second page.
                    next_ord = chunk.ord + 1;
                    written += 1;
                }
            }
            Ok(written)
        })?;
        chunks += written;
    }

    // 5. The checkpoint, and the answer to "may this be searched?".
    //
    //    Outside the write, and the two of them **together**. Outside, because
    //    a document is searchable once its rows are there and a crash before
    //    this point costs a re-index rather than a lie — the cheap arm finds no
    //    finished stage, and step 3 rebuilds.
    //
    //    Together, because that recovery is reached by finding no finished
    //    stage. Written as two autocommit statements, a crash between them
    //    leaves the stage `done` and the status `pending`, and from then on the
    //    cheap arm short-circuits on the stage every single walk: the document
    //    never comes back. It is the same defect the stage check was added to
    //    close, one statement further down, and it is why these two are one
    //    transaction rather than two adjacent lines.
    db.transaction(|_| {
        db.record_stage(&id, STAGE_CHUNK, STATUS_DONE)?;
        db.set_document_status(&id, DocumentStatus::Indexed)
    })?;

    Ok(Ingested::Indexed {
        document_id: id,
        chunks,
    })
}

/// Points `relative` at `id`, and clears away whatever it used to point at if
/// nothing else does.
///
/// The path row is rewritten rather than inserted, because it may already
/// exist: a file edited in place keeps its name and changes its content, and
/// `document.id` **is** the content, so the row has to move to a different
/// document.
///
/// The second half is the part that is easy to leave out and expensive to
/// leave out. Under D33 a document survives as long as some path names it —
/// which is what stops deleting one copy of a file from dropping a document
/// the other copy still has. Turned around, a document that no path names any
/// more is a version of a file that no longer exists, and until this cleanup
/// existed every edit left one behind: its chunks stayed in `chunk_fts`,
/// answering queries with text the file has not contained since, and citing it
/// with no path at all because the `LEFT JOIN` in `citation()` has nothing to
/// join to.
///
/// Only the document this path itself displaced is considered. A document with
/// no path row that was never named by one — one indexed from inside an
/// archive — is not this function's business and is not touched.
fn repoint(
    db: &Db,
    root_id: i64,
    relative: &str,
    id: &str,
    disk: OnDisk,
    displaced: Option<&str>,
) -> Result<(), mnema_index::Error> {
    db.delete_path(root_id, relative)?;
    db.insert_path(root_id, relative, id, disk.size_bytes, disk.mtime)?;
    if let Some(displaced) = displaced {
        // No `displaced != id` guard, and it is not an omission: the insert
        // above has just written a row naming `id`, so when the two are the
        // same the count below is at least 1 and says so. A second condition
        // that cannot change an outcome reads like a case someone thought
        // about, which is worse than no condition at all.
        forget_if_unnamed(db, displaced)?;
    }
    Ok(())
}

/// Journals one file's skip and, when [`displaces`] says so, removes what the
/// index still holds under that path — **as one transaction**.
///
/// The two used to be two transactions, and they could disagree. Forced apart
/// deliberately, the state left behind was a journal saying the file had been
/// skipped as unsupported while the lexical index went on answering with the
/// file's old text under the same filename — precisely the citation the
/// displacement exists to prevent, now with a journal entry asserting it had
/// been dealt with. One transaction makes "this file was skipped" and "what it
/// used to be is gone" a single fact that either holds or does not.
///
/// A failure here still propagates out of `ingest_file` as an `Err` and stops
/// the job, and that is this crate's contract rather than a leftover: a
/// database that cannot be written is not a per-file problem. What the
/// transaction changes is the state left behind when it happens — nothing,
/// rather than half of it.
fn record_skip(
    db: &Db,
    root_id: i64,
    relative: &str,
    reason: &str,
    rule: SkipRule,
    recorded: &Option<PathEntry>,
    on_disk: Option<OnDisk>,
) -> Result<(), mnema_index::Error> {
    db.transaction(|_| {
        db.record_skip(root_id, relative, None, reason, rule, on_disk)?;
        if let Some(recorded) = recorded
            && displaces(rule, recorded, on_disk)
        {
            db.delete_path(root_id, relative)?;
            forget_if_unnamed(db, &recorded.document_id)?;
        }
        Ok(())
    })
}

/// Deletes `document` — and, by cascade, its pages, blocks, chunks and search
/// rows, and its vectors explicitly — if no `path` row names it any more.
///
/// The count is the whole decision. D33 makes a document live as long as some
/// path names it, which is what stops deleting one copy of a file from
/// dropping the document its other copy still needs.
///
/// The single place `Db::delete_document` is called from, and that is load-
/// bearing rather than tidy: an edit that displaces a previous version
/// (`repoint`), a file that becomes unsupported (`record_skip`), and
/// reconciliation's own phase 3 (`walk.rs`) all decide "does anything still
/// name this document?" the same way, through this one function, rather than
/// each repeating the count-then-delete and each having to remember the
/// vector cleanup beside it. `Db::delete_vectors_for_document`'s own doc
/// comment has the reason that cleanup cannot be left to a cascade: a `vec0`
/// table cannot carry a foreign key, so nothing removes a document's vectors
/// on its own.
fn forget_if_unnamed(db: &Db, document: &str) -> Result<(), mnema_index::Error> {
    if db.path_count(document)? == 0 {
        db.delete_vectors_for_document(document)?;
        db.delete_document(document)?;
    }
    Ok(())
}

/// Whether this skip means the index must stop holding what it holds under
/// this path.
///
/// The question is only ever asked when a `path` row is already there, so
/// something *is* held: a version of this file that was readable when it was
/// indexed. Leaving a stale one is the worst citation this product can
/// produce — text the file no longer contains, cited under a filename that
/// exists, offering a highlight over characters that are gone. Removing one
/// that was not stale loses a document over a transient condition. Each rule
/// is on the side it is because of which of those two it risks.
///
/// **The line is what the skip is evidence *of*.** Removing content needs a
/// reproducible determination about that content. A skip that records
/// something which merely *happened* — to the worker, to the machine, to the
/// volume — is evidence about the environment, and the environment applies to
/// every file in the walk, not to this one.
///
/// That distinction is the whole of the rule, because the two errors are not
/// the same size. Keeping a stale document costs one file, is written in the
/// skip journal, and is undone by the next successful read of it. Removing
/// content over an environmental fault costs **every file the walk reaches**,
/// silently, while the progress bar advances: a worker binary that does not
/// match its parent answers the same way for all forty thousand of them.
///
/// **Displace** — `Unsupported`, `NoTextLayer`. The worker read the bytes and
/// determined something about them: there is no reader for this format, or
/// this page carries no text layer. Run it again on the same bytes and it says
/// the same thing. So what the index holds is a previous version of a file
/// that has since become unindexable.
///
/// **Keep** — `Crash`, `Timeout`, `Memory`, `Unreadable`. None of these is a
/// statement about the file:
///
/// * `Crash` — the worker died, or produced output that was not text. That is
///   *usually* a parser faulting on this file's bytes, which is why this rule
///   sat on the other side until a whole-branch review measured the other
///   reading of it: a sidecar that answers every request with raw bytes — a
///   half-finished install, a mismatched release — reports `Crash` for every
///   file it is given, and each one deleted a document. Three indexed files
///   became zero documents, zero paths and three journal rows reading "could
///   not be read". Nothing returned `Err`; the job did not stop. Note also
///   that no reader in this product can fault today: plain text and comrak
///   cannot, so **every** `Crash` reachable now is environmental.
/// * `Timeout` — a machine loaded enough to miss a 120-second deadline misses
///   it for every document, and the same file reads fine when the machine is
///   quiet.
/// * `Memory` — the out-of-memory killer chooses by size, so it keeps choosing
///   the worker, over and over, on a machine under pressure.
/// * `Unreadable` — the file could not be opened at all: missing, not a
///   regular file, refused by permissions, on a volume that is not there. A
///   share that drops mid-walk reports it for everything on the volume.
///
///   **This one is only half safe, and the other half is not built yet.**
///   Nothing anywhere removes a `path` row for a file that was renamed or
///   deleted — this function is handed one name and cannot tell "gone" from
///   "unreachable", which is exactly why it keeps. Measured: rename a file and
///   walk both names, and the old row survives every walk; `citation()` then
///   prefers it deterministically, because its `ORDER BY` picks the first path
///   alphabetically, so every citation of that document names a file the user
///   cannot open. Keeping is still the right answer here — the alternative
///   empties the index for an unplugged volume — but it is only *sufficient*
///   once the walk reaps the paths it did not find, which is the watched-folder
///   spec's to build.
///
/// The cost of keeping `Crash` on this side is real and bounded: a `.txt`
/// replaced by a PDF that genuinely faults the parser goes on answering with
/// its old text until something reads it successfully. The pool's poison
/// record means it will not even be retried within a run. That is one stale
/// file against an index; and the better long-term answer for it is not
/// deletion but `document.status = 'failed'`, which the search spec has to
/// give meaning to before anything can act on it.
///
/// A recurrence counter — stop the job when one rule fires N times in a row —
/// is the general answer to a systemically broken worker and is **not**
/// implemented here. It belongs to whoever owns the walk; this function sees
/// one file at a time and cannot count.
///
/// **`TooLarge` — decided on the size, not on the rule.** This is the one that
/// cannot be answered from the rule alone, and the reason is worth spelling
/// out because the wrong answer is the plausible one.
///
/// The worker refuses from `stat` without opening the file
/// (`crates/mnema-extract/src/bin/worker.rs`, the `max_bytes` branch), so
/// "did the content change?" is not something the refusal can say. It follows
/// from arithmetic instead. **A document only exists under this path because
/// the file was once under the ceiling.** So:
///
/// * the size on disk **differs** from `path.size_bytes` → the file was
///   rewritten, and what the index holds is the old text. Displace.
/// * the size is **equal** → the file is byte-for-byte the size it was when it
///   was indexed, and it is over the ceiling now, so the *ceiling* moved.
///   Nothing about the file changed. Keep.
///
/// There is no grey zone between those. A same-length replacement cannot fool
/// it: a file of that length that is over the ceiling now was over the ceiling
/// then, and could never have been indexed. And when the size on disk is
/// unknown — `stat` failed here while the worker's own succeeded — the
/// comparison cannot be made and nothing is removed, which is the side that
/// loses nothing.
///
/// An earlier version of this decided on the rule and kept everything
/// `TooLarge`, to protect a user who lowers `max_bytes` under an indexed file.
/// That user never reaches this code: nothing about their file changed, so the
/// cheap arm matches size, mtime and stage and answers `Unchanged` before a
/// worker is ever started. What does reach it is a file that **grew** past the
/// ceiling — and keeping that one is the stale citation above. The size test
/// serves both.
///
/// Written as an exhaustive `match` rather than `matches!`, so that a rule
/// added to `SkipRule` has to be placed on one of these sides by whoever adds
/// it.
fn displaces(rule: SkipRule, recorded: &PathEntry, on_disk: Option<OnDisk>) -> bool {
    match rule {
        // A determination about the bytes, reproducible on the same bytes.
        SkipRule::Unsupported | SkipRule::NoTextLayer => true,
        // Something that happened, and that happens to every file alike.
        SkipRule::Crash | SkipRule::Timeout | SkipRule::Memory | SkipRule::Unreadable => false,
        SkipRule::TooLarge => on_disk.is_some_and(|disk| disk.size_bytes != recorded.size_bytes),
    }
}

/// What a chunk is made of, which is not always what its document is made of
/// (D41).
///
/// A fenced block inside a markdown handbook is code; the prose around it is
/// not, and the document as a whole is a `Document`. The difference is not
/// bookkeeping — `prepare_for_search` splits run-together identifiers for
/// `SourceKind::Code` and for nothing else, so this decides whether
/// `let userName` can be found by `user`. Taking the document's kind for every
/// chunk, as this did until markdown arrived, means a code repository's
/// markdown never gets the split and a `.rs` file's every comment does.
///
/// **Majority of characters**, and a mixed chunk is the ordinary case rather
/// than an error. A one-line shell command inside a page of prose is a
/// `Document`; a chunk that is mostly a fence is `Code` even though its
/// document is not.
///
/// The rejected alternative is "any code block at all makes the chunk `Code`",
/// which is what this did for one commit. Under it a handbook whose every
/// chunk happens to hold one command line is entirely `Code`, and a query
/// scoped to documents loses the whole file — the exact confusion
/// `SourceKind` exists to prevent, arrived at from the other side.
///
/// It was also, briefly, "all or none, and panic on a mix", which only held
/// because the chunker refused to build a mixed chunk. That refusal cut the
/// prose stream at every fence and was withdrawn (`mnema_chunk::chunk_blocks`
/// carries the measurement), so there is nothing left to assert.
///
/// **What this costs**, and it is a real loss rather than a rounding: a small
/// fence inside a prose-majority chunk gets no camelCase expansion, so
/// `userName` in it answers to `userName` and not to `user`. The reverse —
/// prose inside a code-majority chunk gaining the split forms — costs nothing
/// but a few extra terms. A tie goes to the document's own kind, since
/// half-and-half is not a code chunk.
///
/// The separators between spans belong to no block and are counted for
/// neither side.
fn chunk_kind(chunk: &Chunk, placed: &[(i64, &Block)], document: SourceKind) -> SourceKind {
    let mut code = 0usize;
    let mut total = 0usize;
    for span in &chunk.locator.spans {
        let block = placed
            .iter()
            .find(|(id, _)| *id == span.block_id)
            .map(|(_, block)| *block)
            .expect("a chunk's spans name blocks this page just wrote");
        let chars = (span.end - span.start) as usize;
        total += chars;
        if block.block_type == BlockType::Code {
            code += chars;
        }
    }
    if code * 2 > total {
        SourceKind::Code
    } else {
        document
    }
}

/// The slices of pages the write loop runs over — **always at least one**,
/// even when the document has no pages at all.
///
/// `[T]::chunks` yields nothing for an empty slice, and slice 0 is where
/// `insert_document` and `repoint` live. So a reader that reported no pages
/// skipped the entire write while step 5 ran anyway: `Indexed { chunks: 0 }`
/// came back naming a `document_id` no row carried, `set_document_status`
/// updated nothing, an orphan `ingest_stage` row was minted — feeding exactly
/// the defect the cascade on that table now closes — and the index went on
/// citing the file's previous text under its own name.
///
/// A reader that produced no pages did read the file, so the honest record is
/// a document with zero pages whose path points at it, not a silently skipped
/// write. That is also somewhere for whoever adds PDF to hang the per-page
/// `skipped` rows Requirements §13 asks for when every page of a scan turns
/// out to have no text layer.
///
/// An empty first slice rather than hoisting `insert_document` and `repoint`
/// out of the loop, which was the other way to reach the same outcome. Hoisting
/// would put them in a transaction of their own, and both of the things slice 0
/// does are only safe *because* they share a transaction with the writes that
/// follow: `clear_document_content` must roll back with them, or an interrupted
/// rebuild leaves a document emptied, and `repoint` must roll back with them,
/// or the path row carries the new size and mtime over content that was never
/// replaced — and the cheap arm then answers `Unchanged` for ever.
///
/// Today no reader can produce this: plain text always emits one page, and
/// markdown starts from one. A worker of the wrong version can, and a PDF whose
/// every page was skipped for having no text layer will.
fn slices<'p, 'a>(pages: &'p [PageOf<'a>]) -> impl Iterator<Item = &'p [PageOf<'a>]> {
    let mut chunks = pages.chunks(PAGES_PER_TRANSACTION);
    let first = chunks.next().unwrap_or(&[]);
    std::iter::once(first).chain(chunks)
}

/// One page's worth of what the writer needs: the blocks on it, and what the
/// chunker cannot see about it.
struct PageOf<'a> {
    page_no: i64,
    section_title: Option<&'a str>,
    blocks: &'a [Block],
    context: PageContext,
}

/// The extracted document's own pages, in the form the write loop wants.
///
/// A straight map, and it is only that because the wire carries a page marker:
/// until it did, this function had to refuse anything with more than one page
/// rather than guess which block belonged to which — every block of a
/// thousand-page document on page 1 is a state the schema accepts without
/// complaint and no test would notice.
///
/// `PageContext::Lines` for every reader that exists today: txt and markdown
/// both have line numbers, and the chunker computes each chunk's coordinate
/// from the blocks it actually covers. A PDF page will want
/// `PageContext::Fixed(Coordinate::Page …)` and will therefore have to decide
/// this per reader — the information is `document.mime`'s to give, not this
/// loop's to assume, and there is nothing to decide between yet.
fn pages_of(document: &Document) -> Vec<PageOf<'_>> {
    document
        .pages
        .iter()
        .map(|page| PageOf {
            page_no: i64::from(page.page_no),
            section_title: page.section_title.as_deref(),
            blocks: &page.blocks,
            context: PageContext::Lines,
        })
        .collect()
}

// `OnDisk`, `stat` and `mtime_nanos` used to live here. Retired: `OnDisk`
// itself is `mnema_core::OnDisk` (the shared-types crate both this crate and
// `mnema-index` already depended on, D45), and the measurement is
// `mnema_walk::stat` — the walk is the only place that looks at the disk, and
// this crate now only ever compares the numbers it is handed (§5).

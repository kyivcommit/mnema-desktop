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
use mnema_core::manifest::{
    Manifest, READER_DOCX, READER_EPUB, READER_HTML, READER_PDF, READER_XLSX,
};
use mnema_core::{Block, BlockType, Coordinate, OnDisk, SourceKind};
use mnema_index::{Db, DocumentStatus, INDEX_FORMAT_VERSION, PathEntry, SkipRule};
use mnema_pool::{Document, Outcome, Pool, PoolError};

mod walk;
pub use walk::{Frozen, FrozenReason, StopReason, WalkProgress, WalkReport, walk_root};

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
    /// ceiling, on whether the size or the modification time moved.
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
///    the file on disk, the reader that made it still the one `manifest` gives
///    its extension, **and** its document's chunking recorded as finished,
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
///
/// `manifest` is what the *worker binary* says its readers are, not a constant
/// this crate could link — D40 forbids depending on `mnema-extract`, which is
/// the whole reason the answer arrives as data. It must come from the same
/// executable the `pool` sends files to (`Pool::manifest` asks that one), and
/// it must be one answer for the whole pass: a caller that re-asked between
/// files would let the parent disagree with itself about which build it is
/// running. Step 1 has what it decides and what it cannot.
pub fn ingest_file(
    pool: &Pool,
    db: &Db,
    root_id: i64,
    absolute: &Path,
    relative: &str,
    on_disk: Option<OnDisk>,
    manifest: &Manifest,
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
    //
    //    The fourth condition is the one that is not about the file at all.
    //    The other three ask whether the bytes moved; this asks whether the
    //    code that read them did. Without it, a document made by a reader this
    //    build no longer has — or by the default arm, before its format got a
    //    reader of its own — answers `Unchanged` for the life of the index. The
    //    case it was built for is `.html`: it is read by the text reader today,
    //    because `identify_plain_text` has no arm for it, so a manifest of
    //    reader *versions* would compare `text@1` against `text@1` and never
    //    re-read a single already-indexed page on the day an html reader
    //    arrives. That is why the manifest is keyed on the extension.
    //
    //    It is checked before the stage, which costs a query, and after the two
    //    numbers, which are already in hand.
    //
    //    **Its first pass over an existing index re-reads every `.md` once.**
    //    Migration 3 credits every row it finds to `text@1` — it cannot do
    //    better, because nothing was recorded about who made them — while the
    //    markdown reader shipped in `fb3a924`, so every markdown row disagrees
    //    with the manifest exactly once and then agrees for ever. That is the
    //    migration's cost, not a fault in this condition, and lowering the
    //    condition to hide it would give up the whole mechanism.
    //
    //    **What it cannot do is tell "the prediction changed" from "the
    //    prediction was never right for this file", and that is a residual with
    //    a name.** `manifest.for_extension` predicts from the extension, while
    //    the row records what actually ran; for a file whose content
    //    contradicts its extension those are two different readers, and the
    //    condition then misses on every walk — the file is handed to a worker,
    //    the worker reports the same reader again, and the next walk asks the
    //    same question. No logic here separates the two cases: the row holds
    //    the truth, and the prediction it should be compared against was never
    //    stored.
    //
    //    Unreachable in this build, and not by luck: every reader `identify`
    //    picks by magic bytes is refused by the worker today (`Unsupported`,
    //    `NotText`, `BinaryTail`), and a refusal writes no `path` row at all —
    //    so every row that exists was made by the extension-deciding branch,
    //    which is exactly what the manifest predicts. The two branches of the
    //    worker are held to agreeing by
    //    `the_manifest_names_the_reader_that_identify_actually_picks`
    //    (`crates/mnema-extract/tests/manifest.rs`), and a sidecar that is not
    //    this worker answers `--manifest` for its own readers, so even a
    //    mismatched binary agrees with itself.
    //
    //    It becomes reachable with the first reader chosen by content — a PDF
    //    under the name `report.md` is then indexed by the pdf reader while the
    //    manifest predicts markdown for it. What that costs is one worker
    //    process and one path-row rewrite per walk, and nothing else: the
    //    document is content-addressed, so the re-read lands on `AlreadyIndexed`
    //    with no rebuild, no chunk id moved and nothing journalled
    //    (`a_reader_no_build_agrees_on_is_re_read_every_pass_and_costs_only_that`
    //    in `tests/slice.rs` is that bill, measured). It is not silent either —
    //    such a file counts as `indexed` in every `WalkReport`, on a folder
    //    where nothing changed. What would close it is a fact stored per path
    //    saying what the prediction was when the row was written, or a manifest
    //    that can say "this reader is chosen by content, not by extension":
    //    a column or a wire field, and a decision of its own rather than a line
    //    in this arm.
    let recorded = db.path_entry(root_id, relative)?;
    let expected = manifest.for_extension(extension_of(relative));
    if let Some(disk) = on_disk
        && let Some(recorded) = &recorded
        && recorded.size_bytes == disk.size_bytes
        && recorded.mtime == disk.mtime
        && recorded.reader == expected.reader
        && recorded.reader_version == i64::from(expected.version)
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
    //
    // **And it may only answer when there is nothing left to decide.** This
    // `return` used to stand ahead of `record_skip` and `displaces` both, so a
    // remembered refusal never displaced anything: the removal was reached only
    // when a worker had actually been asked. Restore a photo over a note's name
    // with its own modification time and this arm matched the journal, the pool
    // was never asked, and the index went on answering under that name with
    // prose the file no longer contains — with nothing in `WalkReport` saying
    // so. Measured at `walk_root`'s own level, over three full walks:
    // `{ found: 1, indexed: 0, skipped: 1, removed: 0, stopped: Completed }`,
    // one lexical hit, the `path` row still naming the note.
    //
    // `displaces` with **no digest** is the question this arm has to ask,
    // rather than a predicate of its own: nothing here read the file, so
    // `None` is the truth about what is known, and `None` is exactly the input
    // that makes `displaces` answer "would this rule take the document away if
    // the bytes could not be identified?". Yes means fall through and let a
    // worker settle it properly; the rules that keep — `BinaryTail` above all,
    // whose whole point is that it must not displace — still short-circuit and
    // still cost no process. Self-limiting, too: once a fall-through displaces,
    // there is no `path` row left and the next walk short-circuits again. The
    // folder of scans this arm was built for has no `path` rows at all, so it
    // pays nothing.
    if let Some(disk) = on_disk
        && let Some(skip) = db.skip_entry(root_id, relative)?
        && skip.format_version == INDEX_FORMAT_VERSION
        && skip.size_bytes == Some(disk.size_bytes)
        && skip.mtime == Some(disk.mtime)
        && !recorded
            .as_ref()
            .is_some_and(|entry| displaces(skip.rule, entry, on_disk, None))
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
            Refusal {
                rule,
                reason: "the walk could not measure this file, so its size and mtime are unknown",
                // Nothing read the file — this arm is reached because the walk
                // could not even stat it.
                content: None,
            },
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
                Refusal {
                    rule,
                    reason: &skip.reason,
                    content: skip.sha256.as_deref(),
                },
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
    // **Whether the rows under this document were made by the reader that just
    // ran**, which is a different question from whether the bytes moved — and
    // the one the content address cannot answer. `document.id` is the sha256 of
    // the file, so a release that gives an extension a new reader changes every
    // page, block and chunk of it while leaving the id exactly where it was.
    //
    // Without this the manifest's whole mechanism stopped one step short. The
    // cheap arm above notices that `.html` changed hands and hands the file to a
    // worker; the worker reads it as prose; and the branch below then finds the
    // document present with its chunk stage `done` and returns having written
    // nothing — so the text reader's markup stays in the index. Worse, `repoint`
    // writes the new reader into the `path` row in the same transaction, the
    // next walk's cheap arm agrees, and the stale reading is there for the life
    // of that index. `INDEX_FORMAT_VERSION` does not reach it either: that lever
    // is read only by the skip journal's arm, and an indexed file is not a skip.
    // Measured before this line existed, in
    // `a_file_indexed_by_another_reader_is_rebuilt_rather_than_left_as_it_was`.
    //
    // Compared against the reader that **ran**, not against
    // `manifest.for_extension`: the manifest is a prediction, and what makes the
    // stored rows stale is which reader actually made them. That also keeps the
    // arm quiet in the case the manifest cannot predict — a reader chosen by
    // content disagrees with the map on every walk and would otherwise rebuild
    // the document, and move every chunk id, on every walk.
    //
    // **What it cannot see is a document reached by a path with no row of its
    // own.** Two paths to identical bytes are one document (D33), and the reader
    // is recorded per path; walking the copy that was never indexed answers
    // "nothing to compare" and leaves the old rows standing until the walk
    // reaches the path that does have a row. Closing that needs the reader
    // recorded on the `document` row, which is a column and a migration rather
    // than a line here.
    let stale_reading = recorded.as_ref().is_some_and(|entry| {
        entry.reader != document.reader
            || entry.reader_version != i64::from(document.reader_version)
    });
    if db.document_exists(&id)? {
        if !stale_reading && db.stage_status(&id, STAGE_CHUNK)?.as_deref() == Some(STATUS_DONE) {
            // Nothing is written here, and that governs the journal too. The
            // path's account of its missing pages is rewritten **only when the
            // path is coming to name a different document**: this pass extracted
            // the file but wrote none of its pages, so for a path that already
            // named this document neither the pages nor the pages missing from
            // them have moved, and the rows that are there were written by the
            // pass that did write them. `journal_skipped_pages` has what
            // rewriting them anyway costs — a true row deleted, silently, on a
            // release that improves a reader.
            let renaming = displaced.as_deref() != Some(id.as_str());
            db.transaction(|_| {
                repoint(db, root_id, relative, &document, disk, displaced.as_deref())?;
                if renaming {
                    journal_skipped_pages(db, root_id, relative, &document)?;
                }
                Ok(())
            })?;
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
                repoint(db, root_id, relative, &document, disk, displaced.as_deref())?;
                // Unconditional here, unlike the `AlreadyIndexed` branch: this
                // pass is the one writing the pages, so its account of which
                // ones are missing is the account, whatever the path named
                // before and whichever reader wrote the rows that are there.
                journal_skipped_pages(db, root_id, relative, &document)?;
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
///
/// **The third thing it does is forget the journal's refusal of this path**,
/// and it is here rather than beside the two calls that reach it because both
/// of them are already inside the transaction the write happens in — the whole
/// argument `record_skip` makes for its own pair, in the other direction. A
/// document written and a refusal still standing for the same name are not two
/// facts that may disagree; they are one fact, and it either holds or it does
/// not.
///
/// Left out, the row was not merely untidy. It is what the window answering
/// "why is this file not in my index?" lists, so that list named files that
/// **are** in it. And it stayed a live verdict for `ingest_file`'s second cheap
/// arm, which compares `(size, mtime, format_version)` and never asks whether
/// the verdict was reached on these bytes: restore a previous version with its
/// own modification time and the stale row matched the disk again, answering
/// for a file no worker had looked at since — under a `path` row now naming
/// something else entirely.
/// **The fourth thing it writes is which reader made this document**, and it
/// takes the whole [`Document`] rather than a content hash so that the reader
/// and the hash cannot come from two different extractions. Spread out as
/// arguments they are three strings in a row that a caller is free to pair
/// wrongly, and nothing downstream would notice: the row would name a real
/// document and credit a reader that never touched it, which reads as
/// "unchanged" against a manifest for ever. It is the argument
/// `Db::insert_block` already makes for taking a `Block`, one level up.
///
/// The pages missing from the document it now names are the other half of the
/// third fact, and they are **not** settled here — [`journal_skipped_pages`] is
/// where, and its doc comment has why the two could not be one call.
fn repoint(
    db: &Db,
    root_id: i64,
    relative: &str,
    document: &Document,
    disk: OnDisk,
    displaced: Option<&str>,
) -> Result<(), mnema_index::Error> {
    db.delete_path(root_id, relative)?;
    db.insert_path(
        root_id,
        relative,
        &document.sha256,
        disk,
        // What the worker said, never a value derived here. `mnema-pool`
        // has already refused a header naming no reader, so this is the one
        // place the column's meaning is established — the `NOT NULL` on it
        // is satisfied by `""` and would establish nothing.
        &document.reader,
        i64::from(document.reader_version),
    )?;
    db.forget_skip(root_id, relative)?;
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

/// Rewrites the skip journal's account of which pages are missing from the
/// document `relative` now names.
///
/// Delete-then-write rather than an upsert per page: an upsert leaves behind
/// exactly the rows this exists to remove — the ones the current account does
/// *not* name. Nothing else in the tree reaches a per-page row for a path a
/// walk still finds (`Db::forget_page_skips` carries the case), so a row left
/// standing here is left standing for the life of the index.
///
/// **A row is written only for a page the index does not hold**, and that
/// filter is the whole reason this is a function rather than four lines inside
/// [`repoint`]. `document.skipped_pages` is what the reader that just ran said
/// about these bytes; the pages in the index were put there by whatever reader
/// ran when the document was first extracted, and a release can change one
/// without changing the other. Written unfiltered, a build whose reader newly
/// drops a page would journal "page 5 has no text layer" about a page this
/// index holds and cites on demand — which is the contradiction `mnema_pool`'s
/// `run_one` stops the whole job over when a worker sends it, arriving through
/// the database door instead and accepted in silence.
///
/// On the write path the filter cannot fire and is not there for that path: the
/// pages are inserted *after* this runs, a rebuild has just cleared them, and
/// the pool has already refused a summary naming a page that also arrived. It
/// is the `AlreadyIndexed` caller it exists for.
///
/// **Its other half is the caller's**, because this function cannot see it:
/// `ingest_file` calls it on the `AlreadyIndexed` branch **only when the path
/// did not already name this document**. A path that keeps naming the same
/// document over a pass that wrote nothing has had neither its pages nor its
/// missing pages changed by that pass, and rewriting the rows from a newer
/// reader's account would delete a true one — a reader that learns to read page
/// 2 makes the row for page 2 disappear while page 2 is still absent from the
/// index, and `repoint` writes the new `reader_version` into the `path` row in
/// the same transaction, so the cheap arm agrees from then on and nothing ever
/// re-reads the file. That is a true fact deleted silently on an ordinary
/// release upgrade, and it is why the branch that writes no pages writes no
/// rows about them either.
///
/// What is left over is named rather than hidden: a path that comes to hold
/// content **already** in the index under another name gets rows from today's
/// reader for the pages the index lacks, and if today's reader reads a page the
/// old extraction dropped, nothing under this path records that the index is
/// still missing it. That is under-reporting where the alternative is a false
/// row, and the choice is not close. Both are cleared by the re-extraction
/// `INDEX_FORMAT_VERSION` forces.
fn journal_skipped_pages(
    db: &Db,
    root_id: i64,
    relative: &str,
    document: &Document,
) -> Result<(), mnema_index::Error> {
    db.forget_page_skips(root_id, relative)?;
    let held = db.indexed_page_numbers(&document.sha256)?;
    for page_no in &document.skipped_pages {
        let page_no = i64::from(*page_no);
        if held.contains(&page_no) {
            continue;
        }
        db.record_skip(
            root_id,
            relative,
            Some(page_no),
            // Written here rather than carried on the wire: `Frame::Summary`
            // sends numbers, and the threshold that decided them belongs to
            // `mnema-extract`, which this crate may not depend on (D40). So the
            // sentence says which page and what is missing, and does not invent
            // a number it cannot see.
            &format!(
                "page {page_no} of this document carries no text layer, so it \
                 was read as a scan and left out"
            ),
            SkipRule::NoTextLayer,
            // **No measurement, although the rule is one `record_skip` stores
            // one for.** What this drops is `size_bytes` and `mtime`, and
            // nothing else: `format_version` is in that statement's `params!`
            // unconditionally and is written either way. Both describe the file
            // the walk stat'ed, and their only reader is `skip_entry`, which
            // takes `page_no IS NULL` — so on a page's row they would be
            // written, never read, and left to go stale, in two columns that
            // read as though they described the page.
            None,
        )?;
    }
    Ok(())
}

/// One file's refusal, as [`record_skip`] needs it: the rule that fired, a
/// sentence a person can read, and the digest of the bytes the verdict was
/// reached on.
///
/// A struct rather than three more parameters because the three always travel
/// together and arrive from the same place — `mnema_pool::Skip` — and because
/// `content` is the one whose absence changes what happens to a document.
/// Passed loose, it is a bare `Option<&str>` in eighth position, which is
/// exactly the shape a caller gets wrong.
struct Refusal<'a> {
    rule: SkipRule,
    reason: &'a str,
    /// The sha256 of what the worker read. `None` when nothing read the file:
    /// the size ceiling refused it from `stat`, the worker died, or this crate
    /// answered without a worker at all.
    content: Option<&'a str>,
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
    refusal: Refusal<'_>,
    recorded: &Option<PathEntry>,
    on_disk: Option<OnDisk>,
) -> Result<(), mnema_index::Error> {
    let Refusal {
        rule,
        reason,
        content,
    } = refusal;
    db.transaction(|_| {
        db.record_skip(root_id, relative, None, reason, rule, on_disk)?;
        if let Some(recorded) = recorded
            && displaces(rule, recorded, on_disk, content)
        {
            db.delete_path(root_id, relative)?;
            forget_if_unnamed(db, &recorded.document_id)?;
            // The pages that were missing from the document this path held are
            // a fact about that document, so they go when it does — and only
            // then. `displaces` is the condition rather than a second rule of
            // its own: on the keeping side the document is still in the index
            // and still missing those pages, and removing the rows would delete
            // the explanation over an event that says nothing about the file.
            //
            // Without it the rows are unreachable. A refusal writes no `path`
            // row, so `repoint` — the only other place that maintains them —
            // never runs for this path again, and `forget_skips_not_in` fires
            // only for paths a walk stops finding. The window answering "why is
            // this not in my index?" would go on naming a missing page of a
            // document the index does not hold.
            db.forget_page_skips(root_id, relative)?;
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
/// **Displace, but only when the bytes moved** — `NotText`, `Unsupported`,
/// `Malformed`, `Encrypted` and `NoTextLayer`. The worker read the file and
/// determined something about its bytes: they are not text at all, no reader in
/// this product can take that format, the right reader could not finish them,
/// they are behind a password, or no page of them carries a text layer worth
/// indexing. Run it again on the same bytes and it says the same thing, so what
/// the index holds is a previous version of a file that has since become
/// unindexable — *if* the bytes are not the ones the index was built from. When
/// they are, the rule changed and the file did not, and deleting the document
/// loses text that is still on disk. The digest the worker refused on is what
/// tells the two apart; the arms below carry the reasoning, and D51 the
/// measurement.
///
/// Four of those five are refusals a *release* can reverse — a reader arrives,
/// a reader gets better at damage, a password prompt is built, a threshold
/// moves — and that is what puts them here rather than on the unconditional
/// side. `NotText` is the exception that shows the condition is not free: it
/// promises the opposite, that no release makes a photo into prose, and it is
/// conditional anyway, because the file under the path can be replaced by one
/// whose bytes the index never saw.
///
/// **Nothing displaces outright.** `NoTextLayer` did, on the strength of being
/// dormant — no wire string reached it and no reader could earn it. The PDF
/// reader earns it now, and it turned out to belong with the four above rather
/// than apart from them; its arm carries why.
///
/// **Keep** — `Crash`, `Timeout`, `Memory`, `Unreadable`, and `BinaryTail`.
/// The first four are not statements about the file at all; the fifth is one,
/// and keeps anyway, which is why it has a section of its own below.
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
/// * `Unreadable` — nothing was learned about the content, for a reason that is
///   not about the content. Usually the file could not be opened at all:
///   missing, not a regular file, refused by permissions, on a volume that is
///   not there. A share that drops mid-walk reports it for everything on the
///   volume. It also covers a reader that could not be started, and cases where
///   no worker ran at all — including the arm in this very function that
///   refuses a file the walk could not measure. `SkipRule::Unreadable`
///   enumerates them, and deliberately neither summarises nor counts them,
///   since every summary so far has been narrower than the rule and the first
///   count was short. What they share is what puts them on this side: one
///   condition outside the file, answering for every file alike.
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
/// **`BinaryTail` — a determination about the bytes that keeps anyway.** The
/// only rule on the keep side that the paragraph above does not cover, and the
/// exception is deliberate: "reproducible determination about the content" is
/// necessary for displacing, and this is where it turns out not to be
/// sufficient.
///
/// The worker read the file, found it text for its first bytes and binary
/// afterwards, and will say the same thing again on the same bytes. Every part
/// of the displacing argument holds — except its conclusion. That conclusion
/// runs "so what the index holds is a previous version of a file that has since
/// become unindexable", and here the file has *not* been replaced: it is the
/// same note, still opening with the same prose, with zeros appended where an
/// append was interrupted. Deleting the document does not remove a stale
/// citation, it removes text that is still on disk in front of the damage and
/// is readable nowhere else — a real class for this product's owner, not a
/// hypothetical one, and one that does not repair itself, because the verdict
/// is reproducible and the next walk answers from the journal.
///
/// **What this trades against is a residual risk, not a bounded one, and the
/// boundary this paragraph used to claim is not one.** `HEAD_BYTES`
/// (`mnema_extract::typing`) is 512 bytes, so a file earning this rule opened
/// with 512 bytes of something without a NUL in it. That is all it says. This
/// arm is a constant: it reads neither `recorded` nor `on_disk` nor a digest,
/// so nothing here ties the file now on disk to the document that stays. A
/// `.txt` **overwritten** by high-entropy bytes — an encrypted or compressed
/// blob — earns `BinaryTail` whenever its first NUL happens to fall past 512,
/// and then the index goes on citing text the file no longer contains.
///
/// Measured, because "how often" is the whole question:
///
/// * uniformly random bytes: **13.46%** (26,917 of 200,000) have their first
///   NUL at or past 512, against `(255/256)^512` = 13.48% analytically;
/// * files through `openssl enc -aes-256-cbc`: **2 of 8**;
/// * real binaries with a header — `/usr/lib`, `/usr/bin`, `/opt/homebrew`,
///   system fonts, with the magic-number formats excluded: **0 of 2,571**.
///
/// So the risk is concentrated on content that is high-entropy from its first
/// byte and carries no recognisable header, and there it is roughly one in
/// seven. It does not repair itself: `SkipRule::BinaryTail.is_about_content()`
/// is true, so the second cheap arm answers from the journal until the file's
/// size or mtime moves.
///
/// This is the same kind of known consequence as the one `classify` records
/// against the other side of the split — a `find -print0` list, genuine text,
/// refused *with* displacement because its first NUL is at byte 23. The two
/// bound the split from either end, and both are accepted rather than fixed.
///
/// A digest does not settle it and adding one here would be a false comfort:
/// an interrupted append and an overwriting blob **both** change the file's
/// bytes, so the condition `NotText` uses cannot separate them. What would
/// separate them is a stored digest of the file's *head*, which is a new
/// column and a decision of its own, not a line in this function.
///
/// **`TooLarge` — decided on what the walk measured, not on the rule.** This is
/// the one that cannot be answered from the rule alone, and the reason is worth
/// spelling out because the wrong answer is the plausible one.
///
/// The worker refuses from `stat` without opening the file
/// (`crates/mnema-extract/src/bin/worker.rs`, the `max_bytes` branch), so "did
/// the content change?" is not something the refusal can say, and **there is no
/// digest to compare — not by omission but by construction.** What is left is
/// the same pair the cheap arm above trusts for exactly this question:
///
/// * the size on disk **differs** from `path.size_bytes` → different length,
///   so certainly different bytes. The index holds the old text. Displace.
/// * the size is equal and the **modification time differs** → the file was
///   written to since it was indexed, or it was not; from here those two are
///   the same observation. Displace, because the two errors are not the same
///   size — see below.
/// * size **and** modification time both match → this is the pair `ingest_file`
///   itself takes as "nothing happened to this file", so taking it as anything
///   else here would be the same product disagreeing with itself one screen
///   apart. Keep. It is reached only when the cheap arm missed on the *stage*
///   instead — an interrupted job — and `a_lowered_ceiling_keeps_what_it_still_
///   recognises` (`tests/slice.rs`) is the test that stands on it.
/// * the size on disk is **unknown** — `stat` failed here while the worker's
///   own succeeded — nothing is removed, which is the side that loses nothing.
///
/// **An earlier version of this compared the size alone, and argued there was
/// no grey zone: "a same-length replacement cannot fool it — a file of that
/// length that is over the ceiling now was over the ceiling then, and could
/// never have been indexed." That argument refutes itself,** and one branch
/// review measured it doing so. It excludes the grey zone by assuming the
/// ceiling never moved, in the middle of a rule whose whole purpose is the case
/// where it did: `max_bytes` is a setting. Lower it under an indexed file, then
/// rewrite that file in place without changing its length, and every clause
/// lines up — the cheap arm misses on the modification time, the pool answers
/// `TooLarge` from `stat`, the size matches `path.size_bytes` — and the
/// document stays. Measured: the old text goes on being found, the new text
/// never is, and every later pass repeats it.
///
/// **What is left over is named rather than hidden.** A replacement of the same
/// length carrying the *same* modification time — one `cp -p` from a file of
/// that size — still passes, and nothing in this function can catch it: the
/// refusal never opened the file, so there is no reading of the content to
/// compare, and both halves of the evidence say "unchanged". That is the size
/// ceiling's own residual risk, of the same kind as `BinaryTail`'s above and
/// bounded by the same thing — a stored digest of the file's head, which is a
/// column and a decision of its own.
///
/// **And the price of the middle case is real, so it is stated too.** A file
/// merely *touched* — a `touch`, a restore that rewrites the timestamp, a sync
/// client — under a ceiling that has since dropped below it now loses its
/// document, although its bytes never moved. That is a loss this rule chooses,
/// against a stale citation it refuses, and the choice is not close. Both are
/// undone by the same event, the ceiling moving back up, so neither is
/// permanent; but for as long as they last, the loss is a file missing from the
/// index with a `too_large` row in the journal saying exactly why, and the
/// stale citation is this product answering a question with text that is not in
/// the file, over a highlight into characters that are gone. It is also the
/// same default `NotText` already takes one arm up, for the same stated reason:
/// where the bytes cannot be identified, displace, and keep a lost measurement
/// loud rather than quiet.
///
/// The user who lowers `max_bytes` under a file they have not touched is not
/// the one paying it, and never was: nothing about their file changed, so the
/// cheap arm matches size, mtime and stage and answers `Unchanged` before a
/// worker is ever started. That premise is asserted directly, in the first half
/// of `a_lowered_ceiling_is_not_even_asked_about_an_untouched_file`.
///
/// Written as an exhaustive `match` rather than `matches!`, so that a rule
/// added to `SkipRule` has to be placed on one of these sides by whoever adds
/// it.
fn displaces(
    rule: SkipRule,
    recorded: &PathEntry,
    on_disk: Option<OnDisk>,
    content: Option<&str>,
) -> bool {
    match rule {
        // A determination about the bytes — but only about *these* bytes. A
        // file whose content is byte-identical to what the index was built
        // from has nothing to displace: the rule changed, the file did not,
        // and deleting the document would lose text that is still on disk.
        // `TooLarge` is conditional for the same reason against a different
        // measure (its own doc comment has the case that taught it).
        //
        // `is_none_or`, and deliberately the opposite default to `TooLarge`'s
        // `is_some_and` below, because the two unknowns are not the same
        // unknown. There, a missing size is this crate's own `stat` failing
        // under a worker whose `stat` succeeded — an environment fault, and
        // nothing is removed on those. Here, a missing digest can only be a
        // worker that predates the field, which is what `#[serde(default)]` on
        // `Frame::Refused` exists for; behaving as the release that worker came
        // from behaved is what its refusal meant, and that release displaced.
        // No in-tree path reaches here with `None`: every skip synthesised
        // without a worker carries `Unreadable`, `Crash`, `Timeout` or
        // `Memory`, and the ceiling carries `TooLarge`.
        //
        // It also keeps a lost digest loud rather than quiet. Two mutation
        // cases catch the pool dropping the field precisely because `None`
        // still displaces; under `is_some_and` both would pass while the
        // migration of an already-poisoned index silently stopped working —
        // and a photo that keeps answering under a note's name is the defect
        // this whole rule was added to end.
        SkipRule::NotText => content.is_none_or(|sha| sha != recorded.document_id),
        // The same condition, on its own line rather than folded in with the
        // one above, because the two rules are refused by different branches of
        // the worker and each is worth being able to break on its own.
        //
        // It arrived later than `NotText`'s and that order was an inversion:
        // the *stable* rule was made conditional first while the *unstable* one
        // stayed unconditional. `NotText` promises "no release adds a reader
        // that makes them prose" (`crates/mnema-extract/src/bin/worker.rs`);
        // this one says "no reader implemented yet" in the same file, which is
        // the thing a release exists to change. A folder of PDFs indexed
        // through a future reader, walked once by a build that has it and once
        // by one that does not, is a document lost per file — with the bytes
        // never having moved.
        SkipRule::Unsupported => content.is_none_or(|sha| sha != recorded.document_id),
        // The same condition again, and again on lines of their own rather
        // than folded in above, for the reason `Unsupported` gives: each is
        // refused by a different branch of a reader and each is worth being
        // able to break on its own.
        //
        // They are here for the *same argument* as `Unsupported` rather than by
        // analogy with it, and the argument is the one the ordering above
        // records: a rule belongs on the conditional side when a release can
        // change the verdict without the file changing. Damage is exactly that
        // — what a reader survives is what a vendored library's next version
        // alters — and so is a password, because "cannot open this" becomes
        // "ask for a key" the day a prompt is built. A folder walked once by a
        // build whose reader recovers and once by a build whose reader gives up
        // would otherwise be a document lost per file, with the bytes never
        // having moved: the identical loss `Unsupported`'s own arm was
        // corrected for, arriving from two more directions.
        SkipRule::Malformed => content.is_none_or(|sha| sha != recorded.document_id),
        SkipRule::Encrypted => content.is_none_or(|sha| sha != recorded.document_id),
        // The same condition once more, and on a line of its own for the same
        // reason as the two above.
        //
        // The comment this replaces asked whoever built the PDF reader to
        // decide this rather than inherit it, because the rule was dormant —
        // no wire string mapped to it and nothing could earn it. The reader is
        // built now, and the decision is that it belongs on this side.
        //
        // As a *file-level* verdict this rule means every page of the document
        // fell below `TEXT_LAYER_MIN_CHARS`, which is a threshold this product
        // picked and may move, read by a library a release may improve. So it
        // is the least stable verdict of the four, not the most: a folder of
        // scans walked once by a build that found text on the page and once by
        // a build that did not is a document lost per file, with the bytes
        // never having moved.
        //
        // It is a rule about a page as well as about a file, and the two
        // meanings do not conflict here. This function is only ever asked
        // about the whole-file row: a page's own row carries `page_no` and is
        // never a verdict on the path (`Db::skip_entry` reads `page_no IS
        // NULL`, `Db::forget_page_skips` is what maintains the others).
        SkipRule::NoTextLayer => content.is_none_or(|sha| sha != recorded.document_id),
        // Something that happened, and that happens to every file alike.
        SkipRule::Crash | SkipRule::Timeout | SkipRule::Memory | SkipRule::Unreadable => false,
        // The one refusal by content that keeps: the file still opens with the
        // prose the index holds, and that prose is readable nowhere else.
        SkipRule::BinaryTail => false,
        // Both halves of the pair, not the size alone: a same-length rewrite in
        // place is invisible to the size and plain to the modification time,
        // and the size alone kept the old text for it on every later pass. See
        // the section on this rule above for what the remaining gap is and why
        // it cannot be closed here.
        SkipRule::TooLarge => on_disk.is_some_and(|disk| {
            disk.size_bytes != recorded.size_bytes || disk.mtime != recorded.mtime
        }),
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

/// The extension the manifest is keyed on.
///
/// Taken from `relative`, although the worker takes its own from the absolute
/// path it was handed: `absolute` is the root joined to `relative`, so the two
/// share a last component, and `relative` is the string the row itself is keyed
/// on. `Path::extension` on both sides rather than a hand-rolled split, so a
/// name with no dot, a dotfile, and a name ending in a dot are answered here
/// exactly as the worker answers them.
///
/// `Option`, and the `None` is not a gap: `Manifest::for_extension` sends it to
/// the same default arm an unlisted extension goes to, mirroring
/// `identify_plain_text`, where a file with no extension and a file with an
/// unrecognised one fall to one `_ =>`.
fn extension_of(relative: &str) -> Option<&str> {
    Path::new(relative).extension().and_then(|ext| ext.to_str())
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
/// **The `PageContext` is chosen by the reader that made the document**, and
/// until it was, a citation from any format without line numbers carried no
/// coordinate at all.
///
/// `PageContext::Lines` used to go on every page unconditionally, which is
/// right for exactly the two readers that existed then: txt and markdown both
/// have line numbers, and the chunker computes each chunk's range from the
/// blocks it actually covers. For a block *without* them the chunker answers
/// `Coordinate::None` (`crates/mnema-chunk/src/lib.rs`'s `line_range`) — so
/// pdf, html, docx and epub, none of which has a line to name, would every one
/// of them have been cited with nothing, silently and without an error
/// anywhere. No test would have seen it either: they are all over txt and md.
///
/// `document.reader` rather than `document.mime`, which is what this comment
/// used to promise. The reader is the record of how the file *was* read — the
/// worker states it in the header, in the branch that ran
/// (`crates/mnema-extract/src/bin/worker.rs`) — while a mime can be shared by
/// two readings and is derived from the same decision anyway. It is also the
/// string the `path` row stores, so a document and the coordinates of its
/// chunks are keyed on one fact rather than two that can disagree.
///
/// **A reader whose name is not among the arms falls to `Lines` without
/// complaint.** That is the right default — a build that adds a text-shaped
/// reader gets line numbers, which is what such a reader emits — and it is also
/// where a name that does not match is spent. For the four page-shaped formats
/// that is `Coordinate::None`, the defect this function was written to fix,
/// back again. For xlsx it is worse and quieter: those blocks *do* carry row
/// numbers, so the default answers `Coordinate::Line { start: 10, end: 20 }` —
/// "рядки 10–20", with no sheet on them and nothing saying which.
///
/// The names are `mnema_core::manifest`'s constants rather than literals, so
/// that this arm and the reader that will emit the name are one symbol.
/// **That is a shared name, not a check.** Nothing here can verify that the pdf
/// reader calls itself `READER_PDF`: D40 forbids depending on the crate that
/// holds it, and the end-to-end tests cannot close that gap either — the
/// stand-in worker they run against states whatever string the test wrote, so
/// both sides of that assertion are written in one place. What those tests do
/// pin is this mapping, name to context, and — because they state the literals
/// rather than the constants — the values of the constants themselves.
///
/// A straight map otherwise, and it is only that because the wire carries a
/// page marker: until it did, this function had to refuse anything with more
/// than one page rather than guess which block belonged to which — every block
/// of a thousand-page document on page 1 is a state the schema accepts without
/// complaint and no test would notice.
fn pages_of(document: &Document) -> Vec<PageOf<'_>> {
    document
        .pages
        .iter()
        .map(|page| PageOf {
            page_no: i64::from(page.page_no),
            section_title: page.section_title.as_deref(),
            blocks: &page.blocks,
            context: match document.reader.as_str() {
                // The page number the reader gave this page, not its position
                // among the pages that arrived: a reader that drops a page it
                // cannot read leaves a gap, and the gap is the honest record.
                READER_PDF => PageContext::Fixed(Coordinate::Page {
                    number: page.page_no,
                }),
                // A section is the whole of what these three have to point at,
                // and it is identical for every chunk of the page — which is
                // what `Fixed` means. An untitled page therefore cites an empty
                // section, and the obligation to name one is the reader's
                // (spec §6, invariant 1), not this loop's to paper over.
                READER_HTML | READER_DOCX | READER_EPUB => {
                    PageContext::Fixed(Coordinate::Section {
                        title: page.section_title.clone().unwrap_or_default(),
                    })
                }
                // **Not `Fixed`.** A sheet is one page, so a fixed coordinate
                // would repeat the sheet's whole extent onto every chunk of it
                // — rows 10–20 cited as "аркуш Дані, рядки 1–500". The range
                // has to come from the blocks each chunk covers, which is what
                // `PageContext::Rows` computes; the page supplies only the
                // sheet's name.
                READER_XLSX => PageContext::Rows {
                    sheet: page.section_title.clone().unwrap_or_default(),
                },
                // text, markdown, and anything a future build adds without
                // deciding: line numbers are what those readers emit.
                _ => PageContext::Lines,
            },
        })
        .collect()
}

// `OnDisk`, `stat` and `mtime_nanos` used to live here. Retired: `OnDisk`
// itself is `mnema_core::OnDisk` (the shared-types crate both this crate and
// `mnema-index` already depended on, D45), and the measurement is
// `mnema_walk::stat` — the walk is the only place that looks at the disk, and
// this crate now only ever compares the numbers it is handed (§5).

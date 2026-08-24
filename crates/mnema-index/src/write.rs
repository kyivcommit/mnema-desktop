use mnema_core::{Block, Coordinate, Locator, OnDisk, Segment, SourceKind};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::Serialize;

use crate::{Db, DocumentStatus, Error};

/// Bumped whenever text preparation or the chunking constants change, so a
/// database holding two formats can tell them apart. D14. Bumped to 2: which
/// files count as text at all is part of the text a document contributes —
/// before this, a photo refused under `unsupported` left a journal row that
/// `ingest_file`'s second cheap arm would keep honouring after the worker
/// learned to refuse it as `not_text` instead (D51).
///
/// **2 → 3: the format readers landed.** A build with no reader for a PDF, a
/// docx, an xlsx or an epub refused all four as `unsupported` — measured, not
/// supposed: `git show fb3a924:crates/mnema-extract/src/bin/worker.rs`, the
/// `Reader::Pdf | Reader::Docx | Reader::Xlsx | Reader::Epub |
/// Reader::Unrecognized` arm — and that verdict is `is_about_content`, so the
/// second cheap arm keeps answering from it without spending a worker. Moving
/// this number is what makes those files be looked at again, and it is the only
/// thing that does: `repoint` clears a row after a successful index and
/// `forget_skips_not_in` clears one whose path left the tree, and neither
/// reaches a file that is still there and still refused.
///
/// **Two things it deliberately does not do, both measured.**
///
/// * It does not re-read an **indexed** file. The first cheap arm compares
///   size, mtime, the recorded reader and the chunk stage and never reads this
///   constant (`ADD_PATH_READER` in `migrations.rs` says so at length). What
///   re-reads a file whose *reader* changed — `.html` leaving the text reader,
///   inside this cycle — is `path.reader`/`path.reader_version`, a different
///   lever with a different owner.
/// * It does not compare one binary against another. A sidecar from another
///   release is caught, if at all, by frame parsing:
///   `Frame::Summary::skipped_pages` became a list this cycle, so an older
///   worker's summary does not parse, and `PoolError::Protocol` stops the whole
///   job rather than journalling one bad file
///   (`a_summary_that_counted_skipped_pages_is_a_protocol_error`,
///   `mnema-core/src/wire.rs`). That a packaged worker and its application are
///   one build is an argument about packaging, not something this number or
///   `scripts/verify-bundle.sh` proves.
///
/// **What the bump releases is every rule `is_about_content` claims, not the
/// one it was raised for.** `Unsupported` is the reason; `NoTextLayer`,
/// `NotText`, `BinaryTail`, `Malformed` and `Encrypted` are remembered exactly
/// as long and released by the same move. For the last two that release is
/// empty today — both were added on this same branch, so no older index holds
/// one — and the obligation is the other way round, owed by whoever ships the
/// next release: a reader that survives damage this one gives up on, or a
/// password prompt, has to move this number, or the files those changes exist
/// for are the ones they never get asked about. `SkipRule::is_about_content`
/// carries the same warning per rule.
///
/// Read by a second table besides `chunk` and `skipped`: `create_space` makes
/// the version part of a space's identity (`space.rs`), so a bump means the
/// next `create_space` mints a new space rather than finding the old one. That
/// is D14's intent — two formats side by side, each saying which it is — and
/// today it costs nothing, since nothing outside tests calls it (D29 ships no
/// local models in v1).
pub const INDEX_FORMAT_VERSION: i64 = 3;

#[derive(Debug, Clone, Serialize)]
pub struct Citation {
    pub text: String,
    /// Where in the source blocks this text came from, read back from
    /// `chunk.char_span`.
    ///
    /// `coordinate` says which lines to scroll to; this says which characters
    /// to paint, and the two answer different questions. Without it a citation
    /// can be shown but not located, which is the one failure the four-level
    /// model exists to prevent — and re-deriving the offsets by searching for
    /// the quote inside the block is what the server does
    /// (`app/index/highlight.py:50-57`), giving up silently on zero or several
    /// hits.
    pub spans: Vec<Segment>,
    /// `None` when the document has no recorded path — it was indexed from
    /// inside an archive, or its last copy on disk has been deleted. Not a
    /// `String` defaulting to `""`, because "we do not know where this is" and
    /// "the path is the empty string" must not render as the same citation.
    pub relative_path: Option<String>,
    pub section_title: Option<String>,
    pub coordinate: Coordinate,
}

/// One row of `path`, as [`Db::path_entry`] reads it back.
///
/// `mtime`'s unit is whatever the writer used; nothing in the schema fixes it.
/// The one writer today — `mnema-ingest` — stores **nanoseconds** since the
/// Unix epoch, because whole seconds leave a blind spot the cheap arm cannot
/// see through: a file edited twice inside one second, to the same length, is
/// indistinguishable from an untouched one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathEntry {
    pub document_id: String,
    pub size_bytes: i64,
    pub mtime: i64,
    /// Which reader made this document, and which version of it — the worker's
    /// own words, carried through `mnema_pool::Document` unchanged.
    ///
    /// Here so the cheap arm can ask a question `size_bytes` and `mtime` cannot
    /// answer: not "did the file move?" but "did the code that read it move?".
    /// See `ADD_PATH_READER` in `migrations.rs` for what goes wrong without it.
    ///
    /// **Neither field is guaranteed meaningful by the schema**, and the two are
    /// guarded unequally. `NOT NULL` is satisfied by `""` and by `0`;
    /// `mnema-pool` refuses a header whose reader is blank
    /// (`crates/mnema-pool/src/lib.rs:1080`) and says nothing about the version.
    /// A `reader_version` of 0 is out of reach today only because the field is
    /// required on the wire and every worker branch sends a published constant —
    /// a fact about the workers that exist, not a check.
    ///
    /// A row that did get here holding a value no manifest names is re-read
    /// **once**, not for ever: the mismatch sends the file to a worker and
    /// `repoint` then overwrites both columns with what the worker said. It is
    /// only a *writer* stuck on a wrong constant that never converges.
    pub reader: String,
    /// `i64` rather than the `u32` the wire carries, because that is what
    /// SQLite stores and reads back; the comparison against a manifest widens
    /// the manifest's `u32` rather than narrowing this.
    pub reader_version: i64,
}

pub struct WatchedRootRow {
    pub id: i64,
    pub absolute_path: String,
}

pub struct IndexedFileRow {
    pub relative_path: String,
    pub document_id: String,
}

pub struct RecentDocRow {
    pub document_id: String,
    pub relative_path: String,
    pub watched_root_id: i64,
    /// The chunk/done completion time (`ingest_stage.updated_at`), not the
    /// document's `created_at`: recency is when indexing *finished*, which a
    /// rebuild refreshes, not when the still-`pending` row was first inserted.
    pub indexed_at: i64,
}

impl Db {
    /// Every watched folder, in the order the user added them.
    pub fn list_watched_roots(&self) -> Result<Vec<WatchedRootRow>, Error> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id, absolute_path FROM watched_root ORDER BY added_at, id")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(WatchedRootRow {
                    id: r.get(0)?,
                    absolute_path: r.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The indexed documents' paths under one root, sorted — the left card's
    /// "Files" neighbours (§7). Only `status = 'indexed'`: a pending, failed or
    /// skipped path is not part of the searchable corpus and cannot be cited.
    pub fn indexed_files_under_root(&self, root_id: i64) -> Result<Vec<IndexedFileRow>, Error> {
        let mut stmt = self.conn().prepare(
            "SELECT p.relative_path, p.document_id
               FROM path p
               JOIN document d ON d.id = p.document_id
              WHERE p.watched_root_id = ?1 AND d.status = 'indexed'
              ORDER BY p.relative_path",
        )?;
        let rows = stmt
            .query_map([root_id], |r| {
                Ok(IndexedFileRow {
                    relative_path: r.get(0)?,
                    document_id: r.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The most-recently-*completed* documents, newest first, each once.
    ///
    /// Recency is the chunk/done checkpoint's `ingest_stage.updated_at`, not
    /// `document.created_at`. `created_at` is stamped when the still-`pending`
    /// row is first inserted, before any chunk is written, and a rebuild keeps
    /// that original value; ordering by it reports when a file first *entered*
    /// the index, not when it became searchable. The chunk/done stage is
    /// written when the document finishes and refreshed on every rebuild, so
    /// its `updated_at` is the honest "finished indexing" time.
    ///
    /// `'chunk'`/`'done'` are hardcoded, not imported: they are `mnema-ingest`'s
    /// `STAGE_CHUNK`/`STATUS_DONE` (`mnema-ingest/src/lib.rs`, written at
    /// `:597`), but `mnema-index` must not depend on `mnema-ingest` — that edge
    /// is the wrong way round and would be a cycle. The INNER JOIN also means an
    /// `indexed` document with no chunk/done stage row is not listed; the
    /// pipeline records that stage when it finishes a document, so this excludes
    /// nothing under the real lifecycle — but a test must record the stage, not
    /// only set the status.
    pub fn recent_indexed_documents(&self, limit: i64) -> Result<Vec<RecentDocRow>, Error> {
        let mut stmt = self.conn().prepare(
            // One MIN() aggregate: SQLite takes the bare watched_root_id from the
            // same row that produced MIN(relative_path), so the pair is
            // consistent. `s.updated_at` is likewise unambiguous under GROUP BY
            // d.id: the ingest_stage PK is (content_hash, stage), so there is
            // exactly one 'chunk' row per document.
            "SELECT d.id, MIN(p.relative_path), p.watched_root_id, s.updated_at
               FROM document d
               JOIN path p ON p.document_id = d.id
               JOIN ingest_stage s ON s.content_hash = d.id AND s.stage = 'chunk' AND s.status = 'done'
              WHERE d.status = 'indexed'
              GROUP BY d.id
              ORDER BY s.updated_at DESC, d.id
              LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(RecentDocRow {
                    document_id: r.get(0)?,
                    relative_path: r.get(1)?,
                    watched_root_id: r.get(2)?,
                    indexed_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn insert_watched_root(&self, absolute_path: &str) -> Result<i64, Error> {
        self.conn().execute(
            "INSERT INTO watched_root (absolute_path) VALUES (?1)",
            params![absolute_path],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// The absolute path a watched root was added under, or `None` if no root
    /// carries this id — a stale id sent by a window with no view onto a
    /// removed folder, or a typo in a JSON body. The shell has to turn an id
    /// back into a filesystem path before it has anything to hand
    /// `mnema_ingest::walk_root`, which walks a `Path`, not a row number.
    pub fn watched_root_path(&self, root_id: i64) -> Result<Option<String>, Error> {
        Ok(self
            .conn()
            .query_row(
                "SELECT absolute_path FROM watched_root WHERE id = ?1",
                params![root_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn insert_document(
        &self,
        content_hash: &str,
        mime: &str,
        size_bytes: i64,
        kind: SourceKind,
    ) -> Result<String, Error> {
        self.conn().execute(
            "INSERT INTO document (id, mime, size_bytes, source_kind) VALUES (?1, ?2, ?3, ?4)",
            params![content_hash, mime, size_bytes, kind.as_str()],
        )?;
        Ok(content_hash.to_string())
    }

    /// `reader` and `reader_version` are what the worker said produced this
    /// document, and are not defaulted here on purpose. The migration's
    /// `DEFAULT 'text'` is a one-off admission about rows written before the
    /// columns existed; a *new* row taking it would credit the text reader for
    /// work it did not do, and — unlike the migrated rows, which converge on the
    /// next walk — nothing would ever correct it. A `.md` written as `text`
    /// mismatches the manifest, is re-read, and is written as `text` again: a
    /// worker process per markdown file per walk, permanently, with no error.
    ///
    /// The size and the modification time arrive as one [`OnDisk`] rather than
    /// as two `i64`s. Loose, they sat between `root` and `reader_version` in a
    /// run of four bare integers that the compiler cannot tell apart, so
    /// transposing them was a silent wrong row rather than an error — and it is
    /// the pair the cheap arm compares, so the wrong way round it answers
    /// "changed" for every file on every walk. `OnDisk`'s own doc comment
    /// already calls it "the two numbers `path` records", and [`Db::record_skip`]
    /// takes the same type for the same two columns on `skipped`.
    pub fn insert_path(
        &self,
        root: i64,
        relative_path: &str,
        document_id: &str,
        disk: OnDisk,
        reader: &str,
        reader_version: i64,
    ) -> Result<(), Error> {
        self.conn().execute(
            "INSERT INTO path
                (watched_root_id, relative_path, document_id, size_bytes, mtime,
                 reader, reader_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                root,
                relative_path,
                document_id,
                disk.size_bytes,
                disk.mtime,
                reader,
                reader_version
            ],
        )?;
        Ok(())
    }

    /// What the index remembers about one path, or `None` if it has never seen
    /// it.
    ///
    /// This is the cheap arm of a multi-hour job over a folder nobody curated:
    /// `size_bytes` and `mtime` are here so that a file whose row still matches
    /// the disk is never opened, hashed or handed to a worker process. Without
    /// a reader the two columns were written and never consulted, which makes
    /// them decoration on a table whose comment (`schema.sql:83`) calls them
    /// "cheap reconciliation without hashing".
    ///
    /// `reader` and `reader_version` come back beside them because those two
    /// columns answer the half of "has anything changed?" that the disk cannot:
    /// the file is untouched, and the code that read it is not the code that
    /// would read it now. Selected here rather than through a call of its own so
    /// that the arm asking the question gets all four facts from the one lookup
    /// it already pays for.
    pub fn path_entry(&self, root: i64, relative_path: &str) -> Result<Option<PathEntry>, Error> {
        Ok(self
            .conn()
            .query_row(
                "SELECT document_id, size_bytes, mtime, reader, reader_version FROM path
                  WHERE watched_root_id = ?1 AND relative_path = ?2",
                params![root, relative_path],
                |r| {
                    Ok(PathEntry {
                        document_id: r.get(0)?,
                        size_bytes: r.get(1)?,
                        mtime: r.get(2)?,
                        reader: r.get(3)?,
                        reader_version: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn delete_path(&self, root: i64, relative_path: &str) -> Result<(), Error> {
        self.conn().execute(
            "DELETE FROM path WHERE watched_root_id = ?1 AND relative_path = ?2",
            params![root, relative_path],
        )?;
        Ok(())
    }

    /// Removes a watched root and every document whose last path was under it.
    /// Returns how many documents went with it.
    ///
    /// The schema cannot do this on its own. `path.watched_root_id` does
    /// cascade from `watched_root` (`schema.sql:80`), so dropping the root
    /// row alone already takes every `path` row under it — but nothing
    /// cascades onward from `path` to `document`, because `path.document_id`
    /// is the other direction of that foreign key (`schema.sql:82`): a
    /// document does not belong to its paths, its paths belong to it. So the
    /// root's own cascade stops at `path`, and the `document` rows those
    /// paths named are left behind — with zero paths left, still answering
    /// `search_lexical`, citing a folder that no longer exists (D33's own
    /// failure mode, one level up).
    ///
    /// A document is doomed only if EVERY path that ever named it sat under
    /// this root — the `NOT EXISTS` clause below excludes a document that also
    /// has a path under some other root, the same rule a second copy of a file
    /// already gets from [`Db::path_count`]. Read and decided inside the one
    /// transaction this method opens, not by the caller: a check-then-delete
    /// split across two calls could read "no other root" and then lose the
    /// race to a path being added under a different root in between.
    ///
    /// Done as one transaction rather than a loop of independent statements —
    /// a half-applied removal, root gone with some doomed documents still
    /// standing or the reverse, is exactly the orphan this closes. Which of
    /// the two is deleted first inside it does not matter for recovery: an
    /// interruption before `commit` leaves nothing committed at all, root and
    /// documents alike, because that is what one transaction means.
    pub fn delete_watched_root(&self, root_id: i64) -> Result<u64, Error> {
        let tx = Transaction::new_unchecked(self.conn(), TransactionBehavior::Immediate)?;
        let doomed: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT DISTINCT p.document_id FROM path p
                  WHERE p.watched_root_id = ?1
                    AND NOT EXISTS (SELECT 1 FROM path q
                                     WHERE q.document_id = p.document_id
                                       AND q.watched_root_id <> ?1)",
            )?;
            stmt.query_map(params![root_id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        for id in &doomed {
            crate::space::delete_vectors_for_document_in(&tx, id)?;
            // The document's own cascade takes its pages, blocks, chunks,
            // search rows, chunk_embedding_state rows, ingest_stage row,
            // document_tag rows, and its remaining path rows (all of them
            // under this same root, since it was doomed).
            tx.execute("DELETE FROM document WHERE id = ?1", params![id])?;
        }
        // Cascades away the path rows of any document that survived — one
        // still named from another root — and this root's tag_rule, skipped
        // and ignore_rule rows.
        tx.execute("DELETE FROM watched_root WHERE id = ?1", params![root_id])?;
        tx.commit()?;
        Ok(doomed.len() as u64)
    }

    pub fn path_count(&self, document_id: &str) -> Result<i64, Error> {
        Ok(self.conn().query_row(
            "SELECT count(*) FROM path WHERE document_id = ?1",
            params![document_id],
            |r| r.get(0),
        )?)
    }

    /// Every path the index currently holds under one watched root, sorted.
    ///
    /// What reconciliation (`mnema-ingest`'s phase 3) compares a completed
    /// walk's own findings against: a relative path in this list that the
    /// walk did not see is a candidate for deletion, never the other
    /// direction — a path the walk found that is missing from this list is
    /// simply new.
    pub fn paths_under_root(&self, root: i64) -> Result<Vec<String>, Error> {
        let mut stmt = self.conn().prepare(
            "SELECT relative_path FROM path WHERE watched_root_id = ?1 ORDER BY relative_path",
        )?;
        let rows = stmt.query_map(params![root], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn document_exists(&self, document_id: &str) -> Result<bool, Error> {
        let n: i64 = self.conn().query_row(
            "SELECT count(*) FROM document WHERE id = ?1",
            params![document_id],
            |r| r.get(0),
        )?;
        Ok(n == 1)
    }

    /// The page numbers this index actually holds for one document, ascending.
    ///
    /// The reader's own numbering, not positions — a document that lost its
    /// page 2 comes back as `[1, 3]`, and the gap is what `Frame::Page`'s doc
    /// comment calls the honest record.
    ///
    /// It exists because "what did the reader that just ran say?" and "what is
    /// in the index?" are two different questions, and `mnema_ingest`'s
    /// `journal_skipped_pages` may only answer the second. A document already
    /// in the index was extracted by whatever reader ran at the time, and a
    /// later reader disagreeing with it does not change a single row — so a
    /// journal row written from the later reader's account can name a page this
    /// table holds and every citation of it can be produced on demand. That is
    /// the contradiction `mnema_pool`'s `run_one` stops the whole job over when
    /// it arrives on the wire, and this is the door it would otherwise come in
    /// through.
    pub fn indexed_page_numbers(&self, document_id: &str) -> Result<Vec<i64>, Error> {
        let mut stmt = self
            .conn()
            .prepare("SELECT page_no FROM page WHERE document_id = ?1 ORDER BY page_no")?;
        let rows = stmt.query_map(params![document_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn insert_page(
        &self,
        document_id: &str,
        page_no: i64,
        text_source: &str,
        section_title: Option<&str>,
    ) -> Result<i64, Error> {
        self.conn().execute(
            "INSERT INTO page (document_id, page_no, text_source, section_title)
             VALUES (?1, ?2, ?3, ?4)",
            params![document_id, page_no, text_source, section_title],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// Writes one block of `page_id` and returns its rowid.
    ///
    /// Takes the [`Block`] itself rather than its fields spread out, for the
    /// same reason [`Db::insert_chunk`] takes a `Locator`: with the fields
    /// spread out there were two places to keep in step, and they fell out of
    /// step. `line_start` and `line_end` have been on `Block` since the
    /// locator was written and on the table since the schema was, and this
    /// statement wrote neither — every row NULL on both, no test reading them
    /// back, and a `Coordinate::Line` computed in memory that stopped being
    /// true the moment anything re-read it. Passing the type through means a
    /// field added to `Block` is a compile error here rather than a column
    /// silently left empty.
    ///
    /// `script`, `confidence` and `bbox` stay NULL: they belong to readers
    /// this product does not have yet, and `Block` deliberately does not carry
    /// them.
    pub fn insert_block(&self, page_id: i64, block: &Block) -> Result<i64, Error> {
        // `document_id` is read back from the page rather than taken as an
        // argument: it is denormalised onto `block` only so the composite
        // foreign keys can carry it down to `chunk`, and a caller that could
        // pass it could pass the wrong one.
        self.conn().execute(
            "INSERT INTO block
                (page_id, document_id, type, reading_order, language, text,
                 line_start, line_end)
             VALUES (?1, (SELECT document_id FROM page WHERE id = ?1), ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                page_id,
                block.block_type.as_str(),
                block.reading_order,
                block.language,
                block.text,
                block.line_start,
                block.line_end,
            ],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// One block's stored text, or `None` when no block has that id.
    ///
    /// `Option`, not `String`, and the distinction is load-bearing: a highlight
    /// measures from `Segment::block_start` into this text, so an id naming no
    /// row is a bug in whatever produced the id, while a block whose text is
    /// empty is an ordinary row. Collapsed onto `""` the first would render as
    /// a highlight over nothing instead of failing where someone can see it.
    pub fn block_text(&self, block_id: i64) -> Result<Option<String>, Error> {
        Ok(self
            .conn()
            .query_row(
                "SELECT text FROM block WHERE id = ?1",
                params![block_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Removes a document and everything below it — pages, blocks, chunks,
    /// search rows — **and every `path` row that names it**. Sweeps every
    /// space's vectors first — pinned by
    /// `delete_document_sweeps_its_own_vectors`.
    ///
    /// That last clause is why this is not the method to reach for when
    /// re-indexing. `path.document_id` is `REFERENCES document(id) ON DELETE
    /// CASCADE` (`schema.sql:82`), so deleting a document that two copies of a
    /// file point at takes both copies out of the index, and the second one
    /// comes back only when a walk reaches it again — a whole pass later, if
    /// the walk had already gone by. Use [`Db::clear_document_content`] to
    /// rebuild a document; this is for a document that should genuinely stop
    /// existing, which today means one no path names any more.
    ///
    /// **No transaction of its own** — every product caller already holds
    /// one, and SQLite forbids nesting. Atomic inside it: pinned by
    /// `delete_document_is_atomic_inside_a_callers_transaction`.
    pub fn delete_document(&self, id: &str) -> Result<(), Error> {
        crate::space::delete_vectors_for_document_in(self.conn(), id)?;
        self.conn()
            .execute("DELETE FROM document WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Empties a document of its content — pages, and by cascade its blocks,
    /// chunks, search rows and embedding state — leaving the `document` row and
    /// every `path` row that names it in place.
    ///
    /// What re-indexing goes through. The rows below a document cannot be
    /// written beside (`UNIQUE(document_id, ord)` collides) and cannot be left
    /// (blocks 2..n of a chunk live inside `char_span`, where no foreign key
    /// reaches them — `schema.sql:169-172`), so a pass over a document whose
    /// previous pass did not finish has to clear them first.
    ///
    /// What it deliberately does **not** clear is the document row itself. Its
    /// id is the sha256 of the file's bytes, and those bytes are what a rebuild
    /// has just re-read: the row is not stale, only the ladder under it is.
    /// Deleting it would cascade to the `path` rows of every other copy of the
    /// file, which is a rebuild of one document losing another path's place in
    /// the index.
    ///
    /// **The row does go back to `pending`, and that is the point of the second
    /// statement.** `status` answers one question — may this document be
    /// searched (`schema.sql:68-70`) — and `search_lexical` is the caller that
    /// asks it. A document whose content has just been emptied cannot honestly
    /// answer `indexed`: the rebuild writes its pages back one slice at a time,
    /// and between the clear and the checkpoint the document is searchable and
    /// silently short — a query for a section already rewritten hits, a query
    /// for one not yet rewritten returns nothing, and the two are
    /// indistinguishable from the window. Measured on an interrupted rebuild of
    /// twenty-five sections, where twenty answered and five did not. D61.
    ///
    /// Here rather than at the call site, and that is the whole of the fix:
    /// `insert_document` leaves a first indexing at `pending` through the
    /// column's own DEFAULT, so a document being built has never been
    /// searchable. This is the same fact for a document being *re*built, and
    /// putting it in the method that empties the content makes the two the same
    /// by construction instead of a rule the next rebuild path has to remember.
    ///
    /// Two statements now, not one, and this method opens a transaction over
    /// them rather than asking to be called inside somebody else's. Before D61
    /// it was a single statement and atomic by itself; the pair is not, and a
    /// caller running it outside a transaction would leave the content gone and
    /// the status still `indexed` — precisely the state D61 exists to abolish,
    /// reintroduced by the fix for it. Stated in prose it is a rule the next
    /// caller can miss, so it is stated in the types instead:
    /// [`Db::clear_document_content_in`] is the form an orchestrator uses, and
    /// this one is the standalone wrapper, exactly as `insert_chunk` /
    /// `insert_chunk_in` are.
    ///
    /// `chunk_search` needs no statement of its own: it cascades from `chunk`,
    /// and its `AFTER DELETE` trigger keeps `chunk_fts` in step even though the
    /// delete arrives through a cascade rather than directly — measured, and
    /// pinned by `tests/citation.rs`.
    ///
    /// **What it reaches now: the vectors too.** `chunk_embedding_state`
    /// cascades from `chunk` and goes; the `vec_emb_<space_id>` tables are
    /// created at runtime and referenced by no foreign key, so the cascade
    /// alone would never touch them —
    /// [`crate::space::delete_vectors_for_document_in`] is what does, called
    /// from [`Db::clear_document_content_in`] **before** the `DELETE FROM
    /// page` that starts the cascade. That order is load-bearing, not
    /// stylistic: once the cascade has taken the chunks there is no
    /// `chunk.document_id` left to look their vector ids up by, and
    /// [`Db::delete_watched_root`] (`write.rs:292`) keeps the same ordering
    /// for the same reason.
    ///
    /// The sharper fact underneath is why leaving this unreached would have
    /// been worse than clutter: `chunk.id` is `INTEGER PRIMARY KEY`
    /// **without** `AUTOINCREMENT`, so a rebuild reuses the ids it just freed.
    /// A surviving vector would then name a chunk whose text is different
    /// content, with the row that recorded it as embedded already gone with
    /// the cascade — search answering with text the file no longer contains,
    /// and nothing anywhere saying so. The same shape as the block-id reuse
    /// D36 closed one level up. Nothing outside a test writes a vector yet —
    /// there is no embedder under D29 — but this cycle is what starts
    /// writing them, and the gap was closed ahead of that rather than after
    /// it. D88.
    ///
    /// **What makes leaving it embedding-less correct, rather than merely
    /// silent.** This method takes a document's vectors away and writes
    /// nothing to replace them — on purpose, it is not this method's job to
    /// re-embed. That is only safe because whatever decides what needs
    /// embedding is meant to be a *computed* pass over `chunk` — "which chunks
    /// have no vector yet, asked fresh" — rather than a list kept in its own
    /// table. A computed pass notices a cleared document the same way it
    /// notices a brand new one, with nothing here having to tell it so. No
    /// such pass exists in this crate yet; it is later work in this cycle.
    ///
    /// The obligation this method would then owe is not a queue forgetting a
    /// rebuilt document — a rebuild writes chunks again, same as a first
    /// build, so a queue fed at write time picks it back up for free. It is a
    /// *record that survives the clear* claiming the document is already
    /// embedded, the same shape `status` had before D61: this method
    /// deliberately leaves the `document` row itself in place (above), and if
    /// "embedded" ever became a mark on that row or elsewhere instead of a
    /// question answered fresh, it would still say so with the vectors gone —
    /// a rebuilt document that looks embedded and answers nothing, and no test
    /// in `tests/citation.rs` would catch it. This method would then owe that
    /// mark the same reset the second statement above already gives `status`.
    ///
    /// [`Db::delete_document`] has never reached them either, so this is not a
    /// regression — it is written down here because this is the method a
    /// rebuild goes through.
    pub fn clear_document_content(&self, id: &str) -> Result<(), Error> {
        self.transaction(|tx| self.clear_document_content_in(tx, id))
    }

    /// [`Db::clear_document_content`] under a transaction the caller already
    /// opened — what a rebuild uses, since SQLite has no nested `BEGIN` and the
    /// clear has to land with the pages written after it.
    ///
    /// The same split, and for the same reason, as `insert_chunk` /
    /// `insert_chunk_in`: the atomicity of the pair is not weakened here, it is
    /// widened to the caller's transaction. `tx` must be a transaction on
    /// **this** `Db`'s connection, which [`same_connection`] is what enforces.
    pub fn clear_document_content_in(&self, tx: &Transaction<'_>, id: &str) -> Result<(), Error> {
        same_connection(self, tx);
        // Before the pages go: once the cascade has taken the chunks there is
        // no `chunk.document_id` left to look their ids up by — the same
        // ordering `delete_watched_root` keeps (`write.rs:292`). A `vec0`
        // table is the target of no foreign key, so nothing else reaches it.
        crate::space::delete_vectors_for_document_in(tx, id)?;
        tx.execute("DELETE FROM page WHERE document_id = ?1", params![id])?;
        crate::journal::write_document_status(tx, id, DocumentStatus::Pending)
    }

    /// Runs `f` inside one IMMEDIATE transaction, committing if it succeeds and
    /// rolling back if it does not.
    ///
    /// IMMEDIATE, not DEFERRED: the write lock is taken at `BEGIN`. A deferred
    /// transaction that reads before it writes fails the upgrade with
    /// SQLITE_BUSY immediately, and no `busy_timeout` applies to that
    /// (`open.rs:110-113`) — which is precisely the shape a job that checks
    /// before it writes would have.
    ///
    /// Every `&self` writer on `Db` goes through `self.conn().execute`, so it
    /// simply joins whatever transaction is open on that connection; calling
    /// them from inside `f` is how a whole document is written as one unit.
    /// The exceptions are the methods that open a transaction of their own —
    /// either directly, with `Transaction::new_unchecked`, or by calling this
    /// method itself — and none of them may be called from inside `f`: SQLite
    /// has no nested `BEGIN`.
    ///
    /// The rule is a reliable form rather than a count written into prose, and
    /// it takes two greps, not one. This sentence once named
    /// [`Db::insert_chunk`] as "the one exception" while there were already
    /// three, and Task 6 made it four — and the single grep that replaced that
    /// count was itself only ever half right, because it named direct
    /// `Transaction::new_unchecked` callers and stayed blind to every method
    /// that opens its transaction by calling `self.transaction(...)` instead,
    /// [`Db::insert_chunk`] among them. So: `grep -rn
    /// 'Transaction::new_unchecked' crates/mnema-index/src/` for the direct
    /// openers, and `grep -rn 'self\.transaction\(' crates/mnema-index/src/`
    /// for everyone who goes through this one — each grep's hit inside this
    /// doc comment and inside this function's own body is not itself a caller
    /// and does not belong on the list either greps names.
    ///
    /// Today: [`Db::insert_chunk`] (use [`Db::insert_chunk_in`] here) and
    /// [`Db::clear_document_content`] (use [`Db::clear_document_content_in`]
    /// here) go through `self.transaction`, and so do [`Db::upsert_vector`]
    /// and [`Db::delete_vector`], added in D95a to make a chunk's embedding
    /// replaceable and removable, and [`Db::upsert_vector_for_text`], added
    /// by Task 6 as the one the embedding pass actually calls.
    /// [`Db::create_space`], [`Db::drop_space`] and
    /// [`Db::delete_watched_root`] open one directly.
    /// [`Db::adopt_embedding_model`] does both at once: one transaction
    /// through [`Db::create_space`], another through `self.transaction` for
    /// the pointer write itself.
    ///
    /// **Keeping this list current is the point of writing it as prose and
    /// not only as the two greps above.** A reader who trusts the sentence
    /// should not have to run either grep to discover it is one method
    /// short — which is exactly what has happened to this list before, by
    /// more than one task.
    ///
    /// [`Db::read_snapshot`], below, opens one directly too and belongs on both
    /// greps' lists — it is the one opener that writes nothing, and the one a
    /// caller may not nest anything from this list inside.
    pub fn transaction<T>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let tx = Transaction::new_unchecked(self.conn(), TransactionBehavior::Immediate)?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    /// Runs `f` against **one snapshot** of the database: every read inside it
    /// sees the same state, however many statements it takes and whatever
    /// another connection commits meanwhile.
    ///
    /// Without it, a caller that reads several counts in a row reads each one in
    /// its own implicit transaction, and a writer committing between two of them
    /// leaves the set describing a state the index never held. `Db` is normally
    /// used from one thread at a time, so this matters in exactly one shape and
    /// it is the shape this product has: the shell keeps a second connection for
    /// the indexing and embedding job (`src-tauri/src/state.rs`,
    /// `AppState::open_job_index`), and the window's own mutex does nothing
    /// about it. The settings screen reads how many chunks are embedded, how
    /// many exist and how many were refused, and a person is asked to trust
    /// those three against each other.
    ///
    /// `Deferred`, not `Immediate`: this takes no write lock and blocks no
    /// writer. In WAL a read transaction fixes its snapshot at its first
    /// statement and holds it until the end, which is the whole of what this
    /// buys — and a `Deferred` transaction that never reads takes nothing at
    /// all.
    ///
    /// It ends in `rollback`, and that is not a failure path: nothing here
    /// wrote, so there is nothing to commit, and rolling back is how a read
    /// transaction is closed. An error out of `f` drops `tx` and rolls back the
    /// same way.
    ///
    /// ⚠️ **`f` must not write, and must not open a transaction of its own.**
    /// SQLite has no nested `BEGIN`; a method from the list on
    /// [`Db::transaction`] called inside this closure fails at run time rather
    /// than at compile time. `&Self` and not `&Transaction` because what makes
    /// this useful is that the ordinary reading methods — the ones a caller
    /// already has — run inside it unchanged; the price is that nothing in the
    /// type stops a writing one being called too.
    pub fn read_snapshot<T>(&self, f: impl FnOnce(&Self) -> Result<T, Error>) -> Result<T, Error> {
        let tx = Transaction::new_unchecked(self.conn(), TransactionBehavior::Deferred)?;
        let value = f(self)?;
        tx.rollback()?;
        Ok(value)
    }

    /// Writes a chunk and its lexical search row together, or writes neither.
    ///
    /// Before this, the pair was two public calls, and a chunk whose second
    /// call never happened sat in the database citable and embeddable but
    /// permanently unfindable by keyword, with no error anywhere — the worst
    /// outcome under D29, since the lexical arm is the only private way in.
    /// One transaction is what makes "written" and "findable" the same fact.
    ///
    /// `block_id` is no longer a parameter: it is read from
    /// `locator.spans[0]`, the element the schema's own CHECK already pins to
    /// `chunk.block_id`, so there is exactly one place to get it wrong instead
    /// of two that could disagree.
    pub fn insert_chunk(
        &self,
        document_id: &str,
        ord: i64,
        text: &str,
        locator: &Locator,
        kind: SourceKind,
    ) -> Result<i64, Error> {
        // IMMEDIATE: the chunk row and its search row must land together or
        // not at all. A reader between two separate writes would see a chunk
        // with no search row — exactly the state this transaction rules out.
        self.transaction(|tx| self.insert_chunk_in(tx, document_id, ord, text, locator, kind))
    }

    /// [`Db::insert_chunk`] under a transaction the caller already opened.
    ///
    /// SQLite has no nested `BEGIN`, so an orchestrator writing a whole
    /// document as one unit cannot call the method above at all — it would
    /// fail with "cannot start a transaction within a transaction" on the first
    /// chunk. Splitting the body is what keeps both callers honest: the
    /// atomicity of the pair is not weakened here, it is widened to the
    /// caller's transaction, which is a stronger promise than the one this
    /// method makes on its own.
    ///
    /// `tx` must be a transaction on **this** `Db`'s connection. Nothing in the
    /// type system says so — `Transaction` borrows a `Connection`, not a `Db` —
    /// so [`same_connection`] says it at run time.
    pub fn insert_chunk_in(
        &self,
        tx: &Transaction<'_>,
        document_id: &str,
        ord: i64,
        text: &str,
        locator: &Locator,
        kind: SourceKind,
    ) -> Result<i64, Error> {
        same_connection(self, tx);
        validate_locator(locator, text)?;
        let block_id = locator.spans[0].block_id;
        let span = serde_json::to_string(&locator.spans).map_err(Error::Json)?;
        let coord = serde_json::to_string(&locator.coordinate).map_err(Error::Json)?;
        let prepared = crate::prepare_for_search(text, kind);

        tx.execute(
            "INSERT INTO chunk
                (document_id, block_id, ord, text, char_span, coordinate,
                 n_chars, content_hash, index_format_version, source_kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                document_id,
                block_id,
                ord,
                text,
                span,
                coord,
                text.chars().count() as i64,
                chunk_content_hash(text),
                INDEX_FORMAT_VERSION,
                kind.as_str(),
            ],
        )?;
        let id = tx.last_insert_rowid();
        crate::search::write_search_row(tx, id, &prepared)?;
        Ok(id)
    }

    /// The chunk's text with everything needed to point a reader at it.
    ///
    /// When the document sits at several paths this returns one of them, chosen
    /// by the `ORDER BY` below. That ordering is here so the choice is stated
    /// rather than inherited from whichever index the planner picked; it is NOT
    /// the selection rule. What a citation should name when one document has
    /// several copies — the first, all of them, the one under the root the query
    /// was scoped to — is the search/RAG spec's decision, still open.
    pub fn citation(&self, chunk_id: i64) -> Result<Option<Citation>, Error> {
        let mut stmt = self.conn().prepare(
            "SELECT c.text, c.coordinate, c.char_span, p.section_title, pa.relative_path
               FROM chunk c
               JOIN block b  ON b.id = c.block_id
               JOIN page  p  ON p.id = b.page_id
               JOIN document d ON d.id = c.document_id
               LEFT JOIN path pa ON pa.document_id = d.id
              WHERE c.id = ?1
              ORDER BY pa.watched_root_id, pa.relative_path
              LIMIT 1",
        )?;
        let mut rows = stmt.query(params![chunk_id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let coord_json: String = row.get(1)?;
        let span_json: String = row.get(2)?;
        Ok(Some(Citation {
            text: row.get(0)?,
            coordinate: serde_json::from_str(&coord_json).map_err(Error::Json)?,
            spans: serde_json::from_str(&span_json).map_err(Error::Json)?,
            section_title: row.get(3)?,
            relative_path: row.get(4)?,
        }))
    }

    /// Every chunk of one document, in `ord` order, with the text as stored.
    ///
    /// `citation` answers about a chunk whose id the caller already has. This is
    /// the other direction, and the evaluation harness is why it exists: a gold
    /// chunk is named by a sentence, and `chunk.id` is reassigned on every
    /// re-index, so the id has to be recovered from the text every run.
    ///
    /// Ordered by `ord` rather than by `id` — the two agree today and the schema
    /// promises only the first (`schema.sql:152`, "explicit; the server has none").
    ///
    /// Unfiltered by `document.status`, on purpose, and that is a disagreement
    /// with `search_lexical` rather than an oversight: `search_lexical` answers
    /// only from a document whose status is `indexed` (`search.rs:42`), while
    /// this returns a document's chunks whatever its status — including
    /// `pending`. A method that filtered here would hide a corpus defect (a
    /// document the harness expects to be searchable but is not) behind an
    /// empty answer identical to "no such document", indistinguishable from
    /// each other by anything this method returns. Whatever judges document
    /// status owes itself that distinction, so it must read the status
    /// directly rather than infer it from this method going quiet — a caller
    /// comparing this method's answer with `search_lexical`'s is comparing two
    /// different populations of chunk.
    pub fn chunks_of_document(&self, document_id: &str) -> Result<Vec<(i64, String)>, Error> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id, text FROM chunk WHERE document_id = ?1 ORDER BY ord")?;
        let rows = stmt.query_map(params![document_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

/// Panics unless `tx` is a transaction on `db`'s own connection.
///
/// The `_in` methods widen their atomicity to a transaction the caller opened,
/// and that is only true of a transaction on **this** connection. `Transaction`
/// borrows a `Connection`, not a `Db`, so the type system cannot say it — and
/// the doc comments used to say the mistake was self-punishing: a foreign
/// transaction "would deadlock against this one's write lock". **It does not.**
/// Measured: `one.clear_document_content_in(&tx_of_two, id)` returns `Ok` in
/// 407 µs, the foreign transaction commits, and the write lands. Nothing on
/// `self` is touched, so there is nothing to block against.
///
/// What actually happens is worse than a deadlock, which is why this is a check
/// and not a corrected sentence: the pair commits atomically with somebody
/// else's unit of work. The caller's rollback silently takes these rows with
/// it, or its commit silently keeps them — a document emptied without being
/// taken out of the search, or the reverse. That is the split D61 exists to
/// close, reached through a different door and just as quiet.
///
/// `assert!` rather than `debug_assert!`, deliberately. The comparison is one
/// pointer against another next to a SQL statement, so its cost is not
/// measurable; and what it prevents is silent in release, which is exactly
/// where `debug_assert!` is compiled out. A programming error that panics is a
/// bug report; the same error shipped is an index that answers with a document
/// it should not, and nobody finds out.
///
/// Pointer equality is sound here rather than approximate: `Db::conn` returns
/// `&self.conn`, and a transaction on it borrows that same field, so the
/// addresses are equal exactly when the connection is the same one. Measured
/// both ways — `true` for a transaction on this `Db`, `false` for one on
/// another `Db` over the same file.
fn same_connection(db: &Db, tx: &Transaction<'_>) {
    assert!(
        std::ptr::eq(db.conn(), &**tx),
        "the transaction belongs to another connection: these writes would \
         commit or roll back with somebody else's unit of work"
    );
}

/// Refuses a locator whose spans are empty, out of order, overlapping, or
/// reach past the text they claim to measure.
///
/// `n_chars` is `text.chars().count()`, never a byte length: a byte-offset
/// implementation passes every test that predates this one and only shows
/// itself as a citation quoting the wrong slice of the first non-ASCII chunk.
///
/// This is checked here, in application code, rather than left to the
/// schema's own guards — those confirm a span names a real block on the right
/// page, not that its `end` is a number the chunk's text can actually support.
/// Nothing downstream re-derives the offsets, so a locator that lies about its
/// own text has to be caught before it is ever written, not after.
fn validate_locator(locator: &Locator, text: &str) -> Result<(), Error> {
    let n_chars = text.chars().count() as u32;
    let mut spans = locator.spans.iter();
    let Some(first) = spans.next() else {
        return Err(Error::InvalidLocator(
            "a chunk must come from at least one span".into(),
        ));
    };
    let mut prev_end = span_end_within(first, n_chars)?;
    for span in spans {
        if span.start < prev_end {
            return Err(Error::InvalidLocator(format!(
                "span [{}, {}) is out of order or overlaps the one ending at {prev_end}",
                span.start, span.end
            )));
        }
        prev_end = span_end_within(span, n_chars)?;
    }
    Ok(())
}

/// One span's own claim — `start <= end <= n_chars` — checked before it is
/// trusted as the previous end in the ordering pass above.
fn span_end_within(span: &Segment, n_chars: u32) -> Result<u32, Error> {
    if span.start > span.end || span.end > n_chars {
        return Err(Error::InvalidLocator(format!(
            "span [{}, {}) is outside the chunk's {n_chars} characters",
            span.start, span.end
        )));
    }
    Ok(span.end)
}

/// Content hash of a chunk: sha256 of its UTF-8 bytes, hex-encoded.
///
/// This is half of the embedding cache key — the other half is the space id,
/// because a vector is only valid within its space. Keying on text alone would
/// return the previous model's vectors into a new space. D33.
fn chunk_content_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    hex(&h.finalize())
}

/// Lower-case hex of a byte string.
///
/// `format!("{:x}", …)` over the digest is what sha2 0.10 allowed; 0.11 returns
/// `hybrid_array::Array`, which does not implement `LowerHex`. Sixteen bytes of
/// lookup table are cheaper than a dependency, and — unlike `write!` into a
/// `String` — this cannot fail, so it needs no `expect` in library code.
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    out
}

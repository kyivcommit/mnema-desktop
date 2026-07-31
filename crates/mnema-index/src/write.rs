use mnema_core::{Block, Coordinate, Locator, Segment, SourceKind};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::Serialize;

use crate::{Db, Error};

/// Bumped whenever text preparation or the chunking constants change, so a
/// database holding two formats can tell them apart. D14.
pub const INDEX_FORMAT_VERSION: i64 = 1;

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
}

impl Db {
    pub fn insert_watched_root(&self, absolute_path: &str) -> Result<i64, Error> {
        self.conn().execute(
            "INSERT INTO watched_root (absolute_path) VALUES (?1)",
            params![absolute_path],
        )?;
        Ok(self.conn().last_insert_rowid())
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

    pub fn insert_path(
        &self,
        root: i64,
        relative_path: &str,
        document_id: &str,
        size_bytes: i64,
        mtime: i64,
    ) -> Result<(), Error> {
        self.conn().execute(
            "INSERT INTO path (watched_root_id, relative_path, document_id, size_bytes, mtime)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![root, relative_path, document_id, size_bytes, mtime],
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
    pub fn path_entry(&self, root: i64, relative_path: &str) -> Result<Option<PathEntry>, Error> {
        Ok(self
            .conn()
            .query_row(
                "SELECT document_id, size_bytes, mtime FROM path
                  WHERE watched_root_id = ?1 AND relative_path = ?2",
                params![root, relative_path],
                |r| {
                    Ok(PathEntry {
                        document_id: r.get(0)?,
                        size_bytes: r.get(1)?,
                        mtime: r.get(2)?,
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
    /// search rows — **and every `path` row that names it**.
    ///
    /// That last clause is why this is not the method to reach for when
    /// re-indexing. `path.document_id` is `REFERENCES document(id) ON DELETE
    /// CASCADE` (`schema.sql:82`), so deleting a document that two copies of a
    /// file point at takes both copies out of the index, and the second one
    /// comes back only when a walk reaches it again — a whole pass later, if
    /// the walk had already gone by. Use [`Db::clear_document_content`] to
    /// rebuild a document; this is for a document that should genuinely stop
    /// existing, which today means one no path names any more.
    pub fn delete_document(&self, id: &str) -> Result<(), Error> {
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
    /// One statement, so that a caller running it inside a transaction with the
    /// rebuild gets the whole recovery atomically. `chunk_search` needs no
    /// statement of its own: it cascades from `chunk`, and its `AFTER DELETE`
    /// trigger keeps `chunk_fts` in step even though the delete arrives through
    /// a cascade rather than directly — measured, and pinned by
    /// `tests/citation.rs`.
    ///
    /// **What it does not reach: the vectors.** `chunk_embedding_state`
    /// cascades from `chunk` and goes; the `vec_emb_<space_id>` tables are
    /// created at runtime, are referenced by no foreign key, and stay. Nothing
    /// today writes one — there is no embedder under D29 — so this is a trap
    /// laid for the indexing-and-embedding spec rather than a live defect, and
    /// it is sharper than it looks: `chunk.id` is `INTEGER PRIMARY KEY`
    /// **without** `AUTOINCREMENT`, so a rebuild reuses the ids it just freed.
    /// A surviving vector would then name a chunk whose text is different
    /// content, with the row that recorded it as embedded already cleared. It
    /// is the same shape as the block-id reuse D36 closed one level up, and it
    /// needs the same kind of answer.
    ///
    /// [`Db::delete_document`] has never reached them either, so this is not a
    /// regression — it is written down here because this is the method a
    /// rebuild goes through.
    pub fn clear_document_content(&self, id: &str) -> Result<(), Error> {
        self.conn()
            .execute("DELETE FROM page WHERE document_id = ?1", params![id])?;
        Ok(())
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
    /// The one exception is [`Db::insert_chunk`], which opens a transaction of
    /// its own and so cannot nest — use [`Db::insert_chunk_in`] here.
    pub fn transaction<T>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let tx = Transaction::new_unchecked(self.conn(), TransactionBehavior::Immediate)?;
        let value = f(&tx)?;
        tx.commit()?;
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
    /// and a transaction from another connection would deadlock against this
    /// one's write lock rather than fail cleanly.
    pub fn insert_chunk_in(
        &self,
        tx: &Transaction<'_>,
        document_id: &str,
        ord: i64,
        text: &str,
        locator: &Locator,
        kind: SourceKind,
    ) -> Result<i64, Error> {
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

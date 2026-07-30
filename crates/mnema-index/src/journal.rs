//! Writers for the three tables `schema.sql` calls "journals": `skipped`,
//! `document.status` and `ingest_stage`. Requirements §13 requires every
//! skipped file and every PDF page with no text layer to be recorded with the
//! rule that fired — a scanned page silently indexed as empty is the worst
//! available behaviour — and D26 requires a checkpoint a multi-hour indexing
//! job can resume from. Before this file all three tables existed with no
//! writer anywhere.

use rusqlite::{OptionalExtension, params};

use crate::{Db, Error};

/// Which rule caused a file, or one page of it, to be skipped. The vocabulary
/// is closed on purpose — an open `rule` column turns a writer's typo into a
/// row `skips_for_root` can still list but a future query grouping by rule can
/// never match again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipRule {
    Crash,
    Timeout,
    Memory,
    Unsupported,
    NoTextLayer,
    /// The file could not be read at all: the path did not exist, was not a
    /// regular file, or permissions refused it. Added by the pool (task 8),
    /// which is the first code that had to name this outcome: the extraction
    /// worker reports it as `wire::Frame::Failed` and none of the five rules
    /// above covers it — a file that was never there is not a crash, not a
    /// timeout, not a memory kill, and not an unsupported format.
    ///
    /// It earns a rule of its own rather than being folded into `Unsupported`
    /// because the two demand different things of the user. An unsupported
    /// format is a limit of this product and stays skipped until the product
    /// grows a reader; an unreadable file is a fact about the user's disk that
    /// may well be transient (a file moved mid-scan, a permission fixed
    /// afterwards) and is worth retrying on the next pass.
    ///
    /// `skipped.rule` is a plain `TEXT` column with no CHECK constraint
    /// (`schema.sql:233`), so adding this value needed no migration and no
    /// `SCHEMA_VERSION` bump.
    Unreadable,
    /// The file is larger than the ceiling the pool was configured with, and
    /// was refused from `stat` before a byte of it was read
    /// (`crates/mnema-extract/src/bin/worker.rs`).
    ///
    /// Split out of `Unsupported`, which it was folded into until the two were
    /// found to want different things.
    ///
    /// It is a different answer to the user. `Unsupported` says this product
    /// has no reader for that format and the file stays skipped until the
    /// product grows one; this says the file is fine and a *setting* excluded
    /// it. "Which files were too large?" is a question someone deciding
    /// whether to raise the ceiling needs the journal to answer, and while the
    /// two shared a rule it could not.
    ///
    /// And it is a different answer to the index. `mnema-ingest` removes what
    /// it holds under a path when the worker read a file and declined its
    /// content — a `.txt` overwritten by a PDF must stop answering under its
    /// own name. This branch never opens the file; it decides from `stat`
    /// alone, so the refusal itself says nothing about whether the content
    /// changed. That is settled there by comparing the size on disk against
    /// `path.size_bytes`, which is exact: a document exists under a path only
    /// because the file was once under the ceiling, so the same size now means
    /// the ceiling moved, and a different size means the file was rewritten.
    /// `mnema_ingest`'s `displaces` carries the argument in full.
    TooLarge,
}

impl SkipRule {
    pub fn as_str(self) -> &'static str {
        match self {
            SkipRule::Crash => "crash",
            SkipRule::Timeout => "timeout",
            SkipRule::Memory => "memory",
            SkipRule::Unsupported => "unsupported",
            SkipRule::NoTextLayer => "no_text_layer",
            SkipRule::Unreadable => "unreadable",
            SkipRule::TooLarge => "too_large",
        }
    }
}

/// Mirrors `document.status`'s own CHECK (`schema.sql:71-72`). Answers only
/// "may this document be searched?" — which stage it reached and why it
/// stopped there is `ingest_stage`'s business, not this column's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentStatus {
    Pending,
    Indexed,
    Failed,
    Skipped,
}

impl DocumentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DocumentStatus::Pending => "pending",
            DocumentStatus::Indexed => "indexed",
            DocumentStatus::Failed => "failed",
            DocumentStatus::Skipped => "skipped",
        }
    }

    /// Not a public `FromStr`: the only source of this string is the `status`
    /// column, guarded by the schema's own CHECK on every write path this
    /// crate exposes — so `None` here means some row was written around it,
    /// not that the caller made a mistake. `document_status` turns that into
    /// `Error::UnknownDocumentStatus` rather than trusting the CHECK as a
    /// proof and panicking on the gap.
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => DocumentStatus::Pending,
            "indexed" => DocumentStatus::Indexed,
            "failed" => DocumentStatus::Failed,
            "skipped" => DocumentStatus::Skipped,
            _ => return None,
        })
    }
}

/// One row of the skip journal, read back for a watched root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFile {
    pub relative_path: String,
    /// `None` for a whole-file skip (a worker crash, a timeout); `Some` when a
    /// single PDF page was skipped inside an otherwise readable document.
    pub page_no: Option<i64>,
    pub reason: String,
    pub rule: String,
}

impl Db {
    /// Records that a file, or one page of it, did not make it into the index.
    pub fn record_skip(
        &self,
        root_id: i64,
        relative_path: &str,
        page_no: Option<i64>,
        reason: &str,
        rule: SkipRule,
    ) -> Result<(), Error> {
        self.conn().execute(
            "INSERT INTO skipped (watched_root_id, relative_path, page_no, reason, rule)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![root_id, relative_path, page_no, reason, rule.as_str()],
        )?;
        Ok(())
    }

    /// Every skip recorded under one watched root, oldest first.
    pub fn skips_for_root(&self, root_id: i64) -> Result<Vec<SkippedFile>, Error> {
        let mut stmt = self.conn().prepare(
            "SELECT relative_path, page_no, reason, rule FROM skipped
              WHERE watched_root_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![root_id], |r| {
                Ok(SkippedFile {
                    relative_path: r.get(0)?,
                    page_no: r.get(1)?,
                    reason: r.get(2)?,
                    rule: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Sets a document's lifecycle status.
    pub fn set_document_status(&self, id: &str, status: DocumentStatus) -> Result<(), Error> {
        self.conn().execute(
            "UPDATE document SET status = ?1 WHERE id = ?2",
            params![status.as_str(), id],
        )?;
        Ok(())
    }

    pub fn document_status(&self, id: &str) -> Result<DocumentStatus, Error> {
        let s: String = self.conn().query_row(
            "SELECT status FROM document WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        DocumentStatus::parse(&s).ok_or_else(|| Error::UnknownDocumentStatus(s))
    }

    /// Records that `stage` reached `status` for the document hashing to
    /// `content_hash` — the checkpoint a multi-hour indexing job resumes from.
    ///
    /// `content_hash` here is the **document's** content hash, settled by D26:
    /// the unit of indexing work is one document, written in one transaction,
    /// so the checkpoint keys on the same thing the transaction does. That
    /// name collides with `chunk.content_hash`, which hashes a *chunk's* own
    /// text and is half of the embedding cache key (`write.rs`) — two columns
    /// sharing a name and meaning different things, not the same fact at two
    /// grains.
    ///
    /// Upserts rather than only inserting: `(content_hash, stage)` is the
    /// primary key, and resuming a job re-attempts whatever stage it did not
    /// finish, which must update the existing row instead of failing with a
    /// uniqueness violation.
    pub fn record_stage(&self, content_hash: &str, stage: &str, status: &str) -> Result<(), Error> {
        self.conn().execute(
            "INSERT INTO ingest_stage (content_hash, stage, status)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (content_hash, stage) DO UPDATE SET
                status = excluded.status,
                updated_at = unixepoch()",
            params![content_hash, stage, status],
        )?;
        Ok(())
    }

    /// What `stage` last reached for this document, or `None` if it has never
    /// been recorded.
    ///
    /// The half of D26's checkpoint that was missing: `record_stage` has been
    /// able to write since task 5 and nothing could read, which makes a
    /// checkpoint a log. A second pass over the same folder asks this before it
    /// spends a worker process on a document it has already finished.
    ///
    /// `Option<String>` rather than a bool or a typed enum. Not a bool, because
    /// "never attempted" and "attempted and failed" ask opposite things of the
    /// next run and a bool cannot hold both. Not an enum, because unlike
    /// `document.status` this column has **no CHECK** (`schema.sql:219-224`) and
    /// no closed vocabulary anywhere — the stages belong to whoever is running
    /// the pipeline, and the day a stage is added is not a day the index crate
    /// should have to be edited.
    pub fn stage_status(&self, content_hash: &str, stage: &str) -> Result<Option<String>, Error> {
        Ok(self
            .conn()
            .query_row(
                "SELECT status FROM ingest_stage WHERE content_hash = ?1 AND stage = ?2",
                params![content_hash, stage],
                |r| r.get(0),
            )
            .optional()?)
    }
}

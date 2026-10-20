//! The `query_history` table: one row per distinct question, newest first.
//!
//! Answers are stored as the JSON the caller already built, not as columns:
//! the reader is the same caller, and a snapshot is only useful if it is the
//! exact answer that was shown.

use rusqlite::{OptionalExtension, params};

use crate::{Db, Error};

/// How many questions the history keeps. The oldest is dropped on the write
/// that would exceed it.
pub const HISTORY_LIMIT: i64 = 50;

/// One entry in the history, without its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    pub id: i64,
    pub query: String,
    /// Unix seconds, as `unixepoch()` writes them.
    pub asked_at: i64,
}

impl Db {
    /// Records a question and its answer, replacing any earlier record of the
    /// same trimmed query. Trims `query` itself, so the stored key is the
    /// trimmed text.
    ///
    /// The replacement is a delete and an insert, not an upsert: an upsert keeps
    /// the old `id`, and with two records in the same second the `id DESC`
    /// tie-break would then sort the repeated question below newer ones.
    ///
    /// Answers the id of the row it wrote: the window marks the conversation a
    /// fresh answer belongs to, and the row's own id is what names it.
    pub fn history_record(&self, query: &str, answer_json: &str) -> Result<i64, Error> {
        let query = query.trim();
        debug_assert!(
            !query.is_empty(),
            "callers reject an empty query before recording"
        );
        self.transaction(|tx| {
            tx.execute("DELETE FROM query_history WHERE query = ?1", params![query])?;
            // The stamp never goes backwards. If the clock stepped back, the new
            // row takes the newest existing stamp instead, so the trim below
            // cannot remove the answer that was just given.
            tx.execute(
                "INSERT INTO query_history (query, answer, asked_at)
                 VALUES (?1, ?2, max(unixepoch(),
                     COALESCE((SELECT max(asked_at) FROM query_history), 0)))",
                params![query, answer_json],
            )?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "DELETE FROM query_history WHERE id NOT IN (
                    SELECT id FROM query_history
                     ORDER BY asked_at DESC, id DESC
                     LIMIT ?1)",
                params![HISTORY_LIMIT],
            )?;
            Ok(id)
        })
    }

    /// Every entry, newest first, without the answers.
    pub fn history_list(&self) -> Result<Vec<HistoryRow>, Error> {
        let mut stmt = self.conn().prepare(
            "SELECT id, query, asked_at FROM query_history
              ORDER BY asked_at DESC, id DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(HistoryRow {
                    id: r.get(0)?,
                    query: r.get(1)?,
                    asked_at: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The query and the stored answer JSON for one entry, or `None` if the id
    /// is not in the history.
    pub fn history_answer(&self, id: i64) -> Result<Option<(String, String)>, Error> {
        Ok(self
            .conn()
            .query_row(
                "SELECT query, answer FROM query_history WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// Removes one entry. Returns whether it existed.
    pub fn history_delete(&self, id: i64) -> Result<bool, Error> {
        let removed = self
            .conn()
            .execute("DELETE FROM query_history WHERE id = ?1", params![id])?;
        Ok(removed > 0)
    }
}

//! One document's whole text, as the window's right pane paints it (§3).
//!
//! Beside `write.rs` rather than inside it because the question is different:
//! `reading_window` answers "a passage and its neighbours", this answers "the
//! file", and the two disagree about what a block is worth showing —
//! `reading_window` drops whitespace-only blocks so `radius` counts visible
//! source, while a whole-file view shows them, because a line of spaces is part
//! of the file.
//!
//! Read-only: nothing on this path writes.

use rusqlite::{OptionalExtension, params};

use crate::{Db, Error};

/// One block of a document in reading order.
pub struct ViewBlockRow {
    pub block_id: i64,
    /// `block.type` verbatim — the pane decides what to draw as a heading.
    pub kind: String,
    pub text: String,
    pub page_no: i64,
    pub section_title: Option<String>,
}

/// A document's blocks, and whether the document holds more than the caller
/// asked for.
///
/// `truncated` rather than a shortened `Vec` on its own: a caller that shows
/// the list must be able to tell a whole file from the first `limit` blocks of
/// one, and a caller that cannot show a partial file refuses instead.
pub struct DocumentViewRow {
    pub blocks: Vec<ViewBlockRow>,
    pub truncated: bool,
}

impl Db {
    /// The document's blocks in `(page_no, reading_order)` order — document
    /// reading order, which is the pair and not `reading_order` alone: that
    /// column restarts on every page, and the schema's own uniqueness is
    /// `(page_id, reading_order)` (`ix_block_page`, `schema.sql:140`).
    ///
    /// `None` when no `document` row has that id. The distinction from `Some`
    /// with no blocks is load-bearing: the pane says "no such document" for one
    /// and "nothing to show" for the other.
    ///
    /// Reads `limit + 1` rows and drops the extra: its presence is what tells
    /// "exactly `limit`" from "more than `limit`", without a second count that
    /// could disagree with the rows themselves.
    pub fn document_view(
        &self,
        document_id: &str,
        limit: usize,
    ) -> Result<Option<DocumentViewRow>, Error> {
        let exists: Option<i64> = self
            .conn()
            .query_row(
                "SELECT 1 FROM document WHERE id = ?1",
                params![document_id],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Ok(None);
        }

        // `LIMIT -1` means *no limit* in SQLite, so a `usize` that does not fit
        // an `i64` must not wrap round to a negative number here.
        let fetch = i64::try_from(limit).unwrap_or(i64::MAX).saturating_add(1);
        let mut stmt = self.conn().prepare(
            "SELECT b.id, b.type, b.text, p.page_no, p.section_title
               FROM block b
               JOIN page p ON p.id = b.page_id
              WHERE b.document_id = ?1
              ORDER BY p.page_no, b.reading_order
              LIMIT ?2",
        )?;
        let mut blocks = stmt
            .query_map(params![document_id, fetch], |r| {
                Ok(ViewBlockRow {
                    block_id: r.get(0)?,
                    kind: r.get(1)?,
                    text: r.get(2)?,
                    page_no: r.get(3)?,
                    section_title: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let truncated = blocks.len() > limit;
        blocks.truncate(limit);
        Ok(Some(DocumentViewRow { blocks, truncated }))
    }

    /// The one `path` row naming a document, or `None` when it has none or
    /// several.
    ///
    /// The fallback for a caller that holds only a `documentId`: one path is
    /// that path, and anything else is no honest single location — picking one
    /// of several would report freshness about a copy the person may not be
    /// looking at. `LIMIT 2` tells "one" from "several" without counting.
    pub fn document_path(&self, document_id: &str) -> Result<Option<(i64, String)>, Error> {
        let mut stmt = self.conn().prepare(
            "SELECT watched_root_id, relative_path
               FROM path
              WHERE document_id = ?1
              ORDER BY watched_root_id, relative_path
              LIMIT 2",
        )?;
        let rows = stmt
            .query_map(params![document_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(match rows.as_slice() {
            [one] => Some(one.clone()),
            _ => None,
        })
    }
}

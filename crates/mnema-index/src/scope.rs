//! Which folders of a watched root take part in search (spec §3, §4).

use rusqlite::{Transaction, TransactionBehavior, params};

use crate::{Db, Error};

/// "This `path` row `pa` is searched" (spec §5): the nearest `search_folder`
/// entry at or above its folder decides, no entry means searched. Written once
/// for the two search arms and for `citation`'s choice of path. The prefix is
/// compared with `substr`, never `LIKE`, so `_`/`%` are literal and `note`
/// does not cover `notes/a.md`.
macro_rules! path_is_searched {
    () => {
        "COALESCE((SELECT f.searched FROM search_folder f
                    WHERE f.watched_root_id = pa.watched_root_id
                      AND (f.folder_path = ''
                           OR substr(pa.relative_path, 1, length(f.folder_path) + 1)
                              = f.folder_path || '/')
                    ORDER BY length(f.folder_path) DESC
                    LIMIT 1), 1) = 1"
    };
}

/// An `AND` clause: the document `d` has no path at all, or at least one
/// searched path — a document outside every folder is never hidden by a choice
/// about folders. An empty `search_folder` short-circuits it, so the common
/// case pays one probe.
macro_rules! searchable_document {
    () => {
        concat!(
            " AND (NOT EXISTS (SELECT 1 FROM search_folder)
                OR NOT EXISTS (SELECT 1 FROM path pn WHERE pn.document_id = d.id)
                OR EXISTS (
                SELECT 1 FROM path pa WHERE pa.document_id = d.id AND ",
            $crate::scope::path_is_searched!(),
            "))"
        )
    };
}
// `path_is_searched` is also what `citation` orders by.
#[allow(unused_imports)]
pub(crate) use {path_is_searched, searchable_document};

impl Db {
    /// Records that `folder_path` (root-relative, `/`-separated, `""` = the
    /// root) is searched or not, and forgets every entry beneath it: a choice
    /// made on a folder covers its subtree, so older choices below it are stale.
    ///
    /// The prefix is compared with `substr`, never `LIKE`: `_` and `%` are
    /// ordinary characters in a folder name, and `notes` must not match
    /// `notes2`. A root id that no longer exists writes nothing and is not an
    /// error — the root can be removed between the click and the write.
    pub fn set_folder_searched(
        &self,
        root_id: i64,
        folder_path: &str,
        searched: bool,
    ) -> Result<(), Error> {
        let tx = Transaction::new_unchecked(self.conn(), TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM search_folder
              WHERE watched_root_id = ?1
                AND (?2 = '' OR substr(folder_path, 1, length(?2) + 1) = ?2 || '/')",
            params![root_id, folder_path],
        )?;
        tx.execute(
            "INSERT INTO search_folder (watched_root_id, folder_path, searched)
             SELECT ?1, ?2, ?3
              WHERE EXISTS (SELECT 1 FROM watched_root WHERE id = ?1)
             ON CONFLICT (watched_root_id, folder_path) DO UPDATE SET searched = ?3",
            params![root_id, folder_path, searched],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Every stored entry of `root_id`, by `folder_path` ascending.
    pub fn search_folders(&self, root_id: i64) -> Result<Vec<(String, bool)>, Error> {
        let mut stmt = self.conn().prepare(
            "SELECT folder_path, searched FROM search_folder
              WHERE watched_root_id = ?1 ORDER BY folder_path",
        )?;
        Ok(stmt
            .query_map(params![root_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Whether any `indexed` document takes part in search — the same
    /// condition both arms apply, so "nothing is searched" and "search finds
    /// nothing" cannot disagree (spec §7).
    pub fn any_document_searchable(&self) -> Result<bool, Error> {
        Ok(self.conn().query_row(
            concat!(
                "SELECT EXISTS (SELECT 1 FROM document d WHERE d.status = 'indexed'",
                searchable_document!(),
                ")"
            ),
            [],
            |r| r.get(0),
        )?)
    }
}

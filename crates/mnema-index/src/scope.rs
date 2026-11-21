//! Which folders of a watched root take part in search (spec §3, §4).

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{Db, Error};

/// "This `path` row `pa` is searched" (spec §5): the nearest `search_folder`
/// entry decides — one naming the path itself (a single file's tick), else
/// one at or above its folder — and no entry means searched. Written once
/// for the two search arms and for `citation`'s choice of path. The prefix is
/// compared with `substr`, never `LIKE`, so `_`/`%` are literal and `note`
/// does not cover `notes/a.md`.
macro_rules! path_is_searched {
    () => {
        "COALESCE((SELECT f.searched FROM search_folder f
                    WHERE f.watched_root_id = pa.watched_root_id
                      AND (f.folder_path = ''
                           OR f.folder_path = pa.relative_path
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
pub(crate) use {path_is_searched, searchable_document};

impl Db {
    /// Records that `folder_path` (root-relative, `/`-separated, `""` = the
    /// root; a file's own path names that one file) is searched or not, and
    /// forgets every entry beneath it: a choice
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
        fold_back(&tx, root_id, folder_path)?;
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

/// Fold-back, the launcher's twin of the folders tree's (`ticks.rs`): after a
/// choice, climb from its folder while every indexed file under the folder is
/// searched alike, and replace the entries under it with one entry on the
/// folder — or none, when the folder already inherits that value. So a folder
/// whose files were all unticked one by one is an unticked folder, and a file
/// a later scan brings into it follows the folder (the tree shows a folder's
/// tick from its files). "Indexed" because that is what the tree lists: a file
/// the window cannot show must not hold a fold back. Runs inside the caller's
/// transaction, so no search ever sees half a fold.
fn fold_back(tx: &Transaction<'_>, root_id: i64, chosen: &str) -> Result<(), Error> {
    let mut dir = chosen;
    while !dir.is_empty() {
        dir = dir.rsplit_once('/').map_or("", |(up, _)| up);
        let (files, searched): (i64, i64) = tx.query_row(
            concat!(
                "SELECT COUNT(*), COALESCE(SUM(",
                path_is_searched!(),
                "), 0)
                   FROM path pa JOIN document d ON d.id = pa.document_id
                  WHERE pa.watched_root_id = ?1 AND d.status = 'indexed'
                    AND (?2 = '' OR substr(pa.relative_path, 1, length(?2) + 1) = ?2 || '/')"
            ),
            params![root_id, dir],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if files == 0 || (searched != 0 && searched != files) {
            break;
        }
        let value = searched == files;
        let inherited: bool = tx
            .query_row(
                "SELECT searched FROM search_folder
                  WHERE watched_root_id = ?1 AND folder_path <> ?2
                    AND (folder_path = ''
                         OR substr(?2, 1, length(folder_path) + 1) = folder_path || '/')
                  ORDER BY length(folder_path) DESC LIMIT 1",
                params![root_id, dir],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(true);
        tx.execute(
            "DELETE FROM search_folder
              WHERE watched_root_id = ?1
                AND (?2 = '' OR folder_path = ?2
                     OR substr(folder_path, 1, length(?2) + 1) = ?2 || '/')",
            params![root_id, dir],
        )?;
        if value != inherited {
            tx.execute(
                "INSERT INTO search_folder (watched_root_id, folder_path, searched)
                 VALUES (?1, ?2, ?3)",
                params![root_id, dir, value],
            )?;
        }
    }
    Ok(())
}

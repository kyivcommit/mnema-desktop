//! Which folders of a watched root take part in search (spec §3, §4).

use rusqlite::{Transaction, TransactionBehavior, params};

use crate::{Db, Error};

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
}

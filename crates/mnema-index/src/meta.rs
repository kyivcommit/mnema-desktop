//! The `meta` table: one current value per key.
//!
//! The keys are constants because they are read in one crate and written in
//! another, with no compiler between them — the same reason the reader names
//! are constants in `mnema-core` rather than literals in three files.

use rusqlite::{OptionalExtension, params};

use crate::{Db, Error};

/// Which embedding space the product is working with. One key, not two: whether
/// that space is full is already in `embedding_space.state`, and a second key
/// would be a vocabulary with one user.
pub const META_ACTIVE_SPACE: &str = "active_space";
/// The rerank model. Leaves nothing on disk, so it may change at any time.
pub const META_RERANK_MODEL: &str = "rerank_model";
/// The chat model. Same.
pub const META_CHAT_MODEL: &str = "chat_model";
/// The `sqlite-vec` version that created the first space here. Written, never
/// checked: the crate is pinned — the `sqlite-vec = "=0.1.9"` entry under
/// `[workspace.dependencies]` in the workspace root — because its on-disk format
/// carries no stability promise, and a check with no migration path behind it
/// would be theatre. What this buys is a diagnosis instead of a mystery on the
/// day the pin moves.
pub const META_VEC_VERSION: &str = "vec_version";

impl Db {
    pub fn meta_get(&self, key: &str) -> Result<Option<String>, Error> {
        Ok(self
            .conn()
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// Writes a key that may be overwritten freely.
    ///
    /// Every key here loses something when it is replaced, and for all but one
    /// what it loses is a diagnosis: [`META_VEC_VERSION`] overwritten costs the
    /// explanation of why an old database stopped opening, not the database.
    /// [`META_ACTIVE_SPACE`] is the exception and is refused — see
    /// [`Error::ActiveSpaceNotWritable`] for what an overwrite of it costs.
    ///
    /// A refusal rather than a silent no-op, because a caller that has just
    /// been stopped from changing which space search reads is a caller whose
    /// next line is wrong.
    pub fn meta_set(&self, key: &str, value: &str) -> Result<(), Error> {
        if key == META_ACTIVE_SPACE {
            return Err(Error::ActiveSpaceNotWritable);
        }
        self.meta_put(key, value)
    }

    /// The unguarded upsert, `meta_set` minus the one rule it enforces.
    ///
    /// `pub(crate)` deliberately: this is the only way [`META_ACTIVE_SPACE`]
    /// can be written, and it is reachable only from inside this crate, where
    /// the check that the space being left behind holds no vectors can be made
    /// in the same transaction as the write. A public function taking any key
    /// would hand every caller a way around that check, which is the difference
    /// between a split index being unreachable and being merely discouraged.
    pub(crate) fn meta_put(&self, key: &str, value: &str) -> Result<(), Error> {
        self.conn().execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

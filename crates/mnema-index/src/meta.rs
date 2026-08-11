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

    /// Writes a key, refusing [`META_ACTIVE_SPACE`]: overwriting it loses data.
    ///
    /// An overwrite costs three different things here, not two, and the whole
    /// design of this function is the third one. [`META_RERANK_MODEL`] and
    /// [`META_CHAT_MODEL`] leave nothing on disk, so replacing them costs
    /// nothing. [`META_VEC_VERSION`] costs the explanation of why an old
    /// database stopped opening — a diagnosis, not data. [`META_ACTIVE_SPACE`]
    /// costs data, and is the one key refused here; see
    /// [`Error::ActiveSpaceNotWritable`] for what the loss looks like.
    ///
    /// Whoever adds a fifth key owes it the same question rather than this
    /// answer: what does replacing this value make unreachable?
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
    /// `pub(crate)` deliberately, and it is worth being exact about what that
    /// buys. This crate's own **typed** API offers a caller outside it no way
    /// to write [`META_ACTIVE_SPACE`]: `meta_set` refuses the key, and this
    /// function cannot be named. What that closes is the convenient route — the
    /// one taken without a decision. It is not a boundary: [`Db::conn`] hands
    /// out the connection, and nothing on `meta` stops raw SQL from writing
    /// this key. So a caller who means to go around the guard can, and one who
    /// never thought about it will not.
    ///
    /// Inside the crate this is where the adoption path writes, which is the
    /// point of leaving it here: the check that the space being left behind
    /// holds no vectors and the write that replaces it can then be one
    /// transaction rather than two.
    pub(crate) fn meta_put(&self, key: &str, value: &str) -> Result<(), Error> {
        self.conn().execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

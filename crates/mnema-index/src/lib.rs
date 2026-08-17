mod journal;
mod meta;
mod migrations;
mod open;
mod search;
mod space;
mod text_prep;
mod write;

pub use journal::{DocumentStatus, SkipEntry, SkipRule, SkippedFile};
pub use meta::{
    META_ACTIVE_SPACE, META_CHAT_MODEL, META_RERANK_MODEL, META_SEARCH_CONTENT_ARM,
    META_SEARCH_TEXT_ARM, META_VEC_VERSION,
};
pub use migrations::SCHEMA_VERSION;
pub use open::{Db, open, register_vector_extension};
pub use search::QueryRule;
pub use space::{AdoptedSpace, Neighbours, PendingChunk, VectorRole};
pub use text_prep::{prepare_for_search, search_terms};
pub use write::{Citation, INDEX_FORMAT_VERSION, PathEntry};

impl Error {
    /// Whether this is contention rather than a fault: another connection held
    /// the write lock for longer than `BUSY_TIMEOUT`.
    ///
    /// The one error in this enum that means "wait and try again" rather than
    /// "something is wrong". A caller that treats every `Error` alike ends a
    /// multi-hour walk because the user added a folder while it ran, and
    /// nothing about the failure says which kind it was without unwrapping two
    /// layers of a dependency's type — so the question is answered here, beside
    /// the variant, rather than in each caller.
    ///
    /// `DatabaseLocked` as well as `DatabaseBusy`: SQLite reports the first for
    /// contention inside one connection's shared cache and the second between
    /// connections, and both are resolved by retrying rather than by anything
    /// the caller can do.
    pub fn is_contention(&self) -> bool {
        matches!(
            self,
            Error::Sqlite(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::DatabaseBusy
                    || e.code == rusqlite::ErrorCode::DatabaseLocked
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Keeps the typed cause rather than a string: callers must be able to tell
    /// `MigrationDefinition(DatabaseTooFarAhead)` — a database written by a
    /// newer Mnema, which asks the user to update — from broken migration SQL,
    /// which is a bug report. Matching on message text is not a way to do that.
    #[error("migration: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("registering the vector extension failed with sqlite code {0}")]
    ExtensionRegistration(i32),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// Checked in application code, ahead of the schema's own guards: those
    /// catch a span naming the wrong block, not one whose `end` is simply past
    /// the text it claims to measure. A locator that lies about its own text
    /// is the thing a highlight built from it cannot recover from later.
    #[error("invalid locator: {0}")]
    InvalidLocator(String),
    #[error("no embedding space with id {0}")]
    NoSuchSpace(i64),
    #[error("no model config with id {0}")]
    NoSuchModelConfig(i64),
    /// The schema's own CHECK already refuses this at every write path this
    /// crate exposes, so reaching it means a row was written around that
    /// path — raw SQL, a hand-edited database, an older Mnema's bug. A typed
    /// error rather than a panic: this is read inside a multi-hour indexing
    /// job, and one bad row must not take the whole run down with it.
    #[error("document.status holds {0:?}, which is outside pending/indexed/failed/skipped")]
    UnknownDocumentStatus(String),
    /// Mirrors `UnknownDocumentStatus` for the same reason: `skipped.rule` has
    /// no CHECK, so reaching this means a row was written around every write
    /// path this crate exposes.
    #[error("skipped.rule holds {0:?}, which is not a known SkipRule")]
    UnknownSkipRule(String),
    /// Both widths are in the message because the interesting question is which
    /// of the two is wrong, and the caller passed only one of them.
    #[error(
        "embedding space of {space_dim} dimensions does not match \
         model config {model_config_id}, which produces {config_dim}"
    )]
    SpaceDimMismatch {
        model_config_id: i64,
        config_dim: i64,
        space_dim: i64,
    },
    /// Carries the existing id rather than only reporting the clash: the caller
    /// that hits this wanted get-or-create, and this is the half it was missing.
    /// Without it the only way to recover is to match on "UNIQUE constraint
    /// failed" in a message, which is what typed errors are here to avoid.
    #[error("embedding space {space_id} already has this model, width, format and chunker")]
    SpaceAlreadyExists { space_id: i64 },
    /// The one `meta` key whose overwrite loses data rather than a diagnosis,
    /// and it loses it silently: the replaced space's vectors are still on
    /// disk, still complete, and no longer reachable by anything, while search
    /// answers from a space that holds a fraction of the archive. No error, no
    /// missing row — just an index that has quietly stopped containing what it
    /// contains.
    ///
    /// So the key does not go through `meta_set` at all. Changing which space
    /// is active is a decision about every vector already written, and it is
    /// made by the adoption path, which can refuse while the space being left
    /// behind still holds rows; `meta_set` cannot, because it sees one key and
    /// one string.
    #[error(
        "{META_ACTIVE_SPACE} cannot be written through meta_set: it would orphan \
         the vectors of the space being replaced, leaving them on disk and \
         unreachable while search answers from the new one"
    )]
    ActiveSpaceNotWritable,
    /// A space other than the one being adopted still has embeddings recorded
    /// in it, so moving the index onto another would leave them where nothing
    /// reads them — the split D25 rejected as impossible in principle rather
    /// than merely unimplemented. Doing it properly means building the new
    /// space, filling it, and switching, which is the indexing subsystem.
    ///
    /// **Not `ActiveSpaceNotEmpty`**, which is what this was called for one
    /// commit. The space that blocks need not be the active one, and need not
    /// be pointed at by anything at all; keying the question on
    /// `meta.active_space` was the defect the rename goes with. See
    /// [`Db::adopt_embedding_model`].
    ///
    /// `embedded_chunks` counts **chunks**, not rows, and the name is the
    /// message: a chunk is recorded as embedded in two places —
    /// `chunk_embedding_state` and the space's own `vec_emb_<id>` table — and
    /// they are two records of one chunk, not two things to rebuild. The
    /// message says *recorded* for the same kind of reason: where only the
    /// bookkeeping row exists, what the space holds is the record of an
    /// embedding rather than the embedding.
    #[error(
        "space {space_id} is not empty: embeddings are recorded for {embedded_chunks} of its \
         chunks, and the index cannot move to a different space without rebuilding them"
    )]
    SpaceNotEmpty { space_id: i64, embedded_chunks: i64 },
    #[error("{role} has a non-finite component at index {index}")]
    NonFiniteVector { role: VectorRole, index: usize },
    /// vec0 divides by the vector's norm in f32, and every way that division can
    /// go wrong is silent. Zero or subnormal: `distance = NULL` or `-inf`, both
    /// sorting ahead of an exact match — the first invisibly, the second past
    /// any `NULLS LAST`. Overflowing: a finite, confident, wrong `1.0`, which
    /// buries the exact match instead of promoting anything. Measured at 1e-25,
    /// all-zeros and 1e20 respectively.
    #[error(
        "{role} cannot be ranked: its squared norm is {squared_norm:e}, which f32 \
         cannot divide by, so vec0 would score it NULL, -inf, or a confident wrong 1.0"
    )]
    UnrankableVector { role: VectorRole, squared_norm: f32 },
}

mod journal;
mod migrations;
mod open;
mod search;
mod space;
mod text_prep;
mod write;

pub use journal::{DocumentStatus, SkipEntry, SkipRule, SkippedFile};
pub use migrations::SCHEMA_VERSION;
pub use open::{Db, open, register_vector_extension};
pub use space::VectorRole;
pub use text_prep::prepare_for_search;
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

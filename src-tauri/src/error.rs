use serde::{Serialize, Serializer};

/// What a command can fail with.
///
/// Typed rather than `String`: a test that wants "the index was not open" can
/// name that case instead of matching on message text, which is the failure mode
/// `mnema_index::Error` already exists to avoid.
///
/// It crosses to the webview as its `Display` string, because a string is the
/// only shape the IPC has for a rejected command.
///
/// Nothing here carries a model-provider credential and nothing may start to.
/// An error message is a log line, and a log line is a place a key leaks from.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the index is not open")]
    IndexNotOpen,
    #[error("could not create the data directory {path}: {source}")]
    DataDir {
        path: String,
        source: std::io::Error,
    },
    #[error("index: {0}")]
    Index(#[from] mnema_index::Error),
    /// A window sent a `root_id` `watched_root` has no row for — a folder
    /// removed by a second window, or a page that reloaded with a stale id
    /// still in its own list. `start_walk_job` cannot walk a row number, only
    /// the path it names.
    #[error("no watched folder with id {0}")]
    UnknownWatchedRoot(i64),
    /// Indexing could not continue at all — the extraction pool is broken, or
    /// the database is.
    ///
    /// Deliberately **not** the way a file that could not be read reaches the
    /// window. `mnema_ingest::ingest_file` returns those as
    /// `Ingested::Skipped`, and they belong in the skip journal and in the
    /// job's counters; turning one into a rejected command would stop a run
    /// over forty thousand files because one of them is a format with no
    /// reader.
    #[error("indexing: {0}")]
    Ingest(#[from] mnema_ingest::IngestError),
    /// One job at a time is the execution model, not a limitation of the probe.
    /// PDF extraction is serialised within the process (D35) and the index has a
    /// single writer, so a second concurrent job would contend for both.
    #[error("a job is already running")]
    JobAlreadyRunning,
    /// Something panicked while holding a lock in the shared state. Reported
    /// rather than re-panicked: taking the webview down with it would turn a
    /// recoverable command failure into a lost window.
    #[error("internal state was left inconsistent by an earlier failure")]
    StatePoisoned,
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

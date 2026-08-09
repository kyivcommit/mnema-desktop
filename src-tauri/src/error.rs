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
    /// The provider answered, and the answer was not the one asked for — a key
    /// it refused, a model it does not have, a body this build could not read.
    ///
    /// Its `Display` is safe to show: no variant of `mnema_provider::Error` can
    /// carry the key — everything it keeps from a provider body has been through
    /// that crate's sanitising pipeline — and
    /// `crates/mnema-provider/tests/probe.rs` holds it to that by running every
    /// failure path and searching the rendering for the key it was given.
    ///
    /// **Not `#[from]`**, and not because a manual conversion is nicer: see the
    /// `From` implementation below for the one variant it has to peel off first.
    #[error("provider: {0}")]
    Provider(mnema_provider::Error),
    /// Nobody answered. The request never reached a provider, so nothing was
    /// refused and nothing was decided about the key.
    ///
    /// Split out from [`Error::Provider`] because the two ask the person at the
    /// window for opposite things. A key the provider refused needs a different
    /// key; a provider that could not be reached needs the same key again later,
    /// and someone who is shown one message for both retypes a working
    /// credential while their network is down. Both facts are true statements
    /// today — the `Display` strings already differ — but they left this layer
    /// as one shape, so nothing above it could branch without matching on text.
    ///
    /// `detail` is `ureq`'s own error text, taken through `Display` and never
    /// `Debug`, and never the request: the key is in a header of the request
    /// this error came from (`crates/mnema-provider/src/http.rs:100`).
    #[error("the provider could not be reached: {detail}")]
    ProviderUnreachable { detail: String },
    /// The OS credential store could not be reached, or would not answer.
    ///
    /// Safe to show for the same reason and by the same argument:
    /// `mnema_secrets::Error` names the reference and never the secret, and
    /// deliberately does not wrap a `keyring_core::Error`, three of whose
    /// variants structurally hold credential material.
    ///
    /// There is deliberately no variant for "no key has been entered" beside
    /// it. Nothing in this build needs the key yet, so nothing could construct
    /// one, and the distinction such a variant would carry — that nobody having
    /// entered a key is a normal state of the application while a keychain that
    /// will not open is a failure — is already made by the shape of
    /// [`crate::models::key_present`], which answers the first with `Ok(false)`
    /// and only the second with an `Err`. A variant no code can produce cannot
    /// be seen to go red: it would state the distinction without keeping it.
    #[error("credential store: {0}")]
    Secrets(#[from] mnema_secrets::Error),
    /// Something panicked while holding a lock in the shared state. Reported
    /// rather than re-panicked: taking the webview down with it would turn a
    /// recoverable command failure into a lost window.
    #[error("internal state was left inconsistent by an earlier failure")]
    StatePoisoned,
}

/// Splits the one provider failure that is not about the provider's answer.
///
/// Written out rather than `#[from]` so that every `?` on a `mnema_provider::Error`
/// gets the split, wherever it is written. Doing it at one call site would leave
/// the next command to remember, and "remember to keep two facts apart" is the
/// mistake this exists to stop.
impl From<mnema_provider::Error> for Error {
    fn from(e: mnema_provider::Error) -> Self {
        match e {
            mnema_provider::Error::Transport(detail) => Error::ProviderUnreachable { detail },
            answered => Error::Provider(answered),
        }
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

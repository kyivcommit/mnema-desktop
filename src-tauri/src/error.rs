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
    /// `ask`'s query is longer than the server accepts (`app/api/ask.py:17`,
    /// `Field(max_length=2048)`). Carries the count and the limit and nothing
    /// else — never the query text, which could be the sensitive part of what
    /// somebody typed.
    #[error("the question is too long: {chars} characters, the limit is {limit}")]
    QueryTooLong { chars: usize, limit: usize },
    /// `ask`'s and `search`'s query is blank — empty or whitespace only. The
    /// server's `Field(..., min_length=1)` (`app/api/ask.py:17`) rejects the
    /// empty string; trimming rejects a whitespace-only question too, since it
    /// is as meaningless and would still send a billable query embed before
    /// finding anything to search or answer. No payload — there is nothing
    /// about a blank question worth carrying, and the query text stays out of
    /// the log line.
    #[error("the question is blank")]
    QueryBlank,
    #[error("could not create the data directory {path}: {source}")]
    DataDir {
        path: String,
        source: std::io::Error,
    },
    /// `locale::write_choice` failed — the prefs file could not be written
    /// (permissions, a full disk, a data dir that vanished underneath it).
    /// `#[from]`, unlike [`Error::DataDir`] above: there is no separate path
    /// to attach here, since `write_choice` already names the file inside its
    /// own `std::io::Error` context where one is available, and this is the
    /// first variant that needs `From<std::io::Error>` at all.
    #[error("could not write preferences: {0}")]
    Prefs(#[from] std::io::Error),
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
    /// D106: two independent toggles, and at least one is always on.
    /// `set_search_arms` refuses here so a meta row nothing rereads to check
    /// cannot make that sentence false. Proven by
    /// `set_search_arms_refuses_to_turn_off_both_arms`.
    #[error("at least one search arm must stay on")]
    NoSearchArm,
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
    /// The request did not complete, so nothing was refused and nothing was
    /// decided about the key.
    ///
    /// Deliberately not "the request never reached a provider", which is more
    /// than the mapping behind it can support: this is built from every
    /// `ureq` error (`crates/mnema-provider/src/http.rs:104`), and three of
    /// those contradict that sentence — a timeout, where the request may well
    /// have arrived and the answer merely did not come back in time; too many
    /// redirects, where the provider answered repeatedly; and a base address
    /// this build malformed, which is our own defect and not the network's.
    /// The sentence shown to a person is inherited from
    /// `mnema_provider::Error::Transport` and unchanged; what is corrected here
    /// is a doc that claimed a cause nobody established.
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
    /// this error came from (`crates/mnema-provider/src/http.rs:71,90`).
    #[error("the provider could not be reached: {detail}")]
    ProviderUnreachable { detail: String },
    /// The OS credential store could not be reached, or would not answer.
    ///
    /// Safe to show for the same reason and by the same argument:
    /// `mnema_secrets::Error` names the reference and never the secret, and
    /// deliberately does not wrap a `keyring_core::Error`, three of whose
    /// variants structurally hold credential material.
    ///
    /// "No key has been entered" is [`Error::NoKey`] beside it and not this
    /// one. Until a command needed the key there was no such variant, on the
    /// ground that a variant no code can produce cannot be seen to go red;
    /// `models::set_embedding_model` is the command that produces it, and the
    /// distinction is stated there.
    #[error("credential store: {0}")]
    Secrets(#[from] mnema_secrets::Error),
    /// A command that needs the key ran without one.
    ///
    /// Kept apart from [`Error::Secrets`], which is the store failing to
    /// answer, because the two ask the person at the window for opposite
    /// things: nobody having entered a key is a normal state of the
    /// application with a sign-in panel behind it, while a keychain that will
    /// not open is a failure, and telling someone to type a key they have
    /// already entered is what one message for both produces. It is the same
    /// line [`crate::models::key_present`] draws in its shape — `Ok(false)`
    /// against `Err` — for the command that asks the question directly.
    #[error("no provider key has been entered")]
    NoKey,
    /// [`crate::models::set_key`] was handed an empty string.
    ///
    /// Kept apart from [`Error::NoKey`], which is about the **store** and not
    /// about what was submitted: somebody with a working key who presses the
    /// button with the box empty would be told "no provider key has been
    /// entered", which is false about their machine and sends them looking for
    /// a key they already have.
    ///
    /// Kept much further apart from [`Error::Provider`], which is what the
    /// empty string used to become. `set_key` handed it straight to
    /// `check_key`, a request went out carrying an empty bearer token, and the
    /// provider's own "Missing Authentication header" reached the window as
    /// *"the key was not saved: provider: the key was refused: Missing
    /// Authentication header"*. Nobody had typed a key, so nothing had been
    /// refused — two facts, one message, and the first one a person meets by
    /// doing the most ordinary thing on the screen (the acceptance run, item
    /// 1).
    ///
    /// The sentence has to carry both facts on its own: this type crosses the
    /// IPC as its `Display` string and nothing above can branch on the variant.
    #[error(
        "an empty key was submitted, so nothing was sent to the provider and nothing was checked"
    )]
    EmptyKey,
    /// The window asked for a list of models in a role this build has none.
    ///
    /// A refusal and not a default. [`mnema_provider::Role`] has three values,
    /// the role crosses the IPC as a string, and `Role::Chat` is the one whose
    /// list is unfiltered — so a typo falling through to it would answer a
    /// question about embedders with the whole chat catalogue, and the window
    /// would draw it as the answer.
    ///
    /// `{0:?}` and never `{0}`: this is caller text reaching a rendered
    /// sentence, and a newline in it would cut a log line in half and let the
    /// remainder pass for an entry of its own. The same rule
    /// `mnema_provider::Refusal::LimitNotUnderstood` states for the provider
    /// text it carries.
    #[error("no such role: {0:?}")]
    UnknownRole(String),
    /// Something panicked while holding a lock in the shared state. Reported
    /// rather than re-panicked: taking the webview down with it would turn a
    /// recoverable command failure into a lost window.
    #[error("internal state was left inconsistent by an earlier failure")]
    StatePoisoned,
    /// A confirmed model change destroyed something and then could not finish.
    ///
    /// **This variant exists because the sentence has to carry a fact the
    /// failure would otherwise take with it.** `set_embedding_model` with
    /// [`crate::models::ExistingVectors::Discard`] retires a space, commits
    /// that, and only then adopts the model. A failure in between used to leave
    /// through [`Error::Index`], which says what went wrong and nothing about
    /// what had already gone — so the window drew "the embedding model was not
    /// recorded", a sentence that reads as *nothing happened*, to somebody whose
    /// embeddings had just been deleted. The retirement is not undone by the
    /// failure and nothing later mentions it either: the space is simply no
    /// longer in `embedding_space`, which is exactly why nothing can notice it
    /// afterwards.
    ///
    /// **Not [`Error::Index`] with a longer message**, and not a wrapper the
    /// caller may forget: the only way to build it is
    /// `models::failure_after_retiring`, which is also the only place `retired`
    /// can be dropped, so the two facts cannot be separated by an ordinary edit
    /// at a call site.
    ///
    /// It carries no credential and cannot: `RetiredSpace` is two integers.
    #[error(
        "{}, and that is not undone by what went wrong next: {source}",
        retirements(.retired)
    )]
    RetiredThenFailed {
        retired: Vec<crate::models::RetiredSpace>,
        source: mnema_index::Error,
    },
}

/// The retirements as a clause, for [`Error::RetiredThenFailed`]'s sentence.
///
/// Both numbers on every space, because "a vector space was retired" is not what
/// a person needs to know — which one, and how much was in it, is. Never called
/// with an empty list: `models::failure_after_retiring` answers [`Error::Index`]
/// for that case, and this would otherwise render an empty clause in front of a
/// cause, which reads as a sentence that lost its subject.
fn retirements(retired: &[crate::models::RetiredSpace]) -> String {
    retired
        .iter()
        .map(|space| {
            format!(
                "vector space {} and the {} embeddings recorded in it were already deleted",
                space.space_id, space.embedded_chunks
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `write_choice(...)?` inside `locale::apply_choice` needs `Error` to
    /// implement `From<std::io::Error>`. Red-first: this fails to compile
    /// until `Error::Prefs` exists, and once it does, the IPC boundary's only
    /// shape for a rejected command — the `Serialize` impl above, which emits
    /// `Display` — must carry both the fixed prefix and the io error's own
    /// text, not swallow either.
    #[test]
    fn prefs_error_serializes_to_its_display_string() {
        let err = Error::Prefs(std::io::Error::other("disk full"));
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(
            json,
            serde_json::Value::String("could not write preferences: disk full".to_string())
        );
    }
}

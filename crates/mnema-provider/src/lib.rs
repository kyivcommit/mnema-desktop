//! Everything this product knows about talking to a model provider.
//!
//! v1 speaks to OpenRouter alone (spec §2.2). The base URL is a parameter
//! rather than a constant inside the request functions, because the tests point
//! it at a local server — and because v2's second provider arrives as another
//! base URL rather than as another code path.

mod catalogue;
mod http;

pub use catalogue::{
    Catalogue, MIN_CONTEXT_TOKENS, ModelEntry, RecordId, Refusal, Role, UnreadableRecord,
    models_from_json,
};

/// Where v1 goes. Not a configuration: v1 has one provider (spec §2.2).
pub const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";

/// Asks the provider for one role's models. The key is optional because this
/// endpoint is public — measured 2026-08-08 — which is what lets the choice be
/// shown before an account exists (spec §2.3).
///
/// An empty catalogue (`{"data":[]}`) is a success, not a failure. Whether
/// zero selectable models is actionable is a question for whoever renders the
/// result — the shell sees the full `Catalogue`, including `unreadable`, and
/// can tell "the provider genuinely has none" from "something upstream ate
/// them" far better than this function can guess. Turning an otherwise
/// well-formed, on-topic answer into an error here would hide it instead of
/// reporting it (Task 2 review, item 3).
pub fn list_models(base: &str, key: Option<&str>, role: Role) -> Result<Catalogue, Error> {
    let path = match role.query() {
        Some(filter) => format!("/models?output_modalities={filter}"),
        None => "/models".to_string(),
    };
    let (status, body) = http::get(base, &path, key)?;
    match status {
        200 => models_from_json(role, &body),
        // Four combinations, four true statements (Task 2 review round 2,
        // G1). 401 means the request was not authenticated: with no key sent,
        // this endpoint now requires one; with a key, that key was refused.
        // 403 means the request WAS authenticated (or none was needed) and
        // still refused: with a key, that key is not permitted to do this;
        // with none, something between this machine and the provider refused
        // an anonymous request — on a public endpoint that is most often a
        // proxy or a gateway, not an account. `list_models` is the one place
        // that knows whether a key was sent at all, so the split happens
        // here, not in `Error`.
        401 if key.is_none() => Err(Error::KeyRequired),
        401 => Err(Error::Unauthorised),
        403 if key.is_some() => Err(Error::Forbidden),
        403 => Err(Error::AnonymousBlocked),
        429 => Err(Error::RateLimited),
        other => Err(Error::Provider { status: other }),
    }
}

/// What a call to the provider can fail with.
///
/// **No variant may carry the key**, and `tests/probe.rs` holds this to it by
/// running every failure path and searching the rendered message for the key
/// it was given. An error message is a log line, and a log line is a place a
/// key leaks from — the same sentence `src-tauri/src/error.rs` already carries.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the provider could not be reached: {0}")]
    Transport(String),
    /// 401 with a key sent: the request was authenticated, and refused.
    #[error("the key was refused")]
    Unauthorised,
    /// 401 on a call that sent no key at all (Task 2 review round 1, F2): not
    /// a credential problem, since there was no credential — the provider now
    /// requires one for an endpoint this build calls anonymously.
    #[error("this endpoint now requires a key, and none was sent")]
    KeyRequired,
    /// 403 with a key sent: the request was understood, the key is real, and
    /// it is not permitted to do this (Task 2 review round 2, G1) — a
    /// different fact from `Unauthorised`, which means the key itself was
    /// rejected outright.
    #[error("the key is not permitted to do this")]
    Forbidden,
    /// 403 on a call that sent no key at all (Task 2 review round 2, G1).
    /// Not naming an account: on a public, key-less endpoint this is most
    /// often something between this machine and the provider — a proxy or a
    /// gateway — refusing an anonymous request, which `Unauthorised` and
    /// `KeyRequired` would both misname as a credential problem when there
    /// was no credential to begin with.
    #[error("something between this machine and the provider refused an anonymous request")]
    AnonymousBlocked,
    #[error("no model named {0}, or it does not make embeddings")]
    NoSuchModel(String),
    /// Deliberately does not say "this key" (Task 2 review round 1, F2):
    /// `list_models`'s public list is called with no key at all, and
    /// anonymous rate limiting on a public endpoint is real — the sentence
    /// would be false exactly when the user is not signed in.
    #[error("the provider is rate-limiting requests")]
    RateLimited,
    #[error("the provider answered {status}")]
    Provider { status: u16 },
    #[error("the provider's answer was not the shape this code expects: {0}")]
    Malformed(&'static str),
    /// The status was read successfully, but reading the rest of the
    /// response failed — kept apart from `Transport`, which means the
    /// opposite: no answer was ever read at all. `detail` is `ureq`'s own
    /// protocol-error text, not provider bytes.
    ///
    /// The top-level sentence says nothing about *why* the read failed on
    /// purpose (Task 2 review round 2, G2): a genuine truncation is the
    /// provider's connection stopping, but `ureq` raises the same error for
    /// `BodyExceedsLimit` (`read_to_string` caps at 10 MB) and for a global
    /// timeout during the body read, and in both of those the provider did
    /// nothing wrong — this build did. `detail` carries whichever of the
    /// three it actually was.
    ///
    /// This bypasses the status table in `list_models` entirely (Task 2
    /// review round 2, G3, deliberately not fixed here): a 401 whose body
    /// happens to be truncated arrives as `BodyUnreadable { status: 401, .. }`,
    /// not `Unauthorised`, and will not open a key dialog that keys off that
    /// variant. Reinterpreting the status inside the body-read failure would
    /// rebuild F1 — a status and a body-read outcome flattened back into one
    /// answer — so whoever matches on `Error` for that dialog must check
    /// `BodyUnreadable`'s `status` field too, not only `Unauthorised`.
    #[error("the provider answered {status}, but reading the response body failed: {detail}")]
    BodyUnreadable { status: u16, detail: String },
    /// The trap this whole subsystem exists to catch (spec §2.6): two texts in
    /// one request came back as a single vector, so the model averages a batch
    /// instead of embedding each text. Measured on Google's embedder,
    /// 2026-07-25, skeleton §6.2.
    #[error("this model returns one averaged vector for a batch, so it cannot embed an archive")]
    AveragedBatch,
    #[error("the provider returned an empty vector")]
    EmptyVector,
}

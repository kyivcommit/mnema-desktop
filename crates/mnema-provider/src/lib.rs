//! Everything this product knows about talking to a model provider.
//!
//! v1 speaks to OpenRouter alone (spec §2.2). The base URL is a parameter
//! rather than a constant inside the request functions, because the tests point
//! it at a local server — and because v2's second provider arrives as another
//! base URL rather than as another code path.

mod catalogue;
mod http;

pub use catalogue::{
    Catalogue, MIN_CONTEXT_TOKENS, ModelEntry, Refusal, Role, UnreadableRecord, models_from_json,
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
        401 | 403 => Err(Error::Unauthorised),
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
    #[error("the key was refused")]
    Unauthorised,
    #[error("no model named {0}, or it does not make embeddings")]
    NoSuchModel(String),
    #[error("the provider is rate-limiting this key")]
    RateLimited,
    #[error("the provider answered {status}")]
    Provider { status: u16 },
    #[error("the provider's answer was not the shape this code expects: {0}")]
    Malformed(&'static str),
    /// The trap this whole subsystem exists to catch (spec §2.6): two texts in
    /// one request came back as a single vector, so the model averages a batch
    /// instead of embedding each text. Measured on Google's embedder,
    /// 2026-07-25, skeleton §6.2.
    #[error("this model returns one averaged vector for a batch, so it cannot embed an archive")]
    AveragedBatch,
    #[error("the provider returned an empty vector")]
    EmptyVector,
}

//! Everything this product knows about talking to a model provider.
//!
//! v1 speaks to OpenRouter alone (spec §2.2). The base URL is a parameter
//! rather than a constant inside the request functions, because the tests point
//! it at a local server — and because v2's second provider arrives as another
//! base URL rather than as another code path.

mod catalogue;

pub use catalogue::{Catalogue, MIN_CONTEXT_TOKENS, ModelEntry, Refusal, Role, models_from_json};

/// Where v1 goes. Not a configuration: v1 has one provider (spec §2.2).
pub const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";

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

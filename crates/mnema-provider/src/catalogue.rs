//! The provider's model list, and the rules that decide what may be chosen.
//!
//! Measured 2026-08-08: the list is public — no key — and it is asked per role,
//! with the three answers pairwise disjoint (400 chat + 33 embedding + 6 rerank
//! = 439 unique). Neither dimensionality nor anything equivalent appears in any
//! field, which is why it is measured by a call instead (spec §2.4).
//!
//! Parsing is per-record rather than all-or-nothing: `data` is read as a list
//! of loosely-typed JSON values, and a record this code cannot make sense of is
//! counted in `Catalogue::unreadable` rather than taking the rest of the list
//! down with it. The point of `Refusal` is to show what cannot be used instead
//! of hiding it; a parse failure that hides the whole page would undo that on
//! the first odd record the provider ever sends.

use serde::Deserialize;
use serde_json::Value;

use crate::Error;

/// The smallest input limit a model may state and still be chosen.
///
/// The chunk ceiling is 1850 characters (D31), and the same thousand characters
/// are 237 tokens in English and 570 in Ukrainian — so a long chunk can pass a
/// thousand tokens. Skeleton §6.4 asks for 2048 with room to spare rather than
/// a limit met exactly. Measured 2026-08-08: this refuses 12 of the 33 models
/// the provider lists, and all twelve state exactly 512.
pub const MIN_CONTEXT_TOKENS: i64 = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Embedding,
    Rerank,
    Chat,
}

impl Role {
    /// The `output_modalities` filter this role's list is asked with. `None`
    /// for chat: the unfiltered list *is* the chat list, and asking for
    /// `output_modalities=text` would drop the fifteen models that also draw
    /// or speak (measured 2026-08-08).
    pub fn query(self) -> Option<&'static str> {
        match self {
            Role::Embedding => Some("embeddings"),
            Role::Rerank => Some("rerank"),
            Role::Chat => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub context_length: Option<i64>,
    /// Price of one input token, as the provider states it. `None` when it does
    /// not — shown as "price unknown" rather than as free.
    pub price_per_token: Option<f64>,
    /// `None` means selectable. Anything else is shown, greyed, with its reason
    /// (spec §2.5): a model the provider lists and we hide sends the user
    /// looking for a fault in this application.
    pub refusal: Option<Refusal>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Refusal {
    InputTooSmall {
        limit: i64,
        floor: i64,
    },
    NoStatedLimit,
    /// The provider's answer had no `architecture` field at all — this code
    /// was never told whether the model writes text, so it must not claim that
    /// text is absent. Kept apart from `NoTextOutput` so that a provider who
    /// renames or drops the field cannot make this code state, as a fact about
    /// the model, something the provider never said (F3, review round 1).
    NoStatedOutputModalities,
    /// The provider's `architecture.output_modalities` was stated, and text is
    /// not among them.
    NoTextOutput,
}

/// What `models_from_json` hands back: the models it could read, and how many
/// records it could not. A record with no usable `id` cannot be shown as a
/// model at all — there is nothing to select — so it is counted here instead
/// of vanishing without a trace (F4, review round 1).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalogue {
    pub entries: Vec<ModelEntry>,
    pub unreadable: usize,
}

#[derive(Deserialize)]
struct Listing {
    /// Loosely typed on purpose: a `Vec<Raw>` would fail to deserialize *at
    /// all* the moment one record does not fit the shape below, which is
    /// exactly the all-or-nothing failure this module exists to avoid. Each
    /// value is decoded into `Raw` independently, one record at a time.
    data: Vec<Value>,
}

#[derive(Deserialize)]
struct Raw {
    /// The one field with no fallback: a record whose `id` is missing or is
    /// not a JSON string fails to deserialize, and the caller counts that
    /// record as unreadable rather than showing a model with no name to select
    /// it by.
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "flexible_i64")]
    context_length: Option<i64>,
    #[serde(default)]
    pricing: Option<Pricing>,
    #[serde(default)]
    architecture: Option<Architecture>,
    #[serde(default)]
    top_provider: Option<TopProvider>,
}

#[derive(Deserialize)]
struct Pricing {
    #[serde(default, deserialize_with = "flexible_f64")]
    prompt: Option<f64>,
}

#[derive(Deserialize)]
struct Architecture {
    #[serde(default)]
    output_modalities: Vec<String>,
}

#[derive(Deserialize)]
struct TopProvider {
    #[serde(default, deserialize_with = "flexible_i64")]
    context_length: Option<i64>,
}

/// Reads a JSON number or a numeric string as an integer; any other shape —
/// an object, an array, a bool, or a string that does not parse — becomes
/// `None` rather than a deserialize error, so this field's odd shape does not
/// take the whole record down with it (F4, review round 1).
fn flexible_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match Value::deserialize(deserializer)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    })
}

/// The `f64` counterpart of [`flexible_i64`], for `pricing.prompt`, which the
/// provider states as a string today but is not promised to keep stating that
/// way.
fn flexible_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match Value::deserialize(deserializer)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    })
}

/// The input limit this product trusts.
///
/// The provider states this number twice per record, `context_length` and
/// `top_provider.context_length`, and does not promise they agree. Measured
/// 2026-08-08 on the live list: among 400 chat models the two disagree in 31,
/// `top_provider` the smaller every time; among the 33 embedding models and 6
/// rerank models they agree in all of them. The optimistic number is the one
/// that would let a chunk through to a model that then truncates it, so this
/// takes whichever of the two is smaller, or whichever exists when only one
/// does (F5, review round 1).
fn narrowest_limit(
    context_length: Option<i64>,
    top_provider_context_length: Option<i64>,
) -> Option<i64> {
    match (context_length, top_provider_context_length) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

/// Parses the provider's answer and applies this product's own rules.
///
/// Unknown fields are ignored by construction — the provider adds them without
/// telling anyone — while a *missing* field this code reads is a `None` the
/// rules then have to answer for, never a silent default.
pub fn models_from_json(role: Role, json: &str) -> Result<Catalogue, Error> {
    let listing: Listing = serde_json::from_str(json)
        .map_err(|_| Error::Malformed("the model list is not the object this code expects"))?;

    let mut entries = Vec::with_capacity(listing.data.len());
    let mut unreadable = 0usize;

    for value in listing.data {
        let raw: Raw = match serde_json::from_value(value) {
            Ok(raw) => raw,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };

        let context_length = narrowest_limit(
            raw.context_length,
            raw.top_provider.as_ref().and_then(|tp| tp.context_length),
        );
        let architecture_stated = raw.architecture.is_some();
        let writes_text = raw
            .architecture
            .as_ref()
            .is_some_and(|a| a.output_modalities.iter().any(|m| m == "text"));

        let refusal = match role {
            Role::Embedding => match context_length {
                Some(limit) if limit < MIN_CONTEXT_TOKENS => Some(Refusal::InputTooSmall {
                    limit,
                    floor: MIN_CONTEXT_TOKENS,
                }),
                None => Some(Refusal::NoStatedLimit),
                Some(_) => None,
            },
            Role::Chat if !architecture_stated => Some(Refusal::NoStatedOutputModalities),
            Role::Chat if !writes_text => Some(Refusal::NoTextOutput),
            Role::Chat | Role::Rerank => None,
        };

        entries.push(ModelEntry {
            name: raw.name.clone().unwrap_or_else(|| raw.id.clone()),
            price_per_token: raw.pricing.and_then(|p| p.prompt),
            id: raw.id,
            context_length,
            refusal,
        });
    }

    Ok(Catalogue {
        entries,
        unreadable,
    })
}

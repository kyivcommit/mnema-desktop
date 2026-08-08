//! The provider's model list, and the rules that decide what may be chosen.
//!
//! Measured 2026-08-08: the list is public — no key — and it is asked per role,
//! with the three answers pairwise disjoint (400 chat + 33 embedding + 6 rerank
//! = 439 unique). Neither dimensionality nor anything equivalent appears in any
//! field, which is why it is measured by a call instead (spec §2.4).

use serde::Deserialize;

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
    InputTooSmall { limit: i64, floor: i64 },
    NoStatedLimit,
    NoTextOutput,
}

#[derive(Deserialize)]
struct Listing {
    data: Vec<Raw>,
}

#[derive(Deserialize)]
struct Raw {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    context_length: Option<i64>,
    #[serde(default)]
    pricing: Option<Pricing>,
    #[serde(default)]
    architecture: Option<Architecture>,
}

#[derive(Deserialize)]
struct Pricing {
    #[serde(default)]
    prompt: Option<String>,
}

#[derive(Deserialize)]
struct Architecture {
    #[serde(default)]
    output_modalities: Vec<String>,
}

/// Parses the provider's answer and applies this product's own rules.
///
/// Unknown fields are ignored by construction — the provider adds them without
/// telling anyone — while a *missing* field this code reads is a `None` the
/// rules then have to answer for, never a silent default.
pub fn models_from_json(role: Role, json: &str) -> Result<Vec<ModelEntry>, Error> {
    let listing: Listing = serde_json::from_str(json)
        .map_err(|_| Error::Malformed("the model list is not the object this code expects"))?;

    Ok(listing
        .data
        .into_iter()
        .map(|raw| {
            let context_length = raw.context_length;
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
                Role::Chat if !writes_text => Some(Refusal::NoTextOutput),
                Role::Chat | Role::Rerank => None,
            };

            ModelEntry {
                name: raw.name.clone().unwrap_or_else(|| raw.id.clone()),
                price_per_token: raw
                    .pricing
                    .and_then(|p| p.prompt)
                    .and_then(|p| p.parse::<f64>().ok()),
                id: raw.id,
                context_length,
                refusal,
            }
        })
        .collect())
}

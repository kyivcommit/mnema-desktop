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
//! the first odd record the provider ever sends — and the same rule applies one
//! level down, to a single field: a value this build cannot read must say so,
//! never be folded into the same `None` a field that was simply never
//! mentioned would produce (N1, review round 2).

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
    /// Neither `context_length` nor `top_provider.context_length` was present.
    NoStatedLimit,
    /// At least one of `context_length` / `top_provider.context_length` was
    /// present, but in a shape this build does not understand — a JSON number
    /// with a fraction (`8192.0`, which `Number::as_i64` refuses even though it
    /// is a whole number), a string that is not an integer, or anything else
    /// neither field normally carries. Kept apart from `NoStatedLimit`, which
    /// means the opposite: nothing was said at all. Before this variant
    /// existed, a value here was read exactly like an absent one, so an
    /// embedding model whose limit the provider stated — just not in a shape
    /// this build parsed — greyed out with a reason that was false about the
    /// provider (N1, review round 2). Carries the raw text so the shape that
    /// confused this build is visible rather than swallowed.
    LimitNotUnderstood {
        raw: String,
    },
    /// Neither `architecture` nor, inside it, `output_modalities` was present
    /// — this code was never told whether the model writes text, so it must
    /// not claim that text is absent. Kept apart from `NoTextOutput` so that a
    /// provider who renames or drops either field cannot make this code state,
    /// as a fact about the model, something the provider never said (F3,
    /// review round 1). The line is drawn at `output_modalities` itself, not
    /// at `architecture`: a provider that states `architecture` but renames
    /// `output_modalities` to something else must still read as "did not say",
    /// not as "said, and text was not among it" (N2, review round 2).
    NoStatedOutputModalities,
    /// `output_modalities` was stated, and text is not among them.
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
    #[serde(default)]
    context_length: Stated,
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
    /// `Option`, not a bare `Vec` defaulting to empty: the empty case must be
    /// told apart from the missing one, or a provider that renames this field
    /// reads as "stated, and empty" instead of "never said" (N2, review round
    /// 2). An explicit JSON `null` deserializes to `None` here for free — the
    /// same as a missing key — where the old `Vec<String>` field failed to
    /// deserialize `null` at all and took the whole record down with it.
    #[serde(default)]
    output_modalities: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct TopProvider {
    #[serde(default)]
    context_length: Stated,
}

/// A single field's own answer, before it is combined with a sibling field's:
/// never mentioned, mentioned and read successfully, or mentioned in a shape
/// this build does not understand. `context_length` and
/// `top_provider.context_length` both deserialize into this rather than into
/// a bare `Option<i64>`, so that "the provider said nothing" and "the provider
/// said something this build could not parse" — a stray fraction, a unit
/// suffix, anything past `i64` — are not silently folded into the same `None`
/// (N1, review round 2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Stated {
    #[default]
    Absent,
    Number(i64),
    Unreadable(String),
}

impl<'de> Deserialize<'de> for Stated {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(match &value {
            Value::Null => Stated::Absent,
            Value::Number(n) => n
                .as_i64()
                .map(Stated::Number)
                .unwrap_or_else(|| Stated::Unreadable(value.to_string())),
            Value::String(s) => s
                .parse::<i64>()
                .map(Stated::Number)
                .unwrap_or_else(|_| Stated::Unreadable(value.to_string())),
            _ => Stated::Unreadable(value.to_string()),
        })
    }
}

/// The `f64` counterpart of the numeric half of [`Stated`], kept separate and
/// unchanged: N1 is scoped to the input limit, not to `pricing.prompt`, whose
/// own honesty question (an unparseable price also reads as "price unknown"
/// today) is recorded in the ledger for the final review rather than fixed
/// here.
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

/// What `context_length` and `top_provider.context_length`, taken together,
/// say about a record's actual input limit. A number either field could
/// actually be read as always wins over reporting a problem — an unreadable
/// sibling next to a real number costs nothing, the same leniency F4 already
/// gives every other field — and `Unreadable` is reported only when nothing at
/// all could be read but something was stated.
enum Limit {
    NotStated,
    Known(i64),
    Unreadable(String),
}

/// The provider states the input limit twice per record, `context_length` and
/// `top_provider.context_length`, and does not promise they agree. Measured
/// 2026-08-08 on the live list: among 400 chat models the two disagree in 31,
/// `top_provider` the smaller every time; among the 33 embedding models and 6
/// rerank models they agree in all of them. The optimistic number is the one
/// that would let a chunk through to a model that then truncates it, so this
/// takes whichever of the two is smaller, or whichever exists when only one
/// does (F5, review round 1).
fn combined_limit(context_length: &Stated, top_provider_context_length: &Stated) -> Limit {
    let numbers = [context_length, top_provider_context_length]
        .into_iter()
        .filter_map(|s| match s {
            Stated::Number(n) => Some(*n),
            _ => None,
        });
    if let Some(min) = numbers.min() {
        return Limit::Known(min);
    }
    for stated in [context_length, top_provider_context_length] {
        if let Stated::Unreadable(raw) = stated {
            return Limit::Unreadable(raw.clone());
        }
    }
    Limit::NotStated
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

        let top_provider_limit: Stated = raw
            .top_provider
            .as_ref()
            .map(|tp| tp.context_length.clone())
            .unwrap_or_default();
        let limit = combined_limit(&raw.context_length, &top_provider_limit);
        let context_length = match &limit {
            Limit::Known(n) => Some(*n),
            Limit::NotStated | Limit::Unreadable(_) => None,
        };

        // The line "did the provider say?" is drawn at `output_modalities`
        // itself (N2, review round 2): a provider that states `architecture`
        // but drops or renames `output_modalities` must read the same as one
        // that never mentioned `architecture` at all.
        let output_modalities = raw
            .architecture
            .as_ref()
            .and_then(|a| a.output_modalities.as_ref());
        let output_modalities_stated = output_modalities.is_some();
        let writes_text = output_modalities.is_some_and(|m| m.iter().any(|x| x == "text"));

        let refusal = match role {
            Role::Embedding => match limit {
                Limit::Known(limit) if limit < MIN_CONTEXT_TOKENS => Some(Refusal::InputTooSmall {
                    limit,
                    floor: MIN_CONTEXT_TOKENS,
                }),
                Limit::Known(_) => None,
                Limit::NotStated => Some(Refusal::NoStatedLimit),
                Limit::Unreadable(raw) => Some(Refusal::LimitNotUnderstood { raw }),
            },
            Role::Chat if !output_modalities_stated => Some(Refusal::NoStatedOutputModalities),
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

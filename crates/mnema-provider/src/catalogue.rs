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
//! mentioned would produce (N1, review round 2). [`InputLimit`] and [`Price`]
//! are that rule as types — the first live run found both fields still
//! answering in an `Option` narrower than what the provider sends.

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
    /// What the provider said about how much input this model takes — see
    /// [`InputLimit`] for why it is not an `Option<i64>`.
    pub input_limit: InputLimit,
    /// What the provider said about the price of one input token — see
    /// [`Price`] for why it is not an `Option<f64>`.
    pub price: Price,
    /// `None` means selectable. Anything else is shown, greyed, with its reason
    /// (spec §2.5): a model the provider lists and we hide sends the user
    /// looking for a fault in this application.
    pub refusal: Option<Refusal>,
}

/// What `context_length` and `top_provider.context_length`, taken together,
/// say about a record's input limit — and the shape that fact travels in.
///
/// It was a private enum flattened into `Option<i64>` on the way out, and the
/// flattening lost a fact for two of the three roles. The refusals that carry
/// it — [`Refusal::NoStatedLimit`] and [`Refusal::LimitNotUnderstood`] — are
/// `Role::Embedding` only, because the floor is an embedding rule, so for
/// rerank and chat "the provider stated no limit" and "the provider stated one
/// this build cannot read" both reached the window as `contextLength: null,
/// refusal: null` and drew the same question mark. Task 10 routed that to the
/// ledger rather than fixing it, on the grounds that recovering it changes this
/// type's wire shape; the acceptance run needed that change for [`Price`]
/// anyway, and had the harm on the screen.
///
/// `NotUnderstood` wins even next to a sibling that parsed fine — see
/// [`combined_limit`] for why that is the honest answer and not a defect (I1,
/// review round 3).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum InputLimit {
    /// Neither `context_length` nor `top_provider.context_length` was present.
    NotStated,
    /// The narrower of what the provider stated, in tokens.
    Known { tokens: i64 },
    /// At least one of the two was present in a shape this build does not
    /// understand. `raw` is the one payload in this enum the crate did not
    /// compute — provider text, capped to `MAX_RAW_LEN` so a malformed value
    /// cannot become an unbounded label in a picker (I2, review round 3), and
    /// untrusted wherever it is rendered. If it ever reaches a log line,
    /// format it with `{:?}` and never `{}` (Task 2 review round 1, spec item
    /// B): a newline inside it would cut the line in half and let provider
    /// text impersonate a log entry.
    NotUnderstood { raw: String },
}

/// What the provider said about the price of one input token.
///
/// It was an `Option<f64>`, and the acceptance run measured the live list
/// answering in more shapes than that can hold. Each of these is a different
/// thing to tell a person, and the two that arrive as a number are the ones
/// `Option` could not separate:
///
/// - a price — rendered as a price;
/// - a stated zero, which is a price and is deliberately **not** a variant of
///   its own; see [`Price::Known`];
/// - a negative number, which cannot be the cost of a token at all; see
///   [`Price::NotAPrice`]. `openrouter/auto-beta` and `openrouter/fusion` state
///   `-1`, nothing rejected it, and the window multiplied it by a million and
///   printed `$-1000000.000 per million tokens`;
/// - a value stated in a shape this build cannot read, which used to be folded
///   into the same "nothing was stated" an absent field produces — the honesty
///   question `Pricing`'s own field left open when N1 fixed the same fold one
///   field over, for the input limit;
/// - nothing said at all.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum Price {
    /// No `pricing` block, no `prompt` inside one, or an explicit JSON `null`.
    #[default]
    NotStated,
    /// A price for one input token, as the provider stated it.
    ///
    /// **A stated zero is one of these**, and that is a ruling rather than an
    /// oversight: zero is a number the provider sent, the same reading
    /// `Balance::Known { amount: 0 }` already carries one module over, and a
    /// type that hid it would be answering "how much?" with "unknown" about a
    /// provider that answered. What a zero may be *rendered* as is a different
    /// question and belongs to whoever renders it: all six rerank models state
    /// `"prompt": "0"` while being billed per search rather than per token, so
    /// "$0.000 per million tokens" on its own tells a person they will not be
    /// charged. Nothing in the payload states the per-search price —
    /// `tests/fixtures/rerank-2026-08-08.json` carries `prompt` and
    /// `completion` and no other price field — so no code here can name what
    /// the model actually costs, and the honest sentence says what was stated
    /// and stops there.
    Known { amount: f64 },
    /// The provider stated a number that cannot be the price of one token:
    /// negative, or not finite. Both are together because they leave a person
    /// with the same question and neither answers it; `raw` is what the
    /// provider actually sent, and is what tells them apart.
    ///
    /// Measured 2026-08-11 on the live list: `openrouter/auto-beta` and
    /// `openrouter/fusion` state `-1`. A non-finite value cannot arrive as a
    /// JSON number, but `"NaN"` and `"inf"` are strings `f64::from_str`
    /// accepts, and this crate reads a price stated as a string.
    ///
    /// `raw` is provider text, capped and untrusted — see
    /// [`InputLimit::NotUnderstood`] for the full rule, which is the same one.
    NotAPrice { raw: String },
    /// The provider stated something this build cannot read as a number at all
    /// — a string that does not parse, an object, a list. Kept apart from
    /// [`Price::NotStated`], which means the opposite: nothing was said.
    ///
    /// `raw` is provider text, capped and untrusted, as above.
    Unreadable { raw: String },
}

/// `rename_all_fields = "camelCase"` alongside `rename_all` (Task 3 review
/// round 4, K6, `mnema-provider`): `rename_all` alone renames variant names
/// for the tag value, not the *fields inside* a struct variant — a separate
/// attribute serde added for exactly that gap, verified in `serde_derive`
/// 1.0.229 sources (`rename_all_fields_rules` defaults to none, independent
/// of `rename_all_rule`). `limit` and `floor` are both one word, so the
/// convention has never been exercised here and nothing would go red
/// without this attribute — kept ahead of the first multi-word field this
/// enum ever gets.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
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
    /// provider (N1, review round 2).
    ///
    /// This wins even next to a sibling field that parsed fine (I1, review
    /// round 3): see `combined_limit` for why refusing is the honest answer
    /// and not the more permissive of the two candidates.
    ///
    /// `raw` is the one payload in this enum the crate did not compute — it is
    /// provider text, capped to `MAX_RAW_LEN` so a malformed value cannot
    /// become an unbounded label in a picker (I2, review round 3), and
    /// whoever renders it downstream must treat it as untrusted. If this ever
    /// reaches a log line, format it with `{:?}`, never `{}` (Task 2 review
    /// round 1, spec item B): a newline inside it would cut the line in half
    /// and let provider text impersonate a log entry.
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
    /// One entry per record counted in `unreadable`, in the order the provider
    /// sent them. `unreadable` is unchanged and stays a plain count; this is
    /// what turns it from a bare number into something a bug report can act
    /// on (Task 2 review, item 4) — 400 live models and "3 records
    /// unreadable" names nothing to go look at without it.
    pub unreadable_records: Vec<UnreadableRecord>,
}

/// Identifies one record `models_from_json` could not turn into a `ModelEntry`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadableRecord {
    /// The record's own `id`, read directly off the raw value before the
    /// failed decode, so a broken `pricing` or `architecture` block elsewhere
    /// in the record does not also cost the one field that would have named
    /// it.
    pub id: RecordId,
    /// The record's position in the provider's list (0-based). Kept even when
    /// `id` is `RecordId::Absent` — "something better than nothing": a
    /// position at least narrows a live list of hundreds down to one (Task 2
    /// review, item 4).
    pub index: usize,
}

/// What a record's `id` field said about itself, before the record's shape
/// decided whether it could become a `ModelEntry`. Three states, not folded
/// together (Task 2 review round 1, F4): no `id` key at all is a different
/// fact from an `id` key that named something which was not a JSON string —
/// `{"id":12345}` did say something, just not in the one shape `Raw::id`
/// accepts, and reporting that record as "stated no id" would be false about
/// the provider. The same distinction `Stated` draws for `context_length`,
/// one field over.
/// `rename_all_fields = "camelCase"` alongside `rename_all` (Task 3 review
/// round 4, K6) — see `Refusal`'s own doc comment for why it is here even
/// though `raw`/`id` are one word each and the attribute changes nothing
/// today.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum RecordId {
    /// No `id` key in the record, or an explicit JSON `null` — the same
    /// "nothing was said" this crate reads a missing key as everywhere else.
    Absent,
    /// An `id` key was present, but its value was not a JSON string. `raw` is
    /// capped the same way `Stated::Unreadable` caps a value it cannot read
    /// (`MAX_RAW_LEN`), and it is provider text: format it with `{:?}`, never
    /// `{}`, if it is ever logged (Task 2 review round 1, spec item B).
    NotAString { raw: String },
    /// The record carried an `id` this build could read.
    Known { id: String },
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
    /// A [`Price`] rather than an `Option<f64>`, for the reason that type's own
    /// doc gives: the field arrives in more shapes than "a number or nothing",
    /// and two of the shapes that *are* numbers are not prices.
    #[serde(default)]
    prompt: Price,
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
                .unwrap_or_else(|| Stated::Unreadable(cap_raw(value.to_string()))),
            // `s.clone()`, not `value.to_string()`: the latter re-serializes a
            // JSON string with its surrounding quotes ("8k" -> `"8k"`), which
            // would make the reason look different depending on whether the
            // provider stated a number or a string in the same broken shape
            // (I5, review round 3).
            Value::String(s) => s
                .parse::<i64>()
                .map(Stated::Number)
                .unwrap_or_else(|_| Stated::Unreadable(cap_raw(s.clone()))),
            _ => Stated::Unreadable(cap_raw(value.to_string())),
        })
    }
}

/// How much of a value this build could not read is worth keeping. Provider
/// text, unbounded and untrusted, must not become an unbounded label in a
/// model picker (I2, review round 3); 64 characters is generous for a number
/// that failed to parse. Truncation lands on a `char` boundary so a multi-byte
/// character is never split into invalid UTF-8.
const MAX_RAW_LEN: usize = 64;

fn cap_raw(mut raw: String) -> String {
    if raw.len() > MAX_RAW_LEN {
        let mut end = MAX_RAW_LEN;
        while !raw.is_char_boundary(end) {
            end -= 1;
        }
        raw.truncate(end);
    }
    raw
}

/// The price half of what [`Stated`] does for the input limit — the honesty
/// question `flexible_f64` left open, now answered rather than recorded. That
/// function read a number, a numeric string, and nothing at all, and answered
/// `None` for everything else: an object, a list and `"free"` all reached the
/// window as the same "the provider did not state a price".
impl<'de> Deserialize<'de> for Price {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(match &value {
            Value::Null => Price::NotStated,
            Value::Number(n) => match n.as_f64() {
                Some(amount) => Price::from_stated_number(amount, || n.to_string()),
                // `as_f64` answers `Some` for every number serde_json parses
                // under the features this workspace builds it with. The arm is
                // written rather than unwrapped because the alternative to
                // "this build could not read it" is a panic in a list of four
                // hundred models, and not because anything is claimed to reach
                // it.
                None => Price::Unreadable {
                    raw: cap_raw(n.to_string()),
                },
            },
            // `s.clone()`, not `value.to_string()`, for the reason [`Stated`]
            // gives one field over: the latter re-serialises a JSON string with
            // its surrounding quotes, so the same unreadable value would read
            // differently depending on how the provider spelled it (I5, review
            // round 3).
            Value::String(s) => match s.parse::<f64>() {
                Ok(amount) => Price::from_stated_number(amount, || s.clone()),
                Err(_) => Price::Unreadable {
                    raw: cap_raw(s.clone()),
                },
            },
            _ => Price::Unreadable {
                raw: cap_raw(value.to_string()),
            },
        })
    }
}

impl Price {
    /// Sorts a number the provider stated into a price and a number that
    /// cannot be one.
    ///
    /// `raw` is a closure because it is needed on one of the two paths only,
    /// and rendering it is what allocates.
    fn from_stated_number(amount: f64, raw: impl FnOnce() -> String) -> Self {
        if amount.is_finite() && amount >= 0.0 {
            Price::Known { amount }
        } else {
            Price::NotAPrice {
                raw: cap_raw(raw()),
            }
        }
    }
}

/// The provider states the input limit twice per record, `context_length` and
/// `top_provider.context_length`, and does not promise they agree. Measured
/// 2026-08-08 on the live list: among 400 chat models the two disagree in 31,
/// `top_provider` the smaller every time; among the 33 embedding models and 6
/// rerank models they agree in all of them. Where both sides are readable, the
/// narrower one wins, for the same reason (F5, review round 1): the optimistic
/// number is the one that would let a chunk through to a model that then
/// truncates it.
///
/// If either side is `Unreadable`, the whole result is `NotUnderstood`, even next
/// to a sibling that parsed fine (I1, review round 3). This function's promise
/// is "the narrower of what the provider stated", and with one side unreadable
/// it cannot keep that promise — the honest answer to "does this model hold
/// our chunk?" is "this build cannot tell", not the more permissive of the two
/// candidates. The two ways this can be wrong are not symmetric: refusing a
/// model that would have been fine is visible and recoverable, the user sees
/// the reason and picks another; accepting one that truncates is silent —
/// chunks go out, vectors come back, the index looks full, and answers cite
/// text the vector never saw. This triggers narrowly: only `Unreadable`
/// counts, never `Absent` — an ordinary record with no `top_provider` block
/// still uses whichever side is readable, exactly as before.
fn combined_limit(context_length: &Stated, top_provider_context_length: &Stated) -> InputLimit {
    for stated in [context_length, top_provider_context_length] {
        if let Stated::Unreadable(raw) = stated {
            return InputLimit::NotUnderstood { raw: raw.clone() };
        }
    }
    let numbers = [context_length, top_provider_context_length]
        .into_iter()
        .filter_map(|s| match s {
            Stated::Number(n) => Some(*n),
            _ => None,
        });
    match numbers.min() {
        Some(tokens) => InputLimit::Known { tokens },
        None => InputLimit::NotStated,
    }
}

/// Parses the provider's answer and applies this product's own rules.
///
/// Unknown fields are ignored by construction — the provider adds them without
/// telling anyone — while a *missing* field this code reads is a `None` the
/// rules then have to answer for, never a silent default.
pub fn models_from_json(role: Role, json: &str) -> Result<Catalogue, Error> {
    let listing: Listing = serde_json::from_str(json).map_err(|e| {
        // Three shapes reach this from the network, and they are three
        // different user problems (Task 2 review, item 2): an HTML error page
        // from a captive portal or a proxy is not JSON at all (`Syntax`); a
        // response cut off mid-transfer stops before the JSON closes (`Eof`);
        // and a provider error envelope (`{"error":{"message":"..."}}`) is
        // valid JSON that simply is not this shape, because it has no `data`
        // field (`Data`). Telling them apart is the difference between "check
        // your network" and "check your account".
        Error::Malformed(match e.classify() {
            serde_json::error::Category::Syntax => {
                "the provider's answer is not JSON at all — likely a proxy or gateway page, \
                 not the provider itself"
            }
            serde_json::error::Category::Eof => {
                "the provider's answer stopped in the middle of the JSON — a truncated response"
            }
            serde_json::error::Category::Data | serde_json::error::Category::Io => {
                "the provider's answer is JSON, but not the model-list shape this code expects"
            }
        })
    })?;

    let mut entries = Vec::with_capacity(listing.data.len());
    let mut unreadable = 0usize;
    let mut unreadable_records = Vec::new();

    for (index, value) in listing.data.into_iter().enumerate() {
        // Read `id` off the raw value before it is consumed below: a record
        // that fails to become a `Raw` because of some other field (a
        // `pricing` or `architecture` block in an unexpected shape) still had
        // a perfectly good id, and that id is worth keeping (Task 2 review,
        // item 4). `null` reads as `Absent`, matching `Stated`'s own rule for
        // an explicit null a few fields over.
        let id = match value.get("id") {
            None | Some(Value::Null) => RecordId::Absent,
            Some(Value::String(s)) => RecordId::Known { id: s.clone() },
            Some(other) => RecordId::NotAString {
                raw: cap_raw(other.to_string()),
            },
        };
        let raw: Raw = match serde_json::from_value(value) {
            Ok(raw) => raw,
            Err(_) => {
                unreadable += 1;
                unreadable_records.push(UnreadableRecord { id, index });
                continue;
            }
        };

        let top_provider_limit: Stated = raw
            .top_provider
            .as_ref()
            .map(|tp| tp.context_length.clone())
            .unwrap_or_default();
        // Kept whole rather than reduced to a number-or-nothing on its way into
        // `ModelEntry`: for rerank and chat nothing else carries it, since the
        // refusals below are the embedding role's (see [`InputLimit`]).
        let input_limit = combined_limit(&raw.context_length, &top_provider_limit);

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
            Role::Embedding => match &input_limit {
                InputLimit::Known { tokens } if *tokens < MIN_CONTEXT_TOKENS => {
                    Some(Refusal::InputTooSmall {
                        limit: *tokens,
                        floor: MIN_CONTEXT_TOKENS,
                    })
                }
                InputLimit::Known { .. } => None,
                InputLimit::NotStated => Some(Refusal::NoStatedLimit),
                InputLimit::NotUnderstood { raw } => {
                    Some(Refusal::LimitNotUnderstood { raw: raw.clone() })
                }
            },
            Role::Chat if !output_modalities_stated => Some(Refusal::NoStatedOutputModalities),
            Role::Chat if !writes_text => Some(Refusal::NoTextOutput),
            Role::Chat | Role::Rerank => None,
        };

        entries.push(ModelEntry {
            name: raw.name.clone().unwrap_or_else(|| raw.id.clone()),
            price: raw.pricing.map(|p| p.prompt).unwrap_or_default(),
            id: raw.id,
            input_limit,
            refusal,
        });
    }

    Ok(Catalogue {
        entries,
        unreadable,
        unreadable_records,
    })
}

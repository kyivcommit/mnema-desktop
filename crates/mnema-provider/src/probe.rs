//! The two calls that stand between a typed key and a long indexing run.
//!
//! §4.5 of the requirements asks for a cheap call at entry so a typo does not
//! surface three hours into indexing. This is that call, split in two: the key
//! is checked without a model (the credits endpoint needs none), and the
//! embedding model is checked separately, when one is chosen.

use serde::Deserialize;
use serde_json::Value;
use unicode_general_category::{GeneralCategory, get_general_category};

use crate::{Error, KeySent, error_for_status, http};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyCheck {
    pub balance: Balance,
}

/// What the provider said about the account's remaining balance — four
/// states, not two (Task 3 review round 2, ruling from the owner). Two
/// states make substituting zero shorter than thinking about it, and "0
/// credits" in front of a funded account sends its owner to pay again.
/// `Unreadable` and `EnvelopeNotUnderstood` are the pair round 1 folded
/// together under one `None` — both are this build's own defect rather than
/// a fact about the account, but at two different levels; see each variant.
///
/// Struct variants throughout, not `Known(f64)` (Task 3 review round 3, J3):
/// measured against this crate's own `#[serde(tag = "kind")]` convention —
/// the same one `Refusal`/`RecordId` already use, and the one the window's
/// code is written against — a newtype variant holding a bare `f64` compiles
/// and then fails at runtime, because serde cannot serialise a tagged
/// newtype variant whose payload is not itself a map. The window expects
/// `{"kind":"known","amount":6.5}`.
///
/// `rename_all_fields = "camelCase"` alongside `rename_all` (Task 3 review
/// round 4, K6): `rename_all` alone renames variant names for the tag
/// value, not the *fields inside* a struct variant — a separate attribute
/// serde added for exactly that gap, verified in `serde_derive` 1.0.229
/// sources (`rename_all_fields_rules` defaults to none, independent of
/// `rename_all_rule`). Every field in this crate's struct variants today is
/// one word (`amount`, `raw`, `text`, `limit`, `floor`, `id`), so the
/// convention has never been exercised and nothing here would go red
/// without this attribute — it is here so the first multi-word field this
/// crate ever gets (Task 4's) serialises under the name the window uses
/// instead of silently falling back to snake_case.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum Balance {
    /// The provider stated a number, and it was read.
    Known { amount: f64 },
    /// `data.total_credits` and `data.total_usage` both parsed — the shape
    /// this build knows — and neither stated a balance. Normal for some
    /// account types: a fact about the account, not a defect in this build.
    NotStated,
    /// `data` parsed into the shape this build knows, but the two fields
    /// together do not name a clean balance — one or both stated something
    /// in a form `Stated` could not read, or one was stated and the other
    /// was not. This build's own defect, worth a bug report: `raw` is the
    /// provider's own `data` object, sanitised the same way any other
    /// provider bytes reaching this crate are (see
    /// `ProviderMessage::from_provider_text`).
    Unreadable { raw: ProviderMessage },
    /// The answer's shape was not one this build knows at all — `data` was
    /// missing, `null`, or not an object — so it cannot even be said
    /// whether a balance was stated. A different defect from `Unreadable`,
    /// one level up: that variant means a shape this build understood
    /// still did not yield a clean pair; this one means the shape itself
    /// was never recognised.
    EnvelopeNotUnderstood,
}

/// The provider's own explanation for a refusal, once this build has
/// finished deciding whether it is safe to render (Task 3 review, item 4;
/// `Withheld` added in round 2). The shortcut this deliberately avoids is a
/// variant that holds the raw string and renders it with `#[error("{0}")]`
/// — forbidden by the same rule `Refusal::LimitNotUnderstood` and
/// `RecordId::NotAString` already keep, one module over: provider bytes are
/// never interpolated into a plain format string.
///
/// `Text` holds a `SanitisedText`, not a bare `String` (Task 3 review round
/// 3, J2): turning this type from a tuple struct into an enum in round 2
/// opened a door the compiler used to hold shut on its own. A tuple
/// struct's field can be private, keeping outside code from constructing one
/// at all; an enum variant's fields are always exactly as visible as the
/// enum, with no way to restrict them further — so `Text(String)` directly
/// would have made `ProviderMessage::Text("anything".into())` constructible,
/// unsanitised, from anywhere in the crate (Task 4's own module included,
/// the reviewer's specific concern), while this doc comment kept claiming
/// nothing else constructs one. `SanitisedText`'s own field is private and
/// it has no public constructor, so a caller with no way to produce one has
/// no way to call `Text` either — see its own doc comment.
///
/// Struct variant (`Text { text }`, not `Text(SanitisedText)`), for the same
/// serialisation reason `Balance` is struct variants throughout (Task 3
/// review round 3, J3): a newtype variant does not serialise under
/// `#[serde(tag = "kind")]`.
///
/// `rename_all_fields` alongside `rename_all` (Task 3 review round 4, K6) —
/// see `Balance`'s own doc comment for why it is here even though `text` is
/// one word and the attribute changes nothing today.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum ProviderMessage {
    /// Sanitised and safe to render — see `SanitisedText` for what enforces
    /// that.
    Text { text: SanitisedText },
    /// The provider stated something, but even after stripping and
    /// redacting, a long enough run of the key's own characters survived —
    /// a *fragment* of the key, which whole-key substring redaction
    /// structurally cannot catch (Task 3 review round 2, C1; see
    /// `contains_key_fragment`). This build refuses to render any of the
    /// surrounding text rather than risk the rest of the key riding along
    /// with a shorter placeholder swap. Kept apart from "the provider sent
    /// nothing" (`None`, one level up in `extract_provider_message`'s
    /// return, and in `ProviderMessage::new`'s): a support conversation
    /// still needs to know the provider tried to explain, even though this
    /// build would not repeat what it said.
    Withheld,
}

/// Provider text, sanitised — the only way to produce one is
/// `ProviderMessage::from_provider_text`'s pipeline (Task 3 review round 3,
/// J2). The field is not `pub`, and there is no public constructor other
/// than `from_provider_text` (private to this module), so
/// `ProviderMessage::Text { text: .. }` is unconstructible from outside
/// `probe.rs` even though the variant itself must be `pub` — a caller with
/// no way to obtain a `SanitisedText` has no way to write one in. `as_str`
/// and `Display` are the only ways out, both read-only.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct SanitisedText(String);

impl SanitisedText {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SanitisedText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Generous for a one-line refusal reason; `catalogue`'s own cap (64 bytes,
/// `MAX_RAW_LEN`) is sized for a broken number, not for a sentence like "This
/// key was disabled on 2026-08-01".
const MAX_MESSAGE_LEN: usize = 200;

/// Stands in for every redacted occurrence of the key (Task 3 review round 1,
/// C1). Fixed and content-free on purpose: a placeholder that echoed even the
/// *length* of the key back would still be more of the key than this crate
/// promises never to carry.
const REDACTED_PLACEHOLDER: &str = "[redacted]";

/// The minimum run of the key's own characters, matched case-insensitively,
/// that this build refuses to let survive into a rendered message (Task 3
/// review round 2, C1) — the property whole-key substring redaction cannot
/// close on its own, since a provider that echoes only a *fragment* of a
/// rejected credential has nothing in it for `redact_key` to find. Twelve
/// against a measured fact, not a guess: this provider's own keys share a
/// nine-character prefix, so a shorter window would flag that ordinary
/// shared prefix as a leak on every key, and a longer one would let a
/// shorter, still-identifying fragment through unflagged.
const FRAGMENT_LEN: usize = 12;

impl ProviderMessage {
    /// Builds a message from unsanitised provider bytes. Order matters and
    /// is fixed (Task 3 review round 2, C1): **strip, then redact, then
    /// cap** — reversed from round 1, which redacted first. Redacting first
    /// was the hazard itself, not merely an inferior choice: stripping
    /// cannot *split* a match, because none of a key's own characters are
    /// strippable — it can only ever *create* one, by removing something
    /// sitting between two of the key's own characters. A body of `invalid
    /// key: test-key<U+001F>-not-a-real-one` fails a substring match
    /// against the whole key at the redact step; stripping the control
    /// character afterward reassembles the clean key with nothing left to
    /// catch it. Strip-then-redact closes that: the control character is
    /// gone before redaction ever looks.
    ///
    /// Never fails on its own — the least this returns is
    /// `Text { text: "" }`. `new`, below, is the caller that treats an
    /// empty result as "nothing to show"; `Balance::Unreadable`'s `raw` is a
    /// caller that does not, since an empty *sanitised* value is still worth
    /// keeping there — what made a balance unreadable was its shape, not the
    /// length of what survived sanitising it.
    fn from_provider_text(raw: &str, key: &str) -> Self {
        // 1. Strip first — see the comment above for why the order is load-bearing.
        let stripped: String = raw.chars().filter(|c| !unsafe_for_display(*c)).collect();
        // 2. Redact whole-key occurrences.
        let redacted = redact_key(&stripped, key);
        // 3. A fragment is a shape substring redaction cannot catch on its
        //    own — see `contains_key_fragment`.
        if contains_key_fragment(&redacted, key) {
            return ProviderMessage::Withheld;
        }
        ProviderMessage::Text {
            text: SanitisedText(cap(redacted.trim().to_string(), MAX_MESSAGE_LEN)),
        }
    }

    /// `redaction` says what to redact — see `Redaction`'s own doc comment
    /// for why this is a type and not a bare `&str`. `None` when the
    /// sanitised result is empty: "the provider said nothing worth showing"
    /// is decided here, not inside `from_provider_text`, which every other
    /// caller needs to NOT decide for it (see that method's own doc
    /// comment).
    ///
    /// `pub(crate)` (Task 3 review round 4, K3, narrowing round 3's `pub`):
    /// this is the controlled front door `SanitisedText` has no other way in
    /// through, so a caller elsewhere in this crate — Task 4's own module,
    /// say — reaching for it instead of copying a narrower version is the
    /// intended path, the same reasoning that keeps it `pub(crate)` rather
    /// than private outright. Not `pub`: nothing outside this crate has a
    /// reason to sanitise arbitrary text through this exact pipeline, and
    /// the previous round's only real caller (`tests/probe.rs`) now has its
    /// own independent oracle instead (K2) and does not call this at all.
    pub(crate) fn new(raw: &str, redaction: Redaction<'_>) -> Option<Self> {
        match Self::from_provider_text(raw, redaction.as_str()) {
            ProviderMessage::Text { text } if text.as_str().is_empty() => None,
            other => Some(other),
        }
    }
}

/// Whether `ProviderMessage::new` should try to redact a credential,
/// spelled out as a type rather than an ordinary `&str` that happens to be
/// empty (Task 3 review round 4, K3). `new` used to take `key: &str`
/// directly, and the one worked example anywhere in this repository of
/// calling it passed `""` — compiling, clippy-clean, and silently disabling
/// both `redact_key` and `contains_key_fragment` for whoever reached for it
/// as a template outside a test. `Redaction::None` says the same thing in a
/// way a reader — and a future caller copying an example — has to notice
/// instead of skim past.
pub(crate) enum Redaction<'a> {
    /// Redact this credential — the shape every real call site (`check_key`
    /// and everything it threads a key through) reaches for.
    Key(&'a str),
    /// Explicitly opt out of redaction. This crate's own tests are the only
    /// legitimate caller: sanitising arbitrary text with no credential at
    /// stake, to exercise the strip step on its own. `#[allow(dead_code)]`:
    /// every constructor of this variant lives under `#[cfg(test)]`, so the
    /// lib target alone (no test code compiled in) has none — the variant
    /// exists for testing, not despite being unreachable outside it.
    #[allow(dead_code)]
    None,
}

impl<'a> Redaction<'a> {
    fn as_str(&self) -> &'a str {
        match self {
            Redaction::Key(k) => k,
            Redaction::None => "",
        }
    }
}

impl std::fmt::Display for ProviderMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderMessage::Text { text } => write!(f, "{text}"),
            ProviderMessage::Withheld => {
                f.write_str("the provider's explanation could not be shown safely")
            }
        }
    }
}

/// Replaces every case-insensitive occurrence of `key` inside `text` with a
/// fixed placeholder (Task 3 review round 1, C1). Called on already-stripped
/// text (`from_provider_text` runs this second, not first — see its own doc
/// comment). A substring search, not a check for exact equality against the
/// whole message: the provider is under no obligation to echo a rejected
/// credential back verbatim and alone — a leading "Bearer " scheme word
/// still attached, or a different case than this build sent, are both real
/// shapes a provider can choose to send, and both still contain the key as a
/// substring, which is what this matches against. Case-insensitive via
/// `to_ascii_lowercase`, not full Unicode case-folding: it preserves byte
/// length, which keeps every position found in the lowercased copy valid in
/// the original — full case-folding can change the byte length of some
/// characters and would break that correspondence. Real keys are ASCII, so
/// this loses nothing in practice.
fn redact_key(text: &str, key: &str) -> String {
    if key.is_empty() {
        return text.to_string();
    }
    let lower_key = key.to_ascii_lowercase();
    let lower_text = text.to_ascii_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut tail = text;
    let mut lower_tail = lower_text.as_str();
    while let Some(pos) = lower_tail.find(&lower_key) {
        result.push_str(&tail[..pos]);
        result.push_str(REDACTED_PLACEHOLDER);
        tail = &tail[pos + key.len()..];
        lower_tail = &lower_tail[pos + key.len()..];
    }
    result.push_str(tail);
    result
}

/// True if `text` contains any run of `FRAGMENT_LEN` consecutive characters
/// taken from `key`, matched case-insensitively. `key` shorter than
/// `FRAGMENT_LEN` has no such run to look for, and this returns `false` —
/// not a reachable case for a real credential from this provider, whose keys
/// are far longer than the window.
fn contains_key_fragment(text: &str, key: &str) -> bool {
    let lower_text = text.to_ascii_lowercase();
    let lower_key: Vec<char> = key.to_ascii_lowercase().chars().collect();
    if lower_key.len() < FRAGMENT_LEN {
        return false;
    }
    lower_key
        .windows(FRAGMENT_LEN)
        .any(|window| lower_text.contains(&window.iter().collect::<String>()))
}

/// True for anything unsafe to place in a rendered message or a log line.
/// By Unicode general category, not a handwritten list of code points (Task
/// 3 review round 3, J5): rounds 1 and 2 named `char::is_control` (category
/// `Cc`) plus a hand-picked set of bidi and zero-width characters (category
/// `Cf`, mostly) one at a time, as each was found — and the reviewer
/// measured that this stays open-ended by construction: three U+2060 WORD
/// JOINER insertions, spaced no more than eleven characters apart, rendered
/// a key verbatim past both the strip list and the fragment check, because
/// `Cf` has far more members than the ones this file had happened to name.
/// `Control` (`Cc`) and `Format` (`Cf`) together close that specific
/// category: every invisible or bidi-affecting character named in rounds 1
/// and 2 is `Cf`, and so is U+2060. `LineSeparator` (`Zl`, U+2028) and
/// `ParagraphSeparator` (`Zp`, U+2029) stay named separately: they are
/// category `Z`, not `C`, and stripping all of `Z` would also strip an
/// ordinary space (`Zs`), which this function must not do.
///
/// **What this does NOT close, measured rather than assumed (Task 3 review
/// round 4, K4):** Unicode's broader `Default_Ignorable_Code_Point`
/// property is not the same set as `Cc ∪ Cf ∪ Zl ∪ Zp`, and this function
/// stops at the latter. U+FE0F VARIATION SELECTOR-16, U+3164 HANGUL FILLER,
/// U+FFA0 HALFWIDTH HANGUL FILLER and U+034F COMBINING GRAPHEME JOINER are
/// all members of the derived property and none is `Cc` or `Cf` — the
/// reviewer reproduced round 2's spaced-filler attack with U+FE0F and the
/// key still renders. Left open on purpose, judged the same way round 2
/// judged an equivalently narrow gap: three evenly spaced filler characters
/// is not provider behaviour. `tests/probe.rs`'s own leak-scan oracle is
/// deliberately wider than this function (K2) precisely so it does not
/// share this specific gap while this function stays a category rather
/// than a list.
fn unsafe_for_display(c: char) -> bool {
    matches!(
        get_general_category(c),
        GeneralCategory::Control
            | GeneralCategory::Format
            | GeneralCategory::LineSeparator
            | GeneralCategory::ParagraphSeparator
    )
}

/// Truncates to at most `max` bytes without splitting a multi-byte character —
/// the same discipline `catalogue::cap_raw` keeps for provider text one
/// module over. Not shared with it: that helper is private to `catalogue.rs`,
/// and this is the only other place that needs it.
fn cap(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

#[derive(Deserialize)]
struct Credits {
    #[serde(default)]
    total_credits: Stated,
    #[serde(default)]
    total_usage: Stated,
}

/// A single field's own answer, before `balance_from` combines it with its
/// sibling: never mentioned, mentioned and read, or mentioned in a shape this
/// build does not understand. The same distinction `catalogue::Stated` draws
/// for `context_length`, needed here for the same reason (Task 3 review, item
/// 1): a plain `#[serde(default)] Option<f64>` field only falls back to
/// `None` when the *key* is absent — a `total_credits` the provider states as
/// `"$10.00"` is present, so serde tries to read it as `f64`, fails, and
/// fails the *whole* body: `serde_json::from_str::<CreditsBody>` errors out
/// before `check_key` ever learns the key worked. A perfectly good key would
/// then read as "the credits answer is not the object this code expects" —
/// false, and a worse answer to "does this key work" than simply not showing
/// a balance.
#[derive(Debug, Default)]
enum Stated {
    #[default]
    Absent,
    Number(f64),
    Unreadable,
}

impl<'de> Deserialize<'de> for Stated {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // `f.is_finite()` (Task 3 review round 1, Minor): reachable through
        // the *string* branch below, where `f64::from_str` accepts `"NaN"`
        // and `"inf"`/`"infinity"` (any case) and their signed forms. NOT
        // reachable through a JSON number literal the way a first guess
        // might suggest (Task 3 review round 2, one-liner correction):
        // measured against `serde_json`, `1e999` is rejected at parse time
        // with "number out of range" and never reaches this function at all
        // — the `Value::Number` arm's guard below is defensive, not proven
        // reachable. Either way, without this guard a non-finite value would
        // read as `Stated::Number`, and a balance of NaN would reach the
        // screen as a number that was successfully read — worse than
        // showing nothing, and it would also make `KeyCheck`'s derived
        // `PartialEq` stop being reflexive, since `NaN != NaN`.
        fn finite_or_unreadable(f: f64) -> Stated {
            if f.is_finite() {
                Stated::Number(f)
            } else {
                Stated::Unreadable
            }
        }
        Ok(match Value::deserialize(deserializer)? {
            Value::Null => Stated::Absent,
            Value::Number(n) => n
                .as_f64()
                .map(finite_or_unreadable)
                .unwrap_or(Stated::Unreadable),
            Value::String(s) => s
                .parse::<f64>()
                .map(finite_or_unreadable)
                .unwrap_or(Stated::Unreadable),
            _ => Stated::Unreadable,
        })
    }
}

#[derive(Deserialize)]
struct ProviderErrorEnvelope {
    error: ProviderErrorDetail,
}

#[derive(Deserialize)]
struct ProviderErrorDetail {
    #[serde(default)]
    message: Option<String>,
}

/// Reads `{"error":{"message":"..."}}` out of a non-200 body, when the
/// provider sent that shape, and sanitises whatever it finds
/// (`ProviderMessage::new`) before handing it back. `None` for anything
/// else: a body that does not fit is not itself a problem worth surfacing
/// here — the status already answered the one question this call promises
/// to answer.
fn extract_provider_message(body: &str, key: &str) -> Option<ProviderMessage> {
    let envelope: ProviderErrorEnvelope = serde_json::from_str(body).ok()?;
    ProviderMessage::new(&envelope.error.message?, Redaction::Key(key))
}

/// `error_for_status` cannot carry the response body — on purpose, since it
/// also serves `list_models`, which never reads a failure body for a message
/// (Task 2 review round 3, H4). Task 3's key check does read one, so the
/// provider's own explanation, when the body states one, is folded in here,
/// one layer up, instead of changing what `error_for_status` hands back to
/// its other caller too (Task 3 review, item 4).
///
/// Attached wherever the resulting error has room for it (Task 3 review
/// round 1, Minor) — every status `error_for_status` can hand `check_key`
/// (`Unauthorised`, `Forbidden`, `RateLimited`, `Provider`) carries a
/// `reason` field, not only `Unauthorised`: a 403 or a 500 can carry the
/// same `{"error":{"message":…}}` shape a 401 can, and the first cut at this
/// only checked the one variant it had a test for. `KeyRequired` and
/// `AnonymousBlocked` are absent on purpose, not an oversight — `check_key`
/// always sends a key, so `error_for_status` never returns either to it.
///
/// `pub(crate)` (Task 3 review round 4, K3, "while you are there"): this
/// function is not specific to `check_key` — it matches on `Error` itself,
/// which Task 4's embedding-model check also returns failures as — and
/// round 1's own doc comment already told Task 4 to reuse it, which a
/// module-private `fn` made impossible to act on. Reachable from elsewhere
/// in the crate now, so that recommendation is executable as written
/// instead of only in a comment nobody could follow.
pub(crate) fn attach_reason(err: Error, body: &str, key: &str) -> Error {
    let reason = || extract_provider_message(body, key);
    match err {
        Error::Unauthorised { reason: None } => Error::Unauthorised { reason: reason() },
        Error::Forbidden { reason: None } => Error::Forbidden { reason: reason() },
        Error::RateLimited { reason: None } => Error::RateLimited { reason: reason() },
        Error::Provider {
            status,
            reason: None,
        } => Error::Provider {
            status,
            reason: reason(),
        },
        other => other,
    }
}

/// Checks a key without needing a model to be chosen yet.
pub fn check_key(base: &str, key: &str) -> Result<KeyCheck, Error> {
    let (status, body) = match http::get(base, "/credits", Some(key)) {
        Ok(pair) => pair,
        // A body-read failure still carries its status (`Error::BodyUnreadable`,
        // Task 2 review round 2, G3) precisely so it is not lost — and this
        // screen's only job is "does the key work", so a 401/403/429 whose
        // body was merely cut off must give the same verdict a clean one
        // would (Task 3 review, item 2), not "reading the response body
        // failed" for a key that was, in fact, refused. Narrowed to exactly
        // those three statuses (Task 3 review round 1, Minor): they are the
        // only ones `error_for_status` turns into a *specific* verdict for a
        // key check, so swapping in that verdict is a strict improvement.
        // Every other status — a 500 whose body-read hit the 10 MB cap or a
        // timeout, say — would only trade `BodyUnreadable`'s `detail` (which
        // names *why* the read failed) for the same generic `Provider
        // { status, .. }` this crate already gives a body it read just fine,
        // buying nothing back for what it gives up. A 200 whose body could
        // not be read falls here too and is returned as-is, for the same
        // reason: this build genuinely does not know whether the key worked.
        Err(Error::BodyUnreadable { status, .. }) if matches!(status, 401 | 403 | 429) => {
            return Err(error_for_status(status, KeySent::Yes));
        }
        Err(other) => return Err(other),
    };
    match status {
        200 => {}
        other => {
            return Err(attach_reason(
                error_for_status(other, KeySent::Yes),
                &body,
                key,
            ));
        }
    }
    Ok(KeyCheck {
        balance: balance_from(&body, key)?,
    })
}

/// Reads the balance out of a 200 body, in the four states `Balance` names
/// (Task 3 review round 2, ruling from the owner). `data` missing, `null`,
/// or not an object at all reads as `EnvelopeNotUnderstood` — this build
/// never even reaches asking about `total_credits`/`total_usage` in that
/// case. Once `data` DOES parse into the shape `Credits` expects, `Stated`
/// handles each field on its own leniency (Task 3 review, item 1); this
/// function combines the two: both `Number` is `Known`, both `Absent` is
/// `NotStated`, and anything else — one or both `Unreadable`, or one stated
/// and the other not — is `Unreadable`, carrying the raw `data` object
/// (sanitised through `ProviderMessage::from_provider_text`, the same
/// pipeline any other provider bytes this crate keeps go through) rather
/// than just the one field that happened to break. That sanitising call is
/// exercised on the success path too (Task 3 review round 3, J1) — a 200
/// whose balance could not be read still carries provider bytes to the
/// screen, and every leak scan before this file's fix history only ever
/// exercised a *failure* path.
///
/// The one shape that still fails the whole call outright, rather than
/// becoming a `Balance` variant, is a body that is not JSON at all — an
/// HTML captive-portal or gateway page, say, the same case
/// `models_from_json`'s `Category::Syntax` branch names for `list_models`.
/// That is a different, stronger fact than any of `Balance`'s four states:
/// it says whatever answered was not the provider's endpoint at all, which
/// "a 200 means the key works" cannot be trusted to mean either.
fn balance_from(body: &str, key: &str) -> Result<Balance, Error> {
    let value: Value = serde_json::from_str(body).map_err(|_| {
        Error::Malformed(
            "the credits answer is not JSON at all — likely a proxy or gateway page, not the \
             provider itself",
        )
    })?;
    let Some(data) = value.get("data") else {
        return Ok(Balance::EnvelopeNotUnderstood);
    };
    let Ok(credits) = serde_json::from_value::<Credits>(data.clone()) else {
        return Ok(Balance::EnvelopeNotUnderstood);
    };
    Ok(match (credits.total_credits, credits.total_usage) {
        (Stated::Number(total), Stated::Number(used)) => Balance::Known {
            amount: total - used,
        },
        (Stated::Absent, Stated::Absent) => Balance::NotStated,
        _ => Balance::Unreadable {
            raw: sanitised_balance_summary(data, key),
        },
    })
}

/// Builds a human-readable summary of `data`'s two known fields, sanitising
/// each field's own *decoded* text directly (Task 3 review round 4, K1) —
/// not `data.to_string()`'s re-serialised form, which is what this function
/// replaces. `serde_json::Value::to_string()` re-escapes a decoded control
/// character back into six printable ASCII characters (a literal
/// backslash, "u", and four hex digits), which
/// `unsafe_for_display` no longer recognises as anything unsafe: the
/// character the strip step exists to remove has already been turned back
/// into ordinary text by the time a whole-object dump would reach it. Once
/// enough of those escapes sit inside a key, neither the exact-substring
/// redaction (the escape text breaks the match) nor the fragment net
/// (checking a segment shorter than it, between two escapes) catches what
/// is left — measured: two insertions into a 24-character key leave three
/// segments short enough that the whole key still reaches the screen, in
/// order, with the escapes as the only sign anything happened to it.
///
/// Reading each field's `Value` directly instead gets the text exactly as
/// the provider's JSON encoder produced it, decoded once and never
/// re-encoded, which is what `from_provider_text`'s first step (strip)
/// assumes. `field` is only ever `total_credits`/`total_usage` from
/// `balance_from`'s own `data.get(..)`.
fn sanitised_balance_summary(data: &Value, key: &str) -> ProviderMessage {
    fn field_text(value: Option<&Value>, key: &str) -> Option<String> {
        match value {
            Some(Value::String(s)) => match ProviderMessage::from_provider_text(s, key) {
                ProviderMessage::Text { text } => Some(text.as_str().to_string()),
                ProviderMessage::Withheld => None,
            },
            // Numbers, booleans, null, arrays and objects cannot smuggle a
            // key character-wise the way a string leaf can — JSON's own
            // number grammar has no room for arbitrary text — so these pass
            // through their own `Display` unchanged.
            Some(other) => Some(other.to_string()),
            None => Some("absent".to_string()),
        }
    }
    let (Some(credits), Some(usage)) = (
        field_text(data.get("total_credits"), key),
        field_text(data.get("total_usage"), key),
    ) else {
        // Either field alone surviving a fragment is still a fragment of
        // the key — withholding the whole summary rather than showing the
        // one field that happened to come back clean.
        return ProviderMessage::Withheld;
    };
    ProviderMessage::Text {
        text: SanitisedText(cap(
            format!("total_credits: {credits}, total_usage: {usage}"),
            MAX_MESSAGE_LEN,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(message: ProviderMessage) -> String {
        match message {
            ProviderMessage::Text { text } => text.as_str().to_string(),
            ProviderMessage::Withheld => panic!("expected Text, got Withheld"),
        }
    }

    /// Task 3 review round 1, I2: `ProviderMessage::new`'s length cap, pinned
    /// on its own — the integration tests in `tests/probe.rs` only prove a
    /// message is *delivered*, and pass just as well if the sanitiser did
    /// nothing.
    #[test]
    fn a_message_past_the_cap_is_truncated_to_it() {
        let long = "x".repeat(MAX_MESSAGE_LEN + 50);
        let message = ProviderMessage::new(&long, Redaction::None).expect("non-empty input");
        assert_eq!(
            text_of(message).len(),
            MAX_MESSAGE_LEN,
            "must be capped to the constant, not merely bounded by it"
        );
    }

    /// Task 3 review round 1, I2: control characters, including the
    /// newline this crate's own rule cares about most, must not survive.
    #[test]
    fn control_characters_and_newlines_are_stripped() {
        let raw = "line one\nline two\r\ttabbed\u{7}bell";
        let message = ProviderMessage::new(raw, Redaction::None).expect("non-empty input");
        let text = text_of(message);
        assert!(
            !text.chars().any(|c| c.is_control()),
            "no ASCII/Latin-1 control character may survive: {text:?}"
        );
    }

    /// Task 3 review round 1, Minor: `char::is_control` alone does not name
    /// U+2028/U+2029 (a renderer treats both as a newline even though
    /// Unicode does not call them controls) or the bidi formatting block
    /// (U+202A-U+202E, which includes U+202E RIGHT-TO-LEFT OVERRIDE, and
    /// U+2066-U+2069) — three more ways to break a rendered line that
    /// `is_control` alone would let through.
    #[test]
    fn line_and_paragraph_separators_and_bidi_overrides_are_stripped_too() {
        let raw = "before\u{2028}mid\u{2029}mid2\u{202E}after";
        let message = ProviderMessage::new(raw, Redaction::None).expect("non-empty input");
        let text = text_of(message);
        for forbidden in ['\u{2028}', '\u{2029}', '\u{202E}'] {
            assert!(
                !text.contains(forbidden),
                "{forbidden:?} must not survive: {text:?}"
            );
        }
    }

    /// Task 3 review round 2: the invisible/zero-width block, added
    /// alongside the strip-then-redact reorder that closed C1 — an
    /// invisible character mid-key is exactly what let a control character
    /// defeat redaction in round 1, and these are the same class of hazard.
    /// Round 3 moved the mechanism from this hand-picked set to the `Cf`
    /// Unicode general category (see `unsafe_for_display`); this test stays
    /// as a pin on the specific characters named so far.
    #[test]
    fn zero_width_and_invisible_characters_are_stripped_too() {
        let raw = "before\u{200B}mid\u{FEFF}mid2\u{00AD}after";
        let message = ProviderMessage::new(raw, Redaction::None).expect("non-empty input");
        let text = text_of(message);
        for forbidden in ['\u{200B}', '\u{FEFF}', '\u{00AD}'] {
            assert!(
                !text.contains(forbidden),
                "{forbidden:?} must not survive: {text:?}"
            );
        }
    }

    /// Task 3 review round 3, J5: the reviewer's measured attack — U+2060
    /// WORD JOINER was not in round 2's handwritten list, and this pins that
    /// the category-based fix (`GeneralCategory::Format`) catches it too,
    /// without the list needing to name it specifically.
    #[test]
    fn word_joiner_is_stripped_by_category_not_by_name() {
        let raw = "before\u{2060}after";
        let message = ProviderMessage::new(raw, Redaction::None).expect("non-empty input");
        let text = text_of(message);
        assert!(
            !text.contains('\u{2060}'),
            "U+2060 must not survive: {text:?}"
        );
    }

    /// Task 3 review round 1, I2: a message that is nothing *but* unsafe
    /// characters must vanish, not survive as a label with nothing in it —
    /// the same "None is not the same as an empty label" rule `Refusal`'s
    /// own text fields already keep.
    #[test]
    fn a_message_that_is_only_control_characters_becomes_nothing() {
        assert_eq!(
            ProviderMessage::new("\n\r\t\u{7}", Redaction::None),
            None,
            "an empty label is worse than none at all"
        );
    }

    /// Task 3 review round 1, I2, corrected in round 2's one-liner: `len()
    /// <= MAX_MESSAGE_LEN` alone is satisfied by an empty string, and what
    /// actually catches a split character is the panic inside
    /// `String::truncate`, not that assertion. The exact length is known —
    /// assert it, and that the multi-byte character itself is absent, so
    /// the test constrains both sides instead of only an upper bound.
    #[test]
    fn truncation_lands_on_a_char_boundary() {
        let mut raw = "x".repeat(MAX_MESSAGE_LEN - 1);
        raw.push('€'); // 3-byte character straddling the cap
        raw.push_str("tail text past the cap");
        let message = ProviderMessage::new(&raw, Redaction::None).expect("non-empty input");
        let text = text_of(message);
        assert_eq!(
            text.len(),
            MAX_MESSAGE_LEN - 1,
            "must back off to just before the multi-byte character, not merely stay under the cap"
        );
        assert!(
            !text.contains('€'),
            "the multi-byte character itself must not survive: {text:?}"
        );
    }

    /// Task 3 review round 2, C1: the property `redact_key` cannot close by
    /// itself — a run of the key's own characters, long enough to identify
    /// it, must withhold the whole message rather than let the fragment
    /// ride along next to a placeholder that only covers the *whole* key.
    #[test]
    fn a_surviving_key_fragment_withholds_the_message_entirely() {
        let key = "test-key-not-a-real-one";
        let fragment = &key[4..16]; // twelve characters, taken from the key
        let raw = format!("invalid credential: {fragment}");
        let message = ProviderMessage::new(&raw, Redaction::Key(key)).expect("non-empty input");
        assert_eq!(
            message,
            ProviderMessage::Withheld,
            "a surviving key fragment must withhold the whole message"
        );
    }

    /// The other half of the fragment property: shorter than `FRAGMENT_LEN`
    /// must NOT withhold — otherwise an ordinary shared prefix could trip
    /// it on every key from this provider.
    #[test]
    fn a_fragment_shorter_than_the_window_does_not_withhold() {
        let key = "test-key-not-a-real-one";
        let short_prefix = &key[..8]; // shorter than FRAGMENT_LEN
        let raw = format!("keys from this provider start with {short_prefix}");
        let message = ProviderMessage::new(&raw, Redaction::Key(key)).expect("non-empty input");
        assert!(
            matches!(message, ProviderMessage::Text { .. }),
            "a fragment shorter than the window must not withhold: {message:?}"
        );
    }

    /// Isolates the strip-then-redact order fix from the fragment safety net
    /// added alongside it (Task 3 review round 2, C1). With a key at least
    /// `FRAGMENT_LEN` long, the fragment check masks an order regression on
    /// its own: reassembling the whole key via a wrong order also reassembles
    /// a run long enough for `contains_key_fragment` to catch and withhold —
    /// which was measured while gathering the round's red evidence, and
    /// meant a plain revert of the order did not turn the delivery tests red.
    /// A key *shorter* than `FRAGMENT_LEN` gives that net nothing to catch
    /// (it only ever looks for a run `FRAGMENT_LEN` long), so this proves the
    /// order matters on its own, not only as a case the fragment check would
    /// have caught anyway.
    #[test]
    fn strip_then_redact_matters_even_when_the_fragment_net_cannot_help() {
        let key = "shortkey"; // shorter than FRAGMENT_LEN
        let raw = "invalid key: short\u{1F}key";
        let message = ProviderMessage::new(raw, Redaction::Key(key)).expect("non-empty input");
        let text = text_of(message);
        assert!(
            !text.to_ascii_lowercase().contains(key),
            "an inserted control character must not let the key reassemble after redaction: \
             {text:?}"
        );
    }

    /// Task 3 review round 3, J3: pins the actual wire shape, not merely
    /// that serialisation succeeds — struct variants under
    /// `#[serde(tag = "kind")]` were required precisely because the
    /// original newtype shapes (`Known(f64)`, `Text(String)`) compile and
    /// then fail *at serialisation time*, which no type-level check catches
    /// and no existing test exercised. The window is written against this
    /// exact shape.
    #[test]
    fn balance_and_message_states_serialise_to_the_shape_the_window_expects() {
        assert_eq!(
            serde_json::to_string(&Balance::Known { amount: 6.5 }).unwrap(),
            r#"{"kind":"known","amount":6.5}"#
        );
        assert_eq!(
            serde_json::to_string(&Balance::NotStated).unwrap(),
            r#"{"kind":"notStated"}"#
        );
        assert_eq!(
            serde_json::to_string(&Balance::EnvelopeNotUnderstood).unwrap(),
            r#"{"kind":"envelopeNotUnderstood"}"#
        );
        let unreadable = Balance::Unreadable {
            raw: ProviderMessage::new("odd shape", Redaction::None).expect("non-empty input"),
        };
        assert_eq!(
            serde_json::to_string(&unreadable).unwrap(),
            r#"{"kind":"unreadable","raw":{"kind":"text","text":"odd shape"}}"#
        );
        assert_eq!(
            serde_json::to_string(&ProviderMessage::Withheld).unwrap(),
            r#"{"kind":"withheld"}"#
        );
    }

    /// Task 3 review round 4, K5: `Balance`'s four states are pinned above,
    /// but `KeyCheck` — the struct that actually carries one to the window
    /// — was not, and the window receives `KeyCheck`, not a bare `Balance`.
    #[test]
    fn key_check_serialises_with_the_balance_nested_under_its_own_field() {
        let check = KeyCheck {
            balance: Balance::Known { amount: 6.5 },
        };
        assert_eq!(
            serde_json::to_string(&check).unwrap(),
            r#"{"balance":{"kind":"known","amount":6.5}}"#
        );
    }
}

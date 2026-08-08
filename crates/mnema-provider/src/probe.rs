//! The two calls that stand between a typed key and a long indexing run.
//!
//! §4.5 of the requirements asks for a cheap call at entry so a typo does not
//! surface three hours into indexing. This is that call, split in two: the key
//! is checked without a model (the credits endpoint needs none), and the
//! embedding model is checked separately, when one is chosen.

use serde::Deserialize;
use serde_json::Value;

use crate::{Error, KeySent, error_for_status, http};

#[derive(Debug, Clone, PartialEq)]
pub struct KeyCheck {
    /// Prepaid credit left, when the provider states it in a shape this code
    /// knows. `None` covers several different facts the provider (or the
    /// wire) can produce — "not stated", "stated in a shape this build
    /// cannot read", and, since Task 3 review round 1 item I3, "the whole
    /// envelope around the balance was not the shape this code expects,
    /// on an otherwise successful 200" — collapsed on purpose, the same way
    /// `ModelEntry::context_length` collapses `Limit::NotStated` and
    /// `Limit::Unreadable` in `catalogue.rs`: none of them is a number this
    /// screen can show, and showing a wrong zero for any of them would be
    /// worse than showing nothing. What must NOT happen, in every one of
    /// those cases: a garbled balance must not fail the *whole* key check —
    /// see `Stated`, below, and `check_key`'s own comments for why.
    pub credits_remaining: Option<f64>,
}

/// The provider's own explanation for a refusal, sanitised at construction so
/// it is safe to interpolate into an `Error` variant's own message (Task 3
/// review, item 4). The shortcut this deliberately avoids is a variant that
/// holds the raw string and renders it with `#[error("{0}")]` — forbidden by
/// the same rule `Refusal::LimitNotUnderstood` and `RecordId::NotAString`
/// already keep, one module over: provider bytes are never interpolated into
/// a plain format string. This type keeps that rule instead of breaking it —
/// construction (`ProviderMessage::new`) does three things, in order, and
/// `Display` below is safe to call unconditionally only because all three
/// already happened:
///
/// 1. **Redacts the key.** `check_key` sends a credential, and a provider
///    that rejects a malformed one commonly echoes it back inside its own
///    error message (Task 3 review round 1, C1 — a measured, not a
///    hypothetical, way for the key to leave this crate despite
///    `lib.rs`'s "no variant may carry the key" rule and the test that
///    holds it to that). Redaction runs first, before anything below, so a
///    match cannot be split by control-character stripping or by the
///    length cap.
/// 2. **Strips everything unsafe to render** — see `unsafe_for_display`.
/// 3. **Caps length** — see `MAX_MESSAGE_LEN`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderMessage(String);

/// Generous for a one-line refusal reason; `catalogue`'s own cap (64 bytes,
/// `MAX_RAW_LEN`) is sized for a broken number, not for a sentence like "This
/// key was disabled on 2026-08-01".
const MAX_MESSAGE_LEN: usize = 200;

/// Stands in for every redacted occurrence of the key (Task 3 review round 1,
/// C1). Fixed and content-free on purpose: a placeholder that echoed even the
/// *length* of the key back would still be more of the key than this crate
/// promises never to carry.
const REDACTED_PLACEHOLDER: &str = "[redacted]";

impl ProviderMessage {
    /// `key` is the credential `check_key` sent — see the type's own doc
    /// comment for why it must be redacted before anything else here runs.
    fn new(raw: &str, key: &str) -> Option<Self> {
        let redacted = redact_key(raw, key);
        let cleaned: String = redacted
            .chars()
            .filter(|c| !unsafe_for_display(*c))
            .collect();
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(Self(cap(trimmed.to_string(), MAX_MESSAGE_LEN)))
    }
}

impl std::fmt::Display for ProviderMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Replaces every case-insensitive occurrence of `key` inside `text` with a
/// fixed placeholder (Task 3 review round 1, C1). A substring search, not a
/// check for exact equality against the whole message: the provider is under
/// no obligation to echo a rejected credential back verbatim and alone — a
/// leading "Bearer " scheme word still attached, or a different case than
/// this build sent, are both real shapes a provider can choose to send, and
/// both still contain the key as a substring, which is what this matches
/// against. Case-insensitive via `to_ascii_lowercase`, not full Unicode
/// case-folding: it preserves byte length, which keeps every position found
/// in the lowercased copy valid in the original — full case-folding can
/// change the byte length of some characters and would break that
/// correspondence. Real keys are ASCII, so this loses nothing in practice.
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

/// True for anything unsafe to place in a rendered message or a log line.
/// `char::is_control` (the ASCII/Latin-1 control block, `\n` and `\r` among
/// them) is most of this, but not all of it (Task 3 review round 1, Minor):
/// U+2028 LINE SEPARATOR and U+2029 PARAGRAPH SEPARATOR act as a newline to
/// a renderer even though Unicode's own General_Category does not call them
/// controls, and the bidi formatting block — U+202A-U+202E (which includes
/// U+202E RIGHT-TO-LEFT OVERRIDE) and U+2066-U+2069 — can make text render in
/// an order that does not match its byte order, which is its own way to
/// forge what a log line appears to say.
fn unsafe_for_display(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{2028}' | '\u{2029}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
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

/// A single field's own answer, before `check_key` combines it with its
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
        // `f.is_finite()` (Task 3 review round 1, Minor): `f64::from_str`
        // accepts `"NaN"`, `"inf"`/`"infinity"` (any case) and their signed
        // forms, and a JSON number as ordinary-looking as `1e999` overflows
        // `as_f64` to `f64::INFINITY` rather than failing to parse. Without
        // this guard any of those reads as `Stated::Number`, and a balance of
        // NaN would reach the screen as a number that was successfully read
        // — worse than showing nothing, and it would also make `KeyCheck`'s
        // derived `PartialEq` stop being reflexive, since `NaN != NaN`.
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
/// provider sent that shape, and redacts `key` from whatever it finds
/// (`ProviderMessage::new`) before handing it back. `None` for anything
/// else: a body that does not fit is not itself a problem worth surfacing
/// here — the status already answered the one question this call promises
/// to answer.
fn extract_provider_message(body: &str, key: &str) -> Option<ProviderMessage> {
    let envelope: ProviderErrorEnvelope = serde_json::from_str(body).ok()?;
    ProviderMessage::new(&envelope.error.message?, key)
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
fn attach_reason(err: Error, body: &str, key: &str) -> Error {
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
        credits_remaining: credits_remaining_from(&body)?,
    })
}

/// Reads `data.total_credits` and `data.total_usage` out of a 200 body, as
/// leniently as `Stated` allows a single field to be read — and, one level up
/// from `Stated`, just as leniently about `data` itself (Task 3 review round
/// 1, I3): a `data` key that is missing, `null`, or not an object at all —
/// the provider renamed it, say — used to fail `check_key` outright with
/// `Error::Malformed`, which told the caller the *key* was broken over a
/// shape problem in the *balance*. A 200 already means the key works; that a
/// balance could not be found inside it is the same "not stated" fact
/// `Stated::Absent` already names for one field, one level up, and belongs in
/// `credits_remaining` as `None`, not in a hard failure of the whole call.
///
/// The one shape that still fails outright is a body that is not JSON at
/// all — an HTML captive-portal or gateway page, say, the same case
/// `models_from_json`'s `Category::Syntax` branch names for `list_models`.
/// That is a different, stronger fact than "the JSON parsed but did not name
/// a balance": it says whatever answered was not the provider's endpoint at
/// all, which "a 200 means the key works" cannot be trusted to mean either.
fn credits_remaining_from(body: &str) -> Result<Option<f64>, Error> {
    let value: Value = serde_json::from_str(body).map_err(|_| {
        Error::Malformed(
            "the credits answer is not JSON at all — likely a proxy or gateway page, not the \
             provider itself",
        )
    })?;
    let credits_remaining = value
        .get("data")
        .cloned()
        .and_then(|d| serde_json::from_value::<Credits>(d).ok())
        .and_then(
            |credits| match (credits.total_credits, credits.total_usage) {
                (Stated::Number(total), Stated::Number(used)) => Some(total - used),
                _ => None,
            },
        );
    Ok(credits_remaining)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Task 3 review round 1, I2: `ProviderMessage::new`'s length cap, pinned
    /// on its own — the integration tests in `tests/probe.rs` only prove a
    /// message is *delivered*, and pass just as well if this body were
    /// replaced with `Some(Self(raw.to_string()))`.
    #[test]
    fn a_message_past_the_cap_is_truncated_to_it() {
        let long = "x".repeat(MAX_MESSAGE_LEN + 50);
        let message = ProviderMessage::new(&long, "").expect("non-empty input");
        assert_eq!(
            message.0.len(),
            MAX_MESSAGE_LEN,
            "must be capped to the constant, not merely bounded by it"
        );
    }

    /// Task 3 review round 1, I2: control characters, including the
    /// newline this crate's own rule cares about most, must not survive.
    #[test]
    fn control_characters_and_newlines_are_stripped() {
        let raw = "line one\nline two\r\ttabbed\u{7}bell";
        let message = ProviderMessage::new(raw, "").expect("non-empty input");
        assert!(
            !message.0.chars().any(|c| c.is_control()),
            "no ASCII/Latin-1 control character may survive: {:?}",
            message.0
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
        let message = ProviderMessage::new(raw, "").expect("non-empty input");
        for forbidden in ['\u{2028}', '\u{2029}', '\u{202E}'] {
            assert!(
                !message.0.contains(forbidden),
                "{forbidden:?} must not survive: {:?}",
                message.0
            );
        }
    }

    /// Task 3 review round 1, I2: a message that is nothing *but* unsafe
    /// characters must vanish, not survive as a label with nothing in it —
    /// the same "None is not the same as an empty label" rule `Refusal`'s
    /// own text fields already keep.
    #[test]
    fn a_message_that_is_only_control_characters_becomes_nothing() {
        assert_eq!(
            ProviderMessage::new("\n\r\t\u{7}", ""),
            None,
            "an empty label is worse than none at all"
        );
    }

    /// Task 3 review round 1, I2: a multi-byte character sitting exactly on
    /// the cap must not be split — `cap`'s `is_char_boundary` walk-back is
    /// what this exercises; a naive byte-count slice would panic here (or,
    /// outside a debug build, produce invalid UTF-8).
    #[test]
    fn truncation_lands_on_a_char_boundary() {
        let mut raw = "x".repeat(MAX_MESSAGE_LEN - 1);
        raw.push('€'); // 3-byte character straddling the cap
        raw.push_str("tail text past the cap");
        let message = ProviderMessage::new(&raw, "").expect("non-empty input");
        assert!(
            message.0.len() <= MAX_MESSAGE_LEN,
            "must not exceed the cap: {} bytes",
            message.0.len()
        );
    }
}

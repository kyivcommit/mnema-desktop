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
    /// knows. `None` covers two different facts the provider can produce —
    /// "not stated" and "stated in a shape this build cannot read" (Task 3
    /// review, item 1) — collapsed on purpose, the same way
    /// `ModelEntry::context_length` collapses `Limit::NotStated` and
    /// `Limit::Unreadable` in `catalogue.rs`: neither is a number this screen
    /// can show, and showing a wrong zero for either would be worse than
    /// showing nothing. What must NOT happen either way, and is the actual
    /// defect this type guards against: a garbled `total_credits` must not
    /// fail the *whole* key check — see `Stated`, below, for why a plain
    /// `Option<f64>` field would do exactly that.
    pub credits_remaining: Option<f64>,
}

/// The provider's own explanation for a refusal, sanitised at construction so
/// it is safe to interpolate into `Error::Unauthorised`'s own message (Task 3
/// review, item 4). The shortcut this deliberately avoids is a variant that
/// holds the raw string and renders it with `#[error("{0}")]` — forbidden by
/// the same rule `Refusal::LimitNotUnderstood` and `RecordId::NotAString`
/// already keep, one module over: provider bytes are never interpolated into
/// a plain format string. This type keeps that rule instead of breaking it:
/// construction strips every control character, including newlines, so the
/// text cannot cut a log line in half or forge a second line that looks like
/// a new entry, and caps length so a hostile or merely verbose provider
/// cannot turn this into an unbounded label on screen. That is what makes
/// `Display` below safe to call unconditionally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderMessage(String);

/// Generous for a one-line refusal reason; `catalogue`'s own cap (64 bytes,
/// `MAX_RAW_LEN`) is sized for a broken number, not for a sentence like "This
/// key was disabled on 2026-08-01".
const MAX_MESSAGE_LEN: usize = 200;

impl ProviderMessage {
    fn new(raw: &str) -> Option<Self> {
        let cleaned: String = raw.chars().filter(|c| !c.is_control()).collect();
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
struct CreditsBody {
    data: Credits,
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
        Ok(match Value::deserialize(deserializer)? {
            Value::Null => Stated::Absent,
            Value::Number(n) => n.as_f64().map(Stated::Number).unwrap_or(Stated::Unreadable),
            Value::String(s) => s
                .parse::<f64>()
                .map(Stated::Number)
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
/// provider sent that shape. `None` for anything else: a body that does not
/// fit is not itself a problem worth surfacing here — the status already
/// answered the one question this call promises to answer.
fn extract_provider_message(body: &str) -> Option<ProviderMessage> {
    let envelope: ProviderErrorEnvelope = serde_json::from_str(body).ok()?;
    ProviderMessage::new(&envelope.error.message?)
}

/// `error_for_status` cannot carry the response body — on purpose, since it
/// also serves `list_models`, which never reads a failure body for a message
/// (Task 2 review round 3, H4). Task 3's key check does read one, so the
/// provider's own explanation, when the body states one, is folded in here,
/// one layer up, instead of changing what `error_for_status` hands back to
/// its other caller too (Task 3 review, item 4).
fn attach_reason(err: Error, body: &str) -> Error {
    match err {
        Error::Unauthorised { reason: None } => Error::Unauthorised {
            reason: extract_provider_message(body),
        },
        other => other,
    }
}

/// Checks a key without needing a model to be chosen yet.
pub fn check_key(base: &str, key: &str) -> Result<KeyCheck, Error> {
    let (status, body) = match http::get(base, "/credits", Some(key)) {
        Ok(pair) => pair,
        // A body-read failure still carries its status (`Error::BodyUnreadable`,
        // Task 2 review round 2, G3) precisely so it is not lost — but this
        // screen exists to answer one question, does the key work, and a 401
        // whose body was merely cut off must give the same verdict a clean 401
        // would, not "reading the response body failed" (Task 3 review, item
        // 2). A 200 whose body could not be read has no substitute below
        // (`error_for_status` is never called for 200, since a 200 means
        // something different to every caller) and is returned as-is: this
        // build genuinely does not know whether the key worked.
        Err(Error::BodyUnreadable { status, .. }) if status != 200 => {
            return Err(error_for_status(status, KeySent::Yes));
        }
        Err(other) => return Err(other),
    };
    match status {
        200 => {}
        other => return Err(attach_reason(error_for_status(other, KeySent::Yes), &body)),
    }
    let parsed: CreditsBody = serde_json::from_str(&body)
        .map_err(|_| Error::Malformed("the credits answer is not the object this code expects"))?;
    let credits_remaining = match (parsed.data.total_credits, parsed.data.total_usage) {
        (Stated::Number(total), Stated::Number(used)) => Some(total - used),
        _ => None,
    };
    Ok(KeyCheck { credits_remaining })
}

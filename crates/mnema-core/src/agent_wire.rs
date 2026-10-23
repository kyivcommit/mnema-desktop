//! The agent API's wire format: one JSON line per request, one per reply, over
//! the application's Unix socket.
//!
//! Both ends need these types — the application answers on the socket and the
//! `mnema` command-line binary asks on it — and neither may depend on the
//! other, so they live here, in the crate that depends on nothing of ours (the
//! argument `wire` makes for the worker protocol).
//!
//! Every reply carries `"v"` and exactly one of `"ok"` / `"err"`. `err.kind` is
//! a closed set a client can branch on; `err.message` is English prose for a
//! person and is never parsed.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The protocol version a request must state and a reply states back.
pub const VERSION: u32 = 1;
/// The longest request line the application will read, `\n` not counted.
pub const MAX_LINE: usize = 65536;

const DEFAULT_K: u32 = 8;
const MAX_K: u32 = 20;
const DEFAULT_CONTEXT: u32 = 2;
const MAX_CONTEXT: u32 = 10;

/// What is being asked. `k` and `context` are `Option` on the wire; after
/// [`parse_request`] both are `Some`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Search { query: String, k: Option<u32> },
    Passage { id: String, context: Option<u32> },
    Status,
    Rescan { wait: Option<bool> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub v: u32,
    /// Who is asking, for the activity log only; never trusted for anything.
    pub client: Option<String>,
    #[serde(flatten)]
    pub op: Op,
}

/// Closed on purpose: a client matches on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrKind {
    Disabled,
    NotRunning,
    Version,
    BadRequest,
    StaleId,
    NotFound,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireError {
    pub kind: ErrKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    Ok(Value),
    Err(WireError),
}

/// `{"v":1,"ok":…}` or `{"v":1,"err":…}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    #[serde(flatten)]
    pub reply: Reply,
}

/// A request that passed every check, with defaults filled in.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub client: Option<String>,
    pub op: Op,
}

fn bad(message: impl Into<String>) -> WireError {
    WireError {
        kind: ErrKind::BadRequest,
        message: message.into(),
    }
}

/// Order matters: length, JSON, version, op, bounds. The version is read from
/// the bare value before the op is, so a future client's `v: 2` with an op this
/// build has never heard of is told "version", not "bad request".
pub fn parse_request(line: &str) -> Result<Checked, WireError> {
    if line.len() > MAX_LINE {
        return Err(bad(format!("request longer than {MAX_LINE} bytes")));
    }
    let value: Value = serde_json::from_str(line).map_err(|e| bad(format!("not JSON: {e}")))?;
    if value.get("v").and_then(Value::as_u64) != Some(u64::from(VERSION)) {
        return Err(WireError {
            kind: ErrKind::Version,
            message: format!("this build speaks protocol version {VERSION}"),
        });
    }
    let req: Request =
        serde_json::from_value(value).map_err(|e| bad(format!("bad request: {e}")))?;
    let op = match req.op {
        Op::Search { query, k } => {
            if query.trim().is_empty() {
                return Err(bad("query is blank"));
            }
            let k = k.unwrap_or(DEFAULT_K);
            if !(1..=MAX_K).contains(&k) {
                return Err(bad(format!("k must be between 1 and {MAX_K}")));
            }
            Op::Search { query, k: Some(k) }
        }
        Op::Passage { id, context } => {
            let context = context.unwrap_or(DEFAULT_CONTEXT);
            if context > MAX_CONTEXT {
                return Err(bad(format!("context must be between 0 and {MAX_CONTEXT}")));
            }
            Op::Passage {
                id,
                context: Some(context),
            }
        }
        other => other,
    };
    Ok(Checked {
        client: req.client,
        op,
    })
}

/// A passage's handle: `"{chunk_id}:{document_id}:{ord}"`. `document_id` is the
/// content's sha256 in hex (`mnema-ingest`, `document.id` **is** the sha256), so
/// it never holds the separator; an id that does is refused rather than split
/// differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassageId {
    pub chunk_id: i64,
    pub document_id: String,
    pub ord: i64,
}

impl PassageId {
    pub fn format(&self) -> String {
        format!("{}:{}:{}", self.chunk_id, self.document_id, self.ord)
    }

    pub fn parse(s: &str) -> Option<PassageId> {
        let mut parts = s.split(':');
        let (chunk_id, document_id, ord) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || document_id.is_empty() {
            return None;
        }
        Some(PassageId {
            chunk_id: chunk_id.parse().ok()?,
            document_id: document_id.to_string(),
            ord: ord.parse().ok()?,
        })
    }
}

/// One reply, newline-terminated. JSON escapes every `\n` inside a string, so
/// the terminator is the only raw one.
pub fn reply_line(env: &Envelope) -> String {
    let mut s = serde_json::to_string(env).expect("an Envelope always serialises");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn search_k(line: &str) -> Result<Option<u32>, ErrKind> {
        match parse_request(line) {
            Ok(Checked {
                op: Op::Search { k, .. },
                ..
            }) => Ok(k),
            Ok(other) => panic!("not a search: {other:?}"),
            Err(e) => Err(e.kind),
        }
    }

    fn passage_context(line: &str) -> Result<Option<u32>, ErrKind> {
        match parse_request(line) {
            Ok(Checked {
                op: Op::Passage { context, .. },
                ..
            }) => Ok(context),
            Ok(other) => panic!("not a passage: {other:?}"),
            Err(e) => Err(e.kind),
        }
    }

    fn kind(line: &str) -> ErrKind {
        parse_request(line).unwrap_err().kind
    }

    #[test]
    fn a_search_without_k_gets_eight() {
        let c = parse_request(r#"{"v":1,"op":"search","query":"fox","client":"t"}"#).unwrap();
        assert_eq!(
            c.op,
            Op::Search {
                query: "fox".into(),
                k: Some(8)
            }
        );
        assert_eq!(c.client.as_deref(), Some("t"));
    }

    #[test]
    fn k_outside_one_to_twenty_is_a_bad_request() {
        let line = |k: u32| format!(r#"{{"v":1,"op":"search","query":"x","k":{k}}}"#);
        assert_eq!(search_k(&line(0)), Err(ErrKind::BadRequest));
        assert_eq!(search_k(&line(21)), Err(ErrKind::BadRequest));
        assert_eq!(search_k(&line(1)), Ok(Some(1)));
        assert_eq!(search_k(&line(20)), Ok(Some(20)));
    }

    #[test]
    fn context_outside_zero_to_ten_is_a_bad_request() {
        let line = |c: u32| format!(r#"{{"v":1,"op":"passage","id":"1:a:0","context":{c}}}"#);
        assert_eq!(passage_context(&line(11)), Err(ErrKind::BadRequest));
        assert_eq!(passage_context(&line(0)), Ok(Some(0)));
        assert_eq!(passage_context(&line(10)), Ok(Some(10)));
        assert_eq!(
            passage_context(r#"{"v":1,"op":"passage","id":"1:a:0"}"#),
            Ok(Some(2))
        );
    }

    #[test]
    fn a_blank_query_is_a_bad_request() {
        assert_eq!(
            kind(r#"{"v":1,"op":"search","query":"  \t"}"#),
            ErrKind::BadRequest
        );
    }

    #[test]
    fn an_unknown_version_is_refused_as_version() {
        assert_eq!(
            kind(r#"{"v":2,"op":"search","query":"x"}"#),
            ErrKind::Version
        );
        // The version is judged before the op: a newer client's new op is a
        // version problem, not a malformed request.
        assert_eq!(kind(r#"{"v":2,"op":"delete"}"#), ErrKind::Version);
    }

    #[test]
    fn an_unknown_op_or_non_json_is_a_bad_request() {
        assert_eq!(kind(r#"{"v":1,"op":"delete"}"#), ErrKind::BadRequest);
        assert_eq!(kind("not json"), ErrKind::BadRequest);
    }

    #[test]
    fn a_line_longer_than_max_line_is_a_bad_request() {
        // A valid request padded to the limit is read on; one byte more is
        // refused for its length although it would otherwise parse.
        let head = r#"{"v":1,"op":"status","client":""#;
        let padded = |len: usize| format!("{head}{}\"}}", "x".repeat(len - head.len() - 2));
        let at_limit = padded(MAX_LINE);
        assert_eq!(at_limit.len(), MAX_LINE);
        assert_eq!(parse_request(&at_limit).unwrap().op, Op::Status);
        assert_eq!(kind(&padded(MAX_LINE + 1)), ErrKind::BadRequest);
    }

    #[test]
    fn passage_id_round_trips_and_rejects_garbage() {
        let id = PassageId {
            chunk_id: 42,
            document_id: "ab12cd".into(),
            ord: 7,
        };
        assert_eq!(id.format(), "42:ab12cd:7");
        assert_eq!(PassageId::parse(&id.format()), Some(id));
        for bad in ["1:abc", "x:abc:1", "1:abc:2:3", "1::2", "1:abc:y", ""] {
            assert_eq!(PassageId::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_multiline_cyrillic_text_stays_one_line() {
        let env = Envelope {
            v: VERSION,
            reply: Reply::Ok(json!({"text": "рядок\nдругий"})),
        };
        let line = reply_line(&env);
        assert_eq!(line.matches('\n').count(), 1);
        assert!(line.ends_with('\n'));
        let back: Envelope = serde_json::from_str(&line).unwrap();
        match back.reply {
            Reply::Ok(v) => assert_eq!(v["text"], "рядок\nдругий"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_wire_shape_is_pinned() {
        let ok = Envelope {
            v: VERSION,
            reply: Reply::Ok(json!({"n": 1})),
        };
        assert_eq!(reply_line(&ok), "{\"v\":1,\"ok\":{\"n\":1}}\n");
        let err = Envelope {
            v: VERSION,
            reply: Reply::Err(WireError {
                kind: ErrKind::StaleId,
                message: "m".into(),
            }),
        };
        assert_eq!(
            reply_line(&err),
            "{\"v\":1,\"err\":{\"kind\":\"stale_id\",\"message\":\"m\"}}\n"
        );
    }
}

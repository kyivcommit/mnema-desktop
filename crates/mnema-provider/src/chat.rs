//! The chat-completions call: the one place the product asks a model to write
//! prose. `probe` answers "does this key work" and "will this model fill an
//! index with numbers that mean something"; this answers "given these
//! messages, what does the model say". A non-streaming POST to
//! `/chat/completions`, ported from the server's `app/llm/litellm_provider.py`
//! — model and messages only, no sampling parameters (§7.4, spec §PR 2).

use serde::{Deserialize, Serialize};

use crate::probe::{attach_reason, extract_provider_message};
use crate::{Error, KeySent, error_for_status, http};

/// One chat message, in the shape OpenRouter's `/chat/completions` accepts.
///
/// `Serialize` only, like `ModelEntry`: it is sent, never parsed back — the
/// answer is read into `chat`'s own private response structs. `build_messages`
/// in `mnema-rag` (PR 3) constructs these; the fields are `pub` so it can.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    /// The `search` call an assistant message made; `None` is not sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// On a `Tool` message: the call it answers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// The one tool this product offers: `search`, with the query the model chose.
/// On the wire it is OpenAI's `{"id","type":"function","function":{…}}` with
/// `arguments` as a JSON *string*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub query: String,
}

impl Serialize for ToolCall {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let arguments = serde_json::json!({ "query": self.query }).to_string();
        serde_json::json!({
            "id": self.id,
            "type": "function",
            "function": { "name": "search", "arguments": arguments },
        })
        .serialize(serializer)
    }
}

/// What one chat turn produced: prose, or a request to search the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Text(String),
    Search(ToolCall),
}

/// Who a message is from. An enum, not a `String`, for the reason the crate
/// keeps `KeySent`/`Role`/`Redaction` as types: a role is a closed set, and a
/// `String` field where only a few values are valid is exactly the "a string
/// that happens to be right" this crate refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// The provider's answer, read only as far as the one field this call needs.
/// Separate private structs, `Deserialize` only — the mirror of `Message`
/// being `Serialize` only.
#[derive(Deserialize)]
struct Completion {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: CompletionMessage,
}

#[derive(Deserialize)]
struct CompletionMessage {
    content: Option<String>,
    tool_calls: Option<Vec<WireCall>>,
}

#[derive(Deserialize)]
struct WireCall {
    id: String,
    function: WireFunction,
}

#[derive(Deserialize)]
struct WireFunction {
    name: String,
    arguments: String,
}

/// The measured tool schema: one `search` function taking one query.
const TOOLS: &str = r#"[{"type":"function","function":{"name":"search","description":"Search the user's document library. Returns up to 8 numbered passages with their file names.","parameters":{"type":"object","properties":{"query":{"type":"string","description":"A self-contained search query"}},"required":["query"]}}}]"#;

/// Sends `messages` to the chat model and returns its answer verbatim.
///
/// Ported from `app/llm/litellm_provider.py:96,139-150`: model and messages
/// only, no streaming and no sampling parameters (§7.4). The returned text is
/// untouched — anchors, whitespace and all — because resolving `<c>N</c>`
/// belongs to `mnema-rag` and the empty-answer refusal belongs to the bridge
/// (spec §4); this call's one job is the round trip.
pub fn complete(base: &str, key: &str, model: &str, messages: &[Message]) -> Result<String, Error> {
    complete_within(base, key, model, messages, http::INTERACTIVE_TIMEOUT)
}

/// [`complete`] with the wait chosen by the caller: a local model on a weak Mac
/// takes longer than the cloud's `INTERACTIVE_TIMEOUT` to write an answer.
pub fn complete_within(
    base: &str,
    key: &str,
    model: &str,
    messages: &[Message],
    timeout: std::time::Duration,
) -> Result<String, Error> {
    round_trip(base, key, model, messages, false, timeout)?
        .content
        .ok_or(Error::Malformed(
            "the chat answer is JSON, but not the shape this code expects",
        ))
}

/// One chat turn that may end in a `search` request instead of prose. With
/// `offer_search` the body carries the tool and `require_parameters`, so the
/// router skips endpoints that would silently ignore `tools`; never
/// `provider.only`. A tool call that is unusable (not JSON, no query, a blank
/// one, another tool's name) is read as the prose beside it, not an error.
pub fn complete_turn(
    base: &str,
    key: &str,
    model: &str,
    messages: &[Message],
    offer_search: bool,
    timeout: std::time::Duration,
) -> Result<Reply, Error> {
    let message = round_trip(base, key, model, messages, offer_search, timeout)?;
    let search = message
        .tool_calls
        .unwrap_or_default()
        .into_iter()
        .find_map(|call| {
            if call.function.name != "search" {
                return None;
            }
            let args: serde_json::Value = serde_json::from_str(&call.function.arguments).ok()?;
            let query = args.get("query")?.as_str()?.trim();
            (!query.is_empty()).then(|| ToolCall {
                id: call.id,
                query: query.to_string(),
            })
        });
    Ok(match search {
        Some(call) => Reply::Search(call),
        None => Reply::Text(message.content.unwrap_or_default()),
    })
}

fn round_trip(
    base: &str,
    key: &str,
    model: &str,
    messages: &[Message],
    offer_search: bool,
    timeout: std::time::Duration,
) -> Result<CompletionMessage, Error> {
    let mut body = serde_json::json!({ "model": model, "messages": messages });
    if offer_search {
        body["tools"] = serde_json::from_str(TOOLS).expect("TOOLS is valid JSON");
        body["provider"] = serde_json::json!({ "require_parameters": true });
    }
    let request = body.to_string();
    let (status, answer) =
        match http::post_json_within(base, "/chat/completions", key, &request, timeout) {
            Ok(pair) => pair,
            // A body cut off on a refusal still carries the refusal's verdict, not
            // the answer — the same trade `check_key`/`check_embedding_model` make.
            // No 404 here: unlike the embedding check, a 404 from chat has no
            // special "no such model" verdict, so it keeps `BodyUnreadable`.
            Err(Error::BodyUnreadable { status, .. }) if matches!(status, 401 | 403 | 429) => {
                return Err(error_for_status(status, KeySent::Yes));
            }
            Err(other) => return Err(other),
        };
    if status == 404 && failed_tool_filter(&answer) {
        return Err(Error::NoToolEndpoint);
    }
    if status != 200 {
        return Err(attach_reason(
            error_for_status(status, KeySent::Yes),
            &answer,
            key,
        ));
    }
    let completion: Completion =
        serde_json::from_str(&answer).map_err(|e| unreadable_completion(&answer, key, &e))?;
    Ok(completion
        .choices
        .into_iter()
        .next()
        .ok_or(Error::Malformed("the chat answer carried no choices"))?
        .message)
}

/// Whether a 404 body is the router saying no endpoint of the model takes tools.
fn failed_tool_filter(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["metadata"]["failed_routing_step"]
                .as_str()
                .map(|s| s == "Filter by Tool Compatibility")
        })
        .unwrap_or(false)
}

/// A 200 this build could not read as a completion, named for which problem it
/// was — the chat mirror of `probe::unreadable_embeddings_answer`. The first
/// case is the provider's own error envelope, kept whole; the rest are the
/// three `models_from_json` tells apart. A choice with no `content` field falls
/// into `Data` here ("JSON, but not the shape this code expects"), which is the
/// honest thing to say about it.
fn unreadable_completion(body: &str, key: &str, error: &serde_json::Error) -> Error {
    if let Some(reason) = extract_provider_message(body, key) {
        return Error::ErrorInsteadOfCompletion { reason };
    }
    Error::Malformed(match error.classify() {
        serde_json::error::Category::Syntax => {
            "the chat answer is not JSON at all — likely a proxy or gateway page, \
             not the provider itself"
        }
        serde_json::error::Category::Eof => {
            "the chat answer stopped in the middle of the JSON — a truncated response"
        }
        serde_json::error::Category::Data | serde_json::error::Category::Io => {
            "the chat answer is JSON, but not the shape this code expects"
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_within_honours_its_timeout() {
        use mnema_mock_provider::{MockServer, Reply};
        use std::time::{Duration, Instant};
        let body = r#"{"choices":[{"message":{"content":"late"}}]}"#;
        let ask = || {
            vec![Message {
                role: MessageRole::User,
                content: "q".into(),
                tool_calls: None,
                tool_call_id: None,
            }]
        };

        let slow = MockServer::new(vec![Reply::ok_after(2, body)]);
        let t0 = Instant::now();
        let short = complete_within(slow.base(), "k", "m", &ask(), Duration::from_secs(1));
        let waited = t0.elapsed();
        assert!(
            matches!(short, Err(Error::NoReply(_)) | Err(Error::Transport(_))),
            "a 1 s wait on a 2 s answer must time out, got {short:?}"
        );
        assert!(waited < Duration::from_secs(2), "waited {waited:?}");

        let patient = MockServer::new(vec![Reply::ok_after(2, body)]);
        let long = complete_within(patient.base(), "k", "m", &ask(), Duration::from_secs(3));
        assert_eq!(long.ok().as_deref(), Some("late"));
    }

    #[test]
    fn a_message_serialises_to_the_openai_role_content_shape() {
        let message = Message {
            role: MessageRole::System,
            content: "you answer with citations".to_string(),
            tool_calls: None,
            tool_call_id: None,
        };
        let value = serde_json::to_value(&message).expect("serialises");
        assert_eq!(
            value["role"], "system",
            "the role is a lowercase wire string: {value}"
        );
        assert_eq!(value["content"], "you answer with citations");
        assert!(
            value.get("tool_calls").is_none() && value.get("tool_call_id").is_none(),
            "None must not reach the wire: {value}"
        );

        let user = serde_json::to_value(MessageRole::User).expect("serialises");
        assert_eq!(
            user, "user",
            "MessageRole::User must serialise as \"user\", not \"User\""
        );
    }

    use mnema_mock_provider::{MockServer, Reply as Mock};
    use std::time::Duration;

    const TOOL_CALL: &str = r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"search","arguments":"{\"query\":\"ext4 journal\"}"}}]},"finish_reason":"tool_calls"}]}"#;

    fn user(text: &str) -> Message {
        Message {
            role: MessageRole::User,
            content: text.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn turn(server: &MockServer, offer: bool) -> Result<Reply, Error> {
        complete_turn(
            server.base(),
            "k",
            "m",
            &[user("q")],
            offer,
            Duration::from_secs(5),
        )
    }

    #[test]
    fn a_tool_call_answer_reads_as_search_with_its_query() {
        let server = MockServer::new(vec![Mock::ok(TOOL_CALL)]);
        let got = turn(&server, true).expect("a reply");
        assert_eq!(
            got,
            Reply::Search(ToolCall {
                id: "c1".into(),
                query: "ext4 journal".into()
            })
        );
    }

    #[test]
    fn tools_and_require_parameters_go_on_the_wire_only_when_offered() {
        let on = MockServer::new(vec![Mock::ok(TOOL_CALL)]);
        turn(&on, true).expect("a reply");
        let sent = on.request();
        assert!(sent.contains("\"tools\""), "tools missing: {sent}");
        assert!(
            sent.contains("\"require_parameters\":true"),
            "require_parameters missing: {sent}"
        );
        assert!(
            !sent.contains("\"only\""),
            "provider.only is forbidden: {sent}"
        );

        let off = MockServer::new(vec![Mock::ok(
            r#"{"choices":[{"message":{"content":"hi"}}]}"#,
        )]);
        turn(&off, false).expect("a reply");
        let sent = off.request();
        assert!(!sent.contains("\"tools\""), "tools sent unasked: {sent}");
        assert!(!sent.contains("require_parameters"), "sent unasked: {sent}");
        assert!(
            !sent.contains("\"only\""),
            "provider.only is forbidden: {sent}"
        );
    }

    #[test]
    fn a_null_content_without_tool_calls_is_empty_text() {
        let server = MockServer::new(vec![Mock::ok(
            r#"{"choices":[{"message":{"content":null}}]}"#,
        )]);
        assert_eq!(turn(&server, true).unwrap(), Reply::Text(String::new()));
    }

    #[test]
    fn a_broken_tool_call_is_text_not_an_error() {
        for (arguments, content, want) in [
            (r#""not json""#, "hello", "hello"),
            (r#""{}""#, "", ""),
            (r#""{\"query\":\"  \"}""#, "partial", "partial"),
        ] {
            let body = format!(
                r#"{{"choices":[{{"message":{{"content":{content:?},"tool_calls":[{{"id":"c1","type":"function","function":{{"name":"search","arguments":{arguments}}}}}]}}}}]}}"#
            );
            let server = MockServer::new(vec![Mock::ok(&body)]);
            assert_eq!(
                turn(&server, true).unwrap(),
                Reply::Text(want.into()),
                "arguments {arguments}"
            );
        }
    }

    #[test]
    fn a_404_from_the_tool_filter_is_no_tool_endpoint() {
        let filtered = r#"{"error":{"message":"No endpoints found that support tool use. Try disabling \"search\".","code":404,"metadata":{"routing_funnel":[{"step":"Initial Endpoints","endpoint_count":1}],"failed_routing_step":"Filter by Tool Compatibility"}}}"#;
        let server = MockServer::new(vec![Mock::status(404, filtered)]);
        assert!(matches!(turn(&server, true), Err(Error::NoToolEndpoint)));

        let other = MockServer::new(vec![Mock::status(
            404,
            r#"{"error":{"message":"No such model","code":404}}"#,
        )]);
        assert!(matches!(
            turn(&other, true),
            Err(Error::Provider { status: 404, .. })
        ));
    }

    #[test]
    fn assistant_and_tool_messages_serialise_to_the_openai_shape() {
        let assistant = Message {
            role: MessageRole::Assistant,
            content: String::new(),
            tool_calls: Some(vec![ToolCall {
                id: "c1".into(),
                query: "ext4 journal".into(),
            }]),
            tool_call_id: None,
        };
        let v = serde_json::to_value(&assistant).unwrap();
        assert_eq!(v["role"], "assistant");
        let call = &v["tool_calls"][0];
        assert_eq!(call["id"], "c1");
        assert_eq!(call["type"], "function");
        assert_eq!(call["function"]["name"], "search");
        assert_eq!(
            call["function"]["arguments"], "{\"query\":\"ext4 journal\"}",
            "arguments is a JSON string, not an object: {v}"
        );

        let tool = Message {
            role: MessageRole::Tool,
            content: "passages".into(),
            tool_calls: None,
            tool_call_id: Some("c1".into()),
        };
        let v = serde_json::to_value(&tool).unwrap();
        assert_eq!(v["role"], "tool");
        assert_eq!(v["tool_call_id"], "c1");
    }

    #[test]
    fn a_call_to_another_tool_is_text() {
        let body = r#"{"choices":[{"message":{"content":"hm","tool_calls":[{"id":"c1","type":"function","function":{"name":"fetch","arguments":"{\"query\":\"x\"}"}}]}}]}"#;
        let server = MockServer::new(vec![Mock::ok(body)]);
        assert_eq!(turn(&server, true).unwrap(), Reply::Text("hm".into()));
    }
}

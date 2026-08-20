//! The chat-completions call: the one place the product asks a model to write
//! prose. `probe` answers "does this key work" and "will this model fill an
//! index with numbers that mean something"; this answers "given these
//! messages, what does the model say". A non-streaming POST to
//! `/chat/completions`, ported from the server's `app/llm/litellm_provider.py`
//! — model and messages only, no sampling parameters (§7.4, spec §PR 2).

use serde::{Deserialize, Serialize};

use crate::{Error, http};

/// One chat message, in the shape OpenRouter's `/chat/completions` accepts.
///
/// `Serialize` only, like `ModelEntry`: it is sent, never parsed back — the
/// answer is read into `chat`'s own private response structs. `build_messages`
/// in `mnema-rag` (PR 3) constructs these; the fields are `pub` so it can.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

/// Who a message is from. An enum, not a `String`, for the reason the crate
/// keeps `KeySent`/`Role`/`Redaction` as types: a role is a closed set, and a
/// `String` field where only two values are valid is exactly the "a string
/// that happens to be right" this crate refuses. Only the two roles
/// `build_messages` produces (`prompt.py:121-124`); `Assistant` is added when a
/// cycle needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
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
    content: String,
}

/// Sends `messages` to the chat model and returns its answer verbatim.
///
/// Ported from `app/llm/litellm_provider.py:96,139-150`: model and messages
/// only, no streaming and no sampling parameters (§7.4). The returned text is
/// untouched — anchors, whitespace and all — because resolving `<c>N</c>`
/// belongs to `mnema-rag` and the empty-answer refusal belongs to the bridge
/// (spec §4); this call's one job is the round trip.
pub fn complete(base: &str, key: &str, model: &str, messages: &[Message]) -> Result<String, Error> {
    let request = serde_json::json!({ "model": model, "messages": messages }).to_string();
    let (status, answer) = http::post_json(base, "/chat/completions", key, &request)?;
    if status != 200 {
        return Err(crate::probe::attach_reason(
            crate::error_for_status(status, crate::KeySent::Yes),
            &answer,
            key,
        ));
    }
    let completion: Completion = serde_json::from_str(&answer)
        .map_err(|_| Error::Malformed("the chat answer is not the shape this code expects"))?;
    let text = completion
        .choices
        .into_iter()
        .next()
        .ok_or(Error::Malformed("the chat answer carried no choices"))?
        .message
        .content;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_serialises_to_the_openai_role_content_shape() {
        let message = Message {
            role: MessageRole::System,
            content: "you answer with citations".to_string(),
        };
        let value = serde_json::to_value(&message).expect("serialises");
        assert_eq!(
            value["role"], "system",
            "the role is a lowercase wire string: {value}"
        );
        assert_eq!(value["content"], "you answer with citations");

        let user = serde_json::to_value(MessageRole::User).expect("serialises");
        assert_eq!(
            user, "user",
            "MessageRole::User must serialise as \"user\", not \"User\""
        );
    }
}

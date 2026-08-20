//! The chat-completions call: the one place the product asks a model to write
//! prose. `probe` answers "does this key work" and "will this model fill an
//! index with numbers that mean something"; this answers "given these
//! messages, what does the model say". A non-streaming POST to
//! `/chat/completions`, ported from the server's `app/llm/litellm_provider.py`
//! — model and messages only, no sampling parameters (§7.4, spec §PR 2).

use serde::Serialize;

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

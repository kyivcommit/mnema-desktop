use mnema_provider::Error;

use crate::anchors::resolve_anchors;
use crate::prompt::{Passage, build_messages};

/// The synthesised answer: the anchor-resolved text and the 1-based ordinals
/// that resolved, in first-occurrence order. Invalid anchors are already gone
/// from `text` and absent from `cited` (`resolve_anchors`, spec §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub text: String,
    pub cited: Vec<usize>,
}

/// The RAG core seam: build the two synthesis messages, ask the chat model,
/// and resolve the `<c>N</c>` anchors it wrote against `passages`
/// (`app/rag/service.py`). Ported flat — `base`/`key`/`model` mirror
/// `mnema_provider::complete`'s own parameters, not a bundling type, so this
/// crate keeps its provider-only dependency.
///
/// `Ok(None)` means the model's raw answer was whitespace only — the empty
/// completion the bridge turns into `Refused{EmptyCompletion}` (spec §6). The
/// check is on the *raw* completion, before anchor resolution, so a completion
/// of prose-plus-anchors is never mistaken for empty and an all-invalid-anchor
/// completion is treated exactly as the server treats it.
pub fn answer(
    base: &str,
    key: &str,
    model: &str,
    question: &str,
    passages: &[Passage],
    answer_lang: Option<&str>,
) -> Result<Option<Answer>, Error> {
    let messages = build_messages(question, passages, answer_lang);
    let raw = mnema_provider::complete(base, key, model, &messages)?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let (text, cited) = resolve_anchors(&raw, passages.len());
    Ok(Some(Answer { text, cited }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Passage;
    use mnema_mock_provider::{MockServer, Reply};

    fn one_passage() -> Vec<Passage> {
        vec![
            Passage {
                text: "The sky is blue.".into(),
                meta: "sky.txt".into(),
            },
            Passage {
                text: "Grass is green.".into(),
                meta: "grass.txt".into(),
            },
        ]
    }

    /// A minimal chat-completions body, built by hand: this crate has no
    /// `serde_json` dependency, and none of the fixtures below need escaping,
    /// so a `format!` template stays valid JSON without one.
    fn completion(content: &str) -> String {
        format!(r#"{{"choices":[{{"message":{{"content":"{content}"}}}}]}}"#)
    }

    #[test]
    fn it_builds_the_prompt_sends_it_and_resolves_the_anchors() {
        let server = MockServer::new(vec![Reply::ok(&completion("The sky is blue <c>1</c>."))]);
        let out = answer(server.base(), "k", "m", "why?", &one_passage(), None)
            .expect("the call succeeds")
            .expect("a non-empty completion is Some");
        assert_eq!(out.text, "The sky is blue <c>1</c>.");
        assert_eq!(out.cited, vec![1]);

        // The request that actually went on the wire carried the built prompt.
        let request = server.request();
        assert!(
            request.contains("/chat/completions"),
            "wrong endpoint: {request}"
        );
        assert!(
            request.contains("The sky is blue."),
            "the source text was not sent: {request}"
        );
    }

    #[test]
    fn an_out_of_range_anchor_never_reaches_the_citations() {
        // Two passages, so <c>9</c> is invalid: resolve_anchors must cut it
        // from the text and never list it. Otherwise a hallucinated citation
        // reaches the window (spec §9).
        let server = MockServer::new(vec![Reply::ok(&completion(
            "Guessing <c>9</c> here <c>2</c>.",
        ))]);
        let out = answer(server.base(), "k", "m", "why?", &one_passage(), None)
            .unwrap()
            .unwrap();
        assert_eq!(out.cited, vec![2], "only the in-range anchor is cited");
        assert!(
            !out.text.contains('9'),
            "the invalid anchor was left in the text: {}",
            out.text
        );
    }

    #[test]
    fn a_whitespace_only_completion_is_none_not_an_empty_answer() {
        // Server returned nothing usable. The empty-completion refusal is the
        // bridge's to name; answer's job is only to report the emptiness of the
        // *raw* completion, before anchor resolution (spec §6). Three plain
        // spaces, no newline, so the JSON body stays valid with no escaping.
        let server = MockServer::new(vec![Reply::ok(&completion("   "))]);
        let out = answer(server.base(), "k", "m", "why?", &one_passage(), None).unwrap();
        assert!(
            out.is_none(),
            "a blank completion must be None, got {out:?}"
        );
    }
}

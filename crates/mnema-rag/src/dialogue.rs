//! The chat messages of a multi-turn launcher conversation: earlier turns as
//! plain user/assistant pairs, only the latest search's sources in context.

use mnema_provider::{Message, MessageRole, ToolCall};

use crate::anchors::strip_anchors;
use crate::lang::Lang;
use crate::prompt::{AUTO_DIRECTIVE, Passage, SYSTEM_PROMPT_HEAD, sources_block};

/// What a model needs on top of the shared system prompt.
pub struct Profile {
    pub system_addendum: &'static str,
}

/// One finished exchange. `sources` is the search made for this turn, if any;
/// `answer` still carries its anchors.
pub struct Turn {
    pub question: String,
    pub answer: String,
    pub sources: Option<Vec<Passage>>,
}

/// The measured search hint: without it the model has no reason to search again.
const HINT: &str = concat!(
    " You have a tool, search, that searches the user's document library. Call it whenever the user asks for ",
    "something from the library that the sources already in this conversation do not contain, including when the ",
    "user asks to find more or to look up another topic. Do not answer library questions from memory. Do not call it ",
    "for thanks, shortening, translation, or questions about this conversation. Source numbers restart with every ",
    "search: cite only the numbers of the sources returned by the most recent search, never numbers from earlier turns.",
);

/// Per-model exceptions go here; none yet, so every model gets the hint.
pub fn profile(_model: &str) -> Profile {
    Profile {
        system_addendum: HINT,
    }
}

fn message(role: MessageRole, content: String) -> Message {
    Message {
        role,
        content,
        tool_calls: None,
        tool_call_id: None,
    }
}

fn with_sources(passages: &[Passage], question: &str) -> String {
    format!(
        "Sources:\n\n{}\n\nQuestion: {question}",
        sources_block(passages)
    )
}

/// The messages for the next model call. Only the latest search's sources are
/// shown (`fresh`, else the newest turn that has them); answers before that
/// search lose their anchors, whose numbers no longer point anywhere.
pub fn build_dialogue(
    turns: &[Turn],
    question: &str,
    fresh: Option<&[Passage]>,
    profile: &Profile,
    lang: Lang,
) -> Vec<Message> {
    let latest = if fresh.is_some() {
        None
    } else {
        turns
            .iter()
            .rposition(|t| t.sources.as_ref().is_some_and(|s| !s.is_empty()))
    };
    let mut out = vec![message(
        MessageRole::System,
        format!(
            "{SYSTEM_PROMPT_HEAD}{AUTO_DIRECTIVE}{}",
            profile.system_addendum
        ),
    )];
    for (i, t) in turns.iter().enumerate() {
        let user = match (&t.sources, latest) {
            (Some(s), Some(l)) if l == i => with_sources(s, &t.question),
            _ => t.question.clone(),
        };
        let kept = latest.is_some_and(|l| i >= l);
        out.push(message(MessageRole::User, user));
        out.push(message(
            MessageRole::Assistant,
            if kept {
                t.answer.clone()
            } else {
                strip_anchors(&t.answer)
            },
        ));
    }
    let current = fresh.map_or_else(|| question.to_string(), |p| with_sources(p, question));
    out.push(message(
        MessageRole::User,
        format!(
            "{current}\n\nWrite your entire answer in {}, even though the Question and the sources may be in another language.",
            lang.name
        ),
    ));
    out
}

/// Append the model's `search` call and the passages it returned.
pub fn with_search_result(messages: &mut Vec<Message>, call: &ToolCall, passages: &[Passage]) {
    messages.push(Message {
        role: MessageRole::Assistant,
        content: String::new(),
        tool_calls: Some(vec![call.clone()]),
        tool_call_id: None,
    });
    let content = if passages.is_empty() {
        "No passages found.".to_string()
    } else {
        sources_block(passages)
    };
    messages.push(Message {
        role: MessageRole::Tool,
        content,
        tool_calls: None,
        tool_call_id: Some(call.id.clone()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::{EN, UK};
    use crate::prompt::{Voice, build_messages};
    use mnema_provider::MessageRole;

    fn ps(text: &str) -> Vec<Passage> {
        vec![Passage {
            text: text.into(),
            meta: "m.txt".into(),
        }]
    }

    fn turn(q: &str, a: &str, sources: Option<Vec<Passage>>) -> Turn {
        Turn {
            question: q.into(),
            answer: a.into(),
            sources,
        }
    }

    const NO_PROFILE: Profile = Profile {
        system_addendum: "",
    };

    #[test]
    fn the_first_turn_differs_from_build_messages_only_by_profile_and_language() {
        let p = ps("The sky is blue.");
        let d = build_dialogue(&[], "Why?", Some(&p), &NO_PROFILE, EN);
        let b = build_messages("Why?", &p, Voice::OpenRouter);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].content, b[0].content);
        let head = b[1]
            .content
            .strip_suffix("Write your entire answer in the same language as the Question above.")
            .unwrap();
        assert!(d[1].content.starts_with(head), "{}", d[1].content);
        assert!(head.ends_with("\n\n"));
    }

    #[test]
    fn only_the_latest_search_reaches_the_context() {
        let turns = [
            turn("q1", "a1", Some(ps("ALPHA-TEXT"))),
            turn("q2", "a2", Some(ps("BETA-TEXT"))),
            turn("q3", "a3", None),
        ];
        let d = build_dialogue(&turns, "q4", None, &NO_PROFILE, EN);
        let with = |needle: &str| d.iter().filter(|m| m.content.contains(needle)).count();
        assert_eq!(with("ALPHA-TEXT"), 0);
        assert_eq!(with("BETA-TEXT"), 1);
        assert_eq!(d.len(), 1 + 2 * 3 + 1);
    }

    #[test]
    fn a_fresh_search_replaces_every_earlier_one() {
        let turns = [turn("q1", "a1<c>1</c>", Some(ps("ALPHA-TEXT")))];
        let fresh = ps("FRESH-TEXT");
        let d = build_dialogue(&turns, "q2", Some(&fresh), &NO_PROFILE, EN);
        let with = |needle: &str| d.iter().filter(|m| m.content.contains(needle)).count();
        assert_eq!(with("ALPHA-TEXT"), 0);
        assert_eq!(with("FRESH-TEXT"), 1);
        assert_eq!(d[2].content, "a1", "the old answer keeps no anchor");
    }

    #[test]
    fn answers_before_the_latest_search_lose_their_anchors() {
        let turns = [
            turn("q1", "one<c>1</c>", Some(ps("A"))),
            turn("q2", "two<c>1</c>", Some(ps("B"))),
        ];
        let d = build_dialogue(&turns, "q3", None, &NO_PROFILE, EN);
        let answers: Vec<&str> = d
            .iter()
            .filter(|m| m.role == MessageRole::Assistant)
            .map(|m| m.content.as_str())
            .collect();
        assert_eq!(answers, ["one", "two<c>1</c>"]);
    }

    #[test]
    fn every_model_gets_the_search_hint() {
        let p = profile("anthropic/x");
        assert!(p.system_addendum.contains("search"));
        assert!(p.system_addendum.starts_with(' '));
        let d = build_dialogue(&[], "q", None, &p, EN);
        assert!(d[0].content.ends_with(p.system_addendum));
        assert_eq!(profile("other/y").system_addendum, p.system_addendum);
    }

    #[test]
    fn the_language_sentence_names_the_given_language() {
        let d = build_dialogue(&[], "q", None, &NO_PROFILE, UK);
        assert!(
            d.last()
                .unwrap()
                .content
                .ends_with("Write your entire answer in Ukrainian, even though the Question and the sources may be in another language."),
            "{}",
            d.last().unwrap().content
        );
    }

    #[test]
    fn a_search_result_follows_its_call() {
        let call = ToolCall {
            id: "c1".into(),
            query: "sky".into(),
        };
        let mut m = Vec::new();
        with_search_result(&mut m, &call, &ps("Blue."));
        assert_eq!(m[0].role, MessageRole::Assistant);
        assert_eq!(m[0].tool_calls.as_ref().unwrap()[0].id, "c1");
        assert_eq!(m[1].role, MessageRole::Tool);
        assert_eq!(m[1].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(m[1].content, "[1] (m.txt)\nBlue.");
        let mut e = Vec::new();
        with_search_result(&mut e, &call, &[]);
        assert_eq!(e[1].content, "No passages found.");
    }
}

//! The chat messages of a multi-turn launcher conversation: earlier turns as
//! plain user/assistant pairs, only the latest search's sources in context.

use mnema_provider::{Message, MessageRole};

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

/// [`HINT`] and one sentence more, for the local Gemma 4 E4B: without it the
/// model answers "the sources do not contain it" from the sources it already
/// has instead of searching (measured 2026-11-03, D188: "find more" 22/24 →
/// 24/24). Written out again because `concat!` takes literals only.
const HINT_SEARCH_INSTEAD: &str = concat!(
    " You have a tool, search, that searches the user's document library. Call it whenever the user asks for ",
    "something from the library that the sources already in this conversation do not contain, including when the ",
    "user asks to find more or to look up another topic. Do not answer library questions from memory. Do not call it ",
    "for thanks, shortening, translation, or questions about this conversation. Source numbers restart with every ",
    "search: cite only the numbers of the sources returned by the most recent search, never numbers from earlier turns.",
    " If the sources already in this conversation do not contain the answer, call search instead of saying that the ",
    "sources do not contain it.",
);

/// The local E4B's name on the wire (`src-tauri`'s `LOCAL_CHAT_TOOLS_MODEL`).
const LOCAL_E4B: &str = "gemma-4-e4b-it";

/// Per-model exceptions: the local E4B gets one sentence more (D188); every
/// other model, the hint as measured on the cloud.
pub fn profile(model: &str) -> Profile {
    Profile {
        system_addendum: if model == LOCAL_E4B {
            HINT_SEARCH_INSTEAD
        } else {
            HINT
        },
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

/// Rewrite the current question (`asked`, as [`build_dialogue`] wrote it) with
/// what the model's searches returned so far: the latest search that found
/// passages, then one line per search that found none. The shape a search made
/// before the call has, not an assistant tool call plus a tool message: with a
/// tool exchange in a history sent without `tools`, DeepSeek asks to search
/// again — Novita, Ionstream, Parasail, Together and AtlasCloud then hand back
/// raw `<｜DSML｜` markup as the answer (measured 2026-11-05: 6/30 bad replies,
/// 0/30 inline).
pub fn with_search_result(
    messages: &mut [Message],
    asked: &str,
    shown: &[Passage],
    misses: &[String],
) {
    let Some(current) = messages.last_mut() else {
        return;
    };
    let mut found: Vec<String> = Vec::new();
    if !shown.is_empty() {
        found.push(format!("Sources:\n\n{}", sources_block(shown)));
    }
    found.extend(
        misses
            .iter()
            .map(|q| format!("No passages found for the search «{q}».")),
    );
    current.content = format!("{}\n\nQuestion: {asked}", found.join("\n\n"));
}

/// Take the `search` hint out of the system prompt, for the call that is
/// offered no tool: told it has one, the model writes the call out as text
/// (`<tool_call>search{…}`, owner's launcher, 2026-11-05).
pub fn drop_search_hint(messages: &mut [Message]) {
    if let Some(system) = messages
        .first_mut()
        .filter(|m| m.role == MessageRole::System)
    {
        // The longer hint first: it begins with the shorter one.
        system.content = system
            .content
            .replace(HINT_SEARCH_INSTEAD, "")
            .replace(HINT, "");
    }
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
    fn an_answer_after_the_latest_search_keeps_its_anchors() {
        let turns = [
            turn("q1", "one<c>1</c>", Some(ps("A"))),
            turn("q2", "two<c>1</c>", Some(ps("B"))),
            turn("q3", "three<c>1</c>", None),
        ];
        let d = build_dialogue(&turns, "q4", None, &NO_PROFILE, EN);
        let answers: Vec<&str> = d
            .iter()
            .filter(|m| m.role == MessageRole::Assistant)
            .map(|m| m.content.as_str())
            .collect();
        assert_eq!(answers, ["one", "two<c>1</c>", "three<c>1</c>"]);
        assert_eq!(
            d[5].content, "q3",
            "a turn without a search is a plain question"
        );
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
    fn the_local_e4b_gets_the_hint_and_one_sentence_more() {
        let e4b = profile("gemma-4-e4b-it").system_addendum;
        let rest = e4b
            .strip_prefix(HINT)
            .expect("the hint itself, unchanged, comes first");
        assert_eq!(
            rest,
            " If the sources already in this conversation do not contain the answer, call search instead of saying that the sources do not contain it."
        );
        for other in ["gemma-4-e2b-it", "openai/gpt-6-luna", "gemma-4-e4b"] {
            assert_eq!(profile(other).system_addendum, HINT, "{other}");
        }
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
    fn a_search_result_joins_the_question_without_a_tool_exchange() {
        let p = profile("m");
        let mut m = build_dialogue(&[], "Why?", None, &p, UK);
        let asked = m.last().unwrap().content.clone();
        with_search_result(&mut m, &asked, &ps("Blue."), &[]);
        let searched_first = build_dialogue(&[], "Why?", Some(&ps("Blue.")), &p, UK);
        assert_eq!(m.len(), searched_first.len());
        assert_eq!(
            m.last().unwrap().content,
            searched_first.last().unwrap().content
        );
        assert!(
            m.iter()
                .all(|x| x.tool_calls.is_none() && x.tool_call_id.is_none())
        );
        let mut e = build_dialogue(&[], "Why?", None, &p, UK);
        with_search_result(&mut e, &asked, &[], &["sky".to_string()]);
        assert!(
            e.last()
                .unwrap()
                .content
                .starts_with("No passages found for the search «sky».\n\nQuestion: Why?\n\n"),
            "{}",
            e.last().unwrap().content
        );
    }

    #[test]
    fn a_second_search_replaces_the_first_and_keeps_its_misses() {
        let p = profile("m");
        let mut m = build_dialogue(&[], "Why?", None, &p, UK);
        let asked = m.last().unwrap().content.clone();
        with_search_result(&mut m, &asked, &ps("Red."), &[]);
        with_search_result(&mut m, &asked, &ps("Blue."), &["sky".to_string()]);
        let last = &m.last().unwrap().content;
        assert!(
            last.starts_with(
                "Sources:\n\n[1] (m.txt)\nBlue.\n\nNo passages found for the search «sky».\n\nQuestion: Why?\n\n"
            ),
            "{last}"
        );
        assert!(!last.contains("Red."), "{last}");
        assert_eq!(last.matches("Question:").count(), 1, "{last}");
    }

    #[test]
    fn the_search_hint_leaves_the_system_prompt_and_nothing_else() {
        for model in ["m", LOCAL_E4B] {
            let mut m = build_dialogue(&[], "Why?", None, &profile(model), UK);
            let before = m[0].content.clone();
            drop_search_hint(&mut m);
            assert!(before.contains("You have a tool, search"), "{model}");
            assert!(!m[0].content.contains("tool"), "{model}: {}", m[0].content);
            assert!(before.starts_with(&m[0].content), "{model}");
            assert!(!m[0].content.is_empty());
        }
    }
}

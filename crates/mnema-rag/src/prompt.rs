//! The synthesis prompt — a pure port of the server's `app/rag/prompt.py`
//! (`_SYSTEM_PROMPT_TEMPLATE`, the language directives, `build_messages`). No
//! I/O. The `_meta` join and `Coordinate::render()` live in the bridge (PR 4);
//! this module receives each source's meta already rendered as a string.

use mnema_provider::{Message, MessageRole};

/// One retrieved source as it enters the prompt. `text` is the original chunk
/// text (verbatim, never truncated — `prompt.py:103-108`); `meta` is the
/// pre-rendered locator line (`relative_path · <coordinate>`, built by the
/// bridge per spec §7.1), empty when there is nothing to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    pub text: String,
    pub meta: String,
}

/// Rules 1-4 plus `"5. "`, ready for the language directive to be appended
/// (`prompt.py:17-41`, `_SYSTEM_PROMPT_TEMPLATE` with `{language_directive}` as
/// the tail of rule 5). Segments mirror the Python literal breaks 1:1.
const SYSTEM_PROMPT_HEAD: &str = concat!(
    "You are a careful research assistant answering questions about a collection of documents. ",
    "You are given numbered SOURCE passages.\n",
    "Rules (follow strictly):\n",
    "1. Answer ONLY using facts found in the numbered sources; do not use outside knowledge. ",
    "Expanding an abbreviation or short form to its full word, and matching an inflected or ",
    "declined word form, is reading comprehension — not outside knowledge. For example, a ",
    "source that writes apt. for apartment still states the apartment; expand such short forms ",
    "to their full word. Match on meaning, not on exact characters.\n",
    "2. After every factual statement, cite the source(s) it came from with the inline anchor ",
    "<c>N</c>, where N is the source number. Cite multiple sources as <c>1</c><c>3</c>.\n",
    "3. Never invent or guess a source number — cite only numbers that appear in the list.\n",
    "4. Say information is missing ONLY when the fact is genuinely absent, never merely because ",
    "the wording or abbreviation differs from the question; add no citation anchors when you ",
    "do.\n",
    "5. ",
);

/// Rule-5 directive when no explicit target language is given (`prompt.py:50-57`,
/// `_AUTO_DIRECTIVE`). FORCEFUL on purpose — a lite chat model otherwise drifts
/// into the source language or English.
const AUTO_DIRECTIVE: &str = concat!(
    "OUTPUT LANGUAGE — THIS IS THE MOST IMPORTANT RULE, AND IT OVERRIDES THE SOURCE LANGUAGE: ",
    "write the ENTIRE answer in the same language as the question. Detect the question's ",
    "language and use exactly that language (for example, a Ukrainian question gets a Ukrainian ",
    "answer). The SOURCE passages may be in another language; translate any facts or quotes you ",
    "cite into the question's language. NEVER answer in English unless the question itself is in ",
    "English. Do NOT state or mention which language you are using — just give the answer.",
);

/// The ISO directive wraps a caller-supplied language code (`prompt.py:58-63`,
/// `_ISO_DIRECTIVE_TEMPLATE`); `{lang}` sits between HEAD and TAIL.
const ISO_DIRECTIVE_HEAD: &str = concat!(
    "OUTPUT LANGUAGE — THIS OVERRIDES THE SOURCE LANGUAGE: write the ENTIRE answer in this ",
    "language (BCP-47 / ISO code): ",
);
const ISO_DIRECTIVE_TAIL: &str = concat!(
    ". The SOURCE passages may be in a different language; translate any facts or quotes you ",
    "cite into that language. Do NOT state which language you are using — just give the answer.",
);

/// Appended to the END of the user turn in auto mode, where the model attends to
/// it most (`prompt.py:66`, `_AUTO_USER_REMINDER`).
const AUTO_USER_REMINDER: &str =
    "Write your entire answer in the same language as the Question above.";

/// Auto = no explicit target language: `None`, empty, whitespace-only, or
/// case-insensitive `"auto"` after trimming (`prompt.py:79-85`, `_is_auto`).
fn is_auto(answer_lang: Option<&str>) -> bool {
    let stripped = answer_lang.unwrap_or("").trim();
    stripped.is_empty() || stripped.eq_ignore_ascii_case("auto")
}

/// Rule-5 directive: the question-language directive for auto, the ISO directive
/// otherwise (`prompt.py:88-92`, `_build_language_directive`).
fn language_directive(answer_lang: Option<&str>) -> String {
    if is_auto(answer_lang) {
        return AUTO_DIRECTIVE.to_string();
    }
    let lang = answer_lang.unwrap_or("").trim();
    format!("{ISO_DIRECTIVE_HEAD}{lang}{ISO_DIRECTIVE_TAIL}")
}

/// Build the two chat messages for synthesis (`prompt.py:95-124`). Each source
/// shows its FULL text — the chunker already size-bounds chunks, and a per-source
/// cap would silently hide a fact past the cap (`prompt.py:103-108`).
pub fn build_messages(
    question: &str,
    passages: &[Passage],
    answer_lang: Option<&str>,
) -> Vec<Message> {
    let blocks: Vec<String> = passages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let n = i + 1;
            let header = if p.meta.is_empty() {
                format!("[{n}]")
            } else {
                format!("[{n}] ({})", p.meta)
            };
            format!("{header}\n{}", p.text)
        })
        .collect();
    let sources = blocks.join("\n\n");
    let mut user = format!("Sources:\n\n{sources}\n\nQuestion: {question}");
    if is_auto(answer_lang) {
        user.push_str("\n\n");
        user.push_str(AUTO_USER_REMINDER);
    }
    let system = format!("{SYSTEM_PROMPT_HEAD}{}", language_directive(answer_lang));
    vec![
        Message {
            role: MessageRole::System,
            content: system,
        },
        Message {
            role: MessageRole::User,
            content: user,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use mnema_provider::MessageRole;

    // Byte-for-byte from the server's own `build_messages` (auto mode), captured
    // by running `app/rag/prompt.py` — an oracle independent of the `concat!`
    // constants above, so a transcription drift fails loudly.
    const EXPECTED_SYSTEM_AUTO: &str = "You are a careful research assistant answering questions about a collection of documents. You are given numbered SOURCE passages.\nRules (follow strictly):\n1. Answer ONLY using facts found in the numbered sources; do not use outside knowledge. Expanding an abbreviation or short form to its full word, and matching an inflected or declined word form, is reading comprehension — not outside knowledge. For example, a source that writes apt. for apartment still states the apartment; expand such short forms to their full word. Match on meaning, not on exact characters.\n2. After every factual statement, cite the source(s) it came from with the inline anchor <c>N</c>, where N is the source number. Cite multiple sources as <c>1</c><c>3</c>.\n3. Never invent or guess a source number — cite only numbers that appear in the list.\n4. Say information is missing ONLY when the fact is genuinely absent, never merely because the wording or abbreviation differs from the question; add no citation anchors when you do.\n5. OUTPUT LANGUAGE — THIS IS THE MOST IMPORTANT RULE, AND IT OVERRIDES THE SOURCE LANGUAGE: write the ENTIRE answer in the same language as the question. Detect the question's language and use exactly that language (for example, a Ukrainian question gets a Ukrainian answer). The SOURCE passages may be in another language; translate any facts or quotes you cite into the question's language. NEVER answer in English unless the question itself is in English. Do NOT state or mention which language you are using — just give the answer.";

    #[test]
    fn the_system_prompt_is_the_ported_rules_plus_the_auto_directive() {
        let messages = build_messages("q?", &[], None);
        assert_eq!(messages.len(), 2, "system + user");
        assert_eq!(messages[0].role, MessageRole::System);
        assert_eq!(messages[1].role, MessageRole::User);
        assert_eq!(messages[0].content, EXPECTED_SYSTEM_AUTO);
    }

    fn two_passages() -> Vec<Passage> {
        vec![
            Passage {
                text: "The sky is blue.".into(),
                meta: "docs/sky.txt".into(),
            },
            Passage {
                text: "Grass is green.".into(),
                meta: "docs/grass.md · Розділ 2".into(),
            },
        ]
    }

    #[test]
    fn the_user_turn_numbers_sources_with_meta_then_the_question_and_reminder() {
        let messages = build_messages("What colour is the sky?", &two_passages(), None);
        let expected = concat!(
            "Sources:\n\n",
            "[1] (docs/sky.txt)\nThe sky is blue.\n\n",
            "[2] (docs/grass.md · Розділ 2)\nGrass is green.\n\n",
            "Question: What colour is the sky?\n\n",
            "Write your entire answer in the same language as the Question above.",
        );
        assert_eq!(messages[1].content, expected);
    }

    #[test]
    fn a_source_with_empty_meta_gets_a_bare_bracket_header() {
        let passages = vec![Passage {
            text: "Bare.".into(),
            meta: String::new(),
        }];
        let messages = build_messages("q?", &passages, None);
        assert!(
            messages[1].content.contains("[1]\nBare."),
            "empty meta must yield `[1]` with no parens: {}",
            messages[1].content
        );
        assert!(
            !messages[1].content.contains("[1] ("),
            "no empty parens for a bare header"
        );
    }
}

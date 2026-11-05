//! The application's own translation of a question into search terms in the
//! languages of the library, so that finding a document written in another
//! language does not depend on the chat model choosing to search that way.
//!
//! No language is named in this file: the library's languages come from its
//! own text, and a library whose languages cannot be told gets no translation.

use mnema_provider::{Message, MessageRole};

/// A language needs this share of the reliably detected samples to count as
/// one of the library's: below it, a few quoted foreign passages would send
/// every question through a translation nobody's documents are written in.
const MIN_SHARE: f64 = 0.2;
/// Two languages cover a bilingual library, and every further one is another
/// term competing for the same few results.
const MAX_LANGUAGES: usize = 2;
/// One key term, its translation, and a name spelled the documents' way.
pub const MAX_TERMS: usize = 3;

/// The library's languages, most common first, from samples of its text.
///
/// Only reliable detections are counted, and they are the denominator: a
/// library of short or mixed snippets says nothing rather than something
/// wrong. Empty means the caller must not translate.
pub fn library_languages(samples: &[&str]) -> Vec<whatlang::Lang> {
    let detected: Vec<whatlang::Lang> = samples
        .iter()
        .filter_map(|s| whatlang::detect(s))
        .filter(whatlang::Info::is_reliable)
        .map(|i| i.lang())
        .collect();
    if detected.is_empty() {
        return Vec::new();
    }
    let mut counts: Vec<(whatlang::Lang, usize)> = Vec::new();
    for lang in &detected {
        match counts.iter_mut().find(|(l, _)| l == lang) {
            Some((_, n)) => *n += 1,
            None => counts.push((*lang, 1)),
        }
    }
    // Stable on ties: the language met first in the samples stays first.
    counts.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
    counts
        .into_iter()
        .filter(|&(_, n)| n as f64 / detected.len() as f64 >= MIN_SHARE)
        .take(MAX_LANGUAGES)
        .map(|(l, _)| l)
        .collect()
}

/// Whether translating this question can find anything its own words cannot:
/// not when the library's languages are unknown, and not when the library has
/// one language and the question is reliably in it.
pub fn needs_translation(question: &str, languages: &[whatlang::Lang]) -> bool {
    match languages {
        [] => false,
        [only] => whatlang::detect(question)
            .filter(whatlang::Info::is_reliable)
            .is_none_or(|i| i.lang() != *only),
        _ => true,
    }
}

/// The one chat call that turns a question into search terms. Offered no
/// tool: asked with none, a model told about one writes the call out as text
/// (D191).
pub fn translation_messages(question: &str, languages: &[whatlang::Lang]) -> Vec<Message> {
    let names: Vec<&str> = languages.iter().map(|l| l.eng_name()).collect();
    let system = format!(
        "You turn a question into search terms for a document library. \
         The documents are written in: {}. Reply with the term in each of these \
         languages, one per line, and nothing else — a term in every language, even \
         when the question is already in one of them. \
         A term is the key word or name the question is about, not a sentence. \
         Spell names of people, products and places as a document in that language would.",
        names.join(", ")
    );
    vec![
        Message {
            role: MessageRole::System,
            content: system,
        },
        Message {
            role: MessageRole::User,
            content: question.to_string(),
        },
    ]
}

/// The terms in a translator's reply: one per line, list markers and quotes
/// removed, a heading line (ending in a colon) dropped, a line in two scripts
/// split into one term per script, blanks, repeats, terms longer than
/// `max_chars` and the `question` itself refused, at most [`MAX_TERMS`]. The
/// question's own words were already searched: the same words again would
/// spend a reserved place on hits the model already has (luna, owner's ask
/// log 2026-11-05).
pub fn parse_terms(raw: &str, question: &str, max_chars: usize) -> Vec<String> {
    let asked = question
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    let mut terms: Vec<String> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.ends_with(':') {
            continue;
        }
        let line = strip_marker(line)
            .trim_matches(|c: char| QUOTES.contains(c) || c.is_whitespace())
            .to_string();
        if mixes_scripts(&line) {
            continue;
        }
        for term in script_runs(&line) {
            let key = term.to_lowercase();
            if term.is_empty()
                || term.chars().count() > max_chars
                || key == asked
                || terms.iter().any(|t| t.to_lowercase() == key)
            {
                continue;
            }
            terms.push(term);
            if terms.len() == MAX_TERMS {
                return terms;
            }
        }
    }
    terms
}

/// «Solpadeine (Солпадеїн)» → «Solpadeine», «Солпадеїн»: a term is searched
/// with all its words required in one chunk, and a library holding both
/// spellings holds them in different chunks, so the pair would find nothing.
/// A line in one script is returned whole, punctuation and all (`.NET`).
fn script_runs(line: &str) -> Vec<String> {
    let script = |w: &str| {
        w.chars()
            .find(|c| c.is_alphabetic())
            .and_then(|c| whatlang::detect_script(c.encode_utf8(&mut [0; 4])))
    };
    let mut runs: Vec<(Option<whatlang::Script>, Vec<&str>)> = Vec::new();
    for word in line.split_whitespace() {
        let s = script(word);
        // A word with no letters (`500`) joins the run it follows.
        match runs.last_mut() {
            Some((last, words)) if s.is_none() || last.is_none() || *last == s => {
                *last = last.or(s);
                words.push(word);
            }
            _ => runs.push((s, vec![word])),
        }
    }
    if runs.len() < 2 {
        return vec![line.to_string()];
    }
    runs.into_iter()
        .map(|(_, words)| {
            words
                .join(" ")
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_string()
        })
        .collect()
}

/// A word whose letters come from more than one script (`солpadein`): no
/// document spells a word that way in any language, so it can only miss, and
/// a reserved place spent on it is lost (E2B, owner's log 2026-11-05). Words
/// are judged one at a time: «Solpadeine (Солпадеїн)» is two words, each fine.
fn mixes_scripts(term: &str) -> bool {
    term.split(|c: char| !c.is_alphabetic()).any(|word| {
        let mut scripts = word
            .chars()
            .filter_map(|c| whatlang::detect_script(c.encode_utf8(&mut [0; 4])));
        scripts
            .next()
            .is_some_and(|first| scripts.any(|s| s != first))
    })
}

const QUOTES: &str = "\"'`«»“”„‘’";

/// `- x`, `* x`, `• x`, `1. x`, `2) x` → `x`.
fn strip_marker(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix(['-', '*', '•']) {
        return rest;
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0
        && let Some(rest) = line[digits..].strip_prefix(['.', ')'])
    {
        return rest;
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use whatlang::Lang;

    const DE: &str = "Die Lieferung wurde am Montag vom Kurier abgeholt und im Lager gezählt.";
    const EN: &str = "The courier collected forty-one crates in the first shipment yesterday.";
    const UK: &str = "Кур'єр забрав сорок одну коробку з першою партією вчора ввечері.";

    #[test]
    fn a_third_language_is_found_and_named_in_the_prompt() {
        // German, which no line of this crate names: the language comes from
        // the samples alone.
        let langs = library_languages(&[DE, DE, DE, EN]);
        assert_eq!(langs, vec![Lang::Deu, Lang::Eng]);
        let prompt = &translation_messages("Wer hat die Kisten abgeholt?", &langs)[0].content;
        assert!(
            prompt.contains("German") && prompt.contains("English"),
            "{prompt}"
        );
        assert!(
            !prompt.contains("Ukrainian"),
            "a language not in the samples: {prompt}"
        );
    }

    #[test]
    fn a_rare_language_below_the_share_is_not_the_librarys() {
        let mut samples = vec![EN; 9];
        samples.push(UK);
        assert_eq!(library_languages(&samples), vec![Lang::Eng]);
        // At the share it counts: 1 of 5 is exactly MIN_SHARE.
        assert_eq!(
            library_languages(&[EN, EN, EN, EN, UK]),
            vec![Lang::Eng, Lang::Ukr]
        );
    }

    #[test]
    fn at_most_two_languages_most_common_first() {
        // Three languages, each above the share: the third is cut, and of the
        // two tied ones the one met first stays.
        let langs = library_languages(&[UK, DE, DE, UK, EN, EN, EN]);
        assert_eq!(langs, vec![Lang::Eng, Lang::Ukr]);
    }

    #[test]
    fn unreliable_samples_give_no_languages() {
        assert_eq!(library_languages(&["ok", "12 34", ""]), Vec::<Lang>::new());
        assert_eq!(library_languages(&[]), Vec::<Lang>::new());
    }

    #[test]
    fn translation_is_needed_only_where_it_can_find_something() {
        assert!(!needs_translation(EN, &[]));
        assert!(!needs_translation(EN, &[Lang::Eng]));
        assert!(needs_translation(UK, &[Lang::Eng]));
        assert!(needs_translation(EN, &[Lang::Eng, Lang::Ukr]));
        // Too short to tell: the cheap mistake is one extra call, not a missed
        // document.
        assert!(needs_translation("Солпадеїн?", &[Lang::Ukr]));
    }

    #[test]
    fn the_translation_call_is_offered_the_question_unchanged() {
        let m = translation_messages("Що таке Лантерна?", &[Lang::Eng]);
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].content, "Що таке Лантерна?");
        assert!(matches!(m[1].role, MessageRole::User));
    }

    #[test]
    fn terms_lose_markers_quotes_and_headings() {
        let raw = "Search terms:\n- «Solpadeine»\n2) \"paracetamol\"\n• codeine";
        assert_eq!(
            parse_terms(raw, "", 100),
            vec!["Solpadeine", "paracetamol", "codeine"]
        );
        // A line that only looks like a marker is kept whole.
        assert_eq!(parse_terms("2024 report", "", 100), vec!["2024 report"]);
        assert_eq!(parse_terms(".NET", "", 100), vec![".NET"]);
    }

    #[test]
    fn a_word_mixing_scripts_is_not_a_term_and_a_line_of_two_scripts_is_split() {
        // E2B's reply for «солпадеїн», verbatim from the owner's ask log.
        assert_eq!(
            parse_terms("солpadein\npainkiller\nanalgesic", "", 100),
            vec!["painkiller", "analgesic"]
        );
        // Two words, each in one script, become two terms: searched as one,
        // a library with both spellings in different chunks finds neither.
        // Accented Latin is still Latin, and one script stays one term.
        assert_eq!(
            parse_terms("Solpadeine (Солпадеїн)\ncafé crème", "", 100),
            vec!["Solpadeine", "Солпадеїн", "café crème"]
        );
        assert_eq!(
            parse_terms("Solpadeine 500 мг таблетки", "", 100),
            vec!["Solpadeine 500", "мг таблетки"]
        );
    }

    #[test]
    fn a_term_that_is_the_question_itself_is_dropped() {
        // luna's reply for «солпадеїн»: the same word back, which searches
        // nothing the question did not.
        assert_eq!(
            parse_terms("Солпадеїн", "солпадеїн?", 100),
            Vec::<String>::new()
        );
        assert_eq!(
            parse_terms("Солпадеїн\nSolpadeine", "солпадеїн", 100),
            vec!["Solpadeine"]
        );
        // A term that only contains the question is a different search.
        assert_eq!(
            parse_terms("Солпадеїн таблетки", "солпадеїн", 100),
            vec!["Солпадеїн таблетки"]
        );
    }

    #[test]
    fn the_prompt_asks_for_a_term_in_every_language() {
        let m = translation_messages("солпадеїн", &[Lang::Eng, Lang::Ukr]);
        assert!(
            m[0].content.contains("a term in every language"),
            "{}",
            m[0].content
        );
    }

    #[test]
    fn terms_drop_blanks_repeats_and_overlong_and_stop_at_three() {
        let raw = "\n  \nSolpadeine\nsolpadeine \n- \nfour\nfive\nsix";
        assert_eq!(
            parse_terms(raw, "", 100),
            vec!["Solpadeine", "four", "five"]
        );
        assert_eq!(parse_terms("abcdef\nabc", "", 5), vec!["abc"]);
        assert_eq!(parse_terms("abcde", "", 5), vec!["abcde"]);
    }
}

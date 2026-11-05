//! Whether an answer stands on the passages it cites, checked by the
//! application rather than taken on the model's word.
//!
//! Deliberately lenient: one shared word between a sentence and a passage it
//! cites is support. The verdict marks an answer, it never replaces one, so
//! the costly mistake is a false alarm on a correct paraphrase, not a missed
//! weak citation. No language is named here: a sentence is checked only when
//! it and its passage are reliably in one language, whichever that is.

use crate::anchors::{extract_anchor_ids, strip_anchors};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grounding {
    /// The answer cites nothing.
    NoCitations,
    /// These anchors sit on sentences that share no word with the passage
    /// they cite.
    Unsupported { anchors: Vec<usize> },
    /// Every sentence that could be checked shares a word with what it cites.
    Supported,
    /// Citations are there, and no sentence could be checked: languages
    /// differ or cannot be told.
    Unchecked,
}

/// A word of at least this many characters counts; shorter ones are mostly
/// function words in any language, and naming them per language is what this
/// file refuses to do.
const MIN_WORD_CHARS: usize = 4;
/// Words are compared by this many leading characters: a crude stem that lets
/// an inflected form meet its base in languages that inflect, without a stemmer
/// per language.
const STEM_CHARS: usize = 5;

/// `answer` is the anchor-resolved text (`<c>N</c>`, 1-based); `passage(n)`
/// gives the text of passage `n`, `None` for one that does not exist.
pub fn grounding<'a>(answer: &str, passage: impl Fn(usize) -> Option<&'a str>) -> Grounding {
    if extract_anchor_ids(answer).is_empty() {
        return Grounding::NoCitations;
    }
    // The whole answer's language, not each sentence's: one answer is written
    // in one language, and more text is a better guess.
    let Some(language) = language(&strip_anchors(answer)) else {
        return Grounding::Unchecked;
    };
    let mut checked = false;
    let mut unsupported: Vec<usize> = Vec::new();
    for (sentence, anchors) in cited_sentences(answer) {
        for n in anchors {
            let Some(text) = passage(n) else { continue };
            if self::language(text) != Some(language) {
                continue;
            }
            checked = true;
            let ours = stems(&sentence);
            if !stems(text).iter().any(|s| ours.contains(s)) && !unsupported.contains(&n) {
                unsupported.push(n);
            }
        }
    }
    match (checked, unsupported.is_empty()) {
        (false, _) => Grounding::Unchecked,
        (true, true) => Grounding::Supported,
        (true, false) => Grounding::Unsupported {
            anchors: unsupported,
        },
    }
}

/// Sentences with the anchors that cite them. An anchor written after the
/// full stop (`… found. <c>2</c>`) belongs to the sentence before it.
fn cited_sentences(answer: &str) -> Vec<(String, Vec<usize>)> {
    let mut out: Vec<(String, Vec<usize>)> = Vec::new();
    for segment in split_sentences(answer) {
        let trimmed = segment.trim_start();
        let lead_len = leading_anchors_len(trimmed);
        let (lead, rest) = trimmed.split_at(lead_len);
        if let Some(last) = out.last_mut() {
            last.1.extend(extract_anchor_ids(lead));
        }
        let text = strip_anchors(rest);
        let anchors = extract_anchor_ids(rest);
        if !text.trim().is_empty() || !anchors.is_empty() {
            out.push((text, anchors));
        }
    }
    out
}

fn split_sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev_end = false;
    for (i, c) in text.char_indices() {
        if c == '\n' || (prev_end && c.is_whitespace()) {
            out.push(&text[start..i]);
            start = i;
        }
        prev_end = matches!(c, '.' | '!' | '?' | '…');
    }
    out.push(&text[start..]);
    out
}

/// Bytes of `<c>N</c>` anchors (and spaces between them) at the start.
fn leading_anchors_len(s: &str) -> usize {
    let mut len = 0;
    loop {
        let rest = &s[len..];
        let spaces = rest.len() - rest.trim_start().len();
        let rest = &rest[spaces..];
        let Some(after) = rest.strip_prefix("<c>") else {
            return len;
        };
        let Some(close) = after.find("</c>") else {
            return len;
        };
        len += spaces + 3 + close + 4;
    }
}

/// whatlang's best guess, with its script. Not filtered by `is_reliable`:
/// that flag stays false on most one-sentence answers and on short passages,
/// which would leave nearly everything unchecked.
/// shortcut: a best guess can call two languages of one script the same and
/// raise a false alarm; task 6 of the plan measures the false-alarm rate, and
/// a reliability floor comes back if it is high.
fn language(text: &str) -> Option<(whatlang::Script, whatlang::Lang)> {
    whatlang::detect(text).map(|i| (i.script(), i.lang()))
}

fn stems(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= MIN_WORD_CHARS || w.chars().any(|c| c.is_ascii_digit()))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase().chars().take(STEM_CHARS).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const P1: &str =
        "The readiness probe waits ninety seconds before it reports a fresh node as healthy.";
    const P2: &str =
        "Draining a node takes sixty seconds, and the service will not accept a shutdown.";
    const UK: &str =
        "Перевірка готовності чекає дев'яносто секунд, перш ніж визнати вузол справним.";

    fn passages(n: usize) -> Option<&'static str> {
        [P1, P2].get(n.wrapping_sub(1)).copied()
    }

    #[test]
    fn an_answer_citing_nothing_says_so() {
        assert_eq!(
            grounding("The probe waits ninety seconds before reporting.", passages),
            Grounding::NoCitations
        );
    }

    #[test]
    fn a_sentence_sharing_a_word_with_its_passage_is_supported() {
        let a =
            "The readiness probe waits ninety seconds before a node counts as healthy <c>1</c>.";
        assert_eq!(grounding(a, passages), Grounding::Supported);
    }

    #[test]
    fn a_sentence_sharing_nothing_with_what_it_cites_is_unsupported() {
        // Sentence about the probe, citing the draining passage.
        let a = "Inventory audits happen every quarter under strict supervision <c>2</c>.";
        assert_eq!(
            grounding(a, passages),
            Grounding::Unsupported { anchors: vec![2] }
        );
    }

    #[test]
    fn only_the_unsupported_anchor_is_named() {
        let a = "The readiness probe waits ninety seconds before declaring health <c>1</c>. \
                 Inventory audits happen every quarter under strict supervision <c>2</c>. \
                 Gardens bloom quietly beside the river every spring <c>2</c>.";
        assert_eq!(
            grounding(a, passages),
            Grounding::Unsupported { anchors: vec![2] }
        );
    }

    #[test]
    fn an_anchor_after_the_full_stop_belongs_to_the_sentence_before() {
        let a = "Inventory audits happen every quarter under strict supervision. <c>2</c> \
                 The readiness probe waits ninety seconds before declaring health. <c>1</c>";
        assert_eq!(
            grounding(a, passages),
            Grounding::Unsupported { anchors: vec![2] }
        );
    }

    #[test]
    fn a_sentence_in_another_language_than_its_passage_is_not_checked() {
        // Ukrainian answer citing an English passage: no shared word is
        // possible, so a verdict would be a false alarm.
        let a = format!("{UK} <c>1</c>");
        assert_eq!(grounding(&a, passages), Grounding::Unchecked);
        // The same passage in the answer's language is checked.
        let uk = |n: usize| (n == 1).then_some(UK);
        assert_eq!(grounding(&a, uk), Grounding::Supported);
    }

    #[test]
    fn an_inflected_form_meets_its_base() {
        // No word is shared whole: «примірників»/«примірниках» and
        // «договору»/«Договір» meet only through their common start.
        let p = |_: usize| Some("Договір складено у двох примірниках для кожної сторони.");
        let a = "Примірників договору виготовили лише пару, кожен окремо <c>1</c>.";
        assert_eq!(grounding(a, p), Grounding::Supported);
    }

    #[test]
    fn an_anchor_to_no_passage_is_not_a_verdict() {
        let a = "The readiness probe waits ninety seconds before declaring health <c>9</c>.";
        assert_eq!(grounding(a, passages), Grounding::Unchecked);
    }

    #[test]
    fn short_words_do_not_count_as_support_but_numbers_do() {
        let p = |_: usize| Some("The fee is 420 per year and it is due in May.");
        // Shares only "the", "is", "per": no support.
        let a = "The cat is fast and the dog is slow per the book <c>1</c>.";
        assert_eq!(grounding(a, p), Grounding::Unsupported { anchors: vec![1] });
        let a = "Somebody must hand over 420 every twelve months without exception <c>1</c>.";
        assert_eq!(grounding(a, p), Grounding::Supported);
    }
}

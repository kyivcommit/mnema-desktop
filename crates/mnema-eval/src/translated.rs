use std::collections::BTreeMap;
use std::time::Duration;

use mnema_index::QueryRule;
use mnema_search::{CANDIDATES, ContentArm, FusionRule, Provider, fuse, rrf_lists};

use crate::{
    Corpus, DenseAnswers, EvalError, IndexedCorpus, Outcome, QuestionSet, SEARCH_LIMIT,
    run_row_scored,
};

/// The longest term a translator may hand back, as the application holds a
/// typed question (`MAX_ASK_QUERY`, `src-tauri/src/bridge.rs`), copied for the
/// same reason `SEARCH_LIMIT` is.
const MAX_TERM_CHARS: usize = 2048;

/// One translated term and what the content arm found for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    pub text: String,
    pub content: Vec<i64>,
}

/// How a question's own search and its terms' searches become one list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Merge {
    /// Every search weighted alike.
    Equal,
    /// The question's own search against all its terms together, half and
    /// half: three terms cannot outvote the question's own words.
    OriginalHalf,
    /// As `OriginalHalf`, but each term is searched by the text arm alone:
    /// the question's own content search already carries its meaning, and a
    /// term's content search adds another thirty candidates of noise that
    /// RRF sums across lists (D191's lesson, spec §5.1).
    LexicalTerms,
}

impl Merge {
    pub const ALL: [Merge; 3] = [Merge::Equal, Merge::OriginalHalf, Merge::LexicalTerms];

    pub fn label(self) -> &'static str {
        match self {
            Merge::Equal => "кожен пошук нарівно",
            Merge::OriginalHalf => "питання проти термінів 1:1",
            Merge::LexicalTerms => "терміни лише текстом, 1:1",
        }
    }
}

/// What the application's translation step gave every question, taken once
/// per chat model — the same primitives the product calls
/// (`mnema_rag::{library_languages, needs_translation, translation_messages,
/// parse_terms}`), so this measures them rather than a copy.
#[derive(Debug)]
pub struct Translated {
    by_question: BTreeMap<String, Vec<Term>>,
    pub model: String,
    /// The library's languages as detected, by English name.
    pub languages: Vec<String>,
    /// Questions whose translation call failed and were searched by their
    /// own words alone, as the application does.
    pub failed: usize,
}

impl Translated {
    pub fn ask(
        corpus: &Corpus,
        indexed: &IndexedCorpus,
        questions: &QuestionSet,
        embed: &Provider,
        chat: &Provider,
        model: &str,
        timeout: Duration,
    ) -> Result<Translated, EvalError> {
        // shortcut: whole documents as the samples, where the application
        // samples chunks; the corpus is twenty short documents, so the two
        // agree here — revisit if the corpus grows mixed-language documents.
        let samples: Vec<&str> = corpus.documents.iter().map(|d| d.text.as_str()).collect();
        let languages = mnema_rag::library_languages(&samples);
        let mut by_question = BTreeMap::new();
        let mut failed = 0;
        for q in &questions.questions {
            if !mnema_rag::needs_translation(&q.text, &languages) {
                continue;
            }
            let messages = mnema_rag::translation_messages(&q.text, &languages);
            let raw = match mnema_provider::complete_within(
                &chat.base, &chat.key, model, &messages, timeout,
            ) {
                Ok(raw) => raw,
                Err(_) => {
                    failed += 1;
                    continue;
                }
            };
            let mut terms = Vec::new();
            for text in mnema_rag::parse_terms(&raw, MAX_TERM_CHARS) {
                match mnema_search::content_arm(
                    indexed.db(),
                    Some(embed.clone()),
                    &text,
                    CANDIDATES,
                ) {
                    ContentArm::Answered { chunks, .. } => terms.push(Term {
                        text,
                        content: chunks,
                    }),
                    // As `DenseAnswers::ask`: a silent content arm ends the
                    // run rather than leaving one term out of one row.
                    other => return Err(EvalError::ContentArmSilent(format!("{other:?}"))),
                }
            }
            by_question.insert(q.id.clone(), terms);
        }
        Ok(Translated {
            by_question,
            model: model.to_string(),
            languages: languages.iter().map(|l| l.eng_name().to_string()).collect(),
            failed,
        })
    }

    /// Built from data, for tests.
    pub fn canned(by_question: BTreeMap<String, Vec<Term>>) -> Translated {
        Translated {
            by_question,
            model: "none".to_string(),
            languages: Vec::new(),
            failed: 0,
        }
    }

    pub fn of(&self, question_id: &str) -> &[Term] {
        self.by_question.get(question_id).map_or(&[], |v| v)
    }
}

/// The product's row (`all-terms / rrf`) with translation: the question's own
/// two-arm search, and one two-arm search per term, merged by `merge`. A
/// question with no terms scores exactly as `run_row` scores it. Pinned by
/// `a_question_without_terms_scores_as_the_row_without_translation` and
/// `a_term_that_finds_the_gold_chunk_lifts_it`.
pub fn run_translated(
    indexed: &IndexedCorpus,
    questions: &QuestionSet,
    dense: &DenseAnswers,
    translated: &Translated,
    merge: Merge,
) -> Result<Vec<Outcome>, EvalError> {
    let db = indexed.db();
    let two_arms = |text: &str, content: &[i64]| -> Result<Vec<i64>, EvalError> {
        let lexical = db.search_lexical_with(text, QueryRule::AllTerms, CANDIDATES)?;
        Ok(fuse(
            FusionRule::Rrf,
            &lexical,
            content,
            CANDIDATES as usize,
        ))
    };
    run_row_scored(indexed, questions, |q| {
        let own = two_arms(&q.text, dense.of(&q.id))?;
        let mut terms: Vec<Vec<i64>> = Vec::new();
        for term in translated.of(&q.id) {
            terms.push(match merge {
                Merge::LexicalTerms => {
                    db.search_lexical_with(&term.text, QueryRule::AllTerms, CANDIDATES)?
                }
                Merge::Equal | Merge::OriginalHalf => two_arms(&term.text, &term.content)?,
            });
        }
        Ok(merged(merge, own, &terms, SEARCH_LIMIT as usize))
    })
}

/// One question's own list and its terms' lists as one, cut to `limit`. With
/// no terms both rules give `own` back in its order: RRF of one list is that
/// list.
fn merged(merge: Merge, own: Vec<i64>, terms: &[Vec<i64>], limit: usize) -> Vec<i64> {
    let all = match merge {
        Merge::Equal => {
            let mut all: Vec<&[i64]> = vec![&own];
            all.extend(terms.iter().map(Vec::as_slice));
            rrf_lists(&all)
        }
        Merge::OriginalHalf | Merge::LexicalTerms => {
            let refs: Vec<&[i64]> = terms.iter().map(Vec::as_slice).collect();
            fuse(FusionRule::Rrf, &own, &rrf_lists(&refs), usize::MAX)
        }
    };
    all.into_iter().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_terms_outvote_the_question_only_when_weighted_alike() {
        let terms = vec![vec![3], vec![3], vec![3]];
        assert_eq!(merged(Merge::Equal, vec![1, 2], &terms, 9), vec![3, 1, 2]);
        // Half and half, and fused by rank, not taken in turns: 2 stands in
        // the question's list and in a term's, so it leads.
        let terms = vec![vec![2], vec![3]];
        assert_eq!(
            merged(Merge::OriginalHalf, vec![1, 2], &terms, 9),
            vec![2, 1, 3]
        );
    }

    #[test]
    fn no_terms_leave_the_question_list_as_it_was() {
        for merge in Merge::ALL {
            assert_eq!(merged(merge, vec![5, 4, 9], &[], 9), vec![5, 4, 9]);
            assert_eq!(merged(merge, vec![5, 4, 9], &[vec![7]], 2).len(), 2);
        }
    }
}

use std::collections::{BTreeMap, BTreeSet};

use mnema_index::search_terms;

use crate::{Class, Corpus, Language, Question};

/// Terms that stand in every document of their language.
///
/// Derived from the corpus, not chosen: a term in all of them discriminates
/// nothing by construction.
pub fn universal_terms(corpus: &Corpus) -> BTreeMap<Language, BTreeSet<String>> {
    let mut per_language: BTreeMap<Language, Vec<BTreeSet<String>>> = BTreeMap::new();
    for document in &corpus.documents {
        let terms: BTreeSet<String> = search_terms(&document.text).into_iter().collect();
        per_language
            .entry(document.language)
            .or_default()
            .push(terms);
    }
    per_language
        .into_iter()
        .map(|(language, sets)| {
            let mut iter = sets.into_iter();
            let first = iter.next().unwrap_or_default();
            let common = iter.fold(first, |acc, next| {
                acc.intersection(&next).cloned().collect()
            });
            (language, common)
        })
        .collect()
}

/// Whether a question's declared class survives the tokenizer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassVerdict {
    Holds,
    /// `shared` is sorted, so the message does not move between runs.
    /// Pinned by `the_shared_list_comes_back_in_one_fixed_order`.
    Violated {
        shared: Vec<String>,
    },
}

pub fn check_class(q: &Question, universal: &BTreeSet<String>) -> ClassVerdict {
    let question_terms: BTreeSet<String> = search_terms(&q.text)
        .into_iter()
        .filter(|t| !universal.contains(t))
        .collect();
    let mut shared: BTreeSet<String> = BTreeSet::new();
    for answer in &q.answers {
        for term in search_terms(answer) {
            if !universal.contains(&term) && question_terms.contains(&term) {
                shared.insert(term);
            }
        }
    }
    let shared: Vec<String> = shared.into_iter().collect();
    let holds = match q.class {
        Class::Literal => !shared.is_empty(),
        Class::Paraphrase | Class::Topical => shared.is_empty(),
    };
    if holds {
        ClassVerdict::Holds
    } else {
        ClassVerdict::Violated { shared }
    }
}

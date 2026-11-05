use std::collections::BTreeMap;

use mnema_eval::{
    Class, Corpus, DenseAnswers, Document, IndexedCorpus, Language, Merge, Question, QuestionSet,
    Term, Translated, run_row, run_translated,
};
use mnema_index::QueryRule;
use mnema_search::FusionRule;

mod support;

const ANSWER: &str = "Договір складено у двох примірниках.";

fn corpus() -> Corpus {
    Corpus {
        documents: vec![
            Document {
                id: "uk/one.md".to_string(),
                language: Language::Uk,
                text: format!("{ANSWER} Кожен має однакову силу."),
            },
            Document {
                id: "uk/two.md".to_string(),
                language: Language::Uk,
                text: "Комісія відклала розгляд заяви до наступного засідання.".to_string(),
            },
        ],
    }
}

fn questions(text: &str) -> QuestionSet {
    QuestionSet {
        questions: vec![Question {
            id: "q-1".to_string(),
            language: Language::En,
            class: Class::CrossLingual,
            text: text.to_string(),
            document: "uk/one.md".to_string(),
            answers: vec![ANSWER.to_string()],
        }],
    }
}

fn translated(terms: &[&str]) -> Translated {
    let terms = terms
        .iter()
        .map(|t| Term {
            text: t.to_string(),
            content: Vec::new(),
        })
        .collect();
    Translated::canned(BTreeMap::from([("q-1".to_string(), terms)]))
}

#[test]
fn a_question_without_terms_scores_as_the_row_without_translation() {
    let indexed = IndexedCorpus::build(&corpus(), support::worker()).unwrap();
    // The question's own words find the gold chunk, so the comparison is
    // between two non-empty lists, not two empty ones.
    let qs = questions("договір");
    let dense = DenseAnswers::empty();
    let plain = run_row(&indexed, &qs, QueryRule::AllTerms, FusionRule::Rrf, &dense).unwrap();
    assert_eq!(plain[0].rank, Some(1));
    for merge in Merge::ALL {
        let with = run_translated(&indexed, &qs, &dense, &translated(&[]), merge).unwrap();
        assert_eq!(with[0].returned, plain[0].returned, "{merge:?}");
        assert_eq!(with[0].rank, plain[0].rank, "{merge:?}");
    }
}

#[test]
fn a_term_that_finds_the_gold_chunk_lifts_it() {
    let indexed = IndexedCorpus::build(&corpus(), support::worker()).unwrap();
    // The English question matches nothing in a Ukrainian corpus; the term
    // does.
    let qs = questions("contract copies");
    let dense = DenseAnswers::empty();
    let none = run_translated(&indexed, &qs, &dense, &translated(&[]), Merge::Equal).unwrap();
    assert_eq!(none[0].rank, None);
    for merge in Merge::ALL {
        let with = run_translated(&indexed, &qs, &dense, &translated(&["договір"]), merge).unwrap();
        assert_eq!(with[0].rank, Some(1), "{merge:?}");
    }
}

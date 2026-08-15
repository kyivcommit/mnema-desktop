use mnema_eval::{
    Class, Corpus, Document, IndexedCorpus, Language, Question, QuestionSet, Report, run_lexical,
};

mod support;

fn corpus() -> Corpus {
    Corpus {
        documents: vec![
            Document {
                id: "uk/one.md".to_string(),
                language: Language::Uk,
                text: "Договір складено у двох примірниках. Кожен має однакову силу.".to_string(),
            },
            Document {
                id: "uk/two.md".to_string(),
                language: Language::Uk,
                text: "Комісія відклала розгляд заяви до наступного засідання.".to_string(),
            },
        ],
    }
}

fn question(id: &str, text: &str, document: &str, answer: &str) -> Question {
    Question {
        id: id.to_string(),
        language: Language::Uk,
        class: Class::Literal,
        text: text.to_string(),
        document: document.to_string(),
        answers: vec![answer.to_string()],
    }
}

/// Long enough to become more than one chunk: the chunker aims at
/// `TARGET_CHARS` = 900 (`crates/mnema-chunk/src/lib.rs:34`), and the filler
/// alone is past twice that. The two marker sentences sit at the very start
/// and the very end, where the 15% overlap cannot copy them into a second
/// chunk — `resolve_gold` would answer `Several` and the run would refuse.
fn two_chunk_document() -> String {
    let filler =
        "Сторони погодили загальні умови співпраці та порядок обміну документами. ".repeat(30);
    format!(
        "Перше речення називає ратифікацію угоди.\n\n{filler}\n\n\
         Останнє речення називає депозитарій конвенції."
    )
}

fn long_corpus() -> Corpus {
    Corpus {
        documents: vec![Document {
            id: "uk/long.md".to_string(),
            language: Language::Uk,
            text: two_chunk_document(),
        }],
    }
}

#[test]
fn an_outcome_carries_the_class_of_its_question() {
    // Every other fixture here is `Literal`, so a field hardwired to `Literal`
    // would look right in all of them. Spec §7 groups the report's table by
    // this field: wrong class, right count on the wrong row.
    let indexed = IndexedCorpus::build(&corpus(), support::worker()).unwrap();
    let mut q = question(
        "q-1",
        "договір",
        "uk/one.md",
        "Договір складено у двох примірниках.",
    );
    q.class = Class::Paraphrase;
    let outcomes = run_lexical(&indexed, &QuestionSet { questions: vec![q] }).unwrap();
    assert_eq!(outcomes[0].class, Class::Paraphrase);
}

#[test]
fn a_rank_is_the_best_placed_gold_chunk_not_the_first_answers() {
    // Two answer sentences in two different chunks, and a query word that only
    // the SECOND one's chunk holds. An implementation that ranked `gold[0]` —
    // the first answer's chunk — would answer `None` here.
    let indexed = IndexedCorpus::build(&long_corpus(), support::worker()).unwrap();
    let questions = QuestionSet {
        questions: vec![Question {
            id: "q-1".to_string(),
            language: Language::Uk,
            class: Class::Topical,
            text: "депозитарій".to_string(),
            document: "uk/long.md".to_string(),
            answers: vec![
                "Перше речення називає ратифікацію угоди.".to_string(),
                "Останнє речення називає депозитарій конвенції.".to_string(),
            ],
        }],
    };
    let outcome = run_lexical(&indexed, &questions).unwrap().remove(0);
    // Without this the fixture could be one chunk, both answers could name it,
    // and `gold[0]` would rank first by accident.
    assert_ne!(
        outcome.gold[0], outcome.gold[1],
        "the document must have split: {outcome:?}"
    );
    assert_eq!(
        outcome.returned,
        vec![outcome.gold[1]],
        "only the second answer's chunk may come back: {outcome:?}"
    );
    assert_eq!(outcome.rank, Some(1), "outcome: {outcome:?}");
}

#[test]
fn a_question_whose_words_are_all_in_the_gold_chunk_finds_it_first() {
    let indexed = IndexedCorpus::build(&corpus(), support::worker()).unwrap();
    let questions = QuestionSet {
        questions: vec![question(
            "q-1",
            "договір примірниках",
            "uk/one.md",
            "Договір складено у двох примірниках.",
        )],
    };
    let outcomes = run_lexical(&indexed, &questions).unwrap();
    assert_eq!(outcomes[0].rank, Some(1), "outcome: {:?}", outcomes[0]);
    assert!(!outcomes[0].gold.is_empty(), "the gold chunk must be named");
}

#[test]
fn a_question_no_chunk_answers_has_no_rank_and_says_what_came_back() {
    // Implicit AND: `неможливе` is in no chunk, so the whole query matches
    // nothing. `rank` is None and `returned` is empty — and both are asserted,
    // because a run that returned everything would also have no rank.
    let indexed = IndexedCorpus::build(&corpus(), support::worker()).unwrap();
    let questions = QuestionSet {
        questions: vec![question(
            "q-1",
            "договір неможливе",
            "uk/one.md",
            "Договір складено у двох примірниках.",
        )],
    };
    let outcomes = run_lexical(&indexed, &questions).unwrap();
    assert_eq!(outcomes[0].rank, None);
    assert_eq!(outcomes[0].returned, Vec::<i64>::new());
}

#[test]
fn a_chunk_that_is_returned_but_is_not_gold_does_not_count() {
    // `комісія` finds the near-miss document and not the gold one. A run that
    // scored any hit would report a rank here.
    let indexed = IndexedCorpus::build(&corpus(), support::worker()).unwrap();
    let questions = QuestionSet {
        questions: vec![question(
            "q-1",
            "комісія",
            "uk/one.md",
            "Договір складено у двох примірниках.",
        )],
    };
    let outcomes = run_lexical(&indexed, &questions).unwrap();
    assert_eq!(outcomes[0].rank, None, "outcome: {:?}", outcomes[0]);
    assert!(
        !outcomes[0].returned.is_empty(),
        "search did return something, and the outcome must record it"
    );
}

#[test]
fn a_returned_chunk_is_reported_with_its_path_and_first_line() {
    // The same near miss as above, carried all the way to the rendered report:
    // the near-miss document comes back and the gold one does not, and the
    // report has to say WHICH document that was. Ids are reassigned every run
    // and the index goes with the process, so a bare number diagnoses nothing
    // afterwards — spec §7 asks for the path and the first lines for that
    // reason. Both directions: the path AND the text.
    let indexed = IndexedCorpus::build(&corpus(), support::worker()).unwrap();
    let questions = QuestionSet {
        questions: vec![question(
            "q-1",
            "комісія",
            "uk/one.md",
            "Договір складено у двох примірниках.",
        )],
    };
    let outcomes = run_lexical(&indexed, &questions).unwrap();
    assert_eq!(outcomes[0].rank, None, "outcome: {:?}", outcomes[0]);
    let chunk_count = indexed.db().chunk_count().unwrap();
    let text = Report::of(&outcomes, chunk_count).render();
    assert!(
        text.contains("uk/two.md"),
        "the returned chunk's document is not named:\n{text}"
    );
    assert!(
        text.contains("Комісія відклала"),
        "the returned chunk's text is not shown:\n{text}"
    );
}

// The two tests below pin a decision the brief left open: a question whose
// gold does not resolve is refused, not skipped and not scored. Skipping would
// return a shorter list than the question set, and every `recall@k` read off it
// would divide by the smaller denominator and report a better number than the
// run earned. Scoring it would charge search for a defect in the fixture.
// Preflight (task 9) refuses these before anything is scored; reaching one here
// means it was not run.

#[test]
fn a_question_whose_answer_is_in_no_chunk_is_refused_not_scored() {
    let indexed = IndexedCorpus::build(&corpus(), support::worker()).unwrap();
    let questions = QuestionSet {
        questions: vec![question(
            "q-1",
            "договір",
            "uk/one.md",
            "Договір складено у трьох примірниках.",
        )],
    };
    let err = run_lexical(&indexed, &questions).unwrap_err().to_string();
    assert!(
        err.contains("q-1"),
        "the error must name the question: {err}"
    );
    assert!(
        err.contains("трьох"),
        "the error must name the sentence that did not resolve: {err}"
    );
}

#[test]
fn a_question_naming_a_document_that_is_not_there_is_refused_not_scored() {
    let indexed = IndexedCorpus::build(&corpus(), support::worker()).unwrap();
    let questions = QuestionSet {
        questions: vec![question(
            "q-1",
            "договір",
            "uk/nowhere.md",
            "Договір складено у двох примірниках.",
        )],
    };
    let err = run_lexical(&indexed, &questions).unwrap_err().to_string();
    assert!(
        err.contains("q-1"),
        "the error must name the question: {err}"
    );
    assert!(
        err.contains("uk/nowhere.md"),
        "the error must name the document: {err}"
    );
}

#[test]
fn every_question_produces_exactly_one_outcome_in_order() {
    let indexed = IndexedCorpus::build(&corpus(), support::worker()).unwrap();
    let questions = QuestionSet {
        questions: vec![
            question(
                "q-1",
                "договір",
                "uk/one.md",
                "Договір складено у двох примірниках.",
            ),
            question(
                "q-2",
                "комісія",
                "uk/two.md",
                "Комісія відклала розгляд заяви до наступного засідання.",
            ),
        ],
    };
    let outcomes = run_lexical(&indexed, &questions).unwrap();
    let ids: Vec<&str> = outcomes.iter().map(|o| o.question.as_str()).collect();
    assert_eq!(ids, vec!["q-1", "q-2"]);
}

/// The rule the harness sweeps is a parameter of the run, and the old entry
/// point is one of its values. The equality matters because every existing
/// assertion in this file was written against `run_lexical`.
#[test]
fn the_unparameterised_run_is_the_all_terms_rule() {
    let (corpus, questions, indexed) = support::small_fixture();
    assert_eq!(
        mnema_eval::run_lexical(&indexed, &questions).unwrap(),
        mnema_eval::run_lexical_with(&indexed, &questions, mnema_index::QueryRule::AllTerms)
            .unwrap()
    );
    let _ = corpus;
}

/// A rule that demands less finds more on this corpus, and the volume column is
/// what shows the price. Both halves asserted: a rule that returned everything
/// would satisfy the first alone.
#[test]
fn a_looser_rule_returns_more_and_the_outcome_records_how_much() {
    let (_c, questions, indexed) = support::small_fixture();
    let strict =
        mnema_eval::run_lexical_with(&indexed, &questions, mnema_index::QueryRule::AllTerms)
            .unwrap();
    let loose = mnema_eval::run_lexical_with(&indexed, &questions, mnema_index::QueryRule::AnyTerm)
        .unwrap();

    let strict_total: usize = strict.iter().filter_map(|o| o.text_matched).sum();
    let loose_total: usize = loose.iter().filter_map(|o| o.text_matched).sum();
    assert!(
        loose_total > strict_total,
        "{loose_total} vs {strict_total}"
    );
    assert!(strict.iter().all(|o| o.content_matched.is_none()));
}

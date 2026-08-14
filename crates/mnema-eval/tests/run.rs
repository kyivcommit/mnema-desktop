use mnema_eval::{
    Class, Corpus, Document, IndexedCorpus, Language, Question, QuestionSet, run_lexical,
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

use mnema_eval::{
    Class, Corpus, Document, IndexedCorpus, Language, Problem, Question, QuestionSet, preflight,
};

mod support;

const ANSWER: &str = "Договір складено у двох примірниках.";
const QUESTION: &str = "Скільки примірників складено?";

/// Three documents, not two, and the third is load-bearing.
///
/// `universal_terms` calls a term universal when it stands in EVERY document of
/// its language. With two documents that is a very low bar: the moment a test
/// puts the answer sentence into the near-miss document as well, every word of
/// that sentence becomes universal, `check_class` sees an empty intersection,
/// and a sound literal question is reported as a class violation — a second
/// problem the test never asked for. A third document that shares only
/// «Комісія» holds the universal set down to that one word.
fn corpus() -> Corpus {
    Corpus {
        documents: vec![
            Document {
                id: "uk/one.md".to_string(),
                language: Language::Uk,
                text: format!("Комісія розглянула заяву. {ANSWER} Ухвалено передати справу далі."),
            },
            Document {
                id: "uk/two.md".to_string(),
                language: Language::Uk,
                text: "Комісія відклала розгляд. Секретар повідомив сторони.".to_string(),
            },
            Document {
                id: "uk/three.md".to_string(),
                language: Language::Uk,
                text: "Комісія затвердила порядок денний засідання.".to_string(),
            },
        ],
    }
}

fn question(class: Class, text: &str, document: &str, answers: &[&str]) -> Question {
    Question {
        id: "q-1".to_string(),
        language: Language::Uk,
        class,
        text: text.to_string(),
        document: document.to_string(),
        answers: answers.iter().map(|s| s.to_string()).collect(),
    }
}

fn set(questions: Vec<Question>) -> QuestionSet {
    QuestionSet { questions }
}

fn run(corpus: &Corpus, questions: &QuestionSet) -> Vec<Problem> {
    let indexed = IndexedCorpus::build(corpus, support::worker()).unwrap();
    preflight(corpus, questions, &indexed).unwrap()
}

#[test]
fn a_sound_corpus_and_question_set_have_no_problems() {
    // The all-green case, and it is not decoration: every test below asserts a
    // specific problem, and all of them would pass against a `preflight` that
    // returned one problem for everything.
    let questions = set(vec![question(
        Class::Literal,
        QUESTION,
        "uk/one.md",
        &[ANSWER],
    )]);
    assert_eq!(run(&corpus(), &questions), vec![]);
}

#[test]
fn a_question_naming_a_document_that_is_not_there_is_a_problem() {
    let questions = set(vec![question(
        Class::Literal,
        QUESTION,
        "uk/nowhere.md",
        &[ANSWER],
    )]);
    assert_eq!(
        run(&corpus(), &questions),
        vec![Problem::DocumentMissing {
            question: "q-1".to_string(),
            document: "uk/nowhere.md".to_string(),
        }]
    );
}

#[test]
fn a_sentence_the_document_does_not_hold_is_a_problem() {
    let questions = set(vec![question(
        Class::Literal,
        QUESTION,
        "uk/one.md",
        &["Договір складено у трьох примірниках."],
    )]);
    assert_eq!(
        run(&corpus(), &questions),
        vec![Problem::SentenceNotFound {
            question: "q-1".to_string(),
            sentence: "Договір складено у трьох примірниках.".to_string(),
        }]
    );
}

#[test]
fn a_sentence_a_near_miss_also_holds_is_a_problem() {
    // The near-miss document is supposed to talk about the same subject and
    // NOT hold the answer. Here it does, so the question has two right chunks
    // and the score counts one.
    let mut corpus = corpus();
    corpus.documents[1].text = format!("Комісія відклала розгляд. {ANSWER}");
    let questions = set(vec![question(
        Class::Literal,
        QUESTION,
        "uk/one.md",
        &[ANSWER],
    )]);
    assert_eq!(
        run(&corpus, &questions),
        vec![Problem::SentenceInAnotherDocument {
            question: "q-1".to_string(),
            sentence: ANSWER.to_string(),
            other: "uk/two.md".to_string(),
        }]
    );
}

#[test]
fn a_paraphrase_that_shares_a_content_word_is_a_problem() {
    // The same text as the sound literal question, declared as a paraphrase:
    // the only thing that changed is the claim about it.
    let questions = set(vec![question(
        Class::Paraphrase,
        QUESTION,
        "uk/one.md",
        &[ANSWER],
    )]);
    match run(&corpus(), &questions).as_slice() {
        [Problem::ClassViolated { question, shared }] => {
            assert_eq!(question, "q-1");
            assert_eq!(shared, &vec!["складено".to_string()]);
        }
        other => panic!("expected one ClassViolated, got {other:?}"),
    }
}

#[test]
fn two_answers_of_one_topical_question_in_one_chunk_are_a_problem() {
    // Both sentences sit in the same short document, so one chunk holds both:
    // "at least one in the top-k" then means "one chunk", and the question is
    // measuring less than it claims.
    let questions = set(vec![question(
        Class::Topical,
        "Що вирішили?",
        "uk/one.md",
        &[
            "Комісія розглянула заяву.",
            "Ухвалено передати справу далі.",
        ],
    )]);
    match run(&corpus(), &questions).as_slice() {
        [Problem::AnswersShareAChunk { question, .. }] => assert_eq!(question, "q-1"),
        other => panic!("expected one AnswersShareAChunk, got {other:?}"),
    }
}

#[test]
fn every_problem_names_the_question_it_came_from() {
    // Two broken questions, two problems, each naming its own id — a preflight
    // that reported the first and stopped would pass every test above.
    let mut first = question(Class::Literal, QUESTION, "uk/nowhere.md", &[ANSWER]);
    first.id = "q-1".to_string();
    let mut second = question(Class::Literal, QUESTION, "uk/elsewhere.md", &[ANSWER]);
    second.id = "q-2".to_string();
    let problems = run(&corpus(), &set(vec![first, second]));
    assert_eq!(
        problems,
        vec![
            Problem::DocumentMissing {
                question: "q-1".to_string(),
                document: "uk/nowhere.md".to_string(),
            },
            Problem::DocumentMissing {
                question: "q-2".to_string(),
                document: "uk/elsewhere.md".to_string(),
            },
        ]
    );
}

#[test]
fn a_question_with_a_missing_document_still_has_its_class_checked() {
    // `check_class` reads the question, its answers and terms taken from the
    // in-memory corpus — never a chunk — so a question whose document never
    // arrived still has a computable verdict. Both problems come back. Behind
    // the document guard the class one would be lost, and the author would
    // learn about the second defect only after fixing the first and rerunning.
    let questions = set(vec![question(
        Class::Paraphrase,
        QUESTION,
        "uk/nowhere.md",
        &[ANSWER],
    )]);
    match run(&corpus(), &questions).as_slice() {
        [
            Problem::ClassViolated { question, shared },
            Problem::DocumentMissing { document, .. },
        ] => {
            assert_eq!(question, "q-1");
            assert_eq!(shared, &vec!["складено".to_string()]);
            assert_eq!(document, "uk/nowhere.md");
        }
        other => panic!("expected ClassViolated then DocumentMissing, got {other:?}"),
    }
}

#[test]
fn two_missing_sentences_come_back_in_the_order_the_question_lists_them() {
    // The other half of the order claim, and the one no other test reaches:
    // every question above produces at most one sentence-level problem, so a
    // preflight that gathered them into a set — or walked the answers
    // backwards — would pass all of them.
    let first = "Договір складено у трьох примірниках.";
    let second = "Комісія оголосила перерву до понеділка.";
    let questions = set(vec![question(
        Class::Topical,
        "Що вирішили?",
        "uk/one.md",
        &[first, second],
    )]);
    assert_eq!(
        run(&corpus(), &questions),
        vec![
            Problem::SentenceNotFound {
                question: "q-1".to_string(),
                sentence: first.to_string(),
            },
            Problem::SentenceNotFound {
                question: "q-1".to_string(),
                sentence: second.to_string(),
            },
        ]
    );
}

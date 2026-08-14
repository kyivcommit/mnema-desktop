use std::collections::{BTreeMap, BTreeSet};

use mnema_eval::{
    Class, ClassVerdict, Corpus, Document, Language, Question, check_class, universal_terms,
};

fn doc(id: &str, language: Language, text: &str) -> Document {
    Document {
        id: id.to_string(),
        language,
        text: text.to_string(),
    }
}

fn question(class: Class, text: &str, answers: &[&str]) -> Question {
    Question {
        id: "q-1".to_string(),
        language: Language::Uk,
        class,
        text: text.to_string(),
        document: "uk/one.md".to_string(),
        answers: answers.iter().map(|s| s.to_string()).collect(),
    }
}

fn universal(corpus: &Corpus, language: Language) -> BTreeSet<String> {
    let map: BTreeMap<Language, BTreeSet<String>> = universal_terms(corpus);
    map.get(&language).cloned().unwrap_or_default()
}

#[test]
fn a_term_in_every_document_of_a_language_is_universal() {
    let corpus = Corpus {
        documents: vec![
            doc("uk/one.md", Language::Uk, "договір за угодою"),
            doc("uk/two.md", Language::Uk, "протокол за ухвалою"),
        ],
    };
    let uk = universal(&corpus, Language::Uk);
    assert!(uk.contains("за"), "«за» is in both documents, got {uk:?}");
}

#[test]
fn a_term_missing_from_one_document_is_not_universal() {
    // The other direction, and it is the one a one-sided assertion would miss:
    // a set that simply contained everything would pass the test above.
    let corpus = Corpus {
        documents: vec![
            doc("uk/one.md", Language::Uk, "договір за угодою"),
            doc("uk/two.md", Language::Uk, "протокол за ухвалою"),
        ],
    };
    let uk = universal(&corpus, Language::Uk);
    assert!(
        !uk.contains("договір"),
        "«договір» is in one document only, got {uk:?}"
    );
}

#[test]
fn languages_do_not_pool_their_universal_terms() {
    // `the` is in every English document and in no Ukrainian one. Pooling the
    // two languages would make it non-universal and every English paraphrase
    // would then fail for a reason that is not the question's.
    let corpus = Corpus {
        documents: vec![
            doc("uk/one.md", Language::Uk, "договір за угодою"),
            doc("en/one.md", Language::En, "the contract"),
            doc("en/two.md", Language::En, "the minutes"),
        ],
    };
    assert!(universal(&corpus, Language::En).contains("the"));
    assert!(!universal(&corpus, Language::Uk).contains("the"));
}

#[test]
fn a_literal_question_holds_when_it_shares_a_content_word() {
    // «складено» stands in both, verbatim. `примірників` against
    // `примірниках` would NOT do: the tokenizer does not stem, so those are
    // two terms and this test would assert the opposite of what it says.
    let q = question(
        Class::Literal,
        "Скільки примірників складено?",
        &["Договір складено у двох примірниках."],
    );
    assert_eq!(check_class(&q, &BTreeSet::new()), ClassVerdict::Holds);
}

#[test]
fn a_literal_question_that_shares_nothing_is_violated() {
    let q = question(
        Class::Literal,
        "Що ухвалили?",
        &["Договір складено у двох примірниках."],
    );
    assert_eq!(
        check_class(&q, &BTreeSet::new()),
        ClassVerdict::Violated { shared: vec![] }
    );
}

#[test]
fn a_paraphrase_that_shares_a_content_word_is_violated_and_names_it() {
    let q = question(
        Class::Paraphrase,
        "Скільки копій складено?",
        &["Договір складено у двох примірниках."],
    );
    assert_eq!(
        check_class(&q, &BTreeSet::new()),
        ClassVerdict::Violated {
            shared: vec!["складено".to_string()]
        }
    );
}

#[test]
fn a_universal_term_does_not_violate_a_paraphrase() {
    // «у» stands in both the question and the answer and discriminates
    // nothing; without this rule no paraphrase could ever hold. Every other
    // word of the question is absent from the answer, so «у» is the only
    // thing this test can turn on.
    let q = question(
        Class::Paraphrase,
        "У скількох копіях?",
        &["Договір складено у двох примірниках."],
    );
    let universal: BTreeSet<String> = ["у"].iter().map(|s| s.to_string()).collect();
    assert_eq!(check_class(&q, &universal), ClassVerdict::Holds);
    // And the other direction: without the rule, the same question is a
    // violation. A test that only showed Holds would pass against a
    // `check_class` that always says Holds.
    assert_eq!(
        check_class(&q, &BTreeSet::new()),
        ClassVerdict::Violated {
            shared: vec!["у".to_string()]
        }
    );
}

#[test]
fn all_answer_sentences_of_a_topical_question_are_checked() {
    // The first sentence shares nothing; the second does. Checking only the
    // first would let a topical question through with a literal half.
    let q = question(
        Class::Topical,
        "Скільки примірників?",
        &[
            "Комісія розглянула заяву.",
            "Другий примірників зберігається у заявника.",
        ],
    );
    assert_eq!(
        check_class(&q, &BTreeSet::new()),
        ClassVerdict::Violated {
            shared: vec!["примірників".to_string()]
        }
    );
}

#[test]
fn the_shared_list_comes_back_in_one_fixed_order() {
    // The question lists the two words in the opposite order to the answer, so
    // an implementation that preserved either side's order would come back the
    // other way round and this fails. Re-sorting the result and comparing it
    // with itself is the shape that cannot fail — it is not what this asserts.
    let q = question(
        Class::Paraphrase,
        "примірник договір",
        &["договір примірник"],
    );
    assert_eq!(
        check_class(&q, &BTreeSet::new()),
        ClassVerdict::Violated {
            shared: vec!["договір".to_string(), "примірник".to_string()]
        }
    );
}

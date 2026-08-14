use mnema_eval::{Class, EvalError, Language, QuestionSet};

fn file(lines: &[&str]) -> tempfile::NamedTempFile {
    use std::io::Write as _;
    let mut f = tempfile::NamedTempFile::new().unwrap();
    for line in lines {
        writeln!(f, "{line}").unwrap();
    }
    f.flush().unwrap();
    f
}

const OK: &str = r#"{"id":"uk-01","language":"uk","class":"literal","text":"Скільки примірників?","document":"uk/one.md","answers":["Договір складено у двох примірниках."]}"#;

#[test]
fn a_well_formed_line_becomes_a_question() {
    let f = file(&[OK]);
    let set = QuestionSet::load(f.path()).unwrap();
    let q = &set.questions[0];
    assert_eq!(q.id, "uk-01");
    assert_eq!(q.language, Language::Uk);
    assert_eq!(q.class, Class::Literal);
    assert_eq!(q.text, "Скільки примірників?");
    assert_eq!(q.document, "uk/one.md");
    assert_eq!(q.answers, vec!["Договір складено у двох примірниках."]);
}

#[test]
fn a_blank_line_is_not_a_question_and_not_an_error() {
    // A trailing newline is what every editor writes; refusing it would make
    // the file hostile to the five authoring tasks that append to it.
    let f = file(&[OK, "", "   "]);
    assert_eq!(QuestionSet::load(f.path()).unwrap().questions.len(), 1);
}

#[test]
fn an_empty_answer_sentence_is_refused() {
    // Every chunk `.contains("")` — the empty needle matches everywhere — so
    // an empty sentence would make `resolve_gold` answer Several over the
    // whole corpus: a question that scores against nothing while looking
    // like it scored.
    let line = OK.replace(r#"["Договір складено у двох примірниках."]"#, r#"[""]"#);
    let f = file(&[&line]);
    let err = QuestionSet::load(f.path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("uk-01")),
        "expected the refusal to name the question, got {err:?}"
    );

    // A mix of one real sentence and one empty one is `.any()` true but
    // `.all()` false — the only shape that tells the two functions apart. A
    // list of only-empty (above) does not.
    let mixed = OK.replace(
        r#"["Договір складено у двох примірниках."]"#,
        r#"["Один.",""]"#,
    );
    let err = QuestionSet::load(file(&[&mixed]).path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("uk-01")),
        "got {err:?}"
    );
}

#[test]
fn an_empty_question_text_is_refused() {
    let line = OK.replace(r#""text":"Скільки примірників?""#, r#""text":"   ""#);
    let err = QuestionSet::load(file(&[&line]).path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("uk-01")),
        "got {err:?}"
    );
}

#[test]
fn a_literal_question_carries_exactly_one_answer() {
    let two = OK.replace(
        r#"["Договір складено у двох примірниках."]"#,
        r#"["Договір складено у двох примірниках.","Другий примірник у заявника."]"#,
    );
    let err = QuestionSet::load(file(&[&two]).path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("uk-01")),
        "got {err:?}"
    );

    let none = OK.replace(r#"["Договір складено у двох примірниках."]"#, "[]");
    let err = QuestionSet::load(file(&[&none]).path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("uk-01")),
        "got {err:?}"
    );
}

#[test]
fn a_paraphrase_question_carries_exactly_one_answer() {
    let two = OK
        .replace(r#""class":"literal""#, r#""class":"paraphrase""#)
        .replace(
            r#"["Договір складено у двох примірниках."]"#,
            r#"["Договір складено у двох примірниках.","Другий примірник у заявника."]"#,
        );
    let err = QuestionSet::load(file(&[&two]).path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("uk-01")),
        "got {err:?}"
    );

    let none = OK
        .replace(r#""class":"literal""#, r#""class":"paraphrase""#)
        .replace(r#"["Договір складено у двох примірниках."]"#, "[]");
    let err = QuestionSet::load(file(&[&none]).path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("uk-01")),
        "got {err:?}"
    );
}

#[test]
fn a_topical_question_carries_one_to_three_answers() {
    let three = OK
        .replace(r#""class":"literal""#, r#""class":"topical""#)
        .replace(
            r#"["Договір складено у двох примірниках."]"#,
            r#"["Один.","Два.","Три."]"#,
        );
    assert_eq!(
        QuestionSet::load(file(&[&three]).path()).unwrap().questions[0]
            .answers
            .len(),
        3
    );

    let four = three.replace(
        r#"["Один.","Два.","Три."]"#,
        r#"["Один.","Два.","Три.","Чотири."]"#,
    );
    let err = QuestionSet::load(file(&[&four]).path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("uk-01")),
        "got {err:?}"
    );
}

#[test]
fn two_questions_may_not_share_an_id() {
    // Both directions: the same id twice is refused, two different ids are not.
    let err = QuestionSet::load(file(&[OK, OK]).path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("uk-01")),
        "got {err:?}"
    );

    let other = OK.replace(r#""id":"uk-01""#, r#""id":"uk-02""#);
    assert_eq!(
        QuestionSet::load(file(&[OK, &other]).path())
            .unwrap()
            .questions
            .len(),
        2
    );
}

#[test]
fn an_unknown_class_is_refused_rather_than_defaulted() {
    let line = OK.replace(r#""class":"literal""#, r#""class":"verbatim""#);
    let err = QuestionSet::load(file(&[&line]).path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("verbatim")),
        "got {err:?}"
    );
}

#[test]
fn an_unknown_language_is_refused_rather_than_defaulted() {
    let line = OK.replace(r#""language":"uk""#, r#""language":"fr""#);
    let err = QuestionSet::load(file(&[&line]).path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("fr")),
        "got {err:?}"
    );
}

#[test]
fn an_english_question_is_recognised() {
    let line = OK.replace(r#""language":"uk""#, r#""language":"en""#);
    let set = QuestionSet::load(file(&[&line]).path()).unwrap();
    assert_eq!(set.questions[0].language, Language::En);
}

#[test]
fn class_as_str_and_the_parser_agree_on_all_three_classes() {
    for class in Class::ALL {
        let line = OK
            .replace(
                r#""class":"literal""#,
                &format!(r#""class":"{}""#, class.as_str()),
            )
            .replace(
                r#"["Договір складено у двох примірниках."]"#,
                r#"["Один."]"#,
            );
        let set = QuestionSet::load(file(&[&line]).path()).unwrap();
        assert_eq!(set.questions[0].class, class);
    }
}

#[test]
fn an_unknown_field_in_the_row_is_refused_rather_than_silently_ignored() {
    // Every question line is hand-written by one of five authoring tasks; a
    // misspelled key must not vanish silently into a row that parses as if
    // the field were never there.
    let line = format!(r#"{},"typo":"x"}}"#, OK.trim_end_matches('}'));
    let err = QuestionSet::load(file(&[&line]).path()).unwrap_err();
    assert!(matches!(&err, EvalError::Questions(_)), "got {err:?}");
}

#[test]
fn the_same_answer_sentence_may_not_appear_twice_in_one_question() {
    // Three identical sentences satisfy "up to three, in different chunks"
    // arithmetically while naming one chunk — the set grew and "at least one"
    // stopped meaning anything.
    let line = OK
        .replace(r#""class":"literal""#, r#""class":"topical""#)
        .replace(
            r#"["Договір складено у двох примірниках."]"#,
            r#"["Один.","Один."]"#,
        );
    let err = QuestionSet::load(file(&[&line]).path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("uk-01")),
        "got {err:?}"
    );
}

#[test]
fn answer_sentences_that_canonicalise_the_same_are_still_a_duplicate() {
    // `resolve_gold` canonicalises before comparing (NFC, whitespace runs
    // collapsed), so two answers that differ only by a trailing space name
    // the same chunk — the duplicate guard must canonicalise the same way,
    // or "at least one" quietly becomes "at least one, plus a near-copy."
    let line = OK
        .replace(r#""class":"literal""#, r#""class":"topical""#)
        .replace(
            r#"["Договір складено у двох примірниках."]"#,
            r#"["Один.","Один. "]"#,
        );
    let err = QuestionSet::load(file(&[&line]).path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("uk-01")),
        "got {err:?}"
    );
}

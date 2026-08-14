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
    // `"".contains(…)` is true of every chunk, so an empty sentence would make
    // `resolve_gold` answer Several over the whole corpus — a question that
    // scores against nothing while looking like it scored.
    let line = OK.replace(r#"["Договір складено у двох примірниках."]"#, r#"[""]"#);
    let f = file(&[&line]);
    let err = QuestionSet::load(f.path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Questions(m) if m.contains("uk-01")),
        "expected the refusal to name the question, got {err:?}"
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

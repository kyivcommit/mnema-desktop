mod support;

/// Every question is asked of the provider exactly once. A sweep to come
/// reads these same answers under many rules; asking again per rule would
/// repeat the same live call while measuring nothing new — the content
/// arm's answer depends on the question and the index, not on any rule.
#[test]
fn every_question_reaches_the_provider_exactly_once() {
    let (_c, questions, indexed) = support::small_fixture_with_vectors();
    let mock = support::mock_counting_requests(questions.questions.len());
    let provider = mnema_search::Provider {
        base: mock.base(),
        key: "k".to_string(),
    };

    let answers = mnema_eval::DenseAnswers::ask(&indexed, &questions, provider).unwrap();

    assert_eq!(mock.request_count(), questions.questions.len());
    for q in &questions.questions {
        assert!(!answers.of(&q.id).is_empty(), "{} got no answer", q.id);
    }
    // A question nobody asked about is empty the same way a question that
    // got zero chunks back would be — `of` cannot tell the two apart.
    assert!(answers.of("no-such-question").is_empty());
    // Each mock reply names a different chunk, so the two questions' own
    // answers must differ too — this would not hold if one reply were
    // silently copied onto every question.
    assert_ne!(answers.of("q-1"), answers.of("q-2"));
}

/// The answers carry the model and the service that produced them. A table
/// without them describes an unknown configuration.
#[test]
fn the_answers_carry_the_model_and_the_service_that_made_them() {
    let (_c, questions, indexed) = support::small_fixture_with_vectors();
    let mock = support::mock_counting_requests(questions.questions.len());
    let provider = mnema_search::Provider {
        base: mock.base(),
        key: "k".to_string(),
    };

    let answers = mnema_eval::DenseAnswers::ask(&indexed, &questions, provider).unwrap();

    assert_eq!(answers.model, support::FIXTURE_MODEL);
    assert_eq!(answers.base, mock.base());
}

/// How much of the index the content arm could see rides along too — it
/// names the index's state at this snapshot, not at whenever it is later
/// read, and the index could differ by then.
#[test]
fn the_answers_carry_how_much_of_the_index_was_embedded() {
    let (_c, questions, indexed) = support::small_fixture_with_vectors();
    let mock = support::mock_counting_requests(questions.questions.len());
    let provider = mnema_search::Provider {
        base: mock.base(),
        key: "k".to_string(),
    };

    let answers = mnema_eval::DenseAnswers::ask(&indexed, &questions, provider).unwrap();

    assert!(answers.total > 0, "the fixture must hold chunks to embed");
    assert_eq!(
        answers.embedded, answers.total,
        "the fixture embeds every chunk"
    );
}

/// A corpus with no active embedding space is refused before any request —
/// there is nothing to embed the questions into, so a live call would only
/// spend the provider's quota on a run that cannot be scored.
#[test]
fn no_active_space_is_refused_before_any_request() {
    let (_c, questions, indexed) = support::small_fixture();
    let mock = support::mock_counting_requests(questions.questions.len());
    let provider = mnema_search::Provider {
        base: mock.base(),
        key: "k".to_string(),
    };

    let err = mnema_eval::DenseAnswers::ask(&indexed, &questions, provider).unwrap_err();

    assert!(matches!(err, mnema_eval::EvalError::NoActiveSpace));
    assert_eq!(mock.request_count(), 0);
}

/// A content arm that failed to answer ends the whole run rather than being
/// skipped: nothing later could tell "silent" apart from "never asked".
#[test]
fn a_failed_content_arm_ends_the_run() {
    let (_c, questions, indexed) = support::small_fixture_with_vectors();
    let mock =
        mnema_mock_provider::MockServer::new(vec![mnema_mock_provider::Reply::status(500, "boom")]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };

    let err = mnema_eval::DenseAnswers::ask(&indexed, &questions, provider).unwrap_err();

    assert!(matches!(err, mnema_eval::EvalError::ContentArmSilent(_)));
}

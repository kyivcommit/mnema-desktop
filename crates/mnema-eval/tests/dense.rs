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
    // A question nobody asked about has no answer, rather than an empty one
    // that would read as "the content arm found nothing".
    assert!(answers.of("no-such-question").is_empty());
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

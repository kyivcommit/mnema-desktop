use mnema_mock_provider::{MockServer, Reply};

mod support;

/// The defect the live path shipped with, put in a test: a corpus that was
/// only walked has no active space, and `DenseAnswers::ask` refuses one —
/// until `embed_corpus` runs between them, the same step the live binary and
/// the live sweep test both take before asking a single question.
#[test]
fn a_corpus_that_was_only_walked_can_be_asked_after_it_is_embedded() {
    let (_c, questions, indexed) = support::small_fixture();
    let mock = support::mock_embedding_a_corpus_then_answering(&indexed, &questions);
    let provider = mnema_search::Provider {
        base: mock.base(),
        key: "k".into(),
    };
    mnema_eval::embed_corpus(&indexed, &provider, mnema_eval::EVAL_MODEL).unwrap();
    let answers = mnema_eval::DenseAnswers::ask(&indexed, &questions, provider).unwrap();
    assert!(answers.total > 0);
    assert_eq!(answers.embedded, answers.total);
}

/// The space is created at the width the provider actually measured, not at
/// a width this crate assumes — `live_provider.rs:84-87` already argues why
/// a model's width may not be read from any list.
#[test]
fn the_space_is_built_at_the_width_the_provider_measured() {
    let (_c, _questions, indexed) = support::small_fixture();
    // A width the caller never names, so a hardcoded one cannot pass by
    // coincidence (`mnema-provider/tests/probe.rs:975` uses the same width
    // for the same reason).
    let mock = MockServer::new(vec![
        Reply::ok(&mnema_mock_provider::two_vectors(8)),
        Reply::ok(&mnema_mock_provider::two_vectors(8)),
    ]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".into(),
    };
    let embedded = mnema_eval::embed_corpus(&indexed, &provider, mnema_eval::EVAL_MODEL).unwrap();
    assert_eq!(embedded.width, 8);
    let space = indexed.db().active_space().unwrap().unwrap();
    assert_eq!(indexed.db().space_model(space).unwrap().1, 8);
}

/// `credential_ref` names the variable the key came from
/// (`env:MNEMA_EVAL_KEY`), never the key itself — `embed_corpus` takes no
/// parameter that could carry it there instead, and this reads the column a
/// caller who broke that would have written it to.
#[test]
fn the_key_never_reaches_the_database() {
    let (_c, questions, indexed) = support::small_fixture();
    let mock = support::mock_embedding_a_corpus_then_answering(&indexed, &questions);
    let provider = mnema_search::Provider {
        base: mock.base(),
        key: "a-real-looking-secret".into(),
    };
    mnema_eval::embed_corpus(&indexed, &provider, mnema_eval::EVAL_MODEL).unwrap();

    let stored: String = indexed
        .db()
        .conn()
        .query_row("SELECT credential_ref FROM model_config", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, "env:MNEMA_EVAL_KEY");
}

/// `mnema_embed::run` can come back `Ok` over a corpus it only partly
/// embedded, once a refusal is corroborated by some other chunk's success
/// (`mnema-embed/src/lib.rs:386-400`). `embed_corpus` must not read that `Ok`
/// as "measured" — a sweep run over a partial space would describe a smaller
/// corpus than the one it claims to.
#[test]
fn a_partly_embedded_corpus_is_refused_rather_than_measured() {
    let (_c, _questions, indexed) = support::small_fixture();
    let mock = MockServer::new(vec![
        Reply::ok(&mnema_mock_provider::two_vectors(support::FIXTURE_WIDTH)), // probe
        Reply::status(400, r#"{"error":{"message":"nope"}}"#), // the batch of two, refused
        support::single_vector_reply(0), // one_at_a_time: the first chunk, alone
        Reply::status(400, r#"{"error":{"message":"nope"}}"#), // the second, refused again
    ]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".into(),
    };
    let err = mnema_eval::embed_corpus(&indexed, &provider, mnema_eval::EVAL_MODEL).unwrap_err();
    assert_eq!(
        err,
        mnema_eval::EvalError::CorpusNotEmbedded {
            embedded: 1,
            total: 2,
        }
    );
}

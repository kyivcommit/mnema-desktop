use mnema_search::{ContentArm, Missing};

mod support;

/// "Off", "cannot be asked", "asked and failed", and "asked and answered
/// with nothing" are separate facts, and a person shown one list for all of
/// them learns something false about their documents. The type is what
/// keeps them apart; this pins that every variant below is told apart from
/// every other.
#[test]
fn the_content_arms_silences_are_told_apart_by_type() {
    let states = [
        ContentArm::Off,
        ContentArm::NotConfigured(Missing::NoKey),
        ContentArm::NotConfigured(Missing::NoModel),
        ContentArm::Failed {
            reason: "no route to host".to_string(),
        },
        ContentArm::Answered {
            chunks: vec![],
            embedded: 0,
            total: 9,
        },
    ];
    // Each is distinct from every other, including each `NotConfigured`
    // payload and including an `Answered` that answered with nothing.
    for (i, a) in states.iter().enumerate() {
        for (j, b) in states.iter().enumerate() {
            assert_eq!(i == j, a == b, "{a:?} vs {b:?}");
        }
    }
}

/// An arm that answered nothing is not an arm that could not be asked, and the
/// coverage numbers ride on the answer rather than beside it.
#[test]
fn an_empty_answer_still_carries_its_coverage() {
    let arm = ContentArm::Answered {
        chunks: vec![],
        embedded: 3,
        total: 9,
    };
    match arm {
        ContentArm::Answered {
            chunks,
            embedded,
            total,
        } => {
            assert!(chunks.is_empty());
            assert_eq!((embedded, total), (3, 9));
        }
        other => panic!("expected an answer, got {other:?}"),
    }
}

/// The whole arm, end to end against a rude little HTTP server: a query
/// becomes a vector, the vector becomes a `knn` answer, and the answer
/// carries how much of the index it could even see.
#[test]
fn the_content_arm_turns_a_query_into_nearest_chunks() {
    let f = support::indexed_space();
    let mock = support::mock_returning_vector_near(&f, f.chunk_ids[1]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };

    let arm = mnema_search::content_arm(&f.db, Some(provider), "ремонт даху", 10);

    match arm {
        mnema_search::ContentArm::Answered {
            chunks,
            embedded,
            total,
        } => {
            assert_eq!(chunks.first(), Some(&f.chunk_ids[1]));
            assert_eq!(
                (embedded, total),
                (f.chunk_ids.len() as i64, f.chunk_ids.len() as i64)
            );
        }
        other => panic!("expected an answer, got {other:?}"),
    }
}

/// No key is not a failure and not an empty answer.
#[test]
fn no_key_is_not_configured_rather_than_failed() {
    let f = support::indexed_space();
    assert_eq!(
        mnema_search::content_arm(&f.db, None, "ремонт даху", 10),
        mnema_search::ContentArm::NotConfigured(mnema_search::Missing::NoKey)
    );
}

/// A half-filled space answers over its half and says so. Without the pair of
/// numbers a person reads "nothing found" as a fact about their documents.
#[test]
fn a_partly_embedded_space_says_how_much_of_the_index_it_saw() {
    let f = support::indexed_space_with_some_vectors_missing();
    let mock = support::mock_returning_vector_near(&f, f.embedded_ids[0]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };

    match mnema_search::content_arm(&f.db, Some(provider), "ремонт даху", 10) {
        mnema_search::ContentArm::Answered {
            embedded, total, ..
        } => {
            assert_eq!(embedded, f.embedded_ids.len() as i64);
            assert_eq!(total, f.chunk_ids.len() as i64);
            assert!(embedded < total, "the fixture must leave chunks unembedded");
        }
        other => panic!("expected an answer, got {other:?}"),
    }
}

/// A count that cannot be read is not a zero. `0 of 0` is a claim about the
/// index; a failed count is a claim about this build.
#[test]
fn a_coverage_count_that_fails_makes_the_arm_failed_not_empty() {
    let f = support::indexed_space();
    let mock = support::mock_returning_vector_near(&f, f.chunk_ids[0]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };
    f.break_chunk_count();

    match mnema_search::content_arm(&f.db, Some(provider), "ремонт даху", 10) {
        mnema_search::ContentArm::Failed { .. } => {}
        other => panic!("expected a failure, got {other:?}"),
    }
}

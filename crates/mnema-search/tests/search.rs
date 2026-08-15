mod support;

/// The text arm shows its own answer even when the content arm could not be
/// asked. A search that failed whole because one arm failed would turn a
/// missing key into "your documents do not say this".
#[test]
fn the_text_arm_still_answers_when_the_content_arm_cannot_be_asked() {
    let f = support::indexed_space();
    let found = mnema_search::search(
        &f.db,
        None,
        "ремонт даху",
        mnema_search::Arms {
            text: true,
            content: true,
        },
        mnema_index::QueryRule::AnyTerm,
        mnema_search::FusionRule::Rrf,
        20,
    )
    .unwrap();

    assert!(!found.chunks.is_empty(), "the text arm had answers to give");
    assert!(matches!(found.text, mnema_search::TextArm::Answered { .. }));
    assert_eq!(
        found.content,
        mnema_search::ContentArm::NotConfigured(mnema_search::Missing::NoKey)
    );
}

/// An arm that was not asked is `Off`, which is neither an empty answer nor a
/// failure — and its chunks take no part in the fusion.
#[test]
fn an_arm_that_is_off_contributes_nothing_and_says_so() {
    let f = support::indexed_space();
    let found = mnema_search::search(
        &f.db,
        None,
        "ремонт даху",
        mnema_search::Arms {
            text: false,
            content: true,
        },
        mnema_index::QueryRule::AnyTerm,
        mnema_search::FusionRule::TextOnly,
        20,
    )
    .unwrap();

    assert_eq!(found.text, mnema_search::TextArm::Off);
    assert!(
        found.chunks.is_empty(),
        "TextOnly over an off text arm is empty"
    );
}

/// Both arms off is answered, not refused: the invariant that one must be on
/// belongs to the window, and a core that panicked here would make the window's
/// job harder rather than safer.
#[test]
fn both_arms_off_is_an_empty_answer_with_both_states_named() {
    let f = support::indexed_space();
    let found = mnema_search::search(
        &f.db,
        None,
        "ремонт даху",
        mnema_search::Arms {
            text: false,
            content: false,
        },
        mnema_index::QueryRule::AnyTerm,
        mnema_search::FusionRule::Rrf,
        20,
    )
    .unwrap();

    assert!(found.chunks.is_empty());
    assert_eq!(found.text, mnema_search::TextArm::Off);
    assert_eq!(found.content, mnema_search::ContentArm::Off);
}

/// The capstone property: with both arms real and on, the content arm's
/// chunks are not dropped before fusing. `"покрівля"` matches only chunk 1
/// lexically, so any other chunk in `found.chunks` must have come from the
/// content arm — a mutant that fused against an empty content list instead
/// of the real one would lose it.
#[test]
fn the_content_arms_chunks_survive_into_the_fused_list() {
    let f = support::indexed_space();
    let mock = support::mock_returning_vector_near(&f, f.chunk_ids[1]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };

    let found = mnema_search::search(
        &f.db,
        Some(provider),
        "покрівля",
        mnema_search::Arms {
            text: true,
            content: true,
        },
        mnema_index::QueryRule::AnyTerm,
        mnema_search::FusionRule::Rrf,
        20,
    )
    .unwrap();

    assert!(matches!(
        found.content,
        mnema_search::ContentArm::Answered { .. }
    ));
    assert!(
        found.chunks.contains(&f.chunk_ids[0]),
        "chunk 0's text never matches \"покрівля\"; only the content arm \
         could have placed it in the fused list"
    );
}

/// Each arm is queried to `CANDIDATES`, deeper than the caller's `limit` —
/// otherwise a fusion rule would have nothing beyond `limit` to reorder.
/// The arm's own answer is checked directly, by count rather than order, so
/// this does not depend on how ties in `rank` or `distance` are broken.
#[test]
fn the_text_arm_is_asked_deeper_than_the_final_limit() {
    let f = support::indexed_space();
    let found = mnema_search::search(
        &f.db,
        None,
        "ремонт даху",
        mnema_search::Arms {
            text: true,
            content: false,
        },
        mnema_index::QueryRule::AnyTerm,
        mnema_search::FusionRule::TextOnly,
        2,
    )
    .unwrap();

    match found.text {
        mnema_search::TextArm::Answered { chunks } => {
            assert_eq!(
                chunks.len(),
                f.chunk_ids.len(),
                "the arm's own answer must hold every match, not just `limit`"
            );
        }
        other => panic!("expected an answer, got {other:?}"),
    }
    assert_eq!(found.chunks.len(), 2, "fuse still cuts to `limit` after");
}

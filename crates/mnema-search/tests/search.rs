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

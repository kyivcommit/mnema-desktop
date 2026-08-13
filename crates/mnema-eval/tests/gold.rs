use mnema_eval::{Gold, resolve_gold};

fn chunks() -> Vec<(i64, String)> {
    vec![
        (10, "Комісія розглянула заяву.".to_string()),
        (11, "Другий примірник зберігається у заявника.".to_string()),
        (12, "Ухвалено передати справу далі.".to_string()),
    ]
}

#[test]
fn a_sentence_in_exactly_one_chunk_names_that_chunk() {
    assert!(matches!(
        resolve_gold(&chunks(), "Другий примірник зберігається у заявника."),
        Gold::One(11)
    ));
}

#[test]
fn a_sentence_in_no_chunk_is_missing_not_a_guess() {
    assert!(matches!(
        resolve_gold(&chunks(), "Такого речення тут немає."),
        Gold::Missing
    ));
}

/// Not a defect: the chunker re-seeds 15% of a finished chunk into the next one
/// (`mnema-chunk`, `OVERLAP_RATIO`), so a sentence in the overlap is in two
/// chunks legitimately. The harness must be able to say so; what to do about it
/// is preflight's decision, not this function's.
#[test]
fn a_sentence_in_two_chunks_names_both() {
    let overlapping = vec![
        (
            10,
            "…кінець першого. Ухвалено передати справу далі.".to_string(),
        ),
        (
            11,
            "Ухвалено передати справу далі. Початок другого…".to_string(),
        ),
    ];
    match resolve_gold(&overlapping, "Ухвалено передати справу далі.") {
        Gold::Several(ids) => assert_eq!(ids, vec![10, 11]),
        other => panic!("expected Several, got {other:?}"),
    }
}

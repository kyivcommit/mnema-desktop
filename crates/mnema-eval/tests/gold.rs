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
    assert_eq!(
        resolve_gold(&chunks(), "Другий примірник зберігається у заявника."),
        Gold::One(11)
    );
}

#[test]
fn a_sentence_in_no_chunk_is_missing_not_a_guess() {
    assert_eq!(
        resolve_gold(&chunks(), "Такого речення тут немає."),
        Gold::Missing
    );
}

/// The doc comment argues "substring, not tokens", and until this test nothing
/// held it: an implementation reading "all the words, in any order" passes every
/// other test in this file. It matters because canonicalisation loosens the
/// comparison deliberately, and nothing else would go red if the loosening went
/// one step too far.
#[test]
fn the_same_words_in_another_order_are_not_the_sentence() {
    let scrambled = vec![(10, "У заявника зберігається примірник другий.".to_string())];
    assert_eq!(
        resolve_gold(&scrambled, "Другий примірник зберігається у заявника."),
        Gold::Missing
    );
}

/// Decomposed on one side, precomposed on the other — the shape macOS hands over
/// against the shape a question file is typed in. Byte comparison reports
/// Missing here and the corpus is blameless.
#[test]
fn a_decomposed_sentence_finds_its_precomposed_chunk() {
    // The chunk holds the precomposed "ї" (U+0457) that extraction's NFC pass
    // leaves behind; the question is typed decomposed (U+0456 U+0308), which is
    // what a macOS keyboard and filesystem hand over.
    let chunk = vec![(10, "Комісія ухвалила \u{457}хнє рішення.".to_string())];
    assert_eq!(
        resolve_gold(&chunk, "ухвалила \u{456}\u{308}хнє рішення."),
        Gold::One(10)
    );
}

/// A hard-wrapped paragraph keeps its newline in the chunk; nobody types the
/// answer sentence with it.
#[test]
fn a_line_break_inside_the_chunk_does_not_hide_the_sentence() {
    let chunk = vec![(10, "Договір складено\nу двох примірниках.".to_string())];
    assert_eq!(
        resolve_gold(&chunk, "Договір складено у двох примірниках."),
        Gold::One(10)
    );
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

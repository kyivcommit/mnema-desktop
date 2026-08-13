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

/// `canonical` promises no reordering, and this is the test that pins it.
/// An implementation comparing word sets instead of running canonicalised
/// text through `.contains` would find this chunk, since it holds the same
/// words in a different order — canonicalisation loosens the comparison
/// deliberately, and this pins how far.
#[test]
fn the_same_words_in_another_order_are_not_the_sentence() {
    // The chunk differs from the sentence in word ORDER and in nothing else:
    // same words, same capitalisation, full stop in the same place. An earlier
    // fixture also moved capitals and punctuation around, and that made a
    // naive token-subset implementation fail for a reason unrelated to order —
    // the test passed while isolating nothing.
    let scrambled = vec![(10, "Другий зберігається примірник у заявника.".to_string())];
    assert_eq!(
        resolve_gold(&scrambled, "Другий примірник зберігається у заявника."),
        Gold::Missing
    );
}

/// `canonical` collapses whitespace; it must not delete it. Every other test in
/// this file passes under deletion too — measured, not assumed — and deletion is
/// materially worse: it erases word boundaries, so a needle can match across a
/// join that is not the sentence anybody wrote.
#[test]
fn words_run_together_are_not_the_sentence() {
    let glued = vec![(10, "Комісіярозглянула заяву.".to_string())];
    assert_eq!(
        resolve_gold(&glued, "Комісія розглянула заяву."),
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

/// `canonical` must not strip combining marks — the danger has two independent
/// shapes. This test pins stripping without first decomposing, which would fold
/// away the stress mark on `сло́во`. Its sibling,
/// `folding_i_kratke_into_i_would_match_a_different_word`, pins decomposing
/// first and then stripping, which folds `й` into `и` and would answer `One`
/// for a chunk holding a different word than the question names.
#[test]
fn a_stress_mark_is_part_of_the_word_here() {
    // `о` + U+0301. No precomposed form exists, so NFC keeps the mark and the
    // oracle keeps it too. FTS5 removes it, and being stricter than the index
    // here is the decision — paid for by the corpus rules, not by loosening.
    let chunk = vec![(10, "Комісія ухвалила сло\u{301}во.".to_string())];
    assert_eq!(resolve_gold(&chunk, "ухвалила слово."), Gold::Missing);
}

#[test]
fn folding_i_kratke_into_i_would_match_a_different_word() {
    // The chunk says `новий`. The needle says `новии` — which is exactly what
    // `новий` becomes if a mark-stripping implementation decomposes first. They
    // are different words and must not match.
    let chunk = vec![(10, "Комісія ухвалила новий склад.".to_string())];
    assert_eq!(resolve_gold(&chunk, "ухвалила новии склад."), Gold::Missing);
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

#[test]
fn case_is_part_of_the_sentence() {
    // `canonical` promises no case folding. Under a folding implementation the
    // needle would find this chunk, and every other test in this file stays
    // green either way.
    let chunk = vec![(10, "Комісія розглянула заяву.".to_string())];
    assert_eq!(
        resolve_gold(&chunk, "комісія розглянула заяву."),
        Gold::Missing
    );
}

#[test]
fn punctuation_is_part_of_the_sentence() {
    // The chunk drops the full stop the needle carries. Strip punctuation on
    // both sides and this becomes a match.
    let chunk = vec![(10, "Ухвалено передати справу далі".to_string())];
    assert_eq!(
        resolve_gold(&chunk, "Ухвалено передати справу далі."),
        Gold::Missing
    );
}

#[test]
fn a_decomposed_chunk_holds_a_precomposed_sentence() {
    // The mirror of `a_decomposed_sentence_finds_its_precomposed_chunk`:
    // normalising only the needle passes that one and fails this one.
    let chunk = vec![(
        10,
        "Комісія ухвалила \u{456}\u{308}хнє рішення.".to_string(),
    )];
    assert_eq!(
        resolve_gold(&chunk, "ухвалила \u{457}хнє рішення."),
        Gold::One(10)
    );
}

#[test]
fn a_doubled_space_in_the_sentence_still_finds_the_chunk() {
    // The mirror of `a_line_break_inside_the_chunk_does_not_hide_the_sentence`:
    // collapsing only the chunk passes that one and fails this one.
    let chunk = vec![(10, "Договір складено у двох примірниках.".to_string())];
    assert_eq!(
        resolve_gold(&chunk, "Договір  складено у двох примірниках."),
        Gold::One(10)
    );
}

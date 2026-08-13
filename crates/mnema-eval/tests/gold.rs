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

/// `canonical` must NOT strip combining marks, and this is the only test that
/// says so. A sister change is making `mnema-index`'s term reporting mirror
/// FTS5, which removes every mark in U+0300–U+0331 — and copying that here would
/// be wrong in a way every other test in this file is blind to: measured, an
/// implementation that strips marks passes all seven. If it decomposes first it
/// folds `й` into `и` and `ї` into `і`, which this project forbids outright, and
/// the oracle would then answer `One` for a chunk holding a **different word**.
///
/// The oracle compares against `chunk.text`, which the index never folds; being
/// stricter than the search here is correct, and the cost — a question that must
/// reproduce a stress mark exactly — is paid in the corpus rules and in
/// pre-flight, not by loosening this.
/// The other half of the same danger, and the likelier edit of the two. The
/// repository already contains a function that strips marks **without**
/// decomposing — `mnema_core::nfc::strip_latin_diacritics`, built deliberately
/// so that `й` survives. Swapping `canonical`'s `normalise` for it is the change
/// someone will reach for ("use the one that mirrors the index"), and measured,
/// **all eight other tests stay green** while the oracle quietly stops telling
/// `сло́во` from `слово` — the exact opposite of what the comment below argues.
///
/// The two tests together cover both halves: this one catches strip-without-
/// decompose, the next catches decompose-then-strip.
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

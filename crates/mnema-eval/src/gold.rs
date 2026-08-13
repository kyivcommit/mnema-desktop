/// Which chunk holds an answer sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gold {
    /// Exactly one — the only shape a question may ship with.
    One(i64),
    /// None. Every cause is a defect in the input — question, corpus, or the
    /// pairing of the two — never a failure of search. Task 9's preflight
    /// refuses a question that resolves here before anything is scored, so a
    /// dropped configuration is never blamed for a miss that isn't one.
    Missing,
    /// Several. Legitimate — chunks overlap — and still not shippable: with two
    /// right answers a rank means two things. Preflight refuses it and the
    /// author moves the sentence.
    ///
    /// Ids come back in the order the caller supplied the chunks. The caller is
    /// `Db::chunks_of_document`, which orders by `chunk.ord`; saying so here is
    /// what keeps a `GoldSeveral` message stable between runs.
    ///
    /// Written as plain text, not an intra-doc link: `mnema-index` is not a
    /// dependency of this crate, so the link would never resolve and
    /// `rustdoc::broken_intra_doc_links` is not in the gates to catch it.
    Several(Vec<i64>),
}

/// NFC, then every run of whitespace collapsed to one space, then trimmed.
///
/// Nothing else: no case folding (`case_is_part_of_the_sentence`), no
/// punctuation stripping (`punctuation_is_part_of_the_sentence`), no
/// reordering (`the_same_words_in_another_order_are_not_the_sentence`). Kept
/// private and small because [`resolve_gold`] documents the reasoning, and a
/// helper that grew a second job would put that reasoning out of date.
fn canonical(text: &str) -> String {
    mnema_core::nfc::normalise(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Finds the chunks whose text holds `sentence`, canonicalised on both sides. Substring, not
/// tokens, so word order still counts
/// (`the_same_words_in_another_order_are_not_the_sentence`). NFC bridges decomposed vs.
/// precomposed text on either side (`a_decomposed_sentence_finds_its_precomposed_chunk`,
/// `a_decomposed_chunk_holds_a_precomposed_sentence`); collapsing (not deleting) whitespace
/// bridges a hard wrap vs. a doubled space on either side
/// (`words_run_together_are_not_the_sentence`,
/// `a_doubled_space_in_the_sentence_still_finds_the_chunk`). The caller owes chunks of an
/// `indexed` document — `Db::chunks_of_document` (`crates/mnema-index/src/write.rs:812`) does
/// not filter by status; task 9's preflight is where that is checked.
pub fn resolve_gold(chunks: &[(i64, String)], sentence: &str) -> Gold {
    let needle = canonical(sentence);
    let hits: Vec<i64> = chunks
        .iter()
        .filter(|(_, text)| canonical(text).contains(&needle))
        .map(|(id, _)| *id)
        .collect();
    match hits.len() {
        0 => Gold::Missing,
        1 => Gold::One(hits[0]),
        _ => Gold::Several(hits),
    }
}

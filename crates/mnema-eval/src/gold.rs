/// Which chunk holds an answer sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gold {
    /// Exactly one — the only shape a question may ship with.
    One(i64),
    /// None. Either the corpus was edited and the question was not, or the
    /// sentence was mistyped. Both are preflight failures, and neither may be
    /// silently scored as a miss: a question with no right answer would make
    /// every configuration look worse for a reason that is not search.
    Missing,
    /// Several. Legitimate — chunks overlap — and still not shippable: with two
    /// right answers a rank means two things. Preflight refuses it and the
    /// author moves the sentence.
    Several(Vec<i64>),
}

/// Finds the chunks whose stored text contains `sentence`.
///
/// Substring, not tokens: the sentence is quoted from the document, and the
/// chunk stores the original text (`schema.sql:153`, "the original, for
/// display"). Matching on prepared terms instead would let a sentence match a
/// chunk that merely shares its words in another order.
pub fn resolve_gold(chunks: &[(i64, String)], sentence: &str) -> Gold {
    let hits: Vec<i64> = chunks
        .iter()
        .filter(|(_, text)| text.contains(sentence))
        .map(|(id, _)| *id)
        .collect();
    match hits.len() {
        0 => Gold::Missing,
        1 => Gold::One(hits[0]),
        _ => Gold::Several(hits),
    }
}

/// Which chunk holds an answer sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gold {
    /// Exactly one — the only shape a question may ship with.
    One(i64),
    /// None, and there are THREE causes, not two: the corpus was edited and the
    /// question was not; the sentence was mistyped; or the sentence was quoted
    /// from text the chunk does not hold in that form. The third is the one a
    /// reader will not think of — the markdown reader stores raw source lines
    /// (`crates/mnema-extract/src/markdown.rs:130`), so inline markup is part of
    /// the chunk's text, and the chunker joins non-adjacent pieces with a
    /// separator of its own (`crates/mnema-chunk/src/pack.rs:44-48`).
    ///
    /// All three are preflight failures, and none may be silently scored as a
    /// miss: a question with no right answer makes every configuration look
    /// worse for a reason that is not search.
    Missing,
    /// Several. Legitimate — chunks overlap — and still not shippable: with two
    /// right answers a rank means two things. Preflight refuses it and the
    /// author moves the sentence.
    ///
    /// Ids come back in the order the caller supplied the chunks. The caller is
    /// [`Db::chunks_of_document`], which orders by `chunk.ord`; saying so here
    /// is what keeps a `GoldSeveral` message stable between runs.
    Several(Vec<i64>),
}

/// Finds the chunks whose stored text contains `sentence`.
///
/// **The caller owes chunks of a document whose `document.status` is
/// `indexed`.** `Db::chunks_of_document` (`crates/mnema-index/src/write.rs:799`)
/// returns a document's chunks whatever its status, while `search_lexical`
/// (`crates/mnema-index/src/search.rs:42`) returns only those of an `indexed`
/// one. Resolve a gold chunk from a `pending` document and this answers
/// confidently with a chunk the search can never return — every question
/// against it scores a miss, and the number reads as "search is bad". Preflight
/// is where that is checked; this comment is where the obligation is legible.
///
/// Substring, not tokens: the sentence is quoted from the document, and the
/// chunk stores the original text (`schema.sql:153`, "the original, for
/// display"). Matching on prepared terms instead would let a sentence match a
/// chunk that merely shares its words in another order.
///
/// **Canonicalised on both sides, and that is not cosmetic.** Extraction
/// NFC-normalises before a block's text is emitted
/// (`crates/mnema-extract/src/text.rs:40`, `markdown.rs:91`), while an answer
/// sentence is typed by hand into a file — on macOS, decomposed. Raw byte
/// comparison would report `Missing` for a sentence the document holds in full.
/// Whitespace runs collapse for the same reason: a hard-wrapped paragraph keeps
/// its newline in the chunk, and nobody types the answer with it.
fn canonical(text: &str) -> String {
    mnema_core::nfc::normalise(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

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

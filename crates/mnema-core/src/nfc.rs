//! Unicode normalisation, owed on both sides of the pipeline.
//!
//! `remove_diacritics 2` — the tokenizer's own folding, `schema.sql` — converges
//! a decomposed and a precomposed spelling onto one token for Latin: measured,
//! `König` in both forms collapses. It does **not** for the product's own
//! language: NFC `й` (U+0439) and decomposed `и` (U+0438) + combining breve
//! (U+0306) tokenize as two different words, and so do the two forms of `ї`.
//! D32.
//!
//! Not because the breve is spared — measured, it is not. A standalone U+0306
//! is deleted whatever precedes it, and so is U+0308; what `remove_diacritics
//! 2` leaves alone is the *precomposed* letter, U+0439 and U+0457, which its
//! table does not carry. So the two spellings diverge harder than "two
//! different words" suggests: the precomposed one stays `й`, the decomposed
//! one loses its mark and becomes plain `и`. Nothing folds them back together
//! in either direction, which is the conclusion D32 rests on and is why NFC
//! runs first on both sides of the pipeline.
//!
//! The consequence is not cosmetic: macOS hands over decomposed text, a query
//! typed on another machine is precomposed, and a document becomes unfindable
//! by its own spelling in either direction — which is why this lives in
//! `mnema-core` rather than beside either caller. `mnema-index::text_prep`
//! calls it as the first step of query preparation; extraction calls it before
//! a block's text is emitted, ahead of the offsets and hashes taken from it.

use std::borrow::Cow;

use unicode_normalization::UnicodeNormalization;

/// Normalises to NFC, borrowing when the input already is one.
///
/// Character count changes on decomposed input — one more reason this must
/// run before anything downstream takes an offset or a hash from the text.
pub fn normalise(s: &str) -> Cow<'_, str> {
    if unicode_normalization::is_nfc(s) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(s.nfc().collect())
    }
}

/// The combining marks `remove_diacritics 2` deletes.
///
/// A set, not a rule, because SQLite's is a set: `fts5_remove_diacritic`
/// binary-searches a hardcoded code point table and never looks at the
/// character before the mark. Measured, not read off the C — `mnema-index`'s
/// `search_terms_matches_what_fts5_stores_for_every_mark` indexes each mark
/// in U+0300–U+036F behind a base that composes with none of them and reads
/// back what the tokenizer actually stored.
///
/// **The set is not the range**, which is the trap this function exists to
/// avoid. Twenty-one code points inside U+0300–U+0331 are kept — U+0305,
/// U+030D–U+030E, U+0310, U+0312–U+031A, U+031C–U+0322, U+0329–U+032C,
/// U+032F — so a predicate written as the whole window strips marks the index
/// keeps, and `search_terms` then under-reports: it would claim a term the
/// index never stored, and a caller comparing the two would record a miss the
/// engine never made.
///
/// Nothing above U+0331 belongs to it. U+0340, U+0341 and U+0344 measure as
/// deleted and are not exceptions: each is a canonical singleton that NFC
/// rewrites into this window before the tokenizer sees it, which is why
/// `strip_latin_diacritics` normalises first rather than trusting a caller to.
///
/// Hebrew niqqud, the Devanagari virama and matra, and Arabic harakat are
/// outside the set — measured in the same sweep. That is the whole of their
/// protection here, and it is the real one: this predicate does not consult
/// the base character, so nothing about their base is doing the work.
fn is_stripped_mark(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{0304}'
            | '\u{0306}'..='\u{030C}'
            | '\u{030F}'
            | '\u{0311}'
            | '\u{031B}'
            | '\u{0323}'..='\u{0328}'
            | '\u{032D}'..='\u{032E}'
            | '\u{0330}'..='\u{0331}'
    )
}

/// Strips what `remove_diacritics 2` strips, and leaves what it leaves.
///
/// `mnema-index::search_terms` is why this is callable rather than staying an
/// implementation detail of the tokenizer: it reports the string the index
/// actually stores as a term, and for a Latin word carrying a diacritic that
/// string has no diacritic in it.
///
/// Two rules, because the tokenizer has two, and they are independent:
///
/// - A mark in `is_stripped_mark` goes, **whatever precedes it**. This is the
///   half that was missing, and Ukrainian is where it showed: `сло́во` keeps
///   its stress accent through NFC — U+043E has no precomposed acute form —
///   so the index stores `слово` while a base-conditional predicate reported
///   `сло́во`, a disagreement in the language this product is for.
/// - A precomposed letter folds to its base only when that base is ASCII.
///   `Zürich` and `Zurich` converge; `й` (U+0439) and `ї` (U+0457) do not
///   fold to `и`/`і`, and neither does `ӧ` (U+04E7) to `о` — all three
///   measured through the real tokenizer. D32.
///
/// The two do not collide, and the ordering below is what keeps them apart.
/// NFC comes first, so `й` is one character by the time either rule runs and
/// the first rule never sees its breve; only then is each character consulted
/// on its own, so nothing re-decomposes what NFC just composed. Normalising
/// here rather than relying on `prepare_for_search` having done it is
/// deliberate: this function's own previous version decomposed the whole
/// string internally, which silently undid the caller's normalisation and put
/// `й`'s breve back in front of the predicate. Measured — with the mark rule
/// added and the decomposition left in, precomposed `йод` came out `иод`.
///
/// `ł`, `ø` and `æ` need no exception and are given none. Unicode assigns
/// them no canonical decomposition, so the second rule finds no ASCII base to
/// fold them to; the same is true of `đ`, `ħ`, `ŋ`, `ð`, `þ`, `œ` and `ı`,
/// which is the difference between deriving this from decomposition and
/// naming three letters. FTS5 agrees on all of them, measured — but by its
/// own table rather than by decomposition, so this is two mechanisms landing
/// on one result, not one mechanism seen twice.
pub fn strip_latin_diacritics(s: &str) -> String {
    let normalised = normalise(s);
    let mut out = String::with_capacity(normalised.len());
    for c in normalised.chars() {
        if is_stripped_mark(c) {
            continue;
        }
        // Canonical decomposition only, so a first character that is an ASCII
        // letter means the rest are combining marks — and every one of them is
        // in `is_stripped_mark`'s set: swept over all of Unicode 16, no
        // ASCII-based precomposed letter carries a mark FTS5 keeps.
        let base = c.nfd().next().unwrap_or(c);
        out.push(if base.is_ascii_alphabetic() { base } else { c });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decomposed_ukrainian_normalises_to_the_precomposed_form() {
        let precomposed = "йод";
        let decomposed = "\u{0438}\u{0306}од";
        assert_ne!(precomposed, decomposed);
        assert_eq!(normalise(decomposed), precomposed);
    }

    #[test]
    fn already_nfc_text_is_borrowed_not_copied() {
        let text = "йод";
        assert!(matches!(normalise(text), Cow::Borrowed(_)));
    }

    #[test]
    fn decomposed_text_is_owned() {
        let decomposed = "\u{0438}\u{0306}од";
        assert!(matches!(normalise(decomposed), Cow::Owned(_)));
    }

    #[test]
    fn latin_diacritics_are_stripped_from_both_spellings() {
        // Precomposed and NFD-decomposed must agree: both are what a caller
        // could plausibly hand in, and the index's own `remove_diacritics 2`
        // does not care which one it received either.
        assert_eq!(strip_latin_diacritics("Zürich"), "Zurich");
        assert_eq!(strip_latin_diacritics("Zu\u{0308}rich"), "Zurich");
        assert_eq!(strip_latin_diacritics("café"), "cafe");
    }

    #[test]
    fn a_plain_ascii_word_is_returned_unchanged() {
        assert_eq!(strip_latin_diacritics("hello"), "hello");
    }

    #[test]
    fn cyrillic_marks_survive_and_stay_one_character() {
        // The asymmetry this function exists for: `remove_diacritics 2` does
        // not touch the Cyrillic breve, so `й` must come back whole — not
        // stripped to `и`, and not left permanently split into `и` plus a
        // trailing combining breve by the NFD/NFC round trip inside.
        let precomposed = "йод";
        let decomposed = "\u{0438}\u{0306}од";
        assert_eq!(strip_latin_diacritics(precomposed), "йод");
        assert_eq!(strip_latin_diacritics(decomposed), "йод");
    }

    #[test]
    fn a_letter_folds_by_its_decomposition_not_by_its_name() {
        // The rule is "NFD hands back an ASCII base". This test has to tell
        // that apart from "`ł`, `ø` and `æ` are spelled out somewhere", which
        // the previous version could not: it asserted only those three, so a
        // hardcoded triple satisfied it exactly.
        //
        // Each row is one atomic legacy letter and one look-alike from the
        // same script that *does* decompose. Nothing but the decomposition
        // separates them, and a list of names gets the second column right
        // while failing the first the moment the list is not exhaustive —
        // which it cannot be, since Unicode keeps assigning letters.
        for (atomic, decomposing, folded) in [
            ("ø", "ō", "o"),
            ("đ", "ď", "d"),
            ("ħ", "ĥ", "h"),
            ("ł", "ĺ", "l"),
        ] {
            assert_eq!(strip_latin_diacritics(atomic), atomic);
            assert_eq!(strip_latin_diacritics(decomposing), folded);
        }

        // The rest of the atomic set, none of which any hardcoded triple names.
        assert_eq!(strip_latin_diacritics("ŋðþœı"), "ŋðþœı");
        assert_eq!(strip_latin_diacritics("łódź"), "łodz");
        assert_eq!(strip_latin_diacritics("Ærø"), "Ærø");
    }

    #[test]
    fn a_mark_nfc_could_not_absorb_is_stripped_whatever_its_base() {
        // The Ukrainian stress accent, and the defect that named this round.
        // `о` (U+043E) has no precomposed acute form, so NFC leaves U+0301
        // standing — and the tokenizer deletes it without consulting the base,
        // storing `слово`. A predicate that demanded an ASCII base reported
        // `сло́во`: a term the index does not hold, in the language this
        // product is for.
        assert_eq!(strip_latin_diacritics("сло\u{0301}во"), "слово");

        // The same mark on an ASCII base, so a regression that fixed one half
        // by breaking the other cannot pass this test.
        assert_eq!(strip_latin_diacritics("wo\u{0301}rd"), "word");
    }

    #[test]
    fn a_mark_the_tokenizer_keeps_is_kept() {
        // U+0305 sits inside U+0300–U+0331 and is *not* in the tokenizer's
        // table — measured. Writing the predicate as the whole window instead
        // of the measured set would take it, and `search_terms` would then
        // report a term the index never stored: the same class of disagreement
        // this round is fixing, pointing the other way.
        assert_eq!(strip_latin_diacritics("о\u{0305}"), "о\u{0305}");
        assert_eq!(strip_latin_diacritics("a\u{0305}"), "a\u{0305}");
    }
}

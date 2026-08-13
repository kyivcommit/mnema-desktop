//! Unicode normalisation, owed on both sides of the pipeline.
//!
//! `remove_diacritics 2` — the tokenizer's own folding, `schema.sql` — converges
//! a decomposed and a precomposed spelling onto one token for Latin: measured,
//! `König` in both forms collapses. It does **not** for the product's own
//! language: NFC `й` (U+0439) and decomposed `и` (U+0438) + combining breve
//! (U+0306) tokenize as two different words, and so do the two forms of `ї`.
//! `remove_diacritics 2` does not touch the Cyrillic breve or diaeresis, so
//! nothing downstream folds them back together. D32.
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

/// Strips the diacritics `remove_diacritics 2` strips, and leaves everything
/// else — Cyrillic among it — untouched. The module doc comment above is the
/// measurement this follows: folds a decomposed-or-precomposed Latin accented
/// letter down to its bare base (`Zürich` and `Zurich` converge), but does
/// not touch the Cyrillic breve or diaeresis, so `й` and `ї` keep theirs.
///
/// `mnema-index::search_terms` is why this is callable rather than staying an
/// implementation detail of the tokenizer: it reports the string the index
/// actually stores as a term, and for a Latin word carrying a diacritic that
/// string has no diacritic in it.
///
/// NFD-decomposes, drops a combining mark that immediately follows an ASCII
/// base letter, and re-composes what is left. Re-composing matters for
/// exactly the case this module exists for: `й` NFD-decomposes to `и` plus a
/// combining breve too, and since its base is not ASCII the breve survives
/// this function untouched — but decomposed, until `nfc()` at the end puts it
/// back together into the one character the rest of this pipeline expects.
///
/// ASCII base, deliberately named that rather than "Latin": a combining mark
/// can also be the *whole point* of a word rather than a decoration on it —
/// Hebrew niqqud, a Devanagari virama — and `remove_diacritics` does not
/// touch those either, so stripping every mark regardless of its base would
/// fix accented Latin at the cost of breaking scripts this function was
/// never asked about.
///
/// `ł`, `ø` and `æ` are not this rule's exception; they are outside its
/// input entirely. Unicode gives each of them no canonical decomposition —
/// they are atomic legacy letters, not a base plus a stroke or a ligature
/// mark — so `nfd()` never hands this function a combining mark to drop for
/// any of them, and `remove_diacritics 2` leaves them alone for what is
/// measured to be the same reason: `mnema-index`'s
/// `search_terms_reports_the_terms_fts5_actually_stored` indexes `łódź` and
/// `Ærø` through the real tokenizer and reads back what it actually stored
/// — `łodz` and `ærø`, agreeing with this function exactly, `ó` and `ź`
/// (which do decompose to an ASCII base) folding while `ł` does not.
pub fn strip_latin_diacritics(s: &str) -> String {
    let mut base_is_ascii_letter = false;
    s.nfd()
        .filter(|&c| {
            if unicode_normalization::char::is_combining_mark(c) {
                !base_is_ascii_letter
            } else {
                base_is_ascii_letter = c.is_ascii_alphabetic();
                true
            }
        })
        .nfc()
        .collect()
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
    fn atomic_legacy_latin_letters_have_no_mark_to_strip() {
        // `ł`, `ø` and `æ` are not exceptions carved out for this function —
        // Unicode gives them no canonical decomposition at all, so `nfd()`
        // never produces a combining mark after them for this function to
        // drop. Measured to agree with the real tokenizer in
        // `mnema-index`'s `search_terms_reports_the_terms_fts5_actually_stored`:
        // `łódź` stores as `łodz` (only `ó`/`ź`, which do decompose, fold)
        // and `Ærø` stores as `ærø`.
        assert_eq!(strip_latin_diacritics("łódź"), "łodz");
        assert_eq!(strip_latin_diacritics("Ærø"), "Ærø");
    }
}

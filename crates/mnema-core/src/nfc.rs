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
}

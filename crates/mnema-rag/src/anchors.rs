//! The `<c>N</c>` citation-anchor grammar — a pure port of the server's
//! `app/rag/anchors.py`. No I/O.
//!
//! The model emits `<c>N</c>` anchors where `N` is a 1-based ordinal into the
//! candidate list shown in the prompt. Resolution happens AFTER generation, so
//! the model cannot fabricate citation metadata. `group_claims` and the
//! sentence-splitting helpers are NOT ported — they belong to the deferred
//! entailment-verify cycle.

use std::sync::LazyLock;

use regex::{Captures, Regex};

/// `<c>N</c>` with optional whitespace inside the tag (`app/rag/anchors.py:13`).
static ANCHOR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<c>\s*(\d+)\s*</c>").unwrap());

/// The two broken forms a loosely-aligned model drifts into: `<cN>` (no opening
/// `>`) and `<c>N>` (open tag, bare `>` for the close — observed live from
/// gemini-3.1-flash-lite). The optional `>?` matches both. Canonical `<c>N</c>`
/// is left untouched: its digit is followed by `<`, not the `>` this pattern
/// requires (`app/rag/anchors.py:14-20`).
static SHORTHAND_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<c>?\s*(\d+)\s*>").unwrap());

/// Two or more spaces, collapsed to one after an anchor is dropped
/// (`app/rag/anchors.py:63`).
static MULTISPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" {2,}").unwrap());

/// The `Nd` decimal-digit "zero" of every Unicode block. Each `Nd` run is a
/// contiguous ten codepoints, so a digit's value is `c - zero`. ASCII `0-9` is
/// handled by `to_digit` and omitted. Sourced to match the `\d` that `ANCHOR_RE`
/// uses — the shipped `regex-syntax` (Unicode 16.0), not a separate data set.
///
/// A later `regex` bump that adds `Nd` blocks would make `\d` match digits this
/// list misses; that does NOT silently drop a citation — the regression test
/// below fails loudly until the new block zeros are added here.
const ND_ZEROS: &[char] = &[
    '\u{0660}',  // Arabic-Indic
    '\u{06F0}',  // Extended Arabic-Indic
    '\u{07C0}',  // Nko
    '\u{0966}',  // Devanagari
    '\u{09E6}',  // Bengali
    '\u{0A66}',  // Gurmukhi
    '\u{0AE6}',  // Gujarati
    '\u{0B66}',  // Oriya
    '\u{0BE6}',  // Tamil
    '\u{0C66}',  // Telugu
    '\u{0CE6}',  // Kannada
    '\u{0D66}',  // Malayalam
    '\u{0DE6}',  // Sinhala Lith
    '\u{0E50}',  // Thai
    '\u{0ED0}',  // Lao
    '\u{0F20}',  // Tibetan
    '\u{1040}',  // Myanmar
    '\u{1090}',  // Myanmar Shan
    '\u{17E0}',  // Khmer
    '\u{1810}',  // Mongolian
    '\u{1946}',  // Limbu
    '\u{19D0}',  // New Tai Lue
    '\u{1A80}',  // Tai Tham Hora
    '\u{1A90}',  // Tai Tham Tham
    '\u{1B50}',  // Balinese
    '\u{1BB0}',  // Sundanese
    '\u{1C40}',  // Lepcha
    '\u{1C50}',  // Ol Chiki
    '\u{A620}',  // Vai
    '\u{A8D0}',  // Saurashtra
    '\u{A900}',  // Kayah Li
    '\u{A9D0}',  // Javanese
    '\u{A9F0}',  // Myanmar Tai Laing
    '\u{AA50}',  // Cham
    '\u{ABF0}',  // Meetei Mayek
    '\u{FF10}',  // Fullwidth
    '\u{104A0}', // Osmanya
    '\u{10D30}', // Hanifi Rohingya
    '\u{11066}', // Brahmi
    '\u{110F0}', // Sora Sompeng
    '\u{11136}', // Chakma
    '\u{111D0}', // Sharada
    '\u{112F0}', // Khudawadi
    '\u{11450}', // Newa
    '\u{114D0}', // Tirhuta
    '\u{11650}', // Modi
    '\u{116C0}', // Takri
    '\u{11730}', // Ahom
    '\u{118E0}', // Warang Citi
    '\u{11950}', // Dives Akuru
    '\u{11C50}', // Bhaiksuki
    '\u{11D50}', // Masaram Gondi
    '\u{11DA0}', // Gunjala Gondi
    '\u{11F50}', // Kawi
    '\u{16A60}', // Mro
    '\u{16AC0}', // Tangsa
    '\u{16B50}', // Pahawh Hmong
    '\u{1D7CE}', // Mathematical Bold
    '\u{1D7D8}', // Mathematical Double-Struck
    '\u{1D7E2}', // Mathematical Sans-Serif
    '\u{1D7EC}', // Mathematical Sans-Serif Bold
    '\u{1D7F6}', // Mathematical Monospace
    '\u{1E140}', // Nyiakeng Puachue Hmong
    '\u{1E2F0}', // Wancho
    '\u{1E4F0}', // Nag Mundari
    '\u{1E950}', // Adlam
    '\u{1FBF0}', // Segmented
    // Unicode 16.0 blocks (beyond the 15.1 set), discovered from the shipped
    // regex; kept complete by the regression test below.
    '\u{10D40}',
    '\u{116D0}',
    '\u{116DA}',
    '\u{11BF0}',
    '\u{16130}',
    '\u{16D70}',
    '\u{1CCF0}',
    '\u{1E5F1}',
];

/// One decimal digit's value 0-9, matching the server's Python `int()`: the ASCII
/// fast-path, then the `Nd` blocks in [`ND_ZEROS`].
fn decimal_digit_value(c: char) -> Option<u32> {
    if let Some(d) = c.to_digit(10) {
        return Some(d);
    }
    let cc = c as u32;
    ND_ZEROS
        .iter()
        .map(|&zero| zero as u32)
        .find(|&z| (z..z + 10).contains(&cc))
        .map(|z| cc - z)
}

/// Parse a run of decimal digits (any `Nd` script) to a `usize`, matching the
/// server's `int(m.group(1))` (`app/rag/anchors.py:56`). `None` on a non-digit
/// char (unreachable under `\d+`) or on overflow.
fn parse_ordinal(digits: &str) -> Option<usize> {
    let mut value: usize = 0;
    for c in digits.chars() {
        let d = decimal_digit_value(c)? as usize;
        value = value.checked_mul(10)?.checked_add(d)?;
    }
    Some(value)
}

/// Rewrite the two drift forms to canonical `<c>N</c>` so the strict logic below
/// resolves them (`app/rag/anchors.py:23-24`).
fn canonicalize(text: &str) -> String {
    SHORTHAND_RE.replace_all(text, "<c>${1}</c>").into_owned()
}

/// Distinct anchor ordinals in first-occurrence order, no range filtering
/// (`app/rag/anchors.py:34-42`). Used by the deferred verify cycle, not by the
/// answer seam.
pub fn extract_anchor_ids(text: &str) -> Vec<usize> {
    let text = canonicalize(text);
    let mut seen: Vec<usize> = Vec::new();
    for caps in ANCHOR_RE.captures_iter(&text) {
        if let Some(k) = parse_ordinal(&caps[1])
            && !seen.contains(&k)
        {
            seen.push(k);
        }
    }
    seen
}

/// Strip out-of-range anchors; return the cleaned text and the valid ordinals in
/// first-occurrence order. An ordinal is valid iff `1 <= N <= n_candidates`;
/// invalid (hallucinated or out of range) anchors are removed from the text so
/// they cannot reach the client (`app/rag/anchors.py:45-64`). The digit may be
/// any Unicode `Nd` script; a valid anchor is normalised to ASCII `<c>N</c>`.
pub fn resolve_anchors(text: &str, n_candidates: usize) -> (String, Vec<usize>) {
    let text = canonicalize(text);
    let mut valid: Vec<usize> = Vec::new();
    let clean = ANCHOR_RE.replace_all(&text, |caps: &Captures| match parse_ordinal(&caps[1]) {
        Some(k) if (1..=n_candidates).contains(&k) => {
            if !valid.contains(&k) {
                valid.push(k);
            }
            // Normalise to ASCII so a localised-digit anchor renders uniformly
            // downstream; the server keeps `m.group(0)` verbatim.
            format!("<c>{k}</c>")
        }
        _ => String::new(),
    });
    let clean = MULTISPACE_RE.replace_all(&clean, " ").into_owned();
    (clean, valid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_valid_canonical_anchors_and_returns_ordinals_in_order() {
        let (clean, ids) = resolve_anchors("The sky is blue<c>1</c> and wide<c>2</c>.", 3);
        assert_eq!(clean, "The sky is blue<c>1</c> and wide<c>2</c>.");
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn strips_an_out_of_range_anchor_from_the_text() {
        // 9 > 3 candidates: the anchor is removed, not merely absent from ids.
        let (clean, ids) = resolve_anchors("A fact<c>9</c> stands.", 3);
        assert_eq!(clean, "A fact stands.");
        assert!(ids.is_empty());
    }

    #[test]
    fn collapses_the_double_space_left_by_a_dropped_anchor() {
        let (clean, ids) = resolve_anchors("A <c>9</c> B", 3);
        assert_eq!(clean, "A B");
        assert!(ids.is_empty());
    }

    #[test]
    fn canonicalises_both_drift_forms_before_resolving() {
        let (clean_a, ids_a) = resolve_anchors("shorthand<c1>", 3);
        assert_eq!(clean_a, "shorthand<c>1</c>");
        assert_eq!(ids_a, vec![1]);

        let (clean_b, ids_b) = resolve_anchors("bare close<c>2>", 3);
        assert_eq!(clean_b, "bare close<c>2</c>");
        assert_eq!(ids_b, vec![2]);
    }

    #[test]
    fn deduplicates_ordinals_by_first_occurrence() {
        let (_clean, ids) = resolve_anchors("<c>2</c> a <c>1</c> b <c>2</c>", 3);
        assert_eq!(ids, vec![2, 1]);
    }

    #[test]
    fn zero_candidates_drops_every_anchor() {
        let (clean, ids) = resolve_anchors("x<c>1</c>y", 0);
        assert_eq!(clean, "xy");
        assert!(ids.is_empty());
    }

    #[test]
    fn extract_returns_distinct_ids_in_first_occurrence_order() {
        assert_eq!(extract_anchor_ids("<c>3</c><c>1</c><c>3</c>"), vec![3, 1]);
    }

    // Full-Unicode digit parity with the server's Python `int()` (the product's
    // `N*`-categories stance, D32): a model that localises the digit inside our
    // markup still resolves, and the anchor is normalised to ASCII in the answer.
    #[test]
    fn resolves_arabic_indic_digit_and_normalises_to_ascii() {
        let (clean, ids) = resolve_anchors("fact<c>\u{0661}</c>.", 3); // ١
        assert_eq!(clean, "fact<c>1</c>.");
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn resolves_eastern_arabic_devanagari_and_fullwidth_digits() {
        assert_eq!(resolve_anchors("<c>\u{06F2}</c>", 3).1, vec![2]); // ۲ Persian
        assert_eq!(resolve_anchors("<c>\u{0969}</c>", 3).1, vec![3]); // ३ Devanagari
        assert_eq!(resolve_anchors("<c>\u{FF11}</c>", 3).1, vec![1]); // １ fullwidth
    }

    #[test]
    fn out_of_range_localised_digit_is_still_dropped() {
        let (clean, ids) = resolve_anchors("a<c>\u{0669}</c>b", 3); // ٩ = 9 > 3
        assert_eq!(clean, "ab");
        assert!(ids.is_empty());
    }

    #[test]
    fn extract_reads_localised_digits() {
        assert_eq!(
            extract_anchor_ids("<c>\u{0663}</c><c>\u{0661}</c>"),
            vec![3, 1]
        ); // ٣ ١
    }

    #[test]
    fn every_digit_the_anchor_regex_matches_parses_to_its_value() {
        // `ANCHOR_RE` uses `\d` (Unicode `Nd`); `ND_ZEROS` must cover the same
        // Unicode version `regex` ships. A maximal run of matched digits is a
        // whole number of `Nd` blocks (ten contiguous, value 0-9), so the i-th
        // codepoint of the run has value `i % 10`.
        let digit = Regex::new(r"^\d$").unwrap();
        let is_digit = |cp: u32| char::from_u32(cp).is_some_and(|c| digit.is_match(&c.to_string()));
        let mut cp = 0u32;
        while cp <= 0x10FFFF {
            if !is_digit(cp) {
                cp += 1;
                continue;
            }
            let start = cp;
            while cp <= 0x10FFFF && is_digit(cp) {
                cp += 1;
            }
            let len = cp - start;
            assert_eq!(
                len % 10,
                0,
                "digit run at U+{start:04X} is not whole blocks"
            );
            for off in 0..len {
                let c = char::from_u32(start + off).unwrap();
                assert_eq!(
                    decimal_digit_value(c),
                    Some(off % 10),
                    "U+{:04X} must parse to {}",
                    start + off,
                    off % 10
                );
            }
        }
    }
}

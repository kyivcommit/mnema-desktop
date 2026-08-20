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

/// Rewrite the two drift forms to canonical `<c>N</c>` so the strict logic below
/// resolves them (`app/rag/anchors.py:23-24`).
fn canonicalize(text: &str) -> String {
    SHORTHAND_RE.replace_all(text, "<c>${1}</c>").into_owned()
}

/// Strip out-of-range anchors; return the cleaned text and the valid ordinals in
/// first-occurrence order. An ordinal is valid iff `1 <= N <= n_candidates`;
/// invalid (hallucinated or out of range) anchors are removed from the text so
/// they cannot reach the client (`app/rag/anchors.py:45-64`). A digit run too
/// large to parse is treated as out of range.
pub fn resolve_anchors(text: &str, n_candidates: usize) -> (String, Vec<usize>) {
    let text = canonicalize(text);
    let mut valid: Vec<usize> = Vec::new();
    let clean = ANCHOR_RE.replace_all(&text, |caps: &Captures| match caps[1].parse::<usize>() {
        Ok(k) if (1..=n_candidates).contains(&k) => {
            if !valid.contains(&k) {
                valid.push(k);
            }
            caps[0].to_owned()
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
}

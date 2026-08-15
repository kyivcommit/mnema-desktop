use mnema_search::{FusionRule, fuse};

/// The two arms alone are configurations of the product, not fusion rules —
/// D106's "text only" and "content only". They are in the same enum because the
/// sweep prints them as rows of the same table.
#[test]
fn a_single_arm_rule_ignores_the_other_arm_entirely() {
    let text = [7, 12, 3];
    let content = [12, 5, 7];

    assert_eq!(
        fuse(FusionRule::TextOnly, &text, &content, 10),
        vec![7, 12, 3]
    );
    assert_eq!(
        fuse(FusionRule::ContentOnly, &text, &content, 10),
        vec![12, 5, 7]
    );

    // The other direction: an empty other-arm changes nothing, which is what
    // makes these usable when an arm is off.
    assert_eq!(fuse(FusionRule::TextOnly, &text, &[], 10), vec![7, 12, 3]);
    assert_eq!(
        fuse(FusionRule::ContentOnly, &[], &content, 10),
        vec![12, 5, 7]
    );
}

/// `limit` cuts the fused list, and it cuts it after fusing rather than before:
/// a rule that truncated its inputs would score its own arms, not the fusion.
/// The `Rrf` case below is the discriminating one: cutting each arm to
/// `limit` first costs 1 and 3 their second contribution — the one from
/// the other arm, past the cut — dropping both to a single `1/61`; 2 loses
/// neither contribution and would end up on top instead.
#[test]
fn the_limit_cuts_the_fused_list_not_the_arms() {
    let text = [1, 2, 3, 4];
    assert_eq!(fuse(FusionRule::TextOnly, &text, &[], 2), vec![1, 2]);
    assert_eq!(fuse(FusionRule::TextOnly, &text, &[], 0), Vec::<i64>::new());

    let text = [1, 2, 3];
    let content = [3, 2, 1];
    assert_eq!(fuse(FusionRule::Rrf, &text, &content, 2), vec![1, 3]);
}

/// `ALL` is a hand-written array; a new variant still compiles without it.
/// Routing through this exhaustive match instead: a variant left off it
/// fails the test's own compilation, not just its assertion.
#[test]
fn all_lists_every_variant_in_declaration_order() {
    fn canonical(rule: &FusionRule) -> FusionRule {
        match rule {
            FusionRule::TextOnly => FusionRule::TextOnly,
            FusionRule::ContentOnly => FusionRule::ContentOnly,
            FusionRule::Rrf => FusionRule::Rrf,
            FusionRule::Interleave => FusionRule::Interleave,
            FusionRule::Cascade => FusionRule::Cascade,
        }
    }

    let order: Vec<FusionRule> = FusionRule::ALL.iter().map(canonical).collect();
    assert_eq!(
        order,
        vec![
            FusionRule::TextOnly,
            FusionRule::ContentOnly,
            FusionRule::Rrf,
            FusionRule::Interleave,
            FusionRule::Cascade,
        ]
    );
}

/// `label` is the row name the sweep prints; each variant gets a distinct,
/// stable string.
#[test]
fn label_names_every_variant() {
    assert_eq!(FusionRule::TextOnly.label(), "text-only");
    assert_eq!(FusionRule::ContentOnly.label(), "content-only");
    assert_eq!(FusionRule::Rrf.label(), "rrf");
    assert_eq!(FusionRule::Interleave.label(), "interleave");
    assert_eq!(FusionRule::Cascade.label(), "cascade");
}

/// Reciprocal rank fusion: a chunk high in BOTH lists outranks one high in a
/// single list. The example is the spec's own — 12 is second in one arm and
/// first in the other, and `TextOnly` alone would put 7 first, not 12.
#[test]
fn rrf_lifts_the_chunk_that_stands_high_in_both_arms() {
    let text = [7, 12, 3, 41];
    let content = [12, 5, 7, 33];

    let fused = fuse(FusionRule::Rrf, &text, &content, 4);
    assert_eq!(fused[0], 12, "12 places 2nd and 1st; 7 places 1st and 3rd");
    assert_eq!(fused[1], 7);

    // A chunk in one arm only still appears — fusion widens, it does not filter.
    assert!(fuse(FusionRule::Rrf, &text, &content, 10).contains(&41));
    assert!(fuse(FusionRule::Rrf, &text, &content, 10).contains(&33));
}

/// Alternates the arms, dropping a chunk the other arm already contributed.
#[test]
fn interleave_alternates_and_keeps_each_chunk_once() {
    let text = [7, 12, 3];
    let content = [12, 5, 7];

    assert_eq!(
        fuse(FusionRule::Interleave, &text, &content, 10),
        vec![7, 12, 5, 3]
    );

    // An exhausted arm does not stall the other: the longer list continues.
    assert_eq!(
        fuse(FusionRule::Interleave, &[1, 2, 3], &[9], 10),
        vec![1, 9, 2, 3]
    );
    assert_eq!(
        fuse(FusionRule::Interleave, &[9], &[1, 2, 3], 10),
        vec![9, 1, 2, 3]
    );
}

/// The whole text arm first, then whatever the content arm adds.
#[test]
fn cascade_exhausts_the_text_arm_before_the_content_arm() {
    let text = [7, 12, 3];
    let content = [12, 5, 7];

    assert_eq!(
        fuse(FusionRule::Cascade, &text, &content, 10),
        vec![7, 12, 3, 5]
    );

    // The order within each arm is the arm's own, and the second arm never
    // reorders the first — the discriminating case against a rule that merely
    // concatenated and re-sorted.
    assert_eq!(
        fuse(FusionRule::Cascade, &[3, 1], &[9, 2], 10),
        vec![3, 1, 9, 2]
    );
}

/// Ties are ordinary here, not an edge case: two chunks each appearing once at
/// the same position score identically. The server calls the tie-break a
/// correctness requirement (`app/search/hybrid.py:50`), and without it two runs
/// over the same data print different tables.
#[test]
fn equal_scores_are_broken_by_chunk_id_so_two_runs_agree() {
    let text = [50, 40];
    let content = [40, 50];
    // Each chunk scores 1/(60+1) + 1/(60+2). Only the id can separate them.
    assert_eq!(fuse(FusionRule::Rrf, &text, &content, 2), vec![40, 50]);
    assert_eq!(fuse(FusionRule::Rrf, &content, &text, 2), vec![40, 50]);
}

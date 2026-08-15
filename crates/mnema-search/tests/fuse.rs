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
#[test]
fn the_limit_cuts_the_fused_list_not_the_arms() {
    let text = [1, 2, 3, 4];
    assert_eq!(fuse(FusionRule::TextOnly, &text, &[], 2), vec![1, 2]);
    assert_eq!(fuse(FusionRule::TextOnly, &text, &[], 0), Vec::<i64>::new());
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

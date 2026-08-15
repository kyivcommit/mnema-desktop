mod support;

/// The sweep's shape, enumerated rather than counted — the count follows the
/// list, and a bare number would be a second definition of it.
///
/// text-only under each query rule; content-only once, because the query rule
/// does not reach it; and each fusion rule under each query rule.
#[test]
fn the_sweep_walks_every_pair_that_can_differ() {
    let (_c, questions, indexed) = support::small_fixture_with_vectors();
    let dense = support::canned_dense_answers(&questions);

    let sweep = mnema_eval::Sweep::run(&indexed, &questions, &dense).unwrap();

    let text_only: Vec<_> = sweep
        .rows
        .iter()
        .filter(|r| r.fusion == mnema_search::FusionRule::TextOnly)
        .collect();
    assert_eq!(text_only.len(), mnema_index::QueryRule::ALL.len());

    let content_only: Vec<_> = sweep
        .rows
        .iter()
        .filter(|r| r.fusion == mnema_search::FusionRule::ContentOnly)
        .collect();
    assert_eq!(
        content_only.len(),
        1,
        "the query rule cannot reach this arm"
    );

    for fusion in [
        mnema_search::FusionRule::Rrf,
        mnema_search::FusionRule::Interleave,
        mnema_search::FusionRule::Cascade,
    ] {
        let rows: Vec<_> = sweep.rows.iter().filter(|r| r.fusion == fusion).collect();
        assert_eq!(rows.len(), mnema_index::QueryRule::ALL.len(), "{fusion:?}");
    }

    // No row appears twice, which a filter-and-count check alone would miss.
    let mut pairs: Vec<_> = sweep.rows.iter().map(|r| (r.rule, r.fusion)).collect();
    let before = pairs.len();
    pairs.sort_by_key(|(rule, fusion)| (format!("{rule:?}"), format!("{fusion:?}")));
    pairs.dedup();
    assert_eq!(pairs.len(), before, "a pair was swept twice");
}

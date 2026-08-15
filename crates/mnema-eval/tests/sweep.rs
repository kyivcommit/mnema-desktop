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

    // The two checks above pass even if every row's `report` came from the
    // same call, as long as `rule`/`fusion` on the row still carry the loop's
    // labels — a mutant that hardcodes the arguments to `run_row` while
    // leaving `Row { rule, fusion, .. }` alone slips past them. These two
    // assert the labels describe a run that actually differed.
    let text_only_all_terms = sweep
        .rows
        .iter()
        .find(|r| {
            r.fusion == mnema_search::FusionRule::TextOnly
                && r.rule == mnema_index::QueryRule::AllTerms
        })
        .unwrap();
    let text_only_any_term = sweep
        .rows
        .iter()
        .find(|r| {
            r.fusion == mnema_search::FusionRule::TextOnly
                && r.rule == mnema_index::QueryRule::AnyTerm
        })
        .unwrap();
    assert_ne!(
        text_only_all_terms.report, text_only_any_term.report,
        "q-1's query has no chunk holding both terms, so AllTerms and AnyTerm \
         must not read the same"
    );
    assert_ne!(
        content_only[0].report, text_only_all_terms.report,
        "ContentOnly must read the content arm, not the text arm"
    );

    assert_eq!(sweep.embedded, dense.embedded);
    assert_eq!(sweep.total, dense.total);
}

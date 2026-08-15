/// The RRF constant, ported from the server: `app/config.py:243`. A starting
/// value of a port, free to move once measured — not a treaty.
pub const RRF_K: i64 = 60;

/// How deep each arm is asked before fusing, ported from `app/config.py:244`.
pub const CANDIDATES: i64 = 30;

/// How two arms' answers become one list.
///
/// `TextOnly` and `ContentOnly` are not fusions at all — they are D106's
/// other configurations, living here because the sweep prints them as rows
/// of one table. Pinned by
/// `a_single_arm_rule_ignores_the_other_arm_entirely`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FusionRule {
    TextOnly,
    ContentOnly,
    Rrf,
    Interleave,
    Cascade,
}

impl FusionRule {
    pub const ALL: [FusionRule; 5] = [
        FusionRule::TextOnly,
        FusionRule::ContentOnly,
        FusionRule::Rrf,
        FusionRule::Interleave,
        FusionRule::Cascade,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FusionRule::TextOnly => "text-only",
            FusionRule::ContentOnly => "content-only",
            FusionRule::Rrf => "rrf",
            FusionRule::Interleave => "interleave",
            FusionRule::Cascade => "cascade",
        }
    }
}

/// Cuts after fusing, never before: truncating the inputs would score the arms
/// rather than the rule. `Rrf`'s output is not a prefix of either arm, which
/// is what lets `the_limit_cuts_the_fused_list_not_the_arms` tell "after" from
/// "before" apart.
pub fn fuse(rule: FusionRule, text: &[i64], content: &[i64], limit: usize) -> Vec<i64> {
    let fused = match rule {
        FusionRule::TextOnly => text.to_vec(),
        FusionRule::ContentOnly => content.to_vec(),
        FusionRule::Rrf => rrf(text, content),
        FusionRule::Interleave => interleave(text, content),
        FusionRule::Cascade => cascade(text, content),
    };
    fused.into_iter().take(limit).collect()
}

/// Takes one from each arm in turn, text first, and skips a chunk already
/// taken. An exhausted arm does not stop the other. Pinned by
/// `interleave_alternates_and_keeps_each_chunk_once`.
fn interleave(text: &[i64], content: &[i64]) -> Vec<i64> {
    use std::collections::BTreeSet;

    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(text.len() + content.len());
    let push = |id: i64, out: &mut Vec<i64>, seen: &mut BTreeSet<i64>| {
        if seen.insert(id) {
            out.push(id);
        }
    };
    for i in 0..text.len().max(content.len()) {
        if let Some(&id) = text.get(i) {
            push(id, &mut out, &mut seen);
        }
        if let Some(&id) = content.get(i) {
            push(id, &mut out, &mut seen);
        }
    }
    out
}

/// Sums `1/(RRF_K + position)` over both arms, positions counted from one, and
/// breaks equal sums by the smaller chunk id. Pinned by
/// `rrf_lifts_the_chunk_that_stands_high_in_both_arms` and by
/// `equal_scores_are_broken_by_chunk_id_so_two_runs_agree`.
fn rrf(text: &[i64], content: &[i64]) -> Vec<i64> {
    use std::collections::BTreeMap;

    let mut score: BTreeMap<i64, f64> = BTreeMap::new();
    for arm in [text, content] {
        for (i, &id) in arm.iter().enumerate() {
            let position = (i + 1) as i64;
            *score.entry(id).or_insert(0.0) += 1.0 / (RRF_K + position) as f64;
        }
    }
    let mut ranked: Vec<(i64, f64)> = score.into_iter().collect();
    // `total_cmp` rather than `partial_cmp().unwrap()`: the sum of reciprocals
    // is never NaN, and an unwrap would be an assertion about arithmetic that
    // has no test. The id is the second key, which is what makes the order a
    // function of the inputs alone.
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    ranked.into_iter().map(|(id, _)| id).collect()
}

/// The text arm entire, in its own order, then the content arm's contributions
/// in theirs. Pinned by `cascade_exhausts_the_text_arm_before_the_content_arm`.
fn cascade(text: &[i64], content: &[i64]) -> Vec<i64> {
    use std::collections::BTreeSet;

    let mut seen: BTreeSet<i64> = BTreeSet::new();
    let mut out = Vec::with_capacity(text.len() + content.len());
    for &id in text.iter().chain(content) {
        if seen.insert(id) {
            out.push(id);
        }
    }
    out
}

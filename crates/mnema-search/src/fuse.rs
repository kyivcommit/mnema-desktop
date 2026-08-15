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
/// rather than the rule. Every rule here is still prefix-preserving, so a test
/// cannot yet tell "after" from "before" apart — task 8's `Rrf`, whose output
/// is not a prefix of either arm, is what will pin it.
pub fn fuse(rule: FusionRule, text: &[i64], content: &[i64], limit: usize) -> Vec<i64> {
    let fused = match rule {
        FusionRule::TextOnly => text.to_vec(),
        FusionRule::ContentOnly => content.to_vec(),
        FusionRule::Rrf => text.to_vec(),
        FusionRule::Interleave => text.to_vec(),
        FusionRule::Cascade => text.to_vec(),
    };
    fused.into_iter().take(limit).collect()
}

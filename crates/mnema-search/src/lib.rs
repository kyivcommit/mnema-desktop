//! The two arms of search, and the rule that puts their answers together.
//!
//! Depends on both `mnema-index` and `mnema-provider`, the same shape as
//! `mnema-embed`: the index cannot reach a network, and the provider knows
//! nothing about chunks.

mod content;
mod fuse;

pub use content::{Arms, ContentArm, Missing, Provider, TextArm, content_arm};
pub use fuse::{CANDIDATES, FusionRule, RRF_K, fuse};

use mnema_index::{Db, QueryRule};

/// One search: the two arms and the list they became.
///
/// Both arm states travel with the list rather than beside it — an empty list
/// means five different things, and only these fields tell them apart. Pinned
/// by `the_text_arm_still_answers_when_the_content_arm_cannot_be_asked`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub chunks: Vec<i64>,
    pub text: TextArm,
    pub content: ContentArm,
}

/// Asks each arm that is on, then fuses. An arm that is off contributes
/// nothing to the fused list. Pinned by
/// `an_arm_that_is_off_contributes_nothing_and_says_so`.
pub fn search(
    db: &Db,
    provider: Option<Provider>,
    query: &str,
    arms: Arms,
    rule: QueryRule,
    fusion: FusionRule,
    limit: i64,
) -> Result<Found, mnema_index::Error> {
    let text = if arms.text {
        TextArm::Answered {
            chunks: db.search_lexical_with(query, rule, CANDIDATES)?,
        }
    } else {
        TextArm::Off
    };
    let content = if arms.content {
        content_arm(db, provider, query, CANDIDATES)
    } else {
        ContentArm::Off
    };

    let text_chunks: &[i64] = match &text {
        TextArm::Answered { chunks } => chunks,
        TextArm::Off => &[],
    };
    let content_chunks: &[i64] = match &content {
        ContentArm::Answered { chunks, .. } => chunks,
        _ => &[],
    };

    Ok(Found {
        chunks: fuse(fusion, text_chunks, content_chunks, limit as usize),
        text,
        content,
    })
}

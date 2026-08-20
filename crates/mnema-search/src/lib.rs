//! The two arms of search, and the rule that puts their answers together.
//!
//! Depends on both `mnema-index` and `mnema-provider`, the same shape as
//! `mnema-embed`: the index cannot reach a network, and the provider knows
//! nothing about chunks.

mod content;
mod fuse;

pub use content::{
    Arms, ContentArm, ContentQuery, Missing, Provider, TextArm, content_arm, embed_query,
};
pub use fuse::{CANDIDATES, FusionRule, RRF_K, fuse};

use mnema_index::{Db, QueryRule};

/// One search: the two arms and the list they became.
///
/// Both arm states travel with the list rather than beside it. `Off`,
/// `NotConfigured`, `Failed`, and an `Answered` that found nothing are
/// each their own silence, and only these fields tell them apart. Pinned
/// by `an_arm_that_is_off_contributes_nothing_and_says_so`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub chunks: Vec<i64>,
    pub text: TextArm,
    pub content: ContentArm,
}

/// Asks each arm that is on, then fuses. Makes no network call: `content`
/// is already embedded by the caller, before any read snapshot opens
/// (`content::embed_query`'s own doc explains why) — a caller that must
/// tell `Off` apart from `NotConfigured`/`Failed` reports those itself.
/// `text_on` stands alone rather than folded into an `Arms`, so nothing
/// here can be asked to run the content arm while also being told it is
/// off. An arm that is off contributes nothing to the fused list, and a
/// real answer from either arm survives into it. Pinned by
/// `an_arm_that_is_off_contributes_nothing_and_says_so` and by
/// `the_content_arms_chunks_survive_into_the_fused_list`.
pub fn search(
    db: &Db,
    content: Option<ContentQuery>,
    query: &str,
    text_on: bool,
    rule: QueryRule,
    fusion: FusionRule,
    limit: i64,
) -> Result<Found, mnema_index::Error> {
    let text = if text_on {
        TextArm::Answered {
            chunks: db.search_lexical_with(query, rule, CANDIDATES)?,
        }
    } else {
        TextArm::Off
    };
    let content = match content {
        Some(ContentQuery { space_id, vector }) => {
            content::content_arm_answered(db, space_id, &vector, CANDIDATES)
        }
        None => ContentArm::Off,
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

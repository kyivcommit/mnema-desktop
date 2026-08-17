use mnema_index::Db;

/// What a search was asked to use. Both false is not refused here — the
/// invariant that at least one is on belongs to the window, which is where
/// a person can be told why, not to this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arms {
    pub text: bool,
    pub content: bool,
}

/// Where the content arm's embedding call goes.
///
/// Owns its strings rather than borrowing them. A search makes one network
/// call, so the copy costs nothing measurable — and a borrowed `base` turns
/// every `Provider { base: &mock.base(), .. }` at a call site into a
/// dropped-temporary error, which is a lifetime puzzle this type has no reason
/// to hand anyone.
#[derive(Clone)]
pub struct Provider {
    pub base: String,
    pub key: String,
}

/// Redacts `key` — nothing formats a `Provider` with `{:?}` today, but a
/// derived `Debug` is one `.unwrap()` away from printing it, and the same
/// module-doc argument `mnema-secrets` makes for its own `Error` applies
/// here. Pinned by `a_providers_debug_rendering_does_not_carry_the_key`.
impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Provider")
            .field("base", &self.base)
            .field("key", &"[redacted]")
            .finish()
    }
}

/// What the content arm needs and does not have — a missing key or a missing
/// model, named apart because each is fixed in a different place rather than
/// folded into one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Missing {
    NoKey,
    NoModel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextArm {
    Off,
    Answered { chunks: Vec<i64> },
}

/// The content arm's outcome. `Off`, `NotConfigured` and `Failed` are each
/// their own silence, and `Answered` with an empty list is none of them —
/// it answered. Pinned by `the_content_arms_silences_are_told_apart_by_type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentArm {
    Off,
    NotConfigured(Missing),
    Failed {
        reason: String,
    },
    /// `embedded` of `total` tells a full index from a partly built one.
    /// Pinned by
    /// `a_partly_embedded_space_says_how_much_of_the_index_it_saw`.
    Answered {
        chunks: Vec<i64>,
        embedded: i64,
        total: i64,
    },
}

/// `Db::knn_searchable`, which already narrows to chunks a search may show
/// — a real chunk row behind an `indexed` document, the same rule the
/// lexical arm applies — so an orphan or a hidden-status chunk cannot
/// reach here. Pinned by
/// `an_orphaned_neighbour_is_skipped_without_shortening_the_answer` and
/// `the_content_arm_and_the_lexical_arm_answer_about_the_same_documents`.
/// A failed lookup fails the call outright. Pinned by
/// `a_citation_lookup_that_fails_makes_the_arm_failed_not_merely_short`.
fn knn_live_chunks(
    db: &Db,
    space_id: i64,
    query: &[f32],
    k: i64,
) -> Result<Vec<i64>, mnema_index::Error> {
    Ok(db.knn_searchable(space_id, query, k, None)?.chunks)
}

/// A query already embedded, and the space it was embedded against — what
/// `search` needs to run the content arm without making a network call of
/// its own. `space_id` travels with `vector` rather than beside it: the two
/// are meaningless apart, and a caller cannot pass one without the other.
pub struct ContentQuery {
    pub space_id: i64,
    pub vector: Vec<f32>,
}

/// This crate's one network call — everything `content_arm` used to do
/// after resolving `model`, pulled out so a caller can run it before
/// opening any read snapshot. `search`'s own doc explains why: a snapshot
/// held through a network round trip blocks a writer's checkpoint for as
/// long as the provider takes to answer.
///
/// Rejects an empty vector answer as a failure, not an empty result — the
/// same refusal `content_arm` always made.
pub fn embed_query(provider: &Provider, model: &str, query: &str) -> Result<Vec<f32>, String> {
    let vectors = mnema_provider::embed(&provider.base, &provider.key, model, &[query.to_string()])
        .map_err(|e| e.to_string())?;
    vectors
        .into_iter()
        .next()
        .ok_or_else(|| "the provider answered with no vector".to_string())
}

/// `knn_live_chunks` plus how much of the index it could even see — the
/// half of `content_arm` that touches no network, so `search` can run it
/// on a vector a caller already resolved, inside its read snapshot.
///
/// A coverage count that cannot be read fails the whole arm rather than
/// being read as zero. Pinned by
/// `a_coverage_count_that_fails_makes_the_arm_failed_not_empty`.
pub(crate) fn content_arm_answered(db: &Db, space_id: i64, vector: &[f32], k: i64) -> ContentArm {
    let chunks = match knn_live_chunks(db, space_id, vector, k) {
        Ok(chunks) => chunks,
        Err(e) => {
            return ContentArm::Failed {
                reason: e.to_string(),
            };
        }
    };
    let embedded = match db.embedded_chunk_count(space_id) {
        Ok(n) => n,
        Err(e) => {
            return ContentArm::Failed {
                reason: e.to_string(),
            };
        }
    };
    let total = match db.chunk_count() {
        Ok(n) => n,
        Err(e) => {
            return ContentArm::Failed {
                reason: e.to_string(),
            };
        }
    };
    ContentArm::Answered {
        chunks,
        embedded,
        total,
    }
}

/// Embeds the query with the model the space was built with, then asks `knn`.
///
/// The model comes from `Db::space_model`, never from the settings' current
/// choice: a vector from another model is a coordinate on another map, and
/// `knn` compares it silently. Pinned by
/// `the_content_arm_refuses_a_model_that_is_not_the_spaces`.
/// Single-connection callers only — `mnema-eval`'s dense sweep, today's
/// one. `search` shares a connection with a job on another one, and uses
/// [`embed_query`] and [`content_arm_answered`] instead, split across its
/// own read snapshot.
pub fn content_arm(db: &Db, provider: Option<Provider>, query: &str, k: i64) -> ContentArm {
    let Some(provider) = provider else {
        return ContentArm::NotConfigured(Missing::NoKey);
    };
    let space = match db.active_space() {
        Ok(Some(space)) => space,
        Ok(None) => return ContentArm::NotConfigured(Missing::NoModel),
        Err(e) => {
            return ContentArm::Failed {
                reason: e.to_string(),
            };
        }
    };
    let (model, _width) = match db.space_model(space) {
        Ok(pair) => pair,
        Err(e) => {
            return ContentArm::Failed {
                reason: e.to_string(),
            };
        }
    };
    let vector = match embed_query(&provider, &model, query) {
        Ok(v) => v,
        Err(reason) => return ContentArm::Failed { reason },
    };
    content_arm_answered(db, space, &vector, k)
}

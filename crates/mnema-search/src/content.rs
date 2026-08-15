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
#[derive(Debug, Clone)]
pub struct Provider {
    pub base: String,
    pub key: String,
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

/// Embeds the query with the model the space was built with, then asks `knn`.
///
/// The model comes from `Db::space_model`, never from the settings' current
/// choice: a vector from another model is a coordinate on another map, and
/// `knn` compares it silently. Will be pinned by task 14's
/// `the_content_arm_refuses_a_model_that_is_not_the_spaces`, not yet written.
///
/// A coverage count that cannot be read fails the whole arm rather than
/// being read as zero. Pinned by
/// `a_coverage_count_that_fails_makes_the_arm_failed_not_empty`.
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
    let vectors =
        match mnema_provider::embed(&provider.base, &provider.key, &model, &[query.to_string()]) {
            Ok(v) => v,
            Err(e) => {
                return ContentArm::Failed {
                    reason: e.to_string(),
                };
            }
        };
    let Some(vector) = vectors.into_iter().next() else {
        return ContentArm::Failed {
            reason: "the provider answered with no vector".to_string(),
        };
    };
    let chunks = match db.knn(space, &vector, k, None) {
        Ok(chunks) => chunks,
        Err(e) => {
            return ContentArm::Failed {
                reason: e.to_string(),
            };
        }
    };
    let embedded = match db.embedded_chunk_count(space) {
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

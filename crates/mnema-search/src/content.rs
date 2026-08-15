/// What a search was asked to use. Both false is a state this crate answers
/// rather than refuses — the invariant that at least one is on belongs to the
/// window, which is where a person can be told why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arms {
    pub text: bool,
    pub content: bool,
}

/// Where the content arm's embedding call goes.
///
/// Owns its two strings rather than borrowing them. A search makes one network
/// call, so the copy costs nothing measurable — and a borrowed `base` turns
/// every `Provider { base: &mock.base(), .. }` at a call site into a
/// dropped-temporary error, which is a lifetime puzzle this type has no reason
/// to hand anyone.
#[derive(Debug, Clone)]
pub struct Provider {
    pub base: String,
    pub key: String,
}

/// What the content arm needs and does not have. Two variants rather than one
/// message, because the two are fixed in different places.
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

/// The content arm's outcome. `Off`, `NotConfigured` and `Failed` are three
/// different silences, and `Answered` with an empty list is a fourth. Pinned by
/// `the_content_arms_silences_are_told_apart_by_type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentArm {
    Off,
    NotConfigured(Missing),
    Failed {
        reason: String,
    },
    Answered {
        chunks: Vec<i64>,
        embedded: i64,
        total: i64,
    },
}

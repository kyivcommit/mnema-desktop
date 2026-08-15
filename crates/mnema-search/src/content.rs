/// What a search was asked to use. Both false is not refused here — the
/// invariant that at least one is on belongs to the window, not to this
/// type, which is where a person can be told why.
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
    Answered {
        chunks: Vec<i64>,
        embedded: i64,
        total: i64,
    },
}

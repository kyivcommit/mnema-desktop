//! Answer generation (RAG core): the seam that turns retrieved passages into a
//! cited answer. Pure + network-only — this crate knows nothing about chunks or
//! the database. This PR lands only the `<c>N</c>` anchor grammar.

mod anchors;

pub use anchors::{extract_anchor_ids, resolve_anchors};

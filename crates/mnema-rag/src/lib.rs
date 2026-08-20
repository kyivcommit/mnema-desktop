//! Answer generation (RAG core): the seam that turns retrieved passages into a
//! cited answer. Pure + network-only — this crate knows nothing about chunks or
//! the database. `anchors` holds the `<c>N</c>` grammar; `prompt` builds the
//! synthesis messages from `Passage`s and a question.

mod anchors;
mod prompt;

pub use anchors::{extract_anchor_ids, resolve_anchors};
pub use prompt::{Passage, build_messages};

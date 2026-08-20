//! Answer generation (RAG core): the seam that turns retrieved passages into a
//! cited answer. Pure + network-only — this crate knows nothing about chunks or
//! the database. `anchors` holds the `<c>N</c>` grammar; `prompt` builds the
//! synthesis messages from `Passage`s and a question; `answer` sends the
//! prompt and resolves what the model cites.

mod anchors;
mod answer;
mod prompt;

pub use anchors::{extract_anchor_ids, resolve_anchors};
pub use answer::{Answer, answer};
pub use prompt::{Passage, build_messages};

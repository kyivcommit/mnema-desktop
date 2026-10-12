//! Answer generation (RAG core): the seam that turns retrieved passages into a
//! cited answer. Pure + network-only — this crate knows nothing about chunks or
//! the database. `anchors` holds the `<c>N</c>` grammar; `prompt` builds the
//! synthesis messages from `Passage`s and a question; `answer` sends the
//! prompt and resolves what the model cites.

mod anchors;
mod answer;
mod dialogue;
mod lang;
mod prompt;

pub use anchors::{extract_anchor_ids, renumber_anchors, resolve_anchors, strip_anchors};
pub use answer::{Answer, answer};
pub use dialogue::{Profile, Turn, build_dialogue, profile, with_search_result};
pub use lang::{EN, Lang, UK, detect};
pub use prompt::{Passage, Voice, build_messages};

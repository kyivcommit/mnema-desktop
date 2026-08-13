//! The instrument that judges whether search is useful.
//!
//! It builds no search. It builds the corpus, the questions and the number, so
//! that the search cycle has something to be judged by before it starts.

mod gold;

pub use gold::{Gold, resolve_gold};

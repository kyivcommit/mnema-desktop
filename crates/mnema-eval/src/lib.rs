//! The instrument that judges whether search is useful.
//!
//! It builds no search. It builds the corpus, the questions and the number, so
//! that the search cycle has something to be judged by before it starts.

mod corpus;
mod dense;
mod embed;
mod gold;
mod indexed;
mod preflight;
mod questions;
mod report;
mod run;
mod sweep;
mod terms;

pub use corpus::{Corpus, Document, EvalError, Language, corpus_dir};
pub use dense::DenseAnswers;
pub use embed::{EVAL_MODEL, Embedded, embed_corpus};
pub use gold::{Gold, resolve_gold};
pub use indexed::IndexedCorpus;
pub use preflight::{Problem, preflight};
pub use questions::{Class, Question, QuestionSet, questions_path};
pub use report::Report;
pub use run::{Location, Outcome, SEARCH_LIMIT, run_lexical, run_lexical_with, run_row};
pub use sweep::{Row, Sweep};
pub use terms::{ClassVerdict, check_class, universal_terms};

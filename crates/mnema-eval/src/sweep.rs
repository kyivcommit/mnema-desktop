use mnema_index::QueryRule;
use mnema_search::FusionRule;

use crate::{DenseAnswers, EvalError, IndexedCorpus, QuestionSet, Report, run_row};

pub struct Row {
    pub rule: QueryRule,
    pub fusion: FusionRule,
    pub report: Report,
}

pub struct Sweep {
    pub rows: Vec<Row>,
    pub model: String,
    pub base: String,
}

impl Sweep {
    /// One row per pair of rules that can differ. `ContentOnly` takes a single
    /// row because the query rule never reaches the content arm: four identical
    /// rows would invite being read as four measurements. Pinned by
    /// `the_sweep_walks_every_pair_that_can_differ`.
    pub fn run(
        indexed: &IndexedCorpus,
        questions: &QuestionSet,
        dense: &DenseAnswers,
    ) -> Result<Sweep, EvalError> {
        let chunk_count = indexed.db().chunk_count()?;
        let mut rows = Vec::new();
        for fusion in FusionRule::ALL {
            let rules: &[QueryRule] = if fusion == FusionRule::ContentOnly {
                &[QueryRule::AllTerms]
            } else {
                &QueryRule::ALL
            };
            for &rule in rules {
                let outcomes = run_row(indexed, questions, rule, fusion, dense)?;
                rows.push(Row {
                    rule,
                    fusion,
                    report: Report::of(&outcomes, chunk_count),
                });
            }
        }
        Ok(Sweep {
            rows,
            model: dense.model.clone(),
            base: dense.base.clone(),
        })
    }
}

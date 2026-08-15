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
    /// Mirror `DenseAnswers::embedded`/`total`: the coverage the content arm
    /// was measured under, carried alongside `model`/`base` rather than
    /// dropped — the render this feeds is not this task's, but the number
    /// disappears here if it is not kept.
    pub embedded: i64,
    pub total: i64,
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
            embedded: dense.embedded,
            total: dense.total,
        })
    }

    /// The header names the model and the service, because a recall figure
    /// without them describes a configuration nobody can return to. Pinned by
    /// `the_table_names_the_model_and_the_service_in_its_header`.
    pub fn render(&self) -> String {
        use std::fmt::Write as _;

        let mut out = String::new();
        let _ = writeln!(out, "модель: {}", self.model);
        let _ = writeln!(out, "служба: {}\n", self.base);
        for row in &self.rows {
            let _ = writeln!(out, "=== {} / {} ===", row.rule.label(), row.fusion.label());
            let _ = write!(out, "{}", row.report.render());
            for class in crate::Class::ALL {
                let (text, content) = row.report.volume(class);
                let _ = writeln!(
                    out,
                    "{}обсяг за текстом {}  обсяг за вмістом {}",
                    crate::report::pad(class.as_str(), crate::report::LABEL_WIDTH),
                    volume_cell(text),
                    volume_cell(content),
                );
            }
            out.push('\n');
        }
        out
    }
}

fn volume_cell(v: Option<f64>) -> String {
    v.map_or_else(
        || crate::report::UNMEASURED.to_string(),
        |v| format!("{v:.1}"),
    )
}

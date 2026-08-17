use std::collections::BTreeMap;

use mnema_search::{ContentArm, Provider};

use crate::{EvalError, IndexedCorpus, QuestionSet};

/// What the content arm said about every question, taken once. The arm's
/// answer depends on the question and on the index, not on any rule a later
/// sweep varies — and `embedded`/`total` ride along with `model` and `base`
/// because they name the index's state at the moment of this one snapshot,
/// not at whenever a caller later reads it: the index could keep being
/// built in between, and a fresh read then would describe a different
/// moment than the one this run measured against.
/// Pinned by `every_question_reaches_the_provider_exactly_once` and
/// `the_answers_carry_how_much_of_the_index_was_embedded`.
#[derive(Debug)]
pub struct DenseAnswers {
    by_question: BTreeMap<String, Vec<i64>>,
    pub model: String,
    pub base: String,
    pub embedded: i64,
    pub total: i64,
}

impl DenseAnswers {
    pub fn ask(
        indexed: &IndexedCorpus,
        questions: &QuestionSet,
        provider: Provider,
    ) -> Result<DenseAnswers, EvalError> {
        let db = indexed.db();
        let space = db.active_space()?.ok_or(EvalError::NoActiveSpace)?;
        let (model, _width) = db.space_model(space)?;
        let mut by_question = BTreeMap::new();
        let mut embedded = 0;
        let mut total = 0;
        for q in &questions.questions {
            match mnema_search::content_arm(
                db,
                Some(provider.clone()),
                &q.text,
                mnema_search::CANDIDATES,
            ) {
                ContentArm::Answered {
                    chunks,
                    embedded: e,
                    total: t,
                    ..
                } => {
                    by_question.insert(q.id.clone(), chunks);
                    embedded = e;
                    total = t;
                }
                // Ends the whole run rather than skipping the question: a
                // sweep table mixing questions the content arm answered
                // with ones it stayed silent on would describe two
                // different measurements as one row. Pinned by
                // `a_failed_content_arm_ends_the_run`.
                other => return Err(EvalError::ContentArmSilent(format!("{other:?}"))),
            }
        }
        Ok(DenseAnswers {
            by_question,
            model,
            base: provider.base,
            embedded,
            total,
        })
    }

    /// No calls and no answers, for a caller that asks no provider.
    pub fn empty() -> DenseAnswers {
        DenseAnswers {
            by_question: BTreeMap::new(),
            model: "none".to_string(),
            base: "none".to_string(),
            embedded: 0,
            total: 0,
        }
    }

    /// The chunks this question's content arm returned — an empty slice
    /// where it returned none, and the same where the question was never
    /// asked.
    pub fn of(&self, question_id: &str) -> &[i64] {
        self.by_question.get(question_id).map_or(&[], |v| v)
    }

    /// Built straight from data, with no provider and no index behind it —
    /// what a test hands `run_row` in place of a live `ask`.
    pub fn canned(by_question: BTreeMap<String, Vec<i64>>) -> DenseAnswers {
        DenseAnswers {
            by_question,
            model: "none".to_string(),
            base: "none".to_string(),
            embedded: 0,
            total: 0,
        }
    }
}

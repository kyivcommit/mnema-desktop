use std::collections::BTreeMap;

use mnema_search::{ContentArm, Provider};

use crate::{EvalError, IndexedCorpus, QuestionSet};

/// What the content arm said about every question, taken once. The arm's
/// answer depends on the question and on the index, not on any rule a later
/// sweep varies. Pinned by `every_question_reaches_the_provider_exactly_once`.
pub struct DenseAnswers {
    by_question: BTreeMap<String, Vec<i64>>,
    pub model: String,
    pub base: String,
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
        for q in &questions.questions {
            match mnema_search::content_arm(
                db,
                Some(provider.clone()),
                &q.text,
                mnema_search::CANDIDATES,
            ) {
                ContentArm::Answered { chunks, .. } => {
                    by_question.insert(q.id.clone(), chunks);
                }
                other => return Err(EvalError::ContentArmSilent(format!("{other:?}"))),
            }
        }
        Ok(DenseAnswers {
            by_question,
            model,
            base: provider.base,
        })
    }

    /// No calls and no answers, for a caller that asks no provider.
    pub fn empty() -> DenseAnswers {
        DenseAnswers {
            by_question: BTreeMap::new(),
            model: "none".to_string(),
            base: "none".to_string(),
        }
    }

    /// The chunks this question's content arm returned — an empty slice
    /// where it returned none, and the same where the question was never
    /// asked.
    pub fn of(&self, question_id: &str) -> &[i64] {
        self.by_question.get(question_id).map_or(&[], |v| v)
    }
}

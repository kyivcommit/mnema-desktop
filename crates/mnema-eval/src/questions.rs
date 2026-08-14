use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::gold::canonical;
use crate::{EvalError, Language};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Literal,
    Paraphrase,
    Topical,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Literal => "literal",
            Class::Paraphrase => "paraphrase",
            Class::Topical => "topical",
        }
    }

    pub const ALL: [Class; 3] = [Class::Literal, Class::Paraphrase, Class::Topical];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub id: String,
    pub language: Language,
    pub class: Class,
    pub text: String,
    pub document: String,
    pub answers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionSet {
    pub questions: Vec<Question>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    id: String,
    language: String,
    class: String,
    text: String,
    document: String,
    answers: Vec<String>,
}

/// The question set shipped with this crate.
pub fn questions_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("questions.jsonl")
}

impl QuestionSet {
    pub fn load(path: &Path) -> Result<QuestionSet, EvalError> {
        let text = std::fs::read_to_string(path)?;
        let mut questions: Vec<Question> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();

        // One JSON object per line, not a JSON array: a diff that adds one
        // question is one added line, so the authoring tasks appending to
        // this file can never conflict with each other by construction.
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let row: Row = serde_json::from_str(line)
                .map_err(|e| EvalError::Questions(format!("line {}: {e}", n + 1)))?;
            let refuse = |why: &str| EvalError::Questions(format!("{}: {why}", row.id));

            // `Corpus::load`'s own mapping, called rather than copied: a third
            // language added to one of two copies would compile.
            let Some(language) = Language::parse(&row.language) else {
                return Err(EvalError::Questions(format!(
                    "{}: {} is not a language",
                    row.id, row.language
                )));
            };
            let class = match row.class.as_str() {
                "literal" => Class::Literal,
                "paraphrase" => Class::Paraphrase,
                "topical" => Class::Topical,
                other => {
                    return Err(EvalError::Questions(format!(
                        "{}: {other} is not a class",
                        row.id
                    )));
                }
            };
            if !seen.insert(row.id.clone()) {
                return Err(refuse("this id is already used"));
            }
            if row.text.trim().is_empty() {
                return Err(refuse("the question text is empty"));
            }
            if row.answers.iter().any(|a| a.trim().is_empty()) {
                return Err(refuse("an answer sentence is empty"));
            }
            // Canonicalised the same way `resolve_gold` compares
            // (`a_sentence_in_two_chunks_names_both`'s reasoning applies here
            // too): two answers differing only in whitespace still name the
            // same chunk. Pinned by
            // answer_sentences_that_canonicalise_the_same_are_still_a_duplicate
            let unique: BTreeSet<String> = row.answers.iter().map(|a| canonical(a)).collect();
            if unique.len() != row.answers.len() {
                return Err(refuse("two answer sentences are the same"));
            }
            // Literal and paraphrase carry exactly one answer sentence;
            // topical carries up to three. Pinned by
            // `a_literal_question_carries_exactly_one_answer`,
            // `a_paraphrase_question_carries_exactly_one_answer`,
            // `a_topical_question_carries_one_to_three_answers`.
            let allowed = match class {
                Class::Topical => 1..=3,
                Class::Literal | Class::Paraphrase => 1..=1,
            };
            if !allowed.contains(&row.answers.len()) {
                return Err(refuse(
                    "the number of answer sentences is not allowed for this class",
                ));
            }

            questions.push(Question {
                id: row.id,
                language,
                class,
                text: row.text,
                document: row.document,
                answers: row.answers,
            });
        }
        Ok(QuestionSet { questions })
    }
}

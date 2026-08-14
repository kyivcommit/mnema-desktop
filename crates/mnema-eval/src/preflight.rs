use std::collections::BTreeSet;

use mnema_index::DocumentStatus;
use mnema_ingest::StopReason;

use crate::{
    ClassVerdict, Corpus, EvalError, Gold, IndexedCorpus, QuestionSet, check_class, resolve_gold,
    universal_terms,
};

fn index_error(e: mnema_index::Error) -> EvalError {
    EvalError::Index(e.to_string())
}

/// Something that would make a score describe an input defect rather than
/// search. Every variant is one row of the spec's "what can quietly lie" table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    DocumentMissing {
        question: String,
        document: String,
    },
    DocumentNotIndexed {
        question: String,
        document: String,
        status: String,
    },
    SentenceNotFound {
        question: String,
        sentence: String,
    },
    SentenceInSeveralChunks {
        question: String,
        sentence: String,
        chunks: Vec<i64>,
    },
    SentenceInAnotherDocument {
        question: String,
        sentence: String,
        other: String,
    },
    AnswersShareAChunk {
        question: String,
        chunk: i64,
    },
    ClassViolated {
        question: String,
        shared: Vec<String>,
    },
    WalkStoppedEarly {
        reason: String,
    },
    WalkSkippedFiles {
        skipped: u64,
        refused: u64,
    },
}

/// Every reason the corpus and the question set are not yet worth scoring.
///
/// The order is fixed, because the list goes into a report: the walk first,
/// then the questions in their own order, and inside a question its answer
/// sentences in theirs. Pinned by
/// `every_problem_names_the_question_it_came_from`.
pub fn preflight(
    corpus: &Corpus,
    questions: &QuestionSet,
    indexed: &IndexedCorpus,
) -> Result<Vec<Problem>, EvalError> {
    let mut problems = Vec::new();

    let report = indexed.report();
    // `IndexedCorpus::build` refuses an early stop itself (`indexed.rs:90`), so
    // today this branch cannot fire through that constructor. It stays because
    // the counts it guards are the ones every question below is read against,
    // and a second way to hand this function a report must not walk past it.
    if report.stopped != StopReason::Completed {
        problems.push(Problem::WalkStoppedEarly {
            reason: format!("{:?}", report.stopped),
        });
    }
    if report.skipped != 0 || report.refused != 0 {
        problems.push(Problem::WalkSkippedFiles {
            skipped: report.skipped,
            refused: report.refused,
        });
    }

    let universal = universal_terms(corpus);
    let nothing = BTreeSet::new();

    for q in &questions.questions {
        // A question whose document never reached the index has one cause and
        // would otherwise grow one derived problem per answer sentence, plus a
        // class verdict read against chunks that are not there. Both document
        // branches therefore end the question.
        let Some(document_id) = indexed.document_id(&q.document)? else {
            problems.push(Problem::DocumentMissing {
                question: q.id.clone(),
                document: q.document.clone(),
            });
            continue;
        };
        let status = indexed
            .db()
            .document_status(&document_id)
            .map_err(index_error)?;
        if status != DocumentStatus::Indexed {
            problems.push(Problem::DocumentNotIndexed {
                question: q.id.clone(),
                document: q.document.clone(),
                status: status.as_str().to_string(),
            });
            continue;
        }

        if let ClassVerdict::Violated { shared } =
            check_class(q, universal.get(&q.language).unwrap_or(&nothing))
        {
            problems.push(Problem::ClassViolated {
                question: q.id.clone(),
                shared,
            });
        }

        let chunks = indexed
            .db()
            .chunks_of_document(&document_id)
            .map_err(index_error)?;
        let mut claimed: Vec<i64> = Vec::new();
        for sentence in &q.answers {
            match resolve_gold(&chunks, sentence) {
                Gold::Missing => problems.push(Problem::SentenceNotFound {
                    question: q.id.clone(),
                    sentence: sentence.clone(),
                }),
                Gold::Several(chunks) => problems.push(Problem::SentenceInSeveralChunks {
                    question: q.id.clone(),
                    sentence: sentence.clone(),
                    chunks,
                }),
                Gold::One(chunk) => {
                    if claimed.contains(&chunk) {
                        problems.push(Problem::AnswersShareAChunk {
                            question: q.id.clone(),
                            chunk,
                        });
                    }
                    claimed.push(chunk);
                }
            }

            // The near-miss row, and the expensive one: chunks of every other
            // document of the corpus, for every answer sentence. The brief
            // says not to optimise the numbers — twenty documents against
            // thirty questions is what this has to carry.
            for other in &corpus.documents {
                if other.id == q.document {
                    continue;
                }
                // A corpus document with no path row is not this question's
                // problem, and inventing one here would name the wrong
                // question in the report.
                let Some(other_id) = indexed.document_id(&other.id)? else {
                    continue;
                };
                let other_chunks = indexed
                    .db()
                    .chunks_of_document(&other_id)
                    .map_err(index_error)?;
                if resolve_gold(&other_chunks, sentence) != Gold::Missing {
                    problems.push(Problem::SentenceInAnotherDocument {
                        question: q.id.clone(),
                        sentence: sentence.clone(),
                        other: other.id.clone(),
                    });
                }
            }
        }
    }

    Ok(problems)
}

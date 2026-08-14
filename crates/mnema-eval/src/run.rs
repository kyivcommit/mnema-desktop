use crate::{Class, EvalError, Gold, IndexedCorpus, Question, QuestionSet, resolve_gold};

fn index_error(e: mnema_index::Error) -> EvalError {
    EvalError::Index(e.to_string())
}

/// How many chunks one question is allowed to see, and the ceiling every
/// `recall@k` in the report is read under.
///
/// The same twenty the application asks for (`src-tauri/src/bridge.rs:24`).
/// Naming a larger number here would measure a search the product does not
/// run, and a smaller one would report a miss the person would not have had.
///
/// Copied, not shared, and nothing fails when the two drift: this crate must
/// not be depended on by shipping code, and depending on the binary from here
/// would invert that. The bridge calls its own number a placeholder for the
/// search/RAG spec to settle, so that spec moves both or neither.
pub const SEARCH_LIMIT: i64 = 20;

/// What one question got back, and where in it the right answer was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The question's id, as `Problem` also names it — not its text.
    pub question: String,
    pub class: Class,
    /// 1-based position of the best-placed gold chunk, or `None`.
    ///
    /// A position rather than a boolean, because spec §7 reads `recall@1`,
    /// `@5` and `@20` off one run. "Found it" would have forced a second run
    /// per k.
    pub rank: Option<usize>,
    /// What came back instead, in the order search returned it. This is what
    /// lets the report list the failures with something to look at, rather
    /// than only counting them.
    pub returned: Vec<i64>,
    pub gold: Vec<i64>,
}

/// Asks the lexical search every question, in order, one outcome each.
///
/// Refuses — rather than scores — a question whose gold does not resolve: a
/// document not in the index, an answer sentence in no chunk, an answer
/// sentence in two. Each is an input defect that task 9's `preflight` reports
/// before anything is scored (`preflight.rs:117-152`), so meeting one here
/// means preflight was not run. Scoring it anyway would charge search for the
/// defect — gold that is empty or ambiguous ranks nowhere, and the
/// configuration would read worse than it is — while skipping it would shorten
/// the outcome list without saying so, which is the same lie under a smaller
/// denominator.
pub fn run_lexical(
    indexed: &IndexedCorpus,
    questions: &QuestionSet,
) -> Result<Vec<Outcome>, EvalError> {
    let mut outcomes = Vec::with_capacity(questions.questions.len());
    for q in &questions.questions {
        let gold = gold_chunks(indexed, q)?;
        // The question goes in exactly as a person would have typed it: not
        // rewritten, not narrowed, not routed through `search_terms`. A
        // sentence-shaped query that is unreachable under FTS5's implicit AND
        // is the thing being measured (spec §2), not a defect for the
        // instrument to route around.
        let returned = indexed
            .db()
            .search_lexical(&q.text, SEARCH_LIMIT)
            .map_err(index_error)?;
        // The first gold chunk to appear is by construction the best-placed
        // one, so scanning `returned` once answers "at least one of them, and
        // how far down".
        let rank = returned
            .iter()
            .position(|id| gold.contains(id))
            .map(|i| i + 1);
        outcomes.push(Outcome {
            question: q.id.clone(),
            class: q.class,
            rank,
            returned,
            gold,
        });
    }
    Ok(outcomes)
}

/// The gold chunks of one question, in the order its answer sentences are
/// listed.
///
/// Chunks of the question's **own** document, and of no other. That an answer
/// sentence does not also lie in some other document of the corpus is task 9's
/// `Problem::SentenceInAnotherDocument` (`preflight.rs:182`); here it is
/// assumed, not paid for again on every question.
fn gold_chunks(indexed: &IndexedCorpus, q: &Question) -> Result<Vec<i64>, EvalError> {
    let refuse = |why: String| EvalError::Questions(format!("{}: {why}; preflight names it", q.id));

    let Some(document_id) = indexed.document_id(&q.document)? else {
        return Err(refuse(format!("{} is not in the index", q.document)));
    };
    let chunks = indexed
        .db()
        .chunks_of_document(&document_id)
        .map_err(index_error)?;

    let mut gold = Vec::with_capacity(q.answers.len());
    for sentence in &q.answers {
        match resolve_gold(&chunks, sentence) {
            Gold::One(chunk) => gold.push(chunk),
            Gold::Missing => {
                return Err(refuse(format!("no chunk holds {sentence:?}")));
            }
            Gold::Several(chunks) => {
                return Err(refuse(format!("{sentence:?} lies in chunks {chunks:?}")));
            }
        }
    }
    Ok(gold)
}

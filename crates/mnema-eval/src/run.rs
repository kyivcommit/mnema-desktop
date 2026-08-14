use crate::{Class, EvalError, Gold, IndexedCorpus, Question, QuestionSet, resolve_gold};

/// How many chunks one question is allowed to see, and the ceiling every
/// `recall@k` in the report is read under.
///
/// The same twenty the application asks for (`src-tauri/src/bridge.rs:24`),
/// copied and not shared: that constant is private, and depending on the
/// binary from here would invert the rule that nothing shipping depends on
/// this crate. Nothing fails when the two drift — the bridge calls its own
/// number a placeholder for the search/RAG spec to settle, so that spec moves
/// both or neither.
pub const SEARCH_LIMIT: i64 = 20;

/// Where one returned chunk sits, read out of the index that returned it.
///
/// Resolved during the run rather than at render time, because neither half of
/// a chunk id outlives it: the index is a temporary directory the process
/// deletes on its way out, and the next run reassigns the ids. Spec §7 asks
/// the failure list for the path and the first lines for exactly that reason.
/// Pinned by `a_returned_chunk_is_reported_with_its_path_and_first_line`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// The document's path relative to the corpus root.
    pub path: String,
    /// The chunk's first line with anything on it — enough to tell which
    /// near-miss document came back, without reprinting the chunk.
    pub first_line: String,
}

/// What one question got back, and where in it the right answer was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The question's id, as `Problem` also names it — not its text.
    pub question: String,
    /// The question's own class, which spec §7 groups the report's table by.
    /// Pinned by `an_outcome_carries_the_class_of_its_question`.
    pub class: Class,
    /// 1-based position of the best-placed gold chunk, or `None`. A position
    /// rather than a boolean, because spec §7 reads `recall@1`, `@5` and `@20`
    /// off one run. Pinned by
    /// `a_question_whose_words_are_all_in_the_gold_chunk_finds_it_first`,
    /// which asserts `Some(1)` and would see a 0-based one.
    pub rank: Option<usize>,
    /// What came back instead, in the order search returned it, written even
    /// when no gold chunk is among it — that is what lets the report show a
    /// failure rather than only count it. Pinned by
    /// `a_chunk_that_is_returned_but_is_not_gold_does_not_count`.
    pub returned: Vec<i64>,
    /// Where each of `returned` is, in that same order. `None` for a chunk the
    /// index cannot place — no row under the id, or a document with no
    /// recorded path — neither of which can happen to a chunk this same run
    /// just returned. Pinned by
    /// `a_chunk_the_index_cannot_place_says_so_instead_of_a_bare_number`.
    pub returned_locations: Vec<Option<Location>>,
    pub gold: Vec<i64>,
}

/// Asks the lexical search every question, in order, one outcome each. Pinned
/// by `every_question_produces_exactly_one_outcome_in_order`.
///
/// A question whose gold does not resolve is refused, not scored: gold that
/// ranks nowhere would charge search for a fixture defect, and skipping would
/// shorten the list a `recall@k` divides by. `preflight` reports those three
/// first (`preflight.rs:113-119`, `:134-142`); the fourth — a document that
/// never reached `indexed` (`:121-128`) — stays there alone. Pinned by
/// `a_question_whose_answer_is_in_no_chunk_is_refused_not_scored` and
/// `a_question_naming_a_document_that_is_not_there_is_refused_not_scored`.
pub fn run_lexical(
    indexed: &IndexedCorpus,
    questions: &QuestionSet,
) -> Result<Vec<Outcome>, EvalError> {
    let mut outcomes = Vec::with_capacity(questions.questions.len());
    for q in &questions.questions {
        let gold = gold_chunks(indexed, q)?;
        // The question goes in exactly as a person would have typed it: not
        // rewritten, not narrowed, not routed through `search_terms`. A
        // sentence-shaped query unreachable under FTS5's implicit AND is the
        // thing being measured (spec §2), not a defect to route around.
        // Pinned by
        // `a_question_no_chunk_answers_has_no_rank_and_says_what_came_back`.
        let returned = indexed.db().search_lexical(&q.text, SEARCH_LIMIT)?;
        let returned_locations = locate(indexed, &returned)?;
        // `position` stops at the first gold chunk to appear, which is the
        // best-placed one — the rank is over ALL gold chunks, not just the
        // first answer's. Pinned by
        // `a_rank_is_the_best_placed_gold_chunk_not_the_first_answers`.
        let rank = returned
            .iter()
            .position(|id| gold.contains(id))
            .map(|i| i + 1);
        outcomes.push(Outcome {
            question: q.id.clone(),
            class: q.class,
            rank,
            returned,
            returned_locations,
            gold,
        });
    }
    Ok(outcomes)
}

/// Places every returned chunk against the index that just returned it.
///
/// A chunk the index cannot place is carried as `None` rather than refused:
/// this is the diagnostic column of a failure list, and losing the whole run
/// over one unreadable row would cost more than the row is worth.
fn locate(indexed: &IndexedCorpus, returned: &[i64]) -> Result<Vec<Option<Location>>, EvalError> {
    let mut located = Vec::with_capacity(returned.len());
    for &id in returned {
        located.push(indexed.db().citation(id)?.and_then(|citation| {
            let first_line = first_line(&citation.text);
            citation
                .relative_path
                .map(|path| Location { path, first_line })
        }));
    }
    Ok(located)
}

/// The chunk's first line with anything on it, trimmed; empty when it has
/// none. Text without a newline is one line, so it answers with the whole of
/// it.
fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The gold chunks of one question, in the order its answer sentences are
/// listed.
///
/// Chunks of the question's **own** document, and of no other. That an answer
/// sentence does not also lie in some other document of the corpus is task 9's
/// `Problem::SentenceInAnotherDocument` (`preflight.rs:169`); here it is
/// assumed, not paid for again on every question.
fn gold_chunks(indexed: &IndexedCorpus, q: &Question) -> Result<Vec<i64>, EvalError> {
    let refuse = |why: String| EvalError::Questions(format!("{}: {why}; preflight names it", q.id));

    let Some(document_id) = indexed.document_id(&q.document)? else {
        return Err(refuse(format!("{} is not in the index", q.document)));
    };
    let chunks = indexed.db().chunks_of_document(&document_id)?;

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

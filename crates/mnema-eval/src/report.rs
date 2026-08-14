use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::{Class, Outcome, SEARCH_LIMIT};

/// The positions every class is read at. The last one is `SEARCH_LIMIT` rather
/// than a literal twenty: no rank can exceed the number of chunks search was
/// asked for, so a column past it would print the same figure as the one
/// before it and read as a measurement. Pinned by
/// `every_number_is_printed_beside_its_chance_level`, whose 28.6% is 20/70.
const KS: [usize; 3] = [1, 5, SEARCH_LIMIT as usize];

/// What a class with no questions prints where the numbers would go — the
/// spelling of "nothing was measured", which a zero would misreport as "every
/// question of this class failed".
const UNMEASURED: &str = "недоступно";

const LABEL_WIDTH: usize = 15;
const CELL_WIDTH: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Outcomes grouped by class. `BTreeMap`, so the list of failed questions
    /// comes out in class order every run and a printed report can be diffed
    /// against the last. It does not order the table: `render` walks
    /// `Class::ALL`, which also reaches a class this map has no key for.
    pub by_class: BTreeMap<Class, Vec<Outcome>>,
    /// Chunks in the index this run measured — the denominator of the chance
    /// level, and the reason it is carried rather than recomputed.
    pub chunk_count: i64,
}

impl Report {
    pub fn of(outcomes: &[Outcome], chunk_count: i64) -> Report {
        let mut by_class: BTreeMap<Class, Vec<Outcome>> = BTreeMap::new();
        for outcome in outcomes {
            by_class
                .entry(outcome.class)
                .or_default()
                .push(outcome.clone());
        }
        Report {
            by_class,
            chunk_count,
        }
    }

    /// The share of this class's questions that placed a gold chunk at
    /// position `k` or better, or `None` when the class was never asked.
    ///
    /// A question with no rank stays in the denominator — it is a question
    /// search failed, not a question that was not put. Pinned by
    /// `a_question_with_no_rank_counts_against_every_k`, and the `None` by
    /// `a_class_with_no_questions_has_no_recall_rather_than_zero`.
    pub fn recall_at(&self, class: Class, k: usize) -> Option<f64> {
        let outcomes = self.by_class.get(&class).filter(|o| !o.is_empty())?;
        let found = outcomes
            .iter()
            .filter(|o| o.rank.is_some_and(|rank| rank <= k))
            .count();
        Some(found as f64 / outcomes.len() as f64)
    }

    /// The whole report as one block of text: the table, what the numbers
    /// would be worth by chance, the configurations that do not exist yet, and
    /// the questions that failed.
    ///
    /// The failures are part of it rather than a second thing a caller has to
    /// remember to print — pinned by
    /// `the_failures_are_in_the_report_not_appended_to_it`.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "Пошук за текстом — {} чанків у покажчику\n",
            self.chunk_count
        );

        let mut header = pad("клас", LABEL_WIDTH);
        for k in KS {
            header.push_str(&pad(&format!("recall@{k}"), CELL_WIDTH));
        }
        let _ = writeln!(out, "{}", header.trim_end());

        for class in Class::ALL {
            let mut row = pad(label(class), LABEL_WIDTH);
            // Collecting into `Option<Vec<_>>` keeps the row and `recall_at`
            // from disagreeing about what "not measured" is: one `None` from
            // it and the whole row is the word instead of numbers.
            let cells: Option<Vec<String>> = KS
                .iter()
                .map(|&k| self.recall_at(class, k).map(|recall| self.cell(recall, k)))
                .collect();
            match cells {
                Some(cells) => row.extend(cells.iter().map(|c| pad(c, CELL_WIDTH))),
                None => row.push_str(UNMEASURED),
            }
            let _ = writeln!(out, "{}", row.trim_end());
        }

        out.push_str("\nУ дужках — рівень випадковості для того самого k.\n");
        out.push_str("Конфігурації «пошук за вмістом» і «суміш» не збудовані.\n\n");

        let failed: Vec<&Outcome> = self
            .by_class
            .values()
            .flatten()
            .filter(|o| o.rank.is_none())
            .collect();
        if failed.is_empty() {
            out.push_str("Провалених питань немає.\n");
        } else {
            out.push_str("Провалені:\n");
            for outcome in failed {
                let _ = writeln!(
                    out,
                    "  {}  золоті чанки: {}; прийшли: {}",
                    outcome.question,
                    ids(&outcome.gold),
                    ids(&outcome.returned)
                );
            }
        }
        out
    }

    /// One number and, in brackets, what the same `k` would score by drawing
    /// chunks at random. Pinned by
    /// `every_number_is_printed_beside_its_chance_level`.
    fn cell(&self, recall: f64, k: usize) -> String {
        let chance = k as f64 / self.chunk_count as f64;
        format!("{:.1}% ({:.1}%)", recall * 100.0, chance * 100.0)
    }
}

fn label(class: Class) -> &'static str {
    match class {
        Class::Literal => "дослівні",
        Class::Paraphrase => "перефразовані",
        Class::Topical => "про зміст",
    }
}

fn pad(text: &str, width: usize) -> String {
    format!("{text:<width$}")
}

/// Chunk ids as one line. Search returning nothing at all is the failure this
/// harness exists to measure, not an impossibility, so it gets a word.
fn ids(chunks: &[i64]) -> String {
    if chunks.is_empty() {
        return "нічого".to_string();
    }
    chunks
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

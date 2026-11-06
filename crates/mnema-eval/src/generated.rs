use std::fmt::Write as _;
use std::time::Duration;

use mnema_index::QueryRule;
use mnema_search::{FusionRule, Provider};

use crate::{Class, DenseAnswers, EvalError, IndexedCorpus, QuestionSet, SHOWN, run_row};

/// One question answered by one chat model over the product row's passages.
#[derive(Debug, Clone)]
pub struct Answered {
    pub question: String,
    pub class: Class,
    /// The answer cites a gold chunk.
    pub cited_gold: bool,
    /// The model answered with text at all.
    pub answered: bool,
    pub grounding: mnema_rag::Grounding,
}

/// Every question answered once by `model`, the way the application's
/// no-conversation path answers (`mnema_rag::answer`), over the passages the
/// product row (`all-terms / rrf`) returns.
pub fn answer_all(
    indexed: &IndexedCorpus,
    questions: &QuestionSet,
    dense: &DenseAnswers,
    chat: &Provider,
    model: &str,
    timeout: Duration,
) -> Result<Vec<Answered>, EvalError> {
    let outcomes = run_row(
        indexed,
        questions,
        QueryRule::AllTerms,
        FusionRule::Rrf,
        dense,
    )?;
    let mut out = Vec::with_capacity(outcomes.len());
    for o in outcomes {
        let shown: Vec<i64> = o.returned.iter().take(SHOWN).copied().collect();
        let mut passages = Vec::with_capacity(shown.len());
        for &id in &shown {
            let citation = indexed.db().citation(id)?;
            passages.push(mnema_rag::Passage {
                text: citation
                    .as_ref()
                    .map(|c| c.text.clone())
                    .unwrap_or_default(),
                meta: citation.and_then(|c| c.relative_path).unwrap_or_default(),
            });
        }
        let question = questions
            .questions
            .iter()
            .find(|q| q.id == o.question)
            .map(|q| q.text.clone())
            .unwrap_or_default();
        let reply = mnema_rag::answer(
            &chat.base,
            &chat.key,
            model,
            &question,
            &passages,
            mnema_rag::Voice::OpenRouter,
            timeout,
        );
        let (answered, cited_gold, grounding) = match reply {
            Ok(Some(a)) => {
                let cited_gold = a.cited.iter().any(|&n| o.gold.contains(&shown[n - 1]));
                let grounding = mnema_rag::grounding(&a.text, |n| {
                    passages.get(n.wrapping_sub(1)).map(|p| p.text.as_str())
                });
                (true, cited_gold, grounding)
            }
            // A failed or empty reply is no answer: it is counted, not
            // checked.
            Ok(None) | Err(_) => (false, false, mnema_rag::Grounding::Unchecked),
        };
        out.push(Answered {
            question: o.question,
            class: o.class,
            cited_gold,
            answered,
            grounding,
        });
    }
    Ok(out)
}

fn alarm(g: &mnema_rag::Grounding) -> bool {
    matches!(
        g,
        mnema_rag::Grounding::NoCitations | mnema_rag::Grounding::Unsupported { .. }
    )
}

/// Per class: answered, cited the gold chunk, and how the grounding mark
/// splits right answers (a mark there is a false alarm) from wrong ones (a
/// mark there is a catch). Pinned by `the_table_splits_false_alarms_from_catches`.
pub fn render(model: &str, rows: &[Answered]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "=== генерація: {model} ===");
    let _ = writeln!(
        out,
        "{:<15}{:>8}{:>10}{:>10}{:>12}{:>10}{:>12}",
        "клас", "питань", "відповів", "золото", "хибна тривога", "влучання", "не перевір."
    );
    let mut classes: Vec<Class> = Class::ALL.to_vec();
    classes.retain(|c| rows.iter().any(|r| r.class == *c));
    for class in classes {
        let of: Vec<&Answered> = rows.iter().filter(|r| r.class == class).collect();
        let count = |f: &dyn Fn(&Answered) -> bool| of.iter().filter(|r| f(r)).count();
        let _ = writeln!(
            out,
            "{:<15}{:>8}{:>10}{:>10}{:>12}{:>10}{:>12}",
            class.as_str(),
            of.len(),
            count(&|r| r.answered),
            count(&|r| r.cited_gold),
            count(&|r| r.cited_gold && alarm(&r.grounding)),
            count(&|r| r.answered && !r.cited_gold && alarm(&r.grounding)),
            count(&|r| r.answered && r.grounding == mnema_rag::Grounding::Unchecked),
        );
    }
    for r in rows.iter().filter(|r| r.answered && alarm(&r.grounding)) {
        let _ = writeln!(
            out,
            "  {}  золото {}  {:?}",
            r.question,
            if r.cited_gold { "так" } else { "ні" },
            r.grounding
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(class: Class, cited_gold: bool, grounding: mnema_rag::Grounding) -> Answered {
        Answered {
            question: "q".into(),
            class,
            cited_gold,
            answered: true,
            grounding,
        }
    }

    #[test]
    fn the_table_splits_false_alarms_from_catches() {
        let rows = [
            row(Class::Literal, true, mnema_rag::Grounding::NoCitations),
            row(
                Class::Literal,
                false,
                mnema_rag::Grounding::Unsupported { anchors: vec![1] },
            ),
            row(Class::Literal, true, mnema_rag::Grounding::Supported),
        ];
        let text = render("m", &rows);
        let line = text.lines().find(|l| l.starts_with("literal")).unwrap();
        let cells: Vec<&str> = line.split_whitespace().collect();
        // клас, питань, відповів, золото, хибна тривога, влучання, не перевір.
        assert_eq!(cells, ["literal", "3", "3", "2", "1", "1", "0"], "{text}");
    }
}

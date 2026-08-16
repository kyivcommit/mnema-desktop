//! The full sweep over the shipped corpus, from the command line.
//!
//! It computes nothing of its own. Every number it prints comes from
//! `Sweep::render`, which is also what
//! `the_live_sweep_prints_a_table_a_decision_can_be_read_from` prints — the
//! two entry points run the same sequence, and a divergence between them is
//! a defect in one of them rather than two opinions.

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use mnema_eval::{
    Corpus, DenseAnswers, EVAL_MODEL, IndexedCorpus, QuestionSet, Sweep, corpus_dir, embed_corpus,
    preflight, questions_path,
};
use mnema_provider::OPENROUTER_BASE;
use mnema_search::Provider;

/// The key is not named, so no content arm can be asked — refusing here is
/// the alternative to silently measuring the lexical arm alone and printing
/// it as though it were the whole sweep.
const NO_PROVIDER: u8 = 1;
/// Not exactly one argument, so no worker path to run with. Distinct from
/// `NO_PROVIDER` because it is the caller's mistake and not the corpus's.
const NO_WORKER: u8 = 2;
/// Preflight found something, so no number was taken: one describing the
/// corpus rather than the search is worse than none.
const PREFLIGHT_FAILED: u8 = 3;

fn main() -> ExitCode {
    // Checked before the worker path: a missing key is this task's refusal
    // to demonstrate, and it must not hide behind a usage error when the
    // worker argument is missing too.
    let Ok(key) = std::env::var("MNEMA_EVAL_KEY") else {
        eprintln!(
            "MNEMA_EVAL_KEY names the key this sweep asks the provider with — \
             it is the one thing that cannot have a default."
        );
        eprintln!(
            "MNEMA_EVAL_BASE and MNEMA_EVAL_MODEL override the product's own \
             ({OPENROUTER_BASE} and {EVAL_MODEL})."
        );
        return ExitCode::from(NO_PROVIDER);
    };
    let base = std::env::var("MNEMA_EVAL_BASE").unwrap_or_else(|_| OPENROUTER_BASE.to_string());
    let model = std::env::var("MNEMA_EVAL_MODEL").unwrap_or_else(|_| EVAL_MODEL.to_string());
    let provider = Provider { base, key };

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let [worker] = args.as_slice() else {
        eprintln!("usage: eval <path to the mnema-extract-worker binary>");
        eprintln!();
        eprintln!("The worker is built by:");
        eprintln!("    cargo build -p mnema-extract --bin mnema-extract-worker");
        eprintln!("and then sits at target/<profile>/mnema-extract-worker.");
        return ExitCode::from(NO_WORKER);
    };

    evaluate(Path::new(worker), provider, &model)
}

/// A failure at any step before there is a table to print — loading the
/// corpus or questions, indexing, embedding, asking the content arm,
/// sweeping — panics rather than returning a code. These are not conditions
/// this tool handles: they mean the instrument itself is broken, and a
/// panic says so with a message and an exit status (101) the codes above
/// never use.
fn evaluate(worker: &Path, provider: Provider, model: &str) -> ExitCode {
    let corpus = Corpus::load(&corpus_dir()).expect("the shipped corpus loads");
    let questions = QuestionSet::load(&questions_path()).expect("the shipped questions load");
    let indexed = IndexedCorpus::build(&corpus, worker).expect("the corpus indexes");

    let problems = preflight(&corpus, &questions, &indexed).expect("preflight completes");
    if !problems.is_empty() {
        // On stderr, and nothing on stdout: stdout carries the table and only
        // the table, which is what makes it comparable with the test's.
        eprintln!("Передпольотна перевірка не пропустила корпус до вимірювання:");
        for problem in &problems {
            eprintln!("  {problem:?}");
        }
        return ExitCode::from(PREFLIGHT_FAILED);
    }

    // Before any call that costs money and after preflight has cleared the
    // corpus — the same point `sweep.rs`'s live test takes it.
    embed_corpus(&indexed, &provider, model).expect("the corpus embeds");
    let dense = DenseAnswers::ask(&indexed, &questions, provider).expect("the content arm runs");
    let sweep = Sweep::run(&indexed, &questions, &dense).expect("the sweep runs");
    println!("{}", sweep.render());
    ExitCode::SUCCESS
}

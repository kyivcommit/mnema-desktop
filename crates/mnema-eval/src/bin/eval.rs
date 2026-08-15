//! The lexical arm over the shipped corpus, from the command line.
//!
//! It computes nothing of its own. Every number it prints comes from
//! `Report::render`, which is also what `tests/lexical.rs` prints — the two
//! entry points are the same measurement reached two ways, and a divergence
//! between them is a defect in one of them rather than two opinions.

use std::ffi::OsString;
use std::path::Path;

use mnema_eval::{
    Corpus, IndexedCorpus, QuestionSet, Report, corpus_dir, preflight, questions_path, run_lexical,
};
use mnema_index::QueryRule;

/// Preflight found something, so no number was taken: one describing the
/// corpus rather than the search is worse than none.
const PREFLIGHT_FAILED: i32 = 1;
/// Not exactly one argument, so no worker path to run with. Distinct from
/// `PREFLIGHT_FAILED` because it is the caller's mistake and not the corpus's.
const NO_WORKER: i32 = 2;

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let [worker] = args.as_slice() else {
        eprintln!("usage: eval <path to the mnema-extract-worker binary>");
        eprintln!();
        eprintln!("The worker is built by:");
        eprintln!("    cargo build -p mnema-extract --bin mnema-extract-worker");
        eprintln!("and then sits at target/<profile>/mnema-extract-worker.");
        std::process::exit(NO_WORKER);
    };

    // Exit only once `evaluate` has returned, so the temporary index directory
    // it holds is deleted on the way out: `std::process::exit` runs no
    // destructors, and calling it from inside would leak one directory per run.
    std::process::exit(evaluate(Path::new(worker)));
}

/// A failure at any step before there is a report to print — loading the
/// corpus or questions, indexing, running the search, counting chunks —
/// panics rather than returning a code. These are not conditions this tool
/// handles: they mean the instrument itself is broken, and a panic says so
/// with a message and an exit status (101) the codes above never use.
fn evaluate(worker: &Path) -> i32 {
    let corpus = Corpus::load(&corpus_dir()).expect("the shipped corpus loads");
    let questions = QuestionSet::load(&questions_path()).expect("the shipped questions load");
    let indexed = IndexedCorpus::build(&corpus, worker).expect("the corpus indexes");

    let problems = preflight(&corpus, &questions, &indexed).expect("preflight completes");
    if !problems.is_empty() {
        // On stderr, and nothing on stdout: stdout carries the report and only
        // the report, which is what makes it comparable with the test's.
        eprintln!("Передпольотна перевірка не пропустила корпус до вимірювання:");
        for problem in &problems {
            eprintln!("  {problem:?}");
        }
        return PREFLIGHT_FAILED;
    }

    let outcomes = run_lexical(&indexed, &questions).expect("the lexical arm runs");
    let chunk_count = indexed
        .db()
        .chunk_count()
        .expect("the index counts its chunks");
    let report = Report::of(&outcomes, chunk_count);
    // `Report::render` names no configuration — it is also what a sweep row
    // prints, where "text-only" would be wrong. This is the one caller that
    // only ever runs the lexical arm alone, so it is the one place that gets
    // to say so.
    println!(
        "Пошук за текстом — правило {}\n",
        QueryRule::AllTerms.label()
    );
    println!("Конфігурації «пошук за вмістом» і «суміш» не збудовані.\n");
    println!("{}", report.render());
    0
}

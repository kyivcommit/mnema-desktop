use mnema_eval::{
    Class, Corpus, IndexedCorpus, QuestionSet, Report, corpus_dir, preflight, questions_path,
    run_lexical,
};

mod support;

#[test]
fn the_shipped_corpus_is_what_the_spec_says_it_is() {
    // The counts belong here and not in the authoring tasks: the corpus grew
    // across five of them, and a count asserted at the first would have gone
    // red at the second for a reason that is not a defect.
    let corpus = Corpus::load(&corpus_dir()).unwrap();
    assert_eq!(corpus.documents.len(), 20);
    assert_eq!(corpus.documents_in(mnema_eval::Language::Uk).count(), 10);
    assert_eq!(corpus.documents_in(mnema_eval::Language::En).count(), 10);

    let questions = QuestionSet::load(&questions_path()).unwrap();
    assert_eq!(questions.questions.len(), 30);
    for class in Class::ALL {
        assert_eq!(
            questions
                .questions
                .iter()
                .filter(|q| q.class == class)
                .count(),
            10,
            "class {} is not ten questions",
            class.as_str()
        );
    }
}

#[test]
fn the_lexical_configuration_runs_end_to_end_and_prints_a_number() {
    let corpus = Corpus::load(&corpus_dir()).unwrap();
    let questions = QuestionSet::load(&questions_path()).unwrap();
    let indexed = IndexedCorpus::build(&corpus, support::worker()).unwrap();

    // Preconditions first: a number taken over a broken corpus describes the
    // breakage, not the search.
    assert_eq!(preflight(&corpus, &questions, &indexed).unwrap(), vec![]);

    let outcomes = run_lexical(&indexed, &questions).unwrap();
    assert_eq!(outcomes.len(), 30);

    let chunk_count = indexed.db().chunk_count().unwrap();
    assert!(chunk_count > 0, "an indexed corpus with no chunks");
    let report = Report::of(&outcomes, chunk_count);
    println!("{}", report.render());

    // No threshold (spec §8.1). What is asserted is that a number exists for
    // every class — a run that measured nothing would satisfy any bound.
    for class in Class::ALL {
        assert!(
            report.recall_at(class, 1).is_some(),
            "class {} produced no measurement",
            class.as_str()
        );
    }
}

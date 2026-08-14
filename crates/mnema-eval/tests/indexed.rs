use mnema_eval::{Corpus, Document, IndexedCorpus, Language};
use mnema_index::DocumentStatus;

mod support;

fn small_corpus() -> Corpus {
    Corpus {
        documents: vec![
            Document {
                id: "uk/one.md".to_string(),
                language: Language::Uk,
                text: "Комісія розглянула заяву. Ухвалено передати справу далі.".to_string(),
            },
            Document {
                id: "en/two.txt".to_string(),
                language: Language::En,
                text: "The committee reviewed the application and passed it on.".to_string(),
            },
        ],
    }
}

#[test]
fn every_document_reaches_the_index_through_the_real_walk() {
    let indexed = IndexedCorpus::build(&small_corpus(), support::worker()).unwrap();

    // Both directions on the walk's own account: nothing skipped, nothing
    // refused, and the count of indexed files is the count of documents. A
    // one-sided assertion here is satisfied by a walk that found nothing.
    let report = indexed.report();
    assert_eq!(report.skipped, 0, "walk report: {report:?}");
    assert_eq!(report.refused, 0, "walk report: {report:?}");
    assert_eq!(report.indexed, 2, "walk report: {report:?}");

    for id in ["uk/one.md", "en/two.txt"] {
        let document_id = indexed
            .document_id(id)
            .unwrap()
            .unwrap_or_else(|| panic!("{id} has no path row"));
        assert_eq!(
            indexed.db().document_status(&document_id).unwrap(),
            DocumentStatus::Indexed,
            "{id} is in the index but not searchable"
        );
    }
}

#[test]
fn a_document_that_is_not_there_has_no_path_row() {
    // The other direction of `document_id`: it must answer None rather than
    // guessing, or preflight would never see a missing document.
    let indexed = IndexedCorpus::build(&small_corpus(), support::worker()).unwrap();
    assert_eq!(indexed.document_id("uk/nowhere.md").unwrap(), None);
}

#[test]
fn the_chunks_carry_the_text_the_document_had() {
    // The point of going through the real pipeline rather than inserting rows:
    // what search sees is what extraction and chunking produced.
    let indexed = IndexedCorpus::build(&small_corpus(), support::worker()).unwrap();
    let document_id = indexed.document_id("uk/one.md").unwrap().unwrap();
    let chunks = indexed.db().chunks_of_document(&document_id).unwrap();
    assert!(
        !chunks.is_empty(),
        "a document with text produced no chunks"
    );
    let joined: String = chunks.iter().map(|(_, t)| t.as_str()).collect();
    assert!(
        joined.contains("Ухвалено передати справу далі."),
        "the sentence did not survive the pipeline: {joined:?}"
    );
}

#[test]
fn two_builds_do_not_share_an_index() {
    // "A temporary directory per run" is an assertion, not a hope: a build that
    // reused a previous index would report the same chunk count with a corpus
    // half the size.
    let one = IndexedCorpus::build(&small_corpus(), support::worker()).unwrap();
    let mut all = small_corpus();
    let smaller = Corpus {
        documents: vec![all.documents.remove(0)],
    };
    let two = IndexedCorpus::build(&smaller, support::worker()).unwrap();
    assert_eq!(two.document_id("en/two.txt").unwrap(), None);
    assert!(one.document_id("en/two.txt").unwrap().is_some());
}

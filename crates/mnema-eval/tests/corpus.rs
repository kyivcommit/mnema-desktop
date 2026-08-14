use std::path::Path;

use mnema_eval::{Corpus, EvalError, Language};

/// Builds a corpus tree under a temporary directory and returns it.
fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (relative, text) in files {
        let path = dir.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
    }
    dir
}

#[test]
fn language_comes_from_the_directory_not_from_the_text() {
    // The English document is written in Ukrainian on purpose: the loader must
    // take the language from where the file sits, not guess it from content.
    let dir = tree(&[
        ("uk/one.md", "Комісія розглянула заяву."),
        ("en/two.md", "Комісія розглянула заяву."),
    ]);
    let corpus = Corpus::load(dir.path()).unwrap();
    let mut got: Vec<(&str, Language)> = corpus
        .documents
        .iter()
        .map(|d| (d.id.as_str(), d.language))
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![("en/two.md", Language::En), ("uk/one.md", Language::Uk)]
    );
}

#[test]
fn the_id_is_the_relative_path_with_forward_slashes() {
    let dir = tree(&[("uk/notes/one.txt", "текст")]);
    let corpus = Corpus::load(dir.path()).unwrap();
    assert_eq!(corpus.documents[0].id, "uk/notes/one.txt");
}

#[test]
fn only_txt_and_md_are_taken() {
    // A `.pdf` in the tree is not a silent omission — it is the corpus rule
    // being enforced, and the file must not become a document.
    let dir = tree(&[
        ("uk/one.md", "текст"),
        ("uk/two.txt", "текст"),
        ("uk/three.pdf", "%PDF-1.7"),
    ]);
    let corpus = Corpus::load(dir.path()).unwrap();
    let mut ids: Vec<&str> = corpus.documents.iter().map(|d| d.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, vec!["uk/one.md", "uk/two.txt"]);
}

#[test]
fn a_directory_that_is_not_a_language_is_refused_not_skipped() {
    // Silently skipping would drop documents from the measurement and the
    // number would describe a smaller corpus than the one on disk.
    let dir = tree(&[("uk/one.md", "текст"), ("de/two.md", "Text")]);
    let err = Corpus::load(dir.path()).unwrap_err();
    assert_eq!(
        err,
        EvalError::Corpus("de is not a language directory".into())
    );
}

#[test]
fn a_non_directory_entry_at_the_corpus_root_is_refused_not_skipped() {
    // A stray file at the root is the same harm as a wrongly named
    // directory: silently skipping it would shrink the measured corpus.
    let dir = tree(&[("uk/one.md", "текст")]);
    std::fs::write(dir.path().join("notes.md"), "текст").unwrap();
    let err = Corpus::load(dir.path()).unwrap_err();
    assert_eq!(err, EvalError::Corpus("notes.md is not a directory".into()));
}

#[test]
fn an_empty_document_is_refused() {
    // An empty document indexes to no chunks, so every question against it
    // resolves Missing — a preflight failure disguised as a search miss.
    let dir = tree(&[("uk/one.md", "   \n  ")]);
    let err = Corpus::load(dir.path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Corpus(msg) if msg.contains("uk/one.md")),
        "expected the refusal to name the document, got {err:?}"
    );
}

#[test]
fn an_empty_corpus_is_refused() {
    // Zero documents means every question resolves Missing for the whole
    // corpus — the same preflight failure as one empty file, one level up.
    let dir = tree(&[]);
    std::fs::create_dir_all(dir.path().join("uk")).unwrap();
    let path = dir.path().display().to_string();
    let err = Corpus::load(dir.path()).unwrap_err();
    assert!(
        matches!(&err, EvalError::Corpus(msg) if msg.contains(&path)),
        "expected the refusal to name the corpus directory, got {err:?}"
    );
}

#[test]
fn documents_in_a_language_are_exactly_that_language() {
    let dir = tree(&[
        ("uk/one.md", "текст"),
        ("uk/two.md", "текст"),
        ("en/three.md", "text"),
    ]);
    let corpus = Corpus::load(dir.path()).unwrap();
    assert_eq!(corpus.documents_in(Language::Uk).count(), 2);
    assert_eq!(corpus.documents_in(Language::En).count(), 1);
    assert!(
        corpus
            .documents_in(Language::Uk)
            .all(|d| d.language == Language::Uk)
    );
}

#[test]
fn the_shipped_corpus_directory_is_where_the_crate_is() {
    // The path ends in `mnema-eval/corpus`.
    let dir = mnema_eval::corpus_dir();
    assert!(
        dir.ends_with(Path::new("mnema-eval").join("corpus")),
        "got {}",
        dir.display()
    );
}

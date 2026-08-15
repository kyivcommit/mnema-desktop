use std::path::{Path, PathBuf};

/// Which language a document is in, taken from the directory it sits in.
/// Pinned by `language_comes_from_the_directory_not_from_the_text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Language {
    Uk,
    En,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Language::Uk => "uk",
            Language::En => "en",
        }
    }

    /// The one place `"uk"`/`"en"` is spelled out. `pub(crate)` because
    /// `QuestionSet::load` reads the same two strings out of the question
    /// file: two copies would let a third language be added to one of them
    /// and leave the two halves disagreeing about what a language is, with
    /// nothing to go red.
    pub(crate) fn parse(s: &str) -> Option<Language> {
        match s {
            "uk" => Some(Language::Uk),
            "en" => Some(Language::En),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Relative path from the corpus root, `/`-separated — the same string
    /// that lands in `path.relative_path`.
    /// Pinned by `the_id_is_the_relative_path_with_forward_slashes`.
    pub id: String,
    pub language: Language,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Corpus {
    pub documents: Vec<Document>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    #[error("corpus: {0}")]
    Corpus(String),
    #[error("io: {0}")]
    Io(String),
    /// A string rather than `mnema_index::Error`, for the same reason `Io` is
    /// one: this enum derives `PartialEq`/`Eq` — `tests/corpus.rs` compares
    /// whole errors — and neither `mnema_index::Error` nor `std::io::Error`
    /// implements them. Nothing here branches on which index error it was.
    #[error("index: {0}")]
    Index(String),
    #[error("questions: {0}")]
    Questions(String),
    #[error("no active embedding space")]
    NoActiveSpace,
    #[error("content arm was silent: {0}")]
    ContentArmSilent(String),
}

impl From<std::io::Error> for EvalError {
    fn from(err: std::io::Error) -> Self {
        EvalError::Io(err.to_string())
    }
}

/// The same trade as `Io` above, and the reason every index call in this crate
/// is a bare `?`: the message survives, the type does not, and nothing here
/// branches on which index error it was.
impl From<mnema_index::Error> for EvalError {
    fn from(err: mnema_index::Error) -> Self {
        EvalError::Index(err.to_string())
    }
}

/// The corpus shipped with this crate.
pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

impl Corpus {
    pub fn load(dir: &Path) -> Result<Corpus, EvalError> {
        let mut documents = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            // Refused, not skipped:
            // `a_non_directory_entry_at_the_corpus_root_is_refused_not_skipped`
            if !entry.file_type()?.is_dir() {
                return Err(EvalError::Corpus(format!("{name} is not a directory")));
            }
            // Refused, not skipped:
            // `a_directory_that_is_not_a_language_is_refused_not_skipped`.
            let language = Language::parse(&name)
                .ok_or_else(|| EvalError::Corpus(format!("{name} is not a language directory")))?;
            collect(&entry.path(), &name, language, &mut documents)?;
        }
        if documents.is_empty() {
            return Err(EvalError::Corpus(format!(
                "{} has no documents",
                dir.display()
            )));
        }
        documents.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Corpus { documents })
    }

    pub fn documents_in(&self, language: Language) -> impl Iterator<Item = &Document> {
        self.documents
            .iter()
            .filter(move |d| d.language == language)
    }
}

fn collect(
    dir: &Path,
    prefix: &str,
    language: Language,
    out: &mut Vec<Document>,
) -> Result<(), EvalError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let id = format!("{prefix}/{name}");
        if entry.file_type()?.is_dir() {
            collect(&entry.path(), &id, language, out)?;
            continue;
        }
        let is_text = name.ends_with(".md") || name.ends_with(".txt");
        if !is_text {
            continue;
        }
        let text = std::fs::read_to_string(entry.path())?;
        if text.trim().is_empty() {
            // Refused: an empty document indexes to no chunks.
            // Pinned by `an_empty_document_is_refused`.
            return Err(EvalError::Corpus(format!("{id} is empty")));
        }
        out.push(Document { id, language, text });
    }
    Ok(())
}

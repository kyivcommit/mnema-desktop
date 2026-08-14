use std::path::{Path, PathBuf};

/// Which language a document is in, taken from the directory it sits in.
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

    fn parse(s: &str) -> Option<Language> {
        match s {
            "uk" => Some(Language::Uk),
            "en" => Some(Language::En),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub id: String,
    pub language: Language,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Corpus {
    pub documents: Vec<Document>,
}

#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("corpus: {0}")]
    Corpus(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
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
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let language = Language::parse(&name)
                .ok_or_else(|| EvalError::Corpus(format!("{name} is not a language directory")))?;
            collect(&entry.path(), &name, language, &mut documents)?;
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
            return Err(EvalError::Corpus(format!("{id} is empty")));
        }
        out.push(Document { id, language, text });
    }
    Ok(())
}

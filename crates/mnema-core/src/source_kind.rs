use serde::{Deserialize, Serialize};

/// Stamped at extraction. Lets a query ask for documents only, without splitting
/// the database — mixing code into one lexical index shifts term weights by up to
/// 2.2x and reorders prose results. G7.0 §5.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Document,
    Code,
    Data,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Document => "document",
            SourceKind::Code => "code",
            SourceKind::Data => "data",
        }
    }
}

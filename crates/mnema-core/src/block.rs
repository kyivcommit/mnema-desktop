use serde::{Deserialize, Serialize};

/// The vocabulary `block.type` enforces (`schema.sql:119-120`): the server's
/// seven (`app/ocr/docling_structurizer.py:29-37`) plus `Code`, which only
/// this product can produce — the server never sees a source file. Closed for
/// the same reason `SourceKind` is: an open `type` column turns a writer's
/// typo into a row a future query grouping by type can never match again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockType {
    Paragraph,
    Headline,
    Caption,
    Table,
    Figure,
    PageHeader,
    PageFooter,
    Code,
}

impl BlockType {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockType::Paragraph => "paragraph",
            BlockType::Headline => "headline",
            BlockType::Caption => "caption",
            BlockType::Table => "table",
            BlockType::Figure => "figure",
            BlockType::PageHeader => "page_header",
            BlockType::PageFooter => "page_footer",
            BlockType::Code => "code",
        }
    }
}

/// One paragraph-sized piece of a page's text.
///
/// Lives here, not in `mnema-extract`, so that `mnema-chunk` — which consumes
/// this type — never has to depend on the crate that links `pdfium`, a C++
/// library. That is what keeps the application binary unable to reach the PDF
/// FFI at all: the guard is which crate `Block` lives in, not a convention
/// anyone has to remember. It is also the shape the extraction worker sends
/// across the NDJSON wire (`mnema_extract::wire::Frame::Block`).
///
/// No `id` and no `page_id`: both are assigned only when the row is written
/// (`mnema-index::write::insert_block`), never before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub block_type: BlockType,
    /// Position within its page; restarts at 0 on the next page — the
    /// schema's own uniqueness is on `(page_id, reading_order)`, not on the
    /// document as a whole (`schema.sql:134`).
    pub reading_order: i64,
    pub language: Option<String>,
    /// Verbatim after NFC (D32, D38): no whitespace collapsing, no reflow, no
    /// dehyphenation. Indentation and tabs survive so that offsets taken
    /// later still point into text that matches the source file.
    pub text: String,
    /// 1-based, inclusive; `None` for formats without lines
    /// (`schema.sql:127-128`).
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
}

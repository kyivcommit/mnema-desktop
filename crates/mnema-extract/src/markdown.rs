//! The markdown reader: sections become pages, and every block's text is a
//! slice of the file.
//!
//! **The constraint that decides this whole module: a block's text is a slice
//! of the source, never a rendering of the parse tree.** D38 makes stored text
//! verbatim after NFC, and every offset in a `Locator` is measured against
//! exactly the string in `block.text` — so re-emitting a node, even into
//! something that looks identical, points a citation's highlight into text the
//! file does not contain. That is the failure the whole locator design exists
//! to prevent. comrak's own `NodeCodeBlock::literal` is the near miss to know
//! about: for an indented block, and for a fence that is itself indented, it
//! hands back the content with that indentation removed.
//!
//! So nothing here reads a node's *value*. It reads a node's `sourcepos` and
//! slices the NFC-normalised source with it. Normalisation happens once, before
//! any slicing, so the offsets and the stored text describe the same string.
//! The one string that is touched after slicing is `section_title`, which is
//! display metadata that no offset is measured into — `heading_title` says
//! what is done to it and why the distinction holds.
//!
//! **Three things in a file reach no block**, and the list is closed
//! deliberately rather than by accident:
//!
//! - a thematic break, which carries no text;
//! - YAML front matter, which is metadata rather than prose — and which has to
//!   be recognised in order to be dropped, since CommonMark reads it as a
//!   thematic break plus a setext heading;
//! - a link reference definition (`[пос]: https://…`), because comrak consumes
//!   it into its link map and emits no node at all. That one is a real loss —
//!   in a documentation folder it is where the URLs are — and recovering it
//!   means re-scanning the source for lines no node claims.
//!
//! **Only the top level of the tree is walked.** One top-level node is one
//! block: a whole list is one block, a blockquote is one block, and their
//! source markers (`- `, `> `) are part of the text because they are part of
//! the file. Descending would mean re-inventing the join between an item's
//! lines, and there is no join that is a slice of the source. The visible cost
//! is that a fence nested inside a list item is not typed as code — it is part
//! of that list's block.
//!
//! D37 makes a page a section, and this reader is the first thing that can
//! produce more than one page for a file. What it deliberately does not do is
//! **nesting**: a page has one `section_title` and no parent, so `##` under a
//! `#` opens its own page rather than a subsection of one. Someone will assume
//! otherwise, which is why it is stated here and asserted in
//! `tests/markdown.rs`.
//!
//! The output is deliberately not the server's. It runs Python-Markdown with
//! `extensions=["tables"]` and no `fenced_code` (`app/textdoc/adapters.py:61`),
//! so a fence there is a paragraph; the divergence is already recorded
//! (in the deleted dependency probe, and in D39) and is part of why byte
//! comparison against the server was withdrawn as a criterion (D39).

use comrak::nodes::{AstNode, NodeValue, Sourcepos};
use comrak::{Arena, Options, parse_document};

use mnema_core::{Block, BlockType, nfc};

/// One page of a markdown file: a section, or the run of content before the
/// first heading.
///
/// Not `mnema_pool::ExtractedPage`, although the two carry the same three
/// things. That type lives on the application's side of the process boundary
/// and is assembled from wire frames; this one is what a reader produces
/// before any of it is serialised. Sharing it would put the pool's type into
/// the crate that links Pdfium, which is the dependency D40 exists to forbid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownPage {
    /// 1-based, and consecutive here — a markdown reader skips nothing, so
    /// unlike a PDF's it never leaves a gap.
    pub page_no: u32,
    pub section_title: Option<String>,
    pub blocks: Vec<Block>,
}

/// Reads markdown into pages, one per section.
///
/// No `Result`, for the same reason `extract_text` has none: decoding cannot
/// fail (an invalid byte sequence becomes U+FFFD), and CommonMark has no
/// parse error — every input is a valid document, which is the point of the
/// specification.
///
/// Always at least one page, including for an empty file: `block.page_id` is
/// NOT NULL, so the four-level model needs a page even where the format has
/// none (D37). The pool also compares the header's count with the page frames
/// that arrive, so a reader that answered "no pages" would have to remember to
/// announce zero as well.
pub fn extract_markdown(bytes: &[u8]) -> Vec<MarkdownPage> {
    let decoded = crate::text::decode(bytes);
    // Once, and before anything takes an offset from it (D32, D38).
    let source = nfc::normalise(&decoded);
    let lines = Lines::new(&source);

    let arena = Arena::new();
    let root = parse_document(&arena, &source, &options());

    let mut pages = vec![MarkdownPage {
        page_no: 1,
        section_title: None,
        blocks: Vec::new(),
    }];

    for node in root.children() {
        let value = &node.data.borrow().value;
        let block_type = match value {
            // A horizontal rule carries no text. Emitting it would put a block
            // holding `---` into the index: searchable, citable and empty of
            // content. Blank lines are dropped by every reader here for the
            // same reason.
            NodeValue::ThematicBreak => continue,
            // Metadata, and the same argument: it is neither a section nor
            // prose. It has to be *recognised* to be dropped — with the
            // delimiter unset, CommonMark reads the opening `---` as a
            // thematic break and the keys plus the closing `---` as a setext
            // heading, so `title: Довідник` became a page and named it.
            NodeValue::FrontMatter(_) => continue,
            NodeValue::Heading(_) => BlockType::Headline,
            // Fenced and indented alike. The fence lines themselves are part
            // of the text: they are in the file, and excluding them would mean
            // deciding whether a fence was ever closed — an unclosed fence at
            // the end of a file has no closing line to exclude.
            NodeValue::CodeBlock(_) => BlockType::Code,
            NodeValue::Table(_) => BlockType::Table,
            _ => BlockType::Paragraph,
        };

        let Some((first, last)) = lines.rows(node.data.borrow().sourcepos) else {
            continue;
        };
        let text = lines.rows_text(first, last);
        if text.trim().is_empty() {
            continue;
        }

        if block_type == BlockType::Headline {
            // A heading with no text at all — `#` on a line of its own — is
            // not a section, and not a row either. Its block would hold `#`,
            // which is the thematic break's case exactly: searchable, citable
            // and empty of content. So the block follows the title.
            let Some(title) = heading_title(&lines, node) else {
                continue;
            };
            open_page(&mut pages, title);
        }

        let page = pages.last_mut().expect("a page is always open");
        page.blocks.push(Block {
            block_type,
            // Restarts on every page: the schema's uniqueness is on
            // `(page_id, reading_order)`, because reading order is what
            // reconstructs a page rather than a document.
            reading_order: page.blocks.len() as i64,
            // Nothing here detects language. A per-block guess is the
            // extraction spec's subject, not this reader's.
            language: None,
            text: text.to_string(),
            line_start: Some(first as u32),
            line_end: Some(last as u32),
        });
    }

    pages
}

/// Starts the page a heading opens — or names the one already open, when that
/// page is still empty and unnamed.
///
/// The second case is a file that begins with a heading: without it, page 1
/// would be an untitled page with no blocks on it, and every real section
/// would be numbered one higher than it is.
fn open_page(pages: &mut Vec<MarkdownPage>, title: String) {
    let empty_and_unnamed = pages
        .last()
        .is_some_and(|page| page.blocks.is_empty() && page.section_title.is_none());
    if empty_and_unnamed {
        pages.last_mut().expect("just inspected").section_title = Some(title);
        return;
    }
    pages.push(MarkdownPage {
        page_no: pages.len() as u32 + 1,
        section_title: Some(title),
        blocks: Vec::new(),
    });
}

/// The longest section title this reader will produce, in characters.
///
/// `page.section_title` is a bare `TEXT` column (`schema.sql:103`) displayed
/// beside a citation, with nothing between the reader and the interface to
/// bound it — and a heading has no length limit of its own. The number is
/// anchored on `mnema_chunk::MIN_CHARS`, 200: a title longer than the shortest
/// thing this product is willing to call a chunk is not a title.
pub const SECTION_TITLE_MAX_CHARS: usize = 200;

/// A heading's own text: the span from its first inline child to its last,
/// flattened onto one line and bounded.
///
/// The span is a slice of the source, which is what keeps the title honest
/// about the file — and what makes `# Розділ **перший**` name a page
/// `Розділ **перший**`, emphasis markers included. Rendering the inlines to
/// plain text would read better; it is not done, because then the title would
/// be the one string here that the file does not contain.
///
/// What **is** done to it is whitespace and length, and that is a different
/// thing from rendering: no offset is ever measured into a title, so unlike
/// `block.text` it is display metadata rather than evidence. A setext heading
/// may span several lines and a heading may be arbitrarily long, and a
/// paragraph in a page title is a rendering problem nobody downstream can
/// undo. The ellipsis is there so that a cut title says it was cut.
///
/// `None` for a heading with no text at all (`#` on a line of its own): a page
/// named by the empty string is worse than an unnamed page, because it renders
/// as a section that exists and has no name.
fn heading_title<'a>(lines: &Lines<'_>, heading: &'a AstNode<'a>) -> Option<String> {
    let mut children = heading.children().map(|c| c.data.borrow().sourcepos);
    let first = children.next()?;
    let last = children.last().unwrap_or(first);
    let flattened = lines
        .between(first, last)?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    bound_section_title(flattened)
}

/// The last step of every reader that names a page after a heading: refuse an
/// empty title, and cut a long one so that the cut is visible.
///
/// Shared rather than repeated, and it lives beside the constant it applies.
/// `html.rs` is the second caller and `epub.rs`/`docx.rs` are the next two —
/// four readers deciding independently how long a title may be is four
/// numbers, and `page.section_title` is one column displayed by one interface.
///
/// The input is already flattened onto one line by its caller: what "the
/// heading's own text" is differs per format (a source span here, a subtree in
/// `html.rs`), and only what happens *after* that is common.
pub(crate) fn bound_section_title(flattened: String) -> Option<String> {
    if flattened.is_empty() {
        return None;
    }
    if flattened.chars().count() <= SECTION_TITLE_MAX_CHARS {
        return Some(flattened);
    }
    let mut cut: String = flattened
        .chars()
        .take(SECTION_TITLE_MAX_CHARS - 1)
        .collect();
    cut.push('…');
    Some(cut)
}

/// CommonMark plus the GFM extensions.
///
/// Only `table` changes anything this reader emits — it is what makes a table
/// a `Table` node instead of a paragraph. The others are enabled because GFM
/// is what a `.md` file in a repository is written in, and because a mapping
/// that later descends below the top level should find GFM's tree rather than
/// a subset of it. `parse` and `render` are left at their defaults: nothing
/// here renders, and `render` is dead weight rather than a decision.
///
/// `front_matter_delimiter` is not GFM and is the one option here that changes
/// what the *document* is. Without it, CommonMark reads a `---` block at the
/// top of a file as a thematic break followed by a setext heading, which this
/// reader then makes a section and names after a metadata key. With it, front
/// matter is a node of its own and can be dropped as the metadata it is.
///
/// What it costs is a file that opens with a horizontal rule and closes it
/// with a second `---` before any other content: comrak takes everything
/// between them for front matter and this reader drops it.
/// `a_file_that_opens_with_a_rule_loses_what_is_between_it_and_the_next_rule`
/// records that. Both losses are silent, so the choice is which is rarer, and
/// a `.md` beginning with a rule is far rarer than one beginning with YAML.
fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.front_matter_delimiter = Some("---".to_string());
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options
}

/// The source with the byte offset of every line start, so that a
/// `Sourcepos` — which counts lines from 1 and **bytes** from 1 within a line
/// — can be turned into a slice.
///
/// Bytes, not characters: measured, not assumed. A paragraph of sixteen
/// Cyrillic letters is reported as ending at column 31, which is its length in
/// bytes. Treating those columns as character offsets would slice inside a
/// character on the first Ukrainian heading in any file.
struct Lines<'a> {
    source: &'a str,
    /// Byte offset where each line begins, `starts[n]` being line `n + 1`.
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    fn new(source: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            source
                .bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| i + 1),
        );
        Self { source, starts }
    }

    /// The byte range of one 1-based line, without its terminator.
    ///
    /// The `\r` of a CRLF file goes with the terminator: it is not part of the
    /// last line of a block, though it stays inside a block that spans several
    /// lines, because there it is between two of them and the text is a slice.
    fn span(&self, line: usize) -> (usize, usize) {
        let start = self.starts[line - 1];
        let mut end = match self.starts.get(line) {
            Some(&next) => next - 1,
            None => self.source.len(),
        };
        if end > start && self.source.as_bytes()[end - 1] == b'\r' {
            end -= 1;
        }
        (start, end)
    }

    /// The lines a node occupies, 1-based and inclusive, or `None` if it
    /// occupies none.
    ///
    /// The correction is not cosmetic. An indented code block followed by a
    /// blank line is reported as ending at `line + 1` **column 0** — a
    /// position one line past its own text. Taken at face value the block
    /// claims a line it does not occupy and its text carries a trailing blank
    /// line the file does not have inside it. Column 0 is comrak's way of
    /// saying "the end of the previous line", so that is what it is read as.
    fn rows(&self, at: Sourcepos) -> Option<(usize, usize)> {
        let first = at.start.line;
        let last = if at.end.column == 0 {
            at.end.line.checked_sub(1)?
        } else {
            at.end.line
        };
        if first == 0 || first > last || first > self.starts.len() {
            return None;
        }
        Some((first, last.min(self.starts.len())))
    }

    fn rows_text(&self, from: usize, to: usize) -> &'a str {
        &self.source[self.span(from).0..self.span(to).1]
    }

    /// The source between the start of one node and the end of another,
    /// columns included — `end.column` is the last byte of the node, not the
    /// one after it.
    ///
    /// `None` rather than a panic when the range is not a slice this string
    /// can produce. Every column comrak reports has been a byte offset in
    /// everything measured here, tabs and multi-byte characters included, but
    /// a wrong offset would otherwise take the whole indexing job down over
    /// one heading, and the caller has an honest answer available: no title.
    fn between(&self, start: Sourcepos, end: Sourcepos) -> Option<&'a str> {
        let from = self.starts.get(start.start.line - 1)? + start.start.column - 1;
        let to = self.starts.get(end.end.line - 1)? + end.end.column;
        if from > to || to > self.source.len() {
            return None;
        }
        if !self.source.is_char_boundary(from) || !self.source.is_char_boundary(to) {
            return None;
        }
        Some(&self.source[from..to])
    }
}

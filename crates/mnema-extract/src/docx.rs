//! The DOCX reader: `word/document.xml` is the prose, and a heading paragraph
//! opens a section.
//!
//! `zip_part::read_member` opens the part under a cap and `markdown`'s
//! `bound_section_title` names the page. What is decided here is the parse —
//! and the parse is where this format hides everything worth losing.
//!
//! **A page is a section, because a DOCX has no pages until something lays it
//! out.** Where a page ends depends on the paper size, the fonts installed and
//! the printer driver; nothing in this process knows any of the three, and
//! G7.1 §2.2 declined to port pagination by character count — a number that
//! would name a "page 4" no copy of the document agrees with. So this reader
//! answers what `markdown.rs` and `html.rs` answer, and `pages_of` cites it the
//! same way: `Coordinate::Section` (`crates/mnema-ingest/src/lib.rs:1392`).
//!
//! **What a heading is, is the one thing here that measurement reversed.** The
//! obvious rule is a `<w:pStyle>` of `Heading1`–`Heading9`. Probed against 24
//! real `.docx` files on the author's machine — element names, style ids and
//! counts only, never their prose — **2** use such an id. Five have a paragraph
//! whose style `word/styles.xml` calls a heading or gives an outline level, and
//! in three of those the id is `11`, with a Cyrillic `<w:name>` and
//! `<w:outlineLvl w:val="0"/>`: Word renames the built-in styles when a document
//! is authored in another language, and only the stylesheet still says what they
//! are. Matching the id alone would find no heading in three of the five files
//! that have any, and those documents would arrive as one unnamed page whose
//! every citation names nothing (spec §6, invariant 1). So this reader opens
//! `word/styles.xml` as well, and [`is_heading`] takes three signals in order of
//! how much they know.
//!
//! **What the parse deliberately does not index**, each answered by a run rather
//! than by reading the specification:
//!
//! - a field *instruction* (`<w:instrText>`) — ` TOC \o "1-3" `, ` PAGE `,
//!   ` HYPERLINK "…" `. It is code, it is never painted, and it is exactly the
//!   plausible-looking noise `html.rs` refuses `<script>` for. The field's
//!   *result* sits beside it in an ordinary `<w:t>` and is kept;
//! - deleted text (`<w:delText>`), which is what the author took out. Indexing
//!   it answers a search with a sentence the document does not contain — this
//!   crate's sharpest failure, in the one format that keeps a record of it;
//! - the old position of a moved paragraph (`<w:moveFrom>`), which unlike a
//!   deletion holds ordinary `<w:t>` and would otherwise store the same sentence
//!   twice, once where it is and once where it is not;
//! - the `<mc:Fallback>` half of `<mc:AlternateContent>`, which restates its
//!   `<mc:Choice>` in older markup. A text box arrives this way, and reading both
//!   halves stores its words twice;
//! - a paragraph's former properties (`<w:pPrChange>`), which carry a whole
//!   `<w:pPr>` including the `<w:pStyle>` that used to be there. Read as current,
//!   a document is cut into sections wherever somebody once edited one;
//! - text an attribute carries — a drawing's `descr`, a content control's alias,
//!   a field's `w:instr`. The same rule `html.rs` states for `alt` and `title`:
//!   it is not a text node, and reading attributes needs a per-attribute
//!   decision about which of them are prose.
//!
//! **What is lost and is not in that list**, because it is a member this reader
//! does not open at all: footnotes and endnotes (`word/footnotes.xml`,
//! `word/endnotes.xml`), comments (`word/comments.xml`), and the running headers
//! and footers (`word/header1.xml`, `word/footer1.xml`). The last two are the
//! furniture of a printed page and are the same thing `BlockType::PageHeader`
//! exists to mark in a PDF; the first two are genuine prose and their loss is
//! genuine. Reading them needs a decision this task does not have — a footnote
//! is anchored at a point inside a paragraph, and this reader's blocks have no
//! place to put one.
//!
//! **The cap, and the number rather than the name.** Two members are opened,
//! each under `zip_part::MEMBER_MAX_BYTES`, so the worst case is a fixed 32 MiB
//! of decompressed XML. That is why there is no `DOCX_MAX_BYTES` beside
//! `epub::BOOK_MAX_BYTES`: a book opens one member per chapter and its worst
//! case grows with the spine, and this one does not grow at all.

use std::collections::HashSet;

use mnema_core::{Block, BlockType, nfc};

use crate::markdown::bound_section_title;
use crate::zip_part::{self, MEMBER_MAX_BYTES, ZipPartError};

/// The part every DOCX has, at the one name the format fixes — and the same
/// member `typing::identify` requires before it names this reader
/// (`typing.rs:285`).
const DOCUMENT_PART: &str = "word/document.xml";

/// The stylesheet, which is what says whether a paragraph style is a heading.
/// Absent, this reader still reads the document — see [`extract_docx`].
const STYLES_PART: &str = "word/styles.xml";

/// One section of a document: one page of what is stored.
///
/// Deliberately not [`crate::MarkdownPage`] or [`crate::HtmlPage`], although the
/// three carry the same three things — see `MarkdownPage`'s own doc for why a
/// reader's page is not the pool's, and why sharing one would put the pool's
/// type into the crate that links Pdfium (D40).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocxSection {
    /// 1-based and consecutive: this reader skips no page. A DOCX has no page
    /// it *could* skip — every paragraph the part declares is read — so unlike
    /// a PDF's or a book's, these numbers never leave a gap and the worker's
    /// `skipped_pages` is empty rather than absent.
    pub page_no: u32,
    pub section_title: Option<String>,
    pub blocks: Vec<Block>,
}

/// Why a document could not be read at all. Three variants, three refusal
/// rules, and every one of them reachable — a variant nothing produces is a
/// branch in the worker that no test can ever redden.
///
/// Three flat variants rather than one wrapping [`ZipPartError`], which is the
/// shape Task 11's plan asked for and its implementer disproved: that type
/// forces an arm for `Missing` on the stylesheet, which is not an error here at
/// all.
#[derive(Debug, Clone, thiserror::Error)]
pub enum DocxError {
    /// The archive parses as a zip and what makes it a document does not: no
    /// `word/document.xml`, a part that will not decompress, XML that does not
    /// parse, or XML that stops in the middle of an element.
    #[error("this document is damaged: {0}")]
    Malformed(String),

    /// A member inflated past [`MEMBER_MAX_BYTES`].
    ///
    /// Decided on what came out of the stream, never on the size the archive
    /// declares — see `zip_part`'s module doc for the forged-size case that
    /// makes the distinction load-bearing.
    #[error("a part of this document inflates past the cap on one member")]
    TooLarge,

    /// The part parsed and not one paragraph of it carries a word: a document
    /// of scanned images, or one that is empty.
    ///
    /// The same answer `pdf.rs` gives a document whose every page is a scan and
    /// `epub.rs` a book of plates. A document with no text in it is a fact
    /// about the file worth telling someone, and storing it as a document with
    /// zero blocks tells them nothing.
    #[error("no paragraph of this document carries any text")]
    NoText,
}

/// Reads a DOCX into sections, one page each.
///
/// Takes bytes rather than a path for the reason every reader in this crate
/// does: `handle_request` reads the file once and hashes the same `Vec<u8>` it
/// hands here, so nothing can change between the digest and the reading.
///
/// **A stylesheet that is not there is not a damaged document.** It holds no
/// prose; what it holds is the answer to "is this style a heading", and without
/// it [`is_heading`] falls back to the canonical ids. Refusing here would take a
/// whole document out of the index over a part nobody reads — the argument
/// `epub.rs` makes for a chapter the archive does not hold, reached from the
/// other side. `TooLarge` is not treated that way, and the asymmetry is
/// deliberate: a stylesheet that inflates to gigabytes is the bomb this cap
/// exists for, whatever member it arrives in.
pub fn extract_docx(bytes: &[u8]) -> Result<Vec<DocxSection>, DocxError> {
    extract(bytes, MEMBER_MAX_BYTES)
}

/// The read, against a stated cap on each member.
///
/// Split out for the reason `epub::extract` is: [`MEMBER_MAX_BYTES`] is 16 MiB,
/// and a test that reached it would have to inflate 16 MiB — twice, once for
/// each member that owes the cap. The rule these tests exist to check is not
/// "16 MiB"; it is that **both** members are read under a cap and that the
/// stylesheet's failures are not all alike, and a cap of a few kilobytes states
/// that just as well.
fn extract(bytes: &[u8], cap: usize) -> Result<Vec<DocxSection>, DocxError> {
    let document = zip_part::read_member(bytes, DOCUMENT_PART, cap).map_err(|e| match e {
        ZipPartError::TooLarge => DocxError::TooLarge,
        ZipPartError::Missing => {
            DocxError::Malformed(format!("{DOCUMENT_PART} is not in the archive"))
        }
        ZipPartError::Malformed => DocxError::Malformed(format!(
            "{DOCUMENT_PART} could not be read out of the archive"
        )),
    })?;

    let headings = match zip_part::read_member(bytes, STYLES_PART, cap) {
        Ok(styles) => heading_styles(&crate::text::decode(&styles)),
        Err(ZipPartError::TooLarge) => return Err(DocxError::TooLarge),
        Err(ZipPartError::Missing | ZipPartError::Malformed) => HashSet::new(),
    };

    // `text::decode`, the crate's one guess, rather than strict UTF-8 — the
    // measurement is `epub.rs`'s and it applies unchanged: chardetng answered
    // UTF-8 for every UTF-8 fixture including the shortest, and read a
    // windows-1251 structure document that strict UTF-8 turns into replacement
    // characters. It is also `text.rs`'s own promise, that the same bytes are
    // read the same way whichever reader opens them.
    let sections = parse(&crate::text::decode(&document), &headings)?;

    if sections.iter().all(|section| section.blocks.is_empty()) {
        return Err(DocxError::NoText);
    }
    Ok(sections)
}

/// The style ids `word/styles.xml` says are headings.
///
/// A stylesheet that will not parse names none, and that is not an error: the
/// document is still read, by the canonical ids. This is the one XML document
/// in this crate whose failure costs nothing, which is why it swallows the
/// error rather than propagating it.
fn heading_styles(styles: &str) -> HashSet<String> {
    let mut reader = quick_xml::Reader::from_str(styles);
    let mut headings = HashSet::new();
    let mut id: Option<String> = None;
    let mut heading = false;
    let mut in_style = false;

    // `while let Ok(…)`, so a stylesheet that will not parse simply stops
    // naming headings — the document is still read. This is the one XML
    // document in this crate whose failure is allowed to be silent.
    while let Ok(event) = reader.read_event() {
        match event {
            quick_xml::events::Event::Eof => break,
            quick_xml::events::Event::Start(ref e) | quick_xml::events::Event::Empty(ref e) => {
                match e.local_name().as_ref() {
                    b"style" => {
                        id = attribute(e, b"styleId");
                        heading = false;
                        in_style = true;
                    }
                    // The built-in style's primary name. Locale-independent in
                    // principle and not in practice — three files in the probe
                    // carry a Cyrillic one — which is why it is the *second*
                    // signal rather than the first.
                    b"name" if in_style => {
                        if let Some(name) = attribute(e, b"val")
                            && names_a_heading(&name)
                        {
                            heading = true;
                        }
                    }
                    // The first signal, and the unambiguous one: an outline
                    // level of 0–8 is precisely what "this style is a heading at
                    // level N" is spelled as. 9 means body text, which is why
                    // the range is checked rather than the attribute's presence.
                    b"outlineLvl" if in_style => {
                        if let Some(level) = attribute(e, b"val")
                            && is_outline_level(&level)
                        {
                            heading = true;
                        }
                    }
                    _ => {}
                }
            }
            quick_xml::events::Event::End(ref e) if e.local_name().as_ref() == b"style" => {
                if heading && let Some(id) = id.take() {
                    headings.insert(id);
                }
                id = None;
                heading = false;
                in_style = false;
            }
            _ => {}
        }
    }

    headings
}

/// Whether a style's stated name is a built-in heading's.
fn names_a_heading(name: &str) -> bool {
    name.trim().to_ascii_lowercase().starts_with("heading")
}

/// Whether an `<w:outlineLvl>` value names a heading level.
///
/// 0–8 are heading levels 1–9; **9 is body text**, and reading the attribute's
/// presence rather than its value would make every ordinary paragraph style
/// that states it a heading.
fn is_outline_level(value: &str) -> bool {
    value.trim().parse::<u32>().is_ok_and(|level| level <= 8)
}

/// A style id Word writes for a built-in heading when nothing has renamed it.
///
/// The third signal and the weakest, and it exists for exactly one case: a
/// document whose `word/styles.xml` this reader could not open. Matched
/// **exactly and case-sensitively**, as `identify_plain_text` matches an
/// extension, because `Heading1` is a value a producer writes rather than a word
/// a person types.
fn is_canonical_heading_id(id: &str) -> bool {
    id.strip_prefix("Heading")
        .and_then(|level| level.parse::<u32>().ok())
        .is_some_and(|level| (1..=9).contains(&level))
}

/// Whether the paragraph now open is a heading, by the three signals in order
/// of how much each one knows.
fn is_heading(paragraph: &Paragraph, headings: &HashSet<String>) -> bool {
    if let Some(level) = &paragraph.outline {
        return is_outline_level(level);
    }
    match &paragraph.style {
        Some(style) => headings.contains(style) || is_canonical_heading_id(style),
        None => false,
    }
}

/// What is known about the paragraph currently open.
///
/// A stack of these rather than one, because `<w:p>` nests: a text box lives
/// inside a run of the paragraph that anchors it and holds paragraphs of its
/// own. Its text belongs to *its* paragraph, not to the one around it.
#[derive(Default)]
struct Paragraph {
    /// What `<w:pStyle>` names, if anything.
    style: Option<String>,
    /// The paragraph's own `<w:outlineLvl>`, which overrides its style's.
    outline: Option<String>,
    /// Resolved once `<w:pPr>` closes, when both of the above are known and
    /// before any run has arrived.
    heading: bool,
    /// Whether this paragraph has already opened its page. A paragraph split
    /// into several blocks — by a text box in the middle of it — must open one
    /// page, not one per block.
    opened: bool,
    /// A paragraph inside `<w:tbl>`. Its blocks are `Table` and it opens no
    /// section, however it is styled — see [`parse`].
    in_table: bool,
}

/// Reads the part into sections.
///
/// **Matched on local names**, so a producer using a different prefix for the
/// WordprocessingML namespace reads the same — the mistake `epub.rs`'s C30 case
/// measures for a package document, where a colon cost a whole book. What it
/// costs here is that DrawingML's own `<a:t>`, the text inside a chart or a
/// diagram, is read as well. That is text the page shows, so it is kept; the
/// cost is that a chart label anchored mid-paragraph joins that paragraph rather
/// than standing alone.
///
/// **A paragraph inside a table cell does not open a section**, whatever its
/// style. Header cells are styled constantly, and one page per cell would
/// shatter a document into dozens of sections named after column headings.
/// The cost is stated rather than hidden: a heading somebody put inside a
/// one-cell layout table is text rather than a section.
fn parse(part: &str, headings: &HashSet<String>) -> Result<Vec<DocxSection>, DocxError> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(part);
    let mut sections = vec![DocxSection {
        page_no: 1,
        section_title: None,
        blocks: Vec::new(),
    }];

    // The text between two paragraph boundaries, and the paragraphs currently
    // open.
    let mut run = String::new();
    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut table_depth = 0usize;
    let mut ppr_depth = 0usize;
    // Non-zero while inside a subtree whose text is not this document's. A
    // depth rather than a name, so nesting needs nothing remembered.
    let mut skip_depth = 0usize;
    // Non-zero while inside `<w:t>`. **This is what excludes `<w:instrText>`
    // and `<w:delText>`**: they are text nodes of elements with other names, so
    // nothing collects them. Excluded by construction is exactly what the PDF
    // reader's first version also claimed, so `tests/docx.rs` asserts both.
    let mut text_depth = 0usize;
    // Element nesting, for the one thing quick-xml will not report: a part that
    // stops inside an element. Measured — the reader answers `Event::Eof`
    // without an error, so a reader without this counter stores the prose before
    // the cut and says nothing about the rest.
    let mut depth = 0i64;

    loop {
        let event = reader
            .read_event()
            .map_err(|e| DocxError::Malformed(format!("{DOCUMENT_PART} does not parse: {e}")))?;
        match event {
            Event::Eof => break,
            Event::Start(ref e) => {
                depth += 1;
                if skip_depth > 0 {
                    skip_depth += 1;
                    continue;
                }
                match e.local_name().as_ref() {
                    name if gives_no_text(name) => skip_depth = 1,
                    b"tbl" => table_depth += 1,
                    b"p" => {
                        // Before the push, so text belonging to the paragraph
                        // around this one is stored as that paragraph's.
                        flush(&mut run, paragraphs.last_mut(), &mut sections);
                        paragraphs.push(Paragraph {
                            in_table: table_depth > 0,
                            ..Paragraph::default()
                        });
                    }
                    b"pPr" => ppr_depth += 1,
                    b"t" => text_depth += 1,
                    _ => properties(e, ppr_depth, paragraphs.last_mut()),
                }
            }
            Event::Empty(ref e) => {
                if skip_depth > 0 {
                    continue;
                }
                match e.local_name().as_ref() {
                    // The characters an element stands for. Dropping any of
                    // them stores a word that is in no file and that a search
                    // for either half will not find — `html.rs` measured that
                    // shape as `передпісля`.
                    //
                    // **`<w:tab/>` only outside `<w:pPr>`**, and that guard is
                    // the whole of this arm's value: the identical element name
                    // inside `<w:pPr><w:tabs>` is a tab *stop* on the ruler.
                    // The probe counted 394 `<w:tab>` against 254 `<w:tabs>`
                    // containers, so a large share of them are definitions and
                    // an unguarded arm puts tabs at the front of paragraph after
                    // paragraph.
                    b"tab" if ppr_depth == 0 => run.push('\t'),
                    b"br" | b"cr" => run.push('\n'),
                    // A hyphen the document paints. `<w:softHyphen/>` is
                    // deliberately not here: it is invisible unless the line
                    // happens to break there, so a character for it would put
                    // one in text no copy of the document shows.
                    b"noBreakHyphen" => run.push('-'),
                    b"p" => flush(&mut run, paragraphs.last_mut(), &mut sections),
                    _ => properties(e, ppr_depth, paragraphs.last_mut()),
                }
            }
            Event::Text(ref e) => {
                if skip_depth > 0 || text_depth == 0 {
                    continue;
                }
                // `xml_content`, not `decode`: XML defines a CRLF inside content
                // to *be* a line feed, so this is what the document says rather
                // than a rewriting of it — the same call `epub.rs` makes on an
                // attribute, and the same version to assume when a part declares
                // none. It is the one transformation D38 does not forbid,
                // because the two spellings are the same character.
                if let Ok(text) = e.xml_content(quick_xml::XmlVersion::Implicit1_0) {
                    run.push_str(&text);
                }
            }
            // **In this version of quick-xml an escape is an event of its own**,
            // not part of the text around it: `Чай &amp; кава` arrives as three
            // events, and a reader that handled only `Event::Text` would store
            // `Чай  кава` — a sentence the document does not contain, in the
            // most ordinary punctuation there is. Measured, not read: the first
            // version of this file called `unescape()` on the text event, which
            // 0.41 does not have.
            Event::GeneralRef(ref e) => {
                if skip_depth > 0 || text_depth == 0 {
                    continue;
                }
                match resolve_reference(e) {
                    Some(text) => run.push_str(&text),
                    // A reference to an entity no DTD here declares. Kept as the
                    // characters it is written with rather than dropped: the cost
                    // is a literal `&name;` inside a block, and the alternative
                    // is a hole in the middle of a sentence.
                    None => {
                        run.push('&');
                        run.push_str(&e.decode().unwrap_or_default());
                        run.push(';');
                    }
                }
            }
            // Not what Word writes, and cheap to accept: a `<![CDATA[…]]>`
            // inside `<w:t>` is that run's text with no references to resolve.
            Event::CData(ref e) => {
                if skip_depth == 0
                    && text_depth > 0
                    && let Ok(text) = e.decode()
                {
                    run.push_str(&text);
                }
            }
            Event::End(ref e) => {
                if depth == 0 {
                    return Err(DocxError::Malformed(format!(
                        "{DOCUMENT_PART} closes an element nothing opened"
                    )));
                }
                depth -= 1;
                if skip_depth > 0 {
                    skip_depth -= 1;
                    continue;
                }
                match e.local_name().as_ref() {
                    b"tbl" => table_depth = table_depth.saturating_sub(1),
                    b"p" => {
                        flush(&mut run, paragraphs.last_mut(), &mut sections);
                        paragraphs.pop();
                    }
                    b"pPr" => {
                        ppr_depth = ppr_depth.saturating_sub(1);
                        // Resolved here, and here is the only place it can be:
                        // `<w:pPr>` is the first child of `<w:p>`, so by now
                        // both signals are known and no run has arrived yet.
                        if ppr_depth == 0
                            && let Some(paragraph) = paragraphs.last_mut()
                        {
                            paragraph.heading = is_heading(paragraph, headings);
                        }
                    }
                    b"t" => text_depth = text_depth.saturating_sub(1),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if depth != 0 {
        return Err(DocxError::Malformed(format!(
            "{DOCUMENT_PART} ends inside an element"
        )));
    }

    // Text the part left outside every paragraph, which a well-formed document
    // does not produce. Kept because "every run is closed by a `</w:p>`" is a
    // claim about the producer, and losing the tail of a document to it would
    // be silent.
    flush(&mut run, paragraphs.last_mut(), &mut sections);

    Ok(sections)
}

/// Subtrees whose text is not this document's, or is this document's twice.
///
/// Closed, and every member is a measured shape of the format rather than a
/// guess — see the module doc for what each one costs if it is read.
fn gives_no_text(name: &[u8]) -> bool {
    matches!(
        name,
        b"Fallback" | b"pPrChange" | b"rPrChange" | b"moveFrom"
    )
}

/// Reads `<w:pStyle>` and `<w:outlineLvl>` into the paragraph now open.
///
/// Guarded on `<w:pPr>` rather than on the element name alone: both appear
/// inside `<w:pPrChange>` as well, where they describe what the paragraph
/// **used to be**. That subtree is skipped whole, so this guard is the second
/// of two — kept because the first is a list and a list is a thing that gets
/// edited.
fn properties(
    e: &quick_xml::events::BytesStart<'_>,
    ppr_depth: usize,
    paragraph: Option<&mut Paragraph>,
) {
    if ppr_depth == 0 {
        return;
    }
    let Some(paragraph) = paragraph else {
        return;
    };
    match e.local_name().as_ref() {
        b"pStyle" => paragraph.style = attribute(e, b"val"),
        b"outlineLvl" => paragraph.outline = attribute(e, b"val"),
        _ => {}
    }
}

/// Closes the run in progress as a block of the page currently open, opening a
/// new page first if a heading paragraph is what produced it.
///
/// A run that is nothing but whitespace produces no block: it is an empty
/// paragraph, which Word writes a great many of, and a block holding `"\n"` is
/// searchable, citable and empty of content — `markdown.rs`'s argument for
/// dropping a thematic break.
fn flush(run: &mut String, paragraph: Option<&mut Paragraph>, sections: &mut Vec<DocxSection>) {
    if run.trim().is_empty() {
        run.clear();
        return;
    }
    // Once per block, over the text the parse produced, and before anything
    // downstream takes an offset or a hash from it (D32, D38). Nothing else
    // touches it: no folding, no reflow, no trimming — which is also why
    // `xml:space="preserve"` is never read. That attribute exists because an
    // XML consumer is otherwise free to trim a `<w:t>`; this one never does, so
    // honouring it could only ever remove characters the file contains.
    let text = nfc::normalise(run).into_owned();
    run.clear();

    let (block_type, opens) = match paragraph {
        Some(paragraph) if paragraph.in_table => (BlockType::Table, None),
        Some(paragraph) if paragraph.heading && !paragraph.opened => {
            paragraph.opened = true;
            (BlockType::Headline, Some(()))
        }
        Some(paragraph) if paragraph.heading => (BlockType::Headline, None),
        _ => (BlockType::Paragraph, None),
    };

    if opens.is_some() {
        // Flattened onto one line and bounded, exactly as a markdown heading's
        // is and for the same reason: no offset is ever measured into a title,
        // so unlike `block.text` it is display metadata rather than evidence.
        // **After NFC**, which the line above already ran: normalisation changes
        // the character count, so bounding first would cut in the wrong place.
        // `None` for a heading whose text is only whitespace — a page named by
        // the empty string is worse than an unnamed page.
        if let Some(title) =
            bound_section_title(text.split_whitespace().collect::<Vec<_>>().join(" "))
        {
            open_page(sections, title);
        }
    }

    let section = sections.last_mut().expect("a page is always open");
    section.blocks.push(Block {
        block_type,
        // Restarts on every page: the schema's uniqueness is on
        // `(page_id, reading_order)`, because reading order is what
        // reconstructs a page rather than a document.
        reading_order: section.blocks.len() as i64,
        // Nothing here detects language; a per-block guess is the extraction
        // spec's subject, as in every other reader.
        language: None,
        text,
        // A DOCX has no lines until something lays it out. `pages_of` gives
        // this reader `Fixed(Coordinate::Section)` precisely because of it, and
        // a number invented here would be cited as "рядки 1–1" of a document
        // that has none.
        line_start: None,
        line_end: None,
    });
}

/// Starts the page a heading opens — or names the one already open, when that
/// page is still empty and unnamed.
///
/// Identical in effect to `markdown.rs`'s and `html.rs`'s: without the second
/// case, a document that begins with a heading would carry an untitled page 1
/// with no blocks on it and every real section would be numbered one higher
/// than it is.
fn open_page(sections: &mut Vec<DocxSection>, title: String) {
    let empty_and_unnamed = sections
        .last()
        .is_some_and(|section| section.blocks.is_empty() && section.section_title.is_none());
    if empty_and_unnamed {
        sections.last_mut().expect("just inspected").section_title = Some(title);
        return;
    }
    sections.push(DocxSection {
        page_no: sections.len() as u32 + 1,
        section_title: Some(title),
        blocks: Vec::new(),
    });
}

/// The characters an `&…;` stands for, or `None` for a reference this parser
/// cannot resolve on its own.
///
/// Two kinds, and only two exist without a DTD: a character reference
/// (`&#1080;`, `&#x438;`), which quick-xml resolves, and the five entities XML
/// itself defines, which it does not — there is no entity table without a
/// document type declaration, and a `word/document.xml` has none. Everything
/// else is a reference to something declared nowhere.
fn resolve_reference(reference: &quick_xml::events::BytesRef<'_>) -> Option<String> {
    if let Ok(Some(character)) = reference.resolve_char_ref() {
        return Some(character.to_string());
    }
    let name = reference.decode().ok()?;
    let resolved = match name.as_ref() {
        "lt" => "<",
        "gt" => ">",
        "amp" => "&",
        "apos" => "'",
        "quot" => "\"",
        _ => return None,
    };
    Some(resolved.to_string())
}

/// One attribute's value, by local name, with the escapes XML defines already
/// undone.
///
/// The same call `epub.rs` makes on a package document's attributes and for the
/// same reason: `&amp;` in a style's name is `&`, and `Implicit1_0` is the
/// version to assume when a document declares none. `None` on an escape the
/// parser will not resolve, which for a style id means the style is treated as
/// unnamed rather than looked up under a literal `&somename;`.
fn attribute(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|attribute| {
        (attribute.key.local_name().as_ref() == name)
            .then(|| {
                attribute
                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .ok()
                    .map(|value| value.into_owned())
            })
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A docx of the given members, each Deflated.
    ///
    /// Deliberately compressible: the archive is a few hundred bytes and what
    /// comes out of it is not, which is the only shape a cap is for.
    fn archive(members: &[(&str, usize)]) -> Vec<u8> {
        use std::io::{Cursor, Write};

        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let deflated: zip::write::FileOptions<()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, padding) in members {
                w.start_file(*name, deflated).unwrap();
                // Well-formed either way: a comment is legal in both parts, and
                // padding one is how a member is made to inflate past a cap
                // without becoming something the parser would reject first.
                w.write_all(b"<w:x><!--").unwrap();
                w.write_all(&vec![b'a'; *padding]).unwrap();
                w.write_all(b"--></w:x>").unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    /// **Both members are read under the cap, and a stylesheet's failures are
    /// not all alike.**
    ///
    /// Three assertions doing three different jobs, and **each one puts exactly
    /// one member over the cap**. That is not tidiness: this test's first
    /// version made a document *and* a stylesheet oversized together, and the
    /// mutation that uncaps the document part stayed green — the stylesheet was
    /// still refusing the same archive, so the assertion never described the
    /// member it named. Measured, in `scripts/mutations/task-12.sh`'s first run.
    ///
    /// The first is the cap the server measured and capped on the stream for
    /// (`app/textdoc/office.py:41-52`). The second is the hole the first leaves:
    /// a cap on `word/document.xml` alone is not a cap on this reader, because a
    /// stylesheet is a second member out of the same archive and inflates just
    /// as far. The third is why the second cannot simply be "any stylesheet
    /// failure is fatal" — a stylesheet that is not there at all is an ordinary
    /// document, and refusing it would cost a document over a part that holds no
    /// prose.
    #[test]
    fn both_members_are_read_under_the_cap_and_only_absence_is_forgiven() {
        // The document alone over the cap, with a stylesheet well under it.
        let fat_document = archive(&[("word/document.xml", 4096), ("word/styles.xml", 16)]);
        assert!(matches!(
            extract(&fat_document, 1024),
            Err(DocxError::TooLarge)
        ));

        // The stylesheet alone over the cap, with a document well under it: the
        // refusal has to come from the second member, so nothing else can be
        // what produced it.
        let fat_styles = archive(&[("word/document.xml", 16), ("word/styles.xml", 4096)]);
        assert!(matches!(
            extract(&fat_styles, 1024),
            Err(DocxError::TooLarge)
        ));

        // And the same document with no stylesheet at all is read — which is
        // what stops the arm above from being written as "any failure refuses".
        // `NoText`, because this fixture is a comment and holds no prose: the
        // point is that it got as far as the parse rather than being refused at
        // the member.
        let no_styles = archive(&[("word/document.xml", 16)]);
        assert!(matches!(extract(&no_styles, 1024), Err(DocxError::NoText)));
    }

    /// The three signals, each on its own, and the value that is **not** a
    /// heading level.
    ///
    /// Both directions in one test: a predicate that answered true for
    /// everything would pass the first three assertions, and one that answered
    /// false for everything would pass the last three.
    #[test]
    fn a_heading_is_recognised_by_any_of_the_three_signals_and_by_nothing_else() {
        let mut headings = HashSet::new();
        headings.insert("11".to_string());

        let styled = |style: &str| Paragraph {
            style: Some(style.to_string()),
            ..Paragraph::default()
        };
        let outlined = |level: &str| Paragraph {
            outline: Some(level.to_string()),
            ..Paragraph::default()
        };

        // The stylesheet said so; the id says so on its own; the paragraph says
        // so on its own.
        assert!(is_heading(&styled("11"), &headings));
        assert!(is_heading(&styled("Heading3"), &headings));
        assert!(is_heading(&outlined("0"), &headings));

        // 9 is body text, not a tenth heading level.
        assert!(!is_heading(&outlined("9"), &headings));
        // A style the stylesheet knows and does not call a heading.
        assert!(!is_heading(&styled("a5"), &headings));
        // No style and no level at all.
        assert!(!is_heading(&Paragraph::default(), &headings));

        // **The paragraph's own level overrides its style's**, in the direction
        // that matters: a paragraph explicitly set to body text is not a
        // heading even under a heading style.
        assert!(!is_heading(
            &Paragraph {
                style: Some("Heading1".to_string()),
                outline: Some("9".to_string()),
                ..Paragraph::default()
            },
            &headings
        ));
    }

    /// `Heading1`–`Heading9` and nothing that merely looks like one.
    #[test]
    fn only_the_nine_canonical_heading_ids_are_canonical() {
        for id in ["Heading1", "Heading9"] {
            assert!(is_canonical_heading_id(id), "{id}");
        }
        for id in ["Heading0", "Heading10", "heading1", "Heading", "Heading1a"] {
            assert!(!is_canonical_heading_id(id), "{id}");
        }
    }
}

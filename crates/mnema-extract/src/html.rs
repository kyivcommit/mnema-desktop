//! The HTML reader: what the page would show becomes prose, and a heading
//! opens a section.
//!
//! **This is the one format in the product that was already wrong rather than
//! merely unread.** `identify_plain_text` had no arm for `.html`, so it fell to
//! the text reader and was indexed whole — measured, one block holding
//! `<!DOCTYPE html>…<style>.a{color:red}</style>…<script>var x=1;</script>…`
//! (spec §2.1). A search hit `color:red`; a citation highlighted markup. Every
//! other format in this cycle refuses honestly and gains a reader; this one
//! stops lying — and for the files an index already holds, only because
//! `mnema_ingest::ingest_file` now rebuilds a document whose reader changed.
//!
//! A section is opened by a heading, `h1`–`h6`, **and by `<title>`**; see
//! [`section_title`] for why the second is not decoration.
//!
//! **The rule that decides what is text: what this file would show a reader.**
//! It is not "everything outside `<script>` and `<style>`", and the difference
//! is measured rather than tidy. `<noscript>`, `<iframe>`, `<noembed>` and
//! `<noframes>` are parsed as *raw text*, so their content arrives as a single
//! text node holding literal markup — `"<p>увімкніть JS</p>"`, one string, tags
//! included. Indexing that is the same defect as indexing CSS, wearing a
//! different tag. `<template>` is inert until a script runs, and this product
//! runs none. So [`gives_no_text`] holds all seven, and the two that look like
//! they belong there and do not — `<xmp>` and `<plaintext>` — are kept, because
//! a browser paints their raw text on the page exactly as it stands.
//!
//! **Verbatim, here, cannot mean "a slice of the file", and pretending
//! otherwise would be the lie in a new place.** Two measured reasons:
//! `&amp;`, `&nbsp;` and `&#1081;` reach the tree as `&`, U+00A0 and `й`, so
//! the characters a reader sees are not the bytes on disk; and a sentence
//! crossing `<b>` arrives as three text nodes, so no contiguous run of the file
//! contains it. What D32 and D38 do bind, and what is enforced here, is that
//! **nothing folds the text the parser hands over**: the server's
//! `_clean = " ".join(text.split())` (`app/textdoc/html_blocks.py:41-42`) is
//! **not** ported (G7.1 §2.3), so runs of spaces, tabs, newlines and
//! non-breaking spaces survive exactly as they are, and NFC is the only pass
//! over the text. It runs once, over the whole document, before parsing — the
//! same order `markdown.rs` and `text.rs` use.
//!
//! The visible cost of not folding: a `<p>` whose prose is indented in the
//! source keeps that indentation at the block's edges. The alternative is a
//! second rule about whitespace that nothing downstream can undo, which is
//! precisely what §2.3 refused.
//!
//! **Nothing else may vanish, and that is a partition rather than a promise.**
//! Every text node outside a [`gives_no_text`] subtree is appended to exactly
//! one block, in document order; the traversal has no exit that ends it early
//! and no list of elements it knows how to descend into. An element this file
//! has never heard of — a web component, a tag from 1998 — ends the run it
//! interrupts and starts a new block, so an unknown name costs a block
//! boundary, never a paragraph. `tests/html.rs`'s
//! `every_word_the_page_would_show_lands_in_exactly_one_block` holds it to that
//! against a fixture whose prose is listed literally.
//!
//! **What is knowingly lost**, each with its reason:
//!
//! - text that only an attribute carries — `alt`, `title`, `aria-label`. It is
//!   not a text node, and reading attributes would need a per-attribute
//!   decision about which ones are prose;
//! - a `<frameset>` document, which the HTML parsing algorithm strips of its
//!   body entirely — measured, `<frameset><frame></frameset>текст` yields no
//!   text at all. A browser shows the framed files, which are separate
//!   documents this reader will meet on their own;
//! - `<![CDATA[…]]>`, which HTML (unlike XHTML) parses as a comment — again,
//!   what a browser does.
//!
//! **One measured limit that belongs to the parser, not to this file.**
//! html5ever's tree building is quadratic in nesting depth: 10,000 nested
//! `<div>`s parse in 0.17 s, 25,000 in 0.90 s, 50,000 in 3.6 s and 100,000 in
//! 14.6 s (release build, this machine). The traversal below is linear and
//! costs 0.000 s at every one of those. A file deep enough to matter is a
//! megabyte of nothing but tags; it occupies one worker process, which is what
//! that process is for, and no reader-side cap can fix a cost paid inside
//! `parse_document`.

use ego_tree::NodeRef;
use ego_tree::iter::Edge;
use html5ever::ns;
use scraper::node::Element;
use scraper::{Html, Node};

use mnema_core::{Block, BlockType, nfc};

use crate::markdown::bound_section_title;

/// One page of an HTML file: a section, or the run of content before the first
/// heading.
///
/// The same three things as [`crate::MarkdownPage`] and deliberately not the
/// same type — see that one's doc for why a reader's page is not the pool's.
///
/// `blocks` carry no line numbers, and that is not a gap left to fill.
/// `mnema_ingest::pages_of` gives this reader
/// `PageContext::Fixed(Coordinate::Section)` precisely *because* an HTML block
/// has no row to name: the file's line numbers do not survive the parse, and a
/// number invented here would cite "рядки 1–1" of a document that has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlPage {
    /// 1-based and consecutive: this reader skips no page, so unlike a PDF's it
    /// never leaves a gap.
    pub page_no: u32,
    pub section_title: Option<String>,
    pub blocks: Vec<Block>,
}

/// Reads HTML into pages, one per section.
///
/// No `Result`, for the same reason `extract_text` and `extract_markdown` have
/// none: decoding cannot fail (an invalid byte sequence becomes U+FFFD), and
/// the HTML parsing algorithm has no failure mode — it is defined to build a
/// tree out of any byte string at all, which is what "stray closers" and
/// "unclosed tags" mean below.
///
/// Always at least one page, including for an empty file: `block.page_id` is
/// NOT NULL, so the four-level model needs a page even where the format has
/// none (D37), and the pool compares the header's count against the page frames
/// that arrive.
///
/// Encoding is guessed by the crate's shared [`crate::text::decode`] rather
/// than read from `<meta charset>`, and that is measured rather than assumed.
/// A declaration is a stated fact and normally beats a guess — but chardetng
/// answered `windows-1251` and `KOI8-U` correctly for Ukrainian prose in both
/// a long and a four-character document, and answered `UTF-8` for a UTF-8 file
/// carrying `<meta charset="windows-1251">`, which the declaration would have
/// mojibaked. One detector also keeps the promise `text.rs` makes: the same
/// bytes read as `.txt`, `.md` and `.html` are decoded the same way.
pub fn extract_html(bytes: &[u8]) -> Vec<HtmlPage> {
    let decoded = crate::text::decode(bytes);
    // Once, over everything, before the parser sees it and before any text is
    // taken out of it (D32, D38). Tag names, attribute names and character
    // references are ASCII, so normalising the markup with the prose changes
    // nothing about the tree.
    let source = nfc::normalise(&decoded);
    let document = Html::parse_document(&source);

    let mut pages = vec![HtmlPage {
        page_no: 1,
        section_title: None,
        blocks: Vec::new(),
    }];

    // The text between two block boundaries, and the boundary elements
    // currently open. `flow` holds a block type per open non-inline element, so
    // the run's type is whatever encloses it at the moment it is flushed —
    // which is why every flush below happens *before* the matching push or pop.
    let mut run = String::new();
    let mut flow: Vec<BlockType> = Vec::new();
    // Non-zero while inside a subtree that gives no text. A depth rather than a
    // node id: `traverse` emits `Close` for every `Open`, including for text
    // nodes, so counting is exact and needs nothing to remember.
    let mut skipping = 0usize;

    // **`traverse`, not recursion, and not a loop over a list of elements this
    // file knows.** It visits every node of the tree exactly twice and has no
    // way to stop early — which is the property the whole partition rests on.
    // The PDF reader paid for the other shape once already: a loop that ends on
    // an element it cannot handle loses the rest of the document silently.
    for edge in document.tree.root().traverse() {
        match edge {
            Edge::Open(node) => {
                if skipping > 0 {
                    skipping += 1;
                    continue;
                }
                match node.value() {
                    Node::Element(element) => {
                        if gives_no_text(element) {
                            skipping = 1;
                            continue;
                        }
                        // An inline element is part of the sentence around it,
                        // so it neither ends the run nor types it.
                        if is_inline(element) {
                            continue;
                        }
                        flush(&mut run, &flow, &mut pages);
                        flow.push(block_type(element));
                        if let Some(title) = section_title(node) {
                            open_page(&mut pages, title);
                        }
                    }
                    Node::Text(text) => run.push_str(&text.text),
                    // A comment, the doctype, a processing instruction, the
                    // document root and a template's content fragment. None of
                    // them is text and none of them is an element, so none of
                    // them bounds a block.
                    _ => {}
                }
            }
            Edge::Close(node) => {
                if skipping > 0 {
                    skipping -= 1;
                    continue;
                }
                if let Node::Element(element) = node.value()
                    && !is_inline(element)
                {
                    flush(&mut run, &flow, &mut pages);
                    flow.pop();
                }
            }
        }
    }
    // The document root's own `Close` is inside the loop, so this only ever
    // catches text the tree left outside every element — which the parsing
    // algorithm does not produce. Kept because "the loop always flushes last"
    // is a claim about html5ever, and losing the tail of a document to it
    // would be silent.
    flush(&mut run, &flow, &mut pages);

    pages
}

/// Closes the run in progress as a block of the page currently open.
///
/// A run that is nothing but whitespace produces no block: it is the
/// indentation between two tags, and a block holding `"\n            "` is
/// searchable, citable and empty of content — the same argument `markdown.rs`
/// makes for dropping a thematic break. It is also the only text this function
/// drops, and it drops no characters that any block would have shown.
fn flush(run: &mut String, flow: &[BlockType], pages: &mut [HtmlPage]) {
    if run.trim().is_empty() {
        run.clear();
        return;
    }
    let block_type = flow.last().copied().unwrap_or(BlockType::Paragraph);
    let page = pages.last_mut().expect("a page is always open");
    page.blocks.push(Block {
        block_type,
        // Restarts on every page: the schema's uniqueness is on
        // `(page_id, reading_order)`, because reading order is what
        // reconstructs a page rather than a document.
        reading_order: page.blocks.len() as i64,
        // Nothing here detects language; a per-block guess is the extraction
        // spec's subject, as in every other reader.
        language: None,
        text: std::mem::take(run),
        line_start: None,
        line_end: None,
    });
}

/// Starts the page a heading opens — or names the one already open, when that
/// page is still empty and unnamed.
///
/// Identical in effect to `markdown.rs`'s: without the second case, a document
/// that begins with a heading would carry an untitled page 1 with no blocks on
/// it and every real section would be numbered one higher than it is.
fn open_page(pages: &mut Vec<HtmlPage>, title: String) {
    let empty_and_unnamed = pages
        .last()
        .is_some_and(|page| page.blocks.is_empty() && page.section_title.is_none());
    if empty_and_unnamed {
        pages.last_mut().expect("just inspected").section_title = Some(title);
        return;
    }
    pages.push(HtmlPage {
        page_no: pages.len() as u32 + 1,
        section_title: Some(title),
        blocks: Vec::new(),
    });
}

/// The title of the section this element opens, or `None` if it opens none.
///
/// **`<title>` opens one, and that is a deliberate addition to "a heading
/// opens a section".** `pages_of` cites an HTML chunk as
/// `Coordinate::Section { title }` and renders an unnamed page as the empty
/// string, so a document with no headings — a single-page report, a mail
/// export — would cite nothing at all. Spec §6 invariant 1 asks every format
/// for a non-empty coordinate and
/// `mnema-ingest/tests/slice.rs::a_page_that_names_no_section_carries_an_empty_one_rather_than_none`
/// writes the obligation down as this reader's. A document's `<title>` is the
/// name it gives itself, and using it costs one extra page whenever a `<title>`
/// and an `<h1>` both exist.
///
/// The namespace test is not decoration: SVG has a `<title>` of its own and it
/// is a tooltip, so `<svg><title>підказка</title></svg>` would otherwise open a
/// section named after a mouseover.
///
/// The title is flattened onto one line and bounded, exactly as a markdown
/// heading's is, and for the same reason: no offset is ever measured into a
/// title, so unlike `block.text` it is display metadata rather than evidence.
/// The element's *own* text is still stored verbatim as its block.
fn section_title<'a>(element: NodeRef<'a, Node>) -> Option<String> {
    if !opens_a_section(element.value().as_element()?) {
        return None;
    }
    let mut words: Vec<&'a str> = Vec::new();
    let mut skipping = 0usize;
    for edge in element.traverse() {
        match edge {
            Edge::Open(node) => {
                if skipping > 0 {
                    skipping += 1;
                    continue;
                }
                match node.value() {
                    Node::Element(inner) if gives_no_text(inner) => skipping = 1,
                    Node::Text(text) => words.extend(text.text.split_whitespace()),
                    _ => {}
                }
            }
            Edge::Close(_) => skipping = skipping.saturating_sub(1),
        }
    }
    // A heading with no text at all — `<h1></h1>`, or one holding only an
    // empty `<b>` — is not a section. A page named by the empty string is
    // worse than an unnamed page: it renders as a section that exists and has
    // no name.
    bound_section_title(words.join(" "))
}

fn opens_a_section(element: &Element) -> bool {
    element.name.ns == ns!(html)
        && matches!(
            element.name(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "title"
        )
}

/// Which of `block.type`'s eight an element's own text is.
///
/// Only the mappings an HTML element really has; everything else is a
/// paragraph rather than a guess. `<header>` and `<footer>` are deliberately
/// **not** `PageHeader`/`PageFooter` — those name the running furniture of a
/// printed page, which is a PDF's problem, and a web page's `<footer>` is
/// content.
fn block_type(element: &Element) -> BlockType {
    if element.name.ns != ns!(html) {
        return BlockType::Paragraph;
    }
    match element.name() {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "title" => BlockType::Headline,
        // Preformatted, all three: `<xmp>` and `<plaintext>` are `<pre>`'s
        // ancestors and their raw content is painted as written.
        "pre" | "xmp" | "plaintext" | "listing" => BlockType::Code,
        "td" | "th" => BlockType::Table,
        "caption" | "figcaption" => BlockType::Caption,
        _ => BlockType::Paragraph,
    }
}

/// Elements whose content is not this document's text.
///
/// Closed, and every member measured rather than assumed:
///
/// - `<script>` and `<style>` are code, and are the defect spec §2.1 measured
///   in the index today. Matched by name in **any** namespace, because
///   `<svg><script>` exists and its content came back as text;
/// - `<noscript>`, `<iframe>`, `<noembed>` and `<noframes>` are fallback
///   content that a browser with scripting on does not render, and html5ever
///   hands each of them back as one text node of *literal markup*
///   (`"<p>увімкніть JS</p>"`), which is markup-as-prose by another route;
/// - `<template>` is inert until a script clones it, and nothing here runs
///   scripts.
///
/// The cost is named rather than hidden: prose that only appears when scripting
/// is off, or only inside a client-side template, is not indexed.
fn gives_no_text(element: &Element) -> bool {
    matches!(
        element.name(),
        "script" | "style" | "noscript" | "iframe" | "noembed" | "noframes" | "template"
    )
}

/// Elements that are part of the sentence around them rather than a block of
/// their own.
///
/// **The list is closed and the direction of its failure is the point.** An
/// element that is not on it ends the run it interrupts, so a name this build
/// has never seen — a web component, a tag from 1998, an element added to HTML
/// after this release — costs one block boundary. The opposite default would
/// merge two paragraphs into one block whenever an unknown element separated
/// them. Neither loses text; only one of them puts words next to each other
/// that the document does not.
///
/// `<br>` is deliberately absent although it is inline: it *is* a line break,
/// and joining across it would store `першийдругий` for a document that shows
/// two lines. Splitting there costs a block boundary the chunker rejoins with
/// `mnema_chunk::JOIN`; gluing costs a word that is in no file.
///
/// `<img>`, `<picture>` and `<input>` carry no text at all and are here only so
/// that an illustration in the middle of a sentence does not cut it in two.
fn is_inline(element: &Element) -> bool {
    matches!(
        element.name(),
        "a" | "abbr"
            | "b"
            | "bdi"
            | "bdo"
            | "big"
            | "cite"
            | "code"
            | "data"
            | "del"
            | "dfn"
            | "em"
            | "font"
            | "i"
            | "img"
            | "input"
            | "ins"
            | "kbd"
            | "mark"
            | "nobr"
            | "picture"
            | "q"
            | "rp"
            | "rt"
            | "rtc"
            | "ruby"
            | "s"
            | "samp"
            | "small"
            | "span"
            | "strike"
            | "strong"
            | "sub"
            | "sup"
            | "time"
            | "tt"
            | "u"
            | "var"
            | "wbr"
    )
}

//! An HTML file, or one chapter of an EPUB, rendered for the page view with a
//! citation's characters marked (D199).
//!
//! The file is read again through the index's own reader — `html::parse`, then
//! `html::walk`, then for a chapter `epub::number_chapter` — and the tree that
//! walk read is the tree that is drawn. The walk says which text nodes each
//! block was built from and where each one starts in it ([`TextAt`]), so a
//! citation counted in a block's text lands in exactly those nodes: a sentence
//! crossing `<b>` is a mark in each node it touches, and the allow-list writes
//! one `<mark>` per piece.
//!
//! Nothing is marked unless the blocks read now hash to the index's, and a
//! block NFC rewrote (`unverbatim`) is never marked: its offsets count the
//! stored text, not the nodes'.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use mnema_core::Block;
use mnema_core::render::{
    DocRange, DocRenderReply, DocRenderRequest, RenderError, TextMatch, doc_text_sha256,
};
use scraper::node::Element;
use scraper::{Html, Node};

use super::markdown::MAX_HTML_BYTES;
use super::sanitize::{MARK_END, MARK_OTHER, MARK_PRIMARY, has_marks, sanitize_tree};
use crate::epub::{ChapterError, chapter_image, chapter_source, number_chapter};
use crate::html::{HeadTitle, TextAt, names_the_document, parse, walk};

/// One document read the way the index read it, its tree kept.
struct Read {
    doc: Html,
    /// `(page_no, block)`, in the order the index stores them.
    blocks: Vec<(u32, Block)>,
    texts: Vec<TextAt>,
    unverbatim: Vec<(u32, u32)>,
}

/// Renders `bytes` as one HTML document for `req`.
pub(crate) fn render_html(bytes: &[u8], req: &DocRenderRequest) -> DocRenderReply {
    html_within(bytes, req, MAX_HTML_BYTES)
}

fn html_within(bytes: &[u8], req: &DocRenderRequest, limit: usize) -> DocRenderReply {
    let doc = parse(bytes);
    let w = walk(&doc, HeadTitle::IsText);
    let pages = w.pages.len() as u32;
    let read = Read {
        blocks: w
            .pages
            .into_iter()
            .flat_map(|p| {
                let n = p.page_no;
                p.blocks.into_iter().map(move |b| (n, b))
            })
            .collect(),
        texts: w.texts,
        unverbatim: w.unverbatim,
        doc,
    };
    // A standalone file's `<title>` is a block of it, so it is shown. Its
    // pictures are not: a file's `src` names the disk, not an archive.
    render(read, pages, req, |_| false, limit, &mut |_| None)
}

/// The most one picture of a chapter may inflate to before it is its alt text
/// instead. The largest measured in a real library was 307 KB.
const IMAGE_MAX_BYTES: usize = 2 << 20;

/// The most a chapter's pictures may add to its HTML, as `data:` URIs, counted
/// in document order: the first picture past it and every later one are alt
/// text. The heaviest measured chapter held 5.2 MB of pictures.
const CHAPTER_IMAGES_MAX_BYTES: usize = 16 << 20;

/// What a picture is, by its first bytes and never by its name: an SVG (a
/// document that can carry script) or anything else the window should not be
/// handed as an image is `None`.
fn image_type(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if b.len() >= 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Renders chapter `req.page_no` of the book in `bytes`.
pub(crate) fn render_epub(bytes: &[u8], req: &DocRenderRequest) -> DocRenderReply {
    epub_within(
        bytes,
        req,
        MAX_HTML_BYTES,
        IMAGE_MAX_BYTES,
        CHAPTER_IMAGES_MAX_BYTES,
    )
}

fn epub_within(
    bytes: &[u8],
    req: &DocRenderRequest,
    limit: usize,
    image_cap: usize,
    image_budget: usize,
) -> DocRenderReply {
    let Some(page_no) = req.page_no else {
        return refused(RenderError::Unreadable, "no chapter named".into());
    };
    let source = match chapter_source(bytes, page_no) {
        Ok(source) => source,
        Err(ChapterError::TooLarge) => {
            return refused(
                RenderError::TooLarge,
                "a member of this EPUB inflates past its cap".into(),
            );
        }
        Err(ChapterError::NotAChapter) => {
            return refused(
                RenderError::Unreadable,
                format!("spine entry {page_no} is not a chapter"),
            );
        }
        Err(ChapterError::Malformed(why)) => return refused(RenderError::Unreadable, why),
    };
    let doc = parse(&source.bytes);
    let c = number_chapter(page_no, walk(&doc, HeadTitle::NamesOnly));
    if c.blocks.is_empty() {
        // A cover or a plate: the index skipped it, so there is no page of the
        // document to show.
        return refused(
            RenderError::Unreadable,
            format!("chapter {page_no} holds no text"),
        );
    }
    let read = Read {
        blocks: c.blocks.into_iter().map(|b| (page_no, b)).collect(),
        texts: c.texts,
        unverbatim: c.unverbatim,
        doc,
    };
    // A chapter's own `<title>` is no block of it (`HeadTitle::NamesOnly`), so
    // the view does not show it either; SVG's `<title>` is a block and stays.
    let mut left = image_budget;
    let mut images = |src: &str| {
        let data = chapter_image(bytes, &source.path, src, image_cap)?;
        let mime = image_type(&data)?;
        let uri = format!("data:{mime};base64,{}", STANDARD.encode(&data));
        if uri.len() > left {
            // This one and every later one: the budget is spent in reading order.
            left = 0;
            return None;
        }
        left -= uri.len();
        Some(uri)
    };
    render(
        read,
        source.spine_len,
        req,
        names_the_document,
        limit,
        &mut images,
    )
}

fn refused(error: RenderError, message: String) -> DocRenderReply {
    DocRenderReply::Refused { error, message }
}

fn render(
    mut read: Read,
    pages: u32,
    req: &DocRenderRequest,
    skip: fn(&Element) -> bool,
    limit: usize,
    images: &mut dyn FnMut(&str) -> Option<String>,
) -> DocRenderReply {
    let hash = doc_text_sha256(
        read.blocks
            .iter()
            .map(|(p, b)| (*p, b.block_type.as_str(), b.text.as_str())),
    );
    let text = match &req.text_sha256 {
        None => TextMatch::None,
        Some(sent) if *sent != hash => TextMatch::Mismatch,
        Some(_) if tree_has_marks(&read.doc) => TextMatch::Unplaced,
        Some(_) if !place_all(&mut read, &req.ranges) => TextMatch::Unplaced,
        Some(_) => TextMatch::Match,
    };
    // No fresh parse after a refused placement, unlike markdown.rs: with
    // `marks` false the allow-list drops every mark character, so a
    // half-marked tree draws exactly what a clean one does.
    // The limit is on the markup: a chapter's pictures have their own budget.
    let mut image_bytes = 0;
    let html = sanitize_tree(
        read.doc.tree.root(),
        text == TextMatch::Match,
        skip,
        &mut |src| {
            let uri = images(src)?;
            image_bytes += uri.len();
            Some(uri)
        },
    );
    if html.len() - image_bytes > limit {
        return refused(RenderError::TooLarge, format!("over {limit} bytes of HTML"));
    }
    DocRenderReply::Rendered { html, pages, text }
}

/// Whether any text node of the file already holds a mark character —
/// whether or not it reaches the view: `&#xE000;` is one once parsed.
fn tree_has_marks(doc: &Html) -> bool {
    doc.tree
        .nodes()
        .any(|n| matches!(n.value(), Node::Text(t) if has_marks(t)))
}

/// Puts every range into the text nodes of its block. False — and the caller
/// renders unmarked — when a range names no block, names one NFC rewrote,
/// runs past its block's text, or covers no character.
fn place_all(read: &mut Read, ranges: &[DocRange]) -> bool {
    for r in ranges {
        let key = (r.page_no, r.reading_order);
        let Some((_, block)) = read
            .blocks
            .iter()
            .find(|(p, b)| (*p, b.reading_order) == (r.page_no, i64::from(r.reading_order)))
        else {
            return false;
        };
        if read.unverbatim.contains(&key) {
            return false;
        }
        let chars = block.text.chars().count();
        if r.start
            .checked_add(r.len)
            .is_none_or(|end| end as usize > chars)
        {
            return false;
        }
    }
    let mut hit = vec![false; ranges.len()];
    for at in &read.texts {
        let Some(mut node) = read.doc.tree.get_mut(at.node) else {
            continue;
        };
        if let Node::Text(t) = node.value()
            && let Some(marked) = mark_text(&t.text, at, ranges, &mut hit)
        {
            t.text = marked.as_str().into();
        }
    }
    hit.iter().all(|&h| h)
}

/// One text node with the marks of the ranges that touch it, `None` when none
/// does. Every boundary is found first and the string built once, so no
/// insertion shifts another; overlapping citations collapse to the strongest
/// that covers each stretch, as `markdown::mark_node` does.
fn mark_text(text: &str, at: &TextAt, ranges: &[DocRange], hit: &mut [bool]) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let (from, to) = (at.start, at.start + chars.len() as u32);
    let mine = |r: &DocRange| (r.page_no, r.reading_order) == (at.page_no, at.reading_order);
    let mut cuts = vec![from, to];
    for r in ranges.iter().filter(|r| mine(r)) {
        cuts.extend(
            [r.start, r.start + r.len]
                .into_iter()
                .filter(|&x| x > from && x < to),
        );
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut out = String::with_capacity(text.len() + 8);
    let mut state: Option<bool> = None;
    let mut marked = false;
    for pair in cuts.windows(2) {
        let (x, y) = (pair[0], pair[1]);
        let mut here: Option<bool> = None;
        for (i, r) in ranges.iter().enumerate() {
            if mine(r) && r.start < y && x < r.start + r.len {
                hit[i] = true;
                here = Some(here.unwrap_or(false) || r.primary);
            }
        }
        if here != state {
            out.push(match here {
                Some(true) => MARK_PRIMARY,
                Some(false) => MARK_OTHER,
                None => MARK_END,
            });
            state = here;
            marked = true;
        }
        out.extend(&chars[(x - from) as usize..(y - from) as usize]);
    }
    if state.is_some() {
        out.push(MARK_END);
    }
    marked.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epub::tests::{XHTML, spine_book};
    use crate::{extract_epub, extract_html};
    use mnema_core::render::DocReader;

    fn hash_of<'a>(blocks: impl IntoIterator<Item = (u32, &'a Block)>) -> String {
        doc_text_sha256(
            blocks
                .into_iter()
                .map(|(p, b)| (p, b.block_type.as_str(), b.text.as_str())),
        )
    }

    /// The hash the application would send for an HTML file.
    fn html_hash(bytes: &[u8]) -> String {
        let pages = extract_html(bytes);
        hash_of(
            pages
                .iter()
                .flat_map(|p| p.blocks.iter().map(move |b| (p.page_no, b))),
        )
    }

    /// The hash of chapter `n` as the index stores it.
    fn chapter_hash(book: &[u8], n: u32) -> String {
        let book = extract_epub(book).unwrap();
        let c = book.chapters.iter().find(|c| c.page_no == n).unwrap();
        hash_of(c.blocks.iter().map(|b| (n, b)))
    }

    fn html_req(bytes: &[u8], ranges: Vec<DocRange>) -> DocRenderRequest {
        DocRenderRequest {
            path: String::new(),
            reader: DocReader::Html,
            page_no: None,
            text_sha256: Some(html_hash(bytes)),
            ranges,
        }
    }

    /// The range of `needle` in the first stored block holding it.
    fn range_in<'a>(
        pages: impl IntoIterator<Item = (u32, &'a [Block])>,
        needle: &str,
        primary: bool,
    ) -> DocRange {
        for (page_no, blocks) in pages {
            for b in blocks {
                if let Some(at) = b.text.find(needle) {
                    return DocRange {
                        page_no,
                        reading_order: b.reading_order as u32,
                        start: b.text[..at].chars().count() as u32,
                        len: needle.chars().count() as u32,
                        primary,
                    };
                }
            }
        }
        panic!("{needle:?} in no block");
    }

    fn html_range(bytes: &[u8], needle: &str, primary: bool) -> DocRange {
        let pages = extract_html(bytes);
        range_in(
            pages.iter().map(|p| (p.page_no, p.blocks.as_slice())),
            needle,
            primary,
        )
    }

    fn rendered(reply: DocRenderReply) -> (String, TextMatch) {
        match reply {
            DocRenderReply::Rendered { html, text, .. } => (html, text),
            other => panic!("{other:?}"),
        }
    }

    fn primary_marks(html: &str) -> Vec<String> {
        html.split("<mark data-primary=\"true\">")
            .skip(1)
            .map(|rest| rest.split("</mark>").next().unwrap().to_string())
            .collect()
    }

    const DOC: &str = "<html><head><title>Назва</title><style>p{color:red}</style>\
        <script>alert(1)</script></head><body>\n<h1>Вступ</h1>\n\
        <p onclick=\"x()\" style=\"color:red\">Перший абзац з <b>жирним словом</b> тут.</p>\n\
        <p>Посилання <a href=\"https://e.com\">тут</a> і кінець.</p>\n</body></html>";

    #[test]
    fn a_cited_phrase_crossing_an_element_is_marked_and_the_text_is_intact() {
        let b = DOC.as_bytes();
        let r = html_range(b, "з жирним", true);
        let (html, text) = rendered(render_html(b, &html_req(b, vec![r])));
        assert_eq!(text, TextMatch::Match);
        assert_eq!(primary_marks(&html), ["з ", "жирним"], "{html}");
        assert!(
            html.contains(
                "<p>Перший абзац <mark data-primary=\"true\">з </mark><b><mark data-primary=\"true\">жирним</mark> словом</b> тут.</p>"
            ),
            "{html}"
        );
    }

    #[test]
    fn a_neighbour_is_marked_without_primary_and_each_range_in_its_own_block() {
        let b = DOC.as_bytes();
        let other = html_range(b, "Посилання", false);
        // Same offsets, another block: marks only its own.
        let primary = html_range(b, "Вступ", true);
        let (html, text) = rendered(render_html(b, &html_req(b, vec![other, primary])));
        assert_eq!(text, TextMatch::Match);
        assert!(
            html.contains("<p><mark>Посилання</mark> <span class=\"link\">тут</span>"),
            "{html}"
        );
        assert_eq!(primary_marks(&html), ["Вступ"], "{html}");
        assert_eq!(html.matches("<mark").count(), 2, "{html}");
    }

    #[test]
    fn a_changed_file_is_rendered_without_marks() {
        let b = DOC.as_bytes();
        let mut request = html_req(b, vec![html_range(b, "жирним", true)]);
        request.text_sha256 = Some(html_hash("<p>Інший файл</p>".as_bytes()));
        let (html, text) = rendered(render_html(b, &request));
        assert_eq!(text, TextMatch::Mismatch);
        assert!(!html.contains("<mark"), "{html}");
        assert!(html.contains("жирним"), "{html}");
    }

    #[test]
    fn no_hash_is_no_comparison() {
        let b = DOC.as_bytes();
        let mut request = html_req(b, vec![html_range(b, "жирним", true)]);
        request.text_sha256 = None;
        let (html, text) = rendered(render_html(b, &request));
        assert_eq!(text, TextMatch::None);
        assert!(!html.contains("<mark"), "{html}");
    }

    #[test]
    fn a_file_holding_a_mark_character_is_not_marked() {
        for spelled in ["\u{E000}", "&#xE000;", "&#57346;"] {
            let doc = format!("<p>Перший {spelled} абзац.</p><p>Другий абзац.</p>");
            let b = doc.as_bytes();
            let (html, text) = rendered(render_html(
                b,
                &html_req(b, vec![html_range(b, "Другий", true)]),
            ));
            assert_eq!(text, TextMatch::Unplaced, "{spelled}");
            assert!(!html.contains("<mark"), "{spelled}: {html}");
            assert!(!has_marks(&html), "{spelled}: {html:?}");
        }
    }

    #[test]
    fn a_block_nfc_rewrote_is_not_marked() {
        let b = "<p>и&#774; тут</p><p>просто</p>".as_bytes();
        let (html, text) = rendered(render_html(
            b,
            &html_req(b, vec![html_range(b, "тут", true)]),
        ));
        assert_eq!(text, TextMatch::Unplaced);
        assert!(!html.contains("<mark"), "{html}");
        // Control: the plain block next to it is placed.
        let (_, text) = rendered(render_html(
            b,
            &html_req(b, vec![html_range(b, "просто", true)]),
        ));
        assert_eq!(text, TextMatch::Match);
    }

    #[test]
    fn a_range_that_cannot_be_placed_marks_nothing_at_all() {
        let b = DOC.as_bytes();
        let good = html_range(b, "жирним", true);
        let block_len = "Перший абзац з жирним словом тут.".chars().count() as u32;
        for bad in [
            DocRange { page_no: 9, ..good },
            DocRange {
                reading_order: 9,
                ..good
            },
            // One past the block's last character.
            DocRange {
                start: block_len - 2,
                len: 3,
                ..good
            },
            DocRange { len: 0, ..good },
            DocRange {
                start: u32::MAX,
                len: 2,
                ..good
            },
        ] {
            let (html, text) = rendered(render_html(b, &html_req(b, vec![good, bad])));
            assert_eq!(text, TextMatch::Unplaced, "{bad:?}");
            assert!(!html.contains("<mark"), "{bad:?}: {html}");
        }
        // Control: the block's very last character is in it.
        let last = DocRange {
            start: block_len - 1,
            len: 1,
            ..good
        };
        let (html, text) = rendered(render_html(b, &html_req(b, vec![last])));
        assert_eq!(text, TextMatch::Match);
        assert_eq!(primary_marks(&html), ["."], "{html}");
    }

    #[test]
    fn overlapping_citations_keep_the_primary_where_it_is() {
        let b = DOC.as_bytes();
        let other = html_range(b, "Перший абзац з", false);
        let primary = html_range(b, "абзац", true);
        let (html, _) = rendered(render_html(b, &html_req(b, vec![other, primary])));
        assert_eq!(primary_marks(&html), ["абзац"], "{html}");
        assert!(
            html.contains(
                "<p><mark>Перший </mark><mark data-primary=\"true\">абзац</mark><mark> з</mark> <b>"
            ),
            "{html}"
        );
    }

    #[test]
    fn scripts_styles_handlers_and_link_targets_do_not_reach_the_view() {
        let b = DOC.as_bytes();
        let (html, _) = rendered(render_html(b, &html_req(b, vec![])));
        for gone in [
            "<script",
            "alert",
            "<style",
            "color:red",
            "onclick",
            "href",
            "style=",
        ] {
            assert!(!html.contains(gone), "{gone}: {html}");
        }
        assert!(html.contains("<span class=\"link\">тут</span>"), "{html}");
    }

    #[test]
    fn an_html_title_is_shown_and_can_be_cited() {
        let b = DOC.as_bytes();
        let (html, text) = rendered(render_html(
            b,
            &html_req(b, vec![html_range(b, "Назва", true)]),
        ));
        assert_eq!(text, TextMatch::Match);
        assert_eq!(primary_marks(&html), ["Назва"], "{html}");
    }

    #[test]
    fn too_much_html_is_refused() {
        let b = DOC.as_bytes();
        let full = rendered(render_html(b, &html_req(b, vec![]))).0.len();
        let reply = html_within(b, &html_req(b, vec![]), full - 1);
        assert!(
            matches!(
                reply,
                DocRenderReply::Refused {
                    error: RenderError::TooLarge,
                    ..
                }
            ),
            "{reply:?}"
        );
        assert!(matches!(
            html_within(b, &html_req(b, vec![]), full),
            DocRenderReply::Rendered { .. }
        ));
    }

    // ── EPUB ────────────────────────────────────────────────────────────────

    const CH1: &str = "<html><head><title>Розділ один</title></head><body>\
        <h1>Один</h1><p>Спільне слово тут.</p></body></html>";
    const COVER: &str = "<html><head><title>Обкладинка</title></head>\
        <body><img src=\"c.jpg\"/></body></html>";
    const CH3: &str = "<html><head><title>Назва розділу</title></head><body>\
        <h1>Три</h1><p>Речення з <b>жирним</b> словом.</p>\
        <svg><title>підказка</title></svg></body></html>";

    fn book() -> Vec<u8> {
        spine_book(&[
            ("ch1.xhtml", XHTML, Some(CH1)),
            ("cover.xhtml", XHTML, Some(COVER)),
            ("ch3.xhtml", XHTML, Some(CH3)),
        ])
    }

    fn epub_req(book: &[u8], page_no: Option<u32>, ranges: Vec<DocRange>) -> DocRenderRequest {
        DocRenderRequest {
            path: String::new(),
            reader: DocReader::Epub,
            page_no,
            text_sha256: page_no.map(|n| chapter_hash(book, n)),
            ranges,
        }
    }

    fn chapter_range(book: &[u8], n: u32, needle: &str, primary: bool) -> DocRange {
        let book = extract_epub(book).unwrap();
        let c = book.chapters.iter().find(|c| c.page_no == n).unwrap();
        range_in([(n, c.blocks.as_slice())], needle, primary)
    }

    #[test]
    fn a_chapter_is_marked_with_its_own_hash_and_not_the_books() {
        let b = book();
        let r = chapter_range(&b, 3, "з жирним", true);
        let reply = render_epub(&b, &epub_req(&b, Some(3), vec![r]));
        let DocRenderReply::Rendered { html, pages, text } = reply else {
            panic!("{reply:?}");
        };
        assert_eq!((pages, text), (3, TextMatch::Match));
        assert_eq!(primary_marks(&html), ["з ", "жирним"], "{html}");
        assert!(!html.contains("Спільне"), "chapter 3 alone: {html}");

        let whole = extract_epub(&b).unwrap();
        let mut request = epub_req(&b, Some(3), vec![r]);
        request.text_sha256 = Some(hash_of(
            whole
                .chapters
                .iter()
                .flat_map(|c| c.blocks.iter().map(move |b| (c.page_no, b))),
        ));
        let (html, text) = rendered(render_epub(&b, &request));
        assert_eq!(text, TextMatch::Mismatch);
        assert!(!html.contains("<mark"), "{html}");
    }

    #[test]
    fn a_chapter_title_is_not_shown_and_an_svg_title_is() {
        let b = book();
        let r = chapter_range(&b, 3, "підказка", true);
        let (html, text) = rendered(render_epub(&b, &epub_req(&b, Some(3), vec![r])));
        assert_eq!(text, TextMatch::Match);
        assert!(!html.contains("Назва розділу"), "{html}");
        assert_eq!(primary_marks(&html), ["підказка"], "{html}");
    }

    #[test]
    fn a_chapter_that_is_not_one_or_is_not_named_is_refused() {
        let b = book();
        for page_no in [None, Some(2), Some(4), Some(0)] {
            let reply = render_epub(
                &b,
                &DocRenderRequest {
                    page_no,
                    ..epub_req(&b, None, vec![])
                },
            );
            assert!(
                matches!(
                    reply,
                    DocRenderReply::Refused {
                        error: RenderError::Unreadable,
                        ..
                    }
                ),
                "{page_no:?}: {reply:?}"
            );
        }
        let reply = render_epub(
            b"not a zip",
            &DocRenderRequest {
                page_no: Some(1),
                ..epub_req(&b, None, vec![])
            },
        );
        assert!(
            matches!(
                reply,
                DocRenderReply::Refused {
                    error: RenderError::Unreadable,
                    ..
                }
            ),
            "{reply:?}"
        );
    }

    #[test]
    fn too_much_chapter_html_is_refused() {
        let b = book();
        let full = rendered(render_epub(&b, &epub_req(&b, Some(1), vec![])))
            .0
            .len();
        assert!(matches!(
            epub_within(
                &b,
                &epub_req(&b, Some(1), vec![]),
                full - 1,
                IMAGE_MAX_BYTES,
                CHAPTER_IMAGES_MAX_BYTES
            ),
            DocRenderReply::Refused {
                error: RenderError::TooLarge,
                ..
            }
        ));
    }

    // ── EPUB pictures ───────────────────────────────────────────────────────

    const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0jpeg body";
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\npng body";

    /// A picture of `n` bytes that sniffs as JPEG.
    fn jpeg_of(n: usize) -> Vec<u8> {
        let mut b = JPEG.to_vec();
        b.resize(n, 7);
        b
    }

    /// A book whose one chapter, `OEBPS/Text/ch1.xhtml`, holds `body` and a
    /// paragraph to cite, with `pictures` beside it in the archive.
    fn picture_book(body: &str, pictures: &[(&str, &[u8])]) -> Vec<u8> {
        let chapter = format!("<html><body><p>Текст біля картинки.</p>{body}</body></html>");
        crate::epub::tests::spine_book_with(
            &[("OEBPS/Text/ch1.xhtml", XHTML, Some(&chapter))],
            pictures,
        )
    }

    fn chapter_one(b: &[u8], ranges: Vec<DocRange>) -> (String, TextMatch) {
        rendered(render_epub(b, &epub_req(b, Some(1), ranges)))
    }

    /// The bytes each shown picture's URI carries, with its type, in order.
    fn shown(html: &str) -> Vec<(String, Vec<u8>)> {
        html.split("<img src=\"data:")
            .skip(1)
            .map(|rest| {
                let uri = rest.split('"').next().unwrap();
                let (mime, data) = uri.split_once(";base64,").unwrap();
                (mime.to_string(), STANDARD.decode(data).unwrap())
            })
            .collect()
    }

    #[test]
    fn a_chapters_pictures_are_shown_from_its_own_folder() {
        let b = picture_book(
            "<img src=\"../Images/a.jpg\" alt=\"Кіт &amp; &quot;пес&quot; <x>\">\
             <img src=\"../Images/b%20c.png\" alt=\"\">",
            &[
                ("OEBPS/Images/a.jpg", JPEG),
                ("OEBPS/Images/b c.png", PNG),
                // The same name beside the chapter: not the one `../` names.
                ("OEBPS/Text/Images/a.jpg", PNG),
            ],
        );
        let (html, _) = chapter_one(&b, vec![]);
        assert_eq!(
            shown(&html),
            [
                ("image/jpeg".to_string(), JPEG.to_vec()),
                ("image/png".to_string(), PNG.to_vec())
            ],
            "{html}"
        );
        assert!(
            html.contains("\" alt=\"Кіт &amp; &quot;пес&quot; &lt;x&gt;\">"),
            "{html}"
        );
        assert!(!html.contains("img-alt"), "{html}");
    }

    #[test]
    fn a_picture_is_its_type_by_its_bytes_not_its_name() {
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>x()</script></svg>";
        let b = picture_book(
            "<img src=\"p.jpg\" alt=\"png\"><img src=\"s.png\" alt=\"svg\">\
             <img src=\"t.png\" alt=\"text\"><img src=\"s.svg\" alt=\"svg file\">",
            &[
                ("OEBPS/Text/p.jpg", PNG),
                ("OEBPS/Text/s.png", svg),
                ("OEBPS/Text/t.png", b"just some text"),
                ("OEBPS/Text/s.svg", svg),
            ],
        );
        let (html, _) = chapter_one(&b, vec![]);
        assert_eq!(
            shown(&html),
            [("image/png".to_string(), PNG.to_vec())],
            "{html}"
        );
        for alt in ["svg", "text", "svg file"] {
            assert!(
                html.contains(&format!("<span class=\"img-alt\">{alt}</span>")),
                "{alt}: {html}"
            );
        }
        assert!(!html.contains("script"), "{html}");
        assert_eq!(image_type(b"GIF89a.."), Some("image/gif"));
        assert_eq!(image_type(b"GIF87a.."), Some("image/gif"));
        assert_eq!(image_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(image_type(b"RIFF\0\0\0\0WAVEfmt "), None);
        assert_eq!(image_type(b"\xFF\xD8"), None);
    }

    #[test]
    fn a_picture_outside_the_archive_or_not_in_it_is_its_alt() {
        let b = picture_book(
            "<img src=\"../../escape.png\" alt=\"вгору\">\
             <img src=\"http://e.com/a.png\" alt=\"мережа\">\
             <img src=\"HTTPS://e.com/a.png\" alt=\"мережа2\">\
             <img src=\"data:image/png;base64,iVBO\" alt=\"дані\">\
             <img src=\"#x\" alt=\"фрагмент\"><img src=\"\" alt=\"порожньо\">\
             <img src=\"gone.png\" alt=\"нема\"><img alt=\"без src\">",
            &[
                // What `http://e.com/a.png` would name read as a path.
                ("OEBPS/Text/http:/e.com/a.png", PNG),
            ],
        );
        let (html, _) = chapter_one(&b, vec![]);
        assert!(shown(&html).is_empty(), "{html}");
        for alt in [
            "вгору",
            "мережа",
            "мережа2",
            "дані",
            "фрагмент",
            "порожньо",
            "нема",
            "без src",
        ] {
            assert!(
                html.contains(&format!("<span class=\"img-alt\">{alt}</span>")),
                "{alt}: {html}"
            );
        }
    }

    #[test]
    fn a_picture_past_its_cap_or_the_chapters_budget_is_its_alt() {
        let b = picture_book(
            "<img src=\"big.jpg\" alt=\"завелика\"><img src=\"a.jpg\" alt=\"перша\">\
             <img src=\"b.jpg\" alt=\"друга\"><img src=\"c.jpg\" alt=\"третя\">",
            &[
                ("OEBPS/Text/big.jpg", &jpeg_of(101)),
                ("OEBPS/Text/a.jpg", &jpeg_of(100)),
                ("OEBPS/Text/b.jpg", &jpeg_of(100)),
                ("OEBPS/Text/c.jpg", &jpeg_of(12)),
            ],
        );
        let uri = |n: usize| format!("data:image/jpeg;base64,{}", STANDARD.encode(jpeg_of(n)));
        // Room for the first and the third, not the second: the budget is
        // spent in reading order, and once spent, it is spent.
        let budget = uri(100).len() + uri(12).len();
        let req = epub_req(&b, Some(1), vec![]);
        let (html, _) = rendered(epub_within(&b, &req, MAX_HTML_BYTES, 100, budget));
        assert_eq!(
            shown(&html),
            [("image/jpeg".to_string(), jpeg_of(100))],
            "{html}"
        );
        for alt in ["завелика", "друга", "третя"] {
            assert!(
                html.contains(&format!("<span class=\"img-alt\">{alt}</span>")),
                "{alt}: {html}"
            );
        }
        // Control: with room, each is shown.
        let (html, _) = rendered(epub_within(&b, &req, MAX_HTML_BYTES, 101, 1 << 20));
        assert_eq!(shown(&html).len(), 4, "{html}");
    }

    #[test]
    fn a_shown_picture_keeps_nothing_of_its_element_but_alt() {
        let b = picture_book(
            "<img src=\"a.jpg\" alt=\"кіт\" onerror=\"x()\" style=\"color:red\" \
             width=\"9\" srcset=\"http://e.com/a.jpg 2x\" class=\"sx-k\" id=\"i\">",
            &[("OEBPS/Text/a.jpg", JPEG)],
        );
        let (html, _) = chapter_one(&b, vec![]);
        assert_eq!(shown(&html).len(), 1, "{html}");
        let tag = html
            .split("<img ")
            .nth(1)
            .unwrap()
            .split('>')
            .next()
            .unwrap();
        assert_eq!(
            tag,
            format!(
                "src=\"data:image/jpeg;base64,{}\" alt=\"кіт\"",
                STANDARD.encode(JPEG)
            ),
            "{html}"
        );
    }

    #[test]
    fn a_chapter_heavy_with_pictures_is_marked_and_not_too_large() {
        // 9 MB of pictures: past the markup's limit, inside the pictures' own.
        let big = jpeg_of(1_800_000);
        let body: String = (0..5)
            .map(|n| format!("<img src=\"../Images/{n}.jpg\" alt=\"{n}\">"))
            .collect();
        let names: Vec<String> = (0..5).map(|n| format!("OEBPS/Images/{n}.jpg")).collect();
        let pictures: Vec<(&str, &[u8])> =
            names.iter().map(|n| (n.as_str(), big.as_slice())).collect();
        let b = picture_book(&body, &pictures);
        let r = chapter_range(&b, 1, "біля", true);
        let (html, text) = chapter_one(&b, vec![r]);
        assert_eq!(text, TextMatch::Match);
        assert_eq!(primary_marks(&html), ["біля"]);
        assert_eq!(shown(&html).len(), 5);
        assert!(html.len() > MAX_HTML_BYTES, "{}", html.len());
    }

    #[test]
    fn an_html_files_picture_stays_its_alt() {
        let b = "<p>Текст</p><img src=\"a.jpg\" alt=\"фото\">".as_bytes();
        let (html, _) = rendered(render_html(b, &html_req(b, vec![])));
        assert!(
            html.contains("<span class=\"img-alt\">фото</span>"),
            "{html}"
        );
        assert!(!html.contains("<img"), "{html}");
    }

    /// Every `.html`, `.htm` and `.epub` under `MNEMA_HTML_DIR`, each file or
    /// chapter in one request citing every block whole, must come back marked
    /// — the corpus check behind D199. Ignored: it needs a folder.
    #[test]
    #[ignore]
    fn every_block_of_a_real_folder_can_be_cited_whole() {
        fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    files(&path, out);
                } else if path
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| matches!(x, "html" | "htm" | "epub"))
                {
                    out.push(path);
                }
            }
        }
        fn whole(pages: &[(u32, &[Block])]) -> Vec<DocRange> {
            let mut ranges = Vec::new();
            for (page_no, blocks) in pages {
                for b in *blocks {
                    ranges.push(DocRange {
                        page_no: *page_no,
                        reading_order: b.reading_order as u32,
                        start: 0,
                        len: b.text.chars().count() as u32,
                        primary: ranges.is_empty(),
                    });
                }
            }
            ranges
        }
        fn check(what: String, reply: DocRenderReply, blocks: usize, failed: &mut Vec<String>) {
            match reply {
                DocRenderReply::Rendered { html, text, .. } => {
                    let marks = html.matches("<mark").count();
                    if text != TextMatch::Match || marks < blocks {
                        failed.push(format!("{what} {text:?} marks {marks} of {blocks}"));
                    }
                }
                other => failed.push(format!("{what} {other:?}")),
            }
        }
        let dir = std::env::var("MNEMA_HTML_DIR").expect("MNEMA_HTML_DIR");
        let mut paths = Vec::new();
        files(std::path::Path::new(&dir), &mut paths);
        let (mut docs, mut failed) = (0, Vec::new());
        for path in paths {
            let b = std::fs::read(&path).unwrap();
            if path.extension().is_some_and(|x| x == "epub") {
                let Ok(book) = extract_epub(&b) else { continue };
                for c in &book.chapters {
                    docs += 1;
                    let ranges = whole(&[(c.page_no, c.blocks.as_slice())]);
                    let mut request = epub_req(&b, Some(c.page_no), ranges);
                    request.text_sha256 = Some(hash_of(c.blocks.iter().map(|x| (c.page_no, x))));
                    let reply = render_epub(&b, &request);
                    let what = format!("{}#{}", path.display(), c.page_no);
                    check(what, reply, c.blocks.len(), &mut failed);
                }
            } else {
                docs += 1;
                let pages = extract_html(&b);
                let listed: Vec<(u32, &[Block])> = pages
                    .iter()
                    .map(|p| (p.page_no, p.blocks.as_slice()))
                    .collect();
                let blocks = pages.iter().map(|p| p.blocks.len()).sum();
                let reply = render_html(&b, &html_req(&b, whole(&listed)));
                check(path.display().to_string(), reply, blocks, &mut failed);
            }
        }
        println!("documents {docs}, not placed {}", failed.len());
        assert!(failed.is_empty(), "{failed:#?}");
    }
}

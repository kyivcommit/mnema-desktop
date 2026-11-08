//! A markdown file rendered for the page view, a citation's characters marked.
//!
//! **The marks go into the tree, never into the source** (D199, measured): a
//! private-use character in front of `#`, `- `, `>`, `|` or a fence turned the
//! block into a paragraph in every case tried, and a citation starts at the
//! start of a block more often than anywhere else. So the file is parsed clean,
//! and the marks are written into the literals of the `Text`, inline `Code` and
//! `CodeBlock` nodes a citation covers — found through inline `sourcepos`,
//! which matched the literal in 21 794 of 21 795 text nodes measured.
//!
//! A citation's offsets were counted in the index's text, so nothing is marked
//! unless the file, read again now through the index's own reader, hashes to
//! what the index holds. Markup characters (`#`, `**`, `- `) are in no literal
//! and are simply not marked; a node whose literal is not a verbatim run of the
//! source (an entity, an escape, inline code across a line) is marked whole
//! when a citation touches it.

use comrak::nodes::{AstNode, NodeValue, Sourcepos};
use comrak::options::Plugins;
use comrak::plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder};
use comrak::{Arena, Options};
use mnema_core::render::{
    DocRange, DocRenderReply, DocRenderRequest, RenderError, TextMatch, doc_text_sha256,
};

use super::sanitize::{MARK_END, MARK_OTHER, MARK_PRIMARY, has_marks, sanitize};
use crate::markdown::{Placed, markdown_source, parse_markdown, read};

/// The largest HTML one render may hand the window. The largest markdown file
/// measured (103 KB) rendered to 148 KB; past this the page view is the wrong
/// tool and the text view is still there.
pub(crate) const MAX_HTML_BYTES: usize = 8 << 20;

/// The options the view renders with: the reader's parse, and nothing raw.
fn render_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.front_matter_delimiter = Some("---".to_string());
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    // The default, named: raw HTML in the file is omitted, not passed on.
    options.render.r#unsafe = false;
    options
}

fn highlighter() -> SyntectAdapter {
    SyntectAdapterBuilder::new()
        .css_with_class_prefix("sx-")
        .build()
}

/// Renders `bytes` as one markdown document for `req`.
pub(crate) fn render_markdown(bytes: &[u8], req: &DocRenderRequest) -> DocRenderReply {
    render_within(bytes, req, MAX_HTML_BYTES)
}

fn render_within(bytes: &[u8], req: &DocRenderRequest, limit: usize) -> DocRenderReply {
    let source = markdown_source(bytes);
    let arena = Arena::new();
    let root = parse_markdown(&arena, &source);
    let (pages, placed) = read(&source, root);

    let hash = doc_text_sha256(pages.iter().flat_map(|page| {
        page.blocks
            .iter()
            .map(move |b| (page.page_no, b.block_type.as_str(), b.text.as_str()))
    }));
    let text = match &req.text_sha256 {
        None => TextMatch::None,
        Some(sent) if *sent != hash => TextMatch::Mismatch,
        Some(_) if tree_has_marks(root) => TextMatch::Unplaced,
        Some(_) if !place_all(&source, &placed, &req.ranges) => TextMatch::Unplaced,
        Some(_) => TextMatch::Match,
    };
    if text != TextMatch::Match {
        // A refused placement may have written some marks already; the view
        // is rendered from a clean parse rather than from a half-marked tree.
        let arena = Arena::new();
        let root = parse_markdown(&arena, &source);
        let (_, placed) = read(&source, root);
        return finish(root, &placed, pages.len() as u32, text, limit);
    }
    finish(root, &placed, pages.len() as u32, text, limit)
}

fn finish<'a>(
    root: &'a AstNode<'a>,
    placed: &[Placed<'a>],
    pages: u32,
    text: TextMatch,
    limit: usize,
) -> DocRenderReply {
    let options = render_options();
    let adapter = highlighter();
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&adapter);

    let mut html = String::new();
    for node in root.children() {
        let mut one = String::new();
        if comrak::format_html_with_plugins(node, &options, &mut one, &plugins).is_err() {
            return DocRenderReply::Refused {
                error: RenderError::Unreadable,
                message: "the markdown could not be rendered".into(),
            };
        }
        let clean = sanitize(&one, text == TextMatch::Match);
        match placed.iter().find(|p| std::ptr::eq(p.node, node)) {
            Some(p) => {
                html.push_str(&format!(
                    "<div data-block=\"{}:{}\">",
                    p.page_no, p.reading_order
                ));
                html.push_str(&clean);
                html.push_str("</div>");
            }
            None => html.push_str(&clean),
        }
        if html.len() > limit {
            return DocRenderReply::Refused {
                error: RenderError::TooLarge,
                message: format!("over {limit} bytes of HTML"),
            };
        }
    }
    DocRenderReply::Rendered { html, pages, text }
}

/// Whether the file, as parsed, already holds a mark character in any text
/// that reaches the view. The parsed tree, not the source: `&#xE000;` holds
/// none of the three characters on disk and one in its `Text` literal, and the
/// literal is what the allow-list would draw (security review, 2026-11-08).
/// comrak 0.54 happens to decode those entities to U+FFFD; this does not lean
/// on that.
fn tree_has_marks<'a>(root: &'a AstNode<'a>) -> bool {
    root.descendants()
        .any(|node| match &node.data.borrow().value {
            NodeValue::Text(t) => has_marks(t),
            NodeValue::Code(c) => has_marks(&c.literal),
            NodeValue::CodeBlock(c) => has_marks(&c.literal) || has_marks(&c.info),
            NodeValue::HtmlInline(h) => has_marks(h),
            NodeValue::HtmlBlock(h) => has_marks(&h.literal),
            _ => false,
        })
}

/// Byte offset of a 1-based `(line, column)` in `source`.
fn offset(starts: &[usize], line: usize, column: usize) -> Option<usize> {
    Some(starts.get(line.checked_sub(1)?)? + column.checked_sub(1)?)
}

/// The byte range a node occupies in `source`, end exclusive.
fn span(starts: &[usize], source: &str, at: Sourcepos) -> Option<(usize, usize)> {
    let a = offset(starts, at.start.line, at.start.column)?;
    let b = (offset(starts, at.end.line, at.end.column)? + 1).min(source.len());
    (a < b && source.is_char_boundary(a) && source.is_char_boundary(b)).then_some((a, b))
}

/// Where a literal lies in the source: `Some(start)` when it is a verbatim run
/// of `source[a..b]` (once), `None` when it is not.
fn verbatim(source: &str, (a, b): (usize, usize), literal: &str) -> Option<usize> {
    let slice = &source[a..b];
    // A code block's literal ends at a newline its fence line does not show;
    // inline code turns a line break into a space of the same length.
    let probe = literal.trim_end_matches('\n');
    if probe.is_empty() {
        return None;
    }
    let flat: String = slice
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    let at = flat.find(probe)?;
    flat[at + probe.len()..]
        .find(probe)
        .is_none()
        .then_some(a + at)
}

/// A citation range in source bytes, and whether it is the clicked one.
#[derive(Clone, Copy)]
struct Want {
    a: usize,
    b: usize,
    primary: bool,
}

/// Puts every range on the tree. False — and the caller renders clean — when a
/// range names no block, runs past its block's text, or covers no character
/// of any literal.
fn place_all<'a>(source: &str, placed: &[Placed<'a>], ranges: &[DocRange]) -> bool {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(
            source
                .bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| i + 1),
        )
        .collect();
    // Ranges grouped by the block they name, in source bytes.
    let mut wants: Vec<(&Placed<'a>, Vec<Want>)> = Vec::new();
    for r in ranges {
        let Some(block) = placed
            .iter()
            .find(|p| p.page_no == r.page_no && p.reading_order == r.reading_order)
        else {
            return false;
        };
        let Some(end) = block_end(&starts, source, block) else {
            return false;
        };
        let text = &source[block.start..end];
        let byte = |chars: u32| {
            text.char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(text.len()))
                .nth(chars as usize)
        };
        let (Some(a), Some(b)) = (byte(r.start), byte(r.start + r.len)) else {
            return false;
        };
        let want = Want {
            a: block.start + a,
            b: block.start + b,
            primary: r.primary,
        };
        match wants.iter_mut().find(|(p, _)| std::ptr::eq(*p, block)) {
            Some((_, list)) => list.push(want),
            None => wants.push((block, vec![want])),
        }
    }
    let mut hit = vec![false; ranges.len()];
    let mut index = 0;
    for (block, list) in &wants {
        for node in block.node.descendants() {
            mark_node(
                source,
                &starts,
                node,
                list,
                &mut hit[index..index + list.len()],
            );
        }
        index += list.len();
    }
    hit.iter().all(|&h| h)
}

/// The end of a placed block's text — its last line, without the terminator.
fn block_end(starts: &[usize], source: &str, block: &Placed<'_>) -> Option<usize> {
    let at = block.node.data.borrow().sourcepos;
    let last = if at.end.column == 0 {
        at.end.line.checked_sub(1)?
    } else {
        at.end.line
    };
    let mut end = match starts.get(last) {
        Some(&next) => next - 1,
        None => source.len(),
    };
    if end > block.start && source.as_bytes()[end - 1] == b'\r' {
        end -= 1;
    }
    (end >= block.start).then_some(end)
}

/// Writes the marks of `wants` into one node's literal, if it has one.
fn mark_node<'a>(
    source: &str,
    starts: &[usize],
    node: &'a AstNode<'a>,
    wants: &[Want],
    hit: &mut [bool],
) {
    let mut data = node.data.borrow_mut();
    let at = data.sourcepos;
    let Some(range) = span(starts, source, at) else {
        return;
    };
    let literal: &mut String = match &mut data.value {
        NodeValue::Text(t) => t.to_mut(),
        NodeValue::Code(c) => &mut c.literal,
        NodeValue::CodeBlock(c) => &mut c.literal,
        _ => return,
    };
    match verbatim(source, range, literal) {
        Some(from) => {
            let to = from + literal.len();
            // Every boundary inside the literal, then one state per stretch:
            // overlapping citations collapse to the strongest that covers it.
            let mut cuts = vec![from, to];
            for w in wants {
                cuts.extend([w.a, w.b].into_iter().filter(|&x| x > from && x < to));
            }
            cuts.sort_unstable();
            cuts.dedup();
            let mut out = String::with_capacity(literal.len() + 8);
            let mut state: Option<bool> = None;
            for pair in cuts.windows(2) {
                let (x, y) = (pair[0], pair[1]);
                let mut here: Option<bool> = None;
                for (i, w) in wants.iter().enumerate() {
                    if w.a < y && x < w.b {
                        hit[i] = true;
                        here = Some(here.unwrap_or(false) || w.primary);
                    }
                }
                if here != state {
                    out.push(match here {
                        Some(true) => MARK_PRIMARY,
                        Some(false) => MARK_OTHER,
                        None => MARK_END,
                    });
                    state = here;
                }
                out.push_str(&literal[x - from..y - from]);
            }
            if state.is_some() {
                out.push(MARK_END);
            }
            *literal = out;
        }
        None => {
            // Not a verbatim run: marked whole when a citation touches it.
            let mut here: Option<bool> = None;
            for (i, w) in wants.iter().enumerate() {
                if w.a < range.1 && range.0 < w.b {
                    hit[i] = true;
                    here = Some(here.unwrap_or(false) || w.primary);
                }
            }
            if let Some(primary) = here {
                let open = if primary { MARK_PRIMARY } else { MARK_OTHER };
                *literal = format!("{open}{literal}{MARK_END}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::extract_markdown;
    use mnema_core::render::DocReader;

    /// The hash the application would send: the index's blocks, which are
    /// what `extract_markdown` stored.
    fn index_hash(bytes: &[u8]) -> String {
        let pages = extract_markdown(bytes);
        doc_text_sha256(pages.iter().flat_map(|p| {
            p.blocks
                .iter()
                .map(move |b| (p.page_no, b.block_type.as_str(), b.text.as_str()))
        }))
    }

    fn req(bytes: &[u8], ranges: Vec<DocRange>) -> DocRenderRequest {
        DocRenderRequest {
            path: String::new(),
            reader: DocReader::Markdown,
            page_no: None,
            text_sha256: Some(index_hash(bytes)),
            ranges,
        }
    }

    /// The range of `needle` in block `(page, order)` of the index's text.
    fn range_of(bytes: &[u8], page: u32, order: u32, needle: &str, primary: bool) -> DocRange {
        let pages = extract_markdown(bytes);
        let text = &pages[page as usize - 1].blocks[order as usize].text;
        let at = text
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} not in {text:?}"));
        DocRange {
            page_no: page,
            reading_order: order,
            start: text[..at].chars().count() as u32,
            len: needle.chars().count() as u32,
            primary,
        }
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

    const DOC: &str = "# Вступ\n\nПерший абзац з **жирним словом** тут.\n\n## Код\n\n```rust\nfn main() { let x = 1; }\n```\n\n- пункт один\n- пункт два\n";

    #[test]
    fn a_cited_phrase_is_marked_once_without_its_markup() {
        let b = DOC.as_bytes();
        let r = range_of(b, 1, 1, "з **жирним", true);
        let (html, text) = rendered(render_markdown(b, &req(b, vec![r])));
        assert_eq!(text, TextMatch::Match);
        assert_eq!(primary_marks(&html), ["з ", "жирним"], "{html}");
        assert!(
            html.contains("<strong><mark data-primary=\"true\">жирним</mark> словом</strong>"),
            "{html}"
        );
    }

    #[test]
    fn a_mark_ending_with_its_text_node_ends_there() {
        let b = DOC.as_bytes();
        let r = range_of(b, 1, 1, "жирним словом", true);
        let (html, _) = rendered(render_markdown(b, &req(b, vec![r])));
        assert!(
            html.contains("<strong><mark data-primary=\"true\">жирним словом</mark></strong> тут."),
            "{html}"
        );
    }

    #[test]
    fn a_citation_from_the_start_of_a_heading_keeps_the_heading() {
        let b = DOC.as_bytes();
        let r = range_of(b, 2, 0, "## Код", true);
        let (html, text) = rendered(render_markdown(b, &req(b, vec![r])));
        assert_eq!(text, TextMatch::Match);
        assert!(
            html.contains(
                "<div data-block=\"2:0\"><h2><mark data-primary=\"true\">Код</mark></h2>"
            ),
            "{html}"
        );
    }

    #[test]
    fn a_neighbour_is_marked_without_primary_and_blocks_carry_their_place() {
        let b = DOC.as_bytes();
        let r = range_of(b, 2, 2, "пункт два", false);
        let (html, _) = rendered(render_markdown(b, &req(b, vec![r])));
        assert!(html.contains("<li><mark>пункт два</mark></li>"), "{html}");
        assert!(primary_marks(&html).is_empty());
        for place in ["1:0", "1:1", "2:0", "2:1", "2:2"] {
            assert!(
                html.contains(&format!("data-block=\"{place}\"")),
                "{place}: {html}"
            );
        }
    }

    #[test]
    fn code_is_highlighted_by_class_and_a_mark_inside_it_survives() {
        let b = DOC.as_bytes();
        let r = range_of(b, 2, 1, "let x", true);
        let (html, text) = rendered(render_markdown(b, &req(b, vec![r])));
        assert_eq!(text, TextMatch::Match);
        assert!(html.contains("sx-"), "{html}");
        assert!(!html.contains("style="), "{html}");
        assert_eq!(primary_marks(&html).concat(), "let x", "{html}");
    }

    #[test]
    fn a_fence_without_a_language_has_no_language_class() {
        let b = b"```\nplain\n```\n";
        let (html, _) = rendered(render_markdown(b, &req(b, vec![])));
        assert!(!html.contains("language-"), "{html}");
        assert!(html.contains("plain"), "{html}");
    }

    #[test]
    fn a_changed_file_is_rendered_without_marks() {
        let b = DOC.as_bytes();
        let mut request = req(b, vec![range_of(b, 1, 1, "жирним", true)]);
        request.text_sha256 = Some(index_hash("# Інший файл\n".as_bytes()));
        let (html, text) = rendered(render_markdown(b, &request));
        assert_eq!(text, TextMatch::Mismatch);
        assert!(!html.contains("<mark"), "{html}");
        assert!(html.contains("жирним"));
    }

    #[test]
    fn no_hash_is_no_comparison() {
        let b = DOC.as_bytes();
        let mut request = req(b, vec![range_of(b, 1, 1, "жирним", true)]);
        request.text_sha256 = None;
        let (html, text) = rendered(render_markdown(b, &request));
        assert_eq!(text, TextMatch::None);
        assert!(!html.contains("<mark"), "{html}");
    }

    #[test]
    fn a_file_holding_a_mark_character_is_not_marked() {
        let doc = "Абзац \u{E000} з символом.\n".to_string();
        let b = doc.as_bytes();
        let (html, text) = rendered(render_markdown(
            b,
            &req(b, vec![range_of(b, 1, 0, "з символом", true)]),
        ));
        assert_eq!(text, TextMatch::Unplaced);
        assert!(!html.contains("<mark"), "{html}");
    }

    #[test]
    fn a_mark_character_spelled_as_an_entity_draws_nothing() {
        // comrak 0.54 decodes these to U+FFFD (measured); `tree_has_marks`
        // holds the line if a later version passes them through.
        for spelled in ["&#xE000;", "&#57344;", "&#xe002;"] {
            let doc = format!("Перший {spelled} абзац.\n\nДругий абзац.\n");
            let b = doc.as_bytes();
            let (html, _) = rendered(render_markdown(
                b,
                &req(b, vec![range_of(b, 1, 1, "Другий", true)]),
            ));
            let first = html.split("data-block=\"1:1\"").next().unwrap();
            assert!(!first.contains("<mark"), "{spelled}: {html}");
            assert!(!has_marks(&html), "{spelled}: {html:?}");
        }
    }

    #[test]
    fn a_range_that_cannot_be_placed_marks_nothing_at_all() {
        let b = DOC.as_bytes();
        let good = range_of(b, 1, 1, "жирним", true);
        for bad in [
            DocRange { page_no: 9, ..good },
            DocRange {
                start: 10_000,
                ..good
            },
            // `**` alone: markup only, no character of any literal.
            range_of(b, 1, 1, "**", false),
        ] {
            let (html, text) = rendered(render_markdown(b, &req(b, vec![good, bad])));
            assert_eq!(text, TextMatch::Unplaced, "{bad:?}");
            assert!(!html.contains("<mark"), "{bad:?}: {html}");
        }
    }

    #[test]
    fn an_escaped_run_is_marked_whole() {
        let b = "Абзац з \\*зірочкою\\* і далі.\n".as_bytes();
        let (html, text) = rendered(render_markdown(
            b,
            &req(b, vec![range_of(b, 1, 0, "зірочкою", true)]),
        ));
        assert_eq!(text, TextMatch::Match);
        assert!(primary_marks(&html).concat().contains("зірочкою"), "{html}");
    }

    #[test]
    fn overlapping_citations_keep_the_primary_where_it_is() {
        let b = DOC.as_bytes();
        let other = range_of(b, 1, 1, "Перший абзац з", false);
        let primary = range_of(b, 1, 1, "абзац", true);
        let (html, _) = rendered(render_markdown(b, &req(b, vec![other, primary])));
        assert_eq!(primary_marks(&html), ["абзац"], "{html}");
        assert!(html.contains("<mark>Перший </mark><mark data-primary=\"true\">абзац</mark><mark> з</mark> <strong>"), "{html}");
    }

    #[test]
    fn raw_html_and_link_targets_do_not_reach_the_view() {
        let b = b"<script>alert(1)</script>\n\n[\xd1\x82\xd1\x83\xd1\x82](javascript:alert(1)) <b onclick=\"x\">a</b>\n";
        let (html, _) = rendered(render_markdown(b, &req(b, vec![])));
        for gone in ["<script", "javascript", "onclick", "href"] {
            assert!(!html.contains(gone), "{gone}: {html}");
        }
        assert!(html.contains("<span class=\"link\">тут</span>"), "{html}");
    }

    #[test]
    fn front_matter_does_not_shift_the_block_numbers() {
        let b = b"---\ntitle: x\n---\n\nAbzac\n";
        let (html, _) = rendered(render_markdown(b, &req(b, vec![])));
        assert!(
            html.starts_with("<div data-block=\"1:0\"><p>Abzac</p>"),
            "{html}"
        );
    }

    #[test]
    fn too_much_html_is_refused() {
        let b = DOC.as_bytes();
        let full = rendered(render_markdown(b, &req(b, vec![]))).0.len();
        let reply = render_within(b, &req(b, vec![]), full - 1);
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
        // Control: at exactly its size the same document renders.
        assert!(matches!(
            render_within(b, &req(b, vec![]), full),
            DocRenderReply::Rendered { .. }
        ));
    }

    /// Every block of every `.md` in `MNEMA_MD_DIR`, cited whole, must be
    /// placed — the corpus check behind D199. Ignored: it needs a folder.
    #[test]
    #[ignore]
    fn every_block_of_a_real_folder_can_be_cited_whole() {
        let dir = std::env::var("MNEMA_MD_DIR").expect("MNEMA_MD_DIR");
        let (mut blocks, mut failed) = (0, Vec::new());
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|x| x != "md") {
                continue;
            }
            let b = std::fs::read(&path).unwrap();
            for page in extract_markdown(&b) {
                for block in &page.blocks {
                    blocks += 1;
                    let r = DocRange {
                        page_no: page.page_no,
                        reading_order: block.reading_order as u32,
                        start: 0,
                        len: block.text.chars().count() as u32,
                        primary: true,
                    };
                    let (html, text) = rendered(render_markdown(&b, &req(&b, vec![r])));
                    if text != TextMatch::Match || !html.contains("data-primary") {
                        failed.push(format!(
                            "{}:{}:{} {text:?}",
                            path.display(),
                            page.page_no,
                            block.reading_order
                        ));
                    }
                }
            }
        }
        println!("blocks {blocks}, not placed {}", failed.len());
        assert!(failed.is_empty(), "{failed:#?}");
    }
}

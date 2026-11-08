//! The one door from a rendered document to the window: allow-listed HTML.
//!
//! What reaches this function is untrusted. Today it is `comrak`'s output, and
//! `comrak` already omits raw HTML; next it is an HTML file or an EPUB chapter
//! as its author wrote it. So nothing here trusts the input to be tame: every
//! element is looked up in [`allowed`], every attribute in [`keep_attr`], and
//! anything not named is dropped. The window's CSP is the second line, not the
//! first (spec R5).
//!
//! **What may vanish, and what may not.** An element the list does not name is
//! unwrapped, never dropped: its text stays, because the text is the file and
//! a tag nobody listed is not a reason to lose a paragraph. Only the seven
//! [`gives_no_text`] elements go with their content — the same seven the index
//! reader skips, so the view shows exactly the words the index holds.
//!
//! **The marks.** A citation reaches here as three private-use characters in
//! the text ([`MARK_PRIMARY`], [`MARK_OTHER`], [`MARK_END`]), put there by the
//! caller. The state "inside a mark" runs across elements, and the `<mark>` is
//! written around each piece of text only — never around a tag — so a citation
//! crossing `<strong>` is three `<mark>`s and the HTML stays properly nested by
//! construction. The characters themselves never reach the output.

use std::fmt::Write as _;

use ego_tree::NodeRef;
use ego_tree::iter::Edge;
use scraper::node::Element;
use scraper::{Html, Node};

use crate::html::gives_no_text;

/// Opens the citation the person clicked.
pub(crate) const MARK_PRIMARY: char = '\u{E000}';
/// Opens a neighbouring citation of the same document.
pub(crate) const MARK_OTHER: char = '\u{E001}';
/// Closes whichever mark is open.
pub(crate) const MARK_END: char = '\u{E002}';

/// Whether `text` holds any of the mark characters. A file that already does
/// cannot be marked: its own character would open a `<mark>` where no
/// citation is.
pub(crate) fn has_marks(text: &str) -> bool {
    text.contains([MARK_PRIMARY, MARK_OTHER, MARK_END])
}

/// What an element of the input becomes in the output.
#[derive(Clone, Copy)]
enum Out {
    /// The same tag, with its listed attributes.
    Same(&'static str),
    /// A void tag: no content, no close.
    Void(&'static str),
    /// A `span` with a fixed class instead of the element (a link the window
    /// does not follow).
    Span(&'static str),
}

const SAME: [&str; 40] = [
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "p",
    "ul",
    "ol",
    "li",
    "blockquote",
    "pre",
    "code",
    "em",
    "strong",
    "b",
    "i",
    "u",
    "s",
    "del",
    "ins",
    "sub",
    "sup",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "th",
    "td",
    "caption",
    "figure",
    "figcaption",
    "dl",
    "dt",
    "dd",
    "span",
    "div",
    "small",
    "kbd",
];

fn allowed(name: &str) -> Option<Out> {
    match name {
        "br" => Some(Out::Void("br")),
        "hr" => Some(Out::Void("hr")),
        "a" => Some(Out::Span("link")),
        _ => SAME.iter().find(|&&n| n == name).map(|&n| Out::Same(n)),
    }
}

/// The attributes an output element keeps, each already checked for shape: a
/// listed name with any value would let a file restyle the window's own marks
/// or smuggle a URL.
///
/// What comes back is only ASCII letters, digits and `-_+#` — which is also why
/// [`open_tag`] writes it without escaping: nothing here can close the quote.
fn keep_attr(tag: &str, name: &str, value: &str) -> Option<String> {
    match (tag, name) {
        ("code" | "span" | "pre", "class") => {
            let kept: Vec<&str> = value
                .split_whitespace()
                .filter(|c| is_code_class(c))
                .collect();
            (!kept.is_empty()).then(|| kept.join(" "))
        }
        ("td" | "th", "colspan" | "rowspan") | ("ol", "start") => {
            let n: u32 = value.trim().parse().ok()?;
            Some(n.to_string())
        }
        ("td" | "th", "align") => {
            matches!(value, "left" | "right" | "center").then(|| value.to_string())
        }
        _ => None,
    }
}

/// A class the syntax highlighter or a code fence names: `sx-…` (spec R6),
/// `language-…`, and the highlighter's own `syntax-highlighting`.
fn is_code_class(class: &str) -> bool {
    let tail = |prefix: &str| {
        class.strip_prefix(prefix).is_some_and(|t| {
            !t.is_empty()
                && t.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '#'))
        })
    };
    class == "syntax-highlighting" || tail("sx-") || tail("language-")
}

fn escape_text(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

fn open_tag(out: &mut String, tag: &str, element: &Element) {
    out.push('<');
    out.push_str(tag);
    // In the input's own order, so the output is deterministic for a test.
    for (name, value) in element.attrs() {
        if let Some(kept) = keep_attr(tag, name, value) {
            let _ = write!(out, " {name}=\"{kept}\"");
        }
    }
    out.push('>');
}

/// One text node, split at the marks; `mark` is the state carried between
/// nodes — `Some(true)` inside the primary citation, `Some(false)` inside a
/// neighbour.
fn write_text(out: &mut String, text: &str, mark: &mut Option<bool>) {
    let mut piece = String::new();
    let flush = |out: &mut String, piece: &mut String, mark: Option<bool>| {
        if piece.is_empty() {
            return;
        }
        match mark {
            Some(true) => out.push_str("<mark data-primary=\"true\">"),
            Some(false) => out.push_str("<mark>"),
            None => {}
        }
        escape_text(out, piece);
        if mark.is_some() {
            out.push_str("</mark>");
        }
        piece.clear();
    };
    for c in text.chars() {
        let next = match c {
            MARK_PRIMARY => Some(Some(true)),
            MARK_OTHER => Some(Some(false)),
            MARK_END => Some(None),
            _ => None,
        };
        match next {
            Some(state) => {
                flush(out, &mut piece, *mark);
                *mark = state;
            }
            None => piece.push(c),
        }
    }
    flush(out, &mut piece, *mark);
}

/// `fragment` as allow-listed HTML, its marks turned into `<mark>` — or, with
/// `marks` false, dropped: the marks are the caller's only when it placed
/// them, and a file that holds one of these characters itself must not get a
/// highlight out of it.
///
/// A mark left open at the end is closed there: the fragment is one block, and
/// a citation never runs out of the block it was counted in.
pub(crate) fn sanitize(fragment: &str, marks: bool) -> String {
    let parsed = Html::parse_fragment(fragment);
    sanitize_tree(parsed.tree.root(), marks, |_| false)
}

/// [`sanitize`] over a tree the caller has already parsed — and, for an HTML
/// file or an EPUB chapter, marked in its text nodes. `skip` drops elements
/// with their content on top of the seven [`gives_no_text`]: a chapter's
/// `<title>`, which its reader stores no block for, so the view must not show
/// it either.
pub(crate) fn sanitize_tree(
    root: NodeRef<'_, Node>,
    marks: bool,
    skip: impl Fn(&Element) -> bool,
) -> String {
    let mut out = String::new();
    let mut mark: Option<bool> = None;
    // The close tag each open element owes, `None` for one that was unwrapped.
    let mut owed: Vec<Option<&'static str>> = Vec::new();
    // Depth inside a `gives_no_text` subtree; nothing there is written.
    let mut skipping = 0usize;
    for edge in root.traverse() {
        match edge {
            Edge::Open(node) => match node.value() {
                Node::Element(element) => {
                    if skipping > 0 || gives_no_text(element) || skip(element) {
                        skipping += 1;
                        continue;
                    }
                    // An `<a>` without `href` is an anchor, not a link — and a book's
                    // `<a id="…"/>` is not closed by the HTML parser, so drawn as a link
                    // it would underline everything after it. It is unwrapped.
                    let anchor = element.name() == "a" && element.attr("href").is_none();
                    match if anchor {
                        None
                    } else {
                        allowed(element.name())
                    } {
                        Some(Out::Same(tag)) => {
                            open_tag(&mut out, tag, element);
                            owed.push(Some(tag));
                        }
                        Some(Out::Void(tag)) => {
                            let _ = write!(out, "<{tag}>");
                            owed.push(None);
                        }
                        Some(Out::Span(class)) => {
                            let _ = write!(out, "<span class=\"{class}\">");
                            owed.push(Some("span"));
                        }
                        None if element.name() == "img" => {
                            let alt = element.attr("alt").unwrap_or("").trim();
                            if !alt.is_empty() {
                                out.push_str("<span class=\"img-alt\">");
                                let alt: String = alt
                                    .chars()
                                    .filter(|&c| !matches!(c, MARK_PRIMARY | MARK_OTHER | MARK_END))
                                    .collect();
                                escape_text(&mut out, &alt);
                                out.push_str("</span>");
                            }
                            owed.push(None);
                        }
                        None => owed.push(None),
                    }
                }
                Node::Text(text) if skipping == 0 && marks => write_text(&mut out, text, &mut mark),
                Node::Text(text) if skipping == 0 => {
                    let plain: String = text
                        .chars()
                        .filter(|&c| !matches!(c, MARK_PRIMARY | MARK_OTHER | MARK_END))
                        .collect();
                    escape_text(&mut out, &plain);
                }
                _ => {}
            },
            Edge::Close(node) => {
                if let Node::Element(_) = node.value() {
                    if skipping > 0 {
                        skipping -= 1;
                        continue;
                    }
                    if let Some(Some(tag)) = owed.pop() {
                        let _ = write!(out, "</{tag}>");
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: char = MARK_PRIMARY;
    const O: char = MARK_OTHER;
    const E: char = MARK_END;

    /// The caller placed the marks: the case every test below but one is about.
    fn sanitize(fragment: &str) -> String {
        super::sanitize(fragment, true)
    }

    #[test]
    fn an_anchor_without_a_target_is_text_not_a_link() {
        let document = Html::parse_document(
            "<h1><a id=\"top\"/>Розділ</h1><p>Текст <a href=\"ch3.html\">розділу 3</a>.</p>",
        );
        assert_eq!(
            sanitize_tree(document.tree.root(), true, |_| false),
            "<h1>Розділ</h1><p>Текст <span class=\"link\">розділу 3</span>.</p>"
        );
    }

    #[test]
    fn keyboard_keys_stay_keys() {
        assert_eq!(
            sanitize("<p>Натисни <kbd>Ctrl</kbd>+<kbd>C</kbd></p>"),
            "<p>Натисни <kbd>Ctrl</kbd>+<kbd>C</kbd></p>"
        );
    }

    #[test]
    fn a_whole_document_keeps_its_text_and_a_skipped_element_goes_with_its_own() {
        let document = Html::parse_document(
            "<html><head><title>Назва</title><meta charset=\"utf-8\"></head>\
             <body><section><p>текст <i>тут</i></p></section></body></html>",
        );
        let root = document.tree.root();
        assert_eq!(
            sanitize_tree(root, true, |_| false),
            "Назва<p>текст <i>тут</i></p>"
        );
        assert_eq!(
            sanitize_tree(root, true, |e| e.name() == "title"),
            "<p>текст <i>тут</i></p>"
        );
    }

    #[test]
    fn marks_the_caller_did_not_place_are_dropped_not_drawn() {
        assert_eq!(
            super::sanitize(&format!("<p>а{P}б{E}в</p>"), false),
            "<p>абв</p>"
        );
    }

    #[test]
    fn listed_elements_stay_with_their_listed_attributes() {
        assert_eq!(
            sanitize(
                r#"<h2 id="x">Т</h2><p>а <strong>б</strong> <em>в</em></p><ol start="3"><li>г</li></ol>"#
            ),
            r#"<h2>Т</h2><p>а <strong>б</strong> <em>в</em></p><ol start="3"><li>г</li></ol>"#
        );
        assert_eq!(
            sanitize(r#"<table><tr><td align="right" colspan="2">x</td></tr></table>"#),
            r#"<table><tbody><tr><td align="right" colspan="2">x</td></tr></tbody></table>"#
        );
    }

    #[test]
    fn scripts_handlers_styles_and_urls_do_not_get_through() {
        let out = sanitize(
            r#"<p onclick="x()" style="color:red">a</p><script>alert(1)</script><style>p{}</style><iframe src="https://e.com">i</iframe><a href="javascript:alert(1)" onmouseover="y()">лінк</a><img src="x.png" onerror="z()" alt="фото">"#,
        );
        assert_eq!(
            out,
            r#"<p>a</p><span class="link">лінк</span><span class="img-alt">фото</span>"#
        );
        for gone in [
            "script",
            "alert",
            "onclick",
            "style",
            "iframe",
            "href",
            "javascript",
            "onerror",
            "src",
        ] {
            assert!(!out.contains(gone), "{gone} in {out}");
        }
    }

    #[test]
    fn an_unknown_element_keeps_its_text() {
        assert_eq!(
            sanitize("<p>до <foo-bar>тут</foo-bar> після</p>"),
            "<p>до тут після</p>"
        );
        assert_eq!(
            sanitize("<article><section>зміст</section></article>"),
            "зміст"
        );
    }

    #[test]
    fn text_and_attributes_are_escaped() {
        assert_eq!(
            sanitize("<p>a &lt;b&gt; &amp; c</p>"),
            "<p>a &lt;b&gt; &amp; c</p>"
        );
        // A class that is not a code class goes, whatever it is.
        assert_eq!(
            sanitize(r#"<code class="language-rust mark sx-k&quot;x">x</code>"#),
            r#"<code class="language-rust">x</code>"#
        );
        assert_eq!(sanitize(r#"<ol start="1&quot;x">"#), "<ol></ol>");
    }

    #[test]
    fn a_mark_crossing_an_element_is_one_mark_per_piece_of_text() {
        assert_eq!(
            sanitize(&format!("<p>а{P}б<strong>в</strong>г{E}д</p>")),
            "<p>а<mark data-primary=\"true\">б</mark><strong><mark data-primary=\"true\">в</mark></strong><mark data-primary=\"true\">г</mark>д</p>"
        );
        assert_eq!(
            sanitize(&format!("<p>{O}а{E} б</p>")),
            "<p><mark>а</mark> б</p>",
            "a neighbour has no data-primary"
        );
    }

    #[test]
    fn a_mark_open_at_the_end_is_closed_and_no_mark_character_survives() {
        let out = sanitize(&format!("<p>а{P}б</p><p>в</p>"));
        assert_eq!(
            out,
            "<p>а<mark data-primary=\"true\">б</mark></p><p><mark data-primary=\"true\">в</mark></p>"
        );
        let out = sanitize(&format!("<p>{E}{P}{O}а{E}</p><img alt=\"x{P}y\">"));
        assert!(!has_marks(&out), "{out:?}");
        assert_eq!(
            out,
            "<p><mark>а</mark></p><span class=\"img-alt\">xy</span>"
        );
    }

    #[test]
    fn has_marks_sees_each_of_the_three() {
        assert!(!has_marks("звичайний текст"));
        for c in [P, O, E] {
            assert!(has_marks(&format!("а{c}б")), "{c:?}");
        }
    }
}

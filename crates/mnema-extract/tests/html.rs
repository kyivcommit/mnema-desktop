//! The HTML reader.
//!
//! Two things are being tested here and they are not the same thing. One is
//! that markup stops reaching the index — the defect spec §2.1 measured in a
//! shipped build. The other, and the one that costs more when it is wrong, is
//! that **prose stops nowhere else**: an HTML document is a tree of elements
//! nobody can enumerate, and a reader that only knows how to descend into the
//! ones it recognises loses the rest silently, with every test green.

use mnema_core::BlockType;
use mnema_extract::{HtmlPage, extract_html};

/// Every block of every page, in order.
fn texts(pages: &[HtmlPage]) -> Vec<&str> {
    pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .map(|block| block.text.as_str())
        .collect()
}

/// One string, so that a test asking "did this reach a chunk at all" does not
/// have to care which block it landed in. Joined with a space rather than
/// concatenated, so that two blocks cannot spell a word neither of them holds.
fn all_text(pages: &[HtmlPage]) -> String {
    texts(pages).join(" ")
}

/// A string with every run of whitespace removed, for the assertions that are
/// about *which words* a block holds rather than about the spaces between
/// them. Whitespace has its own test, below, and it is exact there.
fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------- the defect

/// The measurement in spec §2.1, inverted: `sample.html` used to be indexed as
/// one block holding `<style>.a{color:red}</style>` and `var x=1;` as prose.
#[test]
fn script_and_style_do_not_become_prose() {
    let pages = extract_html(
        "<html><head><style>.a{color:red}</style></head>
            <body><h1>Річний звіт</h1><p>Виторг зріс.</p>
            <script>var x=1;</script></body></html>"
            .as_bytes(),
    );
    let text = all_text(&pages);
    assert!(!text.contains("color:red"), "CSS reached a chunk: {text:?}");
    assert!(
        !text.contains("var x"),
        "JavaScript reached a chunk: {text:?}"
    );
    // Both directions: the prose that should be there, is.
    assert!(text.contains("Виторг зріс."), "{text:?}");
    assert!(text.contains("Річний звіт"), "{text:?}");
}

/// The three that look like prose and arrive as markup.
///
/// Measured, not assumed: html5ever parses `<noscript>` (with scripting on,
/// which is its default and scraper's), `<iframe>`, `<noembed>` and
/// `<noframes>` as **raw text**, so each hands back a single text node whose
/// content is `"<p>…</p>"` — tags included, as one string. A reader that
/// excluded only `<script>` and `<style>` would put literal markup into a
/// chunk, which is the same defect the test above exists for.
#[test]
fn fallback_content_that_arrives_as_raw_markup_is_not_prose_either() {
    let pages = extract_html(
        "<body><noscript><p>Увімкніть JavaScript</p></noscript>\
         <iframe><p>Запасний вміст</p></iframe>\
         <noembed><p>Вбудований запас</p></noembed>\
         <noframes><p>Запас для фреймів</p></noframes>\
         <template><p>Шаблон</p></template>\
         <p>Справжній абзац.</p></body>"
            .as_bytes(),
    );
    let text = all_text(&pages);
    for hidden in [
        "Увімкніть",
        "Запасний",
        "Вбудований",
        "фреймів",
        "Шаблон",
        // The tags themselves, which is what these actually carry.
        "<p>",
    ] {
        assert!(
            !text.contains(hidden),
            "{hidden:?} reached a chunk: {text:?}"
        );
    }
    // And the one paragraph that is genuinely on the page still is.
    assert_eq!(words(&text), "Справжній абзац.");
}

/// `<script>` inside SVG is still a script.
///
/// The namespace test that matters, and it runs the other way from
/// `<title>`'s: `<svg><script>` is a real thing and its content came back as
/// text, so the skip list matches on the name in **any** namespace. An SVG
/// `<text>` element, on the other hand, is painted on the page and is kept.
#[test]
fn a_script_inside_svg_is_still_a_script_and_svg_text_is_still_text() {
    let pages = extract_html(
        "<body><svg><script>svgjs=1;</script><text>Напис на схемі</text></svg></body>".as_bytes(),
    );
    let text = all_text(&pages);
    assert!(!text.contains("svgjs"), "{text:?}");
    assert!(text.contains("Напис на схемі"), "{text:?}");
}

// --------------------------------------------------------------- the section

#[test]
fn a_heading_opens_a_section() {
    let pages = extract_html("<h1>Перший</h1><p>a</p><h1>Другий</h1><p>b</p>".as_bytes());
    assert_eq!(pages.len(), 2, "{pages:#?}");
    assert_eq!(pages[0].section_title.as_deref(), Some("Перший"));
    assert_eq!(pages[1].section_title.as_deref(), Some("Другий"));
    // The heading's own text is a block of its section, not only its name —
    // otherwise a document's headings are searchable nowhere.
    assert_eq!(words(&all_text(&pages[..1])), "Перший a");
    assert_eq!(words(&all_text(&pages[1..])), "Другий b");
    // Consecutive and 1-based: unlike a PDF's, this reader's pages never gap.
    assert_eq!(
        pages.iter().map(|p| p.page_no).collect::<Vec<_>>(),
        vec![1, 2]
    );
}

/// All six levels open a section, and none of them nests.
///
/// `##` under `#` opens a page of its own in markdown for the same reason: a
/// page has one `section_title` and no parent. Someone will assume otherwise,
/// so it is asserted rather than left to the doc comment.
#[test]
fn every_heading_level_opens_a_section_and_none_of_them_nests() {
    let pages = extract_html(
        "<h1>Один</h1><p>a</p><h2>Два</h2><p>b</p><h3>Три</h3><p>c</p>\
         <h4>Чотири</h4><p>d</p><h5>П'ять</h5><p>e</p><h6>Шість</h6><p>f</p>"
            .as_bytes(),
    );
    assert_eq!(
        pages
            .iter()
            .map(|p| p.section_title.as_deref().unwrap_or("—"))
            .collect::<Vec<_>>(),
        vec!["Один", "Два", "Три", "Чотири", "П'ять", "Шість"],
    );
}

/// **A document with no heading still names its section**, and the name is the
/// one the document gives itself.
///
/// `mnema_ingest::pages_of` cites an HTML chunk as `Coordinate::Section`, and
/// an unnamed page renders as the empty string — a citation pointing at
/// nothing. Spec §6 invariant 1 asks every format for a non-empty coordinate
/// and `slice.rs::a_page_that_names_no_section_carries_an_empty_one_rather_than_none`
/// records the obligation as this reader's.
///
/// Both directions, because either alone is satisfied by a mistake: the title
/// names the page, **and** its text is still a block, so a document whose only
/// prose is its title is not indexed empty.
#[test]
fn a_document_with_no_heading_is_named_by_its_title() {
    let pages = extract_html(
        "<html><head><title>Кошторис на 2026 рік</title></head>\
         <body><p>Загальна сума узгоджена.</p></body></html>"
            .as_bytes(),
    );
    assert_eq!(pages.len(), 1, "{pages:#?}");
    assert_eq!(
        pages[0].section_title.as_deref(),
        Some("Кошторис на 2026 рік")
    );
    assert_eq!(
        words(&all_text(&pages)),
        "Кошторис на 2026 рік Загальна сума узгоджена."
    );
}

/// An SVG `<title>` is a tooltip, not a document's name.
///
/// Without the namespace test in `opens_a_section` this page would be cited as
/// a section called "підказка" — non-empty, plausible, and wrong.
#[test]
fn an_svg_title_does_not_name_a_section() {
    let pages =
        extract_html("<body><svg><title>підказка</title></svg><p>текст</p></body>".as_bytes());
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].section_title, None);
}

/// A heading with no text of its own names nothing and leaves no block.
///
/// A page named by the empty string renders as a section that exists and has
/// no name, which is indistinguishable from a reader with a hole in it.
#[test]
fn a_heading_with_no_text_does_not_open_a_section() {
    let pages = extract_html("<h1></h1><h2><b></b></h2><p>текст</p>".as_bytes());
    assert_eq!(pages.len(), 1, "{pages:#?}");
    assert_eq!(pages[0].section_title, None);
    assert_eq!(texts(&pages), vec!["текст"]);
}

/// A title longer than a title is cut, and says it was cut.
///
/// The same bound markdown applies, from the same constant: `section_title` is
/// a bare `TEXT` column displayed beside a citation, and an HTML heading has no
/// length limit of its own.
#[test]
fn a_very_long_heading_is_bounded_and_the_cut_is_visible() {
    let long = "Дуже".repeat(200);
    let pages = extract_html(format!("<h1>{long}</h1><p>a</p>").as_bytes());
    let title = pages[0].section_title.as_deref().expect("a title");
    assert_eq!(
        title.chars().count(),
        mnema_extract::SECTION_TITLE_MAX_CHARS
    );
    assert!(title.ends_with('…'), "{title:?}");
    // And the block keeps the whole heading: only the *title* is display
    // metadata, the text is evidence.
    assert_eq!(texts(&pages)[0].chars().count(), long.chars().count());
}

// ------------------------------------------------------------- the partition

/// A document with one of everything, and the prose it shows, listed by hand.
///
/// The expectation is a literal rather than anything computed from the tree,
/// for the reason task 8 paid for once: a yardstick taken from the thing under
/// test moves with it. Whitespace is normalised on both sides here — the
/// verbatim test below is where spacing is exact.
const RICH: &str = r#"<!DOCTYPE html>
<html lang="uk">
  <head>
    <meta charset="utf-8">
    <title>Річний звіт</title>
    <style>.a{color:red}</style>
    <script>var x = 1;</script>
  </head>
  <body>
    <h1>Розділ 1. Виторг</h1>
    <p>Виторг <b>зріс</b> на 10&nbsp;відсотків.</p>
    <my-widget>Текст усередині невідомого елемента.</my-widget>
    <ul><li>Перший пункт</li><li>Другий пункт</li></ul>
    <table><caption>Кошторис</caption><tr><th>Стаття</th><td>Сума</td></tr></table>
    <h2>Розділ 2. Витрати</h2>
    <div><section><p>Глибоко вкладений абзац.</p>Текст поруч із ним.</section></div>
    <pre>  зберігає   пробіли</pre>
    <figure><img src="x.png" alt="це не текст"><figcaption>Підпис до рисунка</figcaption></figure>
    <noscript><p>Увімкніть JavaScript</p></noscript>
    <p>Останній абзац<br>після розриву.</p>
  </body>
</html>"#;

/// Every word this page would show a reader lands in exactly one block, in
/// order — and nothing else does.
///
/// **This is the test the module exists for.** The other direction of the same
/// claim is above: markup must not reach a chunk. This one is that prose must
/// not stop reaching it, which is the failure nobody would notice — an element
/// the traversal did not expect, a nesting it did not descend into, a loop that
/// ended on something it could not handle. Task 8's PDF reader lost two pages
/// of a three-page contract that way, silently, with the walk green.
///
/// Equality of the whole sequence rather than a `contains` per fragment:
/// `contains` is satisfied by a reader that emits one enormous block, and by
/// one that emits the same paragraph twice.
#[test]
fn every_word_the_page_would_show_lands_in_exactly_one_block() {
    let expected = [
        "Річний звіт",
        "Розділ 1. Виторг",
        "Виторг зріс на 10 відсотків.",
        "Текст усередині невідомого елемента.",
        "Перший пункт",
        "Другий пункт",
        "Кошторис",
        "Стаття",
        "Сума",
        "Розділ 2. Витрати",
        "Глибоко вкладений абзац.",
        "Текст поруч із ним.",
        "зберігає пробіли",
        "Підпис до рисунка",
        "Останній абзац",
        "після розриву.",
    ];
    let pages = extract_html(RICH.as_bytes());
    let got: Vec<String> = texts(&pages).iter().map(|t| words(t)).collect();
    assert_eq!(got, expected, "{pages:#?}");
}

/// Prose inside an element this build has never heard of is still indexed.
///
/// Named separately from the partition above because it is the class rather
/// than one instance of it: `<my-widget>` stands in for every web component,
/// every tag added to HTML after this release, and every 1998 tag nobody
/// remembers. A reader written as "descend into the elements I know" answers
/// this with silence.
#[test]
fn prose_inside_an_element_nobody_enumerated_is_still_read() {
    let pages = extract_html(
        "<body><app-root><x-panel>Текст у вебкомпоненті.</x-panel></app-root>\
         <marquee>Текст у теґу з 1998 року.</marquee></body>"
            .as_bytes(),
    );
    assert_eq!(
        texts(&pages),
        vec!["Текст у вебкомпоненті.", "Текст у теґу з 1998 року."]
    );
}

/// Structure the parser has to repair does not end the read.
///
/// Four shapes in one test, each of which could plausibly stop a traversal
/// that trusted its input: an unclosed element, a stray close tag with no
/// opener, a heading holding a paragraph (which HTML allows and nests), and a
/// paragraph that never closes at end of file.
#[test]
fn nesting_a_parser_had_to_repair_does_not_end_the_read() {
    let pages = extract_html(
        "</p></div><p>перший<p>другий<h1>Заголовок<p>усередині</p>після</h1><p>кінець".as_bytes(),
    );
    let got: Vec<String> = texts(&pages).iter().map(|t| words(t)).collect();
    assert_eq!(
        got,
        vec![
            "перший",
            "другий",
            "Заголовок",
            "усередині",
            "після",
            "кінець"
        ],
        "{pages:#?}"
    );
    // The heading opened its section even though it holds a paragraph, and the
    // paragraph's text is on that page rather than lost.
    assert_eq!(pages.len(), 2);
    assert_eq!(
        pages[1].section_title.as_deref(),
        Some("Заголовок усередині після")
    );
}

/// Deep nesting is walked iteratively, so it costs time rather than the
/// process.
///
/// 20,000 elements deep. The traversal is a loop over `Edge`s and is linear;
/// a recursive walk over the same tree is a stack overflow, which kills the
/// worker rather than refusing the file — and the pool would classify that as
/// a crash, not as anything about this document.
#[test]
fn a_deeply_nested_document_is_read_rather_than_overflowing_the_stack() {
    let depth = 20_000;
    let source = format!(
        "{}Текст на самому дні.{}",
        "<div>".repeat(depth),
        "</div>".repeat(depth)
    );
    let pages = extract_html(source.as_bytes());
    assert_eq!(texts(&pages), vec!["Текст на самому дні."]);
}

// -------------------------------------------------------------- the verbatim

/// The invariant G7.1 §2.3 states, tested with this format's own fixture.
///
/// The same test, written out in full rather than referred to, goes in
/// `tests/epub.rs`, `tests/docx.rs` and `tests/xlsx.rs`: an invariant checked
/// in one of five readers is an invariant four readers do not have.
#[test]
fn the_text_is_verbatim_after_nfc_and_nothing_else() {
    // Cyrillic й as a decomposed pair, plus a tab and a non-breaking space.
    let pages = extract_html("<p>и\u{0306}  a\tb\u{00a0}c</p>".as_bytes());
    let text = &pages[0].blocks[0].text;
    // NFC composed it…
    assert!(text.starts_with('й'), "NFC did not run (D32): {text:?}");
    // …and nothing else touched it. The server's
    // `_clean = " ".join(text.split())` (app/textdoc/html_blocks.py:41-42) is
    // NOT ported: it is applied asymmetrically there, only to html/md/epub,
    // and text stored for a citation must be what the page shows (G7.1 §2.3).
    assert!(text.contains("  "), "whitespace was collapsed: {text:?}");
    assert!(text.contains('\t'), "a tab was rewritten: {text:?}");
    assert!(
        text.contains('\u{00a0}'),
        "a non-breaking space was folded: {text:?}"
    );
    // Exactly, so that a reader which trimmed one end and not the other cannot
    // pass on the three assertions above.
    assert_eq!(text, "й  a\tb\u{00a0}c");
}

/// The other half of "verbatim": indentation inside an element is kept, and
/// whitespace *between* two elements is not a block.
///
/// The second half needs saying because it is a drop: a run holding only the
/// newline and spaces between `</p>` and `<p>` would otherwise be a block that
/// is searchable, citable and empty of content.
#[test]
fn indentation_inside_an_element_is_kept_and_the_space_between_two_is_not_a_block() {
    let pages = extract_html("<body>\n  <p>\n    Виторг зріс.\n  </p>\n</body>".as_bytes());
    assert_eq!(texts(&pages), vec!["\n    Виторг зріс.\n  "]);
}

/// A character reference is the character it names, not the six bytes that
/// spell it.
///
/// This is where "verbatim" stops meaning "a slice of the file" and the module
/// doc says why: `&amp;` on disk is `&` on the page, and a search for `R&D`
/// has to find it. The same decoding is what makes `&nbsp;` the U+00A0 the
/// test above insists on keeping.
#[test]
fn a_character_reference_arrives_as_the_character_it_names() {
    let pages = extract_html("<p>R&amp;D &#1081; &lt;p&gt;</p>".as_bytes());
    assert_eq!(texts(&pages), vec!["R&D й <p>"]);
}

// ------------------------------------------------------------ the block edges

/// A sentence broken by inline markup is one block, not three.
///
/// A browser paints `<b>` without a space around it, so joining is what the
/// page shows — and splitting would put "Виторг" and "зріс" into different
/// blocks, which the chunker rejoins with a blank line between them.
#[test]
fn inline_markup_inside_a_sentence_stays_one_block() {
    let pages = extract_html("<p>Виторг <b>зріс</b> на <i>10</i>%.</p>".as_bytes());
    assert_eq!(texts(&pages), vec!["Виторг зріс на 10%."]);
}

/// …and `<br>` is the one inline element that does not join, because it means
/// a line break.
///
/// Both directions in one test: joining across `<b>` is required, joining
/// across `<br>` would store `першийдругий` — a word that is in no document.
#[test]
fn a_line_break_does_not_glue_two_words_together() {
    let pages = extract_html("<p>перший<br>другий</p>".as_bytes());
    assert_eq!(texts(&pages), vec!["перший", "другий"]);
}

/// Types that an HTML element really has, and `Paragraph` for everything else.
#[test]
fn an_element_that_has_a_block_type_gets_it_and_the_rest_are_paragraphs() {
    let pages = extract_html(
        "<h2>Заголовок</h2><p>абзац</p><pre>код</pre>\
         <table><caption>назва</caption><tr><th>шапка</th><td>клітинка</td></tr></table>\
         <figure><figcaption>підпис</figcaption></figure><div>решта</div>"
            .as_bytes(),
    );
    let got: Vec<(BlockType, &str)> = pages
        .iter()
        .flat_map(|p| p.blocks.iter())
        .map(|b| (b.block_type, b.text.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (BlockType::Headline, "Заголовок"),
            (BlockType::Paragraph, "абзац"),
            (BlockType::Code, "код"),
            (BlockType::Caption, "назва"),
            (BlockType::Table, "шапка"),
            (BlockType::Table, "клітинка"),
            (BlockType::Caption, "підпис"),
            (BlockType::Paragraph, "решта"),
        ]
    );
}

/// `reading_order` restarts on every page and is dense within one.
///
/// The schema's uniqueness is on `(page_id, reading_order)`, so a counter
/// carried across pages is not wrong in the same way — it is a gap in every
/// page after the first, and `chunk_blocks` walks these in the order given.
#[test]
fn reading_order_restarts_on_every_page() {
    let pages = extract_html("<h1>Один</h1><p>a</p><p>b</p><h1>Два</h1><p>c</p>".as_bytes());
    assert_eq!(
        pages
            .iter()
            .map(|p| p.blocks.iter().map(|b| b.reading_order).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        vec![vec![0, 1, 2], vec![0, 1]],
    );
}

/// No block claims a line, in either field.
///
/// `pages_of` gives this reader `Fixed(Coordinate::Section)` *because* these
/// blocks have no rows; a number invented here would be cited as "рядки 1–1"
/// of a document that has none. Asserting one field alone is satisfied by a
/// reader that filled in the other.
#[test]
fn no_html_block_claims_a_line_number() {
    let pages = extract_html(RICH.as_bytes());
    let lined: Vec<_> = pages
        .iter()
        .flat_map(|p| p.blocks.iter())
        .filter(|b| b.line_start.is_some() || b.line_end.is_some())
        .collect();
    assert!(lined.is_empty(), "{lined:#?}");
}

// ------------------------------------------------------------- the empty file

/// A file with no prose in it is still one page, and the page is still there.
///
/// D37: `block.page_id` is NOT NULL, so the model owes a page even where the
/// format has none — and the pool checks the header's count against the page
/// frames that arrive, so a reader answering "no pages" would have to announce
/// zero as well.
#[test]
fn a_file_with_nothing_to_read_is_still_one_page() {
    for source in [
        "",
        "   \n  ",
        "<!-- нічого -->",
        "<html><body></body></html>",
    ] {
        let pages = extract_html(source.as_bytes());
        assert_eq!(pages.len(), 1, "{source:?} -> {pages:#?}");
        assert_eq!(pages[0].page_no, 1);
        assert!(pages[0].blocks.is_empty(), "{source:?} -> {pages:#?}");
    }
}

/// Bytes that are not markup at all are read as the text they are.
///
/// The HTML parsing algorithm has no failure mode, so this is not error
/// handling — it is what happens to a `.html` file somebody saved as plain
/// text, and it must not be an empty document.
#[test]
fn a_file_with_no_markup_at_all_is_read_as_its_text() {
    let pages = extract_html("Просто текст без жодного теґу.".as_bytes());
    assert_eq!(texts(&pages), vec!["Просто текст без жодного теґу."]);
}

/// Windows-1251, which chardetng has to guess: `<meta charset>` is not read.
///
/// Measured before it was relied on — chardetng answers `windows-1251` for
/// Ukrainian prose in that encoding, both for a long document and for a
/// four-character one, and answers UTF-8 for a UTF-8 file whose `<meta>` lies.
/// The reader shares `text::decode` with `.txt` and `.md` so that the same
/// bytes are decoded the same way whatever they are named.
#[test]
fn a_page_in_a_legacy_cyrillic_encoding_is_decoded_rather_than_mojibaked() {
    let page = "<html><head><meta charset=\"windows-1251\"><title>Кошторис</title></head>\
                <body><p>Комісія розглянула звернення щодо умов постачання.</p></body></html>";
    let (encoded, _, had_errors) = encoding_rs::WINDOWS_1251.encode(page);
    assert!(!had_errors, "the fixture must survive the round trip");
    let pages = extract_html(&encoded);
    assert_eq!(pages[0].section_title.as_deref(), Some("Кошторис"));
    assert!(
        all_text(&pages).contains("Комісія розглянула звернення"),
        "{:?}",
        all_text(&pages)
    );
}

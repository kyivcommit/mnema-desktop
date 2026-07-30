//! The markdown reader: sections become pages, fences become code, and every
//! block's text is a slice of the file rather than a rendering of the parse
//! tree.
//!
//! Every fixture is invented — names, places and numbers that belong to
//! nobody.

use mnema_core::{Block, BlockType};
use mnema_extract::{MarkdownPage, extract_markdown};

/// A document with content before its first heading, two sections, a fence and
/// a table: the shapes the mapping has to tell apart, in one file.
const REPORT: &str = "\
вступ без заголовка

# Розділ перший

Комісія розглянула звернення щодо постачання обладнання.

```rust
let userName = \"Равелла\";
    let ціна = 12;
```

## Обчислення

| показник | значення |
| --- | --- |
| строк | 90 днів |

останній абзац
";

fn pages(source: &str) -> Vec<MarkdownPage> {
    extract_markdown(source.as_bytes())
}

fn blocks(pages: &[MarkdownPage]) -> Vec<&Block> {
    pages.iter().flat_map(|p| p.blocks.iter()).collect()
}

/// The lines of `source` from `from` to `to`, both 1-based and inclusive,
/// rebuilt from the text rather than taken from the reader.
///
/// `split('\n')`, not `lines()`: on a CRLF file `lines()` throws the `\r` away,
/// which would make this reconstruction disagree with the file about what the
/// bytes between two line feeds are — and disagreeing with the file is exactly
/// what this is here to detect.
fn window(source: &str, from: u32, to: u32) -> String {
    source
        .split('\n')
        .skip(from as usize - 1)
        .take((to - from + 1) as usize)
        .collect::<Vec<_>>()
        .join("\n")
}

// --------------------------------------------------------------- the mapping

#[test]
fn a_fenced_block_keeps_its_indentation_and_is_typed_as_code() {
    let pages = pages(REPORT);
    let code = blocks(&pages)
        .into_iter()
        .find(|b| b.block_type == BlockType::Code)
        .expect("a fence must be recognised as code");
    assert!(
        code.text.contains("    let ціна = 12;"),
        "the indentation inside a fence is what makes code searchable: {:?}",
        code.text
    );
}

/// The strongest test in this file, and the one a re-rendering implementation
/// fails while passing every other.
///
/// D38 makes a block's text verbatim after NFC, and every offset in a
/// `Locator` is measured against exactly the text stored in `block.text`. A
/// reader that produced its text from the parse tree — comrak's own
/// `NodeCodeBlock::literal`, say, which strips a fence's own indentation, or a
/// paragraph re-emitted through a formatter — can produce a string that looks
/// identical and points a citation's highlight into text the file does not
/// contain.
///
/// **Equality, not `contains`.** It was written as `contains` and that made it
/// a subset test: a reader that kept only the *first line* of every block
/// while `line_start`/`line_end` went on claiming all of them passed it, and
/// died only incidentally in the one other test here that looks inside a
/// fence. `contains` also let `NodeCodeBlock::literal` through on any fence
/// that is not indented, since there `literal` is a substring of the window —
/// the kill rested on one fixture's trailing newline rather than on the
/// property. The implementation stores whole lines, so equality holds; the
/// `\r` clause is the one exception, because `Lines::span` deliberately leaves
/// the carriage return of a CRLF file's last line with the terminator.
#[test]
fn every_block_is_a_verbatim_slice_of_the_source() {
    for source in [
        REPORT,
        "лише абзац\n",
        "# Заголовок\n",
        "  ```py\n  x = 1\n  ```\n",
        "    відступний код\n\nтекст після нього\n",
        "перший\r\n\r\n# Заголовок\r\n\r\nдругий\r\n",
        "> цитата\n> продовження\n\n- пункт\n- інший\n",
        "текст\n\n```\nнезакритий фенс\n",
        "абзац без переводу рядка в кінці",
    ] {
        let normalised = mnema_core::nfc::normalise(source);
        let pages = pages(source);
        for block in blocks(&pages) {
            let (Some(from), Some(to)) = (block.line_start, block.line_end) else {
                panic!("markdown has line numbers for every block: {block:?}");
            };
            let window = window(&normalised, from, to);
            assert!(
                window == block.text || window == format!("{}\r", block.text),
                "block {:?}\n  claims lines {from}..={to} of {source:?}\n  which hold {window:?}",
                block.text
            );
            assert!(
                !block.text.trim().is_empty(),
                "an empty block is a row nothing can cite: {block:?}"
            );
        }
    }
}

/// NFC before slicing, not after — and the two are different strings.
///
/// A decomposed `й` is two characters; normalising after the offsets were
/// taken would leave `line_start`/`line_end` describing a string nothing
/// downstream ever sees again, and the stored text a different length from the
/// one the chunker measures.
#[test]
fn a_decomposed_heading_is_stored_and_named_in_its_precomposed_form() {
    // "Йорж" with И + U+0306 COMBINING BREVE in place of Й.
    let source = "# \u{0418}\u{0306}орж\n\nтекст\n";
    let pages = pages(source);
    assert_eq!(pages[0].section_title.as_deref(), Some("Йорж"));
    assert_eq!(pages[0].blocks[0].text, "# Йорж");
}

// ------------------------------------------------------------- pages (D37)

#[test]
fn a_heading_opens_a_page_and_names_it() {
    let pages = pages(REPORT);
    assert_eq!(
        pages.len(),
        3,
        "the content before the first heading, and one page per heading"
    );

    assert_eq!(pages[0].page_no, 1);
    assert_eq!(
        pages[0].section_title, None,
        "content before the first heading belongs to no section"
    );
    assert_eq!(pages[0].blocks.len(), 1);
    assert_eq!(pages[0].blocks[0].text, "вступ без заголовка");

    assert_eq!(pages[1].page_no, 2);
    assert_eq!(pages[1].section_title.as_deref(), Some("Розділ перший"));
    assert_eq!(
        pages[1].blocks[0].block_type,
        BlockType::Headline,
        "the heading sits on the page it opened, so that a chunk can carry it \
         as context for the paragraph beneath it"
    );

    assert_eq!(pages[2].page_no, 3);
    assert_eq!(pages[2].section_title.as_deref(), Some("Обчислення"));
    assert!(
        pages[2]
            .blocks
            .iter()
            .any(|b| b.block_type == BlockType::Table),
        "the table belongs to the section it sits under"
    );
}

/// Heading *level* is not nesting: `##` under `#` opens its own page rather
/// than a subsection of one. A real limitation, asserted so that nobody
/// mistakes the absence of nesting for an oversight.
#[test]
fn a_deeper_heading_opens_its_own_page_rather_than_nesting() {
    let pages = pages("# Верхній\n\nа\n\n## Вкладений\n\nб\n");
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].section_title.as_deref(), Some("Верхній"));
    assert_eq!(pages[1].section_title.as_deref(), Some("Вкладений"));
}

#[test]
fn a_file_with_no_headings_is_one_untitled_page() {
    let pages = pages("перший абзац\n\nдругий абзац\n");
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].page_no, 1);
    assert_eq!(pages[0].section_title, None);
    assert_eq!(pages[0].blocks.len(), 2);
}

/// An empty file is still one page. `block.page_id` is NOT NULL, so the
/// four-level model needs a page even where the format has none — and the pool
/// compares the header's count with the page frames, so a reader that answered
/// "no pages" here would have to remember to announce zero as well.
#[test]
fn an_empty_file_is_still_one_page_with_no_blocks() {
    for source in ["", "\n\n\n", "   \n"] {
        let pages = pages(source);
        assert_eq!(pages.len(), 1, "{source:?}");
        assert_eq!(pages[0].page_no, 1);
        assert_eq!(pages[0].section_title, None);
        assert!(pages[0].blocks.is_empty(), "{source:?}");
    }
}

/// YAML front matter is metadata, not a section and not prose.
///
/// CommonMark has no idea what it is, so with the delimiter unset comrak reads
/// the opening `---` as a thematic break and the keys **plus the closing
/// `---`** as a setext heading — which this reader then made a page and named.
/// Measured before the fix: page 1 came back named `title: Довідник`, the
/// metadata line was stored as a `headline` block joinable into chunks as
/// context, and the file's real first section was page 2. Obsidian, Hugo,
/// Jekyll, mkdocs and Docusaurus all put this at the top of every file.
#[test]
fn front_matter_is_neither_a_page_nor_a_block() {
    let pages =
        pages("---\ntitle: Довідник\n---\n\nВступ до збірника.\n\n# Розділ перший\n\nтекст\n");
    assert_eq!(pages.len(), 2, "{pages:#?}");
    assert_eq!(pages[0].section_title, None);
    assert_eq!(pages[0].blocks.len(), 1);
    assert_eq!(pages[0].blocks[0].text, "Вступ до збірника.");
    assert_eq!(pages[1].section_title.as_deref(), Some("Розділ перший"));
}

/// …and with several keys, which is where it got worse: the whole block was
/// one setext heading, so the page title became a multi-line string.
#[test]
fn front_matter_with_several_keys_is_still_dropped() {
    let pages = pages(
        "---\ntitle: Довідник постачання\nauthor: Комісія\ntags: [обладнання, строки]\n---\n\n\
         # Вступ\n\nтекст\n",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].section_title.as_deref(), Some("Вступ"));
    assert!(
        pages[0].blocks.iter().all(|b| !b.text.contains("author")),
        "no metadata key may reach a block: {:#?}",
        pages[0].blocks
    );
}

/// What enabling the delimiter costs, pinned so that it is a decision.
///
/// A file that opens with a thematic break and has another `---` later now
/// loses the text between them — comrak takes it for front matter. That is a
/// silent loss, and it is accepted because the shape it prevents is both
/// silent and near-universal, while this one is rare: a `.md` beginning with a
/// horizontal rule is unusual, and one that closes it with a second `---`
/// before any other content more so.
#[test]
fn a_file_that_opens_with_a_rule_loses_what_is_between_it_and_the_next_rule() {
    let pages = pages("---\n\nТекст усередині.\n\n---\n\nТекст після.\n");
    let blocks = blocks(&pages);
    assert_eq!(
        blocks.len(),
        1,
        "recorded, not desired — see the doc comment: {blocks:#?}"
    );
    assert_eq!(blocks[0].text, "Текст після.");
}

/// A heading with no text is not a section and not a row.
///
/// The thematic-break arm's argument applies unchanged: `#` on a line of its
/// own is a block holding `#`, which is searchable, citable and empty of
/// content. `heading_title` already answers `None` rather than `Some("")` for
/// it, so the block and the page follow the title.
#[test]
fn a_heading_with_no_text_is_neither_a_block_nor_a_page() {
    let pages = pages("#\n\nтекст\n");
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].section_title, None);
    assert_eq!(pages[0].blocks.len(), 1);
    assert_eq!(pages[0].blocks[0].text, "текст");
}

/// A link reference definition reaches no block, and that is recorded rather
/// than discovered.
///
/// comrak consumes `[пос]: https://…` into its link map and emits no node at
/// all — there is no `NodeValue` for one — so it falls out of the top-level
/// walk without passing either `continue`. In a documentation folder this is
/// where URLs live, and none of them is indexed. Fixing it means re-scanning
/// the source for lines no node claims, which is a different design; asserting
/// it here at least stops it being an accident.
#[test]
fn a_link_reference_definition_reaches_no_block() {
    let pages = pages("[пос]: https://example.invalid\n\nтекст [пос]\n");
    let blocks = blocks(&pages);
    assert_eq!(blocks.len(), 1, "{blocks:#?}");
    assert_eq!(blocks[0].text, "текст [пос]");
}

// ------------------------------------------------------- what a title may be

/// A section title is display metadata, so it is flattened — unlike a block's
/// text, which no rule here may touch.
///
/// A setext heading may span several lines, and a heading may be arbitrarily
/// long. `page.section_title` is a bare `TEXT` column shown beside a citation
/// with nothing between it and the interface, so a paragraph in a page title
/// is a rendering problem nobody downstream can undo.
#[test]
fn a_section_title_is_one_line_and_bounded() {
    let setext = pages("Заголовок\nпродовження заголовка\n=========\n\nтекст\n");
    let title = setext[0]
        .section_title
        .as_deref()
        .expect("a setext heading names its page");
    assert_eq!(title, "Заголовок продовження заголовка");

    let long = "х".repeat(500);
    let bounded = pages(&format!("# {long}\n\nтекст\n"));
    let title = bounded[0].section_title.as_deref().expect("named");
    assert!(
        title.chars().count() <= mnema_extract::SECTION_TITLE_MAX_CHARS,
        "a title of {} characters reaches the interface unbounded",
        title.chars().count()
    );
    assert!(
        title.ends_with('…'),
        "a cut title says it was cut: {title:?}"
    );
}

#[test]
fn reading_order_restarts_on_every_page() {
    let pages = pages("# А\n\nодин\n\nдва\n\n# Б\n\nтри\n");
    let first: Vec<i64> = pages[0].blocks.iter().map(|b| b.reading_order).collect();
    let second: Vec<i64> = pages[1].blocks.iter().map(|b| b.reading_order).collect();
    // `UNIQUE(page_id, reading_order)`, and the schema's own comment: reading
    // order is what reconstructs a page, so it is a position within one.
    assert_eq!(first, vec![0, 1, 2]);
    assert_eq!(second, vec![0, 1]);
}

// ------------------------------------------------------------ line numbers

#[test]
fn line_numbers_are_one_based_and_inclusive() {
    // 1: # Розділ
    // 2:
    // 3: перший рядок
    // 4: другий рядок
    let pages = pages("# Розділ\n\nперший рядок\nдругий рядок\n");
    let blocks = blocks(&pages);
    assert_eq!(
        (blocks[0].line_start, blocks[0].line_end),
        (Some(1), Some(1))
    );
    assert_eq!(
        (blocks[1].line_start, blocks[1].line_end),
        (Some(3), Some(4))
    );
}

/// An indented code block's own sourcepos ends on the *blank line after it* —
/// comrak reports `end.column == 0` there. Taken at face value the block would
/// claim a line it does not occupy, and its text would carry a trailing blank
/// line the source does not have inside the block.
#[test]
fn an_indented_code_block_does_not_claim_the_blank_line_after_it() {
    let pages = pages("    відступний код\n\nзвичайний абзац\n");
    let blocks = blocks(&pages);
    assert_eq!(blocks[0].block_type, BlockType::Code);
    assert_eq!(
        (blocks[0].line_start, blocks[0].line_end),
        (Some(1), Some(1))
    );
    assert_eq!(blocks[0].text, "    відступний код");
}

/// A horizontal rule carries no text, and a block whose text is `---` is a row
/// that can be searched, cited and read with nothing in it.
#[test]
fn a_thematic_break_produces_no_block() {
    let pages = pages("абзац\n\n---\n\nінший\n");
    let blocks = blocks(&pages);
    assert_eq!(blocks.len(), 2, "{blocks:?}");
    assert!(blocks.iter().all(|b| b.text != "---"));
}

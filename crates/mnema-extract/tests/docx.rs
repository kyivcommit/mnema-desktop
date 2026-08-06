//! Task 12's two tests from the brief, plus the ones its decisions imply — and,
//! mostly, the ones the **parse** implies.
//!
//! **The fixtures are built in code rather than checked in as `.docx` files**,
//! for the reason `tests/epub.rs` states: a binary blob in a public repository
//! is a fixture nobody can read a diff of, and an error inside one looks exactly
//! like an error in the reader. The brief asked for
//! `include_bytes!("fixtures/two-headings.docx")` and this is the same refusal
//! Task 11 made.
//!
//! **What that costs is stated rather than hidden, and here it is not zero.**
//! Nothing below has met a file Word actually wrote. What stands in for that is
//! a structural probe of 24 real `.docx` files on the author's machine — element
//! names, style ids and counts only, never their prose — and the two shapes it
//! turned up that no reasoning produced are pinned here:
//! `a_heading_is_what_styles_xml_says_it_is` and
//! `a_tab_stop_definition_is_not_a_tab_character`.

use std::io::{Cursor, Write};

use mnema_extract::{DocxError, extract_docx};

// ---------------------------------------------------------------- the fixtures

/// The namespaces a real `word/document.xml` declares, and the ones these
/// fixtures use: `w` for WordprocessingML, `mc` for markup compatibility (the
/// `<mc:Fallback>` duplication), `wp`/`a` for a drawing and the text inside one.
const NAMESPACES: &str = concat!(
    " xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"",
    " xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"",
    " xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\"",
    " xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"",
);

/// A whole `word/document.xml` around a body.
fn document(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:document{NAMESPACES}><w:body>{body}</w:body></w:document>"
    )
}

/// A whole `word/styles.xml` around a run of `<w:style>` elements.
fn styles(entries: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:styles{NAMESPACES}>{entries}</w:styles>"
    )
}

/// An ordinary paragraph of one run.
fn p(text: &str) -> String {
    format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
}

/// A paragraph carrying a paragraph style.
fn styled(style: &str, text: &str) -> String {
    format!(
        "<w:p><w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr>\
         <w:r><w:t>{text}</w:t></w:r></w:p>"
    )
}

/// A docx with just the one part a docx is identified by.
fn docx(body: &str) -> Vec<u8> {
    zip_of(&[("word/document.xml", document(body).into_bytes())])
}

/// A docx with a stylesheet as well — the second member this reader opens.
fn docx_with_styles(body: &str, style_entries: &str) -> Vec<u8> {
    zip_of(&[
        ("word/document.xml", document(body).into_bytes()),
        ("word/styles.xml", styles(style_entries).into_bytes()),
    ])
}

fn zip_of(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let deflated: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in members {
            w.start_file(*name, deflated).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap();
    }
    buf.into_inner()
}

/// Every block of every section, in order — what the document's text is.
fn texts(sections: &[mnema_extract::DocxSection]) -> Vec<&str> {
    sections
        .iter()
        .flat_map(|section| section.blocks.iter().map(|block| block.text.as_str()))
        .collect()
}

/// Every section's name, in order.
fn titles(sections: &[mnema_extract::DocxSection]) -> Vec<Option<&str>> {
    sections
        .iter()
        .map(|section| section.section_title.as_deref())
        .collect()
}

// ------------------------------------------------- the two tests from the brief

/// Verbatim from the brief, against a fixture built here rather than a checked-in
/// `.docx`.
///
/// The one adaptation beyond the fixture is `sections[0].blocks[0]`: a heading
/// is a block of its own page as well as its name, exactly as in `markdown.rs`
/// and `html.rs`, so the prose the brief looks for is the *second* block.
#[test]
fn paragraphs_come_back_with_their_headings_as_sections() {
    let body = format!(
        "{}{}{}{}",
        styled("Heading1", "Вступ"),
        p("Цей документ описує порядок роботи."),
        styled("Heading1", "Порядок"),
        p("Спершу зверніться до канцелярії."),
    );
    let sections = extract_docx(&docx(&body)).unwrap();

    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].section_title.as_deref(), Some("Вступ"));
    assert!(sections[0].blocks[1].text.contains("Цей документ"));
    // And the other direction, so that a reader which put everything on one
    // page could not pass: the second section is its own page, numbered and
    // named.
    assert_eq!(sections[1].page_no, 2);
    assert_eq!(sections[1].section_title.as_deref(), Some("Порядок"));
    assert!(sections[1].blocks[1].text.contains("канцелярії"));
}

/// Verbatim from the brief.
///
/// `typing::identify` decides a docx by this member existing, so reaching this
/// means the archive changed between identification and reading — but the
/// verdict still has to be *damage* rather than "a format we do not read",
/// because `unsupported` promises a reader that is coming and this file already
/// has one.
#[test]
fn a_docx_without_word_document_xml_is_malformed_not_unsupported() {
    let archive = zip_of(&[("not/the/part.xml", b"<x/>".to_vec())]);
    assert!(matches!(
        extract_docx(&archive),
        Err(DocxError::Malformed(_))
    ));
}

// ------------------------------------------------- what a heading actually is

/// **The measurement that decides this reader, and it falsifies the obvious
/// rule.**
///
/// Of 24 real `.docx` files probed on this machine, only **2** use a style id
/// matching `Heading1`–`Heading9`. Five have a paragraph whose style
/// `word/styles.xml` calls a heading or gives an outline level — and in three of
/// them the id is `11`, with a Cyrillic `<w:name>` and `<w:outlineLvl w:val="0"/>`.
/// Word renames built-in styles when a document is authored in another
/// language, so matching the id alone finds no heading in 3 of the 5 files that
/// have one: those documents become a single unnamed page, and every citation
/// into them names nothing (spec §6, invariant 1).
///
/// Both directions, because either alone is satisfied by a mistake. A reader
/// that called every styled paragraph a heading would pass the first half; one
/// that called none would pass the second. `w:val="9"` is the value OOXML uses
/// for "body text", which is why it is the negative case rather than an
/// invented one.
#[test]
fn a_heading_is_what_styles_xml_says_it_is() {
    let body = format!(
        "{}{}{}",
        styled("11", "Загальні положення"),
        p("Текст розділу."),
        styled("a5", "Це не заголовок"),
    );
    let entries = "<w:style w:type=\"paragraph\" w:styleId=\"11\">\
                   <w:name w:val=\"Заголовок №1\"/>\
                   <w:pPr><w:outlineLvl w:val=\"0\"/></w:pPr></w:style>\
                   <w:style w:type=\"paragraph\" w:styleId=\"a5\">\
                   <w:name w:val=\"Звичайний абзац\"/>\
                   <w:pPr><w:outlineLvl w:val=\"9\"/></w:pPr></w:style>";

    let sections = extract_docx(&docx_with_styles(&body, entries)).unwrap();
    assert_eq!(titles(&sections), vec![Some("Загальні положення")]);
    // The paragraph the stylesheet does **not** call a heading is text on that
    // same page, not a page of its own.
    assert_eq!(
        texts(&sections),
        vec!["Загальні положення", "Текст розділу.", "Це не заголовок"]
    );
}

/// The other signal a stylesheet gives, for a style that has a name and no
/// outline level: `<w:name w:val="heading 1"/>` is what the file format calls a
/// built-in heading whatever its id has been renamed to.
#[test]
fn a_style_named_a_heading_is_one_even_without_an_outline_level() {
    let body = format!("{}{}", styled("XX", "Розділ"), p("Текст."));
    let entries = "<w:style w:type=\"paragraph\" w:styleId=\"XX\">\
                   <w:name w:val=\"heading 2\"/></w:style>";
    let sections = extract_docx(&docx_with_styles(&body, entries)).unwrap();
    assert_eq!(titles(&sections), vec![Some("Розділ")]);
}

/// A stylesheet this reader cannot open is **not** a damaged document, and the
/// canonical ids still work without one.
///
/// `word/styles.xml` is a second member and every member is a second thing that
/// can be absent. Refusing here would take a document out of the index over a
/// part that holds no prose at all — the same argument `epub.rs` makes for a
/// chapter the archive does not hold, reached from the other side.
#[test]
fn a_missing_stylesheet_does_not_refuse_the_document() {
    let body = format!("{}{}", styled("Heading1", "Вступ"), p("Текст."));
    // `docx` writes no `word/styles.xml` at all.
    let sections = extract_docx(&docx(&body)).unwrap();
    assert_eq!(titles(&sections), vec![Some("Вступ")]);
    assert_eq!(texts(&sections), vec!["Вступ", "Текст."]);
}

/// A paragraph may carry its own outline level, overriding whatever its style
/// says — and then no stylesheet is needed to know it is a heading.
#[test]
fn a_paragraphs_own_outline_level_opens_a_section() {
    let body = format!(
        "<w:p><w:pPr><w:outlineLvl w:val=\"0\"/></w:pPr><w:r><w:t>Розділ</w:t></w:r></w:p>{}",
        p("Текст.")
    );
    let sections = extract_docx(&docx(&body)).unwrap();
    assert_eq!(titles(&sections), vec![Some("Розділ")]);
}

/// **A revision is not a fact about the document as it stands.**
/// `<w:pPrChange>` records the properties a paragraph *used* to have, and it
/// carries a whole `<w:pPr>` inside it — including the `<w:pStyle>` that used to
/// be there. Read as current, an edited paragraph becomes a heading it is not,
/// and the document is cut into sections at the places somebody once changed.
#[test]
fn a_paragraph_that_used_to_be_a_heading_is_not_one_now() {
    let body = "<w:p><w:pPr><w:pPrChange w:id=\"1\" w:author=\"а\">\
                <w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr></w:pPrChange></w:pPr>\
                <w:r><w:t>Звичайний абзац</w:t></w:r></w:p>";
    let sections = extract_docx(&docx(body)).unwrap();
    assert_eq!(titles(&sections), vec![None]);
    assert_eq!(texts(&sections), vec!["Звичайний абзац"]);
}

/// A style declaration that is not inside `<w:pPr>` is not the paragraph's
/// style.
///
/// Invalid OOXML either way, and this test's own history is the interesting
/// part. It was written to hold a guard in `properties` that refused to read a
/// property outside `<w:pPr>` — and the mutation case that removed that guard
/// **stayed green**, which is what says the guard was doing nothing: `docx.rs`
/// resolves `heading` only when a `</w:pPr>` returns the depth to zero, so a
/// property outside one is recorded and never read. The guard is gone; this
/// assertion stays, because the behaviour is still one somebody could change by
/// moving where `heading` is resolved.
#[test]
fn a_style_outside_the_paragraphs_properties_is_not_its_style() {
    let body = "<w:p><w:pStyle w:val=\"Heading1\"/><w:r><w:t>Звичайний абзац</w:t></w:r></w:p>";
    let sections = extract_docx(&docx(body)).unwrap();
    assert_eq!(titles(&sections), vec![None]);
    assert_eq!(texts(&sections), vec!["Звичайний абзац"]);
}

/// A heading with no text at all is not a section, exactly as in `markdown.rs`:
/// a page named by the empty string renders as a section that exists and has no
/// name.
#[test]
fn a_heading_with_no_text_is_not_a_section() {
    let body = format!(
        "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr></w:p>{}",
        p("Текст.")
    );
    let sections = extract_docx(&docx(&body)).unwrap();
    assert_eq!(titles(&sections), vec![None]);
    assert_eq!(texts(&sections), vec!["Текст."]);
}

/// A heading style inside a table cell does not cut the document into sections.
///
/// The decision and its cost, both stated: a table's header cells are very often
/// styled, and one page per cell would shatter a document into dozens of
/// sections named after column headings. What it costs is the layout table —
/// a heading someone put in a one-cell table is text rather than a section.
#[test]
fn a_heading_in_a_table_cell_does_not_open_a_section() {
    let body = format!(
        "<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>{}",
        styled("Heading1", "Найменування"),
        p("Після таблиці."),
    );
    let sections = extract_docx(&docx(&body)).unwrap();
    assert_eq!(titles(&sections), vec![None]);
    assert_eq!(texts(&sections), vec!["Найменування", "Після таблиці."]);
}

/// The name is bounded by the one rule every reader shares, and this reader
/// states the contract as its own so that a later version taking the name from
/// somewhere else cannot quietly stop bounding it.
#[test]
fn a_sections_name_is_bounded_by_the_rule_every_reader_shares() {
    let long = "Розділ ".repeat(60);
    let sections = extract_docx(&docx(&styled("Heading1", &long))).unwrap();
    let title = sections[0]
        .section_title
        .as_deref()
        .expect("the section is named");

    assert_eq!(
        title.chars().count(),
        mnema_extract::SECTION_TITLE_MAX_CHARS
    );
    assert!(title.ends_with('…'), "{title:?}");
    // The other direction, so a reader returning a constant could not pass.
    assert!(title.starts_with("Розділ Розділ"), "{title:?}");

    let short = extract_docx(&docx(&styled("Heading1", "Коротко"))).unwrap();
    assert_eq!(short[0].section_title.as_deref(), Some("Коротко"));
}

// ------------------------------------- what can vanish, asked of the **parse**

/// Text inside a table is prose, and it is typed as a table.
///
/// Measured on the same 24 files: `<w:tbl>` occurs in 19 of them, 2,157 cells in
/// all. A reader that walked only top-level paragraphs would lose most of the
/// text in most real documents and refuse a few of them outright as having
/// none.
#[test]
fn text_in_a_table_is_indexed_and_typed_as_a_table() {
    let body = format!(
        "{}<w:tbl><w:tr><w:tc>{}</w:tc><w:tc>{}</w:tc></w:tr></w:tbl>",
        p("Перед таблицею."),
        p("Найменування"),
        p("Кількість"),
    );
    let sections = extract_docx(&docx(&body)).unwrap();
    assert_eq!(
        texts(&sections),
        vec!["Перед таблицею.", "Найменування", "Кількість"]
    );
    use mnema_core::BlockType;
    let types: Vec<BlockType> = sections[0]
        .blocks
        .iter()
        .map(|block| block.block_type)
        .collect();
    assert_eq!(
        types,
        vec![BlockType::Paragraph, BlockType::Table, BlockType::Table]
    );
}

/// **A break and a tab are the whitespace the document shows.**
///
/// Dropping them stores a word that is in no file and that a search for either
/// half will not find — the `передпісля` defect `html.rs` measured, reached
/// through a different element. `<w:tab/>` occurs 394 times across 16 of the 24
/// files probed, so this is the common case rather than an edge.
#[test]
fn a_break_and_a_tab_carry_the_whitespace_they_stand_for() {
    let body = "<w:p><w:r><w:t>перед</w:t><w:tab/><w:t>після</w:t>\
                <w:br/><w:t>новий рядок</w:t></w:r></w:p>";
    let sections = extract_docx(&docx(body)).unwrap();
    assert_eq!(texts(&sections), vec!["перед\tпісля\nновий рядок"]);
}

/// **The trap the probe found and no reasoning would have.**
///
/// `<w:tab/>` inside `<w:pPr><w:tabs>` is a *tab stop definition* — a position
/// on the ruler — and has the same element name as the tab character inside a
/// run. Of 394 `<w:tab>` elements in the corpus, 254 `<w:tabs>` containers sit
/// beside them, so a large share of them are definitions. Emitting a character
/// for each puts tabs at the front of paragraph after paragraph, in text no
/// document shows.
///
/// Both directions in one test: the definition contributes nothing and the run's
/// own tab still does.
#[test]
fn a_tab_stop_definition_is_not_a_tab_character() {
    let body = "<w:p><w:pPr><w:tabs><w:tab w:val=\"left\" w:pos=\"720\"/>\
                <w:tab w:val=\"right\" w:pos=\"9354\"/></w:tabs></w:pPr>\
                <w:r><w:t>ліворуч</w:t><w:tab/><w:t>праворуч</w:t></w:r></w:p>";
    let sections = extract_docx(&docx(body)).unwrap();
    assert_eq!(texts(&sections), vec!["ліворуч\tправоруч"]);
}

/// **Deleted text is not what the document says, and inserted text is.**
///
/// With revision marking on, a deletion keeps its words in `<w:delText>` and an
/// insertion keeps them in an ordinary `<w:t>`. Indexing the first would answer
/// searches with sentences the author removed — the sharpest form of "text the
/// file no longer contains".
#[test]
fn deleted_text_is_not_indexed_and_inserted_text_is() {
    let body = "<w:p>\
                <w:del w:id=\"1\" w:author=\"а\"><w:r><w:delText>викреслене </w:delText></w:r></w:del>\
                <w:ins w:id=\"2\" w:author=\"а\"><w:r><w:t>додане</w:t></w:r></w:ins>\
                </w:p>";
    let sections = extract_docx(&docx(body)).unwrap();
    assert_eq!(texts(&sections), vec!["додане"]);
}

/// A paragraph moved with revision marking on appears **twice** — once under
/// `<w:moveFrom>` at its old position and once under `<w:moveTo>` at its new
/// one — and both hold ordinary `<w:t>`. Storing both indexes the same sentence
/// in two places, one of which the document does not show.
#[test]
fn a_moved_paragraph_is_stored_once_at_its_new_position() {
    let body = "<w:p><w:moveFrom w:id=\"1\" w:author=\"а\">\
                <w:r><w:t>Перенесене речення.</w:t></w:r></w:moveFrom></w:p>\
                <w:p><w:moveTo w:id=\"2\" w:author=\"а\">\
                <w:r><w:t>Перенесене речення.</w:t></w:r></w:moveTo></w:p>";
    let sections = extract_docx(&docx(body)).unwrap();
    assert_eq!(texts(&sections), vec!["Перенесене речення."]);
}

/// A field's *instruction* is code and its *result* is prose.
///
/// `<w:instrText>` holds ` PAGE `, ` TOC \o "1-3" `, ` HYPERLINK "…" ` — none of
/// it visible, all of it plausible-looking noise in a search index. The result
/// beside it is what the page shows.
#[test]
fn a_field_instruction_is_not_prose_and_its_result_is() {
    let body = "<w:p>\
                <w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>\
                <w:r><w:instrText xml:space=\"preserve\"> HYPERLINK \\l \"розділ2\" </w:instrText></w:r>\
                <w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>\
                <w:r><w:t>див. розділ 2</w:t></w:r>\
                <w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>";
    let sections = extract_docx(&docx(body)).unwrap();
    assert_eq!(texts(&sections), vec!["див. розділ 2"]);
}

/// A hyperlink's text is part of the sentence around it; its target is in
/// `word/_rels/document.xml.rels` and is knowingly not read.
#[test]
fn a_hyperlinks_text_is_part_of_its_paragraph() {
    let body = "<w:p><w:r><w:t>Див. </w:t></w:r>\
                <w:hyperlink r:id=\"rId7\" xmlns:r=\"http://schemas.openxmlformats.org/\
                officeDocument/2006/relationships\"><w:r><w:t>довідник</w:t></w:r></w:hyperlink>\
                <w:r><w:t> на сайті.</w:t></w:r></w:p>";
    let sections = extract_docx(&docx(body)).unwrap();
    assert_eq!(texts(&sections), vec!["Див. довідник на сайті."]);
}

/// **`<mc:AlternateContent>` says the same thing twice on purpose**, once in
/// markup a modern reader understands and once in markup an old one does. A
/// text box arrives this way, and reading both stores the words twice: one of
/// the copies is text the document shows nowhere, and a search hits the same
/// sentence in two blocks.
///
/// Both directions: the choice is read and the fallback is not.
#[test]
fn an_alternate_content_fallback_does_not_store_the_text_twice() {
    let body = "<w:p><w:r><mc:AlternateContent>\
                <mc:Choice Requires=\"wps\"><w:drawing><wp:inline><a:graphic>\
                <w:txbxContent><w:p><w:r><w:t>Напис у рамці</w:t></w:r></w:p>\
                </w:txbxContent></a:graphic></wp:inline></w:drawing></mc:Choice>\
                <mc:Fallback><w:pict><v:shape xmlns:v=\"urn:schemas-microsoft-com:vml\">\
                <v:textbox><w:txbxContent><w:p><w:r><w:t>Напис у рамці</w:t></w:r></w:p>\
                </w:txbxContent></v:textbox></v:shape></w:pict></mc:Fallback>\
                </mc:AlternateContent></w:r></w:p>";
    let sections = extract_docx(&docx(body)).unwrap();
    assert_eq!(texts(&sections), vec!["Напис у рамці"]);
}

/// A drawing's alternative description is an **attribute**, and attributes are
/// not text — the same rule `html.rs` states for `alt` and `title`. The prose
/// inside a text box is a text node and is read.
///
/// Both halves in one test: without the second, a reader that skipped the whole
/// `<w:drawing>` subtree would pass, and it would lose every text box in every
/// document.
#[test]
fn a_drawings_description_is_not_text_and_the_words_inside_it_are() {
    let body = "<w:p><w:r><w:drawing><wp:inline>\
                <wp:docPr id=\"1\" name=\"Рисунок 1\" descr=\"схема руху документів\"/>\
                <a:graphic><w:txbxContent><w:p><w:r><w:t>Канцелярія</w:t></w:r></w:p>\
                </w:txbxContent></a:graphic></wp:inline></w:drawing></w:r></w:p>";
    let sections = extract_docx(&docx(body)).unwrap();
    assert_eq!(texts(&sections), vec!["Канцелярія"]);
    assert!(
        !texts(&sections).iter().any(|t| t.contains("схема руху")),
        "an attribute reached a block: {:?}",
        texts(&sections)
    );
}

/// A paragraph that carries no text at all produces no block, and does not
/// shift what is around it.
///
/// An empty `<w:p>` is what an empty line in a document is, and Word writes a
/// great many of them. A block holding the empty string is searchable, citable
/// and empty of content — `markdown.rs`'s argument for dropping a thematic
/// break.
#[test]
fn an_empty_paragraph_makes_no_block() {
    let body = format!("{}<w:p/><w:p><w:pPr/></w:p>{}", p("Перший."), p("Другий."));
    let sections = extract_docx(&docx(&body)).unwrap();
    assert_eq!(texts(&sections), vec!["Перший.", "Другий."]);
    assert_eq!(
        sections[0]
            .blocks
            .iter()
            .map(|b| b.reading_order)
            .collect::<Vec<_>>(),
        vec![0, 1],
        "reading order counts blocks, not paragraphs"
    );
}

/// **A truncated `word/document.xml` is damage, not half a document.**
///
/// Measured before this was written: quick-xml reaches `Event::Eof` on a part
/// that stops inside an element without reporting an error, so a reader that
/// stopped there would store the prose before the cut and say nothing about the
/// rest. The person holding the file would have a document in the index that
/// answers searches, is missing most of its text, and carries no journal row.
/// Word will not open such a file either.
#[test]
fn a_truncated_document_part_is_damage_rather_than_half_a_document() {
    let truncated = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document{NAMESPACES}><w:body>{}\
         <w:p><w:r><w:t>обірвано",
        p("Ціле речення."),
    );
    let archive = zip_of(&[("word/document.xml", truncated.into_bytes())]);
    assert!(
        matches!(extract_docx(&archive), Err(DocxError::Malformed(_))),
        "a part that stops mid-element came back as a document"
    );
}

/// XML that does not parse at all is damage too, and by a different route: an
/// end tag that closes an element nothing opened.
#[test]
fn a_document_part_that_does_not_parse_is_damage() {
    let archive = zip_of(&[(
        "word/document.xml",
        format!("<w:document{NAMESPACES}><w:body></w:p></w:body></w:document>").into_bytes(),
    )]);
    assert!(matches!(
        extract_docx(&archive),
        Err(DocxError::Malformed(_))
    ));
}

// ------------------------------------------------------------------- the caps
//
// The cap itself is measured in `src/docx.rs`'s own tests, against a stated cap
// of a kilobyte rather than the real 16 MiB — the split `epub.rs` made for the
// same reason, and here it buys more: two members owe the cap, so a test at the
// real number would inflate 32 MiB to say what a kilobyte says.

/// A document with no text in it is refused rather than stored as a document
/// with no blocks.
///
/// The same answer `pdf.rs` gives a file of scans and `epub.rs` a book of
/// plates, and for the same reason: a row in the index that answers no query
/// tells the person who added the file nothing at all.
#[test]
fn a_document_with_no_text_is_refused_rather_than_stored_empty() {
    let body = "<w:p><w:r><w:drawing><wp:inline>\
                <wp:docPr id=\"1\" name=\"Рисунок 1\" descr=\"скан\"/>\
                </wp:inline></w:drawing></w:r></w:p><w:p/>";
    assert!(matches!(extract_docx(&docx(body)), Err(DocxError::NoText)));
}

// --------------------------------------------------------------- the verbatim

/// Task 10's step 6, repeated here with this reader's own fixture.
///
/// An invariant checked in one reader of five is an invariant missing from
/// four. What is stored is what the file shows, after NFC and after nothing
/// else — the server's `_clean = " ".join(text.split())`
/// (`app/textdoc/html_blocks.py:41-42`) is not ported (G7.1 §2.3).
///
/// **`xml:space` is the DOCX-specific half of it.** The attribute exists
/// because an XML consumer is otherwise free to trim a `<w:t>`; this reader
/// never trims, so honouring it could only ever *remove* characters the file
/// contains. It is therefore not read at all, and the paragraph below carries
/// its edge whitespace without one.
#[test]
fn a_docxs_text_is_verbatim_after_nfc_and_nothing_else() {
    // Cyrillic й as a decomposed pair, a doubled space, a real tab character
    // inside the text, a non-breaking space — and no `xml:space="preserve"`.
    let body = "<w:p><w:r><w:t> и\u{0306}  a\tb\u{00a0}c </w:t></w:r></w:p>";
    let sections = extract_docx(&docx(body)).unwrap();
    let text = &sections[0].blocks[0].text;

    assert!(text.contains('й'), "NFC did not run (D32): {text:?}");
    assert!(text.contains("  "), "whitespace was collapsed: {text:?}");
    assert!(text.contains('\t'), "a tab was rewritten: {text:?}");
    assert!(
        text.contains('\u{00a0}'),
        "a non-breaking space was folded: {text:?}"
    );
    // Exactly, so that a reader which trimmed one end and not the other cannot
    // pass on the four assertions above.
    assert_eq!(text, " й  a\tb\u{00a0}c ");
}

/// NFC runs on the text taken **out of** the parse, and a docx is where that
/// matters for the same reason XHTML is: a producer that escapes non-ASCII
/// writes `&#1080;&#774;`, and normalising the source composes nothing because
/// until the parser has decoded the reference there is no combining mark in the
/// document at all.
#[test]
fn a_combining_mark_written_as_a_character_reference_is_composed() {
    let sections = extract_docx(&docx(&p("&#1080;&#774;од"))).unwrap();
    assert_eq!(texts(&sections), vec!["йод"]);
}

/// A section's name is normalised on the same pass its text is.
///
/// Task 10 asserts this for an HTML heading and Task 11 for a chapter's; a
/// docx's title arrives by a third route — the text of the heading paragraph
/// itself — and is asserted here. A page named `и\u{306}од` answers no query
/// typed `йод`, and no offset is ever measured into a title to make that
/// recoverable.
#[test]
fn a_section_name_from_a_character_reference_is_composed_too() {
    let sections = extract_docx(&docx(&styled("Heading1", "&#1080;&#774;од"))).unwrap();
    assert_eq!(sections[0].section_title.as_deref(), Some("йод"));
}

/// An XML escape is a character of the text, not five characters of it.
#[test]
fn an_xml_escape_is_one_character_of_the_text() {
    let sections = extract_docx(&docx(&p("Чай &amp; кава"))).unwrap();
    assert_eq!(texts(&sections), vec!["Чай & кава"]);
}

/// No block of a docx claims a line number.
///
/// `pages_of` gives this reader `Fixed(Coordinate::Section)`
/// (`crates/mnema-ingest/src/lib.rs:1392`) *because* these blocks carry no rows:
/// a docx has no lines until something lays it out, and a number invented here
/// would be cited as "рядки 1–1" of a document that has none.
#[test]
fn no_docx_block_claims_a_line_number() {
    let sections = extract_docx(&docx(&format!("{}{}", p("Один."), p("Два.")))).unwrap();
    let blocks = &sections[0].blocks;
    assert_eq!(blocks.len(), 2);
    assert!(
        blocks
            .iter()
            .all(|block| block.line_start.is_none() && block.line_end.is_none()),
        "{blocks:?}"
    );
}

/// `reading_order` restarts on every page, because the index's uniqueness is on
/// `(page_id, reading_order)`.
#[test]
fn reading_order_restarts_on_every_section() {
    let body = format!(
        "{}{}{}{}",
        styled("Heading1", "Перший"),
        p("Текст."),
        styled("Heading1", "Другий"),
        p("Ще текст."),
    );
    let sections = extract_docx(&docx(&body)).unwrap();
    for section in &sections {
        assert_eq!(
            section
                .blocks
                .iter()
                .map(|b| b.reading_order)
                .collect::<Vec<_>>(),
            vec![0, 1],
            "section {} does not number its blocks from zero",
            section.page_no
        );
    }
}

// ---------------------------------------------------------------------- docm

/// **A document with macros is read, and the decision is here rather than
/// nowhere.**
///
/// The server accepts them (`DOCM_MIME` in `app/ingest/office_mimes.py:11-15`)
/// and the spec left it open (§8.2). A macro is a member of the archive this
/// reader does not open — `word/vbaProject.bin` — and `word/document.xml` in a
/// `.docm` is the same part read the same way, so refusing would take a whole
/// class of documents out of the index over bytes nothing here executes.
///
/// It needs no code: `typing::identify` decides a docx by that member existing,
/// which a `.docm` has. What this test does is stop the behaviour from being
/// accidental — and it names the one inaccuracy it carries, which is that the
/// `mime` recorded is the plain wordprocessingml one.
#[test]
fn a_document_with_macros_is_read_by_the_same_reader() {
    let archive = zip_of(&[
        (
            "word/document.xml",
            document(&p("Текст із макросом.")).into_bytes(),
        ),
        ("word/vbaProject.bin", vec![0u8; 32]),
    ]);
    let file_type = mnema_extract::typing::identify(&archive, Some("docm"));
    assert_eq!(file_type.reader, mnema_extract::typing::Reader::Docx);
    assert_eq!(
        file_type.mime, "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "a docm is recorded under the docx mime — the one cost of accepting it here"
    );
    assert_eq!(
        texts(&extract_docx(&archive).unwrap()),
        vec!["Текст із макросом."]
    );
}

# Mutation cases for Task 12: the DOCX reader. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-12.sh
#
# The brief said this file already existed and was verified. It did not exist —
# `ls scripts/mutations/` at HEAD a8914f1 has task-1 through task-11 and
# task-13, and no task-12 — so every case below is new.
#
# What this file is mostly about is one class of silence, and it is a different
# one from Task 11's. A book loses whole chapters; a document loses *words
# inside a paragraph*, and there are five shapes of it that nothing downstream
# can tell apart:
#
#   * a word that was never in the file — a tab or a break dropped, so
#     `перед<w:tab/>після` is stored as one word neither half will find;
#   * a word the file no longer contains — deleted text indexed, so a search
#     answers with the sentence the author took out;
#   * a word stored twice — the `<mc:Fallback>` half of an alternate content
#     block, or a paragraph's `<w:moveFrom>` position;
#   * a word that is not prose at all — a field instruction, a tab stop, an
#     attribute;
#   * every heading at once — the case the probe of 24 real files found, where
#     matching style *ids* finds no heading in three of the five documents that
#     have one and each arrives as a single unnamed page.
#
# Cases are anchored on code, never on the prose beside it: task 10 lost five of
# its own to a fix round that edited the comments they matched.

# ------------------------------------------------------ the reader's own name

# C1. The name is one symbol across a process boundary and across D40.
# `pages_of` matches it to cite a section (`crates/mnema-ingest/src/lib.rs:1392`);
# one character off falls to `PageContext::Lines`, which asks blocks that carry
# no line numbers for a line range and answers `Coordinate::None`. Green
# everywhere else.
case_ "worker: the header names the docx reader, not a near-miss" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                    reader: manifest::READER_DOCX\.to_string\(\),}{                    reader: "docx-2".to_string(),}' \
  'reader: "docx-2".to_string(),' \
  mnema-extract 'a_docx_is_read_section_by_section_and_its_summary_skips_nothing' --test worker_cli

# C2. The version, which is what decides whether a document already in the index
# was made by today's code — and, since task 10's C22, whether it is replaced.
case_ "worker: the header carries the docx reader's version" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                    reader_version: manifest::DOCX_READER_VERSION,}{                    reader_version: 0,}' \
  'reader_version: 0,' \
  mnema-extract 'a_docx_is_read_section_by_section_and_its_summary_skips_nothing' --test worker_cli

# C3. `native:docx` names the reader, not the file. Text that came out of a word
# processor's part is not the same evidence as text that came out of a
# plain-text read of the same bytes, and `page.text_source` is where that is
# recorded.
case_ "worker: the summary names the docx text source" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                    text_source: "native:docx"\.to_string\(\),}{                    text_source: "native:html".to_string(),}' \
  'text_source: "native:html".to_string(),' \
  mnema-extract 'a_docx_is_read_section_by_section_and_its_summary_skips_nothing' --test worker_cli

# C4. The entry that looks most obviously right of the three and is not. `.docx`
# names one format and nothing else uses it — but `identify` reaches this reader
# through the zip signature plus `word/document.xml`, never through the name, and
# the map is a claim about `identify`. With the entry, prose called `угода.docx`
# is predicted for a reader that never touched it.
case_ "manifest: docx is decided by content, so the map does not claim it" \
  crates/mnema-extract/src/manifest.rs \
  's{    Manifest \{\n        default: ReaderId::new\("text", TEXT_READER_VERSION\),}{    by_extension.insert(\n        "docx".to_string(),\n        ReaderId::new(READER_DOCX, DOCX_READER_VERSION),\n    );\n    Manifest \{\n        default: ReaderId::new("text", TEXT_READER_VERSION),}' \
  'by_extension.insert(
        "docx".to_string(),
        ReaderId::new(READER_DOCX, DOCX_READER_VERSION),
    );' \
  mnema-extract 'a_docx_is_read_by_content_so_the_manifest_predicts_the_wrong_reader_for_it' --test manifest

# ------------------------------------------------ what a heading actually is

# C5. **The case the probe of 24 real files exists for.** Only the canonical ids
# consulted, and the stylesheet ignored: Word renames the built-in styles when a
# document is authored in another language, so three of the five files here that
# have a heading have it under the id `11`. Under this mutation each of them is
# one unnamed page and every citation into it names nothing.
case_ "reader: a heading is what the stylesheet says it is, not what its id spells" \
  crates/mnema-extract/src/docx.rs \
  's{        Some\(style\) => headings\.contains\(style\) \|\| is_canonical_heading_id\(style\),}{        Some(style) => is_canonical_heading_id(style),}' \
  'Some(style) => is_canonical_heading_id(style),' \
  mnema-extract 'a_heading_is_what_styles_xml_says_it_is' --test docx

# C6. …and the filter drawn too wide, which is the same hole from the other side.
# An outline level of 9 is what OOXML spells "body text" with, so reading the
# attribute's presence rather than its value makes an ordinary paragraph style a
# heading and cuts a document into sections at every paragraph.
case_ "reader: outline level 9 is body text, not a tenth heading level" \
  crates/mnema-extract/src/docx.rs \
  's{    value\.trim\(\)\.parse::<u32>\(\)\.is_ok_and\(\|level\| level <= 8\)}{    value.trim().parse::<u32>().is_ok_and(|level| level <= 9)}' \
  'is_ok_and(|level| level <= 9)' \
  mnema-extract 'a_heading_is_what_styles_xml_says_it_is' --test docx

# C7. The second signal dropped: a style the stylesheet *names* a heading and
# gives no outline level.
case_ "reader: a style named a heading is one" \
  crates/mnema-extract/src/docx.rs \
  's{    name\.trim\(\)\.to_ascii_lowercase\(\)\.starts_with\("heading"\)}{    let _ = name;\n    false}' \
  'let _ = name;
    false' \
  mnema-extract 'a_style_named_a_heading_is_one_even_without_an_outline_level' --test docx

# C8. The third signal dropped — the one that carries a document whose
# stylesheet this reader could not open.
case_ "reader: a canonical heading id works without a stylesheet" \
  crates/mnema-extract/src/docx.rs \
  's{    id\.strip_prefix\("Heading"\)}{    let _ = id;\n    None::<\&str>}' \
  'None::<&str>' \
  mnema-extract 'a_missing_stylesheet_does_not_refuse_the_document' --test docx

# C9. A paragraph's own outline level not read, so a document that states its
# structure without naming a style has none.
case_ "reader: a paragraph's own outline level is read" \
  crates/mnema-extract/src/docx.rs \
  's{        b"outlineLvl" => paragraph\.outline = attribute\(e, b"val"\),}{        b"outlineLvl" => \{\},}' \
  'b"outlineLvl" => {},' \
  mnema-extract 'a_paragraphs_own_outline_level_opens_a_section' --test docx

# C9b was here and is deliberately gone. It removed a guard in `properties` that
# refused to read a `<w:pStyle>` outside `<w:pPr>` — and it **stayed green**,
# which is the whole finding: `parse` resolves `heading` only when a `</w:pPr>`
# returns the depth to zero, so a property outside one is recorded and never
# read, and the guard could not change an outcome. It was removed rather than
# kept with a case that cannot redden.
# `a_style_outside_the_paragraphs_properties_is_not_its_style` still asserts the
# behaviour; what no single-line mutation reaches is *where* `heading` is
# resolved, and that is stated here rather than left as a silent gap.

# C10. **A revision read as the present tense.** `<w:pPrChange>` carries the
# whole `<w:pPr>` a paragraph used to have, `<w:pStyle>` included, so a document
# under review is cut into sections wherever somebody once edited one.
case_ "reader: a paragraph that used to be a heading is not one now" \
  crates/mnema-extract/src/docx.rs \
  's{        b"Fallback" \| b"pPrChange" \| b"rPrChange" \| b"moveFrom"}{        b"Fallback" | b"rPrChange" | b"moveFrom"}' \
  'b"Fallback" | b"rPrChange" | b"moveFrom"' \
  mnema-extract 'a_paragraph_that_used_to_be_a_heading_is_not_one_now' --test docx

# C11. A styled cell opening a section. Header cells are styled constantly, and
# one page per cell shatters a document into dozens of sections named after
# column headings — every one of them a citation target a person would not
# recognise.
case_ "reader: a heading style in a table cell does not open a section" \
  crates/mnema-extract/src/docx.rs \
  's{        Some\(paragraph\) if paragraph\.in_table => \(BlockType::Table, None\),}{        Some(paragraph) if paragraph.in_table \&\& !paragraph.heading => (BlockType::Table, None),}' \
  'if paragraph.in_table && !paragraph.heading => (BlockType::Table, None),' \
  mnema-extract 'a_heading_in_a_table_cell_does_not_open_a_section' --test docx

# C12. The section name left unbounded. `page.section_title` is one column shown
# by one interface, and a name three hundred characters long is a citation that
# fills the pane it appears in. This case mutates `markdown.rs` and reddens a
# docx test on purpose: the shared rule is what five readers must not each
# decide.
case_ "reader: a section name is bounded by the rule every reader shares" \
  crates/mnema-extract/src/markdown.rs \
  's{    if flattened\.chars\(\)\.count\(\) <= SECTION_TITLE_MAX_CHARS \{\n        return Some\(flattened\);\n    \}}{    return Some(flattened);}' \
  'return Some(flattened);' \
  mnema-extract 'a_sections_name_is_bounded_by_the_rule_every_reader_shares' --test docx

# C13. The first heading opening page 2 and leaving page 1 empty behind it, so
# every section of every document is numbered one higher than it is.
case_ "reader: a document that begins with a heading has that heading as page 1" \
  crates/mnema-extract/src/docx.rs \
  's{    if empty_and_unnamed \{}{    if false \{}' \
  '    if false {' \
  mnema-extract 'paragraphs_come_back_with_their_headings_as_sections' --test docx

# ------------------------------------- what can vanish, asked of the **parse**

# C14. **The whole of a table's text.** 19 of the 24 files probed carry one, 2,157
# cells between them; a reader that skipped them would lose most of the text in
# most real documents and refuse a few outright as having none.
case_ "reader: the text inside a table is this document's text" \
  crates/mnema-extract/src/docx.rs \
  's{                    b"tbl" => table_depth \+= 1,}{                    b"tbl" => skip_depth = 1,}' \
  'b"tbl" => skip_depth = 1,' \
  mnema-extract 'text_in_a_table_is_indexed_and_typed_as_a_table' --test docx

# C15. …and the other direction: a cell's text stored as an ordinary paragraph,
# which loses the one thing `block.type` had to say about it.
case_ "reader: a cell's text is typed as a table" \
  crates/mnema-extract/src/docx.rs \
  's{        Some\(paragraph\) if paragraph\.in_table => \(BlockType::Table, None\),}{        Some(paragraph) if paragraph.in_table => (BlockType::Paragraph, None),}' \
  'if paragraph.in_table => (BlockType::Paragraph, None),' \
  mnema-extract 'text_in_a_table_is_indexed_and_typed_as_a_table' --test docx

# C16. **Every text node taken, not only the ones inside `<w:t>`.** This is the
# single mutation that reaches the two worst outcomes at once: a field
# instruction (` HYPERLINK \l "…" `) indexed as prose, and deleted text — the
# words the author removed — answering searches out of `<w:delText>`.
case_ "reader: only the text inside a run's own element is this document's" \
  crates/mnema-extract/src/docx.rs \
  's{                if skip_depth > 0 \|\| text_depth == 0 \{\n                    continue;\n                \}\n                // `xml_content`}{                if skip_depth > 0 \{\n                    continue;\n                \}\n                // `xml_content`}' \
  'if skip_depth > 0 {
                    continue;
                }
                // `xml_content`' \
  mnema-extract 'deleted_text_is_not_indexed_and_inserted_text_is' --test docx

# C17. The same mutation measured against the other outcome, because one test
# reddening does not say the second one would.
case_ "reader: a field instruction is not prose" \
  crates/mnema-extract/src/docx.rs \
  's{                if skip_depth > 0 \|\| text_depth == 0 \{\n                    continue;\n                \}\n                // `xml_content`}{                if skip_depth > 0 \{\n                    continue;\n                \}\n                // `xml_content`}' \
  'if skip_depth > 0 {
                    continue;
                }
                // `xml_content`' \
  mnema-extract 'a_field_instruction_is_not_prose_and_its_result_is' --test docx

# C18. `<mc:Fallback>` read as well as `<mc:Choice>`. The two say the same thing
# in two markups on purpose, so a text box arrives twice and a search hits one
# sentence in two blocks — one of which the document shows nowhere.
case_ "reader: an alternate content fallback is not a second copy of the text" \
  crates/mnema-extract/src/docx.rs \
  's{        b"Fallback" \| b"pPrChange" \| b"rPrChange" \| b"moveFrom"}{        b"pPrChange" | b"rPrChange" | b"moveFrom"}' \
  'b"pPrChange" | b"rPrChange" | b"moveFrom"' \
  mnema-extract 'an_alternate_content_fallback_does_not_store_the_text_twice' --test docx

# C19. A moved paragraph's old position stored too. Unlike a deletion it holds an
# ordinary `<w:t>`, so nothing about the element name keeps it out — the sentence
# is indexed where it is and where it is not.
case_ "reader: a moved paragraph is stored at its new position only" \
  crates/mnema-extract/src/docx.rs \
  's{        b"Fallback" \| b"pPrChange" \| b"rPrChange" \| b"moveFrom"}{        b"Fallback" | b"pPrChange" | b"rPrChange"}' \
  'b"Fallback" | b"pPrChange" | b"rPrChange"' \
  mnema-extract 'a_moved_paragraph_is_stored_once_at_its_new_position' --test docx

# C20. **The trap the probe found and no reasoning would have.** `<w:tab/>` inside
# `<w:pPr><w:tabs>` is a position on the ruler, not a character: 394 `<w:tab>`
# elements against 254 `<w:tabs>` containers in the corpus, so an unguarded arm
# puts tabs at the front of paragraph after paragraph, in text no document shows.
case_ "reader: a tab stop on the ruler is not a tab in the text" \
  crates/mnema-extract/src/docx.rs \
  's{                    b"tab" if ppr_depth == 0 => run\.push\('"'"'\\t'"'"'\),}{                    b"tab" => run.push('"'"'\\t'"'"'),}' \
  'b"tab" => run.push('"'"'\t'"'"'),' \
  mnema-extract 'a_tab_stop_definition_is_not_a_tab_character' --test docx

# C21. …and the tab dropped altogether, which is the `передпісля` defect `html.rs`
# measured reached through a different element: a word that is in no file and
# that a search for either half will not find.
case_ "reader: a tab carries the whitespace it stands for" \
  crates/mnema-extract/src/docx.rs \
  's{                    b"tab" if ppr_depth == 0 => run\.push\('"'"'\\t'"'"'\),}{                    b"tab" if ppr_depth == 0 => \{\},}' \
  'b"tab" if ppr_depth == 0 => {},' \
  mnema-extract 'a_break_and_a_tab_carry_the_whitespace_they_stand_for' --test docx

# C22. The same for a line break.
case_ "reader: a break carries the newline it stands for" \
  crates/mnema-extract/src/docx.rs \
  's{                    b"br" \| b"cr" => run\.push\('"'"'\\n'"'"'\),}{                    b"br" | b"cr" => \{\},}' \
  'b"br" | b"cr" => {},' \
  mnema-extract 'a_break_and_a_tab_carry_the_whitespace_they_stand_for' --test docx

# C22b. **The arm the first round left with no case at all**, while the report
# claimed C20–C22 covered it. `будь-який` written with `<w:noBreakHyphen/>` loses
# its hyphen and becomes a word in no file — the same shape as C21, through the
# one character element nobody measured.
case_ "reader: a non-breaking hyphen is a character the document paints" \
  crates/mnema-extract/src/docx.rs \
  's{                    b"noBreakHyphen" => run\.push\('"'"'-'"'"'\),}{                    b"noBreakHyphen" => \{\},}' \
  'b"noBreakHyphen" => {},' \
  mnema-extract 'a_break_and_a_tab_carry_the_whitespace_they_stand_for' --test docx

# C22c. **One spelling of an empty element, because there are two.**
# `<w:br></w:br>` is a `Start` and an `End`, not an `Empty`; with the config off,
# every character arm above is reachable only through the self-closing spelling
# and the other one silently drops the character. Measured in fix round 1: the
# expanded fixture came back `передпісляновийрядок`.
case_ "reader: an empty element written out in full is the same element" \
  crates/mnema-extract/src/docx.rs \
  's{    reader\.config_mut\(\)\.expand_empty_elements = true;\n    let mut sections = vec!\[DocxSection \{}{    reader.config_mut().expand_empty_elements = false;\n    let mut sections = vec![DocxSection \{}' \
  'reader.config_mut().expand_empty_elements = false;
    let mut sections = vec![DocxSection {' \
  mnema-extract 'an_empty_element_written_out_in_full_carries_the_same_character' --test docx

# C22d. The same config in the stylesheet reader, where it is load-bearing for a
# different reason: the match there has no `Empty` arm, so without expansion a
# `<w:name w:val="…"/>` — how every stylesheet writes it — is never read and no
# style is a heading at all.
case_ "reader: the stylesheet is read under the same one-spelling rule" \
  crates/mnema-extract/src/docx.rs \
  's{    reader\.config_mut\(\)\.expand_empty_elements = true;\n    let mut headings = HashSet::new\(\);}{    reader.config_mut().expand_empty_elements = false;\n    let mut headings = HashSet::new();}' \
  'reader.config_mut().expand_empty_elements = false;
    let mut headings = HashSet::new();' \
  mnema-extract 'a_heading_is_what_styles_xml_says_it_is' --test docx

# C23. **The depth count removed, and this case is what says whether it is doing
# anything.** quick-xml reaches `Event::Eof` on a part that stops inside an
# element without reporting an error, so without this the reader stores the prose
# before the cut, calls the document read, and says nothing about the rest. If
# this case ever goes green, quick-xml started catching it and the counter can go.
case_ "reader: a part that stops mid-element is damage, not half a document" \
  crates/mnema-extract/src/docx.rs \
  's{    if depth != 0 \{\n        return Err\(DocxError::Malformed\(format!\(\n            "\{DOCUMENT_PART\} ends inside an element"\n        \)\)\);\n    \}}{    if false \{\n        return Err(DocxError::Malformed(format!(\n            "\{DOCUMENT_PART\} ends inside an element"\n        )));\n    \}}' \
  'if false {
        return Err(DocxError::Malformed(format!(
            "{DOCUMENT_PART} ends inside an element"' \
  mnema-extract 'a_truncated_document_part_is_damage_rather_than_half_a_document' --test docx

# C24. **An escape dropped on the floor.** In quick-xml 0.41 a reference is an
# event of its own, so `Чай &amp; кава` arrives as three events and a reader that
# handles only `Event::Text` stores `Чай  кава` — a sentence the document does
# not contain, produced by the most ordinary punctuation there is.
case_ "reader: an xml escape is a character of the text" \
  crates/mnema-extract/src/docx.rs \
  's{                match resolve_reference\(e\) \{}{                match None::<String> \{}' \
  'match None::<String> {' \
  mnema-extract 'an_xml_escape_is_one_character_of_the_text' --test docx

# C25. A drawing skipped whole. It is where a text box lives, so skipping it
# loses prose that a reader sees on the page — and the fixture that measures it
# has nothing *but* that prose, so the document is refused as having no text.
case_ "reader: the words inside a drawing are this document's words" \
  crates/mnema-extract/src/docx.rs \
  's{        b"Fallback" \| b"pPrChange" \| b"rPrChange" \| b"moveFrom"}{        b"Fallback" | b"pPrChange" | b"rPrChange" | b"moveFrom" | b"drawing"}' \
  'b"moveFrom" | b"drawing"' \
  mnema-extract 'a_drawings_description_is_not_text_and_the_words_inside_it_are' --test docx

# ---------------------------------------------------------------- the verbatim
#
# Task 10 checks both of these for HTML and Task 11 repeats them for a book.
# They are repeated here against this reader's own fixture on purpose: an
# invariant checked in one reader of five is an invariant missing from four.

# C26. NFC dropped. A Ukrainian `й` written decomposed and one written composed
# tokenize as two different words (D32) — and this format is where it bites,
# because these producers write `&#1080;&#774;` rather than the characters.
case_ "reader: a block's text is normalised at all" \
  crates/mnema-extract/src/docx.rs \
  's{    let text = nfc::normalise\(run\)\.into_owned\(\);}{    let text = run.clone();}' \
  'let text = run.clone();' \
  mnema-extract 'a_docxs_text_is_verbatim_after_nfc_and_nothing_else' --test docx

# C27. The server's `_clean = " ".join(text.split())`
# (`app/textdoc/html_blocks.py:41-42`) ported after all. G7.1 §2.3 refused it: a
# rule that rewrites stored text is one nothing downstream can undo.
case_ "reader: a block's text is not folded on its way in" \
  crates/mnema-extract/src/docx.rs \
  's{    let text = nfc::normalise\(run\)\.into_owned\(\);}{    let text = nfc::normalise(run).split_whitespace().collect::<Vec<_>>().join(" ");}' \
  'let text = nfc::normalise(run).split_whitespace().collect::<Vec<_>>().join(" ");' \
  mnema-extract 'a_docxs_text_is_verbatim_after_nfc_and_nothing_else' --test docx

# C28. The same NFC line, measured against the *title* rather than the block —
# a second outcome of one mutation, and one test reddening does not say the
# other would. A section named `и\u{306}од` answers no query typed `йод`, and no
# offset is ever measured into a title to make that recoverable.
case_ "reader: a section's name is normalised, not only its blocks" \
  crates/mnema-extract/src/docx.rs \
  's{    let text = nfc::normalise\(run\)\.into_owned\(\);}{    let text = run.clone();}' \
  'let text = run.clone();' \
  mnema-extract 'a_section_name_from_a_character_reference_is_composed_too' --test docx

# C29. A run of nothing but whitespace stored as a block. Word writes a great
# many empty paragraphs, and a block holding `"\n"` is searchable, citable and
# empty of content.
case_ "reader: an empty paragraph is not a block" \
  crates/mnema-extract/src/docx.rs \
  's{    if run\.trim\(\)\.is_empty\(\) \{\n        run\.clear\(\);\n        return;\n    \}}{    if false \{\n        run.clear();\n        return;\n    \}}' \
  'if false {
        run.clear();
        return;
    }' \
  mnema-extract 'an_empty_paragraph_makes_no_block' --test docx

# C30. `reading_order` frozen. The index's uniqueness is on
# `(page_id, reading_order)`, so every block after the first collides.
case_ "reader: blocks are numbered in the order they are read" \
  crates/mnema-extract/src/docx.rs \
  's{        reading_order: section\.blocks\.len\(\) as i64,}{        reading_order: 0,}' \
  'reading_order: 0,' \
  mnema-extract 'reading_order_restarts_on_every_section' --test docx

# ------------------------------------------------------------- the caps and the errors

# C31. The cap removed from the document part. The server capped its docx read on
# the stream for exactly this (`app/textdoc/office.py:41-52`).
case_ "reader: the document part is read under a cap at all" \
  crates/mnema-extract/src/docx.rs \
  's{    let document = zip_part::read_member\(bytes, DOCUMENT_PART, cap\)}{    let document = zip_part::read_member(bytes, DOCUMENT_PART, usize::MAX)}' \
  'zip_part::read_member(bytes, DOCUMENT_PART, usize::MAX)' \
  mnema-extract 'docx::tests::both_members_are_read_under_the_cap_and_only_absence_is_forgiven' --lib

# C32. The stylesheet read without one. A cap on `word/document.xml` alone is not
# a cap on this reader: the stylesheet is a second member out of the same archive
# and inflates just as far.
case_ "reader: the stylesheet is read under the cap too" \
  crates/mnema-extract/src/docx.rs \
  's{    let headings = match zip_part::read_member\(bytes, STYLES_PART, cap\)}{    let headings = match zip_part::read_member(bytes, STYLES_PART, usize::MAX)}' \
  'zip_part::read_member(bytes, STYLES_PART, usize::MAX)' \
  mnema-extract 'docx::tests::both_members_are_read_under_the_cap_and_only_absence_is_forgiven' --lib

# C33. **The asymmetry, inverted.** A stylesheet that is *absent* is an ordinary
# document; one that inflates to gigabytes is the bomb the cap exists for. Folding
# the two together puts a second, uncapped way into this process behind a part
# nobody reads.
case_ "reader: a stylesheet past the cap is refused rather than ignored" \
  crates/mnema-extract/src/docx.rs \
  's{        Err\(ZipPartError::TooLarge\) => return Err\(DocxError::TooLarge\),\n        Err\(ZipPartError::Missing \| ZipPartError::Malformed\) => HashSet::new\(\),}{        Err(_) => HashSet::new(),}' \
  'Err(_) => HashSet::new(),' \
  mnema-extract 'docx::tests::both_members_are_read_under_the_cap_and_only_absence_is_forgiven' --lib

# C33b. **The unreadable stylesheet, at the zip level.** Fix round 1's finding:
# `Malformed` was folded into the same arm as `Missing` with nothing measuring
# it, so a member whose deflate stream is damaged could have started refusing the
# whole document and no test would have said so.
case_ "reader: a stylesheet whose stream will not decompress does not refuse the document" \
  crates/mnema-extract/src/docx.rs \
  's{        Err\(ZipPartError::Missing \| ZipPartError::Malformed\) => HashSet::new\(\),}{        Err(ZipPartError::Missing) => HashSet::new(),\n        Err(ZipPartError::Malformed) => return Err(DocxError::Malformed("bad stylesheet".to_string())),}' \
  'Err(ZipPartError::Malformed) => return Err(DocxError::Malformed("bad stylesheet".to_string())),' \
  mnema-extract 'a_stylesheet_that_will_not_decompress_costs_headings_and_not_the_document' --test docx

# C33c. **The unreadable stylesheet, at the XML level.** `while let Ok(…)` is what
# swallows a parse failure, and it swallowed it untested. The mutation makes the
# reader stop tolerating a stylesheet it cannot parse, which is the shape any
# later move to a `Result`-returning `heading_styles` would take — this case is
# what such a change has to confront.
case_ "reader: a stylesheet that will not parse does not take the document with it" \
  crates/mnema-extract/src/docx.rs \
  's{    while let Ok\(event\) = reader\.read_event\(\) \{}{    while let event = reader.read_event().expect("the stylesheet parses") \{}' \
  'reader.read_event().expect("the stylesheet parses")' \
  mnema-extract 'a_stylesheet_that_will_not_parse_costs_headings_and_not_the_document' --test docx

# C34. …and the other direction: a stylesheet that is simply not there taking the
# whole document with it. It holds no prose at all, so refusing costs a document
# over a part nobody reads.
case_ "reader: a missing stylesheet does not refuse the document" \
  crates/mnema-extract/src/docx.rs \
  's{        Err\(ZipPartError::Missing \| ZipPartError::Malformed\) => HashSet::new\(\),}{        Err(ZipPartError::Missing | ZipPartError::Malformed) => return Err(DocxError::Malformed("no stylesheet".to_string())),}' \
  'return Err(DocxError::Malformed("no stylesheet".to_string()))' \
  mnema-extract 'a_missing_stylesheet_does_not_refuse_the_document' --test docx

# C35. The part that makes it a document reported as "nothing to read here"
# rather than as damage. `unsupported` promises a reader that is coming and this
# format has one; `no_text_layer` says the file is pictures. Neither is true.
case_ "reader: the part a document needs is damage when it is absent" \
  crates/mnema-extract/src/docx.rs \
  's{DocxError::Malformed\(format!\("\{DOCUMENT_PART\} is not in the archive"\)\)}{DocxError::NoText /* the part is missing */}' \
  'DocxError::NoText /* the part is missing */' \
  mnema-extract 'a_docx_without_word_document_xml_is_malformed_not_unsupported' --test docx

# C36. A document with no text stored as a document with no blocks — a row in the
# index that answers no query and tells the person who added the file nothing.
case_ "reader: a document with no text is refused, not stored empty" \
  crates/mnema-extract/src/docx.rs \
  's{    if sections\.iter\(\)\.all\(\|section\| section\.blocks\.is_empty\(\)\) \{}{    if false \{}' \
  '    if false {' \
  mnema-extract 'a_document_with_no_text_is_refused_rather_than_stored_empty' --test docx

# C37. The digest dropped from a docx refusal. It is the field that tells the
# parent whether the file changed or only the rule did, and `every_refusal…` is a
# hand-written table — which is exactly why a reader that refuses under a new rule
# and does not add its rows is a branch nothing measures.
case_ "worker: a docx refused after being read carries the digest it was read on" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                reason: "no paragraph of this document carries any text"\.to_string\(\),\n                sha256: Some\(sha256\),}{                reason: "no paragraph of this document carries any text".to_string(),\n                sha256: None,}' \
  'reason: "no paragraph of this document carries any text".to_string(),
                sha256: None,' \
  mnema-extract 'every_refusal_that_read_the_file_carries_the_digest_it_read' --test worker_cli

# C38. A docx that cannot be read reported under the rule that promises a reader
# is coming. `unsupported` is what this format got before this task; saying it
# again after the reader exists tells the person holding a damaged file to wait
# for a release that has already happened.
case_ "worker: a damaged docx is refused as damaged, not as unsupported" \
  crates/mnema-extract/src/bin/worker.rs \
  's{            Err\(e @ DocxError::Malformed\(_\)\) => vec!\[Frame::Refused \{\n                rule: "malformed"\.to_string\(\),}{            Err(e @ DocxError::Malformed(_)) => vec![Frame::Refused \{\n                rule: "unsupported".to_string(),}' \
  'rule: "unsupported".to_string(),' \
  mnema-extract 'every_refusal_that_read_the_file_carries_the_digest_it_read' --test worker_cli

# C39. The summary naming a page that was also sent. A docx skips nothing, so
# there is no number this field could honestly carry — and one here stops the
# entire walk with `PoolError::Protocol`, which accuses the worker binary of
# being from another release (`crates/mnema-pool/src/lib.rs:1338`).
case_ "worker: a docx summary names no skipped page" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                    skipped_pages: Vec::new\(\),\n                    text_source: "native:docx"\.to_string\(\),}{                    skipped_pages: vec![1],\n                    text_source: "native:docx".to_string(),}' \
  'skipped_pages: vec![1],
                    text_source: "native:docx".to_string(),' \
  mnema-extract 'a_docx_is_read_section_by_section_and_its_summary_skips_nothing' --test worker_cli

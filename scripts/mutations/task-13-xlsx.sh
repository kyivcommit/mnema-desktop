# Mutation cases for Task 13: the XLSX reader. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-13-xlsx.sh
#
# **`task-13-xlsx.sh`, not `task-13.sh`.** That name is taken by an earlier cycle
# (`15fc7ce`, walk-level cases for the randomised harness, D47) and has not
# changed on this branch; overwriting it would destroy working coverage for the
# sake of a naming convention. The same collision already happened to `task-8`
# and `task-9`.
#
# What this file is mostly about is a class of silence that is this format's own,
# and it is not the one Task 11 or Task 12 measured. A book loses whole chapters;
# a document loses words inside a paragraph. A **workbook loses a coordinate** —
# and it loses it while still storing perfectly good prose, which is what makes
# it invisible:
#
#   * a row cited by the wrong number — the block's position among the rows that
#     had something in them instead of the row it is. Every word is right, the
#     citation points at another part of the sheet;
#   * a citation with no sheet on it — the reader's name one character off, so
#     `pages_of` falls to `PageContext::Lines`. Unlike a docx's, these blocks
#     *do* carry line numbers, so nothing is empty and nothing goes red: the
#     answer just says "рядки 10–20" of nothing in particular;
#   * a name shown in two forms — the sheet's name is both the page's title and
#     the coordinate's `sheet`, so bounding it twice makes the citation and the
#     coordinate disagree;
#   * a sheet that vanishes without a journal row — the shape calamine's own
#     `worksheets()` has, and the reason this reader does not use it;
#   * a sheet reported both sent and skipped, which is not silent at all: the
#     pool stops the whole job for it (`crates/mnema-pool/src/lib.rs:1338`).
#
# Cases are anchored on code, never on the prose beside it: task 10 lost five of
# its own to a fix round that edited the comments they matched.
#
# **Two behaviours below have no case and that is not an omission.**
# `a_date_is_indexed_as_the_number_it_is_in_the_file` and the formula rule record
# what *calamine* does; there is no line in this repository to break, and a case
# that mutated one anyway would be measuring the library.

# ------------------------------------------------------ the reader's own name

# C1. The name is one symbol across a process boundary and across D40, and this
# format is the one where a near-miss is quietest: `pages_of` matches it to reach
# `PageContext::Rows` (`crates/mnema-ingest/src/lib.rs:1403`), and the fallback
# arm asks blocks that DO carry numbers for a line range — so the citation is
# non-empty, plausible and nameless.
case_ "worker: the header names the xlsx reader, not a near-miss" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                    reader: manifest::READER_XLSX\.to_string\(\),}{                    reader: "xlsx-2".to_string(),}' \
  'reader: "xlsx-2".to_string(),' \
  mnema-extract 'an_xlsx_is_read_sheet_by_sheet_and_its_summary_names_what_it_skipped' --test worker_cli

# C2. The value of the constant itself, from the other side of D40. `slice.rs`
# states the literal `"xlsx"` to a stand-in worker and asserts the coordinate that
# comes back, so changing what `READER_XLSX` *is* breaks the match in `pages_of`
# while every string in this crate still agrees with itself.
case_ "core: READER_XLSX is the string mnema-ingest matches" \
  crates/mnema-core/src/manifest.rs \
  's{pub const READER_XLSX: &str = "xlsx";}{pub const READER_XLSX: \&str = "spreadsheet";}' \
  'pub const READER_XLSX: &str = "spreadsheet";' \
  mnema-ingest 'an_xlsx_chunk_cites_the_rows_it_covers_not_the_whole_sheet' --test slice

# C3. The version, which is what decides whether a document already in the index
# was made by today's code — and, since task 10's C22, whether it is replaced.
case_ "worker: the header carries the xlsx reader's version" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                    reader_version: manifest::XLSX_READER_VERSION,}{                    reader_version: 0,}' \
  'reader_version: 0,' \
  mnema-extract 'an_xlsx_is_read_sheet_by_sheet_and_its_summary_names_what_it_skipped' --test worker_cli

# C4. `native:xlsx` names the reader, not the file. Text that came out of a
# spreadsheet's cells is not the same evidence as text that came out of a
# plain-text read of the same bytes, and `page.text_source` is where that is
# recorded.
case_ "worker: the summary names the xlsx text source" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                    text_source: "native:xlsx"\.to_string\(\),}{                    text_source: "native:docx".to_string(),}' \
  'text_source: "native:docx".to_string(),' \
  mnema-extract 'an_xlsx_is_read_sheet_by_sheet_and_its_summary_names_what_it_skipped' --test worker_cli

# C5. The extension is not the decision. An `.xlsm` is `xl/workbook.xml` in a zip
# exactly as an `.xlsx` is, and the manifest must not claim otherwise — an entry
# there predicts the wrong reader for a text file that happens to be named
# `кошторис.xlsx`.
case_ "manifest: xlsx is decided by content and is absent from the extension map" \
  crates/mnema-extract/src/manifest.rs \
  's~    for extension in \["html", "htm"\] \{~    by_extension.insert("xlsx".to_string(), ReaderId::new(READER_XLSX, XLSX_READER_VERSION));\n    for extension in ["html", "htm"] {~' \
  'by_extension.insert("xlsx".to_string(), ReaderId::new(READER_XLSX, XLSX_READER_VERSION));' \
  mnema-extract 'an_xlsx_is_read_by_content_so_the_manifest_predicts_the_wrong_reader_for_it' --test manifest

# --------------------------------------------------------------- the coordinate

# C6. The row is the sheet's, 1-based; calamine counts from 0. Off by one, every
# citation in the product points one row above the text it quotes.
case_ "reader: a block's row is 1-based, as a sheet's rows are" \
  crates/mnema-extract/src/xlsx.rs \
  's{            line_start: Some\(row\.saturating_add\(1\)\),}{            line_start: Some(row),}' \
  'line_start: Some(row),' \
  mnema-extract 'a_row_is_one_block_with_its_sheet_row_number' --test xlsx

# C7. **The defect that would survive every other test in this file.** Counting
# the blocks emitted instead of reading the row gives the right answer for a
# sheet with no gaps — which is what a fixture usually is — and cites row 2 for a
# row that is row 100.
case_ "reader: the row is read off the cell, not counted off the blocks" \
  crates/mnema-extract/src/xlsx.rs \
  's{            line_start: Some\(row\.saturating_add\(1\)\),\n            line_end: Some\(row\.saturating_add\(1\)\),}{            line_start: Some(blocks.len() as u32 + 1),\n            line_end: Some(blocks.len() as u32 + 1),}' \
  'line_start: Some(blocks.len() as u32 + 1),' \
  mnema-extract 'a_block_carries_the_row_it_sits_on_not_its_position_among_the_rows' --test xlsx

# C8. One end of the range missing is the whole range gone: `line_range` answers
# `Coordinate::None` for a chunk covering one block without numbers
# (`crates/mnema-chunk/src/lib.rs`), so the prose is stored and the citation
# points at nothing.
case_ "reader: a block owes both ends of its row range" \
  crates/mnema-extract/src/xlsx.rs \
  's{            line_end: Some\(row\.saturating_add\(1\)\),}{            line_end: None,}' \
  'line_end: None,' \
  mnema-extract 'a_row_is_one_block_with_its_sheet_row_number' --test xlsx

# ---------------------------------------------------------------- the row itself

# C9. The separator this reader chose over `html.rs`'s answer. A space would make
# `Назва Сума` — two column headings a search cannot tell from a phrase.
case_ "reader: cells are joined by a tab, which is the decision this reader made" \
  crates/mnema-extract/src/xlsx.rs \
  "s{const CELL_SEPARATOR: char = '\\\\t';}{const CELL_SEPARATOR: char = ' ';}" \
  "const CELL_SEPARATOR: char = ' ';" \
  mnema-extract 'a_row_is_one_block_with_its_sheet_row_number' --test xlsx

# C10. A value's column is part of what the row says. Starting the join at the
# first occupied column instead of at A moves every value in a row that begins
# with a gap into the wrong column, which is the one thing a table cannot survive.
case_ "reader: a leading gap is a column position, not padding to drop" \
  crates/mnema-extract/src/xlsx.rs \
  's~        for column in 0\.\.=last \{~        for column in *cells.keys().next().expect("a row has a cell")..=last {~' \
  'for column in *cells.keys().next().expect("a row has a cell")..=last {' \
  mnema-extract 'a_value_keeps_its_column_and_a_row_gains_no_trailing_tab' --test xlsx

# C11. The other end of the same rule, and the server writes it as `.rstrip`: an
# explicitly empty cell after the last value must not widen the row. Reached here
# by keeping empty cells at all, which is where the property actually lives.
case_ "reader: an empty cell does not widen its row" \
  crates/mnema-extract/src/xlsx.rs \
  's~                if text\.is_empty\(\) \{~                if false {~' \
  '                if false {' \
  mnema-extract 'a_value_keeps_its_column_and_a_row_gains_no_trailing_tab' --test xlsx

# C12. The server's `.strip()` (`app/textdoc/office.py:254`), ported. It reads as
# tidying and it moves a value out of its column.
case_ "reader: the server's strip is not ported (D32, D38)" \
  crates/mnema-extract/src/xlsx.rs \
  's{        let text = nfc::normalise\(&text\)\.into_owned\(\);}{        let text = nfc::normalise(text.trim()).into_owned();}' \
  'let text = nfc::normalise(text.trim()).into_owned();' \
  mnema-extract 'a_value_keeps_its_column_and_a_row_gains_no_trailing_tab' --test xlsx

# C13. NFC, over the block text, before anything downstream takes an offset or a
# hash from it. macOS hands over decomposed text and a query typed elsewhere is
# precomposed: without this a document becomes unfindable by its own spelling
# (D32).
case_ "reader: NFC runs over a row's text" \
  crates/mnema-extract/src/xlsx.rs \
  's{        let text = nfc::normalise\(&text\)\.into_owned\(\);}{        let text = text.clone();}' \
  'let text = text.clone();' \
  mnema-extract 'the_text_is_verbatim_after_nfc_and_nothing_else' --test xlsx

# C14. A row of separators and spaces is searchable, citable and empty of
# content. `chunk_blocks` filters it out downstream, so storing one leaves a
# `block` row nothing can ever cite — and a row of nothing but uncached formulas
# is exactly that shape.
case_ "reader: a row with no words in it is not a block" \
  crates/mnema-extract/src/xlsx.rs \
  's~        if text\.trim\(\)\.is_empty\(\) \{~        if text.is_empty() {~' \
  '        if text.is_empty() {' \
  mnema-extract 'a_row_with_no_words_in_it_produces_no_block' --test xlsx

# C15. A spreadsheet row is tabular, as an HTML `<td>` and a paragraph inside
# `<w:tbl>` are. `block.type` is a closed vocabulary the index groups on
# (`schema.sql:119-120`).
case_ "reader: a row is a Table block" \
  crates/mnema-extract/src/xlsx.rs \
  's{            block_type: BlockType::Table,}{            block_type: BlockType::Paragraph,}' \
  'block_type: BlockType::Paragraph,' \
  mnema-extract 'a_row_is_one_block_with_its_sheet_row_number' --test xlsx

# C16. `reading_order` restarts on every sheet and is what reconstructs a page.
# The schema's uniqueness is on `(page_id, reading_order)`.
case_ "reader: reading order counts within its sheet" \
  crates/mnema-extract/src/xlsx.rs \
  's{            reading_order: blocks\.len\(\) as i64,}{            reading_order: 0,}' \
  'reading_order: 0,' \
  mnema-extract 'a_row_is_one_block_with_its_sheet_row_number' --test xlsx

# ------------------------------------------------------------- what disappears

# C17. **The shape calamine's own `worksheets()` has**, put back by hand: a sheet
# that cannot be read leaves no number behind, so the journal never learns a
# sheet is missing and the index quietly holds a workbook minus one sheet.
case_ "reader: a damaged sheet leaves its number behind" \
  crates/mnema-extract/src/xlsx.rs \
  's~            SheetRead::Damaged => \{\n                saw_damage = true;\n                skipped\.push\(page_no\);~            SheetRead::Damaged => {\n                saw_damage = true;~' \
  'SheetRead::Damaged => {
                saw_damage = true;
                continue;' \
  mnema-extract 'a_sheet_that_cannot_be_read_is_skipped_by_number_and_the_rest_is_not' --test xlsx

# C18. The same for a sheet the archive does not hold, which is a different arm
# and a different way in — a chartsheet reaches it too.
case_ "reader: an absent sheet leaves its number behind" \
  crates/mnema-extract/src/xlsx.rs \
  's~            SheetRead::Absent => \{\n                skipped\.push\(page_no\);\n                continue;~            SheetRead::Absent => {\n                continue;~' \
  'SheetRead::Absent => {
                continue;' \
  mnema-extract 'a_sheet_that_cannot_be_read_is_skipped_by_number_and_the_rest_is_not' --test xlsx

# C19. **The prefix of a truncated sheet, kept.** calamine hands back every cell
# before the cut and then errors, so this reads as "salvage what we can" and is
# the shape `docx.rs`'s `depth != 0` check refuses: a partial sheet stored as
# though it were whole, with nothing anywhere saying so.
case_ "reader: the cells before a cut are not a sheet" \
  crates/mnema-extract/src/xlsx.rs \
  's{            Err\(_\) => return SheetRead::Damaged,}{            Err(_) => return SheetRead::Rows(rows),}' \
  'Err(_) => return SheetRead::Rows(rows),' \
  mnema-extract 'a_sheet_that_cannot_be_read_is_skipped_by_number_and_the_rest_is_not' --test xlsx

# C20. Two sheets with one name: `worksheet_cells_reader` resolves by `find`, so
# asking twice reads the first sheet twice. Without this guard the workbook holds
# one sheet's rows under two page numbers and the other sheet not at all — wrong
# in both directions from one line.
case_ "reader: a repeated sheet name is not read a second time" \
  crates/mnema-extract/src/xlsx.rs \
  's~        if !asked\.insert\(name\.clone\(\)\) \{~        if false && !asked.insert(name.clone()) {~' \
  'if false && !asked.insert(name.clone()) {' \
  mnema-extract 'a_repeated_sheet_name_is_skipped_rather_than_read_a_second_time' --test xlsx

# C21. The page number is the sheet's position in the workbook, not its position
# among the sheets that came back. Renumbering makes `skipped_pages` and the page
# frames describe two different workbooks.
case_ "reader: a page number is a position in the workbook" \
  crates/mnema-extract/src/xlsx.rs \
  's{        let page_no = index as u32 \+ 1;}{        let page_no = sheets.len() as u32 + 1;}' \
  'let page_no = sheets.len() as u32 + 1;' \
  mnema-extract 'an_xlsx_is_read_sheet_by_sheet_and_its_summary_names_what_it_skipped' --test worker_cli

# C22. **The one failure here that is not silent**, and it is worth a case for
# that reason: an empty sheet both sent and named as skipped is `PoolError::
# Protocol`, which stops the entire job and accuses the worker binary of being
# from another release (`crates/mnema-pool/src/lib.rs:1338`).
case_ "reader: a skipped sheet is not also sent" \
  crates/mnema-extract/src/xlsx.rs \
  's~            skipped\.push\(page_no\);\n            continue;\n        \}\n\n        sheets\.push\(XlsxSheet \{~            skipped.push(page_no);\n        }\n\n        sheets.push(XlsxSheet {~' \
  'skipped.push(page_no);
        }

        sheets.push(XlsxSheet {' \
  mnema-extract 'an_xlsx_is_read_sheet_by_sheet_and_its_summary_names_what_it_skipped' --test worker_cli

# C23. `pages` is counted off the same vector the page frames come from, so the
# pool's count check cannot disagree with itself. Adding the skipped sheets back
# in announces pages that never arrive.
case_ "worker: the header counts the sheets it sends" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                    pages: workbook\.sheets\.len\(\) as u32,}{                    pages: (workbook.sheets.len() + workbook.skipped.len()) as u32,}' \
  'pages: (workbook.sheets.len() + workbook.skipped.len()) as u32,' \
  mnema-extract 'an_xlsx_is_read_sheet_by_sheet_and_its_summary_names_what_it_skipped' --test worker_cli

# C24. A hidden sheet is read, and this is the plausible alternative rule put in
# to prove the decision is tested rather than assumed. It costs a workbook's
# lookup tables and its archived years.
case_ "reader: a hidden sheet is read like any other" \
  crates/mnema-extract/src/xlsx.rs \
  's~        if !asked\.insert\(name\.clone\(\)\) \{~        if workbook.sheets_metadata().get(index).is_some_and(|s| s.visible != calamine::SheetVisible::Visible) || !asked.insert(name.clone()) {~' \
  'if workbook.sheets_metadata().get(index).is_some_and(|s| s.visible != calamine::SheetVisible::Visible) || !asked.insert(name.clone()) {' \
  mnema-extract 'a_hidden_sheet_is_read_like_any_other' --test xlsx

# ------------------------------------------------------------------- the name

# C25. The shared bound, not a rule of this reader's own — one column, one
# window, four readers.
case_ "reader: a sheet name is bounded" \
  crates/mnema-extract/src/xlsx.rs \
  's{    bound_section_title\(flattened\)}{    Some(flattened)}' \
  '    Some(flattened)' \
  mnema-extract 'xlsx::tests::a_sheet_name_is_flattened_and_bounded_once' --lib

# C26. **The defect the brief named, and it is specific to this format.** The
# sheet's name is both the page's title and `Coordinate::SheetRows { sheet }`;
# bounding it a second time on the way to the wire makes the citation show one
# name and the coordinate another, silently, with everything else green.
case_ "worker: the name on the wire is the one the reader bounded" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                        section_title: sheet\.section_title,}{                        section_title: sheet.section_title.map(\|t\| t.chars().take(50).collect()),}' \
  'section_title: sheet.section_title.map(|t| t.chars().take(50).collect()),' \
  mnema-extract 'a_sheet_name_crosses_the_wire_bounded_exactly_once' --test worker_cli

# C27. NFC before the bound, not after: normalisation changes the character
# count, so bounding first cuts a decomposed name in the wrong place — and only
# for the names that are decomposed, which is the ones macOS hands over.
case_ "reader: a sheet name is normalised before it is bounded" \
  crates/mnema-extract/src/xlsx.rs \
  's{    let flattened = nfc::normalise\(name\)}{    let flattened = std::borrow::Cow::Borrowed(name)}' \
  'let flattened = std::borrow::Cow::Borrowed(name)' \
  mnema-extract 'xlsx::tests::a_sheet_name_is_flattened_and_bounded_once' --lib

# --------------------------------------------------------------------- the cap

# C28. **The whole guard is the ORDER.** calamine has no cap of its own and reads
# `xl/sharedStrings.xml` inside its constructor: measured, a 409 KB archive built
# a 400 MiB string there, peaking at 1.68 GB, before a cell was read. A cap
# applied afterwards measures memory already allocated.
case_ "reader: the package is measured before calamine opens it" \
  crates/mnema-extract/src/xlsx.rs \
  's{    measure_package\(bytes, budget\)\?;}{    let _ = budget;}' \
  '    let _ = budget;' \
  mnema-extract 'xlsx::tests::a_member_over_the_cap_refuses_the_workbook_and_one_under_it_does_not' --lib

# C29. The cap stands on what came out of the stream, never on the size the
# archive declares. Reading exactly `cap` bytes makes `inflated > cap` a
# condition that can never be true — a guard that cannot fail, which is worse
# than no guard.
case_ "reader: the cap is decided on one byte past it" \
  crates/mnema-extract/src/xlsx.rs \
  's{Read::take\(&mut member, cap \+ 1\)}{Read::take(\&mut member, cap)}' \
  'Read::take(&mut member, cap)' \
  mnema-extract 'xlsx::tests::a_member_over_the_cap_refuses_the_workbook_and_one_under_it_does_not' --lib

# C30. The budget is a total. N members each just under the per-member cap is the
# same attack with more entries — `zip_part`'s own doc names it — and a workbook
# opens one member per sheet.
case_ "reader: every member draws against one budget" \
  crates/mnema-extract/src/xlsx.rs \
  's{        budget = budget\.saturating_sub\(inflated as usize\);}{        budget = budget;}' \
  '        budget = budget;' \
  mnema-extract 'xlsx::tests::many_small_members_exhaust_one_budget_between_them' --lib

# C31. And the budget has to reach the per-member cap, or the total is a number
# nothing consults.
case_ "reader: the cap on one member is the smaller of the two" \
  crates/mnema-extract/src/xlsx.rs \
  's{        let cap = MEMBER_MAX_BYTES\.min\(budget\) as u64;}{        let cap = MEMBER_MAX_BYTES as u64;}' \
  'let cap = MEMBER_MAX_BYTES as u64;' \
  mnema-extract 'xlsx::tests::many_small_members_exhaust_one_budget_between_them' --lib

# ---------------------------------------------------------------- the refusals

# C32. A workbook of charts and a workbook whose sheets will not parse are
# different sentences to show and different verdicts downstream: one is about
# content that is not there, the other about a file that is damaged.
case_ "reader: damage is not emptiness" \
  crates/mnema-extract/src/xlsx.rs \
  's~        return Err\(if saw_damage \{~        return Err(if false {~' \
  '        return Err(if false {' \
  mnema-extract 'a_workbook_with_nothing_in_it_is_refused_and_damage_is_not_emptiness' --test xlsx

# C33. `no_text_layer`, not `unsupported`. The parent treats a verdict about
# content differently from a promise that a reader is coming — and until this
# build a `.xlsx` got the second one.
case_ "worker: a workbook with no text is refused by content" \
  crates/mnema-extract/src/bin/worker.rs \
  's~            Err\(e \@ XlsxError::NoText\) => vec!\[Frame::Refused \{\n                rule: "no_text_layer"\.to_string\(\),~            Err(e \@ XlsxError::NoText) => vec![Frame::Refused {\n                rule: "unsupported".to_string(),~' \
  'Err(e @ XlsxError::NoText) => vec![Frame::Refused {
                rule: "unsupported".to_string(),' \
  mnema-extract 'every_refusal_that_read_the_file_carries_the_digest_it_read' --test worker_cli

# C34. Every refusal that read the file owes the digest of what it read:
# `displaces` reads a missing one as "the bytes are unknown, displace", so a
# folder of workbooks would lose a document per file with the bytes never having
# moved.
case_ "worker: an xlsx refusal carries the digest it was reached on" \
  crates/mnema-extract/src/bin/worker.rs \
  's~            Err\(e \@ XlsxError::TooLarge\) => vec!\[Frame::Refused \{\n                rule: "too_large"\.to_string\(\),\n                reason: e\.to_string\(\),\n                sha256: Some\(sha256\),~            Err(e \@ XlsxError::TooLarge) => vec![Frame::Refused {\n                rule: "too_large".to_string(),\n                reason: e.to_string(),\n                sha256: None,~' \
  'Err(e @ XlsxError::TooLarge) => vec![Frame::Refused {
                rule: "too_large".to_string(),
                reason: e.to_string(),
                sha256: None,' \
  mnema-extract 'every_refusal_that_read_the_file_carries_the_digest_it_read' --test worker_cli

# C35. An `.xlsm` is the same part read the same way. An extension check anywhere
# on this path takes every macro-enabled workbook out of the index, and the only
# thing that shows it is a test naming the extension.
case_ "worker: an xlsm is not refused for its name" \
  crates/mnema-extract/src/bin/worker.rs \
  's~        Reader::Xlsx => match extract_xlsx\(&bytes\) \{~        Reader::Xlsx if extension == Some("xlsm") => vec![Frame::Failed {\n            message: "no".to_string(),\n        }],\n        Reader::Xlsx => match extract_xlsx(&bytes) {~' \
  'Reader::Xlsx if extension == Some("xlsm") => vec![Frame::Failed {' \
  mnema-extract 'an_xlsm_is_read_by_the_spreadsheet_reader' --test worker_cli

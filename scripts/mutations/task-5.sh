# Mutation cases for Task 5: which coordinate a page's chunks carry, and where
# its numbers come from. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-5.sh
#
# Everything wrong here is silent. A citation with no coordinate renders as an
# empty string beside text that is otherwise correct; a citation naming a whole
# sheet renders as a perfectly plausible sentence. Nothing crashes, no query
# fails, and the only person who finds out is the one who opens the file to
# check the quote.
#
# The pair the file is arranged around is C6/C7. "The chunk has a coordinate" is
# satisfied by the sheet-wide range that is the whole defect `PageContext::Rows`
# exists to prevent — so the assertions have to say *which* rows, and these two
# cases are what proves they do, in the chunker and again in the database.

# ---------------------------------------------------------------- the readers

# C1. The pdf arm is reached by the name the worker states, and a name that
# never arrives falls to the default without a word. This is what a typo in one
# of these strings costs: every page of every PDF cited with nothing. Nothing in
# this crate can check the names against the readers — D40 forbids depending on
# the crate that holds them — so the end-to-end tests are the check, and this is
# the case that says so.
case_ "a pdf page reaches the pdf arm by the name its header carries" \
  crates/mnema-ingest/src/lib.rs \
  's~                READER_PDF => PageContext::Fixed~                "pdf-by-another-name" => PageContext::Fixed~' \
  '                "pdf-by-another-name" => PageContext::Fixed' \
  mnema-ingest 'a_pdf_chunk_cites_its_page_not_nothing' --test slice

# C2. And the number on it is the page's own. A constant passes a one-page
# fixture — which is why the fixture has two pages, numbered 7 and 9 — and reads
# as a coordinate for ever after: every chunk of a four-hundred-page report
# citing page 1.
case_ "the pdf coordinate is the page's own number, not a constant" \
  crates/mnema-ingest/src/lib.rs \
  's~                    number: page\.page_no,~                    number: 1,~' \
  '                    number: 1,' \
  mnema-ingest 'a_pdf_chunk_cites_its_page_not_nothing' --test slice

# C3. Three readers share the section arm, and a list that loses one of them
# loses it silently: the other two keep working, so any fixture over a single
# format stays green. The test iterates all three for exactly this.
case_ "every one of the three section readers is named in the arm" \
  crates/mnema-ingest/src/lib.rs \
  's~                READER_HTML \| READER_DOCX \| READER_EPUB =>~                READER_HTML | READER_DOCX =>~' \
  '                READER_HTML | READER_DOCX =>' \
  mnema-ingest 'html_docx_and_epub_cite_the_section_their_page_names' --test slice

# C4. The section is the page's title, not an empty string. `Coordinate::Section
# { title: "" }` renders as nothing at all — indistinguishable, on screen, from
# the `Coordinate::None` this whole task exists to stop — while still being a
# coordinate as far as any "is it non-empty" assertion is concerned.
case_ "the section coordinate carries the title its page names" \
  crates/mnema-ingest/src/lib.rs \
  's~                        title: page\.section_title\.clone\(\)\.unwrap_or_default\(\),~                        title: String::new(),~' \
  '                        title: String::new(),' \
  mnema-ingest 'html_docx_and_epub_cite_the_section_their_page_names' --test slice

# ------------------------------------------------------------------- the sheet

# C5. The temptation this variant exists to resist, in the form someone would
# actually reach for: a sheet has a name, `Section` holds a name, and `Fixed`
# is already there. It compiles, it renders, and every chunk of the workbook
# cites the sheet instead of the rows.
case_ "a sheet gets Rows, not a fixed coordinate naming the sheet" \
  crates/mnema-ingest/src/lib.rs \
  's~                READER_XLSX => PageContext::Rows \{\n                    sheet: page\.section_title\.clone\(\)\.unwrap_or_default\(\),\n                \},~                READER_XLSX => PageContext::Fixed(Coordinate::Section {\n                    title: page.section_title.clone().unwrap_or_default(),\n                }),~' \
  '                READER_XLSX => PageContext::Fixed(Coordinate::Section {' \
  mnema-ingest 'an_xlsx_chunk_cites_the_rows_it_covers_not_the_whole_sheet' --test slice

# C6. The defect itself, at the level that computes it: a range taken over every
# block of the page rather than the blocks the chunk covers. That is precisely
# what `Fixed` would have given a sheet, reached the other way — and it is
# non-empty, correctly named and correctly ordered, so an assertion that the
# coordinate exists, or that it is a `SheetRows`, or that its ends are sane, is
# satisfied by it. Only "which rows" catches this.
case_ "a chunk's row range covers the chunk (mnema-chunk)" \
  crates/mnema-chunk/src/lib.rs \
  's~        PageContext::Rows \{ sheet \} => match line_range\(segs, views\) \{~        PageContext::Rows { sheet } => match (Coordinate::Line {\n            start: views.iter().filter_map(|v| v.line_start).min().unwrap_or(0),\n            end: views.iter().filter_map(|v| v.line_end).max().unwrap_or(0),\n        }) {~' \
  '        PageContext::Rows { sheet } => match (Coordinate::Line {' \
  mnema-chunk 'a_sheet_range_narrows_to_the_chunk_and_not_to_the_sheet' --test invariants

# C7. The same break, seen from the database. Not a duplicate of C6: it is the
# whole path — reader name, page, blocks, chunker, the JSON in `chunk.coordinate`
# and the citation read back out of it — and it is the level at which a person
# would meet the wrong sentence. C6 alone would leave "does any of this survive
# `insert_chunk`" unanswered.
case_ "a chunk's row range covers the chunk (through the index)" \
  crates/mnema-chunk/src/lib.rs \
  's~        PageContext::Rows \{ sheet \} => match line_range\(segs, views\) \{~        PageContext::Rows { sheet } => match (Coordinate::Line {\n            start: views.iter().filter_map(|v| v.line_start).min().unwrap_or(0),\n            end: views.iter().filter_map(|v| v.line_end).max().unwrap_or(0),\n        }) {~' \
  '        PageContext::Rows { sheet } => match (Coordinate::Line {' \
  mnema-ingest 'an_xlsx_chunk_cites_the_rows_it_covers_not_the_whole_sheet' --test slice

# C8. The sheet's name comes from the page. An empty one renders as "аркуш ,
# рядки 14–19" — the rows are right, the citation is nonsense, and every
# assertion about the range still passes.
case_ "the sheet's name comes from the page that named it" \
  crates/mnema-ingest/src/lib.rs \
  's~                    sheet: page\.section_title\.clone\(\)\.unwrap_or_default\(\),~                    sheet: String::new(),~' \
  '                    sheet: String::new(),' \
  mnema-ingest 'an_xlsx_chunk_cites_the_rows_it_covers_not_the_whole_sheet' --test slice

# C9. The passthrough for a block with no rows. `line_range` answers
# `Coordinate::None` there, and dressing that up as a sheet range is exactly the
# invention `Coordinate::None` exists to refuse — rows 0–0 of a sheet, which no
# spreadsheet has. The xlsx reader is obliged to give every block its rows
# (spec §2.6); this is what happens on the day one does not.
case_ "a block with no rows leaves the chunk uncoordinated" \
  crates/mnema-chunk/src/lib.rs \
  's~            other => other,~            _ => Coordinate::SheetRows {\n                sheet: sheet.clone(),\n                start: 0,\n                end: 0,\n            },~' \
  '            _ => Coordinate::SheetRows {' \
  mnema-chunk 'a_sheet_block_without_rows_leaves_the_chunk_uncoordinated' --test invariants

# ---------------------------------------------------------- the other direction

# C10. Every case above breaks the new arms. This one breaks the old default,
# which is the direction a table of new formats is most likely to damage without
# anyone noticing: txt and md have had line coordinates since the first task,
# and a `_ =>` arm that stops giving them costs every document already in the
# index its citation. No test about pdf, html or xlsx can see it.
case_ "the readers that already had line coordinates keep them" \
  crates/mnema-ingest/src/lib.rs \
  's~                _ => PageContext::Lines,~                _ => PageContext::Fixed(Coordinate::None),~' \
  '                _ => PageContext::Fixed(Coordinate::None),' \
  mnema-ingest 'line_numbers_survive_the_round_trip' --test slice

# What has no case here, named rather than left to be found: `PageOf.page_no`
# and `PageOf.section_title` still feed `insert_page`, and the coordinate is now
# built from the same two values. Breaking either one breaks both at once, so no
# mutation can separate "the page row is right" from "the coordinate is right".
# The page row is task 1's ground and is asserted by
# `a_markdown_file_reaches_the_database_as_several_pages`.

# C11. And the untitled page keeps the two states apart. Collapsing it to
# `Coordinate::None` is the tidy-looking change — the two render identically, so
# nothing on screen would ever show it — and it makes "the html reader forgot to
# name a section" indistinguishable in the database from "this format has no
# coordinate to give". The titled cases stay green under it, which is why the
# untitled one has a test of its own.
case_ "an untitled section page is an empty section, not no coordinate" \
  crates/mnema-ingest/src/lib.rs \
  's~                    PageContext::Fixed\(Coordinate::Section \{\n                        title: page\.section_title\.clone\(\)\.unwrap_or_default\(\),\n                    \}\)~                    PageContext::Fixed(match \&page.section_title {\n                        Some(title) => Coordinate::Section {\n                            title: title.clone(),\n                        },\n                        None => Coordinate::None,\n                    })~' \
  '                    PageContext::Fixed(match &page.section_title {' \
  mnema-ingest 'a_page_that_names_no_section_carries_an_empty_one_rather_than_none' --test slice

# C12. And the constants are not free-floating: their values are what a header
# actually carries, and this is the case that says so. `mnema-ingest` cannot ask
# `mnema-extract` what its readers call themselves (D40), so the closest thing
# to a check is that the arm and the wire agree on a literal — the stand-in
# states "pdf", `pages_of` matches READER_PDF, and a constant that drifts from
# the string on the wire falls to the default with every PDF page uncoordinated.
#
# What no case here can reach, named rather than left to be found: whether the
# pdf reader, when it exists, uses this symbol at all. Both sides of every
# assertion in this file are written in this repository by the same commit; the
# real counterpart is the literal in `worker.rs`, and it is a later task's.
case_ "the constant the pdf arm matches is the string a header carries" \
  crates/mnema-core/src/manifest.rs \
  's~pub const READER_PDF: &str = "pdf";~pub const READER_PDF: \&str = "pdf-2";~' \
  'pub const READER_PDF: &str = "pdf-2";' \
  mnema-ingest 'a_pdf_chunk_cites_its_page_not_nothing' --test slice

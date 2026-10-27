# Mutation cases for the broken-UTF-16 page and the reader-named refusal. Run
# with:
#
#   scripts/mutation-check.sh scripts/mutations/pdf-broken-utf16.sh
#
# **This is a registry, not a new set of findings.** The reader's box and its
# per-character fallback, and the parent's rule about a reader that refuses bytes
# it had already indexed, were each backed by mutants applied by hand and written
# into a table in a report. A table in a report is not a guard: after a later
# refactor nothing re-runs it, and the table says nothing about the day the code
# moves out from under it. Each case below is one row of those tables, and its
# subject says what is silent when the line is wrong. Targets were verified by
# running them rather than copied: one row named a test a later commit had
# renamed, and one named a test that cannot see its own mutant (both fixed by
# running, not by reading).
#
# What is silent when it is wrong is two different things, and this file is
# organised around the pair:
#
#   * **the bound** — text a person can read on the page and the index does not
#     hold (a page journalled as a scan, a line ending mid-word), or text nobody
#     can see and the index holds (a press slug, a cover spread above the
#     MediaBox);
#   * **the refusal** — a document deleted from the index although its bytes
#     never moved, because a version was compared with the wrong side or read
#     when there was none to read.
#
# ⚠️ **There is deliberately no case for "the fallback always on", and it is not
# an omission.** Measured: after the port, `chars_within` computes exactly what
# `inside_rect` computes, so running it unconditionally changes no stored text
# and only costs a walk. It is an *equivalent* mutant — there is no difference
# left for a test to see, which is the point of the port — and a case for it
# would be a case for a difference that does not exist. What keeps it equivalent
# for good is
# `pdfium_probe::tests::the_fallback_computes_what_the_bounded_read_computes_on_every_decodable_page`,
# which asserts that equality on every page of every fixture whose bounded read
# decodes.
#
# ⚠️ **The space branch of `chars_within` has no case either, and this one is a
# measurement rather than a decision.** No fixture draws a space whose own box
# falls outside the bound after a character inside it, so deleting that branch
# leaves the whole `mnema-extract` crate green (measured: 41 + 34 + 38 + 26 + 8 +
# 17 + 23 + 18 + 4 + 16 + 28 + 16 + 5 passed, 0 failed). That rule rests on the
# port alone until a fixture pins it.
#
# ⚠️ **Two rows of the older tables have no case because their code does not
# exist any more**: "look the version up by extension" and "use the pdf reader's
# version". The refusal now carries its own reader on the wire, and `displaces`
# reads the pair off the frame rather than looking anything up in the manifest —
# so the mutant to make is the one this file calls "the refusal's reader name
# ignored", not a lookup that is no longer written.

# --------------------------------------------------------------- the bound
#
# Three ways to get the box wrong, and the three fixtures that tell them apart.
# All three are the same function rewritten, so each case replaces its whole
# body — a pattern anchored on one line of it would not survive its neighbours.

# B1. `page_size()` — width×height from `(0,0)`, the rectangle `all()` used.
# Measured on a library of books (D187): a document with its box moved off the
# origin lost its text page after page, each one journalled as a scan.
case_ "reader: the bound is the page's box, not the rectangle from the origin" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's#fn page_bound\(page: &PdfPage\) -> PdfRect \{\n    page\.boundaries\(\)\n        \.bounding\(\)\n        \.map\(\|box_\| box_\.bounds\)\n.*?\n        \.unwrap_or_else\(\|_\| page\.page_size\(\)\)\n\}#fn page_bound(page: &PdfPage) -> PdfRect {\n    page.page_size()\n}#s' \
  'fn page_bound(page: &PdfPage) -> PdfRect {
    page.page_size()
}' \
  mnema-extract 'a_page_whose_box_starts_away_from_the_origin_is_read_whole' --test pdf

# B2. The MediaBox clip removed: the raw `/CropBox` wins whenever pdfium can read
# one. Page 3 of `page-boxes.pdf` declares a CropBox wider than its MediaBox on
# every side and a cover slug in the part a viewer clips, so this indexes text
# nobody can see. It also takes an empty `/CropBox` at face value — page 2's
# `[0 0 0 0]` — finds nothing inside it, and journals a page of prose as a scan.
case_ "reader: the bound is the raw CropBox when the page has one" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's#fn page_bound\(page: &PdfPage\) -> PdfRect \{\n    page\.boundaries\(\)\n        \.bounding\(\)\n        \.map\(\|box_\| box_\.bounds\)\n.*?\n        \.unwrap_or_else\(\|_\| page\.page_size\(\)\)\n\}#fn page_bound(page: &PdfPage) -> PdfRect {\n    let boundaries = page.boundaries();\n    boundaries\n        .crop()\n        .or_else(|_| boundaries.bounding())\n        .map(|box_| box_.bounds)\n        .unwrap_or_else(|_| page.page_size())\n}#s' \
  '        .crop()
        .or_else(|_| boundaries.bounding())' \
  mnema-extract 'a_page_with_a_degenerate_crop_box_is_read' --test pdf

# B3. The reader's round-1 chain put back: `crop()` then `media()` then
# `page_size()`. Neither of the first two resolves a box inherited from
# `/Pages`, so `page-boxes.pdf` page 1 — whose boxes are only on the node —
# falls to the rectangle from the origin and is journalled as a scan.
case_ "reader: the bound resolves the page tree before falling back" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's#fn page_bound\(page: &PdfPage\) -> PdfRect \{\n    page\.boundaries\(\)\n        \.bounding\(\)\n        \.map\(\|box_\| box_\.bounds\)\n.*?\n        \.unwrap_or_else\(\|_\| page\.page_size\(\)\)\n\}#fn page_bound(page: &PdfPage) -> PdfRect {\n    let boundaries = page.boundaries();\n    boundaries\n        .crop()\n        .map(|box_| box_.bounds)\n        .or_else(|_| boundaries.media().map(|box_| box_.bounds))\n        .unwrap_or_else(|_| page.page_size())\n}#s' \
  '        .or_else(|_| boundaries.media().map(|box_| box_.bounds))' \
  mnema-extract 'a_page_whose_boxes_are_inherited_is_read_whole' --test pdf

# ------------------------------------------------------------ the fallback

# F1. Every character kept, whatever the box says. The page's own press slug is
# above its CropBox and comes back into the stored text: the fallback exists to
# rescue the characters *inside* the page, not to read the plate the book was
# printed from.
case_ "reader: the fallback drops the characters outside the box" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's#        b\.width\(\)\.value != 0\.0 && b\.height\(\)\.value != 0\.0 && b\.does_overlap\(&bound\)#        b.width().value != 0.0 && b.height().value != 0.0#' \
  '        b.width().value != 0.0 && b.height().value != 0.0
    })' \
  mnema-extract 'a_broken_glyph_between_two_words_leaves_one_space' --test pdf

# F2. The fallback removed: a page whose bounded decode fails is dropped, and
# one wrong `/ToUnicode` glyph costs the whole page — the defect this reader was
# written to fix.
case_ "reader: a page whose bounded read fails is rebuilt from its characters" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's#    if !bounded\.is_empty\(\) \{\n        return Ok\(bounded\);\n    \}\n\n    Ok\(text_from_code_points\(chars_within\(&text, bound\)\)\)#    Ok(bounded)#' \
  '    let bounded = text.inside_rect(bound);
    Ok(bounded)' \
  mnema-extract 'a_glyph_that_is_not_a_character_loses_its_word_and_not_its_page' --test pdf

# F3. Containment instead of intersection. pdfium's bounded read is
# `IsRectIntersect(rect, char_info.char_box())`, so a glyph crossing the box's
# edge is on the page; a containment filter drops it, and the rescued page loses
# its first letter (`A damaged mapping` reads `damaged mapping`).
case_ "reader: the fallback keeps a glyph that crosses the box's edge" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's#        b\.width\(\)\.value != 0\.0 && b\.height\(\)\.value != 0\.0 && b\.does_overlap\(&bound\)#        b.width().value != 0.0 && b.height().value != 0.0 && b.is_inside(&bound)#' \
  '&& b.is_inside(&bound)' \
  mnema-extract 'a_broken_glyph_between_two_words_leaves_one_space' --test pdf

# F4. The code point that is not a character dropped rather than replaced. A lone
# surrogate is not a `char`; skipping it fuses the words on either side
# (`wordfrom`), and D186's run-to-space rule never gets a character to decide on.
case_ "reader: a code point that is not a character becomes U+FFFD" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's#        \.map\(\|value\| char::from_u32\(value\)\.unwrap_or\('"'"'\\u\{FFFD\}'"'"'\)\)\n        \.collect\(\)#        .filter_map(char::from_u32)\n        .collect()#' \
  '        .filter_map(char::from_u32)
        .collect()' \
  mnema-extract 'a_broken_glyph_between_two_words_leaves_one_space' --test pdf

# F5. The line break not rebuilt. pdfium writes a break at the end of a line; on
# a line cut by the box that break is outside it, so `inside_rect` synthesises
# it from the y position. A fallback that only filters characters stores
# `…that weand the depot…`: two words lost from search, and the page reads as one
# run-on line.
case_ "reader: the fallback writes the break the box cut away" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's#            if y != pos_y && !after_text && line_feed_due \{\n                pos_y = y;\n                if !values\.is_empty\(\) \{\n                    values\.extend\(\[u32::from\(b\x27\\r\x27\), u32::from\(b\x27\\n\x27\)\]\);\n                \}\n            \}#            if y != pos_y && !after_text && line_feed_due {\n                pos_y = y;\n            }#' \
  '            if y != pos_y && !after_text && line_feed_due {
                pos_y = y;
            }' \
  mnema-extract 'a_line_cut_by_the_box_keeps_its_break_when_the_page_is_rebuilt' --test pdf

# F6. The diagnostic reading a different page from the reader. `--probe-pdfium`
# answers a packaging question, and a probe that says every page of a good
# document is a scan is two answers to one question — the reader's and the one a
# person sees first.
case_ "probe: the diagnostic reads the page the reader reads" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's#        out\.push\(page_text\(&page\)\.map_err\(\|e\| Error::Pdfium\(e\.to_string\(\)\)\)\?\);#        out.push(page.text().map(|text| text.all()).map_err(|e| Error::Pdfium(e.to_string()))?);#' \
  'out.push(page.text().map(|text| text.all())' \
  mnema-extract 'the_probe_reads_the_pages_the_reader_reads' --test pdf

# -------------------------------------------------------------- displaces
#
# One rule reads a refusal's reader and version, and each half of that pair is a
# separate way to lose a document or to keep a stale one. `NoTextLayer` on the
# same bytes is the only state that reaches the new condition at all, so every
# case here names a test built to stand in it.

# D1. The whole second condition gone: a reader that decided these bytes hold no
# text at today's version keeps the document an older version made of them. That
# is D186's own case — the release that stopped indexing pages of U+FFFD would
# never take its predecessor's reading away.
case_ "displaces: a newer version of the same reader takes the document" \
  crates/mnema-ingest/src/lib.rs \
  's#        SkipRule::NoTextLayer => \{\n            content\.is_none_or\(\|sha\| sha != recorded\.document_id\)\n                \|\| refused_by\.is_some_and\(\|\(reader, version\)\| \{\n                    reader == recorded\.reader\n                        && recorded\.reader_version >= 1\n                        && recorded\.reader_version < i64::from\(version\)\n                \}\)\n        \}#        SkipRule::NoTextLayer => content.is_none_or(|sha| sha != recorded.document_id),#' \
  '        SkipRule::NoTextLayer => content.is_none_or(|sha| sha != recorded.document_id),' \
  mnema-ingest 'a_newer_reader_deciding_no_text_layer_takes_the_document_away' --test slice

# D2. `<` becomes `<=`: at today's version the refusal and the row are the same
# reading, and D51 says a machine, a threshold or a library that failed today is
# not evidence about the file.
case_ "displaces: an equal version keeps the document" \
  crates/mnema-ingest/src/lib.rs \
  's#                        && recorded\.reader_version < i64::from\(version\)#                        && recorded.reader_version <= i64::from(version)#' \
  'recorded.reader_version <= i64::from(version)' \
  mnema-ingest 'the_same_reader_version_deciding_no_text_layer_keeps_the_document' --test slice

# D3. The refusal's reader name ignored. A refusal by the pdf reader about bytes
# the text reader indexed is a verdict about a page that reader never read — and
# the version comparison alone cannot tell the two apart (review I2).
case_ "displaces: a refusal by another reader keeps the document" \
  crates/mnema-ingest/src/lib.rs \
  's#                    reader == recorded\.reader\n#                    reader.len() > 0\n#' \
  '                    reader.len() > 0' \
  mnema-ingest 'a_refusal_from_another_reader_keeps_the_document' --test slice

# D4. A refusal that names nobody displaces. An old worker's frame carries no
# reader, and an unknown is not a newer release: deleting on it loses a document
# per file from a folder of scans walked by a build one release behind.
case_ "displaces: a refusal that names no reader keeps the document" \
  crates/mnema-ingest/src/lib.rs \
  's#                \|\| refused_by\.is_some_and\(\|\(reader, version\)\| \{#                || refused_by.is_none_or(|(reader, version)| {#' \
  'refused_by.is_none_or(|(reader, version)|' \
  mnema-ingest 'a_refusal_that_names_no_reader_keeps_the_document' --test slice

# D5. The M4 guard dropped: a recorded version of 0 is not a release this product
# shipped (its headers are 1 or more), and a foreign sidecar can write it. Garbage
# is not "older than everything" — it keeps.
case_ "displaces: a garbage recorded version keeps the document" \
  crates/mnema-ingest/src/lib.rs \
  's#                        && recorded\.reader_version >= 1\n##' \
  '                    reader == recorded.reader
                        && recorded.reader_version < i64::from(version)' \
  mnema-ingest 'a_garbage_recorded_version_keeps_the_document' --test slice

# D6. The second condition where it must not be: on `malformed` too. A newer
# reader that cannot read these bytes at all is not a reader that decided they
# hold no text — the old text is still the better reading of the file, and
# `Malformed`'s arm exists to avoid exactly this loss.
case_ "displaces: only the no-text-layer rule reads the refusing reader" \
  crates/mnema-ingest/src/lib.rs \
  's#        SkipRule::Malformed => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),#        SkipRule::Malformed => content.is_none_or(|sha| sha != recorded.document_id)\n            || refused_by.is_some_and(|(reader, version)| {\n                reader == recorded.reader\n                    && recorded.reader_version >= 1\n                    && recorded.reader_version < i64::from(version)\n            }),#' \
  'SkipRule::Malformed => content.is_none_or(|sha| sha != recorded.document_id)
            || refused_by' \
  mnema-ingest 'a_newer_reader_that_finds_the_bytes_damaged_keeps_the_document' --test slice

# --------------------------------------------------------- the journal's arm
#
# The cheap arm answers a remembered refusal without spending a worker, and it
# now has to know whether the reader that wrote the row has moved on.

# J1. The fall-through gone: a scan skipped by the pdf reader at version 1 is
# answered from the journal for ever, and the pages a newer reader can read are
# never read.
case_ "journal: a row written by an older reader asks the worker again" \
  crates/mnema-ingest/src/lib.rs \
  's#fn the_reader_has_moved_on\(skip: &SkipEntry, manifest: &Manifest\) -> bool \{.*?\n\}#fn the_reader_has_moved_on(skip: &SkipEntry, manifest: &Manifest) -> bool {\n    let _ = (skip, manifest);\n    false\n}#s' \
  '    let _ = (skip, manifest);
    false' \
  mnema-ingest 'a_reader_that_moved_on_re_reads_what_its_predecessor_displaced' --test slice

# J2. `>` becomes `>=`: at today's own version the row and the manifest agree, and
# the arm has to answer instead of paying a worker on every walk for every scan a
# library holds.
case_ "journal: a row at today's version answers without a worker" \
  crates/mnema-ingest/src/lib.rs \
  's#\.is_some_and\(\|today\| i64::from\(today\) > version\)#.is_some_and(|today| i64::from(today) >= version)#' \
  'i64::from(today) >= version' \
  mnema-ingest 'a_scaled_file_skipped_by_an_older_reader_is_read_again_when_the_reader_moves' --test slice

# J3. The row written without the reader. The journal's whole use of the pair is
# this column: a row that names nobody is never re-read on version grounds, so the
# displacement stands for the life of the index.
case_ "journal: the row remembers the reader that refused the file" \
  crates/mnema-index/src/journal.rs \
  's#                refused_by\.map\(\|\(reader, _\)\| reader\),\n#                None::<&str>,\n#' \
  '                None::<&str>,
                refused_by.map(|(_, version)| i64::from(version))' \
  mnema-ingest 'a_newer_reader_deciding_no_text_layer_takes_the_document_away' --test slice

# --------------------------------------------------------------- the manifest
#
# The version the rule compares travels across D40 as the worker's `--manifest`
# output, and nothing links the two sides. These two cases are the table leaving
# this crate.

# M1. `pdf` dropped from the table, with the array's length so it still compiles.
# The parent's lookup then answers `None` for every pdf row, which means "keep" —
# and the journal's fall-through stops re-reading what a newer pdf reader can
# now read.
case_ "manifest: every reader this build names has a row in the version table" \
  crates/mnema-extract/src/manifest.rs \
  's#    \(READER_PDF, PDF_READER_VERSION\),\n##; s#const READERS: \[\(&str, u32\); 7\] = \[#const READERS: [(&str, u32); 6] = [#' \
  'const READERS: [(&str, u32); 6] = [' \
  mnema-extract 'manifest::tests::every_reader_this_build_ships_has_a_version' --lib

# M2. The table never reaching the manifest this build prints: the lookup is
# local, every `version_of` answers `None`, and the parent keeps every document —
# the side that loses nothing, which is exactly why nothing else notices.
case_ "manifest: the version table reaches the manifest the worker prints" \
  crates/mnema-extract/src/manifest.rs \
  's#        by_reader: READERS\n            \.iter\(\)\n            \.map\(\|\(reader, version\)\| \(\(\*reader\)\.to_string\(\), \*version\)\)\n            \.collect\(\),#        by_reader: std::collections::BTreeMap::new(),#' \
  'by_reader: std::collections::BTreeMap::new(),' \
  mnema-extract 'manifest::tests::the_table_is_in_the_manifest_this_build_prints' --lib

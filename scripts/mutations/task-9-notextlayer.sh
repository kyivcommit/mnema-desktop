# Mutation cases for Task 9: what a scanned page costs the index. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-9-notextlayer.sh
#
# NOT `task-9.sh`, which is taken: the previous cycle (G7.4, packaging) numbered
# its own tasks from one, and its `task-9.sh` is a working case file. Task 13 of
# this cycle will meet the same collision. `task-8-pdf.sh` made the same choice
# one task earlier and says so at its top.
#
# The subject is one rule and one number, and every way of getting either wrong
# is silent:
#
#   * a file-level `no_text_layer` that displaces unconditionally loses a
#     document per file over a threshold nobody moved the bytes for;
#   * a page number dropped anywhere between pdfium and the journal leaves the
#     window answering "why is this not in my index?" unable to say which page
#     of a contract the scanner missed — the count it had before said only that
#     one was gone;
#   * a page number that reaches the journal as a *file* verdict makes the
#     second cheap arm answer for the whole document, and the file stops being
#     read at all;
#   * a row nothing ever removes says a page is missing for the life of the
#     index, about a document the index no longer holds.
#
# What the compiler already covers, so no case here duplicates it: `displaces`
# is an exhaustive `match` over `SkipRule`, so a rule added without a decision
# does not compile; and `Frame::Summary.skipped_pages` is a required field, so a
# producer that omits it does not parse.
#
# `task-8-pdf.sh` C16 covers the reader's own end of the number — the worker
# sending `doc.skipped` rather than nothing — and is not repeated here.

# --------------------------------------------- what a scan costs the document

# N1. The arm this task changed, put back. `no_text_layer` is the least stable
# verdict of the five that displace: `TEXT_LAYER_MIN_CHARS` is a product
# decision and pdfium is a vendored library, so a folder of scans walked once by
# a build that found text on the page and once by a build that did not is a
# document lost per file, with the bytes never having moved. Nothing goes red
# anywhere else — the journal even records why each one went.
case_ "ingest: a scan refuses on the bytes it was refused on" \
  crates/mnema-ingest/src/lib.rs \
  's{        SkipRule::NoTextLayer => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),}{        SkipRule::NoTextLayer => true,}' \
  '        SkipRule::NoTextLayer => true,' \
  mnema-ingest 'a_scanned_file_keeps_its_document_when_the_bytes_did_not_move' --test slice

# N2. The other direction of the same arm, and it needs its own case: a rule
# that never displaces is the defect the whole digest exists to prevent, one
# rule further along — a file replaced by a photograph of itself, with the index
# still answering under its name with prose it no longer contains.
case_ "ingest: a scan over different bytes still takes the old document away" \
  crates/mnema-ingest/src/lib.rs \
  's{        SkipRule::NoTextLayer => content\.is_none_or\(\|sha\| sha != recorded\.document_id\),}{        SkipRule::NoTextLayer => false,}' \
  '        SkipRule::NoTextLayer => false,' \
  mnema-ingest 'a_scanned_file_keeps_its_document_when_the_bytes_did_not_move' --test slice

# ------------------------------------------------ the number crossing the wire

# N3. The pool dropping the numbers while building the document. Everything the
# worker's own suite asserts is upstream of this line, so `worker_cli` stays
# green and the parent simply never learns that a page went missing. This was
# untested before task 9: nothing downstream of the pool read the field.
case_ "pool: the summary's page numbers reach the document" \
  crates/mnema-pool/src/lib.rs \
  's{                    pages,\n                    skipped_pages,\n                    text_source,}{                    pages,\n                    skipped_pages: Vec::new(),\n                    text_source,}' \
  'skipped_pages: Vec::new(),
                    text_source,' \
  mnema-ingest 'a_skipped_page_is_journalled_by_number_and_a_later_pass_takes_it_back' --test slice

# N4. The check that a page cannot be both read and skipped, removed. The counts
# still agree, so the check above it passes, and the summary is the only frame
# that ever carries both lists — after it nothing sees them together again. The
# cost is a journal row saying page 1 has no text layer while a search cites
# page 1.
case_ "pool: a page in both lists is a worker that does not speak this protocol" \
  crates/mnema-pool/src/lib.rs \
  's{                if let Some\(both\) = skipped_pages\n                    \.iter\(\)\n                    \.find\(\|no\| pages\.iter\(\)\.any\(\|page\| page\.page_no == \*\*no\)\)\n                \{\n                    return Err\(protocol\(\n                        &line,\n                        &format!\("page \{both\} arrived and was reported skipped"\),\n                    \)\);\n                \}\n}{}' \
  'return Ok(Answer::Document(Document {' \
  mnema-pool 'a_page_that_arrived_and_was_reported_skipped_stops_the_job' --test supervision

# N5. The same check made too strict — any skipped page at all stops the job.
# It is the outcome `SkipRule::NoTextLayer`'s missing parse arm had before task
# 8: a walk over a folder with one scanned page in it stops on that file, and
# `PoolError::Protocol` accuses the worker binary of being from another release.
case_ "pool: an ordinary skipped page is not a protocol failure" \
  crates/mnema-pool/src/lib.rs \
  's{                if let Some\(both\) = skipped_pages\n                    \.iter\(\)\n                    \.find\(\|no\| pages\.iter\(\)\.any\(\|page\| page\.page_no == \*\*no\)\)\n                \{}{                if let Some(both) = skipped_pages.first() \{}' \
  'if let Some(both) = skipped_pages.first() {' \
  mnema-pool 'a_page_that_arrived_and_was_reported_skipped_stops_the_job' --test supervision

# ------------------------------------------------------- the row, and its life

# N6. The row written against the file instead of against the page. It parses,
# it is one row, and the window even shows a sensible sentence — but
# `Db::skip_entry` reads `page_no IS NULL`, so from the next walk on this is the
# whole file's verdict: the second cheap arm answers from it without asking a
# worker, and a contract missing one page becomes a contract that is not in the
# index at all.
case_ "ingest: a skipped page is journalled against the page, not the file" \
  crates/mnema-ingest/src/lib.rs \
  's{            Some\(i64::from\(\*page_no\)\),}{            None,}' \
  '            relative,
            None,
            // Written here rather than carried on the wire' \
  mnema-ingest 'a_skipped_page_is_journalled_by_number_and_a_later_pass_takes_it_back' --test slice

# N7. The number itself, replaced by a plausible constant. `1` is a page that
# *was* read: the row then reports a page the index holds and cites as missing
# from it, which is the wrong answer this whole task exists to be able to give
# correctly.
case_ "ingest: the row names the page the reader skipped" \
  crates/mnema-ingest/src/lib.rs \
  's{            Some\(i64::from\(\*page_no\)\),}{            Some(1),}' \
  '            Some(1),' \
  mnema-ingest 'a_skipped_page_is_journalled_by_number_and_a_later_pass_takes_it_back' --test slice

# N8. The rule the row is filed under. `unsupported` promises a reader that is
# coming; this page was read by the reader that exists and is a photograph.
# Nothing goes red on its own: both rules are about content and both displace.
case_ "ingest: a skipped page is filed under its own rule" \
  crates/mnema-ingest/src/lib.rs \
  's{            SkipRule::NoTextLayer,\n            Some\(disk\),}{            SkipRule::Unsupported,\n            Some(disk),}' \
  'SkipRule::Unsupported,
            Some(disk),' \
  mnema-ingest 'a_skipped_page_is_journalled_by_number_and_a_later_pass_takes_it_back' --test slice

# N9. The removal in `repoint`, taken out. This is the immortal row: nothing
# else in the tree deletes a per-page row for a path a walk keeps finding —
# `forget_skip` excludes them by its own clause, `forget_skips_not_in` fires
# only for paths the walk did not see. The pass that reddens it is the one that
# answers `AlreadyIndexed` and leaves before any content is written, which is
# where a row is easiest to leave behind.
case_ "ingest: a page that stopped being missing stops being listed" \
  crates/mnema-ingest/src/lib.rs \
  's{    db\.forget_page_skips\(root_id, relative\)\?;\n    for page_no in &document\.skipped_pages \{}{    for page_no in &document.skipped_pages \{}' \
  'db.forget_skip(root_id, relative)?;' \
  mnema-ingest 'a_skipped_page_is_journalled_by_number_and_a_later_pass_takes_it_back' --test slice

# N10. The same removal, scoped to the watched root instead of to the path. One
# file being read empties every other file's page rows, on every walk, and the
# file being read looks perfectly correct — which is why the fixture holds two
# documents each missing a different page rather than one.
case_ "ingest: a path's rows are removed, not the whole root's" \
  crates/mnema-index/src/journal.rs \
  's{              WHERE watched_root_id = \?1 AND relative_path = \?2 AND page_no IS NOT NULL",}{              WHERE watched_root_id = ?1 AND (relative_path = ?2 OR 1 = 1) AND page_no IS NOT NULL",}' \
  'AND (relative_path = ?2 OR 1 = 1) AND page_no IS NOT NULL' \
  mnema-ingest 'a_skipped_page_is_journalled_by_number_and_a_later_pass_takes_it_back' --test slice

# N11. The same removal aimed at the file's verdict instead of the pages'. In
# `repoint` it is invisible — `forget_skip` has already removed that row — so
# what is left is the page rows, standing for ever.
case_ "ingest: the page removal takes the page rows" \
  crates/mnema-index/src/journal.rs \
  's{              WHERE watched_root_id = \?1 AND relative_path = \?2 AND page_no IS NOT NULL",}{              WHERE watched_root_id = ?1 AND relative_path = ?2 AND page_no IS NULL",}' \
  'AND relative_path = ?2 AND page_no IS NULL",
            params![root_id, relative_path],
        )?;
        Ok(())
    }

    /// Removes skip rows under `root_id`' \
  mnema-ingest 'a_skipped_page_is_journalled_by_number_and_a_later_pass_takes_it_back' --test slice

# ------------------------------- the other way a row outlives its document

# N12. A refusal writes no `path` row, so `repoint` never runs for this path
# again and nothing above reaches these rows. Without this call a file replaced
# by something no reader takes keeps "page 7 has no text layer" for the life of
# the index — about a document the index does not hold at all.
case_ "ingest: a refusal that removes the document removes its page rows" \
  crates/mnema-ingest/src/lib.rs \
  's{            db\.forget_page_skips\(root_id, relative\)\?;\n        \}\n        Ok\(\(\)\)}{        \}\n        Ok(())}' \
  '            // document the index does not hold.
        }
        Ok(())' \
  mnema-ingest 'a_refusal_that_takes_the_document_away_takes_its_page_rows_with_it' --test slice

# N13. The other direction, and it is what keeps the removal conditional. A
# worker that died says nothing about the file: the document stays, so the
# account of what is missing from it has to stay too. Made unconditional here by
# running it before the `displaces` question is asked.
case_ "ingest: an environmental refusal keeps the rows with the document" \
  crates/mnema-ingest/src/lib.rs \
  's{        db\.record_skip\(root_id, relative, None, reason, rule, on_disk\)\?;}{        db.forget_page_skips(root_id, relative)?;\n        db.record_skip(root_id, relative, None, reason, rule, on_disk)?;}' \
  '        db.forget_page_skips(root_id, relative)?;
        db.record_skip(root_id, relative, None, reason, rule, on_disk)?;' \
  mnema-ingest 'a_refusal_that_takes_the_document_away_takes_its_page_rows_with_it' --test slice

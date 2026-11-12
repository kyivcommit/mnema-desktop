# Mutation cases for OCR of a scanned PDF (D189), the part that runs everywhere.
# Run with:
#
#   scripts/mutation-check.sh scripts/mutations/scan-ocr.sh
#
# **Why this file holds no Vision case.** The `mutations` job runs on
# `ubuntu-24.04` only, and every test that needs Apple Vision is
# `#[cfg(target_os = "macos")]`: off macOS `--exact` selects nothing, the baseline
# reads `0 passed`, and the whole FILE exits 1 (`pr8-exclusions-macos.sh` has the
# measurement). The one case that needs Vision lives in `scan-ocr-macos.sh`, which
# nothing runs automatically. Everything below names a test that runs on the Linux
# leg too — the engine is made "unavailable" through its seam, or the pool is
# driven by a stand-in worker — so the CI leg proves it kills, not only that it
# applies.
#
# What is silent when each line is wrong:
#
#   * **the refusal's kind** — a page that could not be recognised is journalled
#     as content (`no_text_layer`) and outlives the repair of the engine;
#   * **the ceilings** — a scanned book refused by a number meant for text, or a
#     500 MB text file loaded because its name ended in `.pdf`;
#   * **the pool's clock** — a slow scanned volume killed mid-way, a worker
#     that never stops sending pages never killed, or a Stop that waits out a
#     whole volume, and the file it interrupted journalled as a skip;
#   * **what a page remembers** — a page read by OCR indistinguishable from one
#     with a text layer, and the confidence and bitmap that say how far to trust it
#     gone.
#
# ⚠️ **The word "narrowest" has a limit for the page-source cases.** The only
# test that sees a page's own source reach the `page` table without Vision is the
# randomised harness (`random_sequences_do_not_lose_data`, invariant 7, through a
# stand-in worker that sends OCR pages beside native ones). The narrow test,
# `a_mixed_pdf_keeps_each_pages_own_source_and_what_ocr_knew` in `slice.rs`, is
# macOS-only. The harness is the Linux answer; its cost is minutes, not seconds.
#
# ⚠️ **There is no case for the re-check of the ceiling after `fs::read`**
# (the `loaded > ceiling` branch of `worker.rs`) **and that is a measured gap, not
# an omission.** Deleting it leaves `mnema-extract` green: the branch is reached
# only by a file that grows between `stat` and the read, and no test makes one.
# See the report of this task.

# ------------------------------------------------ the kind of a failed engine

# O1. A page the engine could not recognise answered as `no_text_layer`. That is
# a verdict about the file: the parent journals it, `is_about_content()` keeps it
# across walks, and the scan stays "textless" after the engine is repaired (D57).
# The mutant is the `Malformed` arm's shape, which is the nearest refusal.
case_ "worker: an engine that did not run fails the file and does not refuse it" \
  crates/mnema-extract/src/bin/worker.rs \
  's#                Err\(PdfError::Ocr\(e\)\) => vec!\[Frame::Failed \{\n                    message: format!\("a page without a text layer could not be recognised: \{e\}"\),\n                \}\],#                Err(PdfError::Ocr(e)) => vec![reader_refused(\n                    manifest::READER_PDF,\n                    manifest::PDF_READER_VERSION,\n                    "no_text_layer",\n                    e.to_string(),\n                    Some(sha256),\n                )],#' \
  '                    "no_text_layer",
                    e.to_string(),' \
  mnema-extract 'an_unavailable_engine_fails_a_scan_and_leaves_a_native_pdf_alone' --test worker_cli

# ------------------------------------------------------------- the pool's clock

# P1. The deadline not renewed on a `Page`: it is one deadline for the whole
# document again, and a scanned volume that is never silent for the timeout is
# killed for being slow — the defect the per-page rule exists to remove.
case_ "pool: a page renews the deadline" \
  crates/mnema-pool/src/lib.rs \
  's#                deadline = Instant::now\(\) \+ config\.timeout;\n##' \
  '                }
                pages.push(ExtractedPage {' \
  mnema-pool 'a_document_slower_than_the_deadline_but_never_silent_for_it_arrives_whole' --test supervision

# P2. The bound that makes the per-page deadline finite, removed. A worker that
# keeps sending pages renews the clock for ever and is never killed; the test
# names the bound by its detail, so the check moved to the summary (where it
# would still fire for a finite stream) is red too. The comment above the check
# goes with it, so the marker anchors on code: the let-else straight into the
# renewal.
case_ "pool: no more pages than the header promised" \
  crates/mnema-pool/src/lib.rs \
  's#                // The bound that makes the per-page deadline finite\. Without\n                // it, a worker that keeps sending pages is never killed\.\n                if pages\.len\(\) >= promised as usize \{\n                    return Err\(protocol\(\n                        &line,\n                        &format!\("more pages than the header promised \(\{promised\}\)"\),\n                    \)\);\n                \}\n##' \
  '                    return Err(protocol(&line, "a page before any header"));
                };
                deadline = Instant::now() + config.timeout;' \
  mnema-pool 'more_pages_than_the_header_promised_stop_the_job' --test supervision

# P5. The header's promise unbounded: a `u32` straight off the wire makes a
# document's bound `(4·10⁹ + 1) × timeout`. D1's corpus ceiling is the cap.
case_ "pool: a header may promise at most MAX_PAGES" \
  crates/mnema-pool/src/lib.rs \
  's#                if pages > MAX_PAGES \{#                if false && pages > MAX_PAGES {#' \
  '                if false && pages > MAX_PAGES {' \
  mnema-pool 'a_header_promising_more_than_max_pages_costs_that_file_not_the_job' --test supervision

# P7. The cap answered as `PoolError::Protocol` again. The count is the file's
# own (`FPDF_GetPageCount`), so one crafted PDF would stop the whole job; the
# test reads the file after it in the same pool.
case_ "pool: a header over the cap costs that file, not the job" \
  crates/mnema-pool/src/lib.rs \
  's#                if pages > MAX_PAGES \{\n                    return Ok\(Answer::Ended \{\n                        ending: worker\.stop\(false\)\?,\n                        note: Some\(format!\(\n[^\n]*\n[^\n]*\n                        \)\),\n                    \}\);#                if pages > MAX_PAGES {\n                    return Err(protocol(&line, "a header over the page cap"));#' \
  '                    return Err(protocol(&line, "a header over the page cap"));' \
  mnema-pool 'a_header_promising_more_than_max_pages_costs_that_file_not_the_job' --test supervision

# W1. The worker writes a header for a PDF declaring more pages than D1 allows,
# leaving the pool's defence as the only one — and the file journalled as a
# crashed worker rather than as these bytes being malformed.
case_ "worker: a PDF over the page ceiling is refused as malformed" \
  crates/mnema-extract/src/pdf.rs \
  's#    if count as i64 > i64::from\(cap\) \{#    if false && count as i64 > i64::from(cap) {#' \
  '    if false && count as i64 > i64::from(cap) {' \
  mnema-extract 'a_pdf_declaring_more_pages_than_the_ceiling_is_refused_as_malformed' --test worker_cli

# W2. The same ceiling skipped for the readers that count pages from content:
# a markdown file of 64 MiB can hold sixteen million sections.
case_ "worker: a buffered document over the page ceiling is refused" \
  crates/mnema-extract/src/bin/worker.rs \
  's#        \}\) if \*pages > cap => vec!\[reader_refused\(#        }) if false && *pages > cap => vec![reader_refused(#' \
  '        }) if false && *pages > cap => vec![reader_refused(' \
  mnema-extract 'a_document_with_more_sections_than_the_ceiling_is_refused_under_its_reader' --test worker_cli

# P6. The cancel flag never read while a document is in flight. Stop then waits
# out the whole document — minutes for an OCR'd volume — which is what the
# per-page deadline made possible (review of D189).
case_ "pool: a raised cancel stops the document in flight" \
  crates/mnema-pool/src/lib.rs \
  's#        if cancel\.load\(Ordering::SeqCst\) \{\n            worker\.stop\(false\)\?;#        if false && cancel.load(Ordering::SeqCst) {\n            worker.stop(false)?;#' \
  '        if false && cancel.load(Ordering::SeqCst) {
            worker.stop(false)?;' \
  mnema-pool 'a_raised_cancel_kills_the_worker_in_the_middle_of_a_document' --test supervision

# I1. A cancelled file journalled as a skip. Nothing was learned about it, and a
# skip row is a verdict the next walk answers from without a worker.
case_ "ingest: a file cancelled in flight records nothing" \
  crates/mnema-ingest/src/lib.rs \
  's#        Outcome::Cancelled => return Err\(IngestError::Cancelled\),#        Outcome::Cancelled => {\n            db.record_skip(root_id, relative, None, "cancelled", SkipRule::Unreadable, None, None)?;\n            return Err(IngestError::Cancelled);\n        }#' \
  '            db.record_skip(root_id, relative, None, "cancelled", SkipRule::Unreadable, None, None)?;' \
  mnema-ingest 'a_cancel_in_the_middle_of_a_file_leaves_its_previous_document_untouched' --test walk

# P3. One ceiling for everything: `max_bytes` sent as the PDF ceiling too. A
# scanned book is refused by a number chosen for text, which is the whole reason
# the second ceiling exists.
case_ "pool: the PDF ceiling is the config's own" \
  crates/mnema-pool/src/lib.rs \
  's#        pdf_max_bytes: config\.pdf_max_bytes,#        pdf_max_bytes: config.max_bytes,#' \
  '        pdf_max_bytes: config.max_bytes,' \
  mnema-pool 'the_pdf_ceiling_crosses_the_wire_from_the_config' --test supervision

# P4. The page frame's own fields not carried into `ExtractedPage`. The pool is
# the only place the wire's `text_source` and bitmap become the application's, and
# a stand-in worker's OCR pages are the only thing that can tell the two apart
# without Vision.
case_ "pool: a page frame's source and bitmap reach the extracted page" \
  crates/mnema-pool/src/lib.rs \
  's#                    text_source,\n                    ocr_confidence,\n                    width_px,\n                    height_px,\n                    dpi,\n                    blocks: Vec::new\(\),#                    text_source: None,\n                    ocr_confidence: None,\n                    width_px: None,\n                    height_px: None,\n                    dpi: None,\n                    blocks: Vec::new(),#' \
  '                    dpi: None,
                    blocks: Vec::new(),' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# -------------------------------------------------------- the worker's ceilings
#
# Three ways to choose the ceiling wrongly, and the three fixtures that tell them
# apart. All three rewrite the one `is_pdf` line.

# C1. By extension. A PDF called `.txt` is refused by the text ceiling; a text file
# called `.pdf` is loaded under the PDF one. The name is what the file is called.
case_ "worker: the ceiling follows the bytes, not the extension" \
  crates/mnema-extract/src/bin/worker.rs \
  's#    let is_pdf = bytes\.starts_with\(PDF_MAGIC\);#    let is_pdf = request.path.to_lowercase().ends_with(".pdf");#' \
  '    let is_pdf = request.path.to_lowercase().ends_with(".pdf");' \
  mnema-extract 'the_ceiling_is_chosen_by_magic_not_by_extension' --test worker_cli

# C2. Every file is a PDF for the ceiling: the magic check dropped. A 500 MB text
# file passes `stat` under a number chosen for scans and is then held four times
# over in memory.
case_ "worker: a non-PDF is held to the non-PDF ceiling" \
  crates/mnema-extract/src/bin/worker.rs \
  's#    let is_pdf = bytes\.starts_with\(PDF_MAGIC\);#    let is_pdf = true;#' \
  '    let is_pdf = true;' \
  mnema-extract 'a_non_pdf_over_max_bytes_is_refused_even_when_the_pdf_ceiling_would_admit_it' --test worker_cli

# C3. No file is a PDF for the ceiling: every scan is refused by the text ceiling,
# which is the defect the second ceiling was added to remove.
case_ "worker: a PDF is held to the PDF ceiling" \
  crates/mnema-extract/src/bin/worker.rs \
  's#    let is_pdf = bytes\.starts_with\(PDF_MAGIC\);#    let is_pdf = false;#' \
  '    let is_pdf = false;' \
  mnema-extract 'a_pdf_over_max_bytes_is_read_when_it_is_under_pdf_max_bytes' --test worker_cli

# ---------------------------------------------------- what a page remembers
#
# Five places a page's own source and what OCR knew can be dropped between the
# wire and the `page` row; the stand-in worker of the randomised harness sends
# every even page as OCR and every odd one native, so each is a difference the
# harness can see.

# S1. The document's source written for every page. A scan and a text layer in
# one PDF both read `native:pdf`, and nothing says which page to distrust.
case_ "ingest: a page keeps the source its frame named" \
  crates/mnema-ingest/src/lib.rs \
  's#            text_source: page\.text_source\.as_deref\(\)\.unwrap_or\(&document\.text_source\),#            text_source: \&document.text_source,#' \
  '            text_source: &document.text_source,' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# S2. The confidence dropped. The only number that says how far to trust a page
# read from pixels is gone, and the page looks like one read from a text layer.
case_ "ingest: an OCR page keeps its confidence" \
  crates/mnema-ingest/src/lib.rs \
  's#                confidence: page\.ocr_confidence,#                confidence: None,#' \
  '                confidence: None,' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# S3. The bitmap's size and resolution dropped: what a reader needs to put a
# citation's box back on the picture (the `bbox` follow-up) is not recorded.
case_ "ingest: an OCR page keeps the bitmap it was read from" \
  crates/mnema-ingest/src/lib.rs \
  's#                width_px: page\.width_px,\n                height_px: page\.height_px,\n                dpi: page\.dpi,#                width_px: None,\n                height_px: None,\n                dpi: None,#' \
  '                width_px: None,
                height_px: None,
                dpi: None,' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# S4. The same drop one layer down, in the writer: `ocr` arrives and the INSERT
# binds NULL for the confidence.
case_ "index: the page row stores the confidence it is given" \
  crates/mnema-index/src/write.rs \
  's#                ocr\.confidence,\n#                None::<f32>,\n#' \
  '                None::<f32>,' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# S5. And for the bitmap.
case_ "index: the page row stores the bitmap it is given" \
  crates/mnema-index/src/write.rs \
  's#                ocr\.width_px,\n                ocr\.height_px,\n                ocr\.dpi\n#                None::<u32>,\n                None::<u32>,\n                None::<u32>\n#' \
  '                None::<u32>,
                None::<u32>,
                None::<u32>' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# --------------------------------------------------------------- the language

# L1. The guard on Ukrainian relaxed to "some language is supported". An engine
# without the Ukrainian model then reads a Ukrainian library as English and
# returns garbage or nothing, which is journalled as "no text" — a fact about the
# engine presented as one about every scan.
case_ "ocr: an engine without Ukrainian is unavailable" \
  crates/mnema-extract/src/ocr.rs \
  's#    if chosen\.first\(\) != Some\(&LANGUAGES\[0\]\) \{#    if chosen.is_empty() {#' \
  '    if chosen.is_empty() {' \
  mnema-extract 'ocr::tests::an_engine_without_ukrainian_is_unavailable' --lib

# --------------------------------------------------------------- the migration

# M1. Migration 8 written and not registered. A database at version 7 is never
# given `page.ocr_confidence`, and the first OCR page written to it fails the
# INSERT. (`task-3.sh`'s "registered" case removes migration 4's line, not this
# one.)
case_ "index: migration 8 is registered" \
  crates/mnema-index/src/migrations.rs \
  's#        M::up\(ADD_PAGE_OCR_CONFIDENCE\),\n##' \
  '        M::up(ADD_SKIP_READER),
        M::up(ADD_WATCHED_ROOT_CONFIRMED),
        M::up(ADD_GITIGNORE_INCLUDE),
    ])' \
  mnema-index 'migrations::tests::a_v7_index_gains_page_ocr_confidence_and_keeps_its_pages' --lib

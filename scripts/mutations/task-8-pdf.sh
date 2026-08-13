# Mutation cases for Task 8: the PDF reader, the page it refuses to keep, and
# the three failures it must not confuse. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-8-pdf.sh
#
# NOT `task-8.sh`, which is taken: the previous cycle (G7.4, packaging) numbered
# its own tasks from one and its `task-8.sh` holds the Tauri shell's cases.
# Overwriting it would have deleted a working case file to satisfy a naming
# convention. Tasks 9 and 13 of this cycle will meet the same collision.
#
# What this file is mostly about is not "does the reader read". It is that the
# three ways a PDF can fail have three different prices, and every one of them
# is silent when it is wrong:
#
#   * a library that will not load, reported as damage, journals every PDF in a
#     folder as broken, finishes the walk green, and survives the repair;
#   * a page number renumbered to close its own gap cites the wrong page of a
#     contract, plausibly;
#   * a reader name one character off sends every PDF citation to the
#     line-number default, which answers `Coordinate::None`;
#   * a rule string the pool does not parse stops the whole job on the first
#     scanned file.
#
# Note what the compiler already covers, so no case here duplicates it:
# `Failure` is generated from a declaration list, so a variant added without a
# journal decision does not compile (`every_failure_maps_onto_its_own_skip_rule`
# and `journalled_as` in `mnema-pool/tests/supervision.rs`), and the worker's
# `PdfError` match is exhaustive by variant rather than by a catch-all `Err(e)`.

# ------------------------------------------------------ the reader's own name

# C1. The name is one symbol across a process boundary and across D40, so
# nothing but a test can hold the two ends together. `"pdf-2"` is what a typo
# looks like: `pages_of` falls to `_ => PageContext::Lines`, the chunker
# computes a line range from blocks that carry none, and every PDF citation
# comes back with `Coordinate::None`. Green everywhere else.
case_ "worker: the header names the pdf reader, not a near-miss" \
  crates/mnema-extract/src/bin/worker.rs \
  's{reader: manifest::READER_PDF\.to_string\(\),}{reader: "pdf-2".to_string(),}' \
  'reader: "pdf-2".to_string(),' \
  mnema-extract 'a_pdf_is_read_and_its_header_names_the_pdf_reader' --test worker_cli

# C2. The version, which is what decides whether a document already in the index
# was made by today's code.
case_ "worker: the header carries the pdf reader's version" \
  crates/mnema-extract/src/bin/worker.rs \
  's{reader_version: manifest::PDF_READER_VERSION,}{reader_version: 0,}' \
  'reader_version: 0,' \
  mnema-extract 'a_pdf_is_read_and_its_header_names_the_pdf_reader' --test worker_cli

# C3. `native:pdf` names the reader, not the file. A page whose text came out of
# a PDF parse is not the same evidence as one that came out of a text read, and
# `page.text_source` is where that survives.
case_ "worker: the summary names the pdf text source" \
  crates/mnema-extract/src/bin/worker.rs \
  's{text_source: "native:pdf"\.to_string\(\),}{text_source: "native:txt".to_string(),}' \
  'text_source: "native:txt".to_string(),' \
  mnema-extract 'a_pdf_is_read_and_its_header_names_the_pdf_reader' --test worker_cli

# ------------------------------------------- which failure is about the file

# C4. **The most expensive line in this task.** A quarantined `libpdfium.dylib`
# has already happened on this machine. Under `malformed` the walk does not stop
# (`suggests_broken_environment() == false`) and the verdict outlives the repair
# (`is_about_content() == true`), so a folder of PDFs is journalled as damaged
# by a green walk and a fixed install returns nothing.
case_ "worker: a library that will not load is not a damaged file" \
  crates/mnema-extract/src/bin/worker.rs \
  's{Err\(PdfError::Library\(e\)\) => vec!\[Frame::Failed \{\n                message: format!\("pdfium could not be loaded: \{e\}"\),\n            \}\],}{Err(PdfError::Library(e)) => vec![Frame::Refused \{\n                rule: "malformed".to_string(),\n                reason: format!("pdfium could not be loaded: \{e\}"),\n                sha256: Some(sha256),\n            \}],}' \
  'rule: "malformed".to_string(),
                reason: format!("pdfium could not be loaded: {e}"),' \
  mnema-extract 'a_damaged_pdf_is_the_files_fault_and_an_unloadable_library_is_not' --test pdf

# C5. The other direction of the same split, and it needs its own case: an
# implementation that answered `Failed` for everything satisfies C4.
case_ "worker: a damaged file is not an unloadable library" \
  crates/mnema-extract/src/bin/worker.rs \
  's{Err\(e @ PdfError::Malformed\(_\)\) => vec!\[Frame::Refused \{\n                rule: "malformed"\.to_string\(\),\n                reason: e\.to_string\(\),\n                sha256: Some\(sha256\),\n            \}\],}{Err(e @ PdfError::Malformed(_)) => vec![Frame::Failed \{\n                message: e.to_string(),\n            \}],}' \
  'Err(e @ PdfError::Malformed(_)) => vec![Frame::Failed {' \
  mnema-extract 'every_refusal_that_read_the_file_carries_the_digest_it_read' --test worker_cli

# C6. A password is not damage. Both rules refuse and both are about content, so
# nothing downstream goes red — only the sentence in the skip list changes, from
# "locked, find the password" to "broken, go looking for a corrupt file that
# does not exist".
case_ "reader: a locked document is not a damaged one" \
  crates/mnema-extract/src/pdf.rs \
  's{        \) => PdfError::Encrypted,}{        ) => PdfError::Malformed("encrypted".to_string()),}' \
  ') => PdfError::Malformed("encrypted".to_string()),' \
  mnema-extract 'a_password_protected_pdf_is_locked_rather_than_damaged' --test pdf

# C7. Every refusal reached by reading the file owes the digest it was reached
# on. `displaces` reads a missing digest as "the bytes are unknown, so
# displace": a folder of scans walked by a build one Pdfium behind would lose a
# document per file with the bytes never having moved.
case_ "worker: the no_text_layer refusal carries the digest it read" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                     \{TEXT_LAYER_MIN_CHARS\} characters"\n                \),\n                sha256: Some\(sha256\),}{                     \{TEXT_LAYER_MIN_CHARS\} characters"\n                ),\n                sha256: None,}' \
  '{TEXT_LAYER_MIN_CHARS} characters"
                ),
                sha256: None,' \
  mnema-extract 'every_refusal_that_read_the_file_carries_the_digest_it_read' --test worker_cli

# C8. `unsupported` promises a reader that is coming. The reader came; the file
# is a photograph of a page. Two opposite answers to the person reading the
# skip list, and — through `SkipRule` — two different `displaces` decisions.
case_ "worker: a scan is refused under its own rule, not unsupported" \
  crates/mnema-extract/src/bin/worker.rs \
  's{(Ok\(doc\) if doc\.pages\.is_empty\(\) => vec!\[Frame::Refused \{\n                )rule: "no_text_layer"\.to_string\(\),}{${1}rule: "unsupported".to_string(),}' \
  'rule: "unsupported".to_string(),
                reason: format!(' \
  mnema-extract 'a_pdf_with_no_text_layer_on_any_page_is_refused_under_its_own_rule' --test worker_cli

# C9. The threshold is a product decision someone may want to argue with, so the
# refusal states the number rather than saying "no text" — which does not
# distinguish a scan from an empty file.
case_ "worker: the scan refusal names the threshold it applied" \
  crates/mnema-extract/src/bin/worker.rs \
  's{\{TEXT_LAYER_MIN_CHARS\} characters}{some characters}' \
  'some characters"' \
  mnema-extract 'a_pdf_with_no_text_layer_on_any_page_is_refused_under_its_own_rule' --test worker_cli

# ------------------------------------------------------------- the threshold

# C10. The rule the whole page-skipping design exists for. `> 0` is the
# behaviour requirements §13 calls the worst available: a scanned page carries a
# Bates number or a scanner footer, and indexing it as content puts a citable,
# searchable page holding the word `Page` into the index.
case_ "probe: the text layer test is a threshold, not a non-zero check" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's{    char_count >= TEXT_LAYER_MIN_CHARS\n\}}{    char_count > 0\n\}}' \
  'char_count > 0
}' \
  mnema-extract 'a_page_under_the_threshold_is_skipped_by_number_and_its_neighbours_are_not' --test pdf

# C11. One character of the comparison, which no fixture in this repository can
# see: the pages measured are 119 and 8 characters against a threshold of 48.
# Stated against the constant instead, so a product decision to move the number
# costs nothing and an off-by-one costs this.
case_ "probe: the threshold is inclusive at the constant" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's{    char_count >= TEXT_LAYER_MIN_CHARS\n\}}{    char_count > TEXT_LAYER_MIN_CHARS\n\}}' \
  'char_count > TEXT_LAYER_MIN_CHARS
}' \
  mnema-extract 'pdfium_probe::tests::the_threshold_is_inclusive_at_the_constant' --lib

# C12. Counting before normalisation. A Ukrainian `й` written decomposed is two
# characters that become one, so a page of forty such letters counts as eighty,
# clears the threshold, and is stored as forty — under the minimum the threshold
# exists to enforce, in the alphabet this product is built for.
case_ "probe: characters are counted after NFC, not before" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's{    nfc::normalise\(text\)\n        \.chars\(\)}{    text\n        .chars()}' \
  '    text
        .chars()' \
  mnema-extract 'pdfium_probe::tests::characters_are_counted_after_normalisation_not_before' --lib

# ---------------------------------------------------- the page and its number

# C13. The gap closed. A survivor renumbered by its position among the survivors
# cites page 2 of a contract whose page 2 is the one nobody could read — the
# most plausible wrong answer this reader can give, and a `Coordinate::Page` is
# non-empty and of the right type either way.
case_ "reader: a survivor keeps its own page number, gap and all" \
  crates/mnema-extract/src/pdf.rs \
  's{        pages\.push\(PdfPage \{\n            page_no,}{        pages.push(PdfPage \{\n            page_no: pages.len() as u32 + 1,}' \
  'page_no: pages.len() as u32 + 1,' \
  mnema-extract 'a_page_under_the_threshold_is_skipped_by_number_and_its_neighbours_are_not' --test pdf

# C14. A page dropped without being named. `Summary::skipped_pages` would name
# nothing, the journal would hold no row, and a contract missing its middle page
# would read as a contract that never had one. (It carried a count when this
# case was written; task 9 replaced the count with the numbers themselves, which
# is what the wording follows.)
case_ "reader: a skipped page is named, not silently dropped" \
  crates/mnema-extract/src/pdf.rs \
  's{            skipped\.push\(page_no\);\n            continue;}{            continue;}' \
  'if !has_text_layer(text_layer_chars(&raw)) {
            continue;
        }' \
  mnema-extract 'every_page_of_a_document_is_either_read_or_named' --test pdf

# C15. The header's count taken a second way. The pool requires it to equal the
# number of `Page` frames that arrive and deliberately does not look at the
# largest `page_no`; announcing the document's page count would stop the job on
# every PDF with a scanned page in it.
case_ "worker: the header counts the pages sent, not the document's" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                    pages: doc\.pages\.len\(\) as u32,}{                    pages: (doc.pages.len() + doc.skipped.len()) as u32,}' \
  'pages: (doc.pages.len() + doc.skipped.len()) as u32,' \
  mnema-extract 'a_skipped_pdf_page_leaves_a_gap_and_is_named_rather_than_announced' --test worker_cli

# C16. The other half of the same pair: the summary's account of what was
# dropped. Task 9 turned the count into the numbers themselves, so the line this
# mutates and the test it reddens both moved; the case is the same one.
case_ "worker: the summary names the pages that were skipped" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                    skipped_pages: doc\.skipped,}{                    skipped_pages: Vec::new(),}' \
  'skipped_pages: Vec::new(),
                    text_source: "native:pdf"' \
  mnema-extract 'a_skipped_pdf_page_leaves_a_gap_and_is_named_rather_than_announced' --test worker_cli

# C17. A line number invented for a format that has none. `pages_of` gives a PDF
# `PageContext::Fixed(Coordinate::Page)` *because* these blocks carry no rows;
# a number here is a citation to rows of a page that does not have any.
case_ "reader: a pdf block carries no line numbers" \
  crates/mnema-extract/src/pdf.rs \
  's{                line_start: None,\n                line_end: None,}{                line_start: Some(1),\n                line_end: Some(1),}' \
  'line_start: Some(1),
                line_end: Some(1),' \
  mnema-extract 'a_page_with_a_text_layer_becomes_a_block_of_that_page' --test pdf

# ------------------------------------------- every page the document declares
#
# Added in fix round 1. The defect these guard was found by *running* the worker
# on a crafted file, not by reading the loop: `pages().iter()` is
# `self.pages.get(i).ok()`, so the first page `FPDF_LoadPage` declines ends the
# iteration, and a three-page contract came back as a one-page document with
# `skipped_pages: 0`, no gap, the pool's integrity check satisfied, and the walk
# green. Nothing recorded that two pages had gone.

# C21. The iterator put back. This is the defect verbatim.
case_ "reader: pages are counted by FPDF_GetPageCount, not by an iterator that gives up" \
  crates/mnema-extract/src/pdf.rs \
  's{    let count = all_pages\.len\(\);}{    let count = all_pages.iter().count() as i32;}' \
  'let count = all_pages.iter().count() as i32;' \
  mnema-extract 'a_page_that_will_not_load_refuses_the_document_rather_than_ending_it' --test pdf

# C22. The other way to lose it: a page that will not load recorded as a page
# with no text. `no_text_layer` is a verdict about content that the journal
# keeps until `INDEX_FORMAT_VERSION` moves and that no reader upgrade reaches
# (D57), so this is the reader remembering its own failure as a fact about the
# scan — which `pdf.rs`'s own comment forbids and this arm would have done.
case_ "reader: a page that will not load is not a page without text" \
  crates/mnema-extract/src/pdf.rs \
  's{        let page = all_pages\.get\(index\)\.map_err\(\|e\| \{\n            PdfError::Malformed\(format!\(\n                "page \{page_no\} of this PDF could not be loaded: \{e\}"\n            \)\)\n        \}\)\?;}{        let Ok(page) = all_pages.get(index) else \{\n            skipped.push(page_no);\n            continue;\n        \};}' \
  'let Ok(page) = all_pages.get(index) else {' \
  mnema-extract 'a_page_that_will_not_load_refuses_the_document_rather_than_ending_it' --test pdf

# C23. The page number dropped from the message. It is the last place it can be:
# `Frame::Refused` carries a rule and a sentence, and nothing downstream of the
# worker ever learns which page failed.
case_ "reader: the refusal names the page that failed" \
  crates/mnema-extract/src/pdf.rs \
  's{                "page \{page_no\} of this PDF could not be loaded: \{e\}"}{                "a page of this PDF could not be loaded: \{e\}"}' \
  '"a page of this PDF could not be loaded: {e}"' \
  mnema-extract 'a_page_that_will_not_load_refuses_the_document_rather_than_ending_it' --test pdf

# C24. A document with no pages answered as a scan. Vacuously true and
# misleading — "no page carries a text layer of at least 48 characters" about a
# file with no pages — and remembered as a verdict about content.
case_ "reader: a document with no pages is not a scan" \
  crates/mnema-extract/src/pdf.rs \
  's{    if count <= 0 \{}{    if count < 0 \{}' \
  'if count < 0 {' \
  mnema-extract 'a_pdf_with_no_pages_at_all_is_not_reported_as_having_no_text' --test pdf

# C25. A page lost from **both** lists — the class the partition test names and
# the one it could not see while it took its page count from `probe_text_layer`,
# which walks the same iterator the reader did. The yardstick is now a literal
# the fixture generator printed, so it cannot move with the reader.
case_ "reader: the pages read and the pages named are every page of the file" \
  crates/mnema-extract/src/pdf.rs \
  's{    for index in 0\.\.count \{}{    for index in 0..count.saturating_sub(1) \{}' \
  'for index in 0..count.saturating_sub(1) {' \
  mnema-extract 'every_page_of_a_document_is_either_read_or_named' --test pdf

# C26. The same truncation in the probe, which is what `--probe-pdfium` answers
# a packaging question with: a probe that under-counts pages says a bundle is
# fine when it is not, and it must not disagree with the reader about which
# pages a document has.
case_ "probe: the diagnostic counts pages the same way the reader does" \
  crates/mnema-extract/src/pdfium_probe.rs \
  's{    for index in 0\.\.all_pages\.len\(\) \{\n        let page = all_pages\n            \.get\(index\)\n            \.map_err\(\|e\| Error::Pdfium\(format!\("page \{\} could not be loaded: \{e\}", index \+ 1\)\)\)\?;}{    for page in all_pages.iter() \{}' \
  'for page in all_pages.iter() {' \
  mnema-extract 'the_probe_and_the_reader_see_the_same_pages' --test pdf

# ------------------------------------------------------------- the pool's arm

# C18. The arm whose absence would have stopped a walk on the first scanned PDF
# in a folder. Parsing is strict: an unknown rule is `PoolError::Protocol` and
# ends the job, reading as a mismatched worker binary rather than as a missing
# match arm. `SkipRule::NoTextLayer` had existed since the skeleton with nothing
# able to send the string.
case_ "pool: the no_text_layer rule crosses the wire at all" \
  crates/mnema-pool/src/lib.rs \
  's{                    "no_text_layer" => Failure::NoTextLayer,\n}{}' \
  '"encrypted" => Failure::Encrypted,
                    // Strict on purpose.' \
  mnema-pool 'a_scanned_pdf_crosses_the_wire_as_its_own_rule' --test supervision

# C19. And it must reach its own rule rather than the neighbour it is easiest to
# fold into — the two are opposite promises about whether a reader is coming.
case_ "pool: a scan is not folded into unsupported" \
  crates/mnema-pool/src/lib.rs \
  's{                    "no_text_layer" => Failure::NoTextLayer,}{                    "no_text_layer" => Failure::Unsupported,}' \
  '"no_text_layer" => Failure::Unsupported,' \
  mnema-pool 'a_scanned_pdf_crosses_the_wire_as_its_own_rule' --test supervision

# ------------------------------------------------------------- the manifest

# C20. `pdf` added to the extension map: the map is a claim about
# `typing::identify`, and prose in a file named `notes.pdf` is read by the text
# reader. The entry would be a prediction contradicted by the worker, which is
# the one thing this map exists not to be.
case_ "manifest: pdf is decided by content, so it has no extension entry" \
  crates/mnema-extract/src/manifest.rs \
  's{    Manifest \{\n        default: ReaderId::new\("text", TEXT_READER_VERSION\),}{    by_extension.insert(\n        "pdf".to_string(),\n        ReaderId::new(READER_PDF, PDF_READER_VERSION),\n    );\n    Manifest \{\n        default: ReaderId::new("text", TEXT_READER_VERSION),}' \
  'ReaderId::new(READER_PDF, PDF_READER_VERSION),' \
  mnema-extract 'the_manifest_names_the_reader_that_identify_actually_picks' --test manifest

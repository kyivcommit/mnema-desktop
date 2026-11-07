# Mutation cases for the PDF page view: a page drawn as a PNG with the boxes of a
# cited passage. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pdf-page-view.sh
#
# What is silent when it is wrong here is a highlight on the wrong words: the
# window draws whatever boxes it is given, in the colour of a citation, so a map
# shifted by one char, a hash nobody compares, or a rotated page turned as if it
# were upright is a picture that looks right and points at the wrong text. The
# window cannot tell; only these tests can.
#
# The boxes cases share one expression with two tests where two separate
# properties ride on it (an offset crop box, a rotated page); each test is its
# own case so that a test going green on its own is visible.
#
# Every `case_` below has one mutation and one test. Expressions use `~` as the
# delimiter: the Rust they rewrite is full of `|`, `{` and `/`. A quote in the
# source is `\x27` in a replacement and a `.` in a pattern.

P=crates/mnema-extract/src/pdfium_probe.rs
V=crates/mnema-extract/src/page_view.rs
PT='pdfium_probe::tests::'
AT='page_view::tests::'

# ── The map from stored chars to pdfium chars ────────────────────────────────

# M1. Every index one too high: the box lands on the next glyph.
case_ "map: every index is one past the pdfium char it came from" \
  $P \
  's~values\.push\(\(value, Some\(index\)\)\);~values.push((value, Some(index + 1)));~' \
  'Some(index + 1)' \
  mnema-extract "${PT}every_mapped_index_names_the_pdfium_char_the_text_char_came_from" --lib

# M2. The text is read with `inside_rect`, whose string carries the generated
# line breaks as if they were chars: positions in it are not char indices.
case_ "map: the text comes from inside_rect, positions in it are not pdfium indices" \
  $P \
  's~let chars = chars_within\(&text, page_bound\(page\)\)\s*\.into_iter\(\)\s*\.map\(\|\(value, index\)\| \(char::from_u32\(value\)\.unwrap_or\([^)]*\), index\)\)\s*\.collect\(\);~let chars = text.inside_rect(page_bound(page)).chars().enumerate().map(|(i, c)| (c, Some(i))).collect();~' \
  'text.inside_rect(page_bound(page)).chars().enumerate()' \
  mnema-extract "${PT}a_synthesised_line_break_has_no_source_char" --lib

# M3. The `\r\n` the walk writes itself is given the index of the char that
# follows: a range that covers it boxes a glyph that is not there.
case_ "map: a synthesised line break is given a pdfium index" \
  $P \
  's~values\.extend\(\[\(u32::from\(b.\\r.\), None\), \(u32::from\(b.\\n.\), None\)\]\);~values.extend([(u32::from(b\x27\\r\x27), Some(index)), (u32::from(b\x27\\n\x27), Some(index))]);~' \
  "(u32::from(b'\\n'), Some(index))" \
  mnema-extract "${PT}a_synthesised_line_break_has_no_source_char" --lib

# ── Whether the boxes may be drawn at all ────────────────────────────────────

# H1. The hash of the page is read and never compared: a page that changed
# under the index is boxed with offsets counted in other text.
case_ "hash: the page text is not compared with the hash the offsets were counted in" \
  $V \
  's~if hex != \*wanted \{~if false {~' \
  'if false {' \
  mnema-extract 'text_that_is_not_the_text_the_offsets_were_counted_in_gets_the_page_and_no_boxes' --test page_view

# H2. NFC recomposed the page and the map no longer lines up, and the boxes are
# drawn anyway: the hash matches (it is the stored text's own), the chars do not.
case_ "nfc: a page NFC rewrote still gets boxes from a map that no longer lines up" \
  $V \
  's~if !mapped\.nfc_ok \{\s*return \(TextMatch::Unplaced, Vec::new\(\)\);~if false { return (TextMatch::Unplaced, Vec::new());~' \
  'if false { return (TextMatch::Unplaced' \
  mnema-extract 'a_page_whose_text_nfc_rewrote_has_no_boxes_because_its_offsets_no_longer_line_up' --test page_view

# H2b. A page NFC rewrote is reported as changed: the file did not change.
case_ "nfc: a page NFC rewrote is reported as a changed file" \
  $V \
  's~if !mapped\.nfc_ok \{\s*return \(TextMatch::Unplaced, Vec::new\(\)\);~if !mapped.nfc_ok { return (TextMatch::Mismatch, Vec::new());~' \
  'if !mapped.nfc_ok { return (TextMatch::Mismatch' \
  mnema-extract 'a_page_whose_text_nfc_rewrote_has_no_boxes_because_its_offsets_no_longer_line_up' --test page_view

# H2c. A cited range that yields no box is passed off as a match: the page
# reads as one with nothing cited on it.
case_ "unplaced: a range with no box is still a match" \
  $V \
  's~if range\.len > 0 && boxes\.is_empty\(\) \{~if false {~' \
  'if false {' \
  mnema-extract 'a_range_past_the_end_of_the_text_is_clamped_and_draws_what_is_there' --test page_view

# C1. The bitmap's bytes swapped once more: pdfium already wrote them RGBA, so
# red is drawn blue — invisible on a black-on-white page.
case_ "colour: red and blue are swapped" \
  $V \
  's~rgba\.extend_from_slice\(&row\[\.\.w as usize \* 4\]\);~for px in row[..w as usize * 4].chunks_exact(4) { rgba.extend_from_slice(\&[px[2], px[1], px[0], px[3]]); }~' \
  'rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);' \
  mnema-extract 'colours_are_drawn_as_the_page_has_them' --test page_view

# H3. A mismatch is reported as one and the boxes are returned with it.
case_ "mismatch: the boxes are returned together with the word that they cannot be trusted" \
  $V \
  's~return \(TextMatch::Mismatch, Vec::new\(\)\);~return (TextMatch::Mismatch, req.ranges.iter().flat_map(|range| range_rects(page, config, range, &mapped.map, size)).collect());~' \
  'TextMatch::Mismatch, req.ranges' \
  mnema-extract 'text_that_is_not_the_text_the_offsets_were_counted_in_gets_the_page_and_no_boxes' --test page_view

# ── The boxes themselves ─────────────────────────────────────────────────────

# B1. The glyphs' own boxes instead of the line's: a staircase of boxes each as
# tall as its letter, and the word's box starts below the line's top.
case_ "boxes: tight bounds instead of loose ones" \
  $V \
  's~c\.loose_bounds\(\)~c.tight_bounds()~' \
  'c.tight_bounds()' \
  mnema-extract 'a_word_of_the_stored_text_is_boxed_where_the_page_shows_it' --test page_view

# B2. Every char keeps its own box: a word is a box per letter.
case_ "boxes: chars of one line are not merged" \
  $V \
  's~&& r\.0 >= l\.0 =>~&& r.0 >= l.0 && false =>~' \
  '&& r.0 >= l.0 && false =>' \
  mnema-extract 'a_word_of_the_stored_text_is_boxed_where_the_page_shows_it' --test page_view

# B3. All the ranges are folded into one span before they are boxed: two
# neighbouring passages, one cited and one not, become one box with one flag.
case_ "boxes: two ranges are merged into one span" \
  $V \
  's~for range in &req\.ranges \{~let start = req.ranges.iter().map(|r| r.start).min().unwrap_or(0); let end = req.ranges.iter().map(|r| r.start.saturating_add(r.len)).max().unwrap_or(0); for range in \&[RenderRange { start, len: end - start, primary: true }] {~' \
  'for range in &[RenderRange { start, len: end - start' \
  mnema-extract 'ranges_are_boxed_per_line_and_never_merged_with_each_other' --test page_view

# B4/B5. Points turned into pixels by hand: the page's width and height scale
# them, and neither the box's origin nor /Rotate is applied. One mutant, two
# tests, one per thing it forgets.
B_PIXELS='s~(?s)let \(Ok\(\(xa, ya\)\), Ok\(\(xb, yb\)\)\) = \(.*?\) else \{\s*return None;\s*\};~let (sx, sy, ph) = (w as f32 / page.width().value, h as f32 / page.height().value, page.height().value); let (xa, ya, xb, yb) = ((left * sx) as i32, ((ph - top) * sy) as i32, (right * sx) as i32, ((ph - bottom) * sy) as i32);~'

case_ "boxes: the origin of the page box is ignored when points become pixels" \
  $V \
  "$B_PIXELS" \
  '(ph - top) * sy' \
  mnema-extract 'the_box_follows_a_page_whose_box_does_not_start_at_the_origin' --test page_view

case_ "boxes: /Rotate is ignored when points become pixels" \
  $V \
  "$B_PIXELS" \
  '(ph - top) * sy' \
  mnema-extract 'the_box_follows_a_page_shown_rotated' --test page_view

# ── What one request may cost ────────────────────────────────────────────────

# C1. No cap on the width: a request for 5000 px is drawn at 5000.
case_ "cap: the width is not capped" \
  $V \
  's~let width = req\.width_px\.clamp\(1, MAX_WIDTH_PX\);~let width = req.width_px.max(1);~' \
  'req.width_px.max(1)' \
  mnema-extract 'the_png_is_exactly_the_size_reported_and_a_wide_request_is_capped' --test page_view

# C2. No cap on the height: a strip of paper metres long is drawn at its full
# length in rows (pdfium refuses the bitmap, and the strip gets no picture).
case_ "cap: the height is not capped" \
  $V \
  's~\.set_maximum_height\(MAX_HEIGHT_PX as _\)~.set_maximum_height(i32::MAX as _)~' \
  'i32::MAX as _' \
  mnema-extract 'a_page_far_taller_than_wide_is_drawn_within_the_height_cap' --test page_view

# ── What is refused, and by what kind ────────────────────────────────────────

# E1. The magic is not looked at: a PNG is "unreadable" to pdfium, not "not a PDF".
case_ "refusal: a file that is not a PDF is not named as one" \
  $V \
  's~if !bytes\.starts_with\(PDF_MAGIC\) \{~if !bytes.starts_with(PDF_MAGIC) \&\& false {~' \
  'PDF_MAGIC) && false' \
  mnema-extract 'what_is_not_a_page_of_a_pdf_is_refused_by_kind' --test page_view

# E2. Page 0 is a page: `page_no - 1` underflows.
case_ "refusal: page zero is not refused" \
  $V \
  's~req\.page_no == 0 \|\| ~~' \
  'if req.page_no > count {' \
  mnema-extract 'what_is_not_a_page_of_a_pdf_is_refused_by_kind' --test page_view

# ── The worker's line protocol ───────────────────────────────────────────────

# W1. A line that is not a request ends the worker instead of being answered.
case_ "worker: a line that is not a request kills the process" \
  crates/mnema-extract/src/bin/worker.rs \
  's~Err\(e\) => \(\s*RenderError::Unreadable,\s*format!\("not a render request: \{e\}"\),\s*\),~Err(e) => panic!("not a render request: {e}"),~' \
  'panic!("not a render request: {e}")' \
  mnema-extract 'render_page_names_a_non_pdf_and_a_line_that_is_not_a_request' --test worker_cli

# ── The pool: a worker that outlives its deadline ────────────────────────────

# K1. The deadline passes and the worker is left running.
case_ "pool: a worker past its deadline is not killed" \
  crates/mnema-pool/src/lib.rs \
  's~(Ok\(None\) if Instant::now\(\) >= deadline => \{)\s*let _ = child\.kill\(\);\s*let _ = child\.wait\(\);~$1 let _ = \&child;~' \
  'deadline => { let _ = &child;' \
  mnema-pool 'a_worker_that_outlives_the_timeout_is_killed_and_gone' --test render_page

# K2. Killed and not reaped: a zombie is still a process the call left behind.
case_ "pool: a killed worker is not reaped" \
  crates/mnema-pool/src/lib.rs \
  's~(let _ = child\.kill\(\);)\s*let _ = child\.wait\(\);\s*(return Err\(PoolError::RenderTimeout)~$1 $2~' \
  'child.kill(); return Err(PoolError::RenderTimeout' \
  mnema-pool 'a_worker_that_outlives_the_timeout_is_killed_and_gone' --test render_page

# K3. A worker that crashed is read as if it had answered: what it said on
# stderr is lost and the error names an empty stdout.
case_ "pool: a nonzero exit is not a protocol error with the worker's stderr" \
  crates/mnema-pool/src/lib.rs \
  's~if !status\.success\(\) \{~if !status.success() \&\& false {~' \
  'if !status.success() && false {' \
  mnema-pool 'a_crash_and_a_garbled_line_are_protocol_errors_with_what_the_worker_said' --test render_page

# ── The index half ───────────────────────────────────────────────────────────

# I1. The text of the page is not scoped to the document asked for.
case_ "index: a page's text is looked up without the document" \
  crates/mnema-index/src/file_view.rs \
  's~WHERE p\.document_id = \?1 AND p\.page_no = \?2~WHERE p.page_no = ?2~' \
  'WHERE p.page_no = ?2' \
  mnema-index 'a_pages_view_text_is_its_first_block_and_its_source' --test file_view

# I2. Any block of the page is its text, not the first.
case_ "index: a page's text is not its first block" \
  crates/mnema-index/src/file_view.rs \
  's~b\.reading_order = 0\), \x27\x27\)~b.reading_order = 1), \x27\x27)~' \
  'b.reading_order = 1), ' \
  mnema-index 'a_pages_view_text_is_its_first_block_and_its_source' --test file_view

# I3. The view does not say what the file is.
case_ "index: the document view carries no mime" \
  crates/mnema-index/src/file_view.rs \
  's~Ok\(Some\(DocumentViewRow \{\s*mime,~Ok(Some(DocumentViewRow { mime: String::new(),~' \
  'DocumentViewRow { mime: String::new(),' \
  mnema-index 'the_view_carries_the_documents_mime' --test file_view

# ── The application half: what the index decides before the worker is asked ──

AP=src-tauri/src/page_view.rs

# A1. The worker is started for a place the index does not hold: the window's
# own path is opened.
case_ "app: the worker is started when the path is not in the index" \
  $AP \
  's~return unavailable\(PageUnavailable::NoPath\);~let _ = mnema_pool::render_page(worker, \&RenderRequest { path: relative_path.unwrap_or_default().to_string(), page_no, width_px, text_sha256: None, ranges: Vec::new(), recognise: false }, timeout); return unavailable(PageUnavailable::NoPath);~' \
  'relative_path.unwrap_or_default()' \
  mnema-desktop "${AT}a_path_the_index_does_not_hold_is_no_path_and_starts_no_worker" --lib

# A2. The worker is started for a file that is gone.
case_ "app: the worker is started when the file is missing" \
  $AP \
  's~Err\(e\) if e\.kind\(\) == std::io::ErrorKind::NotFound => \{~Err(e) if e.kind() == std::io::ErrorKind::Other => {~' \
  'std::io::ErrorKind::Other => {' \
  mnema-desktop "${AT}a_file_that_is_gone_is_file_missing_and_starts_no_worker" --lib

# A2b. Any stat failure is "missing": a symlink loop or a permission error
# tells the person the file is gone.
case_ "app: a stat that fails for another reason is reported as missing" \
  $AP \
  's~Err\(e\) if e\.kind\(\) == std::io::ErrorKind::NotFound => \{~Err(_) => {~' \
  'Err(_) => {' \
  mnema-desktop "${AT}a_file_that_cannot_be_stated_is_failed_not_missing_and_starts_no_worker" --lib

# A2c. No size gate: the worker reads a file of any size whole.
case_ "app: a file over the PDF ceiling is handed to the worker" \
  $AP \
  's~Ok\(meta\) if meta\.len\(\) > PoolConfig~Ok(meta) if false \&\& meta.len() > PoolConfig~' \
  'Ok(meta) if false && meta.len()' \
  mnema-desktop "${AT}a_file_over_the_pdf_ceiling_is_failed_and_starts_no_worker" --lib

# A2d. The ceiling itself refused: off by one.
case_ "app: a file exactly at the PDF ceiling is refused" \
  $AP \
  's~Ok\(meta\) if meta\.len\(\) > PoolConfig~Ok(meta) if meta.len() >= PoolConfig~' \
  'Ok(meta) if meta.len() >= PoolConfig' \
  mnema-desktop "${AT}a_file_over_the_pdf_ceiling_is_failed_and_starts_no_worker" --lib

# A3. A path that now names another document still sends the hash of the
# window's document: a worker that says Match makes the page "shown".
case_ "app: a reindexed path still sends the hash" \
  $AP \
  's~if occupant\.current_document_id == document_id \{~if true || occupant.current_document_id == document_id {~' \
  'if true || occupant.current_document_id == document_id' \
  mnema-desktop "${AT}a_path_that_now_names_another_document_is_drawn_bare_and_changed" --lib

# A4. A scan page is not recognised: its OCR text is hashed like a native one.
case_ "app: an ocr page is not recognised as a scan" \
  $AP \
  's~source\.starts_with\("ocr:"\)~source.starts_with("ocrx:")~' \
  'source.starts_with("ocrx:")' \
  mnema-desktop "${AT}a_scan_page_is_drawn_bare_and_marked_scan" --lib

# A5. A page the reader skipped is reported as changed.
case_ "app: a skipped page is reported as changed" \
  $AP \
  's~Some\(None\) => \(None, Some\(Highlight::NoText\)\)~Some(None) => (None, Some(Highlight::Changed))~' \
  'Some(None) => (None, Some(Highlight::Changed))' \
  mnema-desktop "${AT}a_page_the_reader_skipped_is_asked_for_without_a_hash" --lib

# A6. The image is passed on without the data: prefix the window sets as src.
case_ "app: the image has no data: prefix" \
  $AP \
  's~image: format!\("data:image/png;base64,\{png_base64\}"\),~image: png_base64,~' \
  'image: png_base64,' \
  mnema-desktop "${AT}a_native_page_that_matches_is_shown_with_the_hash_of_its_stored_text" --lib

# A7. The window's width goes to the worker as sent.
case_ "app: the width is not clamped before the worker" \
  $AP \
  's~width_px: width_px\.min\(MAX_WIDTH_PX\),~width_px: width_px,~' \
  'width_px: width_px,' \
  mnema-desktop "${AT}a_native_page_that_matches_is_shown_with_the_hash_of_its_stored_text" --lib

# A8. The hash is of something other than the page's stored text.
case_ "app: the hash is not of the stored text" \
  $AP \
  's~Some\(sha256_hex\(&text\)\)~Some(sha256_hex(""))~' \
  'Some(sha256_hex(""))' \
  mnema-desktop "${AT}a_native_page_that_matches_is_shown_with_the_hash_of_its_stored_text" --lib

# A9. Start and length swapped on the way to the worker.
case_ "app: a range's length is taken from its start" \
  $AP \
  's~len: r\.length,~len: r.block_start,~' \
  'len: r.block_start,' \
  mnema-desktop "${AT}a_native_page_that_matches_is_shown_with_the_hash_of_its_stored_text" --lib

# A10. A match is not "shown".
case_ "app: a match is not shown" \
  $AP \
  's~TextMatch::Match => Highlight::Shown,~TextMatch::Match => Highlight::Changed,~' \
  'TextMatch::Match => Highlight::Changed,' \
  mnema-desktop "${AT}a_native_page_that_matches_is_shown_with_the_hash_of_its_stored_text" --lib

# A11. A mismatch is "shown": the window draws boxes the worker refused.
case_ "app: a mismatch or an absent text is shown" \
  $AP \
  's~TextMatch::Mismatch \| TextMatch::None => Highlight::Changed,~TextMatch::Mismatch | TextMatch::None => Highlight::Shown,~' \
  'TextMatch::None => Highlight::Shown,' \
  mnema-desktop "${AT}a_text_that_differs_or_is_absent_is_changed_not_shown" --lib

# A12. A passage the worker could not place is reported as a changed file.
case_ "app: unplaced is reported as changed" \
  $AP \
  's~TextMatch::Unplaced => Highlight::Unplaced,~TextMatch::Unplaced => Highlight::Changed,~' \
  'TextMatch::Unplaced => Highlight::Changed,' \
  mnema-desktop "${AT}a_passage_the_worker_could_not_place_is_unplaced_not_changed" --lib

# P1. The request is written on the calling thread: a worker that never reads
# holds the call past its deadline.
case_ "pool: the request write blocks the deadline" \
  crates/mnema-pool/src/lib.rs \
  's~std::thread::spawn\(move \|\| stdin\.write_all\(line\.as_bytes\(\)\)\);~let _ = std::thread::spawn(move || stdin.write_all(line.as_bytes())).join();~' \
  '.write_all(line.as_bytes())).join();' \
  mnema-pool 'a_worker_that_never_reads_its_request_still_times_out' --test render_page

# Mutation cases for the boxes of a scanned page's citation (a second Vision
# recognition at display time): the pure functions behind it, the verdict on
# what was recognised, the app's handling of a scan page. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/scan-highlight.sh
#
# What is silent when it is wrong here is a box on the wrong words, or on a page
# whose text is no longer what the citation counted in: the window draws
# whatever boxes it is given. Every case below runs on the Linux leg; the ones
# that need Vision are in `scan-highlight-macos.sh`, which nothing runs
# automatically.
#
# The native branch has its own cases in `pdf-page-view.sh` (A1-A12); the ones
# here break only what a scan adds: lines that carry `scan`, `recognise` or
# `judge_recognised`.

O=crates/mnema-extract/src/ocr.rs
V=crates/mnema-extract/src/page_view.rs
OT='ocr::tests::'
VT='page_view::tests::'
AP=src-tauri/src/page_view.rs
AT='page_view::tests::'

# ── Vision's offsets and boxes: the pure functions ───────────────────────────

# U1. A char outside the BMP is one UTF-16 unit, not two: every range after it
# on the line is boxed one char to the left.
case_ "utf16: a char outside the BMP counts as one unit" \
  $O \
  's~\.map\(char::len_utf16\)~.map(|_| 1usize)~' \
  '.map(|_| 1usize)' \
  mnema-extract "${OT}a_char_outside_the_bmp_before_the_range_shifts_utf16_by_two" --lib

# U2. A range that ends on the newline gives the next line an empty slice, not
# a box of nothing.
case_ "utf16: an empty slice of a line is a range" \
  $O \
  's~if from >= to \{~if from > to {~' \
  'if from > to {' \
  mnema-extract "${OT}a_range_ending_on_the_newline_gives_the_next_line_nothing" --lib

# F1. Vision's origin is at the bottom left; the window's at the top left.
case_ "to_frac: y is not flipped" \
  $O \
  's~y: \(1\.0 - origin_y - h\) as f32,~y: origin_y as f32,~' \
  'y: origin_y as f32,' \
  mnema-extract "${OT}a_vision_box_is_flipped_to_an_origin_at_the_top" --lib

# F2. Flipped from the box's bottom edge, not its top.
case_ "to_frac: y is flipped without the height" \
  $O \
  's~y: \(1\.0 - origin_y - h\) as f32,~y: (1.0 - origin_y) as f32,~' \
  'y: (1.0 - origin_y) as f32,' \
  mnema-extract "${OT}a_vision_box_is_flipped_to_an_origin_at_the_top" --lib

# ── The verdict on what was recognised ───────────────────────────────────────

# J1. The hash of the recognised text is not compared: any page is boxed with
# the offsets of another.
case_ "judge: the hash is not compared" \
  $V \
  's~if sha256_hex\(&boxed\.text\) != \*wanted \{~if false \&\& sha256_hex(\&boxed.text) != *wanted {~' \
  'if false && sha256_hex(&boxed.text) != *wanted {' \
  mnema-extract "${VT}a_recognised_page_is_boxed_only_as_the_text_it_was_indexed_in" --lib

# J2. A different Vision is not a changed file: the wrong hash is Unplaced.
case_ "judge: a hash that differs is a mismatch" \
  $V \
  's~!= \*wanted \{\n        return \(TextMatch::Unplaced,~!= *wanted {\n        return (TextMatch::Mismatch,~' \
  '        return (TextMatch::Mismatch, Vec::new());' \
  mnema-extract "${VT}a_recognised_page_is_boxed_only_as_the_text_it_was_indexed_in" --lib

# J3. No hash sent is "nothing to compare", not a page that does not agree.
case_ "judge: no hash is unplaced" \
  $V \
  's~(fn judge_recognised\(boxed: Boxed, req: &RenderRequest\) -> \(TextMatch, Vec<RenderRect>\) \{\n    let Some\(wanted\) = &req\.text_sha256 else \{\n        return \(TextMatch::)None,~${1}Unplaced,~' \
  'let Some(wanted) = &req.text_sha256 else {
        return (TextMatch::Unplaced,' \
  mnema-extract "${VT}a_recognised_page_is_boxed_only_as_the_text_it_was_indexed_in" --lib

# J4. Text NFC would rewrite is boxed: its offsets are of the rewritten text.
case_ "judge: text NFC would rewrite is not refused" \
  $V \
  's~if matches!\(nfc::normalise\(&boxed\.text\), ~if false \&\& matches!(nfc::normalise(\&boxed.text), ~' \
  'if false && matches!(nfc::normalise(&boxed.text), ' \
  mnema-extract "${VT}text_nfc_would_rewrite_or_a_page_of_whitespace_is_never_boxed" --lib

# J5. A page of whitespace is compared like any other (and its hash agrees).
case_ "judge: a page of whitespace is compared" \
  $V \
  's~if boxed\.text\.chars\(\)\.all\(char::is_whitespace\) \{~if false \&\& boxed.text.chars().all(char::is_whitespace) {~' \
  'if false && boxed.text.chars().all(char::is_whitespace) {' \
  mnema-extract "${VT}text_nfc_would_rewrite_or_a_page_of_whitespace_is_never_boxed" --lib

# J6. A citation Vision gives no box for reads as "nothing cited here".
case_ "judge: a range without a box is not unplaced" \
  $V \
  's~if boxes\.is_empty\(\) && range\.len > 0 \{~if false \&\& boxes.is_empty() \&\& range.len > 0 {~' \
  'if false && boxes.is_empty() && range.len > 0 {' \
  mnema-extract "${VT}a_range_without_a_box_or_a_missing_list_leaves_the_page_unplaced" --lib

# J7. Fewer lists than ranges: the zip below stops at the shorter and the rest
# of the citation is dropped without a word.
case_ "judge: fewer lists than ranges is not unplaced" \
  $V \
  's~if boxed\.ranges\.len\(\) != req\.ranges\.len\(\) \{~if false \&\& boxed.ranges.len() != req.ranges.len() {~' \
  'if false && boxed.ranges.len() != req.ranges.len() {' \
  mnema-extract "${VT}a_range_without_a_box_or_a_missing_list_leaves_the_page_unplaced" --lib

# ── The wire ─────────────────────────────────────────────────────────────────

# R1. The field has a default: a worker older than the app, or a request built
# by hand, is read as "no second recognition" instead of being refused.
case_ "render: recognise has a serde default" \
  crates/mnema-core/src/render.rs \
  's~\n    pub recognise: bool,~\n    #[serde(default)]\n    pub recognise: bool,~' \
  '#[serde(default)]
    pub recognise: bool,' \
  mnema-core 'render::tests::a_request_without_recognise_is_refused' --lib

# ── The application: what a scan page is asked for and told ──────────────────

# S1. A scan page is asked for like a native one: no second recognition.
case_ "app: a scan page is asked for without a second recognition" \
  $AP \
  's~        recognise: scan,~        recognise: false,~' \
  '        recognise: false,' \
  mnema-desktop "${AT}a_scan_page_is_asked_for_with_the_hash_of_its_text_and_a_second_recognition" --lib

# S2. A scan page is asked for with no hash of its stored text.
case_ "app: a scan page is asked for without a hash" \
  $AP \
  's~Some\(Some\(\(text, source\)\)\) => \(Some\(sha256_hex\(&text\)\), None, source\.starts_with\("ocr:"\)\),~Some(Some((text, source))) => (if source.starts_with("ocr:") { None } else { Some(sha256_hex(\&text)) }, None, source.starts_with("ocr:")),~' \
  'if source.starts_with("ocr:") { None } else' \
  mnema-desktop "${AT}a_scan_page_is_asked_for_with_the_hash_of_its_text_and_a_second_recognition" --lib

# S3. Every page is asked for with a second recognition.
case_ "app: every page is asked for with a second recognition" \
  $AP \
  's~        recognise: scan,~        recognise: true,~' \
  '        recognise: true,' \
  mnema-desktop "${AT}a_native_page_that_matches_is_shown_with_the_hash_of_its_stored_text" --lib

# S4. A path that now names another document is asked for with a second
# recognition: the hash it sends is not the window's, so there is none to send.
case_ "app: another document's path is recognised as a scan" \
  $AP \
  's~None => \(None, Some\(Highlight::Changed\), false\),~None => (None, Some(Highlight::Changed), true),~' \
  'None => (None, Some(Highlight::Changed), true),' \
  mnema-desktop "${AT}a_path_that_now_names_another_document_is_not_recognised_even_for_a_scan" --lib

# S5. No engine for a scan is the native note.
case_ "app: a scan with no engine is changed" \
  $AP \
  's~TextMatch::None if scan => Highlight::Scan,~TextMatch::None if false => Highlight::Scan,~' \
  'TextMatch::None if false => Highlight::Scan,' \
  mnema-desktop "${AT}each_answer_for_a_scan_page_has_its_own_note" --lib

# S6. A second recognition that disagrees is the native note: a changed file.
case_ "app: a scan the worker could not place is changed" \
  $AP \
  's~TextMatch::Unplaced => Highlight::Unplaced,~TextMatch::Unplaced => if scan { Highlight::Changed } else { Highlight::Unplaced },~' \
  'TextMatch::Unplaced => if scan { Highlight::Changed }' \
  mnema-desktop "${AT}each_answer_for_a_scan_page_has_its_own_note" --lib

# S7. The same for a hash that differs: a different Vision is not a changed file.
case_ "app: a scan whose hash differs is changed" \
  $AP \
  's~TextMatch::Mismatch if scan => Highlight::Unplaced,~TextMatch::Mismatch if scan => Highlight::Changed,~' \
  'TextMatch::Mismatch if scan => Highlight::Changed,' \
  mnema-desktop "${AT}each_answer_for_a_scan_page_has_its_own_note" --lib

# D1. A scan page is held to the native deadline: Vision's cold start is cut
# short and the path stays cold.
case_ "app: a scan page gets the native deadline" \
  $AP \
  's~if scan \{ scan_timeout \} else \{ timeout \}~timeout~' \
  'render_page(worker, &request, timeout)' \
  mnema-desktop "${AT}a_scan_page_is_given_the_longer_deadline" --lib

# D2. A native page waits the scan's deadline: a stuck worker holds the window.
case_ "app: a native page gets the scan deadline" \
  $AP \
  's~if scan \{ scan_timeout \} else \{ timeout \}~scan_timeout~' \
  'render_page(worker, &request, scan_timeout)' \
  mnema-desktop "${AT}a_scan_page_is_given_the_longer_deadline" --lib

# D3. The scan deadline is not the pool's own: a worker the pool lets run is
# cut short here, or the other way round.
case_ "app: the scan deadline is not the pool's" \
  $AP \
  's~const RENDER_TIMEOUT_SCAN: Duration = Duration::from_secs\(300\);~const RENDER_TIMEOUT_SCAN: Duration = Duration::from_secs(301);~' \
  'Duration::from_secs(301)' \
  mnema-desktop "${AT}the_scan_deadline_is_the_pools_own" --lib

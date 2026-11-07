# Mutation cases for the boxes of a scanned page's citation whose tests need
# Vision. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/scan-highlight-macos.sh
#
# not-in-matrix: every test named here is #[cfg(target_os = "macos")] (Vision is the oracle); on the ubuntu leg the baseline selects 0 tests and the whole file exits 1 — the same reason as parallel-ocr-macos.sh. Vision's cold start under a new binary path is ~46 s: the first test run of each target directory waits.
#
# Not here, on purpose: `setUsesLanguageCorrection` that differs between a
# plain and a boxed recognition survives (measured 2026-11-07: the fixtures
# read the same either way). The recognition level is case V5 below, and dies
# of an error, not of a different text -- see its comment.

O=crates/mnema-extract/src/ocr.rs
V=crates/mnema-extract/src/page_view.rs
VT='ocr::vision_tests::'

# M1. The second recognition is not asked for: the field is read and ignored.
case_ "worker: recognise is ignored" \
  $V \
  's~let \(text, rects\) = if req\.recognise \{~let (text, rects) = if false \&\& req.recognise {~' \
  'if false && req.recognise {' \
  mnema-extract 'recognise_false_on_a_scan_behaves_as_before_the_field_existed' --test page_view

# M2. Vision reads the bitmap that is shown, not the one indexing read: at a
# width too small to read, the hash no longer agrees.
case_ "worker: the display bitmap is recognised, not render_for_ocr's" \
  $V \
  's~let image = match render_for_ocr\(page, req\.page_no\) \{~let image = match page.render_with_config(\&PdfRenderConfig::new().set_target_width(req.width_px as _).set_format(PdfBitmapFormat::BGRA)).map(|b| { let bgra = b.as_raw_bytes(); let (width, height) = (b.width() as u32, b.height() as u32); crate::ocr::OwnedPageImage { stride: bgra.len() / (height.max(1) as usize), bgra, width, height, dpi: 72 } }) {~' \
  'set_target_width(req.width_px as _)' \
  mnema-extract 'a_scan_page_is_boxed_when_vision_repeats_the_text_indexing_stored' --test page_view

# M3. An engine that failed is Unplaced, not "no engine": the note says the
# page moved, when it is only unread.
case_ "worker: an OCR error is unplaced" \
  $V \
  's~OCR: \{e\}", req\.page_no\);\n            return \(TextMatch::None,~OCR: {e}", req.page_no);\n            return (TextMatch::Unplaced,~' \
  '            return (TextMatch::Unplaced, Vec::new());
        }
    };
    judge_recognised' \
  mnema-extract 'render_page_boxes_a_scan_and_without_the_engine_draws_it_unboxed' --test worker_cli

# V1. The `\n` between two lines is not counted: every range on a later line
# is boxed one char early.
case_ "vision: the newline between lines is not counted" \
  $O \
  's~line_start \+= line\.chars\(\)\.count\(\) \+ 1;~line_start += line.chars().count();~' \
  'line_start += line.chars().count();' \
  mnema-extract "${VT}an_offset_is_exact_to_the_char_at_the_edge_of_a_word" --lib

# V2. A range starts one char late.
case_ "vision: a range starts one char late" \
  $O \
  's~                            range\.start as usize,~                            range.start as usize + 1,~' \
  'range.start as usize + 1,' \
  mnema-extract "${VT}an_offset_is_exact_to_the_char_at_the_edge_of_a_word" --lib

# V3. Vision answers a lone space with a box of size zero; kept, it is a box
# in the corner that makes an unplaceable range look placed.
case_ "vision: a box of size zero is kept" \
  $O \
  's~if b\.size\.width <= 0\.0 \|\| b\.size\.height <= 0\.0 \{~if false {~' \
  'if false {' \
  mnema-extract "${VT}an_offset_is_exact_to_the_char_at_the_edge_of_a_word" --lib

# V4. A box does not carry its range's `primary`.
case_ "vision: a box loses its range's primary flag" \
  $O \
  's~(b\.size\.height,\n\s+)range\.primary,~${1}false,~' \
  'false,
                        ));' \
  mnema-extract "${VT}a_range_is_boxed_where_its_words_are" --lib

# V5. A boxed recognition asks Vision for another level than the plain one.
# `Fast` has no Ukrainian: the request fails ("uk-UA is not among the supported
# languages"), the worker answers `None`, and the hash test sees no `Match`.
# A level that read the fixtures differently but without an error would
# survive; none is known.
case_ "vision: a boxed recognition uses another level" \
  $O \
  's~request\.setRecognitionLevel\(VNRequestTextRecognitionLevel::Accurate\);~request.setRecognitionLevel(if ranges.is_empty() { VNRequestTextRecognitionLevel::Accurate } else { VNRequestTextRecognitionLevel::Fast });~' \
  'VNRequestTextRecognitionLevel::Fast' \
  mnema-extract 'a_scan_page_is_boxed_when_vision_repeats_the_text_indexing_stored' --test page_view

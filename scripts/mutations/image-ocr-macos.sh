# macOS-only sibling of `image-ocr.sh`: the cases whose oracle is ImageIO or
# Apple Vision. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/image-ocr-macos.sh
#
# not-in-matrix: every test named here is #[cfg(target_os = "macos")] or reaches the code under test only through ImageIO; on the ubuntu leg the baseline selects 0 tests, or a picture is refused before any frame is decoded and the mutant is never reached — the same reason as scan-ocr-macos.sh
#
# **Nothing runs this file automatically**, for the reason `scan-ocr-macos.sh`
# gives: the `mutations` job has no macOS leg, and a new one would be a 21st job
# against the free plan's ceiling of 20. `scripts/mutation-staleness.sh` still
# proves on every sweep that each case here APPLIES.
#
# **Cost: up to a minute per case that reaches Vision.** Its cold start is paid
# once per executable path (`ocr.rs`), and every case builds its test binary
# afresh in the harness's worktree.

W=crates/mnema-extract/src/bin/worker.rs
I=crates/mnema-extract/src/image.rs

# O1. The engine not running answered as `not_text`: a verdict about the file,
# journalled, hidden by the folders tree, and kept past the engine's repair
# (D57). Killed at `tests/worker_cli.rs:2589` (the engine made unavailable by
# its seam, so no Vision in the test).
case_ "worker: an engine that did not run fails a picture and does not refuse it" \
  $W \
  's#                Err\(ImageReadError::Ocr\(e\)\) => vec!\[Frame::Failed \{\n                    message: format!\("a picture could not be recognised: \{e\}"\),\n                \}\],#                Err(ImageReadError::Ocr(e)) => not_text(sha256, e.to_string()),#' \
  '                Err(ImageReadError::Ocr(e)) => not_text(sha256, e.to_string()),' \
  mnema-extract 'an_unavailable_engine_fails_a_picture_rather_than_refusing_it' --test worker_cli

# ── The redraw Vision reads ──────────────────────────────────────────────────

# W1. No white under the frame: a transparent PNG of black text is black on
# black. Killed at `tests/image.rs:76`.
case_ "image: a frame is drawn onto white" \
  $I \
  's~            CGContext::fill_rect\(Some\(&ctx\), rect\);\n~~' \
  '            CGContext::set_rgb_fill_color(Some(&ctx), 1.0, 1.0, 1.0, 1.0);
            CGContext::draw_image(' \
  mnema-extract 'macos::a_transparent_picture_is_drawn_on_white' --test image

# W2. The frame's own pixel format instead of the BGRA redraw: an 8-bit gray
# context, its bytes handed on as BGRA. Vision reads such a bitmap as zero lines
# without an error (`ocr.rs`). Killed at `tests/image.rs:155` (Vision).
case_ "image: a frame is redrawn as BGRA, not left in its own format" \
  $I \
  's~(let space = CGColorSpace::)new_device_rgb\(\)(.*?let info =\n\s+)CGImageAlphaInfo::PremultipliedFirst\.0 [|] CGImageByteOrderInfo::Order32Little\.0;~${1}new_device_gray()${2}CGImageAlphaInfo::None.0;~s' \
  'CGImageAlphaInfo::None.0;' \
  mnema-extract 'macos::a_gray_picture_reads_as_text_after_the_redraw' --test image

# W3. No redraw at all: the context stays white and nothing is drawn into it.
# Killed at `tests/image.rs:155` (Vision); the transparency test alone passes it.
case_ "image: the frame is drawn into the bitmap" \
  $I \
  's~            CGContext::draw_image\(Some\(&ctx\), rect, Some\(&image\)\);\n~~' \
  '            CGContext::fill_rect(Some(&ctx), rect);
            drop(ctx);' \
  mnema-extract 'macos::a_gray_picture_reads_as_text_after_the_redraw' --test image

# ── Which frames are pages ───────────────────────────────────────────────────

# P1. An animation's frames counted as pages: an APNG of 50 frames is the same
# text 50 times, and 50 pages. Killed at `tests/image.rs:111`.
case_ "image: an animation is its first frame" \
  $I \
  's~        if paged \{ frames \} else \{ frames\.min\(1\) \}~        if paged { frames } else { frames }~' \
  'if paged { frames } else { frames }' \
  mnema-extract 'macos::an_animated_png_is_its_first_frame_only' --test image

# P2. TIFF out of the paged types: a fax or a scan batch is its first page.
# Killed at `tests/image.rs:92`.
case_ "image: a multi-page TIFF is a page per frame" \
  $I \
  's~const PAGED_TYPES: \[&str; 2\] = \["public\.tiff", "public\.heics"\];~const PAGED_TYPES: [\&str; 1] = ["public.heics"];~' \
  'const PAGED_TYPES: [&str; 1] = ["public.heics"];' \
  mnema-extract 'macos::a_multi_frame_tiff_gives_each_frame_its_own_size' --test image

# ── The threshold a frame's text must pass ───────────────────────────────────

# TH1. Every recognised frame a page: a photo with nothing legible is a
# document of one empty page instead of `not_text`, and shows in the tree.
# Killed at `tests/worker_cli.rs:2545` (Vision on `image-blank.jpg`).
case_ "image: a frame with nothing legible is not a page" \
  $I \
  's~            Some\(read\) if has_text_layer\(text_layer_chars\(&read\.text\)\) => \{~            Some(read) if true => {~' \
  '            Some(read) if true => {' \
  mnema-extract 'a_picture_without_text_is_refused_as_not_text_by_the_image_reader' --test worker_cli

# Known survivor, deliberately not a case: `!read.text.trim().is_empty()` in
# place of the threshold (any text a page, below the 48 characters a PDF page
# needs) stays green. No fixture holds legible text under 48 characters: the
# TIFF's blank frame and `image-blank.jpg` read as nothing at all, so TH1 is the
# only side of the threshold a test sees. Closing it needs a short-text picture
# in `make_scan_fixtures.swift`.

# ── The page view draws the frame it boxes ───────────────────────────────────

# R1. Shown from the first frame, boxed from the frame asked for: page 2 of a
# TIFF draws page 1 under page 2's boxes. Review of task 9, R1; killed at
# `tests/page_view.rs:708`.
case_ "view: the frame shown is the frame recognised" \
  crates/mnema-extract/src/page_view.rs \
  's~    let \(rgba, w, h\) = shown\(&image, req\.width_px\);~    let (rgba, w, h) = shown(\&frames.frame(0).unwrap(), req.width_px);~' \
  '    let (rgba, w, h) = shown(&frames.frame(0).unwrap(), req.width_px);' \
  mnema-extract 'a_page_of_a_tiff_is_its_own_frame_and_past_the_last_is_no_page' --test page_view

# Mutation cases for pictures read for their text (the image reader), the part
# that runs everywhere. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/image-ocr.sh
#
# Every test named here runs on the Linux leg: the magic is plain bytes, the
# citation is the parent's, the migration is SQL, and the two page-view
# ceilings and the scaling are code that never calls ImageIO. The cases whose
# only oracle is Apple Vision or ImageIO are in `image-ocr-macos.sh`.
#
# Cases written elsewhere and not repeated here: the image reader's spelling of
# `not_text` and its digest (`refuse-by-content.sh`), the PDF arm of
# `pages_of` by name (`task-5.sh` C1), and the launcher drawing a picture as its
# page (`pdf-page-view-ui.sh` I1–I3).

TY=crates/mnema-extract/src/typing.rs
MG=crates/mnema-index/src/migrations.rs
MT='migrations::tests::only_whole_file_not_text_rows_are_aged_and_none_is_deleted'
AP=src-tauri/src/page_view.rs
AT='page_view::tests::a_picture_is_held_to_the_ceiling_of_a_file_that_is_not_a_pdf'
V=crates/mnema-extract/src/page_view.rs

# ── What a picture is, by its bytes ──────────────────────────────────────────

# T1. The magic asked only after `classify`: every picture has a NUL in its
# head, so a PNG or a JPEG is `not_text` again — the answer every photo got
# before this reader. Killed at `tests/typing.rs:192`.
case_ "typing: a picture is decided by its magic before classify" \
  $TY \
  's~    if let Some\(mime\) = image_mime\(bytes\) \{~    if let Some(mime) = image_mime(bytes).filter(|_| matches!(classify(bytes), Verdict::Text)) {~' \
  'image_mime(bytes).filter(|_| matches!(classify(bytes), Verdict::Text))' \
  mnema-extract 'each_picture_format_is_recognized_by_its_magic' --test typing

# T2. HEIF without its brand: `ftyp` is MP4's and MOV's too, so every video is
# handed to OCR as a picture. Killed at `tests/typing.rs:218`.
case_ "typing: HEIF is decided by its major brand" \
  $TY \
  's~\.is_some_and\([|]brand[|] HEIF_BRANDS\.iter\(\)\.any\([|]b[|] brand == \*b\)\)~.is_some()~' \
  '            .is_some()' \
  mnema-extract 'an_mp4_is_not_a_heif_picture' --test typing

# T3. HEIF without the box size's two zero bytes: a note that begins
# "abcdftypheic" is a picture, and its text is refused. Killed at
# `tests/typing.rs:261`.
case_ "typing: HEIF begins with a box size, not with text" \
  $TY \
  's~    \} else if bytes\.get\(0\.\.2\) == Some\(&\[0, 0\]\)\n        && bytes\.get\(4\.\.8\) == Some\(b"ftyp"\)~    } else if bytes.get(4..8) == Some(b"ftyp")~' \
  '    } else if bytes.get(4..8) == Some(b"ftyp")' \
  mnema-extract 'text_that_begins_like_a_magic_is_still_text' --test typing

# ── What a picture's chunk cites ─────────────────────────────────────────────

# C1. `pages_of` without the image reader: a frame's chunk cites lines, which a
# picture does not have, and the viewer opens no frame. The narrow oracle is the
# unit test (`lib.rs:1614`)…
case_ "ingest: a picture's page cites its page number" \
  crates/mnema-ingest/src/lib.rs \
  's~                READER_PDF [|] READER_IMAGE => PageContext::Fixed~                READER_PDF => PageContext::Fixed~' \
  '                READER_PDF => PageContext::Fixed' \
  mnema-ingest 'pages_of_tests::an_image_page_cites_its_page_number_and_a_text_page_still_cites_lines' --lib

# C2. …and the same mutant against the randomised harness, whose invariant 8
# (`tests/randomised.rs`, `check_a_picture_cites_its_frame`) judges every chunk
# of every picture the corpus reads, page numbers included. This case is the
# evidence that the invariant is load-bearing, not only written.
case_ "harness: invariant 8 sees a picture cited by lines" \
  crates/mnema-ingest/src/lib.rs \
  's~                READER_PDF [|] READER_IMAGE => PageContext::Fixed~                READER_PDF => PageContext::Fixed~' \
  '                READER_PDF => PageContext::Fixed' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# ── Migration 11: the photos journalled `not_text` are asked again ───────────
#
# All three are killed by the one test, `migrations.rs:905`, which holds a
# page's `not_text`, a whole file's under a name that is not a picture's
# (`clip.dat`), other rules beside them, and every field of every row.

# MG1. Page rows aged too: a page's verdict is not the file's, and the cheap
# arm never reads one.
case_ "migration: only whole-file not_text rows are aged" \
  $MG \
  "s~WHERE rule = 'not_text' AND page_no IS NULL;\";~WHERE rule = 'not_text';\";~" \
  "\"UPDATE skipped SET format_version = 0 WHERE rule = 'not_text';\";" \
  mnema-index "$MT" --lib

# MG2. `DELETE` instead of `UPDATE`: the settings tree hides a file by its row
# whatever the version, so every photo, video and database shows as an unread
# file until the walk reaches it.
case_ "migration: the aged row is kept, not deleted" \
  $MG \
  's~"UPDATE skipped SET format_version = 0 WHERE~"DELETE FROM skipped WHERE~' \
  '"DELETE FROM skipped WHERE rule' \
  mnema-index "$MT" --lib

# MG3. Narrowed to picture names: a photo is a picture by its bytes, and a
# `.dat` (or a `.HEIC`, or an extensionless file) journalled `not_text` would
# stay hidden for the life of the index. Review of task 8, case E.
case_ "migration: the rows are not narrowed by the file's name" \
  $MG \
  "s~WHERE rule = 'not_text' AND page_no IS NULL;\";~WHERE rule = 'not_text' AND page_no IS NULL AND lower(relative_path) GLOB '*.[jp][pn]g';\";~" \
  "AND lower(relative_path) GLOB '*.[jp][pn]g';" \
  mnema-index "$MT" --lib

# ── The page view: which ceiling, and how a frame is shown ───────────────────
#
# M1–M3 (task 10's report): the app holds a file to the ceiling ingest holds it
# to, decided by the bytes on disk now. One test, three assertions
# (`src-tauri/src/page_view.rs:764`), each killing one.

# M1. The PDF ceiling for everything: a 100 MiB photo ingest never read is
# handed to the worker whole ("a picture over its ceiling").
case_ "app: a picture is not held to the PDF ceiling" \
  $AP \
  's~    Ok\(if head\.starts_with\(PDF_MAGIC\) \{~    Ok(if true {~' \
  '    Ok(if true {' \
  mnema-desktop "$AT" --lib

# M2. `max_bytes` for everything: a scanned book ingest read is refused its
# page ("a PDF the same size").
case_ "app: a PDF is held to the PDF ceiling" \
  $AP \
  's~    Ok\(if head\.starts_with\(PDF_MAGIC\) \{~    Ok(if false {~' \
  '    Ok(if false {' \
  mnema-desktop "$AT" --lib

# M3. Decided by the name: `doc.pdf` now holding a picture is held to the PDF
# ceiling ("a PDF's path that now holds a picture").
case_ "app: the ceiling is decided by the bytes, not the name" \
  $AP \
  's~    Ok\(if head\.starts_with\(PDF_MAGIC\) \{~    Ok(if path.extension().is_some_and(|e| e == "pdf") {~' \
  '    Ok(if path.extension().is_some_and(|e| e == "pdf") {' \
  mnema-desktop "$AT" --lib

# S1. `shown` without the worker's width ceiling: a request for 5000 px of a
# wide frame is drawn at 5000. Killed at `page_view.rs:486`.
case_ "view: a picture is shown no wider than MAX_WIDTH_PX" \
  $V \
  's~let target = u64::from\(width_px\.clamp\(1, MAX_WIDTH_PX\)\);~let target = u64::from(width_px.max(1));~' \
  'let target = u64::from(width_px.max(1));' \
  mnema-extract 'page_view::tests::a_picture_wider_than_the_ceiling_is_shown_at_the_ceiling' --lib

# S2. The mean over the box's first row only: a thin horizontal stroke vanishes
# when 4000 px become 800. Killed at `page_view.rs:470`.
case_ "view: a scaled pixel is the mean of every row it covers" \
  $V \
  's~let \(y0, y1\) = \(oy \* sh / h, \(\(oy \+ 1\) \* sh / h\)\.max\(oy \* sh / h \+ 1\)\);~let (y0, y1) = (oy * sh / h, oy * sh / h + 1);~' \
  'let (y0, y1) = (oy * sh / h, oy * sh / h + 1);' \
  mnema-extract 'page_view::tests::a_picture_scaled_down_averages_every_row_it_covers' --lib

# Known survivors, deliberately not cases (each would be green):
#
#   * `ceiling_of` without `.take(PDF_MAGIC.len())` (`src-tauri/src/page_view.rs`):
#     the whole file is read for its magic. Same answer, up to 512 MiB in the UI
#     process per page shown; sparse fixtures read fast and a FIFO only times
#     out. Review of task 10, nit 2 (MD).
#   * `ceiling_of`'s `Err(e)` branch passed through: needs an unreadable file,
#     which root on CI reads anyway; the worker answers `Failed` too. Review of
#     task 10, A (MB).
#   * `starts_with(PDF_MAGIC)` → `starts_with(b"%PDF")`: no fixture starts
#     `%PDF` without the dash; one constant is the guard. Review of task 10, MA.

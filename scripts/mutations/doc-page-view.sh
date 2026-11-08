# Mutation cases for the page view of a Markdown file (D199): the worker reads
# the file again, renders it as allow-listed HTML and marks a citation only when
# the file still hashes to the index's blocks. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/doc-page-view.sh
#
# What is silent when it is wrong here is a highlight on the wrong words, or
# markup from the file reaching the window: the page shows whatever HTML it is
# handed, and a stale mark looks exactly like a right one. Every `case_` below
# has one mutation and one test. Expressions use `~` as the delimiter and
# `\Q…\E` for the literal Rust they replace.

MD=crates/mnema-extract/src/doc_view/markdown.rs
SA=crates/mnema-extract/src/doc_view/sanitize.rs
DV=src-tauri/src/doc_view.rs
DP=ui/src/launcher/DocPage.svelte
LA=ui/src/launcher/Launcher.svelte
MT='doc_view::markdown::tests::'
ST='doc_view::sanitize::tests::'
AT='doc_view::tests::'

# ── Whether marks may be drawn at all ────────────────────────────────────────

# H1. The hash is never compared: a file edited since indexing is marked with
# offsets counted in other text.
case_ "hash: the file's blocks are not compared with the hash the offsets were counted in" \
  $MD \
  's~\QSome(sent) if *sent != hash =>\E~Some(sent) if false && *sent != hash =>~' \
  'if false && *sent != hash' \
  mnema-extract "${MT}a_changed_file_is_rendered_without_marks" --lib

# H2. A file holding a mark character itself is marked anyway: its own
# character opens a mark where no citation is.
case_ "marks: a file that already holds a mark character is not refused" \
  $MD \
  's~\QSome(_) if tree_has_marks(root) =>\E~Some(_) if false && tree_has_marks(root) =>~' \
  'if false && tree_has_marks(root)' \
  mnema-extract "${MT}a_file_holding_a_mark_character_is_not_marked" --lib

# H3. The marks of an unmarked render are drawn: the file's own character is a
# highlight even though the render refused to place any.
case_ "marks: an unmarked render still turns mark characters into <mark>" \
  $MD \
  's~\Qsanitize(&one, text == TextMatch::Match)\E~sanitize(&one, true)~' \
  'sanitize(&one, true)' \
  mnema-extract "${MT}a_file_holding_a_mark_character_is_not_marked" --lib

# H4. A range that could not be placed is ignored: the others are marked and
# the page reads as one whose citation is all there.
case_ "unplaced: a range that marked nothing still counts as placed" \
  $MD \
  's~\Qhit.iter().all(|&h| h)\E~hit.iter().all(|_| true)~' \
  'hit.iter().all(|_| true)' \
  mnema-extract "${MT}a_range_that_cannot_be_placed_marks_nothing_at_all" --lib

# ── The allow-list ───────────────────────────────────────────────────────────

# S1. `script`, `style` and `iframe` are unwrapped like any unknown tag: their
# content reaches the page as text.
case_ "allow-list: the elements that give no text keep their content" \
  $SA \
  's~\Qif skipping > 0 || gives_no_text(element) || skip(element) {\E~if skipping > 0 || skip(element) {~' \
  'if skipping > 0 || skip(element) {' \
  mnema-extract "${ST}scripts_handlers_styles_and_urls_do_not_get_through" --lib

# S2. Any class is kept: a file restyles the window's own marks or labels.
case_ "allow-list: a class that is not a code class is kept" \
  $SA \
  's~\Qclass == "syntax-highlighting" || tail("sx-") || tail("language-")\E~!tail("").then_some(()).is_some() || !class.is_empty()~' \
  '!class.is_empty()' \
  mnema-extract "${ST}text_and_attributes_are_escaped" --lib

# S3. The mark state does not cross an element: a citation through `<strong>`
# loses everything after the tag.
case_ "marks: the state inside a mark does not cross into the next text node" \
  $SA \
  's~\Qwrite_text(&mut out, text, &mut mark)\E~write_text(&mut out, text, &mut None)~' \
  'write_text(&mut out, text, &mut None)' \
  mnema-extract "${ST}a_mark_crossing_an_element_is_one_mark_per_piece_of_text" --lib

# ── The application's request ────────────────────────────────────────────────

# A1. A span of another document is sent as a span of this one.
case_ "app: a span naming another document's block is sent" \
  $DV \
  's~\Q.find(|b| b.block_id == s.block_id)?;\E~.find(|b| b.block_id == s.block_id).or(view.blocks.first())?;~' \
  '.or(view.blocks.first())?' \
  mnema-desktop "${AT}a_span_of_another_document_is_not_sent" --lib

# A2. A path that names another document now is sent this one's hash and spans.
case_ "app: another document at the path is hashed and marked as this one" \
  $DV \
  's~\Qlet (text_sha256, ranges) = if ours {\E~let (text_sha256, ranges) = if ours || true {~' \
  'if ours || true {' \
  mnema-desktop "${AT}a_path_that_names_another_document_now_is_rendered_bare" --lib

# ── The window ───────────────────────────────────────────────────────────────

# W1. An older answer is painted over a newer one.
case_ "window: a slow answer for an older draw paints over the newer one" \
  $DP \
  's~\Q    if (id !== request) return;\E\n(\Q    // An answer ends a failure\E)~$1~' \
  '    // An answer ends a failure' \
  src/launcher/DocPage.test.ts 'a slow answer for an older draw does not paint over the newer one' runner=vitest

# W2. A Markdown file is not offered its page.
case_ "window: the choice is offered for a PDF only" \
  $LA \
  's~\Qnew Set(['"'"'application/pdf'"'"', '"'"'text/markdown'"'"'])\E~new Set(['"'"'application/pdf'"'"'])~' \
  "new Set(['application/pdf'])" \
  src/launcher/LauncherPdf.test.ts 'a Markdown file is offered the choice too, and its page is the HTML view' runner=vitest

# ── Placing a citation in code (review, PR #95) ──────────────────────────────

# P1. An end at column 0 is read literally: an indented code block followed by
# a blank line has no span, and every mark of the render is withdrawn.
case_ "place: an end at column 0 is not read as the end of the previous line" \
  $MD \
  's~\Qlet b = if at.end.column == 0 {\E~let b = if false {~' \
  'let b = if false {' \
  mnema-extract "${MT}an_indented_code_block_followed_by_a_blank_line_is_marked" --lib

# P2. A code block's literal is looked for only with its line breaks flattened:
# a multi-line fence never matches and a citation of one word marks it whole.
case_ "place: a multi-line code literal is not found in its own source" \
  $MD \
  's~\Qonce(slice).or_else(\E~None.or_else(~' \
  'None.or_else(' \
  mnema-extract "${MT}a_partial_citation_in_a_multi_line_fence_marks_only_itself" --lib

# W3. A failure outlives a later answer: the page never comes back.
case_ "window: a failed draw is final" \
  $DP \
  's~\Q    failed = false;\E\n\Q    got = answer;\E~    got = answer;~' \
  '    got = answer;' \
  src/launcher/DocPage.test.ts 'a failed draw is not final: a later answer replaces the failure' runner=vitest

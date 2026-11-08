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
DT=src/launcher/DocPage.test.ts
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

# ── A book, one chapter at a time, and an HTML file (D199) ──────────────────

# E1. The whole book is hashed: the worker hashes one chapter, so every
# citation in a book reads as changed text.
case_ "book: every chapter's blocks go into the hash" \
  $DV \
  's~\Q.filter(|b| shown(b.page_no))\E~.filter(|b| shown(b.page_no) || true)~' \
  'shown(b.page_no) || true' \
  mnema-desktop "${AT}a_book_is_drawn_at_the_chapter_of_its_citation_hashed_and_marked_alone" --lib

# E2. A span in another chapter is sent: its offsets name a block the worker
# never reads, or one of this chapter's at the same place.
case_ "book: a span of another chapter is sent with this one" \
  $DV \
  's~\Qif !shown(block.page_no) {\E~if false && !shown(block.page_no) {~' \
  'if false && !shown(block.page_no) {' \
  mnema-desktop "${AT}a_book_is_drawn_at_the_chapter_of_its_citation_hashed_and_marked_alone" --lib

# E3. The chapter the window asks for loses to the citation's: turning a page
# draws the cited chapter again.
case_ "book: the chapter asked for is overruled by the citation's" \
  $DV \
  's~\Qmatch page_no.or_else(cited).or_else(first) {\E~match cited().or(page_no).or_else(first) {~' \
  'match cited().or(page_no)' \
  mnema-desktop "${AT}a_chapter_asked_for_wins_over_the_citation_s_and_its_spans_stay_home" --lib

# E4. The citation does not choose the chapter: a book opens at its first
# chapter with the clicked passage elsewhere.
case_ "book: the cited chapter is not where the book opens" \
  $DV \
  's~\Qmatch page_no.or_else(cited).or_else(first) {\E~match page_no.or_else(first) {~' \
  'match page_no.or_else(first) {' \
  mnema-desktop "${AT}a_book_is_drawn_at_the_chapter_of_its_citation_hashed_and_marked_alone" --lib

# E5. A chapter with no indexed text is sent: the worker refuses it, and the
# window says the render failed instead of naming the chapter.
case_ "book: a chapter the index holds no text in is sent to the worker" \
  $DV \
  's~\QSome(p) if view.blocks.iter().any(|b| b.page_no as u32 == p) => Some(p),\E~Some(p) => Some(p),~' \
  'Some(p) => Some(p),' \
  mnema-desktop "${AT}a_chapter_with_no_indexed_text_is_not_sent_to_the_worker" --lib

# E6. The reply does not say which chapter is shown: the window cannot tell
# where the book is open.
case_ "book: the reply leaves out the chapter it shows" \
  $DV \
  's~(\Q            },\E\n\Q            page_no: \E)page,~${1}None,~' \
  '            page_no: None,' \
  mnema-desktop "${AT}a_book_is_drawn_at_the_chapter_of_its_citation_hashed_and_marked_alone" --lib

# E7. An EPUB is not drawn as HTML.
case_ "reader: a book's mime has no reader" \
  $DV \
  's~\Q        "application/epub+zip" => Some(DocReader::Epub),\E\n~~' \
  '        "text/html" => Some(DocReader::Html),
        _ => None,' \
  mnema-desktop "${AT}a_book_is_drawn_at_the_chapter_of_its_citation_hashed_and_marked_alone" --lib

# E8. An HTML file is not drawn as HTML.
case_ "reader: an HTML file's mime has no reader" \
  $DV \
  's~\Q        "text/html" => Some(DocReader::Html),\E\n~~' \
  '        "text/markdown" => Some(DocReader::Markdown),
        "application/epub+zip"' \
  mnema-desktop "${AT}an_html_file_is_drawn_whole_with_the_hash_of_all_its_blocks" --lib

# E9. An HTML file is cut to a chapter like a book: its other pages' blocks
# leave the hash and every citation in it reads as changed.
case_ "reader: a whole file is drawn as one chapter" \
  $DV \
  's~\Qlet page = if reader == DocReader::Epub {\E~let page = if reader != DocReader::Markdown {~' \
  'if reader != DocReader::Markdown {' \
  mnema-desktop "${AT}an_html_file_is_drawn_whole_with_the_hash_of_all_its_blocks" --lib

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
  's~\Qnew Set(['"'"'application/pdf'"'"', ...DOC_PAGE_MIMES])\E~new Set(['"'"'application/pdf'"'"'])~' \
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

# ── An EPUB's chapters ───────────────────────────────────────────────────────

# W4. An HTML file is not drawn as a page: it falls through to the text.
case_ "window: an HTML file is not one the page view draws" \
  $DP \
  's~\Q'"'"'text/markdown'"'"', '"'"'text/html'"'"', \E~'"'"'text/markdown'"'"', ~' \
  "new Set(['text/markdown', 'application/epub+zip'])" \
  src/launcher/LauncherPdf.test.ts 'an HTML file is offered the choice, and its page is the HTML view' runner=vitest

# W5. Next is offered on the last chapter.
case_ "chapters: next is not disabled on the last chapter" \
  $DP \
  's~\Qdisabled={at >= chapters.length - 1}\E~disabled={false}~' \
  'disabled={false} onclick={() => go(1)}' \
  $DT 'next and previous ask for the neighbour chapter; there is no next after the last' runner=vitest

# W6. Previous is offered on the first chapter.
case_ "chapters: previous is not disabled on the first chapter" \
  $DP \
  's~\Qdisabled={at <= 0}\E~disabled={false}~' \
  'disabled={false} onclick={() => go(-1)}' \
  $DT 'there is no previous chapter on the first' runner=vitest

# W7. A button asks for the chapter on the page again, not its neighbour.
case_ "chapters: a button draws the chapter it is on" \
  $DP \
  's~\Qchapters[at + step]\E~chapters[at]~' \
  'const next = chapters[at];' \
  $DT 'next and previous ask for the neighbour chapter; there is no next after the last' runner=vitest

# W8. A refresh lets the backend pick: the person is thrown back to the cited chapter.
case_ "chapters: a refresh does not pass the chapter on the page" \
  $DP \
  's~\Qvoid draw(place, spans, reveal, chapter);\E~void draw(place, spans, reveal);~' \
  'void draw(place, spans, reveal);' \
  $DT 'the window coming back draws the chapter on the page, not the cited one' runner=vitest

# W9. Every file gets chapters: a whole HTML file shows a bar for its one page.
case_ "chapters: shown for a file that is not an EPUB" \
  $DP \
  's~\Qview.mime === '"'"'application/epub+zip'"'"'\E\n~true\n~' \
  'const chapters = $derived(true' \
  $DT 'a Markdown or HTML file has no chapters' runner=vitest

# W10. A slow chapter is painted over the one the person moved on to.
case_ "chapters: an older chapter's answer paints over the newer one" \
  $DP \
  's~\Q    if (id !== request) return;\E\n(\Q    // An answer ends a failure\E)~$1~' \
  '    // An answer ends a failure' \
  $DT 'a slow chapter is not painted once the person has moved on' runner=vitest

# W11. A chapter without the citation opens where the last one was scrolled to.
case_ "chapters: a chapter with no mark keeps the old scroll" \
  $DP \
  's~\Q    else scroller.scrollTop = 0;\E\n~~' \
  "    if (target) scrollWithin(scroller, target, 'center');
  }" \
  $DT 'a chapter without the citation is shown from its top' runner=vitest

# W12. A chapter with no indexed text says nothing.
case_ "chapters: noChapter has no message of its own" \
  $DP \
  's~\Q      case '"'"'noChapter'"'"': return t('"'"'page_unavailable_no_chapter'"'"');\E\n~~' \
  "      case 'fileMissing': return t('page_unavailable_missing');
      case 'tooLarge': return t('page_unavailable_too_large');
      case 'noPath'" \
  $DT 'a chapter with no indexed text says so and offers the text' runner=vitest

# W13. An EPUB is not drawn as a page: it falls through to the text.
case_ "window: an EPUB is not one the page view draws" \
  $DP \
  's~\Q, '"'"'application/epub+zip'"'"']);\E~]);~' \
  "new Set(['text/markdown', 'text/html'])" \
  src/launcher/LauncherPdf.test.ts 'an EPUB is offered the choice, and its page is the HTML view' runner=vitest

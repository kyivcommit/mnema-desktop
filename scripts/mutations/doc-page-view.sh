# Mutation cases for the page view of a Markdown, HTML or EPUB file (D199): the worker reads
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
HD=crates/mnema-extract/src/doc_view/html_doc.rs
DM=crates/mnema-extract/src/doc_view/mod.rs
HT='doc_view::html_doc::tests::'

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

# ── HTML files and EPUB chapters (html_doc.rs) ───────────────────────────────

# X1. A block NFC rewrote is marked with offsets that count other text.
case_ "html view: a block NFC rewrote is marked with offsets that count other text" \
  $HD \
  's~\Qif read.unverbatim.contains(&key) {\E~if false && read.unverbatim.contains(&key) {~' \
  'if false && read.unverbatim.contains(&key) {' \
  mnema-extract "${HT}a_block_nfc_rewrote_is_not_marked" --lib

# X2. A chapter is hashed under page 1, so it never matches its own index rows.
case_ "html view: a chapter is hashed under page 1, so it never matches its own index rows" \
  $HD \
  's~\Qc.blocks.into_iter().map(|b| (page_no, b))\E~c.blocks.into_iter().map(|b| (1, b))~' \
  'c.blocks.into_iter().map(|b| (1, b))' \
  mnema-extract "${HT}a_chapter_is_marked_with_its_own_hash_and_not_the_books" --lib

# X3. A chapter's <title>, which the index holds no block for, reaches the view.
case_ "html view: a chapter's <title>, which the index holds no block for, reaches the view" \
  $HD \
  's~\Qnames_the_document,\E(\s+limit,\s+&mut images,)~|_| false,$1~' \
  'req,
        |_| false,' \
  mnema-extract "${HT}a_chapter_title_is_not_shown_and_an_svg_title_is" --lib

# X4. Every title is skipped, so an SVG title the index holds vanishes from the view.
case_ "html view: every title is skipped, so an SVG title the index holds vanishes from the view" \
  $HD \
  's~\Qnames_the_document,\E(\s+limit,\s+&mut images,)~|e| e.name() == "title",$1~' \
  '|e| e.name() == "title",' \
  mnema-extract "${HT}a_chapter_title_is_not_shown_and_an_svg_title_is" --lib

# X5. An HTML file's <title>, a block of the index, is dropped from the view.
case_ "html view: an HTML file's <title>, a block of the index, is dropped from the view" \
  $HD \
  's~\Qrender(read, pages, req, |_| false, limit,\E~render(read, pages, req, names_the_document, limit,~' \
  'render(read, pages, req, names_the_document, limit,' \
  mnema-extract "${HT}an_html_title_is_shown_and_can_be_cited" --lib

# X6. A neighbouring citation is drawn as the clicked one.
case_ "html view: a neighbouring citation is drawn as the clicked one" \
  $HD \
  's~\QSome(false) => MARK_OTHER,\E~Some(false) => MARK_PRIMARY,~' \
  'Some(false) => MARK_PRIMARY,' \
  mnema-extract "${HT}a_neighbour_is_marked_without_primary_and_each_range_in_its_own_block" --lib

# X7. A node that only touches a range at its edge is marked.
case_ "html view: a node that only touches a range at its edge is marked" \
  $HD \
  's~\Qif mine(r) && r.start < y && x < r.start + r.len {\E~if mine(r) && r.start <= y && x < r.start + r.len {~' \
  'if mine(r) && r.start <= y && x < r.start + r.len {' \
  mnema-extract "${HT}a_cited_phrase_crossing_an_element_is_marked_and_the_text_is_intact" --lib

# X8. A range marks every block that has the same offsets.
case_ "html view: a range marks every block that has the same offsets" \
  $HD \
  's~\Qif mine(r) && r.start < y\E~if r.start < y~' \
  'if r.start < y' \
  mnema-extract "${HT}a_neighbour_is_marked_without_primary_and_each_range_in_its_own_block" --lib

# X9. A range ending exactly at its block end is refused.
case_ "html view: a range ending exactly at its block end is refused" \
  $HD \
  's~\Qend as usize > chars)\E~end as usize >= chars)~' \
  'end as usize >= chars)' \
  mnema-extract "${HT}a_range_that_cannot_be_placed_marks_nothing_at_all" --lib

# X10. A range one past its block end is placed.
case_ "html view: a range one past its block end is placed" \
  $HD \
  's~\Qend as usize > chars)\E~end as usize > chars + 1)~' \
  'end as usize > chars + 1)' \
  mnema-extract "${HT}a_range_that_cannot_be_placed_marks_nothing_at_all" --lib

# X11. A range that marked nothing still counts as placed.
case_ "html view: a range that marked nothing still counts as placed" \
  $HD \
  's~\Qhit.iter().all(|&h| h)\E~hit.iter().all(|_| true)~' \
  'hit.iter().all(|_| true)' \
  mnema-extract "${HT}a_range_that_cannot_be_placed_marks_nothing_at_all" --lib

# X12. A file already holding a mark character is marked anyway.
case_ "html view: a file already holding a mark character is marked anyway" \
  $HD \
  's~\QSome(_) if tree_has_marks(&read.doc) =>\E~Some(_) if false && tree_has_marks(&read.doc) =>~' \
  'Some(_) if false && tree_has_marks(&read.doc) =>' \
  mnema-extract "${HT}a_file_holding_a_mark_character_is_not_marked" --lib

# X13. An unmarked render draws the file's own mark characters.
case_ "html view: an unmarked render draws the file's own mark characters" \
  $HD \
  's~\Qtext == TextMatch::Match,\E(\s+skip,)~true,$1~' \
  'true,
        skip,' \
  mnema-extract "${HT}a_file_holding_a_mark_character_is_not_marked" --lib

# X14. The hash is never compared: a changed file is marked.
case_ "html view: the hash is never compared: a changed file is marked" \
  $HD \
  's~\QSome(sent) if *sent != hash =>\E~Some(sent) if false && *sent != hash =>~' \
  'Some(sent) if false && *sent != hash =>' \
  mnema-extract "${HT}a_changed_file_is_rendered_without_marks" --lib

# X15. A request with no hash is treated as a match.
case_ "html view: a request with no hash is treated as a match" \
  $HD \
  's~\QNone => TextMatch::None,\E~None if req.ranges.is_empty() => TextMatch::None,\n        None => TextMatch::Match,~' \
  'None => TextMatch::Match,' \
  mnema-extract "${HT}no_hash_is_no_comparison" --lib

# X16. HTML over the limit reaches the window.
case_ "html view: HTML over the limit reaches the window" \
  $HD \
  's~\Qif html.len() - image_bytes > limit {\E~if html.len() - image_bytes > limit + 1 {~' \
  'if html.len() - image_bytes > limit + 1 {' \
  mnema-extract "${HT}too_much_html_is_refused" --lib

# X17. A chapter with no blocks is drawn as an empty page.
case_ "html view: a chapter with no blocks is drawn as an empty page" \
  $HD \
  's~\Qif c.blocks.is_empty() {\E~if false && c.blocks.is_empty() {~' \
  'if false && c.blocks.is_empty() {' \
  mnema-extract "${HT}a_chapter_that_is_not_one_or_is_not_named_is_refused" --lib

# X18. An EPUB request naming no chapter draws chapter 1.
case_ "html view: an EPUB request naming no chapter draws chapter 1" \
  $HD \
  's~\Qlet Some(page_no) = req.page_no else {\E~let Some(page_no) = req.page_no.or(Some(1)) else {~' \
  'let Some(page_no) = req.page_no.or(Some(1)) else {' \
  mnema-extract "${HT}a_chapter_that_is_not_one_or_is_not_named_is_refused" --lib

# X19. The page count is not the length of the spine.
case_ "html view: the page count is not the length of the spine" \
  $HD \
  's~(render\(\s+read,\s+)\Qsource.spine_len,\E~${1}1,~' \
  'read,
        1,' \
  mnema-extract "${HT}a_chapter_is_marked_with_its_own_hash_and_not_the_books" --lib

# X20. The worker hands an HTML file to the Markdown renderer.
case_ "html view: the worker hands an HTML file to the Markdown renderer" \
  $DM \
  's~\QDocReader::Html => html_doc::render_html(&bytes, req),\E~DocReader::Html => markdown::render_markdown(&bytes, req),~' \
  'DocReader::Html => markdown::render_markdown(&bytes, req),' \
  mnema-extract "render_doc_answers_an_html_file_with_its_citation_marked" --test worker_cli

# X21. A book's `<a id="…"/>` anchor is drawn as a link: the parser leaves it
# open, so everything after it reads as one underlined link.
case_ "allow-list: an anchor without a target is drawn as a link" \
  $SA \
  's~\Qlet anchor = element.name() == "a" && element.attr("href").is_none();\E~let anchor = false;~' \
  'let anchor = false;' \
  mnema-extract "${ST}an_anchor_without_a_target_is_text_not_a_link" --lib

# ── An EPUB chapter's pictures ───────────────────────────────────────────────

EP=crates/mnema-extract/src/epub.rs

# X22. Anything a chapter names is shown as a PNG: an SVG, which can carry
# script, or a text file reaches the window as a picture.
case_ "pictures: a member that is no known picture is shown anyway" \
  $HD \
  's~\Qlet mime = image_type(&data)?;\E~let mime = image_type(&data).unwrap_or("image/png");~' \
  'let mime = image_type(&data).unwrap_or("image/png");' \
  mnema-extract "${HT}a_picture_is_its_type_by_its_bytes_not_its_name" --lib

# X23. A picture's type is taken from its name: a PNG called `.jpg` is sent
# as a JPEG.
case_ "pictures: the type is the file name's, not the bytes'" \
  $HD \
  's~\Qlet mime = image_type(&data)?;\E~let mime = if src.ends_with(".jpg") { "image/jpeg" } else { image_type(&data)? };~' \
  'let mime = if src.ends_with(".jpg") { "image/jpeg" } else { image_type(&data)? };' \
  mnema-extract "${HT}a_picture_is_its_type_by_its_bytes_not_its_name" --lib

# X24. A chapter's pictures have no budget: every one is inlined, however many.
case_ "pictures: the chapter's budget is not enforced" \
  $HD \
  's~\Qlet mut left = image_budget;\E~let mut left = usize::MAX;~' \
  'let mut left = usize::MAX;' \
  mnema-extract "${HT}a_picture_past_its_cap_or_the_chapters_budget_is_its_alt" --lib

# X25. A spent budget is refunded: a smaller picture after the one that did not
# fit is shown, out of reading order.
case_ "pictures: a later, smaller picture is shown after the budget ran out" \
  $HD \
  's~\Qleft = 0;\E~left += 0;~' \
  'left += 0;' \
  mnema-extract "${HT}a_picture_past_its_cap_or_the_chapters_budget_is_its_alt" --lib

# X26. One picture has no cap: a member of any size is read and inlined.
case_ "pictures: the per-picture cap is ignored" \
  $HD \
  's~\Qchapter_image(bytes, &source.path, src, image_cap)\E~chapter_image(bytes, &source.path, src, usize::MAX)~' \
  'chapter_image(bytes, &source.path, src, usize::MAX)' \
  mnema-extract "${HT}a_picture_past_its_cap_or_the_chapters_budget_is_its_alt" --lib

# X27. A shown picture keeps the file's own attributes: `onerror`, `srcset`.
case_ "pictures: the element's attributes reach the window" \
  $SA \
  's~\Qlet _ = write!(out, "<img src=\"{uri}\" alt=\"{alt}\">");\E~let attrs: String = element.attrs().map(|(n, v)| format!(" {n}=\\"{v}\\"")).collect(); let _ = write!(out, "<img src=\\"{uri}\\" alt=\\"{alt}\\"{attrs}>");~' \
  'let attrs: String = element.attrs()' \
  mnema-extract "${HT}a_shown_picture_keeps_nothing_of_its_element_but_alt" --lib

# X28. A quote in alt text closes the attribute: the rest of the alt is markup.
case_ "pictures: a quote in alt text is not escaped" \
  $SA \
  's~\Qlet alt = text.replace('"'"'"'"'"', "&quot;");\E~let alt = text;~' \
  'let alt = text;' \
  mnema-extract "${HT}a_chapters_pictures_are_shown_from_its_own_folder" --lib

# X29. The pictures count against the markup's limit: a chapter heavy with
# pictures is refused as too large.
case_ "pictures: picture bytes are counted in the markup limit" \
  $HD \
  's~\Qif html.len() - image_bytes > limit {\E~if html.len() > limit {~' \
  'if html.len() > limit {' \
  mnema-extract "${HT}a_chapter_heavy_with_pictures_is_marked_and_not_too_large" --lib

# X30. An HTML file's `<img src>` is inlined: a file's src names the disk, and
# the view would hand the window whatever the hook makes of it.
case_ "pictures: an HTML file's picture is shown" \
  $HD \
  's~\Qrender(read, pages, req, |_| false, limit, &mut |_| None)\E~render(read, pages, req, |_| false, limit, &mut |s| Some(s.to_string()))~' \
  'render(read, pages, req, |_| false, limit, &mut |s| Some(s.to_string()))' \
  mnema-extract "${HT}an_html_files_picture_stays_its_alt" --lib

# X31. A picture's src is resolved against the chapter's own path, not its
# folder: `../Images/a.jpg` names a member beside the chapter.
case_ "pictures: src resolves against the chapter file, not its folder" \
  $EP \
  's~\Qresolve(&parent_of(chapter_path), src)\E~resolve(chapter_path, src)~' \
  'resolve(chapter_path, src)' \
  mnema-extract "${HT}a_chapters_pictures_are_shown_from_its_own_folder" --lib

# X32. A src with a scheme is read as a path: `http://…` names whatever member
# happens to be spelled that way.
case_ "pictures: a src with a URL scheme is read from the archive" \
  $EP \
  's~\Qif scheme {\E~if false && scheme {~' \
  'if false && scheme {' \
  mnema-extract "${HT}a_picture_outside_the_archive_or_not_in_it_is_its_alt" --lib

# W14. The label counts spine positions: a book with a cover says "3 of 3" on
# its second readable chapter.
case_ "chapters: the label counts spine numbers, not chapters that can be read" \
  $DP \
  's~\Q{ n: at + 1, m: chapters.length }\E~{ n: chapter ?? 0, m: chapters.length }~' \
  '{ n: chapter ?? 0, m: chapters.length }' \
  $DT 'a chapter with no text is not counted and is stepped over' runner=vitest

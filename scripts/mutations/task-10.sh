# Mutation cases for Task 10: the HTML reader, and the rebuild the reader
# comparison owed. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-10.sh
#
# HTML is the one format this product answered *wrongly* rather than refusing,
# so this file is about two different silences:
#
#   * markup reaching a chunk — CSS, JavaScript, and the raw `<p>…</p>` that
#     four fallback elements hand back as a single text node;
#   * prose reaching nothing — an element the traversal did not expect, a
#     boundary rule inverted, a run cleared instead of flushed. Nothing goes red
#     downstream when a paragraph is missing: the document is still there,
#     shorter.
#
# And one that is neither, found while wiring this reader up and worth more than
# the reader: a file whose reader changed hands was re-read and then **not
# rebuilt**, so the html reader would have changed no document already in an
# index (C22–C25).

# ------------------------------------------------------ the reader's own name

# C1. The name is one symbol across a process boundary and across D40. `pages_of`
# matches it to cite an HTML chunk as `Coordinate::Section`; one character off
# falls to `PageContext::Lines`, which asks blocks that carry no line numbers
# for a line range and answers `Coordinate::None`. Green everywhere else.
case_ "worker: the header names the html reader, not a near-miss" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                reader: manifest::READER_HTML\.to_string\(\),}{                reader: "html-2".to_string(),}' \
  'reader: "html-2".to_string(),' \
  mnema-extract 'an_html_file_is_read_as_prose_and_its_header_names_the_html_reader' --test worker_cli

# C2. The version, which is what decides whether a document already in the index
# was made by today's code — and, since C22 below, whether it is replaced.
case_ "worker: the header carries the html reader's version" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                reader_version: manifest::HTML_READER_VERSION,}{                reader_version: 0,}' \
  'reader_version: 0,' \
  mnema-extract 'an_html_file_is_read_as_prose_and_its_header_names_the_html_reader' --test worker_cli

# C3. `native:html` names the reader, not the file. Text that came out of an
# HTML parse is not the same evidence as text that came out of a plain-text
# read of the same bytes — which for this format is exactly the difference
# between prose and markup.
case_ "worker: the summary names the html text source" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                text_source: "native:html"\.to_string\(\),}{                text_source: "native:txt".to_string(),}' \
  'text_source: "native:txt".to_string(),
            });
            frames
        }' \
  mnema-extract 'an_html_file_is_read_as_prose_and_its_header_names_the_html_reader' --test worker_cli

# ---------------------------------------------------------------- the manifest

# C4. The `htm` spelling dropped. `identify_plain_text` matches both, so a map
# carrying one predicts the *text* reader for every `.htm` on disk: the file is
# handed to a worker on every walk, for ever, and the row is rewritten to the
# value it already had.
case_ "manifest: both spellings of the extension are claimed" \
  crates/mnema-extract/src/manifest.rs \
  's{    for extension in \["html", "htm"\] \{}{    for extension in ["html"] \{}' \
  'for extension in ["html"] {' \
  mnema-extract 'the_worker_states_which_reader_takes_each_extension' --test manifest

# C5. The entry pointing at the wrong reader. This map is a claim about
# `typing::identify`, and the claim being false is the one thing it exists not
# to be: it would predict `text@1` for a file the worker now reads as html, and
# `text@1 == text@1` answers "unchanged" for the life of the index.
case_ "manifest: the html entry names the reader identify really picks" \
  crates/mnema-extract/src/manifest.rs \
  's{            ReaderId::new\(READER_HTML, HTML_READER_VERSION\),}{            ReaderId::new("text", TEXT_READER_VERSION),}' \
  'ReaderId::new("text", TEXT_READER_VERSION),
        );
    }' \
  mnema-extract 'the_manifest_names_the_reader_that_identify_actually_picks' --test manifest

# C6. The other side of the same pair, in `identify`: `.htm` sent back to the
# text reader while the manifest still claims it for html. Same cost, opposite
# file.
case_ "typing: identify takes both spellings, as the manifest claims" \
  crates/mnema-extract/src/typing.rs \
  's{        Some\("html"\) \| Some\("htm"\) => \("text/html", SourceKind::Document, Reader::Html\),}{        Some("html") => ("text/html", SourceKind::Document, Reader::Html),}' \
  'Some("html") => ("text/html", SourceKind::Document, Reader::Html),' \
  mnema-extract 'the_manifest_names_the_reader_that_identify_actually_picks' --test manifest

# ------------------------------------------------- what gives no text at all

# C7. The measurement in spec §2.1, put back: `<style>` reaching a chunk means a
# search hits `color:red` and a citation highlights CSS.
case_ "reader: a stylesheet is not prose" \
  crates/mnema-extract/src/html.rs \
  's{        "script" \| "style" \| "noscript" \| "iframe" \| "noembed" \| "noframes" \| "template"}{        "script" | "noscript" | "iframe" | "noembed" | "noframes" | "template"}' \
  '"script" | "noscript" | "iframe" | "noembed" | "noframes" | "template"' \
  mnema-extract 'script_and_style_do_not_become_prose' --test html

# C8. And the other half of the same sentence, which needs its own case: a list
# that dropped `style` alone satisfies nothing about `script`.
case_ "reader: a script is not prose" \
  crates/mnema-extract/src/html.rs \
  's{        "script" \| "style" \| "noscript" \| "iframe" \| "noembed" \| "noframes" \| "template"}{        "style" | "noscript" | "iframe" | "noembed" | "noframes" | "template"}' \
  '"style" | "noscript" | "iframe" | "noembed" | "noframes" | "template"' \
  mnema-extract 'script_and_style_do_not_become_prose' --test html

# C9. **The four that are not obvious, and the reason the list is longer than
# the brief's.** `<noscript>`, `<iframe>`, `<noembed>` and `<noframes>` are
# parsed as raw text, so each arrives as one text node holding literal markup —
# `"<p>увімкніть JS</p>"`, tags and all. A reader that excluded only script and
# style puts that in a chunk, which is the same defect in a new place.
case_ "reader: fallback content that arrives as raw markup is not prose either" \
  crates/mnema-extract/src/html.rs \
  's{        "script" \| "style" \| "noscript" \| "iframe" \| "noembed" \| "noframes" \| "template"}{        "script" | "style"}' \
  '        "script" | "style"
    )' \
  mnema-extract 'fallback_content_that_arrives_as_raw_markup_is_not_prose_either' --test html

# C10. The skip list narrowed to the HTML namespace. `<svg><script>` is real and
# its content came back as text — measured — so this list matches on the name in
# any namespace, unlike `<title>`'s test in C12.
case_ "reader: a script inside svg is still a script" \
  crates/mnema-extract/src/html.rs \
  's{fn gives_no_text\(element: &Element\) -> bool \{\n    matches!\(}{fn gives_no_text(element: \&Element) -> bool \{\n    if element.name.ns != ns!(html) \{\n        return false;\n    \}\n    matches!(}' \
  'if element.name.ns != ns!(html) {
        return false;
    }
    matches!(' \
  mnema-extract 'a_script_inside_svg_is_still_a_script_and_svg_text_is_still_text' --test html

# ---------------------------------------------------------------- the section

# C11. `<title>` dropped from the elements that open a section. `pages_of` cites
# an HTML chunk as `Coordinate::Section` and renders an unnamed page as the
# empty string, so a document with no headings — a report, a mail export —
# would cite nothing at all. Spec §6 invariant 1.
case_ "reader: a document with no heading is still named by its title" \
  crates/mnema-extract/src/html.rs \
  's{            "h1" \| "h2" \| "h3" \| "h4" \| "h5" \| "h6" \| "title"\n        \)\n\}}{            "h1" | "h2" | "h3" | "h4" | "h5" | "h6"\n        )\n\}}' \
  '"h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        )
}' \
  mnema-extract 'a_document_with_no_heading_is_named_by_its_title' --test html

# C12. The namespace test on the other side. SVG has a `<title>` of its own and
# it is a tooltip: without this, a diagram's mouseover names the section every
# citation on the page points at — non-empty, plausible, and wrong.
case_ "reader: an svg title is a tooltip, not a section" \
  crates/mnema-extract/src/html.rs \
  's{fn opens_a_section\(element: &Element\) -> bool \{\n    element\.name\.ns == ns!\(html\)\n        && matches!\(}{fn opens_a_section(element: \&Element) -> bool \{\n    matches!(}' \
  'fn opens_a_section(element: &Element) -> bool {
    matches!(' \
  mnema-extract 'an_svg_title_does_not_name_a_section' --test html

# C13. A heading with no text at all naming a section. A page titled by the
# empty string renders as a section that exists and has no name, which is
# indistinguishable from a reader with a hole in it — the distinction
# `slice.rs::a_page_that_names_no_section_carries_an_empty_one_rather_than_none`
# is built on.
case_ "reader: an empty heading names nothing" \
  crates/mnema-extract/src/markdown.rs \
  's{    if flattened\.is_empty\(\) \{\n        return None;\n    \}}{    if false \{\n        return None;\n    \}}' \
  '    if false {
        return None;
    }' \
  mnema-extract 'a_heading_with_no_text_does_not_open_a_section' --test html

# C14. The page a heading opens when page 1 is still empty and unnamed. Always
# pushing leaves an untitled page with no blocks at the front and numbers every
# real section one higher than it is — a `page_no` that cites the reader's
# bookkeeping instead of the document.
case_ "reader: a document that begins with a heading has no empty page in front" \
  crates/mnema-extract/src/html.rs \
  's{    let empty_and_unnamed = pages\n        \.last\(\)\n        \.is_some_and\(\|page\| page\.blocks\.is_empty\(\) && page\.section_title\.is_none\(\)\);}{    let empty_and_unnamed = false;}' \
  'let empty_and_unnamed = false;' \
  mnema-extract 'a_heading_opens_a_section' --test html

# ------------------------------------------------------------- the partition

# C15. **The class the whole traversal is shaped around**, expressed as the
# opposite default: only elements this file has heard of end a run, and
# everything else is inline. Nothing is lost, and two paragraphs separated by a
# web component are stored as one block whose text runs the last word of one
# into the first word of the other — words that are in no document.
#
# Written the first time as a change to the *opening* arm alone, and it stayed
# green — correctly: the closing arm still asked `is_inline`, so every run was
# still flushed at the right place and the reader's output did not move. A case
# that will not go red is worse than no case, so this one inverts the default in
# `is_inline` itself, where both arms read it.
case_ "reader: an element nobody enumerated ends a run rather than joining one" \
  crates/mnema-extract/src/html.rs \
  's{fn is_inline\(element: &Element\) -> bool \{\n    matches!\(}{fn is_inline(element: \&Element) -> bool \{\n    if !matches!(element.name(), "p" \| "div" \| "li" \| "td" \| "th" \| "html" \| "head" \| "body") \{\n        return true;\n    \}\n    matches!(}' \
  'if !matches!(element.name(), "p" | "div" | "li" | "td" | "th" | "html" | "head" | "body") {
        return true;
    }' \
  mnema-extract 'prose_inside_an_element_nobody_enumerated_is_still_read' --test html

# C16. The run cleared instead of flushed at a closing boundary — the shape of
# every "prose reached nothing" defect. Nothing downstream says a paragraph is
# missing: the document is there, shorter.
case_ "reader: a run ending at a closing tag becomes a block" \
  crates/mnema-extract/src/html.rs \
  's{                    flush\(&mut run, &flow, &mut pages\);\n                    flow\.pop\(\);}{                    run.clear();\n                    flow.pop();}' \
  'run.clear();
                    flow.pop();' \
  mnema-extract 'every_word_the_page_would_show_lands_in_exactly_one_block' --test html

# C17. …and the same at an opening boundary, which is where text that sits
# directly under `<body>` or `<section>` before its first child element is lost.
case_ "reader: a run ending at an opening tag becomes a block" \
  crates/mnema-extract/src/html.rs \
  's{                        flush\(&mut run, &flow, &mut pages\);\n                        flow\.push\(block_type\(element\)\);}{                        run.clear();\n                        flow.push(block_type(element));}' \
  'run.clear();
                        flow.push(block_type(element));' \
  mnema-extract 'every_word_the_page_would_show_lands_in_exactly_one_block' --test html

# C18. `<br>` treated as the inline element it technically is. It means a line
# break, so joining across it stores `першийдругий` — a word in no file, and one
# a search for either half will not find.
case_ "reader: a line break does not glue two words together" \
  crates/mnema-extract/src/html.rs \
  's{        "a" \| "abbr"}{        "br" \| "a" \| "abbr"}' \
  '"br" | "a" | "abbr"' \
  mnema-extract 'a_line_break_does_not_glue_two_words_together' --test html

# C19. The opposite direction, which C15 does not cover: no element is inline,
# so a sentence broken by `<b>` becomes three blocks. Not a loss — the chunker
# rejoins them — but three citations where the page shows one sentence.
case_ "reader: inline markup inside a sentence stays one block" \
  crates/mnema-extract/src/html.rs \
  's{fn is_inline\(element: &Element\) -> bool \{\n    matches!\(}{fn is_inline(element: \&Element) -> bool \{\n    if element.name() != "\\u{0}" \{\n        return false;\n    \}\n    matches!(}' \
  'if element.name() != "\u{0}" {
        return false;
    }
    matches!(' \
  mnema-extract 'inline_markup_inside_a_sentence_stays_one_block' --test html

# --------------------------------------------------------------- the verbatim

# C20. The server's `_clean = " ".join(text.split())`
# (`app/textdoc/html_blocks.py:41-42`) ported after all. G7.1 §2.3 refused it:
# it is applied asymmetrically there, and a rule that rewrites stored text is
# one nothing downstream can undo.
case_ "reader: the text is not folded on its way into a block" \
  crates/mnema-extract/src/html.rs \
  's{    let text = nfc::normalise\(run\)\.into_owned\(\);}{    let text = nfc::normalise(run).split_whitespace().collect::<Vec<_>>().join(" ");}' \
  'let text = nfc::normalise(run).split_whitespace().collect::<Vec<_>>().join(" ");' \
  mnema-extract 'the_text_is_verbatim_after_nfc_and_nothing_else' --test html

# C21. NFC dropped. A Ukrainian `й` written decomposed and one written composed
# tokenize as two different words (D32), so a document becomes unfindable by its
# own spelling — and macOS hands over decomposed text.
case_ "reader: a block's text is normalised at all" \
  crates/mnema-extract/src/html.rs \
  's{    let text = nfc::normalise\(run\)\.into_owned\(\);}{    let text = run.clone();}' \
  'let text = run.clone();' \
  mnema-extract 'the_text_is_verbatim_after_nfc_and_nothing_else' --test html

# C22. Line numbers invented for a format that has none. `pages_of` gives this
# reader `Fixed(Coordinate::Section)` *because* these blocks carry no rows; a
# number here is cited as "рядки 1–1" of a document with no rows at all.
case_ "reader: an html block carries no line numbers" \
  crates/mnema-extract/src/html.rs \
  's{        line_start: None,\n        line_end: None,}{        line_start: Some(1),\n        line_end: Some(1),}' \
  '        line_start: Some(1),
        line_end: Some(1),' \
  mnema-extract 'no_html_block_claims_a_line_number' --test html

# C23. `reading_order` frozen. The schema's uniqueness is on
# `(page_id, reading_order)` and `chunk_blocks` walks blocks in the order given,
# so a page whose blocks all claim position 0 is a page with no reading order.
case_ "reader: reading order counts the blocks of its page" \
  crates/mnema-extract/src/html.rs \
  's{        reading_order: page\.blocks\.len\(\) as i64,}{        reading_order: 0,}' \
  'reading_order: 0,' \
  mnema-extract 'reading_order_restarts_on_every_page' --test html

# ------------------------------- the rebuild a changed reader has to force
#
# Not the reader at all, and the most expensive four cases in this file. The
# manifest's whole mechanism ran one step short: the cheap arm noticed `.html`
# had changed hands and handed the file to a worker, and step 3 then found the
# document under the same content hash with its chunking finished and returned
# having written nothing. The text reader's markup stayed in the index, and
# `repoint` wrote the new reader into the `path` row in the same transaction, so
# the next walk agreed and never asked again. `INDEX_FORMAT_VERSION` does not
# reach it either: that lever is read only by the skip journal's arm.

# C24. The gate removed entirely — the defect verbatim.
case_ "ingest: a document made by another reader is rebuilt, not confirmed" \
  crates/mnema-ingest/src/lib.rs \
  's{        if !stale_reading && db\.stage_status\(&id, STAGE_CHUNK\)\?\.as_deref\(\) == Some\(STATUS_DONE\) \{}{        let _ = stale_reading;\n        if db.stage_status(&id, STAGE_CHUNK)?.as_deref() == Some(STATUS_DONE) \{}' \
  'let _ = stale_reading;' \
  mnema-ingest 'a_file_indexed_by_another_reader_is_rebuilt_rather_than_left_as_it_was' --test slice

# C25. The name half of the comparison dropped. `.html` moving from the text
# reader to the html one is a change of *name* at the same version, so this is
# the html case exactly.
case_ "ingest: a reader that changed its name changed the reading" \
  crates/mnema-ingest/src/lib.rs \
  's{            entry\.reader != document\.reader\n                \|\| entry\.reader_version != i64::from\(document\.reader_version\)}{            entry.reader_version != i64::from(document.reader_version)}' \
  'is_some_and(|entry| {
            entry.reader_version != i64::from(document.reader_version)
        });' \
  mnema-ingest 'a_file_indexed_by_another_reader_is_rebuilt_rather_than_left_as_it_was' --test slice

# C26. The version half dropped. A reader whose output changes without its name
# changing is the ordinary way this moves — and it is what `PDF_READER_VERSION`
# is for: a release that learns to read page 2 must deliver page 2, not confirm
# the document that is missing it.
case_ "ingest: a reader at a new version changed the reading too" \
  crates/mnema-ingest/src/lib.rs \
  's{            entry\.reader != document\.reader\n                \|\| entry\.reader_version != i64::from\(document\.reader_version\)}{            entry.reader != document.reader}' \
  'is_some_and(|entry| {
            entry.reader != document.reader
        });' \
  mnema-ingest 'a_reader_version_bump_re_extracts_the_document_rather_than_confirming_it' --test slice

# C27. The comparison made against the manifest's *prediction* instead of
# against the reader that ran. It is the reading in the index that goes stale,
# not the guess about it — and where the two can never converge (a reader chosen
# by content) this rebuilds the document, and moves every chunk id under every
# citation into it, on every single walk.
case_ "ingest: the comparison is against the reader that ran, not the predicted one" \
  crates/mnema-ingest/src/lib.rs \
  's{            entry\.reader != document\.reader\n                \|\| entry\.reader_version != i64::from\(document\.reader_version\)}{            entry.reader != expected.reader\n                || entry.reader_version != i64::from(expected.version)}' \
  'entry.reader != expected.reader
                || entry.reader_version != i64::from(expected.version)' \
  mnema-ingest 'a_reader_no_build_agrees_on_is_re_read_every_pass_and_costs_only_that' --test slice

# --------------------------- fix round 1: what the rebuild itself opened up
#
# The rebuild in C24–C27 is reached with the chunk stage already `done`, which
# is a state `ingest_file` had never been in before: every earlier route to
# `rebuild` came from a stage that was *not* finished, and step 5's comment
# ("a crash before this point costs a re-index rather than a lie — the cheap arm
# finds no finished stage") was true because of it. These five are the cases the
# review's Critical and its two behavioural Minors asked for.

# C28. **The Critical.** The stage left claiming `done` while the rows that made
# it true are cleared. Any failure past slice 0 then leaves pages 1..20, a path
# row already crediting the new reader, and all five of the cheap arm's
# conditions satisfied — `Unchanged` for the life of the index, with the rest of
# the document gone. No crash needed: `IngestError::Busy` on a later slice
# re-enters at the top.
case_ "ingest: a rebuild stops claiming the stage it is replacing" \
  crates/mnema-ingest/src/lib.rs \
  's{db\.record_stage\(&id, STAGE_CHUNK, STATUS_REBUILDING\)\?;}{/* the stage keeps claiming done */}' \
  '/* the stage keeps claiming done */' \
  mnema-ingest 'a_rebuild_interrupted_between_slices_is_finished_by_the_next_walk' --test slice

# C29. The reader on a `path` row describes the document that path *named*. Read
# as a statement about the document just read, it makes a perfectly current
# reading look stale: a full clear and rewrite where the answer is that there is
# nothing to do, and every `chunk.id` moves under every citation into it.
case_ "ingest: only the path that already named this document can call it stale" \
  crates/mnema-ingest/src/lib.rs \
  's{    let stale_reading = !renaming\n        && recorded\.as_ref\(\)\.is_some_and\(\|entry\| \{\n            entry\.reader != document\.reader\n                \|\| entry\.reader_version != i64::from\(document\.reader_version\)\n        \}\);}{    let stale_reading = recorded.as_ref().is_some_and(|entry| \{\n        entry.reader != document.reader\n            || entry.reader_version != i64::from(document.reader_version)\n    \});}' \
  'let stale_reading = recorded.as_ref().is_some_and(|entry| {' \
  mnema-ingest 'a_path_that_comes_to_name_a_document_this_reader_made_is_not_rebuilt' --test slice

# C30. NFC put back over the *source*, before the parse — the ordering as it
# shipped, and the reason it is wrong: `и&#774;` holds no combining mark until
# the parser decodes the reference, so normalising the source composes nothing
# and the mark it produces is never composed at all. Two spellings of one word,
# tokenized apart, which is the harm D32 exists to prevent.
case_ "reader: the text is normalised after the parse, not before it" \
  crates/mnema-extract/src/html.rs \
  's{    let document = Html::parse_document\(&decoded\);}{    let source = nfc::normalise(\&decoded);\n    let document = Html::parse_document(\&source);};s{    let text = nfc::normalise\(run\)\.into_owned\(\);\n    run\.clear\(\);}{    let text = std::mem::take(run);}' \
  'let source = nfc::normalise(&decoded);' \
  mnema-extract 'a_combining_mark_written_as_a_character_reference_is_composed_too' --test html

# C31. The run not ended at an element that draws a box. `<iframe>` was skipped
# without flushing, so `<p>перед<iframe></iframe>після</p>` was stored as
# `передпісля` — a word in no file, findable by neither half.
#
# **The condition is matched loosely on purpose, and that is a repair.** This
# case was anchored on the exact text `if renders_a_box(element) {`, and task 11
# widened that line to `|| head_matter` — the substitution then matched nothing,
# the case reported BROKEN, and this gate exited 1 while every test stayed green.
# Anchoring on code rather than on the comment beside it was necessary and not
# sufficient: what makes an anchor durable is naming the thing the change cannot
# move. `renders_a_box(element)` is that thing; whatever else the condition
# grows, this still finds it.
case_ "reader: a box on the page ends the run around it" \
  crates/mnema-extract/src/html.rs \
  's{if renders_a_box\(element\)[^\{]*\{\n\s*flush\(&mut run, &flow, &mut pages\);\n\s*\}}{/* the box does not end the run */}' \
  '/* the box does not end the run */' \
  mnema-extract 'a_box_on_the_page_ends_a_run_and_something_invisible_does_not' --test html

# C32. And the other direction, which C31 does not cover: ending the run at
# *every* skipped element splits a sentence where a browser shows one, because
# the other six are `display: none` and the words around them really are
# adjacent on the page.
case_ "reader: something invisible does not end the run around it" \
  crates/mnema-extract/src/html.rs \
  's{fn renders_a_box\(element: &Element\) -> bool \{\n    element\.name\(\) == "iframe"\n\}}{fn renders_a_box(element: \&Element) -> bool \{\n    let _ = element;\n    true\n\}}' \
  'let _ = element;
    true
}' \
  mnema-extract 'a_box_on_the_page_ends_a_run_and_something_invisible_does_not' --test html

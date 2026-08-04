# Mutation cases for Task 11: the EPUB reader. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-11.sh
#
# A book is the first format on this wire that both sends pages and names pages
# it did not send, so this file is mostly about one class of silence: a chapter
# that goes missing without anyone being told. There are four shapes of it and
# nothing downstream can tell them apart —
#
#   * a chapter skipped and not named, which is a book that is quietly shorter;
#   * a chapter named and also sent, which stops the entire walk with
#     `PoolError::Protocol` and accuses the worker binary of being from another
#     release (`crates/mnema-pool/src/lib.rs:1338`);
#   * a chapter renumbered so the gap closes, which is a journal row pointing at
#     the wrong chapter of the book;
#   * every chapter skipped at once, which is one book lost to one wrong rule
#     about hrefs — and the two cases that measured it (C16, C33) are the most
#     expensive in this file.
#
# Cases are anchored on code, never on the prose beside it: task 10 lost five of
# its own to a fix round that edited the comments they matched.

# ------------------------------------------------------ the reader's own name

# C1. The name is one symbol across a process boundary and across D40.
# `pages_of` matches it to cite a chapter as `Coordinate::Section`
# (`crates/mnema-ingest/src/lib.rs:1392`); one character off falls to
# `PageContext::Lines`, which asks blocks that carry no line numbers for a line
# range and answers `Coordinate::None`. Green everywhere else.
case_ "worker: the header names the epub reader, not a near-miss" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                    reader: manifest::READER_EPUB\.to_string\(\),}{                    reader: "epub-2".to_string(),}' \
  'reader: "epub-2".to_string(),' \
  mnema-extract 'an_epub_is_read_chapter_by_chapter_and_its_summary_names_what_it_skipped' --test worker_cli

# C2. The version, which is what decides whether a book already in the index was
# made by today's code — and, since task 10's C22, whether it is replaced.
case_ "worker: the header carries the epub reader's version" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                    reader_version: manifest::EPUB_READER_VERSION,}{                    reader_version: 0,}' \
  'reader_version: 0,' \
  mnema-extract 'an_epub_is_read_chapter_by_chapter_and_its_summary_names_what_it_skipped' --test worker_cli

# C3. `native:epub` names the reader, not the file. Text that came out of a book
# is not the same evidence as text that came out of a plain-text read of the
# same bytes, and `page.text_source` is where that is recorded.
case_ "worker: the summary names the epub text source" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                    text_source: "native:epub"\.to_string\(\),}{                    text_source: "native:html".to_string(),}' \
  'text_source: "native:html".to_string(),' \
  mnema-extract 'an_epub_is_read_chapter_by_chapter_and_its_summary_names_what_it_skipped' --test worker_cli

# ---------------------------------------------------------------- the manifest

# C4. The entry that looks obviously right and is not. `.epub` is an extension
# nothing else uses, so claiming it reads as harmless — but `identify` reaches
# this reader through magic bytes and the archive's `mimetype` entry, never
# through the name, and the map is a claim about `identify`. With the entry, a
# text file called `notes.epub` is predicted for a reader that never touched it.
case_ "manifest: epub is decided by content, so the map does not claim it" \
  crates/mnema-extract/src/manifest.rs \
  's{    Manifest \{\n        default: ReaderId::new\("text", TEXT_READER_VERSION\),}{    by_extension.insert(\n        "epub".to_string(),\n        ReaderId::new(READER_EPUB, EPUB_READER_VERSION),\n    );\n    Manifest \{\n        default: ReaderId::new("text", TEXT_READER_VERSION),}' \
  'by_extension.insert(
        "epub".to_string(),
        ReaderId::new(READER_EPUB, EPUB_READER_VERSION),
    );' \
  mnema-extract 'an_epub_is_read_by_content_so_the_manifest_predicts_the_wrong_reader_for_it' --test manifest

# ------------------------------------- what can vanish: the skip accounting

# C5. **The decision Task 6's review moved here.** A chapter the spine names and
# the archive does not hold, reported as damage: one broken link removes a whole
# book from the index, and the file it removes opens correctly in every reading
# application.
case_ "reader: a chapter that is not there does not refuse the book" \
  crates/mnema-extract/src/epub.rs \
  's{            Err\(ZipPartError::Missing\) => \{\n                skipped\.push\(page_no\);\n                continue;\n            \}}{            Err(ZipPartError::Missing) => \{\n                return Err(EpubError::Malformed("a chapter is missing".to_string()));\n            \}}' \
  'return Err(EpubError::Malformed("a chapter is missing".to_string()));' \
  mnema-extract 'a_chapter_the_archive_does_not_hold_is_named_rather_than_refusing_the_book' --test epub

# C6. …and the opposite failure, which costs nothing visible and is therefore
# worse: the chapter is skipped and **not named**. The book is still a book, one
# chapter shorter, and the journal has no row saying which one.
case_ "reader: a chapter that is not there is named rather than dropped" \
  crates/mnema-extract/src/epub.rs \
  's{            Err\(ZipPartError::Missing\) => \{\n                skipped\.push\(page_no\);\n                continue;\n            \}}{            Err(ZipPartError::Missing) => \{\n                continue;\n            \}}' \
  'Err(ZipPartError::Missing) => {
                continue;
            }' \
  mnema-extract 'a_chapter_the_archive_does_not_hold_is_named_rather_than_refusing_the_book' --test epub

# C7. **The pair the pool stops the whole job over.** A chapter both sent as a
# page and named in `skipped_pages` is read as a mismatched worker binary, not as
# a bad file: `PoolError::Protocol`, and the walk ends. The natural way to write
# "skip this chapter" — send an empty page and count it too — is exactly this.
case_ "reader: a chapter with no text is skipped instead of also being sent" \
  crates/mnema-extract/src/epub.rs \
  's{if blocks\.is_empty\(\) \{.*?continue;\n        \}}{if blocks.is_empty() \{\n            skipped.push(page_no);\n        \}}s' \
  'if blocks.is_empty() {
            skipped.push(page_no);
        }' \
  mnema-extract 'every_entry_of_the_spine_is_either_read_or_named' --test epub

# C8. The gap closed by renumbering. A page number that counts what came back
# rather than what the spine declares cites the reader's own bookkeeping: the
# journal says chapter 2 was skipped while the page called 2 is chapter 3.
case_ "reader: a chapter is numbered by the spine, not by what came back" \
  crates/mnema-extract/src/epub.rs \
  's{        let page_no = index as u32 \+ 1;}{        let page_no = chapters.len() as u32 + 1;}' \
  'let page_no = chapters.len() as u32 + 1;' \
  mnema-extract 'a_chapter_the_archive_does_not_hold_is_named_rather_than_refusing_the_book' --test epub

# C9. A chapter with nothing in it stored as a page. A cover is a body holding
# one image, and a page with no blocks is a row in the index that answers no
# query — while the number that would have told someone about it goes nowhere.
case_ "reader: a chapter that yields no block is named rather than stored empty" \
  crates/mnema-extract/src/epub.rs \
  's{        if blocks\.is_empty\(\) \{}{        if false \{}' \
  '        if false {' \
  mnema-extract 'a_cover_page_is_named_as_skipped_rather_than_indexed_as_its_own_title' --test epub

# C10. One chapter whose stream will not inflate, taking the book with it. Same
# argument as C5 and a different code path: a book is not damaged because one
# member of it is.
case_ "reader: one chapter that will not decompress does not cost the book" \
  crates/mnema-extract/src/epub.rs \
  's{            Err\(ZipPartError::Malformed\) => \{\n                saw_damage = true;\n                skipped\.push\(page_no\);\n                continue;\n            \}}{            Err(ZipPartError::Malformed) => \{\n                return Err(EpubError::Malformed("a chapter is damaged".to_string()));\n            \}}' \
  'return Err(EpubError::Malformed("a chapter is damaged".to_string()));' \
  mnema-extract 'one_chapter_that_will_not_decompress_does_not_cost_the_book' --test epub

# C11. …and the other direction: a book that produced nothing because its
# archive is damaged, reported as a book with no text in it. Both verdicts are
# about content and they tell the person holding the file to do opposite things
# — look for a better copy, or accept that this one is pictures.
case_ "reader: a damaged archive says damaged, not 'no text in this book'" \
  crates/mnema-extract/src/epub.rs \
  's{        return Err\(if saw_damage \{}{        return Err(if false \{}' \
  'return Err(if false {' \
  mnema-extract 'a_book_whose_chapters_will_not_decompress_says_damaged_rather_than_no_text' --test epub

# C12. A book of pictures stored as a document with no chapters at all. `pdf.rs`
# refuses the same shape for the same reason: a document with no text is a fact
# worth telling someone, and a row with no blocks tells them nothing.
case_ "reader: a book with no readable chapter is refused, not stored empty" \
  crates/mnema-extract/src/epub.rs \
  's{    if chapters\.is_empty\(\) \{}{    if false \{}' \
  '    if false {' \
  mnema-extract 'a_book_with_no_readable_chapter_is_refused_rather_than_stored_empty' --test epub

# ------------------------------------------------- the chapter and its title

# C13. The chapter's `<title>` back as a block. Every chapter carries one, no
# reading system paints it, and the cover is where it stops being cosmetic: a
# body holding one image becomes a chapter whose entire indexed text is the word
# `Обкладинка`, and a search for it cites a page that shows a picture.
case_ "reader: a chapter's tab label is not its text" \
  crates/mnema-extract/src/html.rs \
  's{                        let head_matter =\n                            head_title == HeadTitle::NamesOnly && names_the_document\(element\);}{                        let head_matter = false;}' \
  'let head_matter = false;' \
  mnema-extract 'a_cover_page_is_named_as_skipped_rather_than_indexed_as_its_own_title' --test epub

# C14. …and the direction C13 does not cover: the title stops naming the chapter
# at all. `pages_of` renders an unnamed page as the empty string, so a chapter
# with no heading in its body would cite a section that exists and has no name —
# spec §6, invariant 1.
case_ "reader: a chapter's title still names it" \
  crates/mnema-extract/src/html.rs \
  's{                            if head_matter && let Some\(title\) = section_title\(node\) \{\n                                open_page\(&mut pages, title\);\n                            \}}{                            /* the title names nothing */}' \
  '/* the title names nothing */' \
  mnema-extract 'a_chapters_title_names_it_without_being_its_text' --test epub

# C15. `reading_order` left as the HTML reader gave it. Several of its pages
# become one page here, so a chapter with two headings holds several blocks all
# claiming position 0 — and the index's uniqueness is `(page_id, reading_order)`.
case_ "reader: a chapter's blocks are numbered once across the whole chapter" \
  crates/mnema-extract/src/epub.rs \
  's{                block\.reading_order = blocks\.len\(\) as i64;}{                /* the order the page gave it is kept */}' \
  '/* the order the page gave it is kept */' \
  mnema-extract 'a_chapters_blocks_are_numbered_once_across_the_whole_chapter' --test epub

# ----------------------------------------------------------------- the caps

# C16. The cap removed. The server reads an epub chapter with a bare `zf.read`
# (`app/textdoc/adapters.py:118`) while docx and xlsx go through a capped
# stream; that asymmetry is what spec §9 declined to port.
case_ "reader: a chapter is read under a cap at all" \
  crates/mnema-extract/src/epub.rs \
  's{    let cap = MEMBER_MAX_BYTES\.min\(\*budget\);}{    let cap = usize::MAX;}' \
  'let cap = usize::MAX;' \
  mnema-extract 'a_chapter_is_read_under_the_same_cap_as_a_docx_part' --test epub

# C17. The cap that ignores the book's remaining budget: N members each just
# under `MEMBER_MAX_BYTES` is the same bomb with more entries, and the way this
# process reports gigabytes of chapters is by being killed.
case_ "reader: the member cap is the smaller of the two, not the member's alone" \
  crates/mnema-extract/src/epub.rs \
  's{    let cap = MEMBER_MAX_BYTES\.min\(\*budget\);}{    let cap = MEMBER_MAX_BYTES;}' \
  'let cap = MEMBER_MAX_BYTES;' \
  mnema-extract 'epub::tests::a_book_draws_every_member_against_one_budget' --lib

# C18. …and the budget that never shrinks, which is the same hole reached from
# the other side: a total that is not a total.
case_ "reader: every member spends the budget it draws against" \
  crates/mnema-extract/src/epub.rs \
  's{    \*budget -= member\.len\(\);}{    let _ = member.len();}' \
  'let _ = member.len();' \
  mnema-extract 'epub::tests::a_book_draws_every_member_against_one_budget' --lib

# ------------------------------------------------------- what an href is not

# C19. **The case that costs every chapter of a book at once.** A book whose
# chapters are named in the author's own language has every href
# percent-encoded, and a zip entry's name is not. Read as written, nothing is
# found, every chapter is skipped, and the book is refused as having no text.
case_ "reader: a percent-encoded href names the member it decodes to" \
  crates/mnema-extract/src/epub.rs \
  's{            \.map\(\|segment\| percent_decode\(segment\)\)}{            .map(|segment| segment.to_string())}' \
  '.map(|segment| segment.to_string())' \
  mnema-extract 'a_percent_encoded_href_names_the_member_it_decodes_to' --test epub

# C20. The href resolved against the archive's root instead of against the
# package document. Every book whose files sit in `OEBPS/` loses every chapter.
case_ "reader: an href is relative to the package document" \
  crates/mnema-extract/src/epub.rs \
  's{    let base = parent_of\(&opf_path\);}{    let base = String::new();}' \
  'let base = String::new();' \
  mnema-extract 'an_href_is_resolved_against_the_package_document' --test epub

# C21. `../` left as a literal path component rather than climbing a directory.
case_ "reader: a dot-dot in an href climbs a directory" \
  crates/mnema-extract/src/epub.rs \
  's{            "\.\." => \{\n                segments\.pop\(\);\n            \}}{            ".." => \{\}}' \
  '".." => {}' \
  mnema-extract 'an_href_is_resolved_against_the_package_document' --test epub

# C22. An href that is absolute in the archive appended to the package
# document's directory instead of replacing it.
case_ "reader: an href from the archive root is not appended to the base" \
  crates/mnema-extract/src/epub.rs \
  's{    let \(mut segments, rest\) = match href\.strip_prefix\('"'"'/'"'"'\) \{}{    let (mut segments, rest) = match None::<\&str> \{}' \
  'match None::<&str> {' \
  mnema-extract 'an_href_is_resolved_against_the_package_document' --test epub

# C23. A fragment read as part of the member's name: `ch1.xhtml#part2` is a
# place inside a file, not a second file.
case_ "reader: a fragment is not part of the member name" \
  crates/mnema-extract/src/epub.rs \
  's{    let href = href\.split\(\['"'"'#'"'"', '"'"'\?'"'"'\]\)\.next\(\)\.unwrap_or\(""\);}{    let href = href;}' \
  'let href = href;' \
  mnema-extract 'a_fragment_in_an_href_is_not_part_of_the_member_name' --test epub

# C24. Decoded **before** the path is resolved, which is the ordering that lets
# an escape become path syntax: `%2e%2e` is a filename component that happens to
# spell `..`, and a member really named that stops being reachable.
case_ "reader: an escape that spells a path segment stays a filename" \
  crates/mnema-extract/src/epub.rs \
  's{    for segment in rest\.split\('"'"'/'"'"'\) \{}{    let decoded = percent_decode(rest);\n    let rest = decoded.as_str();\n    for segment in rest.split('"'"'/'"'"') \{}' \
  'let decoded = percent_decode(rest);' \
  mnema-extract 'epub::tests::an_escape_that_spells_a_path_segment_is_a_filename' --lib

# ------------------------------------------- which spine entries are chapters

# C25. The media-type filter removed. An SVG plate read by the HTML parser gives
# back its `<title>` tooltips and its `<text>` labels — markup reaching a chunk
# by a route the HTML reader's own skip list does not cover.
case_ "reader: a picture in the spine is not mined for the text inside its markup" \
  crates/mnema-extract/src/epub.rs \
  's{        if !is_content_document\(item\.media_type\.as_deref\(\)\) \{}{        if false \{}' \
  '        if false {' \
  mnema-extract 'a_spine_entry_that_is_not_a_content_document_is_skipped' --test epub

# C26. …and the filter drawn too tight. `text/html` is what books made before
# EPUB 3 declare, and dropping it skips every chapter of every one of them —
# which reaches the user as "no chapter of this EPUB carries any text".
case_ "reader: the older spelling of a content document is still a chapter" \
  crates/mnema-extract/src/epub.rs \
  's{    matches!\(bare\.as_str\(\), "application/xhtml\+xml" \| "text/html"\)}{    matches!(bare.as_str(), "application/xhtml+xml")}' \
  'matches!(bare.as_str(), "application/xhtml+xml")' \
  mnema-extract 'a_spine_entry_that_is_not_a_content_document_is_skipped' --test epub

# C27. An entry that states no media type at all treated as "not a chapter".
# Invalid per the standard and shipped anyway; the direction of the guess is the
# decision, and this is it inverted.
case_ "reader: an entry with no stated media type is read rather than skipped" \
  crates/mnema-extract/src/epub.rs \
  's{    let Some\(media_type\) = media_type else \{\n        return true;\n    \};}{    let Some(media_type) = media_type else \{\n        return false;\n    \};}' \
  'let Some(media_type) = media_type else {
        return false;
    };' \
  mnema-extract 'a_manifest_entry_that_states_no_media_type_is_still_read' --test epub

# ------------------------------------------------------- the book's structure

# C28. The spine's order not the reading order. Every chapter is still read and
# every word is still indexed; what moves is which chapter a citation names and
# what a document reads like from the top.
case_ "reader: chapters come back in the order the spine gives them" \
  crates/mnema-extract/src/epub.rs \
  's{    for \(index, idref\) in package\.spine\.iter\(\)\.enumerate\(\) \{}{    for (index, idref) in package.spine.iter().rev().enumerate() \{}' \
  'package.spine.iter().rev().enumerate()' \
  mnema-extract 'chapters_come_back_in_spine_order' --test epub

# C29. The rootfile chosen by position rather than by what it says it is. A
# container may list several renditions, and the first one is whichever the
# producer happened to write.
case_ "reader: the container takes the rootfile that says it is a package document" \
  crates/mnema-extract/src/epub.rs \
  's{                if media_type\.as_deref\(\) == Some\(OPF_MEDIA_TYPE\) \{}{                if false \{}' \
  '                if false {' \
  mnema-extract 'a_container_takes_the_rootfile_that_says_it_is_a_package_document' --test epub

# C30. The package document matched on qualified names. `<opf:manifest>` and
# `<manifest>` are both legal and both in the wild; matching the qualified name
# finds no manifest in half of them, and a book whose every id is unknown is
# refused as having no readable chapter. A whole book lost to a colon.
case_ "reader: a package document written with a prefix reads the same" \
  crates/mnema-extract/src/epub.rs \
  's{            quick_xml::events::Event::Start\(ref e\) \| quick_xml::events::Event::Empty\(ref e\) => \{\n                match e\.local_name\(\)\.as_ref\(\) \{}{            quick_xml::events::Event::Start(ref e) | quick_xml::events::Event::Empty(ref e) => \{\n                match e.name().as_ref() \{}' \
  'match e.name().as_ref() {' \
  mnema-extract 'a_package_document_written_with_a_prefix_reads_the_same' --test epub

# C31. A spine with no entries treated as a book rather than as damage. It is
# not the same answer as "nothing in this book had text": there was never
# anything here that was going to.
case_ "reader: a spine that names no chapter is damage" \
  crates/mnema-extract/src/epub.rs \
  's{    if package\.spine\.is_empty\(\) \{}{    if false \{}' \
  '    if false {' \
  mnema-extract 'a_package_document_with_an_empty_spine_is_damaged' --test epub

# C32. A missing container reported as a book with no text. `is_epub` checks the
# `mimetype` entry and nothing else, so a zip with no container really does
# reach this reader — and it is damaged, not empty.
case_ "reader: the structure a book needs is damage when it is absent" \
  crates/mnema-extract/src/epub.rs \
  's{        ZipPartError::Missing => EpubError::Malformed\(format!\("\{path\} is not in the archive"\)\),}{        ZipPartError::Missing => EpubError::NoReadableChapter,}' \
  'ZipPartError::Missing => EpubError::NoReadableChapter,' \
  mnema-extract 'an_archive_with_no_container_is_damaged' --test epub

# C33. **The measurement that reversed this file's first answer.** Decoding the
# package document strictly as UTF-8 — "XML states its encoding, do not guess" —
# was written, measured, and withdrawn: chardetng read every UTF-8 fixture
# correctly including the shortest, and read a windows-1251 package document
# that strict UTF-8 turns into replacement characters. Under the strict rule
# every Cyrillic href in such a book matches no member and the whole book is
# refused as having no text.
case_ "reader: a package document in a legacy encoding is still read" \
  crates/mnema-extract/src/epub.rs \
  's{    let package = parse_package\(&crate::text::decode\(&opf\)\)\?;}{    let package = parse_package(\&encoding_rs::UTF_8.decode(\&opf).0)?;}' \
  'parse_package(&encoding_rs::UTF_8.decode(&opf).0)?' \
  mnema-extract 'a_package_document_in_a_legacy_encoding_is_still_read' --test epub

# --------------------------------------------------------------- the verbatim
#
# Task 10 checks both of these for HTML. They are repeated here against a book's
# own fixture on purpose: an invariant checked in one reader of five is an
# invariant missing from four, and these two are the whole design — what is
# stored is what the file shows, after NFC and after nothing else.

# C34. The server's `_clean = " ".join(text.split())`
# (`app/textdoc/html_blocks.py:41-42`) ported after all. It is applied to epub
# there, and G7.1 §2.3 refused it: a rule that rewrites stored text is one
# nothing downstream can undo.
case_ "reader: a chapter's text is not folded on its way into a block" \
  crates/mnema-extract/src/html.rs \
  's{    let text = nfc::normalise\(run\)\.into_owned\(\);}{    let text = nfc::normalise(run).split_whitespace().collect::<Vec<_>>().join(" ");}' \
  'let text = nfc::normalise(run).split_whitespace().collect::<Vec<_>>().join(" ");' \
  mnema-extract 'a_chapters_text_is_verbatim_after_nfc_and_nothing_else' --test epub

# C35. NFC dropped. A Ukrainian `й` written decomposed and one written composed
# tokenize as two different words (D32) — and an XHTML chapter is where this
# bites hardest, because these producers write `&#1080;&#774;` rather than the
# characters themselves.
case_ "reader: a chapter's block text is normalised at all" \
  crates/mnema-extract/src/html.rs \
  's{    let text = nfc::normalise\(run\)\.into_owned\(\);}{    let text = run.clone();}' \
  'let text = run.clone();' \
  mnema-extract 'a_chapters_text_is_verbatim_after_nfc_and_nothing_else' --test epub

# C36. The chapter's name left unbounded. `section_title` is one column shown by
# one interface, and a name three hundred characters long is a citation that
# fills the pane it appears in. Reached through the HTML reader rather than
# called here, which is why this case mutates `markdown.rs` and reddens an epub
# test: the shared rule is what four readers must not each decide.
case_ "reader: a chapter's name is bounded by the rule every reader shares" \
  crates/mnema-extract/src/markdown.rs \
  's{    if flattened\.chars\(\)\.count\(\) <= SECTION_TITLE_MAX_CHARS \{\n        return Some\(flattened\);\n    \}}{    return Some(flattened);}' \
  'return Some(flattened);' \
  mnema-extract 'a_chapters_name_is_bounded_by_the_rule_every_reader_shares' --test epub

# C37. The digest dropped from an epub refusal. It is the field that tells the
# parent whether the file changed or only the rule did — and this branch is the
# second way into `too_large`, the one that *did* open the file, so unlike the
# ceiling above it the digest is real and owed. `every_refusal…` is a
# hand-written table, which is exactly why a reader that refuses under a new
# rule and does not add its rows is a branch nothing measures.
case_ "worker: an epub refused after being read carries the digest it was read on" \
  crates/mnema-extract/src/bin/worker.rs \
  's{                reason: "a member of this EPUB inflates past the cap on one member"\.to_string\(\),\n                sha256: Some\(sha256\),}{                reason: "a member of this EPUB inflates past the cap on one member".to_string(),\n                sha256: None,}' \
  'reason: "a member of this EPUB inflates past the cap on one member".to_string(),
                sha256: None,' \
  mnema-extract 'every_refusal_that_read_the_file_carries_the_digest_it_read' --test worker_cli

//! The EPUB reader: a book is a zip of XHTML chapters, and each chapter is one
//! page of the document.
//!
//! Almost none of this file is reading. `zip_part::read_member` opens a member
//! under a cap and `html::extract_html_chapter` turns one chapter into prose;
//! what is decided here is the three seams between them — which member is a
//! chapter, what happens when one of them is not there, and what a book made of
//! nothing but pictures is.
//!
//! **One chapter is one page, and that is what makes a missing chapter
//! nameable.** `Summary::skipped_pages` carries page *numbers* out of this
//! process and the parent journals a row per number, so a chapter has to have a
//! number before it is read — its position in the spine. Letting a chapter
//! produce as many pages as it has headings would read better in a citation and
//! would make the skip unrecordable: a chapter that was never opened has no
//! heading count, so there is no number to name it by and every later chapter
//! shifts up to cover the hole. The gap in `page_no` is the honest record, the
//! same one `pdf.rs` leaves.
//!
//! **A chapter the spine names and the archive does not hold is an ordinary
//! book, not a damaged one.** `typing::is_epub` (`typing.rs:312-330`) requires
//! only the `mimetype` entry and no chapter at all, so a broken link inside is
//! not something identification promised was absent; the server treats it as
//! routine too (`app/textdoc/adapters.py:119`, `except KeyError: continue`).
//! Refusing the file would take a whole book out of the index over one link, so
//! the chapter is skipped by number and only a book with **no** readable
//! chapter is refused — for the same reason a PDF whose every page is a scan is
//! refused rather than stored as a document with no text.
//!
//! **Parsed by the HTML parsing algorithm, although an EPUB chapter is XHTML.**
//! The two differ: `<![CDATA[…]]>` is text to an XML parser and a comment to
//! this one, and `<div/>` closes itself in XML while html5ever reads it as an
//! opening tag. Both are real and both cost a block boundary at most. What an
//! XML parser costs is the whole chapter: XML is defined to *stop* at the first
//! well-formedness error, so one stray `&`, one unescaped `<`, or one undeclared
//! entity — all of which real EPUB producers emit — loses every word after it,
//! silently, in a file that opens correctly in every reading application. This
//! reader is asked what can vanish, and that is the answer: an HTML parser has
//! no failure mode at all, and its worst case here is a paragraph split in two.
//! CDATA in a chapter is in practice a wrapper around `<script>` or `<style>`,
//! whose content this reader drops on purpose either way.
//!
//! **The one cost that was measured rather than assumed.**
//! `zip_part::read_member` reopens the archive on every call, so a book of N
//! chapters parses the central directory N times — quadratic in the number of
//! entries, and the reason Task 6's review asked for a number before anyone
//! reshaped `zip_part` around it. The number is small: 500 chapters (past any
//! real book) read in **52–87 ms**, release build, this machine, and the shape
//! is nearer linear than quadratic — 125 chapters in 11 ms, 250 in 21 ms, 500
//! in 52 ms, ×2.4 per doubling rather than ×4. What dominates is the HTML parse
//! of each chapter, not the directory. `zip_part` keeps its shape.
//!
//! **What is knowingly not read**, each with its reason:
//!
//! - the navigation document (EPUB 3 `nav`, EPUB 2 `toc.ncx`), which carries a
//!   human-written label per chapter and would name a section better than the
//!   chapter's own `<title>`. It is a second structure with two incompatible
//!   spellings, and a chapter that is in the spine and not in the navigation
//!   would then have no name at all;
//! - spine items that are not XHTML — an SVG cover page, a DTBook chapter. The
//!   `media-type` the manifest *states* decides this, rather than a guess from
//!   the bytes: an SVG read by the HTML parser gives back its `<title>`
//!   tooltips and its labels, which is markup-as-prose in a new place. Such an
//!   item is skipped by number, so it is in the journal rather than nowhere;
//! - metadata: the book's own title, its author, its language. Nothing in the
//!   four-level model holds them yet (`document` has no such column), and the
//!   language of a *block* is the extraction spec's subject.

use std::collections::HashMap;

use mnema_core::Block;

use crate::html;
use crate::zip_part::{self, MEMBER_MAX_BYTES, ZipPartError};

/// The path of the one member every EPUB must carry, at the one name the
/// standard fixes. Everything else about a book's layout is stated inside it.
const CONTAINER_PATH: &str = "META-INF/container.xml";

/// The media type of the package document, as `container.xml` names it.
const OPF_MEDIA_TYPE: &str = "application/oebps-package+xml";

/// What a whole book may inflate to across every member this reader opens.
///
/// [`zip_part::MEMBER_MAX_BYTES`] bounds one member and nothing else, which for
/// this format is not a bound at all: a spine of 500 chapters each just under
/// that cap is 8 GiB out of an archive that passed the request's 64 MiB
/// ceiling, and the way this process would report it is by being killed. A
/// worker that dies is classified by how it died, not by the file that did it,
/// so the amplification would cost more than the book.
///
/// Larger than any real book by a wide margin: the request's own ceiling is
/// 64 MiB of *compressed* bytes (`mnema_pool`'s `max_bytes`), and prose deflates
/// at roughly 4:1, so a legitimate book at that ceiling holds a few hundred
/// megabytes of text at the very most.
pub const BOOK_MAX_BYTES: usize = 256 << 20;

/// One chapter of a book: one page of the document it becomes.
///
/// Deliberately not [`crate::HtmlPage`], although a chapter is read by the HTML
/// reader. An `HtmlPage` is numbered within its own file and a chapter is
/// numbered within the spine, and the two counts are not the same one — see the
/// module doc for why the spine's is the one that survives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpubChapter {
    /// The chapter's own 1-based position in the spine, **not** its position
    /// among the chapters that came back. Chapters 1 and 3 arriving with 2
    /// skipped is the intended shape.
    pub page_no: u32,
    /// The name the chapter gives itself: its `<title>`, or the first heading
    /// in it if it has no title. Bounded by `markdown::bound_section_title`
    /// through the HTML reader, so that four readers do not each decide how
    /// long a section name may be.
    pub section_title: Option<String>,
    pub blocks: Vec<Block>,
}

/// What an EPUB read produced: the chapters that had text, and the numbers of
/// the chapters that did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpubBook {
    pub chapters: Vec<EpubChapter>,
    /// Ascending, 1-based, and disjoint from `chapters` — every entry the spine
    /// declares appears in exactly one of the two. The disjointness is not
    /// politeness: `mnema-pool` stops the entire job when one number is in both
    /// lists, because a page that was read and reported skipped is a journal row
    /// telling someone a chapter is missing while the index holds it.
    pub skipped: Vec<u32>,
}

/// Why a book could not be read at all. Three variants, three refusal rules,
/// and every one of them reachable — a variant nothing produces is a branch in
/// the worker that no test can ever redden.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EpubError {
    /// The archive parses as a zip and what makes it a book does not: no
    /// `META-INF/container.xml`, no package document where that file points, or
    /// XML that does not parse.
    ///
    /// **Not the verdict for a chapter.** A chapter that is missing or whose
    /// stream is corrupt is skipped by number; this names the structure that
    /// says which chapters there are, and without it there is no spine to skip
    /// anything from.
    #[error("this EPUB is damaged: {0}")]
    Malformed(String),

    /// A member inflated past [`MEMBER_MAX_BYTES`], or the book as a whole past
    /// [`BOOK_MAX_BYTES`].
    ///
    /// Decided on what came out of the stream, never on the size the archive
    /// declares — see `zip_part`'s module doc for the forged-size case that
    /// makes the distinction load-bearing.
    #[error("a member of this EPUB inflates past the cap on one member")]
    TooLarge,

    /// The spine names chapters and not one of them produced a word: a book of
    /// scanned page images, a book of SVG plates, a book whose chapters are all
    /// empty.
    ///
    /// The same answer `pdf.rs` gives a document whose every page is below the
    /// text-layer threshold, and for the same reason: a document with no text
    /// in it is a fact about the file worth telling someone, and storing it as
    /// a document with zero blocks tells them nothing.
    #[error("no chapter of this EPUB carries any text")]
    NoReadableChapter,
}

/// Reads an EPUB into chapters, one page each.
///
/// Takes bytes rather than a path for the reason every reader in this crate
/// does: `handle_request` reads the file once and hashes the same `Vec<u8>` it
/// hands here, so nothing can change between the digest and the reading.
pub fn extract_epub(bytes: &[u8]) -> Result<EpubBook, EpubError> {
    extract(bytes, BOOK_MAX_BYTES)
}

/// The read, against a stated budget.
///
/// Split out for one reason: [`BOOK_MAX_BYTES`] is a quarter of a gigabyte, and
/// a test that reached it would have to inflate a quarter of a gigabyte. The
/// rule it exists to check is not "256 MiB" — it is that every member draws
/// against **one** total, which a budget of a few kilobytes states just as well.
fn extract(bytes: &[u8], budget: usize) -> Result<EpubBook, EpubError> {
    // Every member this function opens is drawn against one budget, chapters
    // and structure alike. Passed down rather than checked afterwards: the
    // point is to stop inflating, and a total measured after the fact has
    // already allocated what it is measuring.
    let mut budget = budget;

    // **`text::decode` — the crate's one guess — for these two XML documents as
    // well as for prose, and that is a correction to what this file did first.**
    //
    // The argument for decoding structure strictly as UTF-8 is a good one on
    // paper: XML states its encoding rather than leaving it to be guessed, EPUB
    // narrows it to UTF-8 or UTF-16, and a mis-guessed package document is a
    // Cyrillic href turned into mojibake, a member name matching nothing, and
    // every chapter of the book silently skipped. It was written that way, and
    // then measured, and the measurement went the other way:
    //
    //   package document                        chardetng   strict UTF-8
    //   UTF-8, one Cyrillic letter in one href  correct     correct
    //   UTF-8, minimal, two Cyrillic letters    correct     correct
    //   windows-1251, long                      correct     mojibake
    //   windows-1251, minimal                   mojibake    mojibake
    //
    // The case the strict rule was written for — a short, mostly-ASCII document
    // where a detector has little to go on — does not exist: chardetng answered
    // UTF-8 for every UTF-8 fixture including the shortest. What does exist is
    // the row under it: EPUB forbids a windows-1251 package document and
    // producers have shipped them, and there the guess is right where the
    // standard is not. Strict decoding is never better and sometimes worse, so
    // the crate keeps one decoder — which is also `text.rs`'s own promise, that
    // the same bytes are read the same way whichever reader opens them.
    // `a_package_document_in_a_legacy_encoding_is_still_read` holds the bottom
    // two rows; mutation C33 puts the strict rule back and reddens it.
    let container = read_structure(bytes, CONTAINER_PATH, &mut budget)?;
    let opf_path = rootfile_path(&crate::text::decode(&container))?;
    let opf = read_structure(bytes, &opf_path, &mut budget)?;
    let package = parse_package(&crate::text::decode(&opf))?;

    if package.spine.is_empty() {
        return Err(EpubError::Malformed(
            "the package document's spine names no chapter".to_string(),
        ));
    }

    // Every href in the package is relative to the package document, not to the
    // archive's root — `OEBPS/content.opf` naming `Text/ch1.xhtml` means
    // `OEBPS/Text/ch1.xhtml`.
    let base = parent_of(&opf_path);

    let mut chapters = Vec::new();
    let mut skipped = Vec::new();
    // Set by a chapter whose *stream* would not decompress, as opposed to one
    // that is simply not there or simply has no text. It only matters when
    // nothing at all was read: a book that produced nothing because its archive
    // is damaged should say so, rather than report the same "no text in this
    // book" as a book of photographs.
    let mut saw_damage = false;

    for (index, idref) in package.spine.iter().enumerate() {
        // 1-based, and the position in the spine rather than in `chapters`:
        // this is the number that goes into `skipped_pages` when nothing comes
        // back, and it has to mean the same thing in both lists.
        let page_no = index as u32 + 1;

        // Two ways to be here and one answer: a spine entry that names an id
        // the manifest does not declare, and one that names no id this reader
        // could read at all (`<itemref/>`, or an `idref` holding an entity no
        // package document declares). Both are invalid and both are survivable
        // — what must not happen is that either leaves no number behind.
        let Some(item) = idref.as_ref().and_then(|idref| package.items.get(idref)) else {
            skipped.push(page_no);
            continue;
        };
        if !is_content_document(item.media_type.as_deref()) {
            skipped.push(page_no);
            continue;
        }
        let Some(path) = resolve(&base, &item.href) else {
            // An href that names no member at all — empty, or `..` climbing
            // out of the archive.
            skipped.push(page_no);
            continue;
        };

        let chapter = match read_member(bytes, &path, &mut budget) {
            Ok(chapter) => chapter,
            // The bomb, and the only failure inside a chapter that stops the
            // book. Continuing would mean inflating the next chapter of a file
            // that has already asked for more memory than the whole book may
            // have.
            Err(ZipPartError::TooLarge) => return Err(EpubError::TooLarge),
            Err(ZipPartError::Missing) => {
                skipped.push(page_no);
                continue;
            }
            Err(ZipPartError::Malformed) => {
                saw_damage = true;
                skipped.push(page_no);
                continue;
            }
        };

        let pages = html::extract_html_chapter(&chapter);
        // The first name anything in the chapter gives it: its `<title>` if it
        // has one, and otherwise its first heading. `None` when it has neither,
        // which `pages_of` renders as an empty section rather than inventing a
        // name.
        let section_title = pages.iter().find_map(|page| page.section_title.clone());
        let mut blocks: Vec<Block> = Vec::new();
        for page in pages {
            for mut block in page.blocks {
                // Renumbered across the whole chapter. The HTML reader restarts
                // `reading_order` on every page it makes, and several of its
                // pages become one page here — left alone, a chapter with two
                // headings would hold two blocks claiming position 0, and the
                // index's uniqueness is on `(page_id, reading_order)`.
                block.reading_order = blocks.len() as i64;
                blocks.push(block);
            }
        }

        if blocks.is_empty() {
            // A chapter that is there and says nothing: a cover, a plate, a
            // section divider. Named as skipped rather than stored as an empty
            // page, so that the journal can say which chapter of the book this
            // reader got nothing out of.
            skipped.push(page_no);
            continue;
        }

        chapters.push(EpubChapter {
            page_no,
            section_title,
            blocks,
        });
    }

    if chapters.is_empty() {
        return Err(if saw_damage {
            EpubError::Malformed("no chapter of this EPUB could be decompressed".to_string())
        } else {
            EpubError::NoReadableChapter
        });
    }

    Ok(EpubBook { chapters, skipped })
}

/// A member the book's structure depends on: absent, it is not a book.
///
/// The mapping is the whole difference between this and [`read_member`]:
/// `Missing` is damage here and routine there.
fn read_structure(bytes: &[u8], path: &str, budget: &mut usize) -> Result<Vec<u8>, EpubError> {
    read_member(bytes, path, budget).map_err(|e| match e {
        ZipPartError::TooLarge => EpubError::TooLarge,
        ZipPartError::Missing => EpubError::Malformed(format!("{path} is not in the archive")),
        ZipPartError::Malformed => {
            EpubError::Malformed(format!("{path} could not be read out of the archive"))
        }
    })
}

/// One member, against both caps at once.
///
/// The cap handed to `zip_part` is the smaller of the two, so a member that
/// would exhaust the book's budget is refused by the same code path — and by
/// the same measurement of what actually came out of the stream — as one that
/// is simply too big on its own.
fn read_member(bytes: &[u8], path: &str, budget: &mut usize) -> Result<Vec<u8>, ZipPartError> {
    let cap = MEMBER_MAX_BYTES.min(*budget);
    let member = zip_part::read_member(bytes, path, cap)?;
    // **The invariant, written down: `member.len() <= cap <= *budget`.** The
    // first half is `zip_part::read_member`'s — it returns `Ok` only when what
    // came out of the stream is at most `cap` (`zip_part.rs`) — and the second
    // is the line above. So the subtraction cannot go below zero today, and it
    // holds across two files, which is exactly the kind of invariant an edit
    // separates without noticing.
    //
    // `saturating_sub` rather than `-=` for what the failure would look like,
    // not for whether it can happen: a plain `-=` panics in the worker under
    // debug and, under release, wraps to something near `usize::MAX`, after
    // which the book's budget silently stops bounding anything — the one
    // outcome this cap exists to prevent, arriving through the cap itself.
    // Saturating to zero fails the other way: every later member gets a cap of
    // zero and the book is refused, loudly and wrongly, instead of read
    // without a limit.
    //
    // **Deliberately not `checked_sub` with an early `TooLarge`.** That would
    // enforce the budget a second time, on its own, and mutation C17 — which
    // takes `*budget` out of `cap` — would then still be refused by this line
    // and stop reddening. A second enforcement path that hides the guard on the
    // first is worse than no second path.
    *budget = budget.saturating_sub(member.len());
    Ok(member)
}

/// One attribute's value, with the escapes XML defines already undone.
///
/// `&amp;` in an href is `&` in the member's name, and a book whose chapter is
/// called `Q&A.xhtml` writes it that way and no other. `Implicit1_0` is the
/// version to assume when the document declares none, which is also what every
/// package document in practice declares.
///
/// `None` on an escape the parser will not resolve — a reference to an entity
/// declared in a DTD this reader does not read. The attribute is then treated
/// as absent, which for an href means the chapter is skipped by number rather
/// than looked for under a name holding a literal `&somename;`.
fn attribute_value(attribute: &quick_xml::events::attributes::Attribute<'_>) -> Option<String> {
    attribute
        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
        .ok()
        .map(|value| value.into_owned())
}

/// Where `container.xml` says the package document is.
fn rootfile_path(container: &str) -> Result<String, EpubError> {
    let mut reader = quick_xml::Reader::from_str(container);
    let mut fallback: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)) => {
                if e.local_name().as_ref() != b"rootfile" {
                    continue;
                }
                let mut full_path = None;
                let mut media_type = None;
                for attribute in e.attributes().flatten() {
                    match attribute.key.local_name().as_ref() {
                        b"full-path" => full_path = attribute_value(&attribute),
                        b"media-type" => media_type = attribute_value(&attribute),
                        _ => {}
                    }
                }
                let Some(full_path) = full_path.filter(|p| !p.is_empty()) else {
                    continue;
                };
                // The one whose media type says it is a package document, and
                // only if none says so the first one at all. A container may
                // legitimately list several rootfiles — an EPUB carrying a
                // second rendition — and taking the first blindly would read
                // whichever the producer happened to write first.
                if media_type.as_deref() == Some(OPF_MEDIA_TYPE) {
                    return Ok(normalise(&full_path));
                }
                fallback.get_or_insert(full_path);
            }
            Ok(_) => {}
            Err(e) => {
                return Err(EpubError::Malformed(format!(
                    "{CONTAINER_PATH} does not parse: {e}"
                )));
            }
        }
    }

    fallback
        .map(|path| normalise(&path))
        .ok_or_else(|| EpubError::Malformed(format!("{CONTAINER_PATH} names no package document")))
}

/// One entry of the package document's manifest.
struct Item {
    href: String,
    /// `None` when the manifest states none. Invalid per the standard, and read
    /// as a chapter anyway: a producer that omitted the attribute is more
    /// likely to have shipped an XHTML file than an SVG one, and the cost of
    /// being wrong is a block of tooltip text rather than a lost chapter.
    media_type: Option<String>,
}

/// The two halves of the package document this reader needs: what the book's
/// files are, and in what order they are read.
struct Package {
    items: HashMap<String, Item>,
    /// One entry per `<itemref>` the package document declares, in order —
    /// `None` for one that names no id this reader can use. The length is what
    /// numbers the chapters, so an entry dropped here is a chapter that leaves
    /// no number behind and moves every later one down.
    spine: Vec<Option<String>>,
}

/// Reads the manifest and the spine out of the package document.
///
/// Matched on local names, so a package written with a prefix — `<opf:manifest>`
/// — reads the same as one without. Nesting is tracked rather than assumed:
/// `<item>` means a manifest entry inside `<manifest>` and nothing at all inside
/// `<metadata>`, where an `<item>` of someone's custom vocabulary may sit.
fn parse_package(opf: &str) -> Result<Package, EpubError> {
    let mut reader = quick_xml::Reader::from_str(opf);
    let mut items: HashMap<String, Item> = HashMap::new();
    let mut spine: Vec<Option<String>> = Vec::new();
    let mut in_manifest = false;
    let mut in_spine = false;

    loop {
        let event = reader.read_event().map_err(|e| {
            EpubError::Malformed(format!("the package document does not parse: {e}"))
        })?;
        match event {
            quick_xml::events::Event::Eof => break,
            quick_xml::events::Event::Start(ref e) | quick_xml::events::Event::Empty(ref e) => {
                match e.local_name().as_ref() {
                    b"manifest" => in_manifest = true,
                    b"spine" => in_spine = true,
                    b"item" if in_manifest => {
                        let mut id = None;
                        let mut href = None;
                        let mut media_type = None;
                        for attribute in e.attributes().flatten() {
                            match attribute.key.local_name().as_ref() {
                                b"id" => id = attribute_value(&attribute),
                                b"href" => href = attribute_value(&attribute),
                                b"media-type" => media_type = attribute_value(&attribute),
                                _ => {}
                            }
                        }
                        if let (Some(id), Some(href)) = (id, href) {
                            // **First declaration wins, and this is the one
                            // broken package document in this file whose cost
                            // is not a skip.** A manifest that declares an id
                            // twice has two readable chapters competing for one
                            // spine entry, so whichever loses is a chapter
                            // whose text is silently replaced by another's
                            // under a number that names neither. `insert` took
                            // the last, which is "whichever the producer wrote
                            // second" — not a rule anyone can reason about. An
                            // XML id binds at its first declaration and a
                            // second is the error, so that is the answer.
                            items.entry(id).or_insert(Item { href, media_type });
                        }
                    }
                    // **Every `<itemref>` is a spine entry, including one this
                    // reader cannot use.** Pushing only the ones that name a
                    // usable id is how a declared chapter leaves no trace at
                    // all: no page, no number in `skipped`, and every later
                    // chapter moved down one to cover the hole. Measured — an
                    // `<itemref/>` between two chapters gave `page_no [1, 2]`
                    // and an empty `skipped`, so the book's third chapter came
                    // back as its second.
                    //
                    // `None` rather than an empty string, and the difference is
                    // not tidiness. `items` is keyed on whatever the manifest
                    // declared, and a manifest may declare `id=""`; an empty
                    // string here would then *match* it, and the entry that
                    // names no chapter would be read as that one — the wrong
                    // chapter's text under this chapter's number, which is the
                    // one outcome worse than a skip.
                    b"itemref" if in_spine => {
                        let idref = e.attributes().flatten().find_map(|attribute| {
                            (attribute.key.local_name().as_ref() == b"idref")
                                .then(|| attribute_value(&attribute))
                                .flatten()
                        });
                        spine.push(idref);
                    }
                    _ => {}
                }
            }
            quick_xml::events::Event::End(ref e) => match e.local_name().as_ref() {
                b"manifest" => in_manifest = false,
                b"spine" => in_spine = false,
                _ => {}
            },
            _ => {}
        }
    }

    Ok(Package { items, spine })
}

/// Whether the manifest's stated media type is one this reader turns into
/// prose.
///
/// **Enumerated, and the enumeration is the decision.** `application/xhtml+xml`
/// is what EPUB 3 requires and what EPUB 2 uses; `text/html` appears in books
/// made by tools that predate the requirement. Everything else in a spine —
/// `image/svg+xml` for a plate or a cover, `application/x-dtbook+xml` for a
/// talking book — is a page this reader has nothing to say about, and running
/// the HTML parser over it would produce a block of SVG tooltips rather than
/// nothing.
///
/// A parameter after `;` is ignored (`text/html; charset=utf-8`), and the
/// comparison is case-insensitive, because a media type's type and subtype are.
fn is_content_document(media_type: Option<&str>) -> bool {
    let Some(media_type) = media_type else {
        return true;
    };
    let bare = media_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(bare.as_str(), "application/xhtml+xml" | "text/html")
}

/// The directory part of a member's path, `""` at the archive's root.
fn parent_of(path: &str) -> String {
    match path.rfind('/') {
        Some(cut) => path[..cut].to_string(),
        None => String::new(),
    }
}

/// The member an href names, or `None` if it names nothing inside this archive.
///
/// **An href in a package document is a URL, and a member's name is not.** The
/// three differences all cost a chapter when they are missed, and the second
/// has been measured to cost every chapter of a book at once:
///
/// - a fragment (`ch1.xhtml#part2`) names a place in a file, not a second file;
/// - a space, a Cyrillic letter or an ampersand in a filename is percent-encoded
///   in the href and is not in the zip entry. A book whose chapters are named in
///   the author's own language has every href encoded, so reading them as
///   written finds nothing at all and the whole book is refused as having no
///   readable chapter;
/// - `../` and `./` are path syntax that a zip entry name does not carry.
///
/// Decoded **after** the path is resolved, deliberately: `%2e%2e` is a filename
/// component that happens to spell `..`, and resolving first means it can never
/// become one. Nothing outside the archive is reachable from a zip entry name
/// anyway — this is not a defence, it is the answer being the same one a reading
/// application gives.
///
/// **There is deliberately no check for a URL scheme**, although a spine may
/// name `https://…`. One was written and then removed, because it changed no
/// outcome that anything can observe: with it a remote href resolves to `None`
/// and the chapter is skipped by number, and without it the href resolves to a
/// member called `https:/example.org/ch1.xhtml`, which no archive holds, and the
/// chapter is skipped by number. A branch no test can redden is a branch that
/// earns nothing, and `a_remote_href_is_skipped_by_number` states the outcome
/// that does matter.
fn resolve(base: &str, href: &str) -> Option<String> {
    let href = href.split(['#', '?']).next().unwrap_or("");
    if href.is_empty() {
        return None;
    }

    let (mut segments, rest) = match href.strip_prefix('/') {
        // An absolute path, from the archive's root rather than from the
        // package document's directory.
        Some(rest) => (Vec::new(), rest),
        None => (
            base.split('/')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>(),
            href,
        ),
    };
    for segment in rest.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            segment => segments.push(segment),
        }
    }
    if segments.is_empty() {
        return None;
    }
    Some(
        segments
            .iter()
            .map(|segment| percent_decode(segment))
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// The same resolution for a path that is already absolute in the archive —
/// `container.xml`'s `full-path`, which is stated relative to the root.
fn normalise(path: &str) -> String {
    resolve("", path).unwrap_or_default()
}

/// `%D0%A0` back into the bytes a zip entry name is made of.
///
/// Decoded into bytes and only then read as text, because one percent-encoded
/// character is several escapes: `Розділ` is twelve of them, and decoding each
/// escape to a `char` on its own would produce six replacement characters.
fn percent_decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // An incomplete or malformed escape is left as the literal `%` it is,
        // rather than dropped: `100%.xhtml` is a name a zip can hold.
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2]))
        {
            out.push(high * 16 + low);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A book of `chapters` members, each holding `bytes_each` repeated bytes.
    ///
    /// Deliberately compressible: the archive on disk is a few kilobytes and
    /// what comes out of it is not, which is the only shape a budget is for.
    fn book(chapters: usize, bytes_each: usize) -> Vec<u8> {
        use std::io::Write;

        let manifest: String = (0..chapters)
            .map(|n| {
                format!(
                    "<item id=\"c{n}\" href=\"ch{n}.xhtml\" media-type=\"application/xhtml+xml\"/>"
                )
            })
            .collect();
        let spine: String = (0..chapters)
            .map(|n| format!("<itemref idref=\"c{n}\"/>"))
            .collect();
        let opf = format!(
            "<package xmlns=\"http://www.idpf.org/2007/opf\">\
             <manifest>{manifest}</manifest><spine>{spine}</spine></package>"
        );
        let container = "<container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
                         <rootfiles><rootfile full-path=\"content.opf\" \
                         media-type=\"application/oebps-package+xml\"/></rootfiles></container>";

        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let stored: zip::write::FileOptions<()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            let deflated: zip::write::FileOptions<()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            w.start_file("mimetype", stored).unwrap();
            w.write_all(b"application/epub+zip").unwrap();
            w.start_file("META-INF/container.xml", deflated).unwrap();
            w.write_all(container.as_bytes()).unwrap();
            w.start_file("content.opf", deflated).unwrap();
            w.write_all(opf.as_bytes()).unwrap();
            for n in 0..chapters {
                w.start_file(format!("ch{n}.xhtml"), deflated).unwrap();
                w.write_all(b"<p>").unwrap();
                w.write_all(&vec![b'a'; bytes_each]).unwrap();
                w.write_all(b"</p>").unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    /// **The amplification `MEMBER_MAX_BYTES` alone does not stop.** A cap on
    /// one member is not a cap on a book: N chapters each just under it is the
    /// same attack with more entries, and the way this process would report
    /// gigabytes of chapters is by being killed — after which the file is not
    /// what gets blamed.
    ///
    /// Both directions. A book that fits reads; the same book one chapter
    /// longer does not, and the difference is only the total.
    #[test]
    fn a_book_draws_every_member_against_one_budget() {
        // Two chapters of 4 KiB, plus the container and the package document,
        // is under 16 KiB and over 8 KiB.
        assert!(extract(&book(2, 4096), 16 << 10).is_ok());
        assert!(matches!(
            extract(&book(2, 4096), 8 << 10),
            Err(EpubError::TooLarge)
        ));
        // And the budget really is a *total* rather than a per-member cap worn
        // twice: one chapter of the same size fits inside the budget the two
        // exhaust.
        assert!(extract(&book(1, 4096), 8 << 10).is_ok());
    }

    /// A path is resolved before it is decoded, so an escape cannot become path
    /// syntax after the fact.
    ///
    /// Both directions again: a real `..` climbs a directory, and `%2e%2e` —
    /// which decodes to the same two characters — is a filename.
    #[test]
    fn an_escape_that_spells_a_path_segment_is_a_filename() {
        assert_eq!(
            resolve("OEBPS/pkg", "../text/ch1.xhtml").as_deref(),
            Some("OEBPS/text/ch1.xhtml")
        );
        assert_eq!(
            resolve("OEBPS/pkg", "%2e%2e/ch1.xhtml").as_deref(),
            Some("OEBPS/pkg/../ch1.xhtml")
        );
    }

    /// An incomplete or malformed escape is the literal `%` it is, rather than
    /// a dropped character: `100%.xhtml` is a name a zip can hold.
    /// An incomplete escape is the literal `%` it is, rather than a dropped
    /// character: `100%.xhtml` is a name a zip can hold.
    #[test]
    fn a_percent_that_is_not_an_escape_survives() {
        assert_eq!(percent_decode("100%.xhtml"), "100%.xhtml");
        assert_eq!(percent_decode("a%zz"), "a%zz");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%D0%A0"), "Р");
    }
}

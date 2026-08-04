//! Task 11's two tests from the brief, plus the ones its three decisions imply.
//!
//! **The fixtures are built in code rather than checked in as `.epub` files**,
//! the same way `tests/zip_part.rs` builds its archives. A binary blob in a
//! public repository is a fixture nobody can read a diff of, and the cases that
//! matter here — an href that is percent-encoded, a spine entry the archive does
//! not hold, a member whose declared size lies — are ones no real book would
//! contain and no editor would let anyone author. What it costs is stated
//! rather than hidden: nothing here has met a book that Calibre or Sigil
//! actually wrote, so the quirks tested below are the ones the standard and the
//! wild are known to produce, not ones observed in a corpus.

use std::io::{Cursor, Write};

use mnema_extract::{EpubError, extract_epub};

// ---------------------------------------------------------------- the fixtures

/// One entry of a book's manifest, and the archive member behind it (or the
/// absence of one).
struct Item {
    id: String,
    /// The href **exactly as the package document writes it**: percent-encoding,
    /// `../`, a fragment and all. Every interesting case lives in the gap
    /// between this and the member's real name.
    href: String,
    media_type: Option<String>,
    /// The archive member: its name and its bytes. `None` is a manifest that
    /// names a file the archive does not hold.
    member: Option<(String, Vec<u8>)>,
}

/// The ordinary case: an XHTML chapter whose href is its member's name.
fn chapter(id: &str, href: &str, body: &str) -> Item {
    Item {
        id: id.to_string(),
        href: href.to_string(),
        media_type: Some("application/xhtml+xml".to_string()),
        member: Some((href.to_string(), body.as_bytes().to_vec())),
    }
}

/// A chapter of a book whose package document sits in `dir`: the href is
/// relative to the package document and the member's name is not.
fn chapter_under(dir: &str, id: &str, name: &str, body: &str) -> Item {
    Item {
        id: id.to_string(),
        href: name.to_string(),
        media_type: Some("application/xhtml+xml".to_string()),
        member: Some((format!("{dir}/{name}"), body.as_bytes().to_vec())),
    }
}

/// A chapter the spine names and the archive does not hold.
fn missing_chapter(id: &str, href: &str) -> Item {
    Item {
        id: id.to_string(),
        href: href.to_string(),
        media_type: Some("application/xhtml+xml".to_string()),
        member: None,
    }
}

/// An XHTML document of the shape every chapter of every book has: a `<title>`
/// in the head that no reading system paints, and a body.
fn xhtml(title: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>{title}</title></head>\
         <body>{body}</body></html>"
    )
}

/// Builds an EPUB: the uncompressed `mimetype` entry `typing::is_epub`
/// requires, a container pointing at `opf_path`, a package document holding
/// `items` and `spine`, and one member per item that has one.
fn epub(opf_path: &str, items: &[Item], spine: &[&str]) -> Vec<u8> {
    let manifest: String = items
        .iter()
        .map(|item| {
            let media_type = match &item.media_type {
                Some(media_type) => format!(" media-type=\"{media_type}\""),
                None => String::new(),
            };
            format!(
                "<item id=\"{}\" href=\"{}\"{media_type}/>",
                item.id, item.href
            )
        })
        .collect();
    let itemrefs: String = spine
        .iter()
        .map(|idref| format!("<itemref idref=\"{idref}\"/>"))
        .collect();
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"id\">\
         <metadata/><manifest>{manifest}</manifest><spine>{itemrefs}</spine></package>"
    );
    let container = format!(
        "<?xml version=\"1.0\"?>\
         <container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
         <rootfiles><rootfile full-path=\"{opf_path}\" \
         media-type=\"application/oebps-package+xml\"/></rootfiles></container>"
    );

    let mut members: Vec<(String, Vec<u8>)> = vec![
        ("META-INF/container.xml".to_string(), container.into_bytes()),
        (opf_path.to_string(), opf.into_bytes()),
    ];
    members.extend(items.iter().filter_map(|item| item.member.clone()));
    zip_with(&members)
}

/// A zip whose first entry is the stored `mimetype` an EPUB is identified by,
/// followed by `members` in the order given.
fn zip_with(members: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let stored: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        // First, uncompressed, holding exactly the media type — the three
        // things `typing::is_epub` checks (`src/typing.rs:312-330`).
        w.start_file("mimetype", stored).unwrap();
        w.write_all(b"application/epub+zip").unwrap();
        for (name, bytes) in members {
            w.start_file(name.as_str(), deflated).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap();
    }
    buf.into_inner()
}

/// Overwrites the *declared* uncompressed size of one member, in both the local
/// file header and the central directory, leaving the compressed bytes and the
/// compressed-size field that delimits them untouched.
///
/// The same forgery `tests/zip_part.rs` applies to a docx part, aimed at a
/// chapter instead — and aimed by name rather than at offset 0, because an
/// EPUB's first entry is always `mimetype`.
fn forge_declared_size(bytes: &mut [u8], member: &str, forged: u32) {
    let forged = forged.to_le_bytes();
    let name = member.as_bytes();

    // Local file header: uncompressed size at offset 22, name length at 26,
    // extra length at 28, the name itself at 30 (PKWARE APPNOTE 4.3.7).
    let local = find_header(bytes, b"PK\x03\x04", 26, 30, name).expect("a local file header");
    bytes[local + 22..local + 26].copy_from_slice(&forged);

    // Central directory header: uncompressed size at 24, name length at 28,
    // the name at 46 (APPNOTE 4.3.12).
    let central =
        find_header(bytes, b"PK\x01\x02", 28, 46, name).expect("a central directory entry");
    bytes[central + 24..central + 28].copy_from_slice(&forged);
}

/// The offset of the header with `signature` whose filename is `name`.
///
/// Matched on the name rather than on the signature alone: compressed data can
/// hold four bytes that look like a header, and an archive has one of these per
/// member anyway.
fn find_header(
    bytes: &[u8],
    signature: &[u8; 4],
    name_len_at: usize,
    name_at: usize,
    name: &[u8],
) -> Option<usize> {
    (0..bytes.len().saturating_sub(name_at)).find(|&at| {
        if &bytes[at..at + 4] != signature {
            return false;
        }
        let len =
            u16::from_le_bytes([bytes[at + name_len_at], bytes[at + name_len_at + 1]]) as usize;
        len == name.len()
            && at + name_at + len <= bytes.len()
            && &bytes[at + name_at..at + name_at + len] == name
    })
}

/// Every block of every chapter, in order — what the book's text is.
fn texts(book: &mnema_extract::EpubBook) -> Vec<&str> {
    book.chapters
        .iter()
        .flat_map(|chapter| chapter.blocks.iter().map(|block| block.text.as_str()))
        .collect()
}

// ------------------------------------------------- the brief's two tests

/// Verbatim from the brief, against a fixture built here.
///
/// The one adaptation is the return type: the brief's `Vec<EpubChapter>` cannot
/// carry the numbers of the chapters that were skipped, which its own decision 1
/// requires — so `extract_epub` answers an `EpubBook`, as `extract_pdf` answers
/// a `PdfDocument`, for the same reason.
#[test]
fn chapters_come_back_in_spine_order() {
    let book = extract_epub(&epub(
        "OEBPS/content.opf",
        &[
            chapter_under(
                "OEBPS",
                "c1",
                "ch1.xhtml",
                &xhtml("Розділ перший", "<p>Спершу було так.</p>"),
            ),
            chapter_under(
                "OEBPS",
                "c2",
                "ch2.xhtml",
                &xhtml("Розділ другий", "<p>А потім інакше.</p>"),
            ),
        ],
        &["c1", "c2"],
    ))
    .unwrap();

    assert_eq!(book.chapters.len(), 2);
    assert_eq!(
        book.chapters[0].section_title.as_deref(),
        Some("Розділ перший")
    );
    assert_eq!(
        book.chapters[1].section_title.as_deref(),
        Some("Розділ другий")
    );
    // The order is the spine's, so the *other* direction has to be asserted
    // too: a reader that sorted by member name, or that walked the manifest
    // instead of the spine, would satisfy the titles above on this fixture and
    // put the second chapter's prose first.
    assert_eq!(texts(&book), vec!["Спершу було так.", "А потім інакше."]);
}

/// Verbatim from the brief.
///
/// The server reads epub chapters with a bare `zf.read`
/// (`app/textdoc/adapters.py:118`) while docx and xlsx go through a capped
/// stream. That asymmetry is not ported: spec §9.
#[test]
fn a_chapter_is_read_under_the_same_cap_as_a_docx_part() {
    let bomb = epub_with_forged_chapter_size();
    assert!(matches!(extract_epub(&bomb), Err(EpubError::TooLarge)));
}

/// A book holding one chapter that inflates past the cap on a member while
/// declaring itself ten bytes long.
///
/// 32 MiB of one repeated byte: past `zip_part::MEMBER_MAX_BYTES`, and Deflate
/// turns it into a few kilobytes, so the archive on disk is small. That is the
/// whole shape of the attack — the number in the central directory is the
/// author's to write, and `read_member` decides on what came out of the stream.
fn epub_with_forged_chapter_size() -> Vec<u8> {
    let mut bytes = epub(
        "content.opf",
        &[Item {
            id: "c1".to_string(),
            href: "ch1.xhtml".to_string(),
            media_type: Some("application/xhtml+xml".to_string()),
            member: Some(("ch1.xhtml".to_string(), vec![b'A'; 32 << 20])),
        }],
        &["c1"],
    );
    forge_declared_size(&mut bytes, "ch1.xhtml", 10);
    bytes
}

// ------------------------------------- a chapter that is not there, and the book

/// **Decision 1 of the brief, in both directions at once.** A chapter the spine
/// names and the archive does not hold does not refuse the book, does not
/// produce a page, and is named by its spine number so the parent can journal a
/// row for it.
///
/// All three matter and none implies the others. A reader that refused would
/// remove a whole book from the index over one broken link; one that produced
/// an empty page for it would leave the parent with nothing to journal; one
/// that sent both a page and the number stops the entire job
/// (`crates/mnema-pool/src/lib.rs:1324`), which is the reason this is asserted
/// here rather than downstream.
#[test]
fn a_chapter_the_archive_does_not_hold_is_named_rather_than_refusing_the_book() {
    let book = extract_epub(&epub(
        "content.opf",
        &[
            chapter("c1", "ch1.xhtml", &xhtml("Перший", "<p>Є текст.</p>")),
            missing_chapter("c2", "ch2.xhtml"),
            chapter("c3", "ch3.xhtml", &xhtml("Третій", "<p>І тут теж.</p>")),
        ],
        &["c1", "c2", "c3"],
    ))
    .unwrap();

    assert_eq!(book.skipped, vec![2]);
    // The gap is the record: chapter 3 keeps its own number rather than moving
    // up into the hole, exactly as a PDF page does.
    assert_eq!(
        book.chapters.iter().map(|c| c.page_no).collect::<Vec<_>>(),
        vec![1, 3]
    );
    // And the number in `skipped` is in no page frame, which is the pair the
    // pool stops the job over.
    assert!(
        !book.chapters.iter().any(|c| c.page_no == 2),
        "chapter 2 was both read and reported skipped"
    );
    assert_eq!(texts(&book), vec!["Є текст.", "І тут теж."]);
}

/// The whole of the accounting, as a partition rather than as a promise: every
/// entry the spine declares is in exactly one of the two lists, and nothing is
/// in neither.
///
/// This is the invariant `pdf.rs` calls
/// `every_page_of_a_document_is_either_read_or_named`, and the loop keeps it by
/// construction — but "by construction" is what the PDF reader's first version
/// also claimed, while it stopped at the first page it could not handle and
/// left pages in neither list and in no journal row.
///
/// The fixture puts one of each failure into a six-entry spine: a chapter that
/// reads, one the archive does not hold, one whose manifest id nothing declares,
/// one that is a picture, one that is an empty file, and a second that reads.
#[test]
fn every_entry_of_the_spine_is_either_read_or_named() {
    let book = extract_epub(&epub(
        "OEBPS/content.opf",
        &[
            chapter_under(
                "OEBPS",
                "c1",
                "ch1.xhtml",
                &xhtml("Перший", "<p>Перший розділ.</p>"),
            ),
            missing_chapter("c2", "ch2.xhtml"),
            Item {
                id: "cover".to_string(),
                href: "cover.svg".to_string(),
                media_type: Some("image/svg+xml".to_string()),
                member: Some((
                    "OEBPS/cover.svg".to_string(),
                    "<svg xmlns=\"http://www.w3.org/2000/svg\"><title>Обкладинка</title></svg>"
                        .as_bytes()
                        .to_vec(),
                )),
            },
            chapter_under("OEBPS", "empty", "empty.xhtml", ""),
            chapter_under(
                "OEBPS",
                "c6",
                "ch6.xhtml",
                &xhtml("Останній", "<p>Останній розділ.</p>"),
            ),
        ],
        // `nosuch` is in no manifest at all — a spine entry naming an id that
        // was never declared.
        &["c1", "c2", "nosuch", "cover", "empty", "c6"],
    ))
    .unwrap();

    let read: Vec<u32> = book.chapters.iter().map(|c| c.page_no).collect();
    assert_eq!(read, vec![1, 6]);
    assert_eq!(book.skipped, vec![2, 3, 4, 5]);

    // The partition, stated as one: the union is the whole spine and the
    // intersection is empty. Either half alone is satisfied by a mistake —
    // a reader that named every entry skipped passes the second.
    let mut every: Vec<u32> = read.iter().chain(book.skipped.iter()).copied().collect();
    every.sort_unstable();
    assert_eq!(every, vec![1, 2, 3, 4, 5, 6]);
    every.dedup();
    assert_eq!(every.len(), 6, "an entry was both read and named skipped");
}

/// A book of pictures is refused rather than stored as a document with no text.
///
/// The same answer `pdf.rs` gives a scan, and the reason `epub.rs` needs it is
/// the same: a document with no blocks is a row in the index that answers no
/// query and tells the person who added the file nothing at all.
#[test]
fn a_book_with_no_readable_chapter_is_refused_rather_than_stored_empty() {
    let book = epub(
        "content.opf",
        &[Item {
            id: "cover".to_string(),
            href: "cover.svg".to_string(),
            media_type: Some("image/svg+xml".to_string()),
            member: Some((
                "cover.svg".to_string(),
                b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
            )),
        }],
        &["cover"],
    );
    assert!(matches!(
        extract_epub(&book),
        Err(EpubError::NoReadableChapter)
    ));
}

/// A book whose every chapter is missing says the same thing — one broken link
/// is routine, all of them is a book with nothing in it.
#[test]
fn a_book_whose_every_chapter_is_missing_is_refused() {
    let book = epub(
        "content.opf",
        &[
            missing_chapter("c1", "ch1.xhtml"),
            missing_chapter("c2", "ch2.xhtml"),
        ],
        &["c1", "c2"],
    );
    assert!(matches!(
        extract_epub(&book),
        Err(EpubError::NoReadableChapter)
    ));
}

/// …and a book that produced nothing because its archive is *damaged* says that
/// instead.
///
/// The two refusals reach different rules — `no_text_layer` and `malformed` —
/// and the difference is what the person holding the file is told to do: look
/// for a better copy, or accept that this one is pictures. A single verdict for
/// both would be wrong for one of them every time.
#[test]
fn a_book_whose_chapters_will_not_decompress_says_damaged_rather_than_no_text() {
    let mut bytes = epub(
        "content.opf",
        &[chapter(
            "c1",
            "ch1.xhtml",
            &xhtml("Перший", "<p>Текст.</p>"),
        )],
        &["c1"],
    );
    // Corrupt the chapter's compressed stream, and nothing else: the archive
    // still parses, the central directory still lists the member, and the
    // member's own bytes no longer inflate.
    let local = find_header(&bytes, b"PK\x03\x04", 26, 30, b"ch1.xhtml").expect("a local header");
    let name_len = u16::from_le_bytes([bytes[local + 26], bytes[local + 27]]) as usize;
    let extra_len = u16::from_le_bytes([bytes[local + 28], bytes[local + 29]]) as usize;
    let data = local + 30 + name_len + extra_len;
    for byte in &mut bytes[data..data + 8] {
        *byte ^= 0xff;
    }

    assert!(
        matches!(extract_epub(&bytes), Err(EpubError::Malformed(_))),
        "a damaged archive was reported as a book with no text in it"
    );
}

// ---------------------------------------------------- the structure of a book

/// No container: not a book, and refused as damaged rather than as one with no
/// text.
///
/// `typing::is_epub` checks the `mimetype` entry and nothing else, so this file
/// reaches this reader — which is the point. The check that decides the format
/// and the structure the format needs are two different claims.
#[test]
fn an_archive_with_no_container_is_damaged() {
    let bytes = zip_with(&[("OEBPS/ch1.xhtml".to_string(), b"<p>text</p>".to_vec())]);
    assert!(matches!(extract_epub(&bytes), Err(EpubError::Malformed(_))));
}

/// A container that points at a package document the archive does not hold.
#[test]
fn a_container_pointing_at_nothing_is_damaged() {
    // A chapter is present and the package document that would name it is not:
    // the container points at a member the archive does not hold.
    let bytes = zip_with(&[
        (
            "META-INF/container.xml".to_string(),
            b"<?xml version=\"1.0\"?><container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
              <rootfiles><rootfile full-path=\"OEBPS/content.opf\" \
              media-type=\"application/oebps-package+xml\"/></rootfiles></container>"
                .to_vec(),
        ),
        (
            "OEBPS/ch1.xhtml".to_string(),
            "<p>Є.</p>".as_bytes().to_vec(),
        ),
    ]);
    assert!(matches!(extract_epub(&bytes), Err(EpubError::Malformed(_))));
}

/// A spine with no entries at all. Invalid per the standard, and told apart
/// from a book whose chapters yielded nothing: there is nothing here that was
/// ever going to yield anything.
#[test]
fn a_package_document_with_an_empty_spine_is_damaged() {
    let bytes = epub(
        "content.opf",
        &[chapter("c1", "ch1.xhtml", &xhtml("Перший", "<p>Є.</p>"))],
        &[],
    );
    assert!(matches!(extract_epub(&bytes), Err(EpubError::Malformed(_))));
}

/// XML that does not parse is damage, and it is the one place this reader uses
/// an XML parser at all — see `epub.rs`'s module doc for why a *chapter* is not
/// read this way.
#[test]
fn a_package_document_that_does_not_parse_is_damaged() {
    let bytes = zip_with(&[
        (
            "META-INF/container.xml".to_string(),
            b"<?xml version=\"1.0\"?><container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
              <rootfiles><rootfile full-path=\"content.opf\" \
              media-type=\"application/oebps-package+xml\"/></rootfiles></container>"
                .to_vec(),
        ),
        (
            "content.opf".to_string(),
            b"<package><manifest><item id=\"c1\" href=\"ch1.xhtml\"".to_vec(),
        ),
    ]);
    assert!(matches!(extract_epub(&bytes), Err(EpubError::Malformed(_))));
}

/// A package document written with a namespace prefix reads the same as one
/// without.
///
/// Both spellings are legal and both are in the wild; matching on the qualified
/// name would find no manifest in this one, and a book whose every chapter is
/// unknown is refused as having no readable chapter — a whole book lost to a
/// colon.
#[test]
fn a_package_document_written_with_a_prefix_reads_the_same() {
    let bytes = zip_with(&[
        (
            "META-INF/container.xml".to_string(),
            b"<?xml version=\"1.0\"?><ocf:container xmlns:ocf=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
              <ocf:rootfiles><ocf:rootfile full-path=\"content.opf\" \
              media-type=\"application/oebps-package+xml\"/></ocf:rootfiles></ocf:container>"
                .to_vec(),
        ),
        (
            "content.opf".to_string(),
            b"<?xml version=\"1.0\"?><opf:package xmlns:opf=\"http://www.idpf.org/2007/opf\">\
              <opf:manifest><opf:item id=\"c1\" href=\"ch1.xhtml\" \
              media-type=\"application/xhtml+xml\"/></opf:manifest>\
              <opf:spine><opf:itemref idref=\"c1\"/></opf:spine></opf:package>"
                .to_vec(),
        ),
        (
            "ch1.xhtml".to_string(),
            xhtml("Перший", "<p>Прочитано попри двокрапку.</p>").into_bytes(),
        ),
    ]);
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(texts(&book), vec!["Прочитано попри двокрапку."]);
}

/// A container listing two renditions takes the one whose media type says it is
/// a package document, not whichever was written first.
#[test]
fn a_container_takes_the_rootfile_that_says_it_is_a_package_document() {
    let bytes = zip_with(&[
        (
            "META-INF/container.xml".to_string(),
            b"<?xml version=\"1.0\"?><container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
              <rootfiles>\
              <rootfile full-path=\"other.xml\" media-type=\"application/something+xml\"/>\
              <rootfile full-path=\"content.opf\" media-type=\"application/oebps-package+xml\"/>\
              </rootfiles></container>"
                .to_vec(),
        ),
        ("other.xml".to_string(), b"<nothing/>".to_vec()),
        (
            "content.opf".to_string(),
            b"<?xml version=\"1.0\"?><package xmlns=\"http://www.idpf.org/2007/opf\">\
              <manifest><item id=\"c1\" href=\"ch1.xhtml\" media-type=\"application/xhtml+xml\"/>\
              </manifest><spine><itemref idref=\"c1\"/></spine></package>"
                .to_vec(),
        ),
        (
            "ch1.xhtml".to_string(),
            xhtml("Перший", "<p>Друга з двох.</p>").into_bytes(),
        ),
    ]);
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(texts(&book), vec!["Друга з двох."]);
}

// ------------------------------------------------- what an href is, and is not

/// **The case that costs a whole book at once.** A book whose chapters are
/// named in the author's own language has every href percent-encoded, and a
/// zip entry's name is not: read as written, not one chapter is found, every
/// one of them is skipped, and the book is refused as having no readable
/// chapter. Nothing anywhere says why.
#[test]
fn a_percent_encoded_href_names_the_member_it_decodes_to() {
    let bytes = epub(
        "OEBPS/content.opf",
        &[Item {
            id: "c1".to_string(),
            href: "%D0%A0%D0%BE%D0%B7%D0%B4%D1%96%D0%BB%201.xhtml".to_string(),
            media_type: Some("application/xhtml+xml".to_string()),
            member: Some((
                "OEBPS/Розділ 1.xhtml".to_string(),
                xhtml("Розділ 1", "<p>Знайдено за декодованим іменем.</p>").into_bytes(),
            )),
        }],
        &["c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(texts(&book), vec!["Знайдено за декодованим іменем."]);
    assert!(book.skipped.is_empty(), "{:?}", book.skipped);
}

/// An href is relative to the package document, and carries path syntax a zip
/// entry name does not.
///
/// Three at once because each alone is satisfied by a reader that ignores
/// paths entirely: a plain relative name, one that climbs out of the package
/// document's directory, and one that is absolute from the archive's root.
#[test]
fn an_href_is_resolved_against_the_package_document() {
    // Every item states its href and its member name separately: that gap is
    // exactly what resolution has to close, and `chapter()`'s shortcut (href ==
    // member name) would close it by construction.
    let bytes = epub(
        "OEBPS/pkg/content.opf",
        &[
            Item {
                id: "near".to_string(),
                href: "near.xhtml".to_string(),
                media_type: Some("application/xhtml+xml".to_string()),
                member: Some((
                    "OEBPS/pkg/near.xhtml".to_string(),
                    xhtml("Поруч", "<p>Поруч із пакетом.</p>").into_bytes(),
                )),
            },
            Item {
                id: "up".to_string(),
                href: "../text/up.xhtml".to_string(),
                media_type: Some("application/xhtml+xml".to_string()),
                member: Some((
                    "OEBPS/text/up.xhtml".to_string(),
                    xhtml("Вище", "<p>Каталогом вище.</p>").into_bytes(),
                )),
            },
            Item {
                id: "root".to_string(),
                href: "/root.xhtml".to_string(),
                media_type: Some("application/xhtml+xml".to_string()),
                member: Some((
                    "root.xhtml".to_string(),
                    xhtml("Корінь", "<p>Від кореня архіву.</p>").into_bytes(),
                )),
            },
        ],
        &["near", "up", "root"],
    );

    let book = extract_epub(&bytes).unwrap();
    assert!(book.skipped.is_empty(), "{:?}", book.skipped);
    assert_eq!(
        texts(&book),
        vec!["Поруч із пакетом.", "Каталогом вище.", "Від кореня архіву."]
    );
}

/// A fragment names a place inside a file, not a second file.
#[test]
fn a_fragment_in_an_href_is_not_part_of_the_member_name() {
    let bytes = epub(
        "content.opf",
        &[Item {
            id: "c1".to_string(),
            href: "ch1.xhtml#part2".to_string(),
            media_type: Some("application/xhtml+xml".to_string()),
            member: Some((
                "ch1.xhtml".to_string(),
                xhtml("Перший", "<p>Після ґратки.</p>").into_bytes(),
            )),
        }],
        &["c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(texts(&book), vec!["Після ґратки."]);
}

/// A spine entry naming something outside the archive is skipped by number, not
/// looked for as a member called `http:`.
#[test]
fn a_remote_href_is_skipped_by_number() {
    let bytes = epub(
        "content.opf",
        &[
            Item {
                id: "remote".to_string(),
                href: "https://example.org/ch1.xhtml".to_string(),
                media_type: Some("application/xhtml+xml".to_string()),
                member: None,
            },
            chapter("c2", "ch2.xhtml", &xhtml("Другий", "<p>Місцевий.</p>")),
        ],
        &["remote", "c2"],
    );
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(book.skipped, vec![1]);
    assert_eq!(texts(&book), vec!["Місцевий."]);
}

/// The manifest's stated media type decides what is a chapter, and a picture in
/// the spine is skipped rather than mined for the text inside its markup.
///
/// Both directions: an SVG plate produces no page, and `text/html` — which books
/// made before EPUB 3 use — still does.
#[test]
fn a_spine_entry_that_is_not_a_content_document_is_skipped() {
    let bytes = epub(
        "content.opf",
        &[
            Item {
                id: "plate".to_string(),
                href: "plate.svg".to_string(),
                media_type: Some("image/svg+xml".to_string()),
                member: Some((
                    "plate.svg".to_string(),
                    "<svg xmlns=\"http://www.w3.org/2000/svg\"><text>Мапа Європи</text></svg>"
                        .as_bytes()
                        .to_vec(),
                )),
            },
            Item {
                id: "old".to_string(),
                href: "old.html".to_string(),
                media_type: Some("text/html".to_string()),
                member: Some((
                    "old.html".to_string(),
                    xhtml("Старий", "<p>Книжка з двохтисячних.</p>").into_bytes(),
                )),
            },
        ],
        &["plate", "old"],
    );
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(book.skipped, vec![1]);
    assert_eq!(texts(&book), vec!["Книжка з двохтисячних."]);
    // The plate's own words are in no block: an SVG read by the HTML parser
    // gives back its labels, which is markup reaching a chunk by another route.
    assert!(
        !texts(&book).iter().any(|text| text.contains("Мапа")),
        "{:?}",
        texts(&book)
    );
}

// ------------------------------------------------ what a chapter's page holds

/// **The cover page, and the reason a chapter's `<title>` is not a block.**
///
/// Every chapter of every book carries a `<title>` in its head; no reading
/// system paints it. A cover is a body holding one image, so keeping the title
/// as text would make it a chapter whose entire indexed content is the word
/// `Обкладинка` — a page that answers a search and shows a picture. Dropping it
/// leaves the chapter with no blocks, which is what lets it be named as skipped
/// instead.
#[test]
fn a_cover_page_is_named_as_skipped_rather_than_indexed_as_its_own_title() {
    let bytes = epub(
        "content.opf",
        &[
            chapter(
                "cover",
                "cover.xhtml",
                &xhtml("Обкладинка", "<img src=\"cover.jpg\" alt=\"\"/>"),
            ),
            chapter(
                "c1",
                "ch1.xhtml",
                &xhtml("Перший", "<p>Справжній текст.</p>"),
            ),
        ],
        &["cover", "c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(book.skipped, vec![1]);
    assert_eq!(texts(&book), vec!["Справжній текст."]);
    assert!(
        !texts(&book).iter().any(|text| text.contains("Обкладинка")),
        "a chapter's tab label reached the index: {:?}",
        texts(&book)
    );
}

/// …and the other direction, which the cover does not cover: the title still
/// *names* the chapter it is not stored in.
///
/// `pages_of` cites an epub chunk as `Coordinate::Section` and renders an
/// unnamed page as the empty string (spec §6, invariant 1), so a chapter with
/// no heading in its body would otherwise cite nothing at all.
#[test]
fn a_chapters_title_names_it_without_being_its_text() {
    let bytes = epub(
        "content.opf",
        &[chapter(
            "c1",
            "ch1.xhtml",
            &xhtml("Передмова", "<p>Текст без жодного заголовка.</p>"),
        )],
        &["c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(book.chapters[0].section_title.as_deref(), Some("Передмова"));
    assert_eq!(texts(&book), vec!["Текст без жодного заголовка."]);
}

/// A chapter with headings in it is still one page, and its blocks are numbered
/// once across the whole chapter.
///
/// The HTML reader makes a page per heading and restarts `reading_order` on
/// each; several of its pages become one page here. Left alone, this chapter
/// would hold three blocks claiming position 0 — and the index's uniqueness is
/// on `(page_id, reading_order)`, so the rows collide or the page loses its
/// order.
#[test]
fn a_chapters_blocks_are_numbered_once_across_the_whole_chapter() {
    let bytes = epub(
        "content.opf",
        &[chapter(
            "c1",
            "ch1.xhtml",
            &xhtml(
                "Розділ",
                "<p>Вступ.</p><h1>Перша частина</h1><p>Один.</p><h1>Друга частина</h1><p>Два.</p>",
            ),
        )],
        &["c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(book.chapters.len(), 1, "a chapter is one page");
    let orders: Vec<i64> = book.chapters[0]
        .blocks
        .iter()
        .map(|block| block.reading_order)
        .collect();
    assert_eq!(orders, (0..orders.len() as i64).collect::<Vec<_>>());
    // Both directions: the numbers are consecutive *and* there are as many
    // blocks as the chapter shows. A reader that dropped everything after the
    // first heading would satisfy the assertion above with one block.
    assert_eq!(
        texts(&book),
        vec!["Вступ.", "Перша частина", "Один.", "Друга частина", "Два."]
    );
}

/// The chapter is named by the first name anything in it gives itself — its
/// heading, when it has no title of its own.
#[test]
fn a_chapter_with_no_title_is_named_by_its_first_heading() {
    let bytes = epub(
        "content.opf",
        &[chapter(
            "c1",
            "ch1.xhtml",
            "<html><body><p>Вступ.</p><h1>Початок</h1><p>Далі.</p></body></html>",
        )],
        &["c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(book.chapters[0].section_title.as_deref(), Some("Початок"));
}

/// The same chapter twice in the spine is two pages, not one.
///
/// It is legal, it is how a book repeats an interstitial, and collapsing the
/// two would leave a spine number naming nothing while the pool counts pages
/// against the header.
#[test]
fn a_chapter_listed_twice_in_the_spine_is_two_pages() {
    let bytes = epub(
        "content.opf",
        &[chapter(
            "c1",
            "ch1.xhtml",
            &xhtml("Той самий", "<p>Двічі.</p>"),
        )],
        &["c1", "c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(
        book.chapters.iter().map(|c| c.page_no).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(texts(&book), vec!["Двічі.", "Двічі."]);
}

// -------------------------------------------------------------- the verbatim

/// Task 10's step 6, repeated here with a chapter of its own.
///
/// An invariant checked in one reader of five is an invariant missing from
/// four, and this one is the whole design: what is stored is what the file
/// shows, after NFC and after nothing else. The server's
/// `_clean = " ".join(text.split())` (`app/textdoc/html_blocks.py:41-42`) is
/// applied to epub there and is **not** ported (G7.1 §2.3).
#[test]
fn a_chapters_text_is_verbatim_after_nfc_and_nothing_else() {
    let bytes = epub(
        "content.opf",
        &[chapter(
            "c1",
            "ch1.xhtml",
            // Cyrillic й as a decomposed pair, plus a tab and a non-breaking
            // space.
            &xhtml("Розділ", "<p>и\u{0306}  a\tb\u{00a0}c</p>"),
        )],
        &["c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    let text = &book.chapters[0].blocks[0].text;

    assert!(text.starts_with('й'), "NFC did not run (D32): {text:?}");
    assert!(text.contains("  "), "whitespace was collapsed: {text:?}");
    assert!(text.contains('\t'), "a tab was rewritten: {text:?}");
    assert!(
        text.contains('\u{00a0}'),
        "a non-breaking space was folded: {text:?}"
    );
    // Exactly, so that a reader which trimmed one end and not the other cannot
    // pass on the four assertions above.
    assert_eq!(text, "й  a\tb\u{00a0}c");
}

/// **NFC runs on the text taken out of the tree, and XHTML is where that
/// matters most.** A chapter written by a tool that escapes non-ASCII emits
/// `&#1080;&#774;` rather than the characters themselves; normalising the
/// source composes nothing, because until the parser has decoded the references
/// there is no combining mark in the document to compose.
#[test]
fn a_combining_mark_written_as_a_character_reference_is_composed_in_a_chapter_too() {
    let bytes = epub(
        "content.opf",
        &[chapter(
            "c1",
            "ch1.xhtml",
            &xhtml("Розділ", "<p>&#1080;&#774;од</p>"),
        )],
        &["c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(texts(&book), vec!["йод"]);
}

/// No block of a chapter claims a line number.
///
/// `pages_of` gives this reader `Fixed(Coordinate::Section)` *because* these
/// blocks carry no rows: the chapter's own line numbers do not survive the
/// parse, and a number invented here is cited as "рядки 1–1" of a document that
/// has none.
#[test]
fn no_chapter_block_claims_a_line_number() {
    let bytes = epub(
        "content.opf",
        &[chapter(
            "c1",
            "ch1.xhtml",
            &xhtml("Розділ", "<p>Один.</p><p>Два.</p>"),
        )],
        &["c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    let blocks = &book.chapters[0].blocks;
    assert_eq!(blocks.len(), 2);
    assert!(
        blocks
            .iter()
            .all(|block| block.line_start.is_none() && block.line_end.is_none()),
        "{blocks:?}"
    );
}

// ------------------------------------------- what the two parsers do differently

/// **The measurement behind reading a chapter with the HTML parser rather than
/// as XML.** XML stops at the first well-formedness error; an HTML parser has
/// no failure mode at all. This chapter has a bare `&` and an unclosed tag —
/// both of which real producers emit and every reading application renders —
/// and the words after them are what an XML parser would lose.
#[test]
fn a_chapter_that_is_not_well_formed_xml_is_still_read_to_the_end() {
    let bytes = epub(
        "content.opf",
        &[chapter(
            "c1",
            "ch1.xhtml",
            "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Розділ</title></head>\
             <body><p>Чай & кава<p>Наступний абзац.</body></html>",
        )],
        &["c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(texts(&book), vec!["Чай & кава", "Наступний абзац."]);
}

/// A chapter in UTF-16, which EPUB permits and which no 8-bit detector can
/// guess.
///
/// `encoding_rs::Encoding::decode` honours a byte order mark over the encoding
/// it was asked for, so `text::decode`'s guess is overruled by the file's own
/// statement — the stated fact beating the plausible proxy. Without it the
/// chapter comes back as mojibake with a NUL between every letter: text the
/// file does not contain, indexed and searchable.
#[test]
fn a_chapter_in_utf16_is_read_as_the_text_it_is() {
    let source = xhtml("Розділ", "<p>Виторг зріс.</p>");
    let mut utf16 = vec![0xff, 0xfe];
    for unit in source.encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    let bytes = epub(
        "content.opf",
        &[Item {
            id: "c1".to_string(),
            href: "ch1.xhtml".to_string(),
            media_type: Some("application/xhtml+xml".to_string()),
            member: Some(("ch1.xhtml".to_string(), utf16)),
        }],
        &["c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(texts(&book), vec!["Виторг зріс."]);
}

/// A book of many chapters is read in an amount of time worth writing down.
///
/// `zip_part::read_member` reopens the archive per member, so a book of N
/// chapters parses the central directory N times — O(N²) in the number of
/// entries. Measured rather than argued: this fixture is 500 chapters, which is
/// past any real book, and the assertion is only that it finishes. The number
/// itself is in `epub.rs`'s module doc.
#[test]
fn a_book_of_five_hundred_chapters_is_read() {
    let items: Vec<Item> = (0..500)
        .map(|n| {
            chapter(
                &format!("c{n}"),
                &format!("ch{n}.xhtml"),
                &xhtml(
                    &format!("Розділ {n}"),
                    &format!("<p>Текст розділу {n}.</p>"),
                ),
            )
        })
        .collect();
    let spine: Vec<String> = (0..500).map(|n| format!("c{n}")).collect();
    let spine: Vec<&str> = spine.iter().map(String::as_str).collect();

    let started = std::time::Instant::now();
    let book = extract_epub(&epub("content.opf", &items, &spine)).unwrap();
    eprintln!("500 chapters in {:?}", started.elapsed());

    assert_eq!(book.chapters.len(), 500);
    assert!(book.skipped.is_empty());
    assert_eq!(book.chapters[499].blocks[0].text, "Текст розділу 499.");
}

/// A manifest entry that states no media type at all is read as a chapter.
///
/// Invalid per the standard and shipped anyway. The direction of the guess is
/// what is being fixed here: an item with no stated type is far more likely to
/// be the XHTML that almost every spine entry is than the SVG plate that a few
/// are, and the cost of being wrong is a block of tooltip text rather than a
/// chapter that vanishes.
#[test]
fn a_manifest_entry_that_states_no_media_type_is_still_read() {
    let bytes = epub(
        "content.opf",
        &[Item {
            id: "c1".to_string(),
            href: "ch1.xhtml".to_string(),
            media_type: None,
            member: Some((
                "ch1.xhtml".to_string(),
                xhtml("Розділ", "<p>Без media-type.</p>").into_bytes(),
            )),
        }],
        &["c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    assert_eq!(texts(&book), vec!["Без media-type."]);
}

/// **A package document in windows-1251, which EPUB forbids and producers have
/// shipped.**
///
/// This is the fixture that decided which decoder reads the structure. Strict
/// UTF-8 — "XML states its encoding, so do not guess" — turns every Cyrillic
/// href here into replacement characters, matches no member, skips every
/// chapter and refuses the book as having no text in it. `text::decode`'s guess
/// reads it. Measured both ways before the choice was made; the table is in
/// `epub.rs`, above `resolve`'s decoder note.
///
/// The href is written unencoded, which is also what such producers do.
#[test]
fn a_package_document_in_a_legacy_encoding_is_still_read() {
    let opf = "<?xml version=\"1.0\" encoding=\"windows-1251\"?>\
               <package xmlns=\"http://www.idpf.org/2007/opf\" version=\"2.0\">\
               <metadata><title>Книжка про мову</title>\
               <creator>Автор Авторенко</creator>\
               <description>Опис книжки, достатньо довгий, щоб детектор мав із чим працювати, \
               бо на двох літерах не має чого визначати.</description></metadata>\
               <manifest><item id=\"c1\" href=\"Розділ.xhtml\" \
               media-type=\"application/xhtml+xml\"/></manifest>\
               <spine><itemref idref=\"c1\"/></spine></package>";
    let (opf_1251, _, had_errors) = encoding_rs::WINDOWS_1251.encode(opf);
    assert!(!had_errors, "the fixture is not representable in cp1251");

    let bytes = zip_with(&[
        (
            "META-INF/container.xml".to_string(),
            b"<container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
              <rootfiles><rootfile full-path=\"content.opf\" \
              media-type=\"application/oebps-package+xml\"/></rootfiles></container>"
                .to_vec(),
        ),
        ("content.opf".to_string(), opf_1251.into_owned()),
        (
            "Розділ.xhtml".to_string(),
            xhtml("Розділ", "<p>Знайдено попри кодування.</p>").into_bytes(),
        ),
    ]);

    let book = extract_epub(&bytes).unwrap();
    assert_eq!(texts(&book), vec!["Знайдено попри кодування."]);
    assert!(book.skipped.is_empty(), "{:?}", book.skipped);
}

/// **One chapter whose stream will not inflate does not cost the book**, and it
/// is the same argument as a chapter that is not there at all: a book is not
/// damaged because one member of it is.
///
/// The pair matters, and neither half implies the other. The book above, whose
/// *every* chapter is corrupt, is refused as damaged; this one, with a single
/// bad chapter among good ones, is read and names the bad one. A reader that
/// returned `Malformed` from the chapter loop satisfies the first and loses the
/// whole book here.
#[test]
fn one_chapter_that_will_not_decompress_does_not_cost_the_book() {
    let mut bytes = epub(
        "content.opf",
        &[
            chapter("c1", "ch1.xhtml", &xhtml("Перший", "<p>Цілий розділ.</p>")),
            chapter(
                "c2",
                "ch2.xhtml",
                &xhtml("Другий", "<p>Цей не розпакується.</p>"),
            ),
            chapter("c3", "ch3.xhtml", &xhtml("Третій", "<p>І цей цілий.</p>")),
        ],
        &["c1", "c2", "c3"],
    );
    let local = find_header(&bytes, b"PK\x03\x04", 26, 30, b"ch2.xhtml").expect("a local header");
    let name_len = u16::from_le_bytes([bytes[local + 26], bytes[local + 27]]) as usize;
    let extra_len = u16::from_le_bytes([bytes[local + 28], bytes[local + 29]]) as usize;
    let data = local + 30 + name_len + extra_len;
    for byte in &mut bytes[data..data + 8] {
        *byte ^= 0xff;
    }

    let book = extract_epub(&bytes).expect("one bad chapter is not a bad book");
    assert_eq!(book.skipped, vec![2]);
    assert_eq!(texts(&book), vec!["Цілий розділ.", "І цей цілий."]);
}

/// A chapter's name is bounded by the one rule every reader shares.
///
/// `markdown::bound_section_title` is reached through the HTML reader here
/// rather than called directly, which is the point: `section_title` is one
/// column shown by one interface, and four readers each deciding how long a
/// name may be is four numbers. This states the contract as *this* reader's, so
/// that a later version taking the name from somewhere else — the navigation
/// document, say — cannot quietly stop bounding it.
#[test]
fn a_chapters_name_is_bounded_by_the_rule_every_reader_shares() {
    let long = "Розділ ".repeat(60);
    let bytes = epub(
        "content.opf",
        &[chapter("c1", "ch1.xhtml", &xhtml(&long, "<p>Текст.</p>"))],
        &["c1"],
    );
    let book = extract_epub(&bytes).unwrap();
    let title = book.chapters[0]
        .section_title
        .as_deref()
        .expect("the chapter is named");

    assert_eq!(
        title.chars().count(),
        mnema_extract::SECTION_TITLE_MAX_CHARS
    );
    // Cut *visibly*, and the ellipsis is the whole of what makes it visible: a
    // name silently truncated reads as the name the chapter has.
    assert!(title.ends_with('…'), "{title:?}");
    // And the other direction, so that a reader returning a constant string
    // could not pass: what is kept is this chapter's own name, from the front.
    assert!(title.starts_with("Розділ Розділ"), "{title:?}");
    // A short name is not touched at all.
    let bytes = epub(
        "content.opf",
        &[chapter(
            "c1",
            "ch1.xhtml",
            &xhtml("Коротко", "<p>Текст.</p>"),
        )],
        &["c1"],
    );
    assert_eq!(
        extract_epub(&bytes).unwrap().chapters[0]
            .section_title
            .as_deref(),
        Some("Коротко")
    );
}

//! Task 6a's four tests, verbatim from the brief, plus the coverage Step 3
//! asks for but the brief's four assertions do not reach on their own: a
//! positive match for each zip-shaped format, and `source_kind` for csv and
//! for source code. Without those, `identify`'s genuine-member branches and
//! the code/data split would be implemented but never actually exercised.

use std::io::Write;

use mnema_core::SourceKind;
use mnema_extract::typing::{Reader, identify};

/// Builds a minimal zip archive containing exactly one member, `name`, with
/// `contents`. `stored` picks `Stored` (uncompressed, what epub's `mimetype`
/// member requires) over `Deflated` (what every other member here uses).
fn zip_with_member(name: &str, contents: &[u8], stored: bool) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let method = if stored {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        };
        let opts: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(method);
        w.start_file(name, opts).unwrap();
        w.write_all(contents).unwrap();
        w.finish().unwrap();
    }
    buf.into_inner()
}

/// A zip archive with one member, deliberately not `member`: a bare zip
/// renamed to look like an office format that does not actually carry the
/// part that format requires.
fn zip_without_member(member: &str) -> Vec<u8> {
    assert_ne!(
        member, "readme.txt",
        "the fixture must not accidentally provide it"
    );
    zip_with_member("readme.txt", b"not an office document", false)
}

#[test]
fn magic_beats_a_lying_extension() {
    let pdf = b"%PDF-1.7\n...";
    assert_eq!(
        identify(pdf, Some("txt")).mime,
        "application/pdf",
        "the server states this outright: app/ingest/extract.py:67"
    );
}

#[test]
fn a_bare_zip_renamed_docx_is_not_a_docx() {
    let zip = zip_without_member("word/document.xml");
    assert_ne!(
        identify(&zip, Some("docx")).mime,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    );
}

#[test]
fn plain_text_falls_back_to_the_extension_and_says_so() {
    let src = b"# heading\n\ntext\n";
    assert_eq!(identify(src, Some("md")).mime, "text/markdown");
    assert_eq!(identify(src, Some("txt")).mime, "text/plain");
    assert_eq!(
        identify(src, None).mime,
        "text/plain",
        "no extension, no better guess"
    );
}

#[test]
fn the_same_bytes_under_two_text_extensions_are_one_document_with_two_readings() {
    // notes.txt and notes.md have one document id (sha256 of bytes) but different readers.
    // This is the D41 hole: the winner is whichever path the walk saw first, and that is
    // recorded rather than resolved here — resolving it belongs to the watched-folder spec,
    // which owns walk order.
    assert_ne!(
        identify(b"# x\n", Some("md")).mime,
        identify(b"# x\n", Some("txt")).mime
    );
}

// --- Supplementary: Step 3's other stated rules, not reached above ---

#[test]
fn a_genuine_docx_is_recognized_by_its_required_member() {
    let docx = zip_with_member("word/document.xml", b"<w:document/>", false);
    let file_type = identify(&docx, Some("docx"));
    assert_eq!(
        file_type.mime,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    );
    assert_eq!(file_type.reader, Reader::Docx);
    assert_eq!(file_type.source_kind, SourceKind::Document);
}

#[test]
fn a_genuine_xlsx_is_recognized_by_its_required_member() {
    let xlsx = zip_with_member("xl/workbook.xml", b"<workbook/>", false);
    let file_type = identify(&xlsx, Some("xlsx"));
    assert_eq!(
        file_type.mime,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );
    assert_eq!(file_type.reader, Reader::Xlsx);
}

#[test]
fn a_genuine_epub_is_recognized_by_its_uncompressed_mimetype_entry() {
    let epub = zip_with_member("mimetype", b"application/epub+zip", true);
    let file_type = identify(&epub, Some("epub"));
    assert_eq!(file_type.mime, "application/epub+zip");
    assert_eq!(file_type.reader, Reader::Epub);
}

#[test]
fn a_compressed_mimetype_entry_does_not_count_as_the_epub_signature() {
    // Same name, same content, wrong storage: epub requires the first entry to
    // be uncompressed. A deflated one is the server's docx/xlsx-style member
    // check ported past its own rule, not a real epub.
    let not_epub = zip_with_member("mimetype", b"application/epub+zip", false);
    assert_ne!(
        identify(&not_epub, Some("epub")).mime,
        "application/epub+zip"
    );
}

#[test]
fn csv_is_data_source_kind() {
    let file_type = identify(b"a,b,c\n1,2,3\n", Some("csv"));
    assert_eq!(file_type.mime, "text/csv");
    assert_eq!(file_type.source_kind, SourceKind::Data);
    assert_eq!(file_type.reader, Reader::PlainText);
}

#[test]
fn a_recognized_source_extension_is_code_source_kind() {
    let file_type = identify(b"fn main() {}\n", Some("rs"));
    assert_eq!(file_type.source_kind, SourceKind::Code);
    assert_eq!(file_type.reader, Reader::PlainText);
}

#[test]
fn an_unrecognized_extension_falls_back_to_document_like_no_extension_at_all() {
    let file_type = identify(b"whatever this is\n", Some("xyz-not-a-real-extension"));
    assert_eq!(file_type.mime, "text/plain");
    assert_eq!(file_type.source_kind, SourceKind::Document);
}

#[test]
fn a_truncated_zip_is_not_mistaken_for_any_office_format_and_does_not_panic() {
    // The zip signature with no valid archive behind it: a file that started
    // downloading and stopped. `identify` never returns a `Result`, so the
    // only way this can go wrong is a panic — there is no reader for this
    // either, but it must come back as *some* `FileType`, not a crash.
    let truncated = b"PK\x03\x04truncated, not a real archive";
    let file_type = identify(truncated, Some("docx"));
    assert_ne!(
        file_type.mime,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    );
}

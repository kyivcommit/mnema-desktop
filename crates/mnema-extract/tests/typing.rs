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

/// D51. `identify`'s fourth branch used to mean "plain text" for anything
/// it did not recognise, so a correctly named photo came back as
/// `text/plain` and was read as one. Measured before the fix, through
/// this same function: `photo.jpg` with extension `jpg`, `photo.png`,
/// `photo.heic`, `data.sqlite`, a Mach-O binary with no extension and
/// random bytes named `.mp4` — all six `mime=text/plain reader=PlainText`.
/// A photo now reaches the image reader by its magic bytes; what is not a
/// picture and not text is `binary.bin`'s case below.
#[test]
fn a_correctly_named_photo_is_not_plain_text() {
    let png = include_bytes!("fixtures/solid.png");
    let t = identify(png, Some("png"));
    assert_eq!(t.reader, Reader::Image);
    assert_eq!(t.mime, "image/png");
    assert_eq!(t.source_kind, SourceKind::Document);

    // The extension is not what decides it, in either direction: the same
    // bytes are a picture named `png`, named `txt`, and named nothing.
    assert_eq!(identify(png, None).reader, Reader::Image);
    assert_eq!(identify(png, Some("txt")).reader, Reader::Image);
}

/// Every picture format the image reader takes is named by its own magic.
#[test]
fn each_picture_format_is_recognized_by_its_magic() {
    let cases: [(&[u8], &str); 6] = [
        (include_bytes!("fixtures/image-uk.png"), "image/png"),
        (include_bytes!("fixtures/image-uk.jpg"), "image/jpeg"),
        (include_bytes!("fixtures/image-uk.heic"), "image/heic"),
        (include_bytes!("fixtures/image-uk.webp"), "image/webp"),
        (include_bytes!("fixtures/image-uk-gray.png"), "image/png"),
        (
            include_bytes!("fixtures/image-three-frames.tif"),
            "image/tiff",
        ),
    ];
    for (bytes, mime) in cases {
        let t = identify(bytes, None);
        assert_eq!((t.reader, t.mime), (Reader::Image, mime));
        assert_eq!(t.source_kind, SourceKind::Document);
    }
    // The fixture is big-endian (`MM\0*`); little-endian (`II*\0`) is built.
    let mut le = vec![0x49, 0x49, 0x2A, 0x00];
    le.extend_from_slice(&[0u8; 16]);
    let t = identify(&le, None);
    assert_eq!((t.reader, t.mime), (Reader::Image, "image/tiff"));
}

/// HEIF shares `ftyp` with MP4/MOV, so only the major brand says picture.
#[test]
fn an_mp4_is_not_a_heif_picture() {
    let t = identify(include_bytes!("fixtures/not-heif.mp4"), Some("heic"));
    assert_eq!(t.reader, Reader::NotText);
}

/// Content decides: a JPEG named `txt` or nothing is still a picture.
#[test]
fn a_jpeg_is_a_picture_whatever_it_is_named() {
    let jpg = include_bytes!("fixtures/image-uk.jpg");
    assert_eq!(identify(jpg, Some("txt")).reader, Reader::Image);
    assert_eq!(identify(jpg, None).reader, Reader::Image);
}

/// A magic that is only partly there is not a picture, and a short file must
/// not panic the slicing.
#[test]
fn a_partial_magic_is_not_a_picture() {
    let mut riff_wave = b"RIFF".to_vec();
    riff_wave.extend_from_slice(&[0, 0, 0, 0]);
    riff_wave.extend_from_slice(b"WAVE");
    riff_wave.extend_from_slice(&[0u8; 8]);
    let mut isom = vec![0, 0, 0, 0x18];
    isom.extend_from_slice(b"ftypisom");
    isom.extend_from_slice(&[0u8; 8]);
    let mut short_ftyp = vec![0, 0, 0, 0x18];
    short_ftyp.extend_from_slice(b"ftyp"); // 8 bytes: no brand to read
    let mut heic_cut = short_ftyp.clone();
    heic_cut.extend_from_slice(b"hei"); // 11 bytes: brand cut short
    let mut jpeg_cut = vec![0xFF, 0xD8, 0x00];
    jpeg_cut.extend_from_slice(&[0u8; 8]);
    // Two bytes with no NUL are text to `classify`; the point is only that
    // they are not a picture.
    assert_ne!(identify(&[0xFF, 0xD8], None).reader, Reader::Image);
    let cases: [&[u8]; 5] = [&riff_wave, &isom, &short_ftyp, &heic_cut, &jpeg_cut];
    for bytes in cases {
        assert_eq!(identify(bytes, None).reader, Reader::NotText, "{bytes:?}");
    }
}

/// Magic made of printable bytes is text until the byte that is not: `II*`
/// is TIFF only with the NUL after it, and `ftyp` is HEIF only inside a box
/// whose size leads with zeros. Both lines are prose and stay prose.
#[test]
fn text_that_begins_like_a_magic_is_still_text() {
    for line in [
        "II* — нотатка, що випадково почалась так.\n".as_bytes(),
        "abcdftypheic і далі звичайний текст нотатки.\n".as_bytes(),
    ] {
        assert_eq!(
            identify(line, Some("txt")).reader,
            Reader::PlainText,
            "{line:?}"
        );
    }
}

/// `mif1` is the brand HEIF writers use as the major one when the file only
/// declares itself generic; a real camera HEIC may carry it.
#[test]
fn a_heif_with_the_generic_brand_is_a_picture() {
    let mut mif1 = vec![0, 0, 0, 0x18];
    mif1.extend_from_slice(b"ftypmif1");
    mif1.extend_from_slice(&[0u8; 12]);
    let t = identify(&mif1, None);
    assert_eq!((t.reader, t.mime), (Reader::Image, "image/heic"));
}

/// The other direction: an image detector that says yes to every binary
/// fails here.
#[test]
fn binary_that_is_not_a_picture_stays_not_text() {
    let t = identify(include_bytes!("fixtures/binary.bin"), Some("png"));
    assert_eq!(t.reader, Reader::NotText);
    assert_eq!(t.mime, "application/octet-stream");
}

/// D51. A file that opens as text and stops being one gets its own reader, and
/// `identify` is where the two refusals become visible to everything
/// downstream: the worker's rule, the journal's rule, and the decision about
/// whether the index keeps what it already holds.
///
/// Both directions, because one alone is satisfied by a mistake. A verdict
/// that answered `BinaryTail` for everything binary would delete nothing ever,
/// which loses the photo case that `a_correctly_named_photo_is_not_plain_text`
/// pins; a verdict that never answered it deletes the interrupted note. So the
/// photo and the damaged note are asserted here against each other.
#[test]
fn a_file_that_starts_as_text_and_stops_is_not_the_same_as_a_photo() {
    let mut damaged = "нотатка, дописування якої обірвалось\n"
        .repeat(200)
        .into_bytes();
    damaged.extend_from_slice(&[0u8; 4096]);

    let t = identify(&damaged, Some("txt"));
    assert_eq!(t.reader, Reader::BinaryTail);
    assert_eq!(t.mime, "application/octet-stream");
    assert_eq!(t.source_kind, SourceKind::Document);

    // The extension does not decide it here either, in either direction.
    assert_eq!(identify(&damaged, None).reader, Reader::BinaryTail);
    assert_eq!(identify(&damaged, Some("png")).reader, Reader::BinaryTail);

    // And a file binary from the start does not drift onto this side: it stays
    // the verdict that is allowed to displace. Not solid.png, which the image
    // reader now claims by its magic before this verdict is asked.
    assert_eq!(
        identify(include_bytes!("fixtures/binary.bin"), Some("txt")).reader,
        Reader::NotText
    );
    // Nor does the note reach a reader: it is refused, just not deleted.
    assert_ne!(identify(&damaged, Some("txt")).reader, Reader::PlainText);
}

/// The check sits *after* the magic branches, and this is why: both a PDF
/// and every zip-based format carry NUL bytes, so a check placed first
/// would refuse every document the product can actually read.
#[test]
fn the_formats_with_readers_still_reach_their_readers() {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    pdf.extend_from_slice(&[0x00, 0x01, 0x02]);
    assert_eq!(identify(&pdf, Some("pdf")).reader, Reader::Pdf);

    // A zip signature with NUL bytes behind it: still routed by the zip
    // branch, not refused as binary. It has no `word/document.xml`, so the
    // zip branch itself answers `Unrecognized` — the point here is that it
    // never reaches `NotText`, not what it lands on instead.
    let mut zip = b"PK\x03\x04".to_vec();
    zip.extend_from_slice(&[0x00; 32]);
    assert_eq!(identify(&zip, Some("docx")).reader, Reader::Unrecognized);
}

/// Text keeps its reader, and keeps the extension deciding *which* one.
#[test]
fn text_still_routes_by_extension() {
    assert_eq!(
        identify(b"# heading\n", Some("md")).reader,
        Reader::Markdown
    );
    assert_eq!(
        identify(b"a,b\n1,2\n", Some("csv")).source_kind,
        SourceKind::Data
    );
    assert_eq!(
        identify(b"fn main() {}\n", Some("rs")).source_kind,
        SourceKind::Code
    );
    assert_eq!(identify(b"plain\n", Some("json")).reader, Reader::PlainText);
    assert_eq!(identify(b"plain\n", None).reader, Reader::PlainText);
}

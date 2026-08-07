//! Task 6's two tests, verbatim from the brief, plus three the brief's
//! interface implies but does not spell out: `ZipPartError` names three
//! variants and only one of them (`TooLarge`) had a test until these were
//! added, and the `> cap` boundary the module doc promises had nothing
//! checking it at the exact cap rather than far under it.

use std::io::{Cursor, Write};

use mnema_extract::zip_part::{ZipPartError, read_member};

/// Builds a minimal zip archive containing exactly one Deflated member,
/// `name`, holding `contents`.
fn zip_with_member(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let opts: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        w.start_file(name, opts).unwrap();
        w.write_all(contents).unwrap();
        w.finish().unwrap();
    }
    buf.into_inner()
}

/// A valid zip archive containing one Deflated member whose *declared*
/// uncompressed size — in both the local file header and the central
/// directory — has been overwritten with a value far smaller than what the
/// member actually decompresses to. The compressed bytes themselves, and the
/// compressed-size field that delimits them, are left untouched: what is
/// forged is exactly the number a naive caller would trust
/// (`ZipInfo.file_size`'s equivalent here), and nothing else. This is the
/// same shape the server measured: a bomb whose declared size is forged
/// small still fully inflates under a plain read (`app/textdoc/office.py:45`).
fn zip_with_forged_size() -> Vec<u8> {
    // Comfortably larger than the 1024-byte cap the test below reads it
    // against, and repetitive enough that Deflate turns it into a compressed
    // blob far too short to coincidentally contain a zip signature.
    let real_contents = vec![b'A'; 4096];
    let mut bytes = zip_with_member("word/document.xml", &real_contents);

    // The local file header is always the archive's first four bytes.
    assert_eq!(
        &bytes[0..4],
        b"PK\x03\x04",
        "no local file header at offset 0"
    );
    let forged = 10u32.to_le_bytes();
    // Uncompressed size sits at offset 22 in the local file header (PKWARE
    // APPNOTE 4.3.7) and at offset 24 in the central directory header
    // (APPNOTE 4.3.12). Compressed size — offsets 18 and 20 — is left alone,
    // so the reader still finds the true end of the compressed data.
    bytes[22..26].copy_from_slice(&forged);
    let central_pos = bytes
        .windows(4)
        .rposition(|w| w == b"PK\x01\x02")
        .expect("central directory header");
    bytes[central_pos + 24..central_pos + 28].copy_from_slice(&forged);

    bytes
}

#[test]
fn a_declared_size_does_not_decide_anything() {
    // A member whose central-directory size is small and whose real content is
    // not. `zip`'s reader must not be trusted to pre-check this: the cap has to
    // stand on the stream (app/textdoc/office.py:45 measured the same thing).
    let archive = zip_with_forged_size();
    assert!(matches!(
        read_member(&archive, "word/document.xml", 1024),
        Err(ZipPartError::TooLarge)
    ));
}

#[test]
fn a_member_under_the_cap_comes_back_whole() {
    let archive = zip_with_member("word/document.xml", b"<w:document/>");
    assert_eq!(
        read_member(&archive, "word/document.xml", 1024).unwrap(),
        b"<w:document/>"
    );
}

/// The boundary the module doc promises: refusal is on what came out being
/// *more* than the cap, not merely as large. A member of exactly `cap` bytes
/// is one `cap as u64 + 1` read away from looking oversized, which is exactly
/// the value `a_member_under_the_cap_comes_back_whole` (13 bytes, cap 1024)
/// is nowhere near — this is the case that would catch `>` slipping to `>=`.
#[test]
fn a_member_exactly_at_the_cap_comes_back_whole() {
    let contents = vec![b'x'; 1024];
    let archive = zip_with_member("word/document.xml", &contents);
    assert_eq!(
        read_member(&archive, "word/document.xml", 1024).unwrap(),
        contents
    );
}

#[test]
fn a_missing_member_is_reported_as_missing_not_malformed() {
    // A well-formed archive that simply does not hold the part asked for —
    // distinct from a broken archive, and a caller (a later docx/xlsx/epub
    // reader, or the journal's skip rules once Task 7 lands) needs to be able
    // to tell the two apart.
    let archive = zip_with_member("readme.txt", b"not the part we asked for");
    assert!(matches!(
        read_member(&archive, "word/document.xml", 1024),
        Err(ZipPartError::Missing)
    ));
}

#[test]
fn bytes_that_are_not_a_zip_at_all_are_malformed() {
    let not_a_zip = b"this is not a zip archive".to_vec();
    assert!(matches!(
        read_member(&not_a_zip, "word/document.xml", 1024),
        Err(ZipPartError::Malformed)
    ));
}

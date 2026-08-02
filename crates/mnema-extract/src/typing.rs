//! Deciding what a file is.
//!
//! D41 says type is decided by content, not by extension, because
//! `document.id` is the sha256 of the file's bytes and an extension belongs
//! to the *path*: the same bytes at `notes.txt` and `notes.py` are one
//! document row, and letting the extension decide would let whichever path a
//! walk saw first fix `mime`, `source_kind` and every reader downstream.
//!
//! That rule is narrower than D41 states. `txt`, `md`, `csv` and source code
//! are all plain text — no magic bytes distinguish them — so for those the
//! extension is the only signal there is. The order below is therefore magic
//! first, where magic exists (`%PDF-`; the zip signature plus its required
//! member), and the extension only for plain text.
//!
//! What this module does **not** do is resolve the hole that narrowing
//! leaves: the same bytes at `notes.txt` and `notes.md` are one document by
//! content addressing but two different readings, and the winner is
//! whichever path a watched-folder walk reaches first. That is recorded here
//! (`the_same_bytes_under_two_text_extensions_are_one_document_with_two_readings`
//! in `tests/typing.rs`) rather than resolved — resolving it needs walk
//! order, which belongs to the watched-folder spec.

use std::io::{Cursor, Read};

use mnema_core::SourceKind;

/// Which reader eventually reads `FileType::mime`'s bytes.
///
/// Named ahead of the readers themselves, the way `mnema_extract::Error`
/// names the pdfium path before PDF extraction exists: deciding what a file
/// *is* is this task's job, how each of these actually reads is a later
/// task's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reader {
    /// txt, csv, source code — anything `text::extract_text` reads. One
    /// variant for all of them: they differ in `mime` and `source_kind`, not
    /// in how the bytes are decoded.
    PlainText,
    /// `md`, read by `markdown::extract_markdown`: the same bytes as plain
    /// text, but a structure — sections, fences, tables — that plain text
    /// cannot see. It left this list's `PlainText` arm in task 11.
    Markdown,
    Pdf,
    Docx,
    Xlsx,
    Epub,
    /// A zip signature whose required member is missing: a bare zip, or a
    /// zip-based format this crate does not yet know. Not an error — this
    /// function never fails — but there is no reader for it either.
    Unrecognized,
    /// Not text at all: a photo, a video, a database, an executable. Decided
    /// by `looks_like_text` over the file's own bytes (D51).
    ///
    /// Separate from `Unrecognized`, which means "a zip whose required member
    /// is missing" — a format this crate may yet learn. This one never will:
    /// there is no future release in which a JPEG is read as prose.
    NotText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileType {
    pub mime: &'static str,
    pub source_kind: SourceKind,
    pub reader: Reader,
}

const PDF_MAGIC: &[u8] = b"%PDF-";

/// Local file header signature, present at the start of every non-empty zip.
const ZIP_LOCAL_FILE_MAGIC: &[u8] = b"PK\x03\x04";
/// End-of-central-directory signature, present at the start of an *empty*
/// zip (one with no entries at all) — such an archive has no local file
/// header to match the signature above.
const ZIP_EMPTY_MAGIC: &[u8] = b"PK\x05\x06";

/// Whether these bytes are text at all — decided by content, never by name
/// (D51).
///
/// Two steps, and the order is the whole design.
///
/// A **UTF-16 byte-order mark** says the NUL bytes that follow are part of
/// correctly encoded text, so it exits early. Only UTF-16's two marks do
/// this: in UTF-8 a NUL is never legitimate, so a file carrying `EF BB BF`
/// and a NUL is corrupt, and refusing it is right. Widening this to "any
/// mark" would let three prepended bytes carry anything past the check.
///
/// Otherwise a **NUL anywhere in the slice** refuses the file. This is the
/// signal the criterion D51 originally wrote down does not have: chardetng
/// guesses windows-1252 for binary input, that encoding maps all 256 bytes,
/// and so a decode of a JPEG produces no replacement characters at all —
/// measured as 0.00% on JPEG, PNG, HEIC, SQLite, Mach-O and random bytes
/// alike, with `had_errors` false in every one.
///
/// The scan covers the whole slice rather than a prefix, and that is
/// affordable rather than generous: a whole-file scan costs less than the
/// SHA-256 `worker.rs` has already computed over the same `Vec<u8>` (0.69 µs
/// against 1.03 on 2.4 KB of text; on a 6.6 MB photo it is 0.00 against
/// 2789, because the first NUL sits in the first bytes).
pub(crate) fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return true;
    }
    !bytes.contains(&0)
}

/// Decides what `bytes` are: magic first, where magic exists; the extension
/// only when the bytes are plain text, or when no magic recognises them.
pub fn identify(bytes: &[u8], extension: Option<&str>) -> FileType {
    if bytes.starts_with(PDF_MAGIC) {
        return FileType {
            mime: "application/pdf",
            source_kind: SourceKind::Document,
            reader: Reader::Pdf,
        };
    }

    if bytes.starts_with(ZIP_LOCAL_FILE_MAGIC) || bytes.starts_with(ZIP_EMPTY_MAGIC) {
        return identify_zip(bytes);
    }

    // After the magic branches, never before: a PDF and every zip-based
    // format carry NUL bytes, so this check placed first would refuse exactly
    // the documents the product exists to read.
    if !looks_like_text(bytes) {
        return FileType {
            mime: "application/octet-stream",
            source_kind: SourceKind::Document,
            reader: Reader::NotText,
        };
    }

    identify_plain_text(extension)
}

/// A zip signature is not sufficient on its own: docx, xlsx and epub are all
/// zip archives, so what discriminates them is the member required inside —
/// `word/document.xml`, `xl/workbook.xml`, or (epub, our own work, no server
/// precedent) an uncompressed first entry named `mimetype`.
fn identify_zip(bytes: &[u8]) -> FileType {
    let unrecognized = FileType {
        mime: "application/zip",
        source_kind: SourceKind::Document,
        reader: Reader::Unrecognized,
    };

    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        // The signature matched but the bytes are not a well-formed zip —
        // a corrupt or truncated archive. There is still no reader for it.
        return unrecognized;
    };

    if archive.by_name("word/document.xml").is_ok() {
        return FileType {
            mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            source_kind: SourceKind::Document,
            reader: Reader::Docx,
        };
    }

    if archive.by_name("xl/workbook.xml").is_ok() {
        return FileType {
            mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            source_kind: SourceKind::Document,
            reader: Reader::Xlsx,
        };
    }

    if is_epub(&mut archive) {
        return FileType {
            mime: "application/epub+zip",
            source_kind: SourceKind::Document,
            reader: Reader::Epub,
        };
    }

    unrecognized
}

/// Epub's signature is its first entry: named `mimetype`, stored
/// uncompressed (so a generic zip tool can read it without inflating
/// anything), and holding exactly `application/epub+zip`. Checking the
/// content, not just the name and the storage method, is what tells a
/// genuine epub from a docx-style check ported past the one format where the
/// required part is a whole-file match rather than an XML member.
fn is_epub<R: Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>) -> bool {
    let Ok(mut first) = archive.by_index(0) else {
        return false;
    };
    if first.name() != "mimetype" || first.compression() != zip::CompressionMethod::Stored {
        return false;
    }
    let mut content = String::new();
    if first.read_to_string(&mut content).is_err() {
        return false;
    }
    content.trim() == "application/epub+zip"
}

/// No magic distinguishes plain text from plain text: the extension is the
/// only signal there is, and it decides `mime`, `source_kind` **and** which
/// reader takes the bytes. An extension this function does not recognise gets
/// the same answer as no extension at all — "no better guess" is a fallback,
/// not a third classification of its own.
///
/// Markdown is the one of these that has a reader of its own. It was read as
/// plain text until task 11, which indexed it — badly: no sections, and a
/// fenced block indistinguishable from the prose around it.
fn identify_plain_text(extension: Option<&str>) -> FileType {
    let (mime, source_kind, reader) = match extension {
        Some("md") => ("text/markdown", SourceKind::Document, Reader::Markdown),
        Some("csv") => ("text/csv", SourceKind::Data, Reader::PlainText),
        Some(ext) if is_source_extension(ext) => {
            ("text/plain", SourceKind::Code, Reader::PlainText)
        }
        _ => ("text/plain", SourceKind::Document, Reader::PlainText),
    };
    FileType {
        mime,
        source_kind,
        reader,
    }
}

/// A deliberately small, named list rather than "everything that is not
/// txt/md/csv": source code is what D28 means by indexing a code repository,
/// and guessing at every extension a build tool or a data format might use
/// would classify files this list was never asked to cover. Extend it as
/// real corpora need it.
fn is_source_extension(ext: &str) -> bool {
    matches!(
        ext,
        "rs" | "py"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "go"
            | "java"
            | "kt"
            | "c"
            | "h"
            | "cc"
            | "cpp"
            | "hpp"
            | "hxx"
            | "cs"
            | "rb"
            | "php"
            | "sh"
            | "bash"
            | "zsh"
            | "swift"
            | "scala"
            | "pl"
            | "lua"
            | "r"
            | "sql"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D51. The whole point of this function, in both directions: a real
    /// binary is refused, and every legitimate text encoding is not.
    ///
    /// Both directions in one test on purpose. An assertion that only
    /// constrains the refusal side is satisfied by a function that refuses
    /// everything, and that shape went unnoticed nine times in the previous
    /// branch.
    #[test]
    fn content_decides_what_is_text() {
        // Refused: a real PNG, invented outright by `tests/fixtures/make_fixtures.py`.
        assert!(!looks_like_text(include_bytes!(
            "../tests/fixtures/solid.png"
        )));
        // Refused: the openings of formats a folder of photos is full of.
        let mut jpeg = vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00,
        ];
        jpeg.extend_from_slice(&[0x01, 0x02, 0x03]);
        assert!(!looks_like_text(&jpeg));
        assert!(!looks_like_text(b"SQLite format 3\x00"));
        // Refused: a NUL anywhere is the signal, wherever it sits.
        assert!(!looks_like_text(b"\x00"));
        assert!(!looks_like_text("текст, а далі нуль\0".as_bytes()));

        // Accepted: text in every encoding the product meets.
        assert!(looks_like_text("звичайний текст\n".as_bytes()));
        assert!(looks_like_text(b"plain ascii\n"));
        assert!(looks_like_text(&[0xEF, 0xBB, 0xBF, b'o', b'k'])); // UTF-8 BOM
        assert!(looks_like_text(b"\x1b[31mERROR\x1b[0m a coloured log\n"));
        // Accepted: an empty file has no evidence either way and is not binary.
        assert!(looks_like_text(b""));
        assert!(looks_like_text(b"ok\n"));
    }

    /// UTF-16 is the one text encoding whose NUL bytes are legitimate, and the
    /// byte-order mark is what says so. Without this branch the check would
    /// refuse a file the product reads correctly today — and, through
    /// `displaces`, delete it from the index.
    ///
    /// Measured before this was written: `encoding_rs::decode` does its own
    /// BOM sniffing and overrides chardetng's guess, so a UTF-16LE file with a
    /// mark decodes to 0.00% control characters while the same text without
    /// one comes back as windows-1250 at 51.52%.
    #[test]
    fn a_utf16_byte_order_mark_is_text_despite_its_nul_bytes() {
        let mut le = vec![0xFF, 0xFE];
        le.extend_from_slice(&[0x54, 0x00, 0x65, 0x00]); // "Te" in UTF-16LE
        assert!(looks_like_text(&le));

        let mut be = vec![0xFE, 0xFF];
        be.extend_from_slice(&[0x00, 0x54, 0x00, 0x65]); // "Te" in UTF-16BE
        assert!(looks_like_text(&be));

        // The same text without a mark is refused, and that cost is accepted
        // deliberately (spec §4): it is mojibake today, and a refusal with a
        // journal row is the better of the two.
        assert!(!looks_like_text(&[0x54, 0x00, 0x65, 0x00]));
    }

    /// The scan covers the whole slice, not a prefix. A file that is text for
    /// a long stretch and binary afterwards — a SQL dump carrying binary
    /// columns, for instance — is the case a prefix check would pass.
    ///
    /// Measured: scanning a whole file for a NUL costs less than the SHA-256
    /// the same slice already pays for (0.69 µs against 1.03 on 2.4 KB), so
    /// there is nothing bought by stopping early.
    #[test]
    fn the_scan_does_not_stop_at_a_prefix() {
        let mut bytes = "текст, який довго лишається текстом\n"
            .repeat(1000)
            .into_bytes();
        assert!(
            bytes.len() > 16_384,
            "the tail must sit past any plausible prefix"
        );
        assert!(looks_like_text(&bytes));
        bytes.push(0);
        assert!(!looks_like_text(&bytes));
    }
}

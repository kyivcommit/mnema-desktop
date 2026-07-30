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

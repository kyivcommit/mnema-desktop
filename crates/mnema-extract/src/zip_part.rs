//! Reads one member out of a zip-shaped archive against a cap on decompressed
//! bytes — the piece `docx.rs`, `xlsx.rs` and `epub.rs` share, since all three
//! open a member (`word/document.xml`, a sheet, a chapter) out of a file that
//! is itself a zip container.
//!
//! The cap stands on what the stream actually produces, never on the member's
//! declared uncompressed size. That size lives in the archive's central
//! directory, which is written by whoever crafted the file, and the `zip`
//! crate does not enforce it while decompressing: a Deflate member's declared
//! size is informational, and decoding runs until the compressed stream's own
//! end-of-block marker, however much output that turns out to be. A zip bomb
//! can declare its size small and still fully inflate under a naive
//! `read_to_end`. The server measured exactly this and capped the read on the
//! stream for the same reason (`app/textdoc/office.py:41-52`, the comment at
//! `:45` names the forged-size case directly) — but only for `docx`/`xlsx`;
//! its `epub` reader (`app/textdoc/adapters.py:118`) reads a chapter
//! uncapped. That split does not carry over here: this module gives all three
//! formats the same cap.

use std::io::{Cursor, Read};

/// What one member of an archive may inflate to before it is refused.
///
/// Declared here rather than per reader so that the three formats that open an
/// archive answer alike: a chapter, `word/document.xml` and a sheet are all one
/// member of one container, and three numbers would be three different answers
/// to the same question.
///
/// The value is a ceiling on the absurd, not a budget for the ordinary. A
/// chapter of a novel is tens of kilobytes and the largest `word/document.xml`
/// in a real corpus is a few megabytes; 16 MiB of *decompressed* XML is already
/// far past anything a person authored. What it exists for is the other end:
/// Deflate's maximum ratio is 1032:1, so a member of a few kilobytes can ask
/// this process for gigabytes, and `read_member` decides on what came out of
/// the stream rather than on what the archive declares.
///
/// It bounds one member and deliberately not a whole file — a format that opens
/// many members owes its own total (`epub::BOOK_MAX_BYTES`), because N members
/// each just under this cap is the same attack with more entries.
pub const MEMBER_MAX_BYTES: usize = 16 << 20;

/// Why [`read_member`] could not return a member's bytes.
#[derive(Debug, thiserror::Error)]
pub enum ZipPartError {
    /// The archive parses and this member is not in it.
    ///
    /// For docx and xlsx that should not happen: `typing::identify` already
    /// required `word/document.xml` / `xl/workbook.xml` to exist before
    /// naming either reader (`typing.rs:279`, `:287`), so reaching this for
    /// one of them means the archive changed under us between
    /// identification and reading. Not so for epub — `typing::identify`'s
    /// `is_epub` (`typing.rs:312-324`) checks only the `mimetype` entry, not
    /// any member the spine will later name, so an epub whose spine points
    /// at a member the archive doesn't actually have reaches this on an
    /// otherwise ordinary file.
    #[error("zip member not found")]
    Missing,

    /// The archive itself does not parse: a corrupt or truncated zip.
    #[error("malformed zip archive")]
    Malformed,

    /// The member decompressed past the cap. Decided on the stream, not on
    /// the member's declared size — see the module doc.
    #[error("zip member exceeds the size cap")]
    TooLarge,
}

/// Reads the member named `name` out of the zip archive `bytes`, refusing it
/// once more than `cap` bytes have come out of the stream.
///
/// Reads `cap + 1` bytes and rejects on what was actually read, never on
/// `member.size()`: that number is the central directory's declared
/// uncompressed size, which the archive's author controls and the `zip`
/// crate does not check against the real decompressed output. See the
/// module doc for why that number cannot be trusted.
pub fn read_member(bytes: &[u8], name: &str, cap: usize) -> Result<Vec<u8>, ZipPartError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| ZipPartError::Malformed)?;
    let member = archive.by_name(name).map_err(|e| match e {
        zip::result::ZipError::FileNotFound => ZipPartError::Missing,
        _ => ZipPartError::Malformed,
    })?;

    let mut out = Vec::new();
    member
        // `saturating_add`: no caller reaches `cap == usize::MAX` today, but
        // it costs nothing and an overflowing `+ 1` would wrap to 0 and cap
        // every read at zero bytes instead.
        .take((cap as u64).saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|_| ZipPartError::Malformed)?;
    if out.len() > cap {
        return Err(ZipPartError::TooLarge);
    }
    Ok(out)
}

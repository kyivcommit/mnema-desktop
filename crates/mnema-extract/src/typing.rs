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
    /// `html`/`htm`, read by `html::extract_html`. Plain text as far as
    /// `classify` is concerned — no magic distinguishes it — and the one format
    /// in this product that was **wrong** rather than unread before its reader
    /// arrived: it fell to `PlainText` below and was indexed with its CSS and
    /// its JavaScript in the prose (spec §2.1).
    Html,
    Pdf,
    Docx,
    Xlsx,
    Epub,
    /// A zip signature whose required member is missing: a bare zip, or a
    /// zip-based format this crate does not yet know. Not an error — this
    /// function never fails — but there is no reader for it either.
    Unrecognized,
    /// Not text at all: a photo, a video, a database, an executable. Decided
    /// by `classify` over the file's own bytes (D51).
    ///
    /// Separate from `Unrecognized`, which means "a zip whose required member
    /// is missing" — a format this crate may yet learn. This one never will:
    /// there is no future release in which a JPEG is read as prose.
    NotText,
    /// Text for its first `HEAD_BYTES` and binary after that: a file that
    /// *began* as text and stopped being one. An interrupted append is the
    /// case this exists for.
    ///
    /// Separate from `NotText` although both are refused and both carry
    /// `application/octet-stream`, because the two say different things about
    /// what the index should still hold under the path. `NotText` says the
    /// file is a photo and whatever text the index has under that name is a
    /// previous file; this says the prose the index has is probably still the
    /// first 84% of the file on disk, so deleting it would lose text readable
    /// nowhere else.
    BinaryTail,
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

/// How far into a file a NUL still means "this whole thing is binary".
///
/// 512 rather than a round guess: measured, every binary sample carries its
/// first NUL at offset 0, 4, 5, 8, 15 or 254, so this clears the furthest of
/// them twice over. A NUL past this point means the file was text up to here.
///
/// **What this window cannot see at all, stated with its numbers: a short,
/// high-entropy file that happens to contain no NUL byte is indexed as text.**
/// The criterion is "no NUL anywhere", so a file with none passes whatever its
/// length — measured on a 400-byte zlib blob, the real worker answers
/// `mime=text/plain` and hands back one block of 411 characters of mangled
/// Latin. `HEAD_BYTES` is not what lets it through and raising it would not
/// close it; the whole-file scan simply finds nothing to object to.
///
/// Scale, from the same corpus the figures above come from: of 117,786 real
/// files of 4 KiB or less, **166 (0.14%)** carry no NUL and are not plain text.
///
/// It is the same physics as the accepted residual on the other side of
/// `HEAD_BYTES` — a NUL-free run says less about a short file than about a long
/// one — but the consequence is the opposite one and worse. That residual keeps
/// an old document standing; this one puts bytes that are not prose *into* the
/// index, and under D29 everything indexed goes to a third-party embedding
/// provider. Recorded here as a named limit of the criterion, deliberately
/// without changing it: closing it means an entropy or a decoder test, which is
/// a decision of its own and not a constant.
pub(crate) const HEAD_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Text,
    /// Binary from the start — a photo, a database, an executable.
    NotText,
    /// Text for at least the first `HEAD_BYTES`, then a NUL. What an
    /// interrupted append leaves behind. Refused all the same, but it must not
    /// displace what the path already held: the prose it opens with is probably
    /// still on disk, and deleting the earlier document would lose text no
    /// longer readable anywhere.
    ///
    /// **Not** what a UTF-16 file without a byte-order mark looks like, which
    /// this said until it was run: such a file's first NUL is in its opening
    /// bytes, not past its first page, so it earns `NotText` — measured, at
    /// byte 15 for Ukrainian, byte 1 for English, and never at all for
    /// unbroken Cyrillic, which comes back `Text`. The claim mattered because
    /// `mnema_index::journal`'s `SkipRule::NotText` names markless UTF-16 as
    /// the first candidate for loosening `classify`, and a reader who believed
    /// this line would think those files were already safe from displacement.
    /// They are the ones it deletes.
    BinaryTail,
}

/// What these bytes are — decided by content, never by name (D51).
///
/// Two steps, and the order is the whole design.
///
/// A **UTF-16 byte-order mark** says the NUL bytes that follow are half of a
/// code unit rather than corruption. It changes *how* the question is asked,
/// not whether it is asked: behind a mark the same criterion runs over code
/// units instead of bytes. Only UTF-16's two marks do this: in UTF-8 a NUL is
/// never legitimate, so a file carrying `EF BB BF` and a NUL is corrupt, and
/// refusing it is right.
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
///
/// **Where the NUL sits is a second question, and it used to go unasked.**
/// This returned a bool, and every refusal displaced whatever the path already
/// held — which deletes a note whose append was interrupted, because the power
/// went out and the tail came back zeroed while the prose stayed on disk.
/// `HEAD_BYTES` splits the two: binary from the start is [`Verdict::NotText`],
/// text that stops being text is [`Verdict::BinaryTail`]. Both are refused;
/// only the first is allowed to delete.
///
/// A known consequence, recorded rather than worked around: a list produced by
/// `find -print0` carries its first NUL at byte 23, so it lands in the head and
/// is refused with displacement. It is genuine text, and it is rare in a folder
/// of documents.
pub(crate) fn classify(bytes: &[u8]) -> Verdict {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        // A mark says the NUL *bytes* that follow are half of a code unit —
        // it does not say the file is text. What decides that is the same
        // question one level up: a `U+0000` code unit does not occur in
        // correct UTF-16, exactly as a NUL byte does not occur in correct
        // UTF-8. Reading pairs answers it without a decoder, and it also
        // refuses a UTF-32LE mark, whose first code unit after `FF FE` is
        // `0000` — which matters because `encoding_rs` has no UTF-32 decoder
        // and would otherwise read the file as something it is not.
        //
        // `position`, not `any`: behind a mark the offset that matters is
        // measured in code units, so it is converted back to bytes before it
        // meets `HEAD_BYTES`, which counts bytes. The conversion is `units * 2`
        // and it counts from *after* the mark, so this window is two file bytes
        // wider than the byte branch's: measured, a zero code unit at file
        // offset 512 is `NotText` and one at 514 is `BinaryTail`. Whether the
        // two windows should end on the same byte is a separate question from
        // whether this comment describes them, and only the second is settled
        // here.
        //
        // **The two windows also differ in how hard they are to trip, by about
        // 256×, and that is a different fact from the two extra bytes.** The
        // byte branch refuses on any one zero byte; this one needs an aligned
        // *pair* of them. Measured on 4 KiB of uniformly random bytes: 0.00%
        // come back `Text` without a mark, against 96.55% behind `FF FE` — and
        // real binaries with `FF FE` prepended come back `Text` 33.67% of the
        // time. So a file that opens with those two bytes and is not UTF-16 at
        // all is very likely to be read as text and indexed.
        //
        // The exposure is small and the price is accepted, but it is accepted
        // knowingly rather than by not having looked: of 1,102 real binaries
        // sampled from `/usr/lib`, `/usr/bin`, `/opt/homebrew` and the system
        // fonts, **0** begin with `FF FE`. What makes this branch worth having
        // is the file it rescues — markless UTF-16 is refused *with*
        // displacement, so a marked one being refused would delete a document
        // over an encoding.

        return match bytes[2..].chunks_exact(2).position(|pair| pair == [0, 0]) {
            None => Verdict::Text,
            Some(units) if units * 2 < HEAD_BYTES => Verdict::NotText,
            Some(_) => Verdict::BinaryTail,
        };
    }
    match bytes.iter().position(|b| *b == 0) {
        None => Verdict::Text,
        Some(at) if at < HEAD_BYTES => Verdict::NotText,
        Some(_) => Verdict::BinaryTail,
    }
}

/// Decides what `bytes` are: magic first, where magic exists; the extension
/// only when the bytes are plain text.
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
    match classify(bytes) {
        Verdict::Text => identify_plain_text(extension),
        // The same mime for both refusals: neither is a format this product
        // reads, and `octet-stream` is what "bytes, not prose" is called. What
        // separates them is the reader, which is what the journal and
        // `displaces` end up asking about.
        Verdict::NotText => FileType {
            mime: "application/octet-stream",
            source_kind: SourceKind::Document,
            reader: Reader::NotText,
        },
        Verdict::BinaryTail => FileType {
            mime: "application/octet-stream",
            source_kind: SourceKind::Document,
            reader: Reader::BinaryTail,
        },
    }
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
///
/// HTML is the second, and its arrival is the reason the manifest exists.
/// Until it landed, `.html` fell to the `_ =>` arm below and was indexed as
/// `text/plain` — markup, CSS and JavaScript together (spec §2.1) — and every
/// such file is recorded as `text@1`, which no version bump can distinguish
/// from today's reading. `crates/mnema-extract/src/manifest.rs` gains the
/// matching entries in the same commit; the two have to move together or the
/// parent predicts a reader that never ran.
fn identify_plain_text(extension: Option<&str>) -> FileType {
    let (mime, source_kind, reader) = match extension {
        Some("md") => ("text/markdown", SourceKind::Document, Reader::Markdown),
        // Both spellings, because both are the same format and a manifest keyed
        // on extension has to carry each one it claims.
        Some("html") | Some("htm") => ("text/html", SourceKind::Document, Reader::Html),
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

    /// D51. A NUL in the first bytes says the whole file is binary; a NUL only
    /// far in says the file *began* as text and stopped being one. The two
    /// deserve different answers, because the second is what a note looks like
    /// after the power went out mid-append — prose intact on disk, tail zeroed.
    ///
    /// Measured: every binary sample carries its first NUL at offset 0, 4, 5,
    /// 8, 15 or 254; the interrupted note carries it at 84.7% of the file.
    #[test]
    fn a_nul_in_the_tail_is_not_the_same_verdict_as_a_nul_in_the_head() {
        let mut interrupted = "нотатка про засідання\n".repeat(200).into_bytes();
        assert!(
            interrupted.len() > HEAD_BYTES,
            "the prose must outrun the head window"
        );
        interrupted.extend_from_slice(&[0u8; 4096]);
        assert_eq!(classify(&interrupted), Verdict::BinaryTail);

        assert_eq!(
            classify(include_bytes!("../tests/fixtures/solid.png")),
            Verdict::NotText
        );
        assert_eq!(classify(b"\x00 at the very front"), Verdict::NotText);
        assert_eq!(classify("звичайний текст\n".as_bytes()), Verdict::Text);
    }

    /// Where exactly the head window ends, and that 512 is the number.
    ///
    /// Two assertions doing two different jobs, and neither covers the other's
    /// case. That is the reason they sit together rather than one being enough.
    ///
    /// The **boundary** pair is written through `HEAD_BYTES`, so it survives a
    /// deliberate change of the threshold and pins the comparison instead: the
    /// window is exclusive at `HEAD_BYTES`, so the last byte *inside* it is
    /// binary and the first byte *past* it is a tail. An off-by-one in
    /// `classify` — `<=` where `<` is written — flips the second of these and
    /// nothing else in the suite notices.
    ///
    /// The **value** assertion is what the boundary pair cannot do. Written
    /// through the constant, that pair follows the constant wherever it goes:
    /// measured, moving `HEAD_BYTES` from 512 to 4096 left the whole workspace
    /// green, 51 test binaries and no failure. So the number itself is pinned
    /// here, deliberately as a literal.
    ///
    /// 512 is measured, not chosen: every binary sample carries its first NUL
    /// at offset 0, 4, 5, 8, 15 or 254, and the furthest of those is cleared
    /// twice over. Moving it is a decision that needs a new measurement behind
    /// it — this assertion exists so that it cannot be made in passing.
    #[test]
    fn the_head_window_ends_where_the_constant_says_and_the_constant_is_512() {
        // One shape, two positions for the single NUL: every other byte is
        // ordinary text and both slices are the same length, so nothing but
        // the offset can move the verdict.
        let with_nul_at = |offset: usize| {
            let mut bytes = vec![b'a'; HEAD_BYTES + 64];
            bytes[offset] = 0;
            bytes
        };

        assert_eq!(
            classify(&with_nul_at(HEAD_BYTES - 1)),
            Verdict::NotText,
            "the last byte inside the head window is still binary-from-the-start"
        );
        assert_eq!(
            classify(&with_nul_at(HEAD_BYTES)),
            Verdict::BinaryTail,
            "the first byte past the head window is already a tail"
        );

        assert_eq!(
            HEAD_BYTES, 512,
            "512 is a measurement, not a round number: every binary sample \
             carries its first NUL at offset 0, 4, 5, 8, 15 or 254. Moving \
             this threshold needs a new measurement, not a new opinion — and \
             it moves silently otherwise, because every other assertion about \
             it is written through the constant"
        );
    }

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
        assert_eq!(
            classify(include_bytes!("../tests/fixtures/solid.png")),
            Verdict::NotText
        );
        // Refused: the openings of formats a folder of photos is full of.
        let mut jpeg = vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00,
        ];
        jpeg.extend_from_slice(&[0x01, 0x02, 0x03]);
        assert_eq!(classify(&jpeg), Verdict::NotText);
        assert_eq!(classify(b"SQLite format 3\x00"), Verdict::NotText);
        // Refused: a NUL anywhere is the signal, wherever it sits. Where it
        // sits decides which refusal — both of these are inside the head, so
        // both are `NotText`; the tail case is
        // `a_nul_in_the_tail_is_not_the_same_verdict_as_a_nul_in_the_head`.
        assert_eq!(classify(b"\x00"), Verdict::NotText);
        assert_eq!(
            classify("текст, а далі нуль\0".as_bytes()),
            Verdict::NotText
        );

        // Accepted: text in every encoding the product meets.
        assert_eq!(classify("звичайний текст\n".as_bytes()), Verdict::Text);
        assert_eq!(classify(b"plain ascii\n"), Verdict::Text);
        assert_eq!(classify(&[0xEF, 0xBB, 0xBF, b'o', b'k']), Verdict::Text); // UTF-8 BOM
        assert_eq!(
            classify(b"\x1b[31mERROR\x1b[0m a coloured log\n"),
            Verdict::Text
        );
        // Accepted: an empty file has no evidence either way and is not binary.
        assert_eq!(classify(b""), Verdict::Text);
        assert_eq!(classify(b"ok\n"), Verdict::Text);
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
        assert_eq!(classify(&le), Verdict::Text);

        let mut be = vec![0xFE, 0xFF];
        be.extend_from_slice(&[0x00, 0x54, 0x00, 0x65]); // "Te" in UTF-16BE
        assert_eq!(classify(&be), Verdict::Text);

        // The same text without a mark is refused, and that cost is accepted
        // deliberately (spec §4): it is mojibake today, and a refusal with a
        // journal row is the better of the two.
        assert_eq!(classify(&[0x54, 0x00, 0x65, 0x00]), Verdict::NotText);

        // Long enough that a fix which only inspects a few bytes cannot pass
        // by accident, and every code unit is real text.
        let mut long = vec![0xFF, 0xFE];
        for ch in "кропива росте попід тином і нікого не питає\n"
            .repeat(50)
            .encode_utf16()
        {
            long.extend_from_slice(&ch.to_le_bytes());
        }
        assert_eq!(
            classify(&long),
            Verdict::Text,
            "genuine UTF-16 text must still pass"
        );
    }

    /// D51. The mark branch used to be an unconditional `return true` on a
    /// two-byte prefix, so anything at all could ride behind it. Measured
    /// before this fix, through this same function: a real PNG with `FF FE`
    /// prepended came back `true`, as did a UTF-32LE mark and an MPEG-1
    /// Layer I frame sync — the last two are byte sequences that occur
    /// without anyone intending them.
    #[test]
    fn a_mark_does_not_carry_arbitrary_bytes_past_the_check() {
        let mut disguised = vec![0xFF, 0xFE];
        disguised.extend_from_slice(include_bytes!("../tests/fixtures/solid.png"));
        assert_eq!(
            classify(&disguised),
            Verdict::NotText,
            "a photo rode in behind a mark"
        );

        // UTF-32LE's mark starts with UTF-16LE's. `encoding_rs` has no UTF-32
        // decoder, so accepting this reads the file as something it is not.
        assert_eq!(
            classify(&[0xFF, 0xFE, 0x00, 0x00, 0x54, 0x00, 0x00, 0x00]),
            Verdict::NotText
        );

        // An MPEG-1 Layer I frame sync happens to open with the same two bytes.
        assert_eq!(
            classify(&[0xFF, 0xFE, 0x18, 0xC4, 0x00, 0x00, 0x00, 0x00]),
            Verdict::NotText
        );
    }

    /// D51. The tail case *behind a mark*, which nothing covered at all.
    ///
    /// Measured before this test existed: replacing this branch's
    /// `Some(_) => Verdict::BinaryTail` with `Verdict::NotText` left every one
    /// of this crate's eight targets green, `mnema-ingest --test slice` at 35
    /// passed, and mnema-pool green. The entire output of one arm of the
    /// verdict that decides whether a document is deleted was pinned by
    /// nothing.
    ///
    /// The cost is exactly what this cycle exists to prevent, reached through
    /// the one file shape the randomised harness cannot make:
    /// `interrupted_append_body` generates UTF-8 prose only. A UTF-16 note with
    /// a zeroed tail classified as `NotText` has changed bytes, so `displaces`
    /// answers true, and the prose still sitting on disk in front of the damage
    /// is deleted from the index.
    ///
    /// Both sides of the boundary, and the boundary is **two file bytes later
    /// than the byte branch's**: `units * 2` counts from after the mark, so a
    /// zero code unit at file offset `HEAD_BYTES` is still inside the window
    /// and one at `HEAD_BYTES + 2` is past it.
    #[test]
    fn an_interrupted_utf16_note_is_a_tail_and_not_a_photo() {
        // Mark, then real text up to `zero_at`, then the zeros an interrupted
        // append leaves behind. Every code unit before the offset is prose, so
        // nothing but the offset can move the verdict.
        let note = |zero_at: usize| {
            assert_eq!(zero_at % 2, 0, "a code unit starts on an even offset");
            let mut bytes = vec![0xFF, 0xFE];
            for unit in "нотатка про засідання ".repeat(200).encode_utf16() {
                if bytes.len() >= zero_at {
                    break;
                }
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            assert_eq!(bytes.len(), zero_at, "the prose must reach the offset");
            bytes.extend_from_slice(&[0u8; 4096]);
            bytes
        };

        assert_eq!(
            classify(&note(4096)),
            Verdict::BinaryTail,
            "a UTF-16 note whose append was interrupted is text that stopped \
             being text, and deleting its document loses prose that is still \
             on disk"
        );
        assert_eq!(
            classify(&note(HEAD_BYTES)),
            Verdict::NotText,
            "the last code unit inside the window is binary-from-the-start"
        );
        assert_eq!(
            classify(&note(HEAD_BYTES + 2)),
            Verdict::BinaryTail,
            "the first code unit past the window is already a tail — two file \
             bytes later than the byte branch's edge, because `units * 2` \
             counts from after the mark"
        );
    }

    /// The scan covers the whole slice, not a prefix. A file that is text for
    /// a long stretch and binary afterwards — a SQL dump carrying binary
    /// columns, for instance — is the case a prefix check would pass.
    ///
    /// `HEAD_BYTES` is not that prefix, and this is the test that says so. It
    /// decides *which* refusal a NUL earns, never whether the bytes past it are
    /// looked at: a check that stopped at 512 would answer `Text` here, and the
    /// verdict below is what distinguishes the two.
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
        assert_eq!(classify(&bytes), Verdict::Text);
        bytes.push(0);
        assert_eq!(
            classify(&bytes),
            Verdict::BinaryTail,
            "the NUL past the head window must be seen, and seen as a tail"
        );
    }
}

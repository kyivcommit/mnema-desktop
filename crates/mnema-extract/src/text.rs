//! The plain-text reader: bytes to `Block`s, verbatim after NFC.
//!
//! Encoding is guessed with `chardetng`, the configuration the skeleton
//! already measured, in a probe crate since deleted: UTF-8 must
//! be `Allow`, since most files this product indexes are unlabelled UTF-8 and
//! the browser-oriented `Deny` exists so web content cannot come to depend on
//! unlabelled detection — which is a different audience from ours. Do **not**
//! port the server's `("utf-8-sig","utf-8","cp1251","latin-1")` ladder: it is
//! measured to mojibake input, because `cp1251` accepts nearly any byte and
//! never complains.

use mnema_core::{Block, BlockType, nfc};

/// Decodes `bytes` as plain text and splits it into `Block`s.
///
/// No `Result`: plain-text extraction cannot fail. `chardetng`'s guess always
/// names an encoding `encoding_rs` can decode, and decoding never errors —
/// only replaces an invalid byte sequence with U+FFFD. An earlier draft kept
/// a `Result<Vec<Block>, Error>` with a `#[non_exhaustive]`, zero-variant
/// `Error` so every reader in this crate would share one signature — but a
/// `Result` that can never be `Err` is ceremony every caller pays for
/// nothing, and an uninhabited public type is a shape nobody can construct or
/// test. Readers that can genuinely fail (pdf: crash, timeout; a zip-based
/// format: a corrupt archive) will carry their own error, arriving with
/// task 7, which is also where file I/O — the other thing that can fail
/// here — is added.
///
/// A block is a run of non-empty lines; one or more empty lines separate two
/// blocks. `reading_order` starts at 0 and never resets within this function,
/// because a txt file is exactly one page (D37) — there is nothing to reset
/// it for. `line_start`/`line_end` are 1-based and inclusive, matching
/// `block.line_start`/`line_end`.
///
/// NFC runs immediately after decoding, before a single line is counted or a
/// byte of block text is taken (D32, D38): character counts change on
/// decomposed input, and offsets or hashes taken before normalisation would
/// describe a string nothing downstream ever sees again.
pub fn extract_text(bytes: &[u8]) -> Vec<Block> {
    let decoded = decode(bytes);
    let normalised = nfc::normalise(&decoded);

    let mut blocks = Vec::new();
    let mut reading_order = 0i64;
    let mut current: Option<(u32, u32, String)> = None;

    // `split_terminator('\n')`, not `lines()`, and this is the difference
    // between "verbatim" being true and being a claim. `lines()` strips the
    // `\r` of a CRLF ending, so a block spanning several lines was stored as a
    // string the file does not contain — and every offset taken into it lands
    // up to one character per line early against the file on disk, which is
    // the one thing this whole design exists to get right. The markdown reader
    // has always kept it (`markdown.rs`), so the same bytes read as `.txt` and
    // as `.md` stored different text.
    for (i, line) in normalised.split_terminator('\n').enumerate() {
        let line_no = (i + 1) as u32;
        // A `\r` alone is what an empty line looks like under CRLF: the line
        // is a separator, and its carriage return belongs to the terminator
        // rather than to any block. A line of *spaces* is still content, as it
        // was before — only genuinely empty lines separate blocks.
        if line.is_empty() || line == "\r" {
            if let Some(block) = take_block(&mut current, &mut reading_order) {
                blocks.push(block);
            }
            continue;
        }
        match &mut current {
            Some((_, end, text)) => {
                text.push('\n');
                text.push_str(line);
                *end = line_no;
            }
            None => current = Some((line_no, line_no, line.to_string())),
        }
    }
    if let Some(block) = take_block(&mut current, &mut reading_order) {
        blocks.push(block);
    }

    blocks
}

/// Closes the block in progress, if any, and advances `reading_order` past it.
fn take_block(current: &mut Option<(u32, u32, String)>, reading_order: &mut i64) -> Option<Block> {
    let (start, end, mut text) = current.take()?;
    // The last line's `\r` is not part of the block: it belongs to the
    // terminator that ends the block, the way the final `\n` does. Every
    // *interior* `\r` is kept, because it sits between two lines of the block
    // and the source really does contain it — which is what makes the stored
    // text a slice of the file.
    if text.ends_with('\r') {
        text.pop();
    }
    let block = Block {
        block_type: BlockType::Paragraph,
        reading_order: *reading_order,
        language: None,
        text,
        line_start: Some(start),
        line_end: Some(end),
    };
    *reading_order += 1;
    Some(block)
}

/// Guesses `bytes`' encoding and decodes it to `String`, lossily replacing any
/// byte sequence invalid under the guessed encoding rather than failing.
///
/// Shared with `markdown.rs`, which has the same bytes-to-string problem and
/// must not answer it differently: a `.md` file in an unlabelled encoding is
/// the same file as a `.txt` in one, and two detectors would eventually
/// disagree about which.
pub(crate) fn decode(bytes: &[u8]) -> String {
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let encoding = detector.guess(None, chardetng::Utf8Detection::Allow);
    let (decoded, _, _had_errors) = encoding.decode(bytes);
    decoded.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ascii_is_one_block() {
        let blocks = extract_text(b"hello\nworld\n");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "hello\nworld");
        assert_eq!(blocks[0].block_type, BlockType::Paragraph);
        assert_eq!(blocks[0].reading_order, 0);
    }

    #[test]
    fn an_empty_file_yields_no_blocks() {
        assert_eq!(extract_text(b""), Vec::new());
    }

    /// Every block's text must be a substring of the file it came from —
    /// verbatim after NFC, which is what D38 means and what this module's own
    /// doc claims.
    ///
    /// `lines()` broke it for CRLF, silently and only there: it strips the
    /// `\r`, so a block spanning several lines was rebuilt as a string the
    /// file does not contain. Nothing downstream noticed, because every offset
    /// is taken *into the stored text* and stays self-consistent — the break
    /// only shows when someone takes those offsets to the file on disk, which
    /// is the step this whole design exists for. A highlight in a k-line block
    /// lands up to k−1 characters early.
    ///
    /// Both endings, so a fix that broke LF to fix CRLF is caught too.
    #[test]
    fn a_blocks_text_is_a_slice_of_the_file_under_either_line_ending() {
        for (name, source) in [
            ("LF", "Перший рядок\nдругий рядок\n\nтретій\n"),
            ("CRLF", "Перший рядок\r\nдругий рядок\r\n\r\nтретій\r\n"),
        ] {
            let blocks = extract_text(source.as_bytes());
            assert_eq!(
                blocks.len(),
                2,
                "{name}: the blank line separates two blocks"
            );
            for block in &blocks {
                assert!(
                    source.contains(&block.text),
                    "{name}: {:?} is not a slice of the file",
                    block.text
                );
            }
            assert_eq!(blocks[0].line_start, Some(1));
            assert_eq!(blocks[0].line_end, Some(2));
            assert_eq!(blocks[1].line_start, Some(4));
        }
    }

    /// …and the CR really is carried, rather than the test above passing
    /// because the block happens to stop at a line boundary. A one-line block
    /// has no interior `\r` to keep; a two-line block does.
    #[test]
    fn an_interior_carriage_return_is_kept_and_a_trailing_one_is_not() {
        let blocks = extract_text(b"alpha\r\nbeta\r\n");
        assert_eq!(blocks[0].text, "alpha\r\nbeta");

        let blocks = extract_text(b"alpha\r\n");
        assert_eq!(
            blocks[0].text, "alpha",
            "the last line's carriage return belongs to the terminator, not the block"
        );
    }

    /// A line of spaces is content, not a separator — unchanged by the CRLF
    /// fix, and worth pinning because the new emptiness test could easily have
    /// been written as `trim().is_empty()`.
    #[test]
    fn a_line_of_spaces_does_not_separate_two_blocks() {
        let blocks = extract_text("один\n   \nдва\n".as_bytes());
        assert_eq!(blocks.len(), 1, "{blocks:?}");
    }
}

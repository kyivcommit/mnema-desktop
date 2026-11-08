//! The one-shot exchanges that draw a document for the window's page view:
//! one request line on the worker's stdin, one reply line on its stdout. A PDF
//! page is [`RenderRequest`]/[`RenderReply`] (`--render-page`); a document the
//! window shows as HTML — Markdown, HTML, an EPUB chapter — is [`DocRenderRequest`]/[`DocRenderReply`]
//! (`--render-doc`).
//!
//! Here, beside `wire`, for the same reason `wire` is here (D40): the worker
//! writes these and the application reads them, and the application may not
//! link the crate that draws the page.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Draw page `page_no` of the PDF at `path`, `width_px` wide, and box the
/// characters of `ranges`.
///
/// `ranges` count Unicode scalar values into the page's stored text. The boxes
/// are only drawn when the page's text, read again now, still hashes to
/// `text_sha256` — the offsets were counted in that text and in no other. `None`
/// asks for the page alone (a scan, or a file the index no longer holds as
/// this document).
///
/// Unknown fields are refused rather than ignored: a parent and a worker that
/// disagree about this line are two builds, and a silently dropped field is a
/// highlight silently not drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderRequest {
    pub path: String,
    /// 1-based, the reader's own page numbering (`Frame::Page::page_no`).
    pub page_no: u32,
    pub width_px: u32,
    pub text_sha256: Option<String>,
    pub ranges: Vec<RenderRange>,
    /// True only for a page the index holds as `ocr:*`: the worker then
    /// recognises the page again to box the citation (spec R3). Required, not
    /// defaulted: the application and the worker ship together.
    pub recognise: bool,
}

/// A stretch of the page's stored text: `len` chars from `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderRange {
    pub start: u32,
    pub len: u32,
    /// The citation the person clicked, as opposed to its neighbours.
    pub primary: bool,
}

/// The worker's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RenderReply {
    Rendered {
        /// Every page the document declares, for "page N of M".
        pages: u32,
        width_px: u32,
        height_px: u32,
        /// The page as PNG, base64 — passed to the window as a data URL, never
        /// decoded by the application.
        png_base64: String,
        text: TextMatch,
        /// Empty unless `text` is [`TextMatch::Match`].
        rects: Vec<RenderRect>,
    },
    Refused {
        error: RenderError,
        message: String,
    },
}

/// Whether the page's text, read again, is the text the offsets were counted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextMatch {
    /// Same text: the boxes are where the citation is.
    Match,
    /// Different text: the file or the reader changed. No boxes.
    Mismatch,
    /// The same text, but its ranges could not be placed on the page: the
    /// char map does not line up with it (NFC rewrote the page), the text
    /// layer could not be read again, or a range yielded no box. Not a change
    /// to the file, so not [`TextMatch::Mismatch`].
    Unplaced,
    /// No text to compare: the page has no text layer, or no hash was sent.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderError {
    NotPdf,
    NoSuchPage,
    /// The file could not be opened or parsed, or the request line was not one.
    Unreadable,
    /// The file, or what it renders to, is over the ceiling the page view
    /// holds (`--render-doc`); the text view is still there.
    TooLarge,
}

/// A box in fractions of the drawn page (0..1 on each axis, origin top left),
/// so the window can scale it with the image instead of asking again.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RenderRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub primary: bool,
}

/// Render the document at `path` as HTML with the characters of `ranges`
/// marked.
///
/// The marks are only placed when the document's blocks, read again now, still
/// hash to `text_sha256` under [`doc_text_sha256`]; otherwise the document is
/// rendered alone. Unknown fields are refused for [`RenderRequest`]'s reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocRenderRequest {
    pub path: String,
    /// The reader the index read the document with — chosen by the
    /// application from `document.mime`, never guessed again by the worker.
    pub reader: DocReader,
    /// The one page to render (an EPUB chapter); `None` is the whole file.
    pub page_no: Option<u32>,
    pub text_sha256: Option<String>,
    pub ranges: Vec<DocRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocReader {
    Markdown,
    Html,
    /// One chapter of a book, named by `page_no` (its spine position).
    Epub,
}

/// `len` chars from `start` in the text of one block, the block named by its
/// place in the reading order — the worker has no index, so no block id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocRange {
    pub page_no: u32,
    pub reading_order: u32,
    pub start: u32,
    pub len: u32,
    /// The citation the person clicked, as opposed to its neighbours.
    pub primary: bool,
}

/// The worker's answer to a [`DocRenderRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum DocRenderReply {
    Rendered {
        /// Allow-listed HTML, safe to put into the window as it stands.
        html: String,
        /// Every page the document declares.
        pages: u32,
        /// Marks are in `html` only when this is [`TextMatch::Match`].
        text: TextMatch,
    },
    Refused {
        error: RenderError,
        message: String,
    },
}

/// The hash of a document's blocks as the page view compares them: each
/// block's `page_no`, `block.type` and text, in the order given.
///
/// One function for both sides on purpose: the application hashes the rows of
/// the index, the worker the blocks it has just read again, and two spellings
/// of one formula disagree silently — every document would render without its
/// highlight and nothing would look wrong.
///
/// Every field is preceded by its length, so no text can be read as the end of
/// one field and the start of the next.
pub fn doc_text_sha256<'a>(blocks: impl IntoIterator<Item = (u32, &'a str, &'a str)>) -> String {
    let mut hash = Sha256::new();
    for (page_no, kind, text) in blocks {
        hash.update(page_no.to_le_bytes());
        for field in [kind, text] {
            hash.update((field.len() as u64).to_le_bytes());
            hash.update(field.as_bytes());
        }
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_round_trips_and_refuses_a_field_it_does_not_know() {
        let request = RenderRequest {
            path: "/tmp/a.pdf".into(),
            page_no: 3,
            width_px: 1200,
            text_sha256: Some("ab".into()),
            ranges: vec![RenderRange {
                start: 4,
                len: 7,
                primary: true,
            }],
            recognise: true,
        };
        let line = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<RenderRequest>(&line).unwrap(),
            request
        );

        let extra = line.replacen('{', "{\"zoom\":2,", 1);
        assert!(
            serde_json::from_str::<RenderRequest>(&extra).is_err(),
            "{extra}"
        );
    }

    #[test]
    fn a_request_without_recognise_is_refused() {
        let line =
            r#"{"path":"/tmp/a.pdf","page_no":1,"width_px":400,"text_sha256":null,"ranges":[]}"#;
        assert!(serde_json::from_str::<RenderRequest>(line).is_err());
        // Control: the same line with the field is a request, so the refusal
        // above is about `recognise` and nothing else.
        let with = line.replacen('}', ",\"recognise\":false}", 1);
        assert!(
            serde_json::from_str::<RenderRequest>(&with).is_ok(),
            "{with}"
        );
    }

    #[test]
    fn both_replies_round_trip_under_their_outcome_tag() {
        let rendered = RenderReply::Rendered {
            pages: 10,
            width_px: 1200,
            height_px: 1553,
            png_base64: "iVBO".into(),
            text: TextMatch::Match,
            rects: vec![RenderRect {
                x: 0.1,
                y: 0.2,
                w: 0.3,
                h: 0.01,
                primary: false,
            }],
        };
        let refused = RenderReply::Refused {
            error: RenderError::NoSuchPage,
            message: "page 11 of 10".into(),
        };
        for (reply, tag, word) in [
            (rendered, "\"outcome\":\"rendered\"", "\"text\":\"match\""),
            (
                refused,
                "\"outcome\":\"refused\"",
                "\"error\":\"no_such_page\"",
            ),
        ] {
            let line = serde_json::to_string(&reply).unwrap();
            assert!(line.contains(tag) && line.contains(word), "{line}");
            assert_eq!(serde_json::from_str::<RenderReply>(&line).unwrap(), reply);
        }
    }

    fn doc_request() -> DocRenderRequest {
        DocRenderRequest {
            path: "/tmp/a.md".into(),
            reader: DocReader::Markdown,
            page_no: None,
            text_sha256: Some("ab".into()),
            ranges: vec![DocRange {
                page_no: 2,
                reading_order: 1,
                start: 4,
                len: 7,
                primary: true,
            }],
        }
    }

    #[test]
    fn a_doc_request_round_trips_and_refuses_a_field_it_does_not_know() {
        let request = doc_request();
        let line = serde_json::to_string(&request).unwrap();
        assert!(line.contains("\"reader\":\"markdown\""), "{line}");
        assert_eq!(
            serde_json::from_str::<DocRenderRequest>(&line).unwrap(),
            request
        );
        let extra = line.replacen('{', "{\"zoom\":2,", 1);
        assert!(
            serde_json::from_str::<DocRenderRequest>(&extra).is_err(),
            "{extra}"
        );
        // A range is refused for an unknown field too: the block it names
        // would otherwise be the wrong one with no sign of it.
        let extra = line.replacen("\"start\"", "\"block_id\":9,\"start\"", 1);
        assert!(
            serde_json::from_str::<DocRenderRequest>(&extra).is_err(),
            "{extra}"
        );
    }

    #[test]
    fn every_reader_has_its_wire_name() {
        for (reader, name) in [
            (DocReader::Markdown, "\"markdown\""),
            (DocReader::Html, "\"html\""),
            (DocReader::Epub, "\"epub\""),
        ] {
            assert_eq!(serde_json::to_string(&reader).unwrap(), name);
            assert_eq!(serde_json::from_str::<DocReader>(name).unwrap(), reader);
        }
    }

    #[test]
    fn both_doc_replies_round_trip_under_their_outcome_tag() {
        let rendered = DocRenderReply::Rendered {
            html: "<p>a</p>".into(),
            pages: 3,
            text: TextMatch::Mismatch,
        };
        let refused = DocRenderReply::Refused {
            error: RenderError::TooLarge,
            message: "9 MB".into(),
        };
        for (reply, tag, word) in [
            (
                rendered,
                "\"outcome\":\"rendered\"",
                "\"text\":\"mismatch\"",
            ),
            (
                refused,
                "\"outcome\":\"refused\"",
                "\"error\":\"too_large\"",
            ),
        ] {
            let line = serde_json::to_string(&reply).unwrap();
            assert!(line.contains(tag) && line.contains(word), "{line}");
            assert_eq!(
                serde_json::from_str::<DocRenderReply>(&line).unwrap(),
                reply
            );
        }
    }

    #[test]
    fn the_doc_hash_changes_with_every_field_and_with_the_order() {
        let base = [(1, "paragraph", "один"), (2, "headline", "два")];
        let h = doc_text_sha256(base);
        assert_eq!(h.len(), 64);
        assert_eq!(h, doc_text_sha256(base), "the same blocks, the same hash");
        for changed in [
            [(3, "paragraph", "один"), (2, "headline", "два")],
            [(1, "code", "один"), (2, "headline", "два")],
            [(1, "paragraph", "одне"), (2, "headline", "два")],
            [(2, "headline", "два"), (1, "paragraph", "один")],
        ] {
            assert_ne!(doc_text_sha256(changed), h, "{changed:?}");
        }
    }

    #[test]
    fn the_doc_hash_does_not_read_one_field_into_the_next() {
        assert_ne!(
            doc_text_sha256([(1, "a", "bc")]),
            doc_text_sha256([(1, "ab", "c")])
        );
        assert_ne!(
            doc_text_sha256([(1, "p", "a"), (1, "p", "b")]),
            doc_text_sha256([(1, "p", "ab")]),
            "two blocks are not their concatenation"
        );
    }

    #[test]
    fn the_doc_hash_of_nothing_is_the_hash_of_no_bytes() {
        // Pins the formula: a change to it is a change to every stored
        // comparison, and should be seen in this file.
        assert_eq!(
            doc_text_sha256([]),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            doc_text_sha256([(1, "p", "a")]),
            doc_text_sha256([(1, "p", "a")].iter().copied())
        );
    }
}

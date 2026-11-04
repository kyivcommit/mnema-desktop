//! The one-shot exchange that draws a PDF page for the window's page view:
//! one [`RenderRequest`] line on the worker's stdin, one [`RenderReply`] line on
//! its stdout (`mnema-extract-worker --render-page`).
//!
//! Here, beside `wire`, for the same reason `wire` is here (D40): the worker
//! writes these and the application reads them, and the application may not
//! link the crate that draws the page.

use serde::{Deserialize, Serialize};

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
}

//! NDJSON: the wire format that crosses the worker/application process
//! boundary.
//!
//! Extraction runs in a separate worker process (task 7) because file parsers
//! consume untrusted input and a crashing C++ library — `pdfium` — must not
//! take the application down with it. The worker has no database access; it
//! prints frames to stdout, one JSON object per line, and the application
//! (`mnema-pool`) reads them back. A document's frames arrive in one of two
//! shapes: a file the worker can read produces one `Header`, then one `Page`
//! per page — each followed by that page's `Block`s in reading order — and
//! finally one `Summary`; a file it declines or cannot read produces exactly
//! one `Refused` or `Failed` frame and nothing else.
//!
//! **Why this module lives in `mnema-core` and not in `mnema-extract`, where
//! task 7 first wrote it.** D40 requires that `mnema-extract` be linked *only*
//! into the worker binary and never into the application: that is what makes
//! D35's "no code in the application reaches the Pdfium FFI" a structural fact
//! rather than a convention. But both sides of this protocol need these types
//! — the worker writes them and `mnema-pool`, which runs inside the
//! application, parses them. Leaving them in `mnema-extract` would put the
//! crate that links Pdfium into the application's dependency graph through the
//! back door. This is the same argument, and the same resolution, that already
//! put `Block` here (see `block.rs`): the type that crosses a boundary belongs
//! to neither side of it. `mnema_extract::wire` is kept as a re-export of this
//! module, so the name task 7 committed to still resolves.

use crate::{Block, SourceKind};
use serde::{Deserialize, Serialize};

/// One line of the wire protocol.
///
/// Internally tagged on a field named `frame` rather than `type`, so it does
/// not collide with `Block`'s own `block_type` field once a `Block` frame is
/// flattened onto the same JSON object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case")]
pub enum Frame {
    /// Sent once, before any block. `pages` is the page count the worker
    /// already knows without reading a single block — for txt/csv/code it is
    /// always 1 (D37: one page per file); it lets the application size its
    /// per-page bookkeeping before the blocks start arriving.
    ///
    /// It is also the one cheap integrity check on this stream: `mnema-pool`
    /// requires it to equal the number of [`Frame::Page`] frames that actually
    /// arrive.
    ///
    /// What that catches is a **worker binary that does not agree with the
    /// parent about the protocol** — a half-finished install, a sidecar from
    /// another release — which is the same failure
    /// `PoolError::Protocol` exists for elsewhere. It is deliberately *not*
    /// a truncation check, although it reads like one: a worker killed
    /// mid-document loses its `Summary` too and is classified by how it died,
    /// and a pipe does not drop interior bytes while still delivering later
    /// ones. For a correct reader the check is structurally unable to fire,
    /// because the worker takes both numbers from the same vector — which is
    /// the point, and is worth knowing before being clever with the count.
    Header {
        sha256: String,
        mime: String,
        source_kind: SourceKind,
        /// Which reader produced the frames that follow, and which version of
        /// it. Stated by the worker rather than derived by the parent: the
        /// parent may not link the crate that holds the readers (D40), and the
        /// answer has to survive the process boundary anyway.
        ///
        /// It is the reader that actually ran, not the one
        /// `manifest::for_extension` predicts — the two agree today, and the
        /// day they stop, this field is the one that is true. The manifest
        /// answers "would this file be read differently now"; this answers
        /// "how was it read".
        reader: String,
        reader_version: u32,
        pages: u32,
    },
    /// Opens a page. Every `Block` after it belongs to this page, until the
    /// next `Page` frame or the `Summary`.
    ///
    /// **Every reader sends this, plain text included** — exactly one
    /// `Page { page_no: 1, section_title: None }` before its blocks (D37: one
    /// page per text file). A protocol whose shape depended on the format
    /// would be one the pool could not parse without knowing formats, and the
    /// pool is the side of the boundary that deliberately knows none.
    ///
    /// A delimiter frame rather than a field on `Block`, for two reasons.
    /// `section_title` belongs to the page: as a field on `Block` it would be
    /// repeated on every block of the page, and two neighbours could then
    /// disagree about which section they are in. And a page with no blocks at
    /// all — a markdown heading with nothing under it, a PDF page whose text
    /// layer is empty — has no other way to exist, while the schema still
    /// needs its `page` row.
    ///
    /// `page_no` is the reader's own numbering, not a counter of these frames:
    /// a reader that skips a page (`Summary::skipped_pages` names it) leaves a
    /// gap here, and the gap is the honest record of it.
    Page {
        page_no: u32,
        section_title: Option<String>,
    },
    /// One source block, in reading order within its page.
    Block(Block),
    /// Sent once, after the last block. `skipped_pages` **names** the pages a
    /// reader dropped mid-document (a scanned PDF page with no text layer,
    /// say) — empty for every format that cannot skip a page, and disjoint
    /// from the `Page` frames that arrived. `text_source` matches
    /// `page.text_source`'s vocabulary (`schema.sql:101-102`); a document
    /// with several pages of different sources is not representable by this
    /// one field, which is fine for the readers this task ships (txt is
    /// always `native:txt`) and is a known limit for whoever adds a
    /// multi-page-source-family reader later.
    ///
    /// Numbers rather than a count, because the behaviour requirements ask for
    /// a journal row per skipped page and a count cannot say which page of the
    /// contract the scanner missed. The count is `skipped_pages.len()` and is
    /// **not** carried beside them, for the reason `mnema_pool::Document`
    /// gives for not keeping the header's page count: two numbers that can
    /// disagree leave a caller free to trust the wrong one.
    Summary {
        skipped_pages: Vec<u32>,
        text_source: String,
    },
    /// The worker looked at the file (or its metadata) and declined to read
    /// it: its size exceeds the request's `max_bytes` ceiling — checked from
    /// `stat`, before a byte is loaded — or `typing::identify` named a
    /// `Reader` that crate does not implement yet (`Docx`, `Xlsx`, `Epub`, or
    /// `Reader::Unrecognized` itself), or a reader ran and refused what it
    /// found: bytes that are not text, a text file with a binary tail, a PDF
    /// that is damaged, locked, or carries no text layer on any page.
    ///
    /// `rule` is a plain string rather than `mnema_index::SkipRule`
    /// on purpose: neither this crate nor `mnema-extract` may depend on
    /// `mnema-index` — a worker that links the database library it is
    /// forbidden from opening would undercut the very boundary task 7 exists
    /// to draw (D26, D40). The values are chosen to match that enum's
    /// vocabulary, so `mnema-pool` can parse them directly rather than
    /// translate.
    ///
    /// The two producers send **different** rules, and the string is the only
    /// thing that carries the difference across this boundary:
    /// `"too_large"` for the ceiling, `"unsupported"` for a format with no
    /// reader. They were one value until the parent was found to owe them
    /// opposite answers — it removes what the index holds under a path when a
    /// worker read a file and declined its content, and the ceiling branch
    /// decides from `stat` without opening anything. `mnema_index::SkipRule`
    /// carries the rest of the reasoning.
    /// `sha256` is the digest of the bytes the refusal was decided on, and it
    /// is what lets the parent tell "this file changed and became unindexable"
    /// from "this file did not change and the rule did". Without it the parent
    /// deletes on both, and the second is a document lost over a release
    /// upgrade — measured by the data-loss harness, which found a file whose
    /// bytes never moved disappearing from the index after any event that
    /// touched its modification time.
    ///
    /// `Option`, and not out of caution: the size ceiling refuses a file from
    /// `stat` **before a byte is read**, so there is no digest to send and
    /// there cannot be one. That is the same asymmetry that already makes
    /// `SkipRule::TooLarge` conditional on the size rather than on the rule.
    ///
    /// `#[serde(default)]` so that a worker from an older release — one that
    /// does not send this field at all — still parses. What the parent then
    /// does with `None` is its own decision and is written down at
    /// `mnema_ingest`'s `displaces`.
    Refused {
        rule: String,
        reason: String,
        #[serde(default)]
        sha256: Option<String>,
    },
    /// The worker could not carry out the request, having learned nothing
    /// about the file's content in the attempt. Usually because it could not
    /// obtain the bytes to classify: the path does not exist, is not a regular
    /// file (a directory, say), could not be read for permissions, or — the
    /// request line itself — was not valid JSON.
    ///
    /// **Also when the reader the file needs could not be brought up at all**,
    /// which is not a fact about the file and must not be reported as one. A
    /// dynamic library that is missing, is the wrong build, or is refused by
    /// code signing (`crates/mnema-extract/src/pdfium_probe.rs` splits those
    /// three) leaves the worker with bytes it cannot say anything about. The
    /// alternative — folding it into a content rule — is the failure
    /// `SkipRule::Malformed`'s own doc comment is written to forbid: every PDF
    /// journalled as damaged by a walk that reports success, and the rows
    /// outliving the repair.
    ///
    /// Distinct from `Refused`: a refusal is a decision about content the
    /// worker did manage to look at, a `Failed` is the world not matching what
    /// the request claimed. That line is what the two have in common across
    /// both cases above — neither says anything about what is *in* the file,
    /// which is why both are safe to retry and neither may remove a document.
    ///
    /// `message` is the only place the difference survives: the rule this maps
    /// onto cannot distinguish them, and the reason column can. A worker in
    /// this state should say which library and why.
    ///
    /// `mnema_index::SkipRule::Unreadable` is the rule this maps
    /// onto; `mnema_pool::Failure` performs the mapping and its doc comment
    /// carries the reasoning. That variant did not exist while task 7 was
    /// written, which is why this comment once said the case had no home.
    ///
    /// The rule is **wider than this frame** and should not be read back as its
    /// definition: three of the ways into it never involve a worker at all, so
    /// they cannot arrive as a frame. Its own variant enumerates them.
    Failed { message: String },
}

/// One line of input to the worker: which file to read, and the largest
/// number of bytes it may hold without being refused unread. The worker only
/// ever deserialises this; `Serialize` is carried too because `mnema-pool` is
/// what constructs these lines, the same split `Frame` has between the two
/// sides of the boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub path: String,
    pub max_bytes: u64,
}

/// Serialises `frame` as a single NDJSON line: compact JSON followed by
/// exactly one `\n`.
///
/// `serde_json` escapes a literal newline inside a string field as the two
/// characters `\` `n`, so a block whose text spans several lines still
/// produces exactly one line of output — the reason line framing is safe to
/// use at all for text that is stored verbatim.
pub fn to_line(frame: &Frame) -> Result<String, serde_json::Error> {
    let mut line = serde_json::to_string(frame)?;
    line.push('\n');
    Ok(line)
}

/// The same framing in the other direction: one `Request` as one NDJSON line.
///
/// Exists so that the newline discipline lives in one place. The pool could
/// call `serde_json::to_string` and append a byte itself, but then the rule
/// "exactly one `\n`, and the payload may never contain a raw one" would be
/// stated on the writing side of the protocol and enforced on the reading
/// side by a different crate.
pub fn to_request_line(request: &Request) -> Result<String, serde_json::Error> {
    let mut line = serde_json::to_string(request)?;
    line.push('\n');
    Ok(line)
}

/// Parses one line back into a `Frame`. Accepts a trailing `\n` or `\r\n` —
/// whatever `to_line` appended, or a line read by a `BufRead` that kept its
/// terminator — since `serde_json` treats trailing bytes after a complete
/// value as an error rather than ignoring them.
pub fn from_line(line: &str) -> Result<Frame, serde_json::Error> {
    serde_json::from_str(line.trim_end_matches(['\n', '\r']))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockType;

    fn sample_header() -> Frame {
        Frame::Header {
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b85".to_string(),
            mime: "text/plain".to_string(),
            source_kind: SourceKind::Document,
            reader: "text".to_string(),
            reader_version: 1,
            pages: 1,
        }
    }

    /// A page that carries a title, and one that does not: `section_title` is
    /// `Option<String>`, and a frame fixture that only ever exercises `Some`
    /// would not notice a serialisation that dropped the field's absence.
    fn sample_page() -> Frame {
        Frame::Page {
            page_no: 4,
            section_title: Some("Розділ другий".to_string()),
        }
    }

    fn sample_untitled_page() -> Frame {
        Frame::Page {
            page_no: 1,
            section_title: None,
        }
    }

    fn sample_block_frame() -> Frame {
        Frame::Block(Block {
            block_type: BlockType::Paragraph,
            reading_order: 0,
            language: None,
            text: "перший рядок\nдругий рядок".to_string(),
            line_start: Some(1),
            line_end: Some(2),
        })
    }

    /// Two skipped pages, not none: an empty vector round-trips through a
    /// serialiser that drops the field entirely, so a fixture others copy has
    /// to carry numbers for the round-trip test to be asking anything.
    fn sample_summary() -> Frame {
        Frame::Summary {
            skipped_pages: vec![2, 5],
            text_source: "native:txt".to_string(),
        }
    }

    /// The ceiling's refusal, under the rule the ceiling actually sends. The
    /// pairing matters in a fixture others copy: `"unsupported"` beside a
    /// reason about a byte ceiling is the confusion that let the two producers
    /// share one rule.
    fn sample_refused() -> Frame {
        Frame::Refused {
            rule: "too_large".to_string(),
            reason: "invented.zip is 4096 bytes, over the 1024-byte ceiling".to_string(),
            // The ceiling decides from `stat` and never opens the file, so this
            // fixture is also the one refusal that legitimately carries no
            // digest — and round-tripping `None` is worth having in the
            // fixture others copy.
            sha256: None,
        }
    }

    fn sample_failed() -> Frame {
        Frame::Failed {
            message: "invented/missing.txt: No such file or directory (os error 2)".to_string(),
        }
    }

    #[test]
    fn every_frame_kind_round_trips_through_one_line() {
        for frame in [
            sample_header(),
            sample_page(),
            sample_untitled_page(),
            sample_block_frame(),
            sample_summary(),
            sample_refused(),
            sample_failed(),
        ] {
            let line = to_line(&frame).unwrap();
            assert_eq!(line.matches('\n').count(), 1);
            assert_eq!(from_line(&line).unwrap(), frame);
        }
    }

    /// A refusal from a worker that predates the digest still parses, and one
    /// that carries it keeps it. Both directions, because either alone is
    /// satisfied by a mistake: a field that is always dropped would pass the
    /// first, and a field that is required would pass the second.
    ///
    /// The `#[serde(default)]` on `sha256` exists for exactly the first line
    /// here and for nothing else. Without this test the attribute is a claim
    /// with nothing behind it — and the failure it prevents is not a parse
    /// error but a whole walk answered as protocol failures by a parent that
    /// met a sidecar one release behind.
    #[test]
    fn a_refusal_without_a_digest_still_parses_and_one_with_it_keeps_it() {
        let old = r#"{"frame":"refused","rule":"not_text","reason":"not text"}"#;
        assert_eq!(
            from_line(old).unwrap(),
            Frame::Refused {
                rule: "not_text".to_string(),
                reason: "not text".to_string(),
                sha256: None,
            }
        );

        let new = r#"{"frame":"refused","rule":"not_text","reason":"not text","sha256":"abc123"}"#;
        assert_eq!(
            from_line(new).unwrap(),
            Frame::Refused {
                rule: "not_text".to_string(),
                reason: "not text".to_string(),
                sha256: Some("abc123".to_string()),
            }
        );
    }

    /// The opposite decision to the one above, on purpose, and worth stating
    /// next to it: a header from a worker that predates `reader` does **not**
    /// parse. No `#[serde(default)]`.
    ///
    /// `Refused::sha256` defaults because `None` is a real answer there — the
    /// size ceiling refuses from `stat` and there is no digest to send. There
    /// is no such thing as a document produced by no reader, so a default here
    /// could only be a placeholder, and a placeholder is worse than a stop:
    /// every file such a worker read would be recorded as made by the empty
    /// reader at version 0, which never matches any manifest, so every one of
    /// them would be re-read on every run for ever. A `PoolError::Protocol`
    /// stops the job instead and names the mismatch — the same answer this
    /// crate's parent already gives an unknown `Refused` rule, and for the
    /// same reason.
    #[test]
    fn a_header_from_a_worker_that_predates_the_reader_field_is_a_protocol_error() {
        let old = r#"{"frame":"header","sha256":"abc","mime":"text/plain","source_kind":"document","pages":1}"#;
        let error = from_line(old).expect_err("a header with no reader must not parse");
        assert!(
            error.to_string().contains("reader"),
            "the error must name the missing field, got: {error}"
        );

        // The other direction: a header that carries the fields keeps their
        // values rather than any default. Without this, an implementation that
        // rejected every header would pass the assertion above.
        let new = r#"{"frame":"header","sha256":"abc","mime":"text/markdown","source_kind":"document","reader":"markdown","reader_version":4,"pages":2}"#;
        assert_eq!(
            from_line(new).unwrap(),
            Frame::Header {
                sha256: "abc".to_string(),
                mime: "text/markdown".to_string(),
                source_kind: SourceKind::Document,
                reader: "markdown".to_string(),
                reader_version: 4,
                pages: 2,
            }
        );
    }

    #[test]
    fn a_crlf_terminated_line_still_parses() {
        let line = to_line(&sample_summary()).unwrap();
        let crlf = format!("{}\r\n", line.trim_end());
        assert_eq!(from_line(&crlf).unwrap(), sample_summary());
    }

    #[test]
    fn a_request_is_one_line_the_worker_can_read_back() {
        let request = Request {
            path: "документи/звіт.txt".to_string(),
            max_bytes: 1 << 26,
        };
        let line = to_request_line(&request).unwrap();
        assert_eq!(line.matches('\n').count(), 1);
        assert!(line.ends_with('\n'));
        // The worker parses the line after `BufRead` has kept its terminator,
        // so what it actually calls is `from_str` on a string ending in `\n`
        // — mirrored here rather than trimming first.
        let back: Request = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(back, request);
    }
}

//! The extraction worker (task 7): reads one request per line on stdin,
//! writes NDJSON frames (`mnema_extract::wire::Frame`) on stdout, and holds
//! no database connection of any kind — see `mnema_extract::wire`'s module
//! doc for why that boundary exists. File parsers consume untrusted bytes; a
//! malformed PDF can fault the C++ library that reads it and take the whole
//! process down, and no guard in Rust catches that. Running extraction here,
//! in a process with nothing valuable in it, is what keeps a crash from
//! reaching the application that holds the index.
//!
//! **stdout carries frames and nothing else.** Every diagnostic this binary
//! has to say goes to stderr; a stray print on stdout would corrupt the
//! stream the parent parses line by line.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use mnema_extract::typing::{Reader, identify};
use mnema_extract::wire::{Frame, Request, to_line};
use mnema_extract::{extract_markdown, extract_text};
use sha2::{Digest, Sha256};

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                // The pipe itself failed (not a JSON error) — nothing further
                // can be read, so there is nothing left to do but stop.
                eprintln!("mnema-extract-worker: could not read stdin: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        for frame in handle_request(&line) {
            write_frame(&mut stdout, &frame);
        }
    }
}

/// Serialises and writes one frame, flushing immediately: a multi-hour job's
/// parent may be reading these as they arrive, or may kill this process on a
/// timeout, and either way a frame sitting in an unflushed buffer is a frame
/// the parent never sees.
fn write_frame(stdout: &mut io::Stdout, frame: &Frame) {
    let line = to_line(frame).expect("a Frame always serialises to JSON");
    if let Err(e) = stdout
        .write_all(line.as_bytes())
        .and_then(|()| stdout.flush())
    {
        eprintln!("mnema-extract-worker: could not write to stdout: {e}");
    }
}

/// Handles one request line, returning the frames it produces: `[Header,
/// Page, Block..., Summary]` for a file this crate can read — one `Page` per
/// page, each followed by its own blocks — or exactly one `Refused`/`Failed`
/// frame for a file it declines or cannot read.
fn handle_request(line: &str) -> Vec<Frame> {
    let request: Request = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(e) => {
            return vec![Frame::Failed {
                message: format!("malformed request: {e}"),
            }];
        }
    };

    let path = Path::new(&request.path);

    // `metadata` follows symlinks, so a link to a real file is accepted and a
    // link to a directory (or a broken link) is refused below by the same
    // check that catches a directory named directly.
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(e) => {
            return vec![Frame::Failed {
                message: format!("{}: {e}", request.path),
            }];
        }
    };

    if !metadata.is_file() {
        return vec![Frame::Failed {
            message: format!("{} is not a regular file", request.path),
        }];
    }

    // The ceiling is checked from `stat`, before a single byte is loaded —
    // "refused without being read" is a real property of this branch, not
    // just its name: no allocation sized by `size` happens on this path.
    //
    // `too_large`, not `unsupported`, and the difference is load-bearing on
    // the other side of the wire. The parent removes what the index holds
    // under a path when a worker read a file and would not have its content;
    // this branch never opened the file and decided on a configured number, so
    // lowering `max_bytes` must not delete anything. See `SkipRule::TooLarge`.
    let size = metadata.len();
    if size > request.max_bytes {
        return vec![Frame::Refused {
            rule: "too_large".to_string(),
            reason: format!(
                "{} is {size} bytes, over the {}-byte ceiling",
                request.path, request.max_bytes
            ),
        }];
    }

    // Read once: this same `Vec<u8>` is hashed and handed to the reader, so
    // there is no window between "the bytes we hashed" and "the bytes we
    // read" for the file to change in.
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return vec![Frame::Failed {
                message: format!("{}: {e}", request.path),
            }];
        }
    };

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha256 = hex(&hasher.finalize());

    let extension = path.extension().and_then(|e| e.to_str());
    let file_type = identify(&bytes, extension);

    match file_type.reader {
        Reader::PlainText => {
            let blocks = extract_text(&bytes);
            let mut frames = Vec::with_capacity(blocks.len() + 3);
            frames.push(Frame::Header {
                sha256,
                mime: file_type.mime.to_string(),
                source_kind: file_type.source_kind,
                pages: 1,
            });
            // One page, always, even for an empty file with no blocks under
            // it: the page frame is what the pool counts against the header,
            // and `block.page_id` is NOT NULL, so a format with no pages of
            // its own still owes the schema one (D37).
            frames.push(Frame::Page {
                page_no: 1,
                section_title: None,
            });
            frames.extend(blocks.into_iter().map(Frame::Block));
            frames.push(Frame::Summary {
                skipped_pages: 0,
                text_source: "native:txt".to_string(),
            });
            frames
        }
        Reader::Markdown => {
            let pages = extract_markdown(&bytes);
            let blocks: usize = pages.iter().map(|page| page.blocks.len()).sum();
            let mut frames = Vec::with_capacity(blocks + pages.len() + 2);
            frames.push(Frame::Header {
                sha256,
                mime: file_type.mime.to_string(),
                source_kind: file_type.source_kind,
                // The count the pool checks the page frames against, so it is
                // taken from the same vector those frames come from rather
                // than counted a second way.
                pages: pages.len() as u32,
            });
            for page in pages {
                frames.push(Frame::Page {
                    page_no: page.page_no,
                    section_title: page.section_title,
                });
                frames.extend(page.blocks.into_iter().map(Frame::Block));
            }
            frames.push(Frame::Summary {
                skipped_pages: 0,
                // `native:md` satisfies `page.text_source`'s CHECK
                // (`crates/mnema-index/src/schema.sql:101-102`) and names the
                // reader rather than the file: text that came out of a
                // markdown parse is not the same evidence as text that came
                // out of a plain-text read, even for the same bytes.
                text_source: "native:md".to_string(),
            });
            frames
        }
        // Not "no reader yet" — the answer this branch gives for the other
        // five. These bytes are not text at all, and no release adds a reader
        // that makes them prose (D51).
        Reader::NotText => {
            vec![Frame::Refused {
                rule: "not_text".to_string(),
                reason: "this file is not text: its bytes are not something this product reads"
                    .to_string(),
            }]
        }
        // None of these five formats has a `Vec<Block>` reader in this crate
        // yet — task 6 shipped only plain text, and `pdfium_probe` proves the
        // binding links without deciding what a page's text *is* (its own
        // doc comment). Reporting them alike as "unsupported" is honestly
        // what is true today: this worker can read text and nothing else.
        Reader::Pdf | Reader::Docx | Reader::Xlsx | Reader::Epub | Reader::Unrecognized => {
            vec![Frame::Refused {
                rule: "unsupported".to_string(),
                reason: format!(
                    "no reader implemented yet for {} ({:?})",
                    file_type.mime, file_type.reader
                ),
            }]
        }
    }
}

/// Lower-case hex of a digest.
///
/// Mirrors `crates/mnema-index/src/write.rs`'s own helper rather than
/// depending on it: sha2 0.11's `finalize()` returns `hybrid_array::Array`,
/// which implements neither `LowerHex` nor a route to `format!("{:x}")`, so
/// both crates carry this same small lookup table instead of either reaching
/// for a `hex` dependency.
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    out
}

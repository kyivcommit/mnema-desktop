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

use mnema_extract::manifest;
use mnema_extract::typing::{Reader, identify};
use mnema_extract::wire::{Frame, Request, to_line};
use mnema_extract::{
    PdfError, TEXT_LAYER_MIN_CHARS, extract_html, extract_markdown, extract_pdf, extract_text,
};
use sha2::{Digest, Sha256};

fn main() {
    // A diagnostic branch, not part of the NDJSON protocol: it answers one
    // question — does this build load Pdfium from where it is installed — and
    // exits (D53, D54).
    //
    // It was written when no reader called the library, so the wire could not
    // be asked at all. The wire can be asked now: send a PDF and a bundle
    // whose library will not load answers `Frame::Failed`. The flag stays
    // because the two answers are not the same one. This branch needs no
    // readable PDF and no file the caller had to bring, and it names *which*
    // of the three loading stages failed — while the wire's answer is a
    // sentence about one file, from a run that had to have a file to send.
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--probe-pdfium" {
        let line = match mnema_extract::probe_text_layer(Path::new(&args[2])) {
            Ok(probes) => format!(
                "{{\"loaded\":true,\"pages\":{},\"stage\":\"ok\"}}",
                probes.len()
            ),
            // `stage` names which of library_dir/verify_build/bind failed,
            // separately from `error`'s free text: those three collapse onto
            // the same `loaded:false` and a caller reading only the boolean
            // cannot tell "the library is not where expected" apart from
            // "code signing refused to load it" — see `Stage`'s own doc for
            // why that gap is not hypothetical.
            Err(e) => format!(
                "{{\"loaded\":false,\"stage\":{},\"error\":{}}}",
                serde_json::to_string(e.stage()).expect("a string serialises"),
                serde_json::to_string(&e.to_string()).expect("a string serialises")
            ),
        };
        println!("{line}");
        return;
    }

    // The second diagnostic branch, and the one the application actually calls
    // on every run: it prints what this build's readers are and exits. Not part
    // of the NDJSON protocol either — the parent needs the answer *before* it
    // decides which files to send, and it may not link this crate to read the
    // constants directly (D40). See `mnema_core::manifest`.
    if args.len() == 2 && args[1] == "--manifest" {
        println!(
            "{}",
            serde_json::to_string(&manifest::manifest()).expect("the manifest serialises")
        );
        return;
    }

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
            // The one refusal with no digest, and it cannot have one: this
            // branch decided from `stat` and never opened the file. Hashing it
            // here to fill the field in would read exactly the bytes the
            // ceiling exists to avoid reading.
            sha256: None,
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
                // Named in the branch that ran, not looked up from the
                // manifest by extension: this is the record of how the file
                // *was* read, and a lookup here would make the two agree by
                // construction and hide the day they stop agreeing.
                reader: "text".to_string(),
                reader_version: manifest::TEXT_READER_VERSION,
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
                // Empty, not absent: a format with one page has no page it
                // could drop, and the field says so rather than being optional.
                skipped_pages: Vec::new(),
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
                reader: "markdown".to_string(),
                reader_version: manifest::MARKDOWN_READER_VERSION,
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
                // Markdown drops no page: `extract_markdown` makes one per
                // heading and keeps every one it makes.
                skipped_pages: Vec::new(),
                // `native:md` satisfies `page.text_source`'s CHECK
                // (`crates/mnema-index/src/schema.sql:101-102`) and names the
                // reader rather than the file: text that came out of a
                // markdown parse is not the same evidence as text that came
                // out of a plain-text read, even for the same bytes.
                text_source: "native:md".to_string(),
            });
            frames
        }
        Reader::Html => {
            let pages = extract_html(&bytes);
            let blocks: usize = pages.iter().map(|page| page.blocks.len()).sum();
            let mut frames = Vec::with_capacity(blocks + pages.len() + 2);
            frames.push(Frame::Header {
                sha256,
                mime: file_type.mime.to_string(),
                source_kind: file_type.source_kind,
                // The constant, not the literal `"html"`. `pages_of` on the
                // other side of the wire matches this exact string to cite an
                // HTML chunk by its section, and may not link this crate (D40);
                // a typo here falls to `PageContext::Lines`, which asks blocks
                // that carry no line numbers for a line range and answers
                // `Coordinate::None` — a citation with no coordinate at all,
                // silently, with everything else green.
                reader: manifest::READER_HTML.to_string(),
                reader_version: manifest::HTML_READER_VERSION,
                // From the same vector the Page frames come from, so the pool's
                // count check cannot disagree with itself.
                pages: pages.len() as u32,
            });
            for page in pages {
                frames.push(Frame::Page {
                    page_no: page.page_no,
                    // Unlike a PDF's, an HTML page *is* a section, and this is
                    // the whole of what a citation into it points at.
                    section_title: page.section_title,
                });
                frames.extend(page.blocks.into_iter().map(Frame::Block));
            }
            frames.push(Frame::Summary {
                // Empty, not absent: this reader cannot skip a page. It makes
                // one per section and keeps every one it makes, so a number
                // here would name a page that was also sent — which the pool
                // reads as a mismatched worker binary and stops the job for.
                skipped_pages: Vec::new(),
                text_source: "native:html".to_string(),
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
                // The digest of the bytes this verdict was reached on, taken
                // above, before `identify` ran. It is what tells the parent
                // whether the file changed or only the rule did.
                sha256: Some(sha256),
            }]
        }
        // Also refused, and deliberately under a rule of its own: the parent
        // removes what the index holds under a path when a worker read a file
        // and declined its content, and this is the one refusal by content
        // that must not trigger it. The file opened as text and stopped, which
        // is what an interrupted append leaves behind — the prose is still on
        // disk, and the document under this path is still mostly that prose
        // (D51). `SkipRule::BinaryTail` carries the rest.
        Reader::BinaryTail => {
            vec![Frame::Refused {
                rule: "binary_tail".to_string(),
                reason: "this file starts as text and then stops being one: it may be truncated \
                         or damaged"
                    .to_string(),
                sha256: Some(sha256),
            }]
        }
        Reader::Pdf => match extract_pdf(&bytes) {
            // Every page was below the text-layer threshold, so there is no
            // document to build — but the file *was* read, and this says so
            // under a rule about content rather than under `unsupported`,
            // which promises a reader that is coming. `SkipRule::NoTextLayer`
            // is what the parent records.
            Ok(doc) if doc.pages.is_empty() => vec![Frame::Refused {
                rule: "no_text_layer".to_string(),
                reason: format!(
                    "no page of this PDF carries a text layer of at least \
                     {TEXT_LAYER_MIN_CHARS} characters"
                ),
                sha256: Some(sha256),
            }],
            Ok(doc) => {
                let blocks: usize = doc.pages.iter().map(|p| p.blocks.len()).sum();
                let mut frames = Vec::with_capacity(blocks + doc.pages.len() + 2);
                frames.push(Frame::Header {
                    sha256,
                    mime: file_type.mime.to_string(),
                    source_kind: file_type.source_kind,
                    // The constant, not the literal `"pdf"`. `pages_of` on the
                    // other side of the wire picks a PDF chunk's coordinate by
                    // this exact string and may not link this crate (D40), so
                    // a typo here would cost every PDF citation its page
                    // number and nothing would go red.
                    reader: manifest::READER_PDF.to_string(),
                    reader_version: manifest::PDF_READER_VERSION,
                    // From the same vector the Page frames come from, so the
                    // pool's count check cannot disagree with itself. Skipped
                    // pages are NOT in this number: they produce no Page frame
                    // and are named by `skipped_pages` instead.
                    pages: doc.pages.len() as u32,
                });
                for page in doc.pages {
                    frames.push(Frame::Page {
                        page_no: page.page_no,
                        // A PDF page is not a section. `pages_of` gives this
                        // reader `Coordinate::Page`, which is what a citation
                        // into a PDF points at.
                        section_title: None,
                    });
                    frames.extend(page.blocks.into_iter().map(Frame::Block));
                }
                frames.push(Frame::Summary {
                    // The numbers, not their count. This reader is the only
                    // thing in the product that ever knows *which* page of a
                    // contract the scanner missed, and the parent owes a
                    // journal row per page — which a count cannot fill in.
                    skipped_pages: doc.skipped,
                    text_source: "native:pdf".to_string(),
                });
                frames
            }
            Err(PdfError::Encrypted) => vec![Frame::Refused {
                rule: "encrypted".to_string(),
                reason: "this PDF is password-protected".to_string(),
                sha256: Some(sha256),
            }],
            // A reader that could not load its own library is NOT a damaged
            // file, and this arm must not swallow it. Under `malformed` the
            // walk would not stop (`suggests_broken_environment() == false`)
            // and the verdict would outlive the repair
            // (`is_about_content() == true`): every PDF in a folder journalled
            // as damaged by a green walk, and a fixed install returning
            // nothing. `crates/mnema-pool/src/lib.rs:300-303` names exactly
            // that outcome, and a quarantined `libpdfium.dylib` has already
            // happened on this machine.
            Err(PdfError::Library(e)) => vec![Frame::Failed {
                message: format!("pdfium could not be loaded: {e}"),
            }],
            // Bound to the one remaining variant rather than written as a
            // catch-all `Err(e)`. A catch-all is how a later variant — a
            // timeout, a page limit — would arrive silently as "this file is
            // damaged", which is the mistake the arm above exists to undo.
            Err(e @ PdfError::Malformed(_)) => vec![Frame::Refused {
                rule: "malformed".to_string(),
                reason: e.to_string(),
                sha256: Some(sha256),
            }],
        },
        // None of these four formats has a `Vec<Block>` reader in this crate
        // yet. Reporting them alike as "unsupported" is honestly what is true
        // today: this worker reads text, markdown, PDF and HTML, and nothing
        // else.
        Reader::Docx | Reader::Xlsx | Reader::Epub | Reader::Unrecognized => {
            vec![Frame::Refused {
                rule: "unsupported".to_string(),
                reason: format!(
                    "no reader implemented yet for {} ({:?})",
                    file_type.mime, file_type.reader
                ),
                sha256: Some(sha256),
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

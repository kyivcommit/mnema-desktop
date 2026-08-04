//! A stand-in extraction worker, for `tests/supervision.rs` only. **Not a
//! product binary** — nothing outside this crate's tests should ever run it.
//!
//! It speaks the real protocol (`mnema_core::wire`) so the supervisor under
//! test parses genuine frames, and it takes its instructions from the one field
//! a request carries: the path. A prefix before the first `:` selects a
//! behaviour, and anything unrecognised is answered like an ordinary readable
//! file. Driving it through the request rather than through argv or an
//! environment variable is deliberate: the pool spawns the worker exactly once
//! per batch and passes no arguments, and two tests running in parallel in one
//! process cannot share a mutable environment without racing.
//!
//! The real worker is `mnema-extract-worker`. It cannot be used here: it has no
//! way to hang, crash or flood stderr on demand — and Cargo does not build a
//! *dependency's* binaries for `cargo test -p mnema-pool`, so its path would
//! not reliably exist.

use std::io::{self, BufRead, Write};
use std::time::Duration;

use mnema_core::wire::{Frame, Request, to_line};
use mnema_core::{Block, BlockType, SourceKind};

/// Nothing this binary does may outlive the test that started it. Every mode
/// that waits, waits under this bound: if the supervisor fails to kill a hung
/// worker, the leak lasts two minutes rather than until the machine reboots.
const SELF_DESTRUCT: Duration = Duration::from_secs(120);

/// Exit code for the self-destruct path, chosen not to collide with anything
/// meaningful: a test that sees it knows the worker outlived its usefulness.
const SELF_DESTRUCT_CODE: i32 = 97;

fn main() {
    std::thread::spawn(|| {
        std::thread::sleep(SELF_DESTRUCT);
        eprintln!("test worker: self-destruct after {SELF_DESTRUCT:?}");
        std::process::exit(SELF_DESTRUCT_CODE);
    });

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = serde_json::from_str(&line).expect("the pool sends valid requests");
        let (mode, rest) = match request.path.split_once(':') {
            Some((mode, rest)) => (mode, rest),
            None => ("ok", request.path.as_str()),
        };
        act(mode, rest, &mut stdout);
    }
}

fn act(mode: &str, rest: &str, stdout: &mut io::Stdout) {
    match mode {
        // Four times the 65,536-byte pipe capacity measured on this platform.
        // A parent that gave the child a stderr *pipe* and drained only stdout
        // wedges here; one that gave it a file does not notice.
        "noisy" => {
            let noise = "x".repeat(1024);
            for _ in 0..256 {
                eprintln!("{noise}");
            }
            answer(stdout);
        }
        // Reads the request, says nothing. The supervisor's deadline is the
        // only thing that ends this.
        "hang" => std::thread::sleep(SELF_DESTRUCT),
        // Dies by SIGABRT without answering, the way a C++ parser faulting on
        // a malformed document does.
        "crash" => std::process::abort(),
        // Allocates `rest` megabytes in 16 MiB steps, touching every page so
        // that a lazily-mapped reservation cannot stand in for real growth.
        // Under an address-space ceiling this dies; under a ceiling large
        // enough it answers normally, which is what makes the pair of hog
        // tests a controlled comparison rather than one observation.
        "hog" => {
            let target_mb: usize = rest.parse().expect("hog:<megabytes>");
            let mut held: Vec<Vec<u8>> = Vec::new();
            while held.len() * 16 < target_mb {
                let mut chunk = vec![0u8; 16 << 20];
                for page in (0..chunk.len()).step_by(4096) {
                    chunk[page] = 1;
                }
                held.push(chunk);
            }
            answer(stdout);
        }
        // Waits until the file named by `rest` appears, then answers. Lets a
        // test hold workers occupied for as long as it needs without a sleep
        // deciding whether the test passes.
        "gate" => {
            let deadline = std::time::Instant::now() + SELF_DESTRUCT;
            while !std::path::Path::new(rest).exists() {
                if std::time::Instant::now() > deadline {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            answer(stdout);
        }
        // Records its own process id in the file named by `rest`, then answers
        // normally. Lets a test kill this exact worker between documents, which
        // is the only way to arrange an idle worker's death deterministically.
        "pid" => {
            std::fs::write(rest, std::process::id().to_string()).expect("the pid file is writable");
            answer(stdout);
        }
        // Stops listening — closes its own end of the request pipe — and then
        // answers. The order matters: by the time the pool has read this
        // answer, the pipe is already closed, so the *next* request cannot be
        // handed over. That is `Answer::Unsendable` on a worker that is still
        // running, which no amount of asking whether it has exited can predict.
        #[cfg(unix)]
        "deaf" => {
            // SAFETY: closing file descriptor 0 while nothing is reading it.
            // This binary never touches stdin again — it sleeps below instead of
            // returning to the request loop, which would see the closed
            // descriptor and exit.
            unsafe {
                libc::close(0);
            }
            answer(stdout);
            std::thread::sleep(SELF_DESTRUCT);
        }
        // Writes bytes that are not UTF-8 to **stdout**, where frames belong,
        // and stays alive. This is what a C++ library that logs to the wrong
        // descriptor looks like from the parent: not a disagreement about the
        // protocol, just unusable output.
        "raw-bytes" => {
            stdout
                .write_all(&[0xff, 0xfe, 0x00, b'\n'])
                .and_then(|()| stdout.flush())
                .expect("the pool keeps reading stdout");
            std::thread::sleep(SELF_DESTRUCT);
        }
        // `sha256: None` in all four refusals below. This stand-in never reads
        // the file it is asked about — it answers from a prefix on the path —
        // so it has no digest to report, and inventing one would make the
        // parent's "did the bytes change?" question answerable by a worker that
        // never looked. The journey of a real digest across this wire is
        // covered end to end by `a_file_whose_bytes_did_not_change_keeps_its_document`
        // in `mnema-ingest/tests/slice.rs`, through the real pool.
        "refuse" => write_frame(
            stdout,
            &Frame::Refused {
                rule: "unsupported".to_string(),
                reason: format!("no reader for {rest}"),
                sha256: None,
            },
        ),
        // The other producer of `Frame::Refused`, and the reason the two need
        // separate rules: the real worker takes this branch from `stat`,
        // without opening the file, so the parent must not treat it as "read
        // and declined".
        "toobig" => write_frame(
            stdout,
            &Frame::Refused {
                rule: "too_large".to_string(),
                reason: format!("{rest} is over the ceiling"),
                sha256: None,
            },
        ),
        // The refusal added for D51: the worker opened the file, read the
        // bytes, and they are not text. A rule the pool has to carry across
        // the wire under its own name rather than fold into `"unsupported"`.
        "notext" => write_frame(
            stdout,
            &Frame::Refused {
                rule: "not_text".to_string(),
                reason: format!("{rest} is not text"),
                sha256: None,
            },
        ),
        // A refusal under a rule this pool has never heard of, which is what a
        // worker from another release looks like.
        //
        // The string was `"encrypted"` until that became a rule the pool does
        // know, and the swap is the whole lesson of this branch: a stand-in for
        // "unknown" must be a name nobody will later implement, or the test
        // above it quietly stops testing anything. It did not go quiet here —
        // `a_refusal_under_an_unknown_rule_stops_the_job` reddened the moment
        // the pool learned the word — but only because that test also asserts
        // the error names the rule. A test that had merely checked "some
        // refusal came back" would have gone on passing.
        "newrule" => write_frame(
            stdout,
            &Frame::Refused {
                rule: "rule_from_a_later_release".to_string(),
                reason: format!("{rest} was refused for a reason this build cannot name"),
                sha256: None,
            },
        ),
        "fail" => write_frame(
            stdout,
            &Frame::Failed {
                message: format!("{rest}: No such file or directory (os error 2)"),
            },
        ),
        // Announces more pages in the header than it then sends: a worker
        // binary that does not agree with the parent about the protocol, which
        // is the only way this disagreement occurs — the real worker takes
        // both numbers from one vector. Without the count the parent cannot
        // tell this from a genuinely short document, and the reason the header
        // carries one.
        "short-count" => {
            write_frame(
                stdout,
                &Frame::Header {
                    sha256: "0".repeat(64),
                    mime: "text/markdown".to_string(),
                    source_kind: SourceKind::Document,
                    reader: "markdown".to_string(),
                    reader_version: 1,
                    pages: 3,
                },
            );
            write_frame(
                stdout,
                &Frame::Page {
                    page_no: 1,
                    section_title: Some("Розділ перший".to_string()),
                },
            );
            write_frame(
                stdout,
                &Frame::Summary {
                    skipped_pages: 0,
                    text_source: "native:md".to_string(),
                },
            );
        }
        // A header whose reader has no name. Not a hypothetical shape: `""` is
        // what `#[serde(default)]` on that field would hand the parent, and it
        // is what any producer writing the frame by hand leaves behind — the
        // four worker stubs under scripts/ did exactly that until this field
        // existed. The frame is otherwise complete and parses cleanly, which is
        // the whole difficulty: nothing downstream would notice.
        "nameless-reader" => {
            write_frame(
                stdout,
                &Frame::Header {
                    sha256: "0".repeat(64),
                    mime: "text/plain".to_string(),
                    source_kind: SourceKind::Document,
                    reader: String::new(),
                    reader_version: 0,
                    pages: 1,
                },
            );
            write_frame(
                stdout,
                &Frame::Page {
                    page_no: 1,
                    section_title: None,
                },
            );
            write_frame(
                stdout,
                &Frame::Summary {
                    skipped_pages: 0,
                    text_source: "native:txt".to_string(),
                },
            );
        }
        // A block with no page open before it: a worker that skipped the page
        // marker altogether, which is the older protocol still speaking.
        "pageless" => {
            write_frame(
                stdout,
                &Frame::Header {
                    sha256: "0".repeat(64),
                    mime: "text/plain".to_string(),
                    source_kind: SourceKind::Document,
                    reader: "text".to_string(),
                    reader_version: 1,
                    pages: 1,
                },
            );
            write_frame(
                stdout,
                &Frame::Block(Block {
                    block_type: BlockType::Paragraph,
                    reading_order: 0,
                    language: None,
                    text: "блок без сторінки".to_string(),
                    line_start: Some(1),
                    line_end: Some(1),
                }),
            );
            // A complete document apart from the missing page marker, so that
            // a supervisor which accepted the block answers promptly with a
            // wrong document rather than sitting on the deadline. The test
            // then fails on its assertion instead of on a watchdog.
            write_frame(
                stdout,
                &Frame::Summary {
                    skipped_pages: 0,
                    text_source: "native:txt".to_string(),
                },
            );
        }
        // A line that is not a frame at all: what a worker binary from a
        // different release would look like.
        "garbage" => {
            println!("this is not a frame");
            let _ = stdout.flush();
        }
        // The same, and then a flood the parent will never read, because it
        // gave up on the first line. Fills the pool's read-ahead channel while
        // the worker is being retired, which is the one arrangement under which
        // a supervisor that joins its reader thread too early wedges. The count
        // is far above the channel's bound; the child blocks on its own stdout
        // long before it finishes.
        "garbage-flood" => {
            println!("this is not a frame");
            for _ in 0..5000 {
                println!("nor is this");
            }
            let _ = stdout.flush();
        }
        _ => answer(stdout),
    }
}

/// The shape of a readable document: one header, one page, one block, one
/// summary.
fn answer(stdout: &mut io::Stdout) {
    write_frame(
        stdout,
        &Frame::Header {
            sha256: "0".repeat(64),
            mime: "text/plain".to_string(),
            source_kind: SourceKind::Document,
            reader: "text".to_string(),
            reader_version: 1,
            pages: 1,
        },
    );
    write_frame(
        stdout,
        &Frame::Page {
            page_no: 1,
            section_title: None,
        },
    );
    write_frame(
        stdout,
        &Frame::Block(Block {
            block_type: BlockType::Paragraph,
            reading_order: 0,
            language: None,
            text: "текст, який пройшов через процес".to_string(),
            line_start: Some(1),
            line_end: Some(1),
        }),
    );
    write_frame(
        stdout,
        &Frame::Summary {
            skipped_pages: 0,
            text_source: "native:txt".to_string(),
        },
    );
}

fn write_frame(stdout: &mut io::Stdout, frame: &Frame) {
    let line = to_line(frame).expect("a Frame always serialises");
    stdout
        .write_all(line.as_bytes())
        .and_then(|()| stdout.flush())
        .expect("the pool keeps reading stdout");
}

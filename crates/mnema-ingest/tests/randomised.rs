//! A randomised hunt for the one failure this crate exists to prevent: the
//! index quietly holding less than it should, or holding the wrong thing.
//!
//! `tests/slice.rs` beside this file asks *named* questions — one sequence per
//! defect, each written after the defect was understood. This file asks the
//! question nobody has thought of yet. It draws a random sequence of the things
//! a person does to a folder (edit, copy, rename, delete, save a format nothing
//! reads over the top, exclude a file by rule, eject the volume it lives on),
//! mixes in the three ways the machine breaks under a walk (a worker binary
//! that is not the worker, a deadline nothing can meet, a database write that
//! fails part-way), runs either `ingest_file` directly or a real `walk_root`
//! over the result, and after **every** step checks a set of properties that
//! must hold no matter what the sequence was.
//!
//! `walk_root` matters here for a reason no single-file call can stand in
//! for: it is the only code in this product that deletes — phase 3 removes a
//! `path` row once a *complete* enumeration has found no evidence the file
//! it names is still there (§7). Every other operation in this file offers
//! `ingest_file` one path at a time, which can repoint or skip a row but
//! never delete one outright; `RunWalk`, drawn like any other operation, is
//! what actually exercises that removal, and `SimulateEjectedVolume` is what
//! reaches the one guard against removing too much of it (D33).
//!
//! **The model is deliberately not a second implementation of `ingest_file`.**
//! Predicting the outcome of each call would mean re-deriving `displaces`, the
//! cheap arm and the rebuild here, and a harness that agrees with the code by
//! construction cannot disagree with it usefully. What is modelled instead is
//! only what the product cannot know and the test can: which bytes are at which
//! path, whether they have changed since the last time this path was offered to
//! `ingest_file`, and what that call answered. Every invariant below is then a
//! safety property phrased against the bytes on disk — never against a
//! predicted outcome.
//!
//! Deterministic end to end. A run is a pure function of its seed: the same
//! seed draws the same operations, writes the same bytes and sets the same
//! modification times, so a failure prints one number that reproduces it.
//!
//! What the default run covers, and how to run it longer, is on
//! [`random_sequences_do_not_lose_data`].
//!
//! Every fixture is invented — the words are ordinary Ukrainian nouns about
//! deliveries and nothing here belongs to anybody.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mnema_core::OnDisk;
use mnema_core::manifest::Manifest;
use mnema_index::{Db, SkipRule, open, register_vector_extension};
use mnema_ingest::{Ingested, StopReason, WalkReport, ingest_file, walk_root};
use mnema_pool::{Pool, PoolConfig};
use mnema_walk::WalkRules;
use sha2::{Digest, Sha256};

// --------------------------------------------------------------- the worker
//
// The same derivation as `tests/slice.rs::worker`, and duplicated rather than
// shared on purpose: cargo compiles each file in `tests/` as its own binary, so
// a shared `mod` would have to be introduced into `slice.rs` as well, and that
// file is a reviewed test suite this task has no business editing. The comment
// there explains why the path cannot come from `CARGO_BIN_EXE_*`.

fn worker() -> &'static Path {
    static WORKER: OnceLock<PathBuf> = OnceLock::new();
    WORKER.get_or_init(|| {
        let exe = std::env::current_exe().expect("a test binary knows its own path");
        let profile_dir = exe
            .parent()
            .and_then(Path::parent)
            .expect("a test binary sits in <target>/<profile>/deps");
        let target_dir = profile_dir
            .parent()
            .expect("<target>/<profile> sits inside <target>");
        let profile = profile_dir
            .file_name()
            .and_then(|name| name.to_str())
            .expect("the profile directory is named");
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("crates/mnema-ingest sits two levels below the workspace root");

        let mut cargo = Command::new(env!("CARGO"));
        cargo
            .args([
                "build",
                "-p",
                "mnema-extract",
                "--bin",
                "mnema-extract-worker",
            ])
            .arg("--manifest-path")
            .arg(workspace.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(target_dir);
        if profile != "debug" {
            cargo.args(["--profile", profile]);
        }
        let status = cargo.status().expect("cargo runs");
        assert!(
            status.success(),
            "the extraction worker did not build, so this whole file is unanswered \
             rather than passing"
        );

        let path = profile_dir.join(format!(
            "mnema-extract-worker{}",
            std::env::consts::EXE_SUFFIX
        ));
        assert!(
            path.exists(),
            "cargo reported success but {} is not there",
            path.display()
        );
        path
    })
}

/// A sidecar that is not the worker: it answers every request with bytes that
/// are not UTF-8, which is what a half-finished install or a mismatched release
/// looks like from the parent's side. `read_line` raises an `io::Error` on
/// them, the pool classifies it as `Failure::Crash`, and `SkipRule::Crash` is
/// the rule this harness must watch: it is the one that empties an index one
/// file at a time if it is ever put on the displacing side.
#[cfg(unix)]
fn babbling_worker(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("babbling-worker");
    std::fs::write(
        &path,
        "#!/bin/sh\nwhile read -r _line; do\nprintf '\\377\\376\\n'\ndone\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A worker whose content rule got stricter between releases: it calls every
/// file not text, whatever the bytes are.
///
/// It reports the file's **real** digest, and that is the whole fidelity of
/// this stand-in. A worker whose classifier changed still reads the bytes and
/// still hashes them before deciding (`worker.rs` takes the digest before
/// `identify`), so a sidecar that omitted it would be modelling an *older*
/// worker instead — a different failure, and one that would let this operation
/// pass for the wrong reason.
///
/// `$1` is not available: the pool sends a JSON request line, not an argument,
/// so the script reads the path out of the line with `sed` and hashes the file
/// itself. `shasum -a 256` is on every macOS and Linux runner this repository
/// targets; `sha256sum` is not on macOS.
#[cfg(unix)]
fn stricter_worker(dir: &Path, rule: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    // One script per rule, because two operations in one run must not race for
    // the same file — and because the name is what the trace shows.
    let path = dir.join(format!("stricter-worker-{rule}"));
    std::fs::write(
        &path,
        format!(
            r#"#!/bin/sh
while read -r line; do
  file=$(printf '%s' "$line" | sed -n 's/.*"path":"\([^"]*\)".*/\1/p')
  sha=$(shasum -a 256 "$file" 2>/dev/null | cut -d' ' -f1)
  printf '{{"frame":"refused","rule":"{rule}","reason":"the threshold moved","sha256":"%s"}}\n' "$sha"
done
"#
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A sidecar that **succeeds**: it reads the file and answers with a valid frame
/// stream, at whatever `reader_version` it is built with.
///
/// ⚠️ **This file assumes unix, and says so here because nothing else does.**
/// Every sidecar is a `/bin/sh` script behind `#[cfg(unix)]`, and since Task 14
/// the corpus assertion has *required* their results: `want_rules` contains
/// `"crash"`, which only `babbling_sidecar` produces, and `want_states`
/// contains three states only the sidecars below can reach. So on a platform
/// without them the assertion cannot pass at all — the harness does not
/// degrade there, it fails. D64 deepened that assumption rather than creating
/// it, and the honest thing is one sentence saying so rather than three
/// `#[cfg(not(unix))]` arms quietly making a broken corpus look green.
///
/// **This is the capability the harness did not have.** Every other sidecar here
/// refuses — `babbling_worker` is not a worker at all, `stricter_worker` answers
/// one refusal — and refusals can never reach the machinery this cycle added:
/// a reader version bump **rebuilds** a document instead of confirming it,
/// `ingest_stage.status` passes through `rebuilding`, `document.status` goes
/// back to `pending` so a document being written answers no search (D61), and an
/// interrupted rebuild is finished by the next walk rather than left as
/// `Unchanged`. Reaching any of that needs a worker that gets as far as writing.
///
/// **One page per non-empty line, rather than one page per file.** The real text
/// reader makes a single page of many blocks, and a single page cannot be cut
/// across transactions — `PAGES_PER_TRANSACTION` is 20, so a document has to
/// declare more than twenty *pages* before the write loop makes more than one
/// slice, and a failure between two slices is exactly the state invariant 2
/// exists for. A line per page is the cheapest honest shape that gets there.
///
/// Every marker the file holds is therefore still in some block, which is what
/// keeps `check_stored_is_findable` a real check over a rebuilt document rather
/// than a vacuous one.
///
/// ⚠️ **The frame shapes here were read off a run of the real binary, and the
/// first read was of a stale one.** `target/debug/mnema-extract-worker` was
/// three days old — `cargo clean -p` deletes it and not everything puts it back
/// — and its header carried no `reader` or `reader_version` at all, with
/// `skipped_pages` still a number rather than a list. A sidecar derived from
/// that would have been missing precisely the two fields this whole task turns
/// on, and would have looked like a product defect.
#[cfg(unix)]
fn better_reader_worker(dir: &Path, reader: &str, version: u32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(format!("better-reader-{reader}-{version}"));
    std::fs::write(
        &path,
        format!(
            r#"#!/bin/sh
while read -r line; do
  file=$(printf '%s' "$line" | sed -n 's/.*"path":"\([^"]*\)".*/\1/p')
  sha=$(shasum -a 256 "$file" 2>/dev/null | cut -d' ' -f1)
  pages=$(grep -c . "$file")
  printf '{{"frame":"header","sha256":"%s","mime":"text/plain","source_kind":"document","reader":"{reader}","reader_version":{version},"pages":%s}}
' "$sha" "$pages"
  n=0
  while IFS= read -r raw; do
    [ -z "$raw" ] && continue
    # The two characters a JSON string cannot carry raw. No body this
    # generator writes contains either today — which is exactly why it is worth
    # doing now rather than after one does and the frame stops parsing for a
    # reason that looks like a reader defect.
    text=$(printf '%s' "$raw" | sed 's/\\/\\\\/g; s/"/\\"/g')
    n=$((n+1))
    printf '{{"frame":"page","page_no":%s,"section_title":null}}
' "$n"
    printf '{{"frame":"block","block_type":"paragraph","reading_order":0,"language":null,"text":"%s","line_start":%s,"line_end":%s}}
' "$text" "$n" "$n"
  done < "$file"
  printf '{{"frame":"summary","skipped_pages":[],"text_source":"native:txt"}}
'
done
"#
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

// ------------------------------------------------------------------ the dice

/// splitmix64 — three lines, no dependency, and identical on every platform.
///
/// A generator is written out here rather than pulled in because the property
/// this file needs from it is reproducibility across machines and across
/// versions, which a dependency does not promise: `rand`'s `StdRng` is
/// explicitly allowed to change its stream between releases, and a seed printed
/// by a failing CI run would then not reproduce anything locally.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n`. `n` must not be zero.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

// ---------------------------------------------------------------- the corpus

/// The largest file the harness's pool will read.
///
/// Small on purpose. "A file grew past the ceiling" is one of the sequences
/// being drawn, and the alternative to lowering the ceiling is writing 64 MiB
/// of fixture — which would make the run about the filesystem rather than about
/// the index.
const CEILING: u64 = 8192;

/// The alphabet markers are spelled in.
///
/// Letters rather than digits, and ten of them, so that a marker is one FTS
/// token with a base-ten counter inside it. `search_lexical` quotes each term
/// as a phrase and does no prefix matching, so two markers never answer to each
/// other.
const ALPHABET: [char; 10] = ['а', 'б', 'в', 'г', 'д', 'е', 'ж', 'з', 'и', 'к'];

/// Every marker is this many letters wide, so that two bodies of the same shape
/// are the same number of bytes.
///
/// That is what makes an in-place edit of unchanged length possible, which is
/// the half of the cheap arm the modification time answers for. A
/// variable-width counter would silently turn every such edit into a
/// length-changing one and the size column would carry the whole comparison.
const MARKER_WIDTH: usize = 5;

/// A word that appears in exactly one version of one file, and can be searched
/// for.
///
/// Findability is checked marker by marker, and each page of a generated
/// document carries its own — so a document that is missing its later pages is
/// caught by the markers that are missing with them, without this file having
/// to model the extractor or the chunker.
fn marker(mut n: u64) -> String {
    let mut s = String::from("мітк");
    for _ in 0..MARKER_WIDTH {
        s.push(ALPHABET[(n % 10) as usize]);
        n /= 10;
    }
    s
}

/// Whether the corpus is expected to contain this rule, and why not when it is
/// not.
///
/// **Exhaustive on purpose: a new `SkipRule` variant fails to compile here.**
/// That is the half of the corpus assertion that a list cannot give — the list
/// says what was reached, this says what *ought* to be, and a rule added to the
/// product without a thought about the harness stops the build rather than
/// joining the set of things nothing measures.
fn must_the_corpus_reach(rule: SkipRule) -> bool {
    match rule {
        // The five refusals by content and the size ceiling: every one has a
        // `displaces` arm of its own, and an arm no generator reaches is an arm
        // judged by nothing.
        SkipRule::Unsupported
        | SkipRule::NoTextLayer
        | SkipRule::NotText
        | SkipRule::BinaryTail
        | SkipRule::Malformed
        | SkipRule::Encrypted
        | SkipRule::TooLarge => true,
        // The machine breaking rather than the file being refused. `Crash` and
        // `Timeout` are drawn by the two sidecar operations, and `Unreadable`
        // arrives from a file this crate's own `stat` cannot answer for — an
        // ejected volume, a path that went away under the walk.
        //
        // **`Unreadable` is here because the run said so, not because I did.**
        // I classified it as unmodelled and the very first pass of the
        // bidirectional assertion reddened on it: the corpus reaches it and my
        // claim about the corpus was wrong. That is the direction a
        // containment check could never have caught — it only ever asks
        // whether the listed things happened, never whether the happened
        // things were listed.
        SkipRule::Crash | SkipRule::Timeout | SkipRule::Unreadable => true,
        // The one rule with no generator here. Named rather than omitted, so
        // "not modelled" is a decision on the record instead of an absence
        // somebody has to notice: nothing in this file can make a worker run
        // out of memory, and a sidecar that merely *said* `memory` would be
        // testing the string rather than the condition.
        SkipRule::Memory => false,
    }
}

/// Every rule the product can journal, once each.
///
/// The same arrangement as [`Shape::EVERY_LABEL`] and the same admission: the
/// list is hand-written because Rust will not enumerate variants, and what
/// makes it safe is that `must_the_corpus_reach` above is exhaustive and the
/// corpus assertion compares sets rather than containment.
const EVERY_RULE: [SkipRule; 11] = [
    SkipRule::Crash,
    SkipRule::Timeout,
    SkipRule::Memory,
    SkipRule::Unsupported,
    SkipRule::NoTextLayer,
    SkipRule::Unreadable,
    SkipRule::TooLarge,
    SkipRule::NotText,
    SkipRule::BinaryTail,
    SkipRule::Malformed,
    SkipRule::Encrypted,
];

/// A zip of the given members, **stored rather than deflated**.
///
/// Storing is an invariant here, not a preference. `edit_keeping_length` is the
/// operation that leaves a file exactly as long as it was, so the size column
/// cannot see the edit and the modification time carries the whole of the cheap
/// arm's evidence — and [`MARKER_WIDTH`] exists so that two versions of one body
/// are the same number of bytes. Deflate breaks that: two markers of equal width
/// compress to different lengths, so every "edit keeping length" over a zip
/// format would silently become a length-changing one and the cheap arm's mtime
/// branch would go untested for four of the six formats. Stored, an archive's
/// size is a function of its member names and their lengths, and the property
/// holds again.
///
/// It costs nothing else: every archive this file writes is a few kilobytes, and
/// `zip_part::read_member` and calamine read stored members exactly as they read
/// deflated ones.
fn zip_of(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    zip_with_mimetype(None, members)
}

/// The same, with an optional uncompressed first entry — which is what
/// `typing::is_epub` requires before it will call anything an EPUB
/// (`crates/mnema-extract/src/typing.rs`): first entry named `mimetype`, stored,
/// holding exactly the media type.
fn zip_with_mimetype(mimetype: Option<&str>, members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    use std::io::{Cursor, Write};

    let mut buf = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let stored: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        if let Some(mimetype) = mimetype {
            w.start_file("mimetype", stored).unwrap();
            w.write_all(mimetype.as_bytes()).unwrap();
        }
        for (name, body) in members {
            w.start_file(*name, stored).unwrap();
            w.write_all(body).unwrap();
        }
        w.finish().unwrap();
    }
    buf.into_inner()
}

/// One entry of a generated book's spine.
///
/// **The degenerate variants are the point of this type, and they are here
/// because measurement said so, not symmetry.** Both defects Task 11 found were
/// on books that are *valid as structure and degenerate as content* — an
/// `<itemref/>` with no `idref` made the entry vanish from the spine and shifted
/// every later chapter up by one, and a manifest declaring `id=""` made a
/// "sensible" fix put one chapter's text under another's number. A truncated
/// archive produces neither. Corrupt bytes stay in this file because they are
/// cheap, not because they are coverage.
enum SpineEntry {
    /// Declared in the manifest, present in the archive, read as a page.
    Chapter(usize),
    /// `<itemref/>` with no `idref` at all: the entry the spine still counts and
    /// no manifest item answers.
    NoIdref,
    /// A manifest item declaring `id=""`, and a spine entry naming that empty
    /// id.
    EmptyId,
    /// Declared in both manifest and spine, and simply not in the archive — the
    /// broken internal link that must skip one chapter by number rather than
    /// refusing a whole book.
    MissingMember(usize),
    /// An ordinary chapter whose manifest entry states its media type **with a
    /// parameter** — `application/xhtml+xml; charset=utf-8`.
    ///
    /// A real producer writes this and the standard allows it, so a reader that
    /// compares the media type as a string instead of parsing it drops the
    /// chapter and skips a page nothing is wrong with. Measured: it indexes
    /// exactly like a bare media type, one page, which is why it can be a
    /// drop-in rather than needing a shape of its own.
    Parameterised(usize),
    /// A second spine entry pointing at a chapter **already in the spine**.
    ///
    /// Measured: the book comes back with **two pages holding the same text**,
    /// which is the honest reading — the spine really does say to show that
    /// chapter twice. What it exercises here is that neither page is lost and
    /// neither is confused with the other.
    Repeat(usize),
}

/// A whole EPUB: `mimetype`, container, package document and the chapters given.
///
/// The spine is written in the order handed in, so a book's `skipped_pages` can
/// be predicted from the entries: every non-`Chapter` entry is a page number the
/// reader will report as skipped, and its position in this slice is that number
/// minus one.
fn epub_of(spine: &[SpineEntry], chapters: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut manifest = String::new();
    let mut refs = String::new();
    for entry in spine {
        match entry {
            SpineEntry::Chapter(i) | SpineEntry::MissingMember(i) => {
                manifest.push_str(&format!(
                    "<item id=\"c{i}\" href=\"ch{i}.xhtml\" \
                     media-type=\"application/xhtml+xml\"/>"
                ));
                refs.push_str(&format!("<itemref idref=\"c{i}\"/>"));
            }
            SpineEntry::Parameterised(i) => {
                manifest.push_str(&format!(
                    "<item id=\"c{i}\" href=\"ch{i}.xhtml\" \
                     media-type=\"application/xhtml+xml; charset=utf-8\"/>"
                ));
                refs.push_str(&format!("<itemref idref=\"c{i}\"/>"));
            }
            // Only a second reference: the manifest item is written by the
            // `Chapter` entry this one repeats.
            SpineEntry::Repeat(i) => refs.push_str(&format!("<itemref idref=\"c{i}\"/>")),
            SpineEntry::NoIdref => refs.push_str("<itemref/>"),
            SpineEntry::EmptyId => {
                manifest.push_str(
                    "<item id=\"\" href=\"nowhere.xhtml\" \
                     media-type=\"application/xhtml+xml\"/>",
                );
                refs.push_str("<itemref idref=\"\"/>");
            }
        }
    }
    let opf = format!(
        "<package xmlns=\"http://www.idpf.org/2007/opf\"><manifest>{manifest}</manifest>\
         <spine>{refs}</spine></package>"
    );
    let container = "<container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
                     <rootfiles><rootfile full-path=\"content.opf\" \
                     media-type=\"application/oebps-package+xml\"/></rootfiles></container>";

    let mut members: Vec<(&str, Vec<u8>)> = vec![
        ("META-INF/container.xml", container.as_bytes().to_vec()),
        ("content.opf", opf.into_bytes()),
    ];
    for (name, body) in chapters {
        members.push((name.as_str(), body.clone()));
    }
    zip_with_mimetype(Some("application/epub+zip"), &members)
}

/// A whole DOCX around a `<w:body>`.
fn docx_of(body: &str) -> Vec<u8> {
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
         <w:body>{body}</w:body></w:document>"
    );
    zip_of(&[("word/document.xml", document.into_bytes())])
}

/// A whole XLSX of the sheets given, each `(name, rows)`.
///
/// A sheet whose rows are `None` is **declared by the workbook and absent from
/// the archive** — the spreadsheet twin of `SpineEntry::MissingMember`, and one
/// of the five measured ways a sheet fails while the rest of a workbook reads.
fn xlsx_of(sheets: &[(&str, Option<&str>)]) -> Vec<u8> {
    let declared: String = sheets
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            format!(
                "<sheet name=\"{name}\" sheetId=\"{}\" r:id=\"rId{}\"/>",
                i + 1,
                i + 1
            )
        })
        .collect();
    let relationships: String = (1..=sheets.len())
        .map(|i| {
            format!(
                "<Relationship Id=\"rId{i}\" Type=\"http://schemas.openxmlformats.org/\
                 officeDocument/2006/relationships/worksheet\" \
                 Target=\"worksheets/sheet{i}.xml\"/>"
            )
        })
        .collect();

    let mut members: Vec<(String, Vec<u8>)> = vec![
        (
            "_rels/.rels".to_string(),
            b"<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>".to_vec(),
        ),
        (
            "xl/workbook.xml".to_string(),
            format!(
                "<?xml version=\"1.0\"?><workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets>{declared}</sheets></workbook>"
            )
            .into_bytes(),
        ),
        (
            "xl/_rels/workbook.xml.rels".to_string(),
            format!(
                "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{relationships}</Relationships>"
            )
            .into_bytes(),
        ),
    ];
    for (i, (_, rows)) in sheets.iter().enumerate() {
        if let Some(rows) = rows {
            members.push((
                format!("xl/worksheets/sheet{}.xml", i + 1),
                format!(
                    "<?xml version=\"1.0\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>{rows}</sheetData></worksheet>"
                )
                .into_bytes(),
            ));
        }
    }
    zip_of(
        &members
            .iter()
            .map(|(n, b)| (n.as_str(), b.clone()))
            .collect::<Vec<_>>(),
    )
}

/// The formats that reach the index through a reader of their own.
///
/// One variant per reader that this generator can produce *readable* files for,
/// which is four of the five G7.1 formats. **PDF is deliberately absent and the
/// report says so as a gap rather than leaving it to be inferred:** a PDF
/// carrying a fresh marker per version would mean generating a content stream
/// and a font, and the checked-in fixtures carry fixed text — a marker that is
/// not unique to one version of one file breaks the property every findability
/// check in this file rests on. PDF is reachable here only through its
/// *refusals*, which is what [`Shape::Refused`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Html,
    Epub,
    Docx,
    Xlsx,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Format::Html => "html",
            Format::Epub => "epub",
            Format::Docx => "docx",
            Format::Xlsx => "xlsx",
        }
    }
}

/// A refusal this generator can produce on purpose, named by the rule the
/// worker answers with.
///
/// Every one of these was **measured against the worker binary** rather than
/// derived from the readers' source: `typing::identify` decides by content, so a
/// body written to earn one rule can perfectly well land in another reader's
/// branch. The run that fixed each of them is in the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// A document that opens and holds no words: an EPUB of plates, a DOCX of
    /// empty paragraphs, a workbook whose only sheet has no rows.
    NoTextLayer,
    /// Structure that does not parse: a `word/document.xml` cut mid-element, an
    /// EPUB with no container, a workbook with no package relationships.
    Malformed,
    /// A password-protected PDF.
    Encrypted,
}

impl Shape {
    /// The name this shape answers to in the corpus-coverage assertion.
    ///
    /// A `match` rather than `Debug`, for the reason `tests/manifest.rs` gives
    /// for reader names: a variant renamed in passing must not silently rename
    /// what the coverage list is looking for and turn the assertion into one
    /// that can never pass.
    /// Every label [`Shape::label`] can return, exactly once.
    ///
    /// **Hand-written, and the `match` below is what keeps it honest.** Rust
    /// cannot enumerate an enum's variants without a derive, so this list
    /// cannot be generated — what it can be is *checked*, and it is, twice
    /// over: adding a `Shape` variant fails to compile in `label`, and the
    /// corpus assertion compares this list to what a run reached with
    /// `assert_eq!` on sets rather than by containment. A new label that is
    /// generated and not listed fails; a listed label that stops being
    /// generated fails. Neither can go quiet, which is what the first version
    /// of this list did — it named twelve of the thirteen and `pages-skipped`
    /// was the one it missed.
    const EVERY_LABEL: [&'static str; 13] = [
        "text",
        "markdown",
        "html",
        "epub",
        "docx",
        "xlsx",
        "pages-skipped",
        "unsupported-container",
        "photo",
        "binary-tail",
        "no-text-layer",
        "malformed",
        "encrypted",
    ];

    fn label(self) -> &'static str {
        match self {
            Shape::Text(_) => "text",
            Shape::Markdown(_) => "markdown",
            Shape::Rich(Format::Html, _) => "html",
            Shape::Rich(Format::Epub, _) => "epub",
            Shape::Rich(Format::Docx, _) => "docx",
            Shape::Rich(Format::Xlsx, _) => "xlsx",
            Shape::Gappy(_, _) => "pages-skipped",
            Shape::Opaque => "unsupported-container",
            Shape::NotText => "photo",
            Shape::BinaryTail => "binary-tail",
            Shape::Refused(Refusal::NoTextLayer) => "no-text-layer",
            Shape::Refused(Refusal::Malformed) => "malformed",
            Shape::Refused(Refusal::Encrypted) => "encrypted",
        }
    }
}

/// What kind of thing is at a path, in the only detail this file needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// Paragraphs of prose. One page, one block each.
    Text(usize),
    /// Sections. One page each, so a count above `PAGES_PER_TRANSACTION` is a
    /// document the write loop cuts into more than one transaction.
    Markdown(usize),
    /// Bytes no reader can take — a PDF header, since there is no PDF reader.
    Opaque,
    /// Not text at all: a photo. Refused as `SkipRule::NotText`, and the one
    /// refusal in this file that **removes** what the path used to hold — the
    /// text the index has under this name belongs to a file that is gone.
    ///
    /// Separate from `Opaque` although both are refused and neither is ever
    /// indexed. `Opaque` is a PDF header: a format with no reader *yet*, which
    /// leaves the earlier document alone. This one says the file stopped being
    /// prose, and the earlier document has to go with it (D51).
    NotText,
    /// Prose followed by a zeroed tail — what an interrupted append leaves
    /// behind. Refused as `SkipRule::BinaryTail`, and the refusal that must
    /// **not** remove anything: the prose is still on disk in front of the
    /// damage, and it is readable nowhere else.
    BinaryTail,
    /// A file one of the four new readers takes, carrying one marker per page.
    ///
    /// Indexed like `Text` and `Markdown`, and separate from them only because
    /// the bytes are a container: what it costs to write, and what an edit of
    /// unchanged length has to preserve, are the archive's rules rather than
    /// prose's.
    Rich(Format, usize),
    /// A document one of the four readers takes, some of whose **declared pages
    /// it cannot read** — so the read succeeds, the readable pages are indexed,
    /// and the rest come back as numbers in `Frame::Summary.skipped_pages`.
    ///
    /// The count is the number of pages that *do* carry a marker; the gaps are
    /// extra. This is the class Task 9 added and this harness had never
    /// generated: per-page journal rows, written from those numbers, living in
    /// the same table as the file-level verdicts and cleared by a **separate**
    /// path (`Db::forget_page_skips`), because `forget_skip` deliberately leaves
    /// them alone.
    ///
    /// It is also the shape that carries Task 11's own defect class into the
    /// harness. A book whose spine holds an `<itemref/>` with no `idref` is
    /// valid as structure and degenerate as content — the entry counts as a page
    /// and answers to no manifest item — and that is the file on which one
    /// chapter's text was very nearly stored under another chapter's number.
    Gappy(Format, usize),
    /// A file that opens and is refused **by content**, under a rule this shape
    /// names.
    ///
    /// The whole reason the rule is carried rather than lumped into `Opaque`:
    /// `displaces` gives `NoTextLayer`, `Malformed` and `Encrypted` a decision
    /// each about whether an already-indexed document survives, and a harness
    /// that models "refused" without modelling *which* refusal cannot tell a
    /// rule that moved to the wrong side of that table from one that did not.
    /// That is the same blindness that left `Unsupported` with no generator at
    /// all while every seed stayed green.
    Refused(Refusal),
}

/// Bytes and the words that must be findable in them once they are indexed.
struct Content {
    bytes: Vec<u8>,
    markers: Vec<String>,
    shape: Shape,
}

/// One version of one file, kept so that it can be put back exactly as it was
/// — bytes **and** the modification time that went with them.
///
/// The modification time is the whole reason this type exists. Every other
/// operation in this file writes with [`World::next_tick`], a clock that only
/// ever goes forward, so no sequence it can draw ever returns a path to a
/// `(size, mtime)` pair the index or the journal has already seen. That pair is
/// exactly what `ingest_file`'s two cheap arms key on, so a whole region of the
/// product's behaviour was unreachable by construction rather than by
/// coincidence — see [`World::restore_a_previous_version`].
#[derive(Clone)]
struct Version {
    bytes: Vec<u8>,
    markers: Vec<String>,
    shape: Shape,
    mtime: SystemTime,
    /// [`FileState::refused_by_content`] as it stood for **this** version.
    ///
    /// It travels with the version because the journal row it describes is
    /// keyed on the version's own `(size, mtime)`: put those bytes back at that
    /// time and the row matches again, so the flag is true again. Restoring
    /// without it left `settle` expecting the index to hold a file the second
    /// cheap arm will refuse for the life of the index — a real, documented
    /// price of D51 reported as a defect.
    ///
    /// Conservative in one direction on purpose: a successful index of this
    /// path since then removes the row (`repoint`), and this flag does not
    /// learn that. The cost is `settle` declining to check a path it could
    /// have; the opposite error would be a failure the product did not cause.
    refused_by_content: bool,
}

/// What the harness believes is at a path, which is only what the product
/// cannot know: the shape it wrote, the words it put there, and the
/// modification time it chose.
struct FileState {
    shape: Shape,
    markers: Vec<String>,
    mtime: SystemTime,
    /// What this path held before its current content, if anything — kept so
    /// that a restore can put it back with its own modification time.
    ///
    /// One version deep, which is what "restore the previous version" means and
    /// is enough for the sequences that matter: a refusal, an edit over it, and
    /// the refusal put back. Restoring twice toggles between the two, because
    /// the restore itself is an ordinary write and files the version it
    /// replaced.
    previous: Option<Box<Version>>,
    /// A content rule refused this path, and the journal still remembers it
    /// against the file's current size and mtime.
    ///
    /// While that is true and nothing under this path is indexed, the file does
    /// not come back no matter how many clean walks run over it: `ingest_file`'s
    /// **second** cheap arm answers from the skip journal, and no worker is
    /// asked again. That was D51's accepted price in its original form, and two
    /// changes have since narrowed it — this comment claimed both were
    /// impossible until the review that found them:
    ///
    /// * the arm now declines to answer for a document the rule would remove,
    ///   so a live `path` row under this name sends the file to a worker after
    ///   all (`mnema_ingest`'s second cheap arm, and `displaces` behind it);
    /// * a successful index of this path clears the row outright (`repoint`),
    ///   so `INDEX_FORMAT_VERSION` is no longer the only lever — an ordinary
    ///   walk moves it.
    ///
    /// Cleared here by anything that moves the file's size or modification
    /// time, because the journal row is keyed on those. That is narrower than
    /// what actually clears the verdict, and deliberately so: this flag only
    /// ever suppresses a check, so believing the refusal outlives it costs a
    /// check `settle` could have made, while the opposite error would be a
    /// failure the product did not cause. `Version::refused_by_content` above
    /// records the same asymmetry from the other side.
    refused_by_content: bool,
}

/// What the last call to `ingest_file` for a path answered, and what the file
/// hashed to at that moment.
///
/// Both halves are load-bearing. The index is allowed to be behind the disk
/// when nothing has offered it the new bytes yet, and it is allowed to be
/// behind when the offer was refused — and in no other case. `hash` is what
/// distinguishes the first, `verdict` the second.
struct LastCall {
    hash: Option<String>,
    verdict: Verdict,
    /// The call **read the file and wrote a document** — `Ingested::Indexed`,
    /// not the two cheap arms beside it.
    ///
    /// `Verdict::Settled` folds all three together, which is right for every
    /// other invariant here and wrong for the per-page journal rows: `Unchanged`
    /// and `AlreadyIndexed` never open the file, so rows about its pages are
    /// still true, while a fresh index is exactly the pass after which a page
    /// that has text again must have no row saying it has none.
    indexed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// `Indexed`, `Unchanged` or `AlreadyIndexed` — the index accepted this
    /// path and says it is up to date with it.
    Settled,
    /// Journalled and stepped over, under the rule that fired.
    ///
    /// The rule is carried because two of them owe the index opposite answers
    /// about what it may keep, and without it every invariant here reads
    /// "refused" as blanket permission for the index to be behind — which is
    /// true for `Crash` and false for `NotText`. Invariant 3c is the one that
    /// needs to tell them apart.
    Skipped(SkipRule),
    /// The database refused a write and the job would have stopped.
    Failed,
    /// No call to `ingest_file` produced this — the harness wrote it down
    /// itself, to say "this path's freshness is not something I assert".
    ///
    /// Two sites need it, and both used to write `Skipped`, which was
    /// harmless only while `Skipped` carried nothing: settling records what the
    /// index ended up holding without offering the file again, and
    /// `toggle_exclusion` marks a newly excluded path so that invariant 2b does
    /// not read a rule's own removal as data loss. Neither is a rule firing,
    /// and invariant 3c must not treat either as one — a synthesised rule there
    /// would be the harness asserting a fact it invented.
    Unoffered,
}

// ----------------------------------------------------------------- the world

struct World {
    db: Db,
    pool: Pool,
    /// What the real worker says its readers are, asked once for the run.
    ///
    /// The same manifest for every call, including the ones that go through a
    /// second pool built over a sidecar: the parent's idea of this build's
    /// readers comes from the product's own binary, and a harness that handed
    /// each pool its own manifest would quietly make the two agree in exactly
    /// the case where they must not.
    manifest: Manifest,
    root_id: i64,
    root: PathBuf,
    dir: tempfile::TempDir,
    rng: Rng,
    seed: u64,
    /// Bumped for every marker and every filename, so both are unique within a
    /// run and identical between two runs of the same seed.
    counter: u64,
    /// Logical time. Modification times are chosen, never taken from the wall
    /// clock: an assertion about whether the index noticed a change must not
    /// depend on where a second boundary fell between two writes.
    tick: u64,
    files: BTreeMap<String, FileState>,
    last: BTreeMap<String, LastCall>,
    /// The path the most recent call to `ingest_file` was about.
    ///
    /// Invariant 3c needs it and no other check does. `last` remembers every
    /// path this run has ever offered, and judging a rule's contract against
    /// all of them re-judges verdicts recorded many steps ago, against a
    /// `before`/`after` pair belonging to some unrelated call. Measured on seed
    /// 1592590556: a file refused as `binary_tail` at step 17 was legitimately
    /// removed by phase 3 at step 24, and invariant 3c read that as the
    /// refusal having deleted it.
    calling: Option<String>,
    log: Vec<String>,
    trace: bool,
    /// Every content hash this run has ever seen finished — kept so that a
    /// failure can say whether the document in front of it is new or is a
    /// content hash the index has indexed, dropped, and met again.
    settled_before: BTreeSet<String>,
    /// The rules `RunWalk` hands to `walk_root`, rebuilt from `excluded`
    /// every time it changes. Exactly one layer is ever exercised — a
    /// well-formed user prefix — because the built-in list and `.gitignore`
    /// are fixed decisions this crate does not draw at random; what a walk
    /// disagreeing with itself about is the one the user can change between
    /// two walks of the same folder.
    rules: WalkRules,
    /// Exact relative paths currently excluded by `rules` — a whole path
    /// rather than a folder, so excluding one file never reaches into
    /// whatever else its directory holds. `WalkRules` matches a prefix
    /// exactly the same way whether it names a file or a directory
    /// (`crates/mnema-walk/src/rules.rs`'s own `anchored_pattern`), so a
    /// leaf path is a well-formed rule in its own right.
    excluded: BTreeSet<String>,
    /// True only for the `check(...)` call that immediately follows a
    /// `walk_root` call inside `run_walk`/`simulate_ejected_volume`, false
    /// everywhere else. Invariant 3 reads it to excuse a row phase 3 itself
    /// just removed for an excluded path — see that invariant's own doc
    /// comment (Task 13, fix round 1) for why the exception has to be this
    /// narrow: outside a walk, nothing may remove an excluded path's row at
    /// all (§5 — exclusion takes effect on the *next* walk, not before), so
    /// widening this to every step would forgive a cascade this invariant
    /// exists to catch.
    walking: bool,
    /// What this run's generator actually produced — see [`Reached`].
    reached: Reached,
    /// Which content rule the next stricter-rule walk answers with. See
    /// `stricter_rule_over_an_unchanged_folder` for why it rotates.
    stricter_rotation: usize,
    /// Which reader version the next rebuild announces. Separate from
    /// `stricter_rotation` on purpose — see `the_build_learned_to_read_better`.
    rebuild_rotation: usize,
}

impl World {
    fn new(seed: u64) -> Self {
        register_vector_extension().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("watched");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::create_dir_all(root.join("backup")).unwrap();
        let db = open(&dir.path().join("index.sqlite")).unwrap();
        let root_id = db
            .insert_watched_root(root.to_str().expect("a temp path is UTF-8"))
            .unwrap();
        let pool = Pool::new(Self::config()).unwrap();
        let manifest = pool.manifest().unwrap();
        World {
            db,
            pool,
            manifest,
            root_id,
            root,
            dir,
            rng: Rng::new(seed),
            seed,
            counter: 0,
            tick: 0,
            files: BTreeMap::new(),
            last: BTreeMap::new(),
            calling: None,
            log: Vec::new(),
            trace: std::env::var("MNEMA_FUZZ_TRACE").is_ok(),
            settled_before: BTreeSet::new(),
            rules: WalkRules::none(),
            excluded: BTreeSet::new(),
            walking: false,
            reached: Reached::default(),
            // Started from the seed rather than at zero, so twelve runs begin
            // the rotation at four different places. Starting every run at
            // `not_text` meant the fourth rule was reached only by a run that
            // drew this operation four times — measured, `encrypted` stayed
            // unreached across the whole default corpus while the other three
            // did not.
            stricter_rotation: (seed % 4) as usize,
            rebuild_rotation: (seed % 3) as usize,
        }
    }

    /// Rebuilds `rules` from `excluded` after every change to it — cheap, and
    /// what keeps the two from drifting apart the way a cache that is
    /// updated by hand eventually does.
    fn rebuild_rules(&mut self) {
        self.rules = WalkRules::new(false, false, self.excluded.iter().cloned().collect()).expect(
            "every excluded path here is a plain relative path this file generated itself, \
                 which `WalkRules::new` always accepts",
        );
    }

    /// The pool every ordinary step goes through. One worker, because the
    /// harness is single-threaded and a second one only makes the trace harder
    /// to read; a ten-second deadline, because a plain text file that takes
    /// longer means something is wrong and a test that fails beats one that
    /// waits.
    fn config() -> PoolConfig {
        PoolConfig {
            workers: 1,
            batch: 100,
            timeout: Duration::from_secs(10),
            max_bytes: CEILING,
            ..PoolConfig::new(worker())
        }
    }

    // ------------------------------------------------------------ the disk

    fn absolute(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn next_tick(&mut self) -> SystemTime {
        self.tick += 1;
        // A quarter of a second apart: inside one second, so a writer that
        // truncated modification times to whole seconds could not tell two
        // consecutive ticks apart, and coarse enough for any filesystem with
        // sub-second timestamps to keep them apart.
        UNIX_EPOCH + Duration::new(1_700_000_000, 0) + Duration::from_millis(250 * self.tick)
    }

    fn write_at(&mut self, relative: &str, content: Content, at: SystemTime) {
        // What is being written over, read off the disk rather than off the
        // model, so a restore puts back bytes that were really there.
        let previous = self.on_disk(relative).and_then(|bytes| {
            let state = self.files.get(relative)?;
            Some(Box::new(Version {
                bytes,
                markers: state.markers.clone(),
                shape: state.shape,
                mtime: state.mtime,
                refused_by_content: state.refused_by_content,
            }))
        });
        self.reached.shapes.insert(content.shape.label());
        let path = self.absolute(relative);
        std::fs::write(&path, &content.bytes).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(at)
            .unwrap();
        self.files.insert(
            relative.to_string(),
            FileState {
                shape: content.shape,
                markers: content.markers,
                mtime: at,
                previous,
                // Whatever the journal remembers about this path was recorded
                // against some `(size, mtime)` pair, and the question is only
                // ever whether *this* write lands back on it. Every caller but
                // one writes at a tick nothing has used before, so the answer
                // is no; `restore_a_previous_version` is the exception and sets
                // this itself, from the version it put back.
                refused_by_content: false,
            },
        );
    }

    /// The bytes at a path as the disk has them, never as the model remembers
    /// them. Every invariant is phrased against this rather than against the
    /// harness's own bookkeeping, so a harness that lost track of a write does
    /// not quietly weaken a check.
    fn on_disk(&self, relative: &str) -> Option<Vec<u8>> {
        std::fs::read(self.absolute(relative)).ok()
    }

    /// `document.id` is the sha256 of the file's bytes
    /// (`crates/mnema-extract/src/bin/worker.rs:126-128`), so this is what the
    /// index must be holding under a path for its answer to be the file's own
    /// text.
    fn hash_on_disk(&self, relative: &str) -> Option<String> {
        self.on_disk(relative).map(|bytes| {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            hasher
                .finalize()
                .iter()
                .fold(String::with_capacity(64), |mut s, b| {
                    let _ = write!(s, "{b:02x}");
                    s
                })
        })
    }

    // --------------------------------------------------------- the fixtures

    fn next_counter(&mut self) -> u64 {
        self.counter += 1;
        self.counter
    }

    fn text_body(&mut self, paragraphs: usize) -> Content {
        let mut markers = Vec::with_capacity(paragraphs);
        let mut s = String::new();
        for _ in 0..paragraphs {
            let m = marker(self.next_counter());
            s.push_str("Сторона ");
            s.push_str(&m);
            s.push_str(" погодила строки приймання робіт за цим етапом.\n\n");
            markers.push(m);
        }
        Content {
            bytes: s.into_bytes(),
            markers,
            shape: Shape::Text(paragraphs),
        }
    }

    fn markdown_body(&mut self, sections: usize) -> Content {
        let mut markers = Vec::with_capacity(sections);
        let mut s = String::new();
        for _ in 0..sections {
            let m = marker(self.next_counter());
            // The heading is consumed as the page's `section_title` and is not
            // a block, so the marker goes in the body underneath it — a marker
            // that only ever appeared in a heading would be unfindable for a
            // reason that has nothing to do with data loss.
            s.push_str("# Розділ постачання\n\nПоложення ");
            s.push_str(&m);
            s.push_str(" про строки приймання робіт.\n\n");
            markers.push(m);
        }
        Content {
            bytes: s.into_bytes(),
            markers,
            shape: Shape::Markdown(sections),
        }
    }

    /// One page's worth of prose carrying its own marker, in whichever markup
    /// the format wants.
    ///
    /// The marker never goes in a heading alone. A heading is consumed as the
    /// page's `section_title` by markdown, html, epub and docx alike, so a
    /// marker that only ever appeared there would be unfindable for a reason
    /// that has nothing to do with data loss — the trap `markdown_body` already
    /// names, repeated here because four more readers now walk into it.
    fn rich_body(&mut self, format: Format, pages: usize) -> Content {
        let mut markers = Vec::with_capacity(pages);
        let mut parts: Vec<String> = Vec::with_capacity(pages);
        for _ in 0..pages {
            let m = marker(self.next_counter());
            parts.push(m.clone());
            markers.push(m);
        }

        let bytes = match format {
            Format::Html => {
                let mut s =
                    String::from("<!DOCTYPE html><html><head><title>Звіт</title></head><body>");
                for m in &parts {
                    s.push_str("<h1>Розділ постачання</h1><p>Положення ");
                    s.push_str(m);
                    s.push_str(" про строки приймання робіт.</p>");
                }
                s.push_str("</body></html>");
                s.into_bytes()
            }
            Format::Epub => {
                // One chapter per page, because a chapter *is* a page here — and
                // that is what makes a missing one nameable in `skipped_pages`.
                let chapters: Vec<(String, Vec<u8>)> = parts
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        (
                            format!("ch{i}.xhtml"),
                            format!(
                                "<html><head><title>Розділ {i}</title></head><body><p>Положення \
                                 {m} про строки приймання робіт.</p></body></html>"
                            )
                            .into_bytes(),
                        )
                    })
                    .collect();
                // Chapter 0 always states its media type **with a parameter**,
                // deterministically rather than by dice: it indexes identically
                // (measured), so it costs nothing and it means every ordinary
                // book in the corpus carries the case a reader comparing media
                // types as strings would drop. Deterministic matters —
                // `edit_keeping_length` needs one shape and unit count to
                // produce one byte count.
                let spine: Vec<SpineEntry> = (0..pages)
                    .map(|i| {
                        if i == 0 {
                            SpineEntry::Parameterised(i)
                        } else {
                            SpineEntry::Chapter(i)
                        }
                    })
                    .collect();
                epub_of(&spine, &chapters)
            }
            Format::Docx => {
                let mut body = String::new();
                for m in &parts {
                    body.push_str(
                        "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr>\
                         <w:r><w:t>Розділ постачання</w:t></w:r></w:p>",
                    );
                    body.push_str("<w:p><w:r><w:t>Положення ");
                    body.push_str(m);
                    body.push_str(" про строки приймання робіт.</w:t></w:r></w:p>");
                }
                docx_of(&body)
            }
            Format::Xlsx => {
                // One sheet per page and one row on it, so `pages` means the
                // same thing for this format as for the other three.
                let sheets: Vec<(String, String)> = parts
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        (
                            format!("Аркуш{i}"),
                            format!(
                                "<row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>Положення {m} \
                                 про строки</t></is></c></row>"
                            ),
                        )
                    })
                    .collect();
                xlsx_of(
                    &sheets
                        .iter()
                        .map(|(n, r)| (n.as_str(), Some(r.as_str())))
                        .collect::<Vec<_>>(),
                )
            }
        };

        Content {
            bytes,
            markers,
            shape: Shape::Rich(format, pages),
        }
    }

    /// A document whose reader can read some of its declared pages and not the
    /// rest.
    ///
    /// Two formats reach it, and both were measured against the worker before
    /// they were written down. A book gets a spine entry that is **valid and
    /// degenerate** — `<itemref/>` with no `idref`, a manifest item declaring
    /// `id=""`, or an entry naming a member the archive does not hold — and a
    /// workbook gets a sheet the workbook declares and the archive does not
    /// hold. Neither is corrupt bytes, and that is the point: an archive cut in
    /// half produces this class not at all.
    ///
    /// The gap goes **first**, so the readable pages carry numbers above it and
    /// a reader that quietly renumbered what came back — the exact defect Task
    /// 11 found — puts its markers at the wrong page numbers.
    fn gappy_body(&mut self, format: Format, pages: usize) -> Content {
        let mut markers = Vec::with_capacity(pages);
        for _ in 0..pages {
            markers.push(marker(self.next_counter()));
        }

        let bytes = match format {
            Format::Epub => {
                let gap = match self.rng.below(3) {
                    0 => SpineEntry::NoIdref,
                    1 => SpineEntry::EmptyId,
                    _ => SpineEntry::MissingMember(900),
                };
                let mut spine = vec![gap];
                let mut chapters = Vec::with_capacity(pages);
                for (i, m) in markers.iter().enumerate() {
                    spine.push(SpineEntry::Chapter(i));
                    chapters.push((
                        format!("ch{i}.xhtml"),
                        format!(
                            "<html><head><title>Розділ {i}</title></head><body><p>Положення \
                             {m} про строки приймання робіт.</p></body></html>"
                        )
                        .into_bytes(),
                    ));
                }
                // **The same member named twice, at the end.** Deterministic
                // rather than drawn, so the page layout invariant 3e checks
                // stays knowable: page 1 is the gap, pages 2..=n+1 are the
                // chapters in order, and the last page is chapter 0 over again.
                // A spine really is allowed to say "show that chapter here too".
                spine.push(SpineEntry::Repeat(0));
                epub_of(&spine, &chapters)
            }
            // Every other format falls back to a workbook, because a sheet the
            // workbook declares and the archive does not hold is the only other
            // measured way to skip a page by number in this build.
            _ => {
                let mut sheets: Vec<(String, Option<String>)> = vec![("Немає".to_string(), None)];
                for (i, m) in markers.iter().enumerate() {
                    sheets.push((
                        format!("Аркуш{i}"),
                        Some(format!(
                            "<row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>Положення {m} \
                             про строки</t></is></c></row>"
                        )),
                    ));
                }
                xlsx_of(
                    &sheets
                        .iter()
                        .map(|(n, r)| (n.as_str(), r.as_deref()))
                        .collect::<Vec<_>>(),
                )
            }
        };

        Content {
            bytes,
            markers,
            shape: Shape::Gappy(format, pages),
        }
    }

    /// A file that opens and is refused by content, under the rule asked for.
    ///
    /// Every body here was checked against the worker binary before it was
    /// written down — see the report for the run. Two of the three carry no
    /// markers for the reason `not_text_body` gives: nothing here is ever
    /// indexed, and claiming a marker would make a refused file look indexed.
    fn refused_body(&mut self, refusal: Refusal) -> Content {
        let bytes = match refusal {
            // Three formats reach this rule and the generator rotates through
            // them, because they are three different branches of three readers
            // answering one rule — a book of plates, a document of empty
            // paragraphs, a workbook whose sheet has no rows.
            Refusal::NoTextLayer => match self.rng.below(3) {
                0 => epub_of(
                    &[SpineEntry::Chapter(0)],
                    &[(
                        "ch0.xhtml".to_string(),
                        b"<html><body><img src=\"plate.png\"/></body></html>".to_vec(),
                    )],
                ),
                1 => docx_of("<w:p/><w:p><w:pPr/></w:p>"),
                _ => xlsx_of(&[("Порожній", Some(""))]),
            },
            Refusal::Malformed => match self.rng.below(3) {
                // A `word/document.xml` that stops inside an element.
                0 => docx_of("<w:p><w:r><w:t>початок"),
                // An EPUB whose `mimetype` is right and whose container is not
                // there at all.
                1 => zip_with_mimetype(
                    Some("application/epub+zip"),
                    &[("ch0.xhtml", "<p>розділ</p>".as_bytes().to_vec())],
                ),
                // A workbook with `xl/workbook.xml` and no package
                // relationships, so calamine cannot find where the workbook is.
                _ => zip_of(&[(
                    "xl/workbook.xml",
                    b"<workbook><sheets/></workbook>".to_vec(),
                )]),
            },
            // The one refusal here that is a checked-in fixture rather than
            // generated bytes: a password-protected PDF is an encrypted
            // document, and encryption is not something this file can synthesise
            // without becoming a PDF writer. 1 029 bytes, well under `CEILING`.
            //
            // **It reaches across a crate boundary into another crate's test
            // fixtures, and that is a deliberate trade rather than an
            // oversight.** Copying the file here would put a second binary blob
            // in the repository whose only relationship to the first is that
            // somebody remembered to update both; `include_bytes!` binds them,
            // and it binds them at **compile time** — moving or deleting the
            // fixture fails the build with the path in the message rather than
            // making this generator quietly produce something else. A test
            // layout coupled loudly beats two fixtures drifting quietly.
            Refusal::Encrypted => {
                include_bytes!("../../mnema-extract/tests/fixtures/password-locked.pdf").to_vec()
            }
        };
        Content {
            bytes,
            markers: Vec::new(),
            shape: Shape::Refused(refusal),
        }
    }

    /// A zip holding nothing any reader recognises: the one shape in this file
    /// that still earns `Unsupported`.
    ///
    /// **This used to be a `%PDF-` stub, and the note left here by Task 8 was
    /// right.** Those bytes meant "a format with no reader" only while there was
    /// no PDF reader; once there was one, pdfium was handed a truncated document
    /// and said so, and the verdict became `malformed`. Nothing went red,
    /// because invariant 3c asked only that a refused file stay out of the index
    /// and not which rule refused it — so `Unsupported`, a rule with a
    /// `displaces` decision of its own, was left with **no generator at all**
    /// while the harness stayed green.
    ///
    /// The replacement is measured against the binary rather than reasoned from
    /// `typing.rs`: a zip whose members are none of `word/document.xml`,
    /// `xl/workbook.xml` or an epub `mimetype` reaches `Reader::Unrecognized`,
    /// and the worker answers `unsupported` — "no reader implemented yet for
    /// application/zip". That is the sentence this shape is supposed to model,
    /// and now does.
    ///
    /// It is also the honest one to keep modelling: `Unsupported` is the rule a
    /// *release* changes without the file changing, which is exactly why
    /// `displaces` made it conditional (`crates/mnema-ingest/src/lib.rs`), and a
    /// container format nobody has written a reader for is the ordinary way a
    /// user meets it.
    fn opaque_body(&self) -> Content {
        Content {
            bytes: zip_of(&[("readme.nfo", b"nothing any reader here knows".to_vec())]),
            markers: Vec::new(),
            shape: Shape::Opaque,
        }
    }

    /// A photo — the same synthetic solid-colour PNG the deterministic tests
    /// use, invented outright by `make_fixtures.py`. Its first NUL sits at
    /// offset 8, well inside the head window, so it is binary from the start.
    ///
    /// No markers: nothing here is ever indexed, and that is the point.
    fn not_text_body(&self) -> Content {
        Content {
            bytes: include_bytes!("../../mnema-extract/tests/fixtures/solid.png").to_vec(),
            markers: Vec::new(),
            shape: Shape::NotText,
        }
    }

    /// Prose long enough to outrun the head window, then zeros: a note whose
    /// append the power cut short.
    ///
    /// The prose is deliberately the file's **existing** text where there is
    /// any, because that is what makes the case what it is — the index's copy
    /// is still the opening of the file on disk, and deleting it would lose
    /// text the disk still has.
    ///
    /// No markers, for the same reason as `not_text_body` and with a sharper
    /// edge: the words in this file are not newly findable, so claiming them
    /// would make a refused file look indexed. Whatever the earlier document
    /// already made findable stays findable through its own path row, which is
    /// exactly the property this shape exists to test.
    fn interrupted_append_body(&mut self, keeping: Option<Vec<u8>>) -> Content {
        let mut bytes = match keeping {
            Some(prose) if prose.len() > 512 => prose,
            // Nothing usable under the path (it was a photo, or too short to
            // clear the head window), so the shape has to bring its own prose.
            _ => self.text_body(6).bytes,
        };
        bytes.extend_from_slice(&[0u8; 4096]);
        Content {
            bytes,
            markers: Vec::new(),
            shape: Shape::BinaryTail,
        }
    }

    /// Fresh readable content of the kind the extension implies, of a given
    /// size in pages or paragraphs.
    ///
    /// **Keyed on the extension because the product is not.** `typing::identify`
    /// decides html by extension and the other four by content, so a `.docx`
    /// holding a workbook is read as a workbook — and this function writing the
    /// format its own name promises is what keeps the harness's model of "what
    /// is at this path" true. Every operation that rewrites a file goes through
    /// here, so a path never changes format under itself except when an
    /// operation means it to.
    fn body_for(&mut self, relative: &str, units: usize) -> Content {
        match Self::format_of(relative) {
            Some(format) => self.rich_body(format, units),
            None if relative.ends_with(".md") => self.markdown_body(units),
            None => self.text_body(units),
        }
    }

    /// The extension a path already carries, so a copy or a rename keeps it.
    ///
    /// **Both of those used to re-derive it as "md, else txt", and it was a
    /// generator defect the moment a fifth extension existed.** A workbook
    /// copied to `backup/copy-7.txt` keeps its bytes and its `Shape`, so the
    /// model says `Rich(Xlsx, 1)` while `body_for` — which keys on the name —
    /// would rewrite it as prose. The harness caught it itself, on the first
    /// run, through `edit_keeping_length`'s own assertion that a shape's length
    /// is reproducible: 1 314 bytes against 242.
    ///
    /// It matters beyond that assertion, and this is the part worth keeping in
    /// mind: four of the five formats are identified by **content**, so a
    /// workbook called `.txt` is still read as a workbook — but html is
    /// identified by **extension** (`typing::identify_plain_text`), so renaming
    /// `page.html` to `page.txt` really does change how the product reads it.
    /// A generator that renames across formats is therefore modelling something
    /// real, and modelling it wrongly; keeping the extension is what makes the
    /// model true.
    fn extension_of(relative: &str) -> &str {
        // `rsplit_once`, not `rsplit().next()`: the latter always yields
        // something, so its `unwrap_or` was a fallback that could not run — and
        // for a path with no dot at all it returned the **whole path**, which
        // would have put a slash inside `backup/copy-{n}.{ext}`. No generated
        // path is dotless today, so it could not fire; a guard that cannot fire
        // reads as protection and is not any.
        match relative.rsplit_once('.') {
            Some((_, extension)) if !extension.contains('/') => extension,
            _ => "txt",
        }
    }

    /// Which of the four container readers a path's name asks for, if any.
    fn format_of(relative: &str) -> Option<Format> {
        [Format::Html, Format::Epub, Format::Docx, Format::Xlsx]
            .into_iter()
            .find(|format| relative.ends_with(format.extension()))
    }

    /// A number of units that keeps the file comfortably under the ceiling,
    /// and that crosses `PAGES_PER_TRANSACTION` often enough for the write
    /// loop's second slice to be reached.
    fn ordinary_units(&mut self, relative: &str) -> usize {
        // The multi-transaction draw is not markdown's alone any more: an EPUB
        // of 22 chapters and a workbook of 22 sheets cut the write loop in the
        // same place, and each does it through a different reader's page
        // numbering. A book is the sharper of the two — its page numbers come
        // from the spine rather than from what came back, so a document cut
        // across transactions is also one whose pages can have gaps in them.
        let many = matches!(
            Self::format_of(relative),
            Some(Format::Epub) | Some(Format::Xlsx)
        ) || relative.ends_with(".md");
        if many && self.rng.chance(30) {
            mnema_ingest::PAGES_PER_TRANSACTION + 2
        } else if Self::format_of(relative) == Some(Format::Html) && self.rng.chance(15) {
            // **Zero sections — a document that declares nothing.** Measured
            // rather than assumed, because the answer differs per format and
            // only one of them is a *new* class: an html page with an empty
            // body still indexes (one page, no marker), while an epub with an
            // empty spine is `malformed` and a docx or workbook with nothing in
            // it is `no_text_layer` — both already generated by
            // `refused_body`. So the class is reached here for the one format
            // where it is not another class in disguise.
            0
        } else {
            1 + self.rng.below(5)
        }
    }

    /// Enough units to go over [`CEILING`].
    ///
    /// Measured per format rather than guessed: a container carries its own
    /// overhead — an EPUB writes two structure members and one per chapter, a
    /// workbook four and one per sheet — so the same unit count crosses 8 KiB at
    /// very different places. `a_file_over_the_ceiling_is_really_over_it` holds
    /// these numbers to what they claim.
    fn oversized_units(relative: &str) -> usize {
        match Self::format_of(relative) {
            Some(Format::Html) => 60,
            Some(Format::Epub) => 26,
            Some(Format::Docx) => 45,
            Some(Format::Xlsx) => 22,
            None if relative.ends_with(".md") => 80,
            None => 100,
        }
    }

    // ---------------------------------------------------------- the calls

    /// One line of the trace a failure prints back.
    ///
    /// `MNEMA_FUZZ_TRACE=1` also puts it on stderr as it happens, which is the
    /// only way to see what a **passing** run did — and that matters more than
    /// it sounds: a generator that never reaches an interesting state passes
    /// beautifully, and the trace is what caught this one drawing faults
    /// against files the cheap arm was answering without writing anything.
    fn note(&mut self, line: String) {
        if self.trace {
            eprintln!("{line}");
        }
        self.log.push(line);
    }

    /// Files away what one call answered: the verdict the invariants reason
    /// about, and a line of trace a failure can be read back from.
    ///
    /// Returns that verdict, because an operation that wants to record
    /// something about its own call must read what the call answered rather
    /// than assume it — see `stricter_rule_over_an_unchanged_folder`, where
    /// assuming it disabled a check on 29 paths out of 200 seeds.
    fn record(
        &mut self,
        relative: &str,
        hash: Option<String>,
        outcome: Result<Ingested, mnema_ingest::IngestError>,
        how: &str,
    ) -> Verdict {
        let verdict = match &outcome {
            Ok(
                Ingested::Indexed { .. }
                | Ingested::Unchanged { .. }
                | Ingested::AlreadyIndexed { .. },
            ) => Verdict::Settled,
            Ok(Ingested::Skipped { rule }) => Verdict::Skipped(*rule),
            Err(_) => Verdict::Failed,
        };
        let rendered = match &outcome {
            Ok(Ingested::Indexed { chunks, .. }) => format!("Indexed({chunks} chunks)"),
            Ok(Ingested::Unchanged { .. }) => "Unchanged".to_string(),
            Ok(Ingested::AlreadyIndexed { .. }) => "AlreadyIndexed".to_string(),
            Ok(Ingested::Skipped { rule }) => format!("Skipped({rule:?})"),
            Err(e) => format!("Err({e})"),
        };
        self.note(format!("    ingest{how} {relative} -> {rendered}"));
        match verdict {
            Verdict::Skipped(rule) => {
                self.reached.rules.insert(rule.as_str());
            }
            Verdict::Settled => {
                self.record_reader_of(relative);
                // Scoped to this run's root, which the first version was not:
                // every `World` has a database of its own today, so an unscoped
                // count answered the same number — but it was a claim about
                // every root rather than about this one, and the next test to
                // put two roots in one database would have inherited it.
                let rows: i64 = self
                    .db
                    .conn()
                    .query_row(
                        "SELECT count(*) FROM skipped WHERE watched_root_id = ?1 \
                         AND page_no IS NOT NULL",
                        [self.root_id],
                        |r| r.get(0),
                    )
                    .unwrap();
                self.reached.page_skips += rows as usize;
            }
            Verdict::Failed | Verdict::Unoffered => {}
        }
        self.calling = Some(relative.to_string());
        self.last.insert(
            relative.to_string(),
            LastCall {
                hash,
                verdict,
                indexed: matches!(outcome, Ok(Ingested::Indexed { .. })),
            },
        );
        verdict
    }

    fn ingest(&mut self, relative: &str) {
        self.ingest_labelled(relative, "");
    }

    fn ingest_labelled(&mut self, relative: &str, how: &str) {
        // The harness is modelling the disk here, exactly the role the walk
        // plays in production — so it is `mnema_walk::stat`, not a second
        // reading of its own, that it hands to `ingest_file` (§5). Taken
        // immediately before the call, which is the most forgiving shape a
        // caller can have: `walk` below is the one that stats first and
        // ingests afterwards, which is the window a real walk actually
        // opens.
        let on_disk = mnema_walk::stat(&self.absolute(relative));
        self.ingest_measured(relative, on_disk, how);
    }

    /// The same call, with the measurement taken by the caller rather than
    /// just before this call — see `walk`, the one caller that needs the gap
    /// between the two to be real.
    ///
    /// **The first of the two narrow points every individual-file offer in
    /// this file funnels through** (`ingest_with`, below, is the other).
    /// `ingest_file` has exactly one non-test caller in the product
    /// (`crates/mnema-ingest/src/walk.rs:851`, inside phase 2 of
    /// `walk_root`, always downstream of phase 1's rule filtering), so an
    /// excluded path is never offered to it by anything real — and guarding
    /// it here, once, is what makes that true of every caller in this file
    /// too, present and future, rather than of whichever caller happened to
    /// be patched last (Task 13, fix round 2: four call sites carried this
    /// check individually after round 1; a systematic probe over 400 seeds
    /// found four more that did not — 22 unrealistic calls from `rename`'s
    /// offer of its own old name, 201 from the three fault-injecting
    /// operations that go through `ingest_with` instead of this function.
    /// None were measured harmful — a second probe over 2000 seeds × 40
    /// steps found zero excluded paths reaching a `Settled` verdict — but
    /// that is a fact about which operations happen to be drawn today, and
    /// the harmless subspecies would not have stayed harmless past the next
    /// reweighting of `draw`'s odds. Two guards close the whole class rather
    /// than the four instances of it anyone had gone looking for.)
    fn ingest_measured(&mut self, relative: &str, on_disk: Option<OnDisk>, how: &str) {
        if self.excluded.contains(relative) {
            self.note(format!("    (excluded, not offered: {relative}{how})"));
            return;
        }
        let before = self.paths_now();
        let hash = self.hash_on_disk(relative);
        let absolute = self.absolute(relative);
        let outcome = ingest_file(
            &self.pool,
            &self.db,
            self.root_id,
            &absolute,
            relative,
            on_disk,
            &self.manifest,
        );
        self.record(relative, hash, outcome, how);
        self.check(&before);
    }

    /// The same index walked by a pool built differently — a lowered ceiling,
    /// a deadline nothing can meet, a sidecar that is not the worker. Each of
    /// those is fixed when a pool is constructed, so they are second pools over
    /// the same database.
    ///
    /// A pool of its own also keeps the poison record separate: a file that
    /// killed a worker is never handed to a second process *by that pool*
    /// (`crates/mnema-pool/src/lib.rs:466-468,549`), and a timeout injected
    /// here must not make the ordinary pool answer from a cached skip for the
    /// rest of the run.
    ///
    /// **The second of the two narrow points** — see `ingest_measured`'s own
    /// doc comment. This one builds its own `Pool` and calls `ingest_file`
    /// directly rather than going through `ingest_measured`, which is
    /// exactly why the guard there could not see it and needs its own copy
    /// here.
    /// Returns what this call answered, or `None` when the guard above
    /// declined to make it — a caller that records something about its own
    /// offer must be able to tell "refused by content" from "never offered",
    /// and from any of the cheap arms that answer without a worker.
    fn ingest_with(&mut self, relative: &str, config: PoolConfig, how: &str) -> Option<Verdict> {
        if self.excluded.contains(relative) {
            self.note(format!("    (excluded, not offered: {relative}{how})"));
            return None;
        }
        let pool = Pool::new(config).unwrap();
        let before = self.paths_now();
        let hash = self.hash_on_disk(relative);
        let absolute = self.absolute(relative);
        let on_disk = mnema_walk::stat(&absolute);
        let outcome = ingest_file(
            &pool,
            &self.db,
            self.root_id,
            &absolute,
            relative,
            on_disk,
            &self.manifest,
        );
        let verdict = self.record(relative, hash, outcome, how);
        self.check(&before);
        self.remember_settled();
        Some(verdict)
    }

    /// Runs `relative` past a database that refuses one write.
    ///
    /// A `BEFORE` trigger that aborts is the only way from outside to force a
    /// database error at a chosen point in a sequence of writes; the
    /// alternative is a fault-injection seam in production code, which would be
    /// a shape the product carries for the tests' sake. The same mechanism
    /// `tests/slice.rs::break_writes_to` uses, with one addition: a `WHEN`
    /// clause, so the write can fail **part-way** through a document rather
    /// than on its first row. That is what makes a torn multi-transaction write
    /// reachable at all — an unconditional trigger on `chunk` aborts slice 0
    /// and rolls the whole document back, which is the easy case.
    fn ingest_with_broken_database(
        &mut self,
        relative: &str,
        event: &str,
        table: &str,
        when: &str,
    ) {
        let clause = if when.is_empty() {
            String::new()
        } else {
            format!(" WHEN {when}")
        };
        self.db
            .conn()
            .execute_batch(&format!(
                "CREATE TRIGGER forced_failure BEFORE {event} ON {table}{clause} BEGIN
                     SELECT RAISE(ABORT, 'forced failure');
                 END;"
            ))
            .unwrap();
        let how = format!(" [{event} {table}{clause} aborts]");
        self.ingest_labelled(relative, &how);
        self.db
            .conn()
            .execute_batch("DROP TRIGGER forced_failure")
            .unwrap();
    }

    // ------------------------------------------------------- reading the index

    /// Notes every document whose chunking has finished, so that a later
    /// failure can distinguish a document that has never been settled from one
    /// whose content hash is on its second visit to the index.
    fn remember_settled(&mut self) {
        for (id, (status, stage)) in self.documents_now() {
            if status == "indexed" && stage.as_deref() == Some("done") {
                self.settled_before.insert(id);
            }
        }
    }

    /// Folds the reader that produced the document now standing at `relative`
    /// into this run's coverage.
    ///
    /// Read out of the `path` row rather than tracked beside it, because the
    /// model's own idea of what it *offered* is a much weaker claim than the
    /// product's record of what it *read*: a body this file believes is a
    /// workbook could be identified as something else entirely, and the whole
    /// point of the coverage assertion is to catch that.
    ///
    /// **Called at the moment a call settles, not at the end of the run**, and
    /// the first version did the latter. Measured: over the default corpus of
    /// twelve seeds, no epub and no workbook was still standing at the settle —
    /// they had been overwritten, excluded or deleted by the sequence — so the
    /// assertion failed on formats the generator was in fact producing and
    /// indexing correctly. Survival to the end is not the question; every
    /// invariant in this file runs after **every call**, so a document that
    /// existed at any point was judged, and that is what the corpus needs to
    /// have contained.
    fn record_reader_of(&mut self, relative: &str) {
        let reader: Option<String> = self
            .db
            .conn()
            .query_row(
                "SELECT reader FROM path WHERE watched_root_id = ?1 AND relative_path = ?2",
                (self.root_id, relative),
                |r| r.get(0),
            )
            .unwrap_or(None);
        if let Some(reader) = reader {
            self.reached.readers.insert(reader);
        }
    }

    fn paths_now(&self) -> BTreeMap<String, String> {
        self.db
            .conn()
            .prepare("SELECT relative_path, document_id FROM path WHERE watched_root_id = ?1")
            .unwrap()
            .query_map([self.root_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    /// Every document, with the two columns that between them answer "may this
    /// be searched, and did its chunking finish?".
    fn documents_now(&self) -> BTreeMap<String, (String, Option<String>)> {
        self.db
            .conn()
            .prepare(
                "SELECT d.id, d.status,
                        (SELECT s.status FROM ingest_stage s
                          WHERE s.content_hash = d.id AND s.stage = ?1)
                   FROM document d",
            )
            .unwrap()
            .query_map([mnema_ingest::STAGE_CHUNK], |r| {
                Ok((r.get(0)?, (r.get(1)?, r.get(2)?)))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn count(&self, sql: &str) -> i64 {
        self.db.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn document_of_chunk(&self, chunk_id: i64) -> Option<String> {
        self.db
            .conn()
            .query_row(
                "SELECT document_id FROM chunk WHERE id = ?1",
                [chunk_id],
                |r| r.get(0),
            )
            .ok()
    }

    /// A document is "settled" when both of its checkpoints agree that its
    /// chunking finished. Only such a document is required to be complete and
    /// findable; one that is half-written after an interrupted job is
    /// legitimately neither.
    fn is_settled(&self, documents: &BTreeMap<String, (String, Option<String>)>, id: &str) -> bool {
        documents
            .get(id)
            .is_some_and(|(status, stage)| status == "indexed" && stage.as_deref() == Some("done"))
    }

    // ------------------------------------------------------------ the failure

    fn fail(&self, what: String) -> ! {
        let mut report = String::new();
        writeln!(report, "\n\n{what}\n").unwrap();
        writeln!(report, "seed {} — replay it with:", self.seed).unwrap();
        writeln!(
            report,
            "    MNEMA_FUZZ_SEED={} cargo test -p mnema-ingest --test randomised -- --nocapture",
            self.seed
        )
        .unwrap();
        writeln!(report, "\nthe sequence that got here:").unwrap();
        for line in &self.log {
            writeln!(report, "{line}").unwrap();
        }
        // The temporary directory is removed when `World` drops, which is after
        // this panic unwinds — naming it is the only way to look at the index a
        // failure left behind, and only useful with the drop suppressed.
        writeln!(report, "\nindex: {}", self.dir.path().display()).unwrap();
        panic!("{report}");
    }
}

// =========================================================== the invariants

impl World {
    /// Everything that must hold after every single step, whatever the step
    /// was.
    ///
    /// `before` is the `path` table as it looked before the step, which is what
    /// invariant 4 compares against; every other invariant is absolute.
    fn check(&self, before: &BTreeMap<String, String>) {
        let after = self.paths_now();
        let documents = self.documents_now();

        self.check_referential_integrity();
        self.check_no_orphan_documents(&after, &documents);
        self.check_no_stale_answers(&after);
        self.check_nothing_settled_went_missing(&after, &documents);
        self.check_nothing_removed_that_the_disk_still_holds(before, &after);
        self.check_a_refusal_by_content_did_what_its_rule_says(before, &after);
        self.check_a_page_that_has_text_again_leaves_no_row();
        self.check_stored_is_findable(&after, &documents);
        self.check_an_unfinished_document_answers_nothing(&after, &documents);
        self.check_chunks_are_searchable();
        self.check_ord_is_dense();
        self.check_checkpoints_agree(&documents);
        self.check_citations_locate();
    }

    /// **0. The schema's own claims still hold.**
    ///
    /// Not one of the five, and the cheapest thing in the file: a cascade that
    /// half-fired or a row written around a foreign key turns every invariant
    /// below into a statement about rubble. Checked first so a failure names
    /// the real cause rather than its symptom.
    fn check_referential_integrity(&self) {
        let broken = self.count("SELECT count(*) FROM pragma_foreign_key_check");
        if broken != 0 {
            self.fail(format!(
                "invariant 0 — {broken} row(s) violate a foreign key, so the index is \
                 no longer describable by its own schema"
            ));
        }
    }

    /// **1. No orphan documents.** Every `document` row is named by at least one
    /// `path` row.
    ///
    /// The model's exception — a document indexed from somewhere a path cannot
    /// describe, inside an archive — does not exist today, and this file writes
    /// no such document, so the invariant is unconditional here. An orphan is a
    /// document nothing can cite: `citation()` joins to `path` with a `LEFT
    /// JOIN`, so it renders with no filename at all while still answering
    /// searches.
    ///
    /// **Task 13, fix round 1: a second, database-driven version of this
    /// check (`db.path_count(id) == 0`, "invariant 1b") was added and then
    /// removed.** It never once fired on its own: `World` inserts exactly one
    /// watched root (`World::new`'s single `insert_watched_root`), `after`
    /// (this function's `BTreeMap` of every `path` row) is already scoped to
    /// that one root, and `path_count` counts globally — over one root, the
    /// two counts are the same rows read two different ways only in the
    /// sense that they are always equal, never in the sense that one can
    /// disagree while the other does not. Confirmed directly: a mutation
    /// that breaks `forget_if_unnamed` turns *this* invariant red and never
    /// 1b. A genuine second witness would need a second watched root in the
    /// world, which is a real, separate piece of modelling (every operation
    /// would need to pick which root it acts on, `paths_now`/`documents_now`
    /// would need to stay root-scoped rather than silently mixing them) —
    /// worth doing on its own, not as a two-line addition riding on this
    /// task's back, so it was left out rather than kept as a witness that
    /// cannot see anything the first one does not. It would not be idle
    /// modelling, either: a second root is also the only way this file could
    /// exercise `Db::delete_watched_root`'s own `NOT EXISTS` clause
    /// (in `Db::delete_watched_root`, the query that decides
    /// whether a document survives deleting the *other* root — it only ever
    /// has one to compare against here), which nothing random touches today.
    fn check_no_orphan_documents(
        &self,
        after: &BTreeMap<String, String>,
        documents: &BTreeMap<String, (String, Option<String>)>,
    ) {
        let named: BTreeSet<&str> = after.values().map(String::as_str).collect();
        let orphans: Vec<&String> = documents
            .keys()
            .filter(|id| !named.contains(id.as_str()))
            .collect();
        if !orphans.is_empty() {
            self.fail(format!(
                "invariant 1 — {} document(s) are named by no path and can only be \
                 cited with no filename: {orphans:?}",
                orphans.len()
            ));
        }
    }

    /// **2. No stale answers.** For every `path` row, either the document it
    /// names holds the file's *current* bytes, or the index has not been told
    /// about those bytes yet.
    ///
    /// This is the one that catches "the index answers with text the file no
    /// longer contains, under a filename that exists" — the worst citation this
    /// product can produce, because it offers a highlight over characters that
    /// are gone.
    ///
    /// There are exactly two ways to be behind and be right, and they are the
    /// two things the model tracks rather than predicts:
    ///
    /// * the bytes changed since the last time this path was offered to
    ///   `ingest_file` — the walk has not reached it, and nothing has claimed
    ///   otherwise;
    /// * the last offer was **refused** — journalled as a skip, or lost to a
    ///   database that would have stopped the job. `displaces` decides which
    ///   refusals also remove what the index held, and this file deliberately
    ///   does not re-derive that decision; it only insists that a *settled*
    ///   answer means what it says.
    ///
    /// A file that is not on the disk at all makes no claim: whether a deleted
    /// file's rows should go is the watched folder's business, and `ingest_file`
    /// is handed one name rather than a directory listing.
    fn check_no_stale_answers(&self, after: &BTreeMap<String, String>) {
        for (relative, document) in after {
            let Some(hash) = self.hash_on_disk(relative) else {
                continue;
            };
            if &hash == document {
                continue;
            }
            match self.last.get(relative) {
                // Never offered, or offered when the file was different: the
                // index has not had its chance yet.
                None => continue,
                Some(last) if last.hash.as_ref() != Some(&hash) => continue,
                Some(last) if last.verdict != Verdict::Settled => continue,
                Some(_) => self.fail(format!(
                    "invariant 2 — {relative} answers with a document whose content is \
                     not the file's. The last ingest of it settled, and the bytes have \
                     not moved since, so there is no version of events in which the \
                     index is merely behind.\n  index holds: {document}\n  file is:     \
                     {hash}"
                )),
            }
        }
    }

    /// **2b. The other direction: nothing settled went missing.**
    ///
    /// Invariant 2 is about a `path` row that says the wrong thing. This is
    /// about one that stopped saying anything: a path `ingest_file` answered
    /// `Indexed`, `Unchanged` or `AlreadyIndexed` for, whose file has not
    /// changed since, must still be in the index — and the document it names
    /// must be one whose chunking is finished, since all three of those answers
    /// assert exactly that.
    ///
    /// Without this half, an implementation that deleted everything it touched
    /// would satisfy invariant 2 perfectly.
    ///
    /// **Task 13, fix round 1: this carried an exception for `self.excluded`
    /// through its first review, and it was the wrong fix.** The state it
    /// was covering for — a settled path, unchanged, gone with no path row —
    /// only ever arose here because an ordinary operation picked an excluded
    /// path with [`World::a_file`] and called `self.ingest` on it directly,
    /// which no real walk can do (`ingest_file` has exactly one non-test
    /// caller, inside phase 2 of `walk_root`, always downstream of phase 1's
    /// rule filtering). That is a generator artefact, not a legitimate state
    /// this invariant has to make room for — `settle()` already skipped
    /// excluded names for exactly this reason in the same change that added
    /// the exception here. `maybe_ingest`, `reingest` and the two recovery
    /// offers in `database_refuses_a_write` now skip an excluded path too, so
    /// this loop no longer needs to know `excluded` exists at all.
    fn check_nothing_settled_went_missing(
        &self,
        after: &BTreeMap<String, String>,
        documents: &BTreeMap<String, (String, Option<String>)>,
    ) {
        for (relative, last) in &self.last {
            if last.verdict != Verdict::Settled {
                continue;
            }
            let Some(hash) = self.hash_on_disk(relative) else {
                continue;
            };
            if last.hash.as_ref() != Some(&hash) {
                continue;
            }
            match after.get(relative) {
                None => self.fail(format!(
                    "invariant 2b — the index settled {relative} and the file has not \
                     changed since, but no path row names it any more: the file is \
                     silently out of the index until a whole further pass reaches it"
                )),
                Some(document) if document != &hash => continue, // invariant 2's business
                Some(document) if !self.is_settled(documents, document) => self.fail(format!(
                    "invariant 2b — ingest_file answered that {relative} was up to date, \
                     but its document {document} is {:?} with stage {:?}: the answer \
                     asserts a finished chunking that did not finish",
                    documents.get(document).map(|d| d.0.clone()),
                    documents.get(document).and_then(|d| d.1.clone()),
                )),
                Some(_) => {}
            }
        }
    }

    /// **3 (the core). Content is removed only when the file stopped holding
    /// it.**
    ///
    /// If a `path` row named document `d` before the step and does not after —
    /// deleted, or repointed at something else — then the bytes at that path
    /// must no longer hash to `d`. Removing the index's record of a path whose
    /// file still holds *exactly* the content the index held is data loss with
    /// no reading under which it is right.
    ///
    /// It is stated over **every** path rather than the one the step touched,
    /// which is what makes it catch the collateral case: a rebuild of one copy
    /// of a file taking the other copy's row with it through
    /// `path.document_id`'s `ON DELETE CASCADE`. And it holds even when the
    /// call returned `Err`, because a database that refused a write must leave
    /// less behind, never less content.
    ///
    /// A file that is no longer on the disk is not evidence either way — its
    /// bytes cannot be hashed — so a path whose file has gone is excluded here.
    /// The `Unreadable` half of that is covered by invariant 2b instead.
    ///
    /// **Amended for Task 13, and only once measured, not on suspicion:**
    /// a path currently named by `excluded` is excused too, but **only**
    /// while `self.walking` is set — the narrow window right after a
    /// `walk_root` call that could legitimately be the one removing it.
    /// Every invariant above this one was written before `RunWalk` existed,
    /// in a world where nothing ever removed a path row for a file whose
    /// bytes had not moved — `WalkRules` gives phase 3 a second, legitimate
    /// reason to (§5, "I excluded that folder" has to mean it), and the very
    /// first run of this harness after `toggle_exclusion` was added found it:
    /// seed 1592590336, `backup/copy-17.txt`, excluded at step 15 and
    /// reconciled away three steps later inside `settle`, failed here with
    /// `it now holds: None` before this exception existed.
    ///
    /// **Fix round 1 narrowed the exception to `self.walking` only**, after
    /// review found the un-narrowed form tolerated a state correct code never
    /// produces: excluding a path does not take effect until the *next*
    /// walk (§5's own words again), so a path row disappearing on an
    /// ordinary, non-walk step — through the very `ON DELETE CASCADE` this
    /// invariant's own first paragraph names as its quarry — must still be
    /// caught, exclusion or not. The narrowed form was run for 400 seeds
    /// (release, bases 0 and 1000000) with no change in outcome, so it costs
    /// nothing where the wide form was needed and closes where it was not.
    /// The claim this invariant states — content vanishes only when the file
    /// itself stopped holding it — is unchanged; what changed is that "the
    /// file itself" now includes whether a rule says *this walk* may look at
    /// it, which the file's bytes alone never could.
    ///
    /// **Fix round 2 added the second exception, and it is the same amendment
    /// as the first one rather than a new kind.** A refusal by the size ceiling
    /// is made from `stat`, without the file being opened, so a call that gets
    /// one has read nothing: from there a file merely touched and a file
    /// rewritten in place at the same length are the same two numbers, and
    /// `displaces` takes the loss over the stale citation (its own section on
    /// `TooLarge` has the trade and what is left over). This invariant's claim
    /// still holds as stated — content vanishes only when the file stopped
    /// holding it, *or when a setting forbade this call to find out*, which is
    /// exactly what excluding a folder already meant here.
    ///
    /// It is scoped to the one path the call in flight was about, and to a
    /// verdict of `TooLarge`, so it excuses nothing else: no other rule reaches
    /// it, and a collateral removal elsewhere in the same call is still caught.
    /// It does cost this invariant the only witness it had for that rule
    /// keeping a document — `a_lowered_ceiling_keeps_what_it_still_recognises`
    /// (`tests/slice.rs`) is where that direction is asserted now, on the one
    /// state where keeping is still required — and invariant 3c gained a
    /// `TooLarge` arm in the same change, which is the direction that was
    /// missing here before either of them: nothing at all forbade the index to
    /// go on answering for an oversized file whose bytes had moved.
    fn check_nothing_removed_that_the_disk_still_holds(
        &self,
        before: &BTreeMap<String, String>,
        after: &BTreeMap<String, String>,
    ) {
        for (relative, document) in before {
            if after.get(relative) == Some(document) {
                continue;
            }
            if self.walking && self.excluded.contains(relative) {
                continue;
            }
            if self.calling.as_deref() == Some(relative.as_str())
                && matches!(
                    self.last.get(relative).map(|last| last.verdict),
                    Some(Verdict::Skipped(SkipRule::TooLarge))
                )
            {
                continue;
            }
            if self.hash_on_disk(relative).as_ref() == Some(document) {
                self.fail(format!(
                    "invariant 3 — the index stopped holding {document} under {relative}, \
                     and the file at that path still contains exactly those bytes. \
                     Whatever the step was, it deleted indexed content over something \
                     that was not a change to the file.\n  it now holds: {:?}",
                    after.get(relative)
                ));
            }
        }
    }

    /// **3c. A refusal decided on the file's own bytes did what its rule says
    /// about the document already under that path.**
    ///
    /// This exists because none of the invariants above can see the defect it
    /// is written for, and the reason is structural rather than an oversight.
    /// [`LastCall`]'s own doc comment states the rule they all share: the index
    /// is allowed to be behind the disk when the offer was **refused**. So a
    /// skip — any skip, under any rule — is blanket permission, and invariant 2
    /// steps over the path, invariant 2b excuses it because the file changed,
    /// and invariant 3 excuses it because the bytes on disk no longer hash to
    /// what the index holds. Both directions of the D51 defect therefore land
    /// in the gap between them:
    ///
    /// * a photo replacing a note, where the index goes on answering with text
    ///   the file no longer contains — the worst citation this product can
    ///   produce;
    /// * a note whose append was interrupted, where the index **deletes** prose
    ///   that is still on disk in front of the damage and readable nowhere
    ///   else.
    ///
    /// Measured, and this is why it is worth a whole invariant: with the two
    /// operations added and this check absent, moving `SkipRule::BinaryTail`
    /// onto the displacing side of `displaces` left every seed of this harness
    /// green.
    ///
    /// `Crash`, `Timeout`, `Memory` and `Unreadable` are readings of the
    /// environment and keep, which invariant 3 already covers whenever the
    /// bytes did not move.
    ///
    /// **`TooLarge` was left out of this check, and that was the structural
    /// hole fix round 2 closed.** The reasoning for leaving it out was that its
    /// answer turns on a size comparison rather than on the rule, so restating
    /// it here would re-derive `displaces` inside the tool built to check it —
    /// which is right about `displaces` and wrong about what this invariant
    /// asks. The three checks around it left a gap shaped exactly like that
    /// rule: invariant 2 excuses any path whose last answer was a refusal, this
    /// one had `TooLarge` among its empty arms, and invariant 3 only ever fires
    /// on a removal. So nothing anywhere forbade the index to go on answering,
    /// under a name that exists, with a document built from bytes that file no
    /// longer holds — for as long as it stayed over the ceiling, which is
    /// forever, since the worker keeps refusing it from `stat`.
    ///
    /// The arm added below does **not** restate `displaces`. It asserts what
    /// this harness knows and the product cannot: the digest of the bytes the
    /// call was made on. When those bytes are not the document still standing
    /// under that path, the index is answering with text the file does not
    /// contain, whatever evidence `displaces` had to reach that with. The
    /// keeping direction is deliberately not asserted here — a refusal made
    /// without opening the file cannot tell an untouched file from a
    /// same-length rewrite, so demanding a keep would be demanding a
    /// distinction that does not exist. It is asserted deterministically
    /// instead, in `a_lowered_ceiling_keeps_what_it_still_recognises`
    /// (`tests/slice.rs`), on the one state where it is still owed.
    ///
    /// Scoped to the path this call was about: a step may legitimately do
    /// several things, and `before` here is the table as it stood immediately
    /// before this one `ingest_file`.
    fn check_a_refusal_by_content_did_what_its_rule_says(
        &self,
        before: &BTreeMap<String, String>,
        after: &BTreeMap<String, String>,
    ) {
        // The call that just happened, and only it. Iterating `self.last`
        // instead re-judges every verdict this run ever recorded against a
        // `before`/`after` pair that belongs to someone else's call — which is
        // how a `binary_tail` refusal at step 17 was read as having deleted a
        // path that phase 3 legitimately reaped at step 24.
        let Some(relative) = self.calling.as_deref() else {
            return;
        };
        if let Some(last) = self.last.get(relative) {
            let Verdict::Skipped(rule) = last.verdict else {
                return;
            };
            let Some(held) = before.get(relative) else {
                // Nothing was under the path, so there is nothing either rule
                // could have displaced or kept.
                return;
            };
            match rule {
                // Conditional on the digest since task 10, and both branches
                // are asserted here because each one is a document lost or a
                // citation left lying.
                //
                // This arm used to demand displacement unconditionally, which
                // was a faithful statement of `displaces` as it then stood —
                // and this harness is what proved that contract wrong: a file
                // whose bytes had never moved lost its document the first time
                // anything pushed its mtime past the cheap arm and the rule
                // under it had changed. What follows is the corrected
                // contract, not a relaxed one.
                //
                // The keep side is **not** a claim nothing asserted before,
                // which this comment used to say: invariant 3 already forbids
                // the index to stop holding a document whose bytes are still on
                // disk, and the report of the run that added this arm shows it
                // — with `NotText => true` restored, the invariant that went red
                // was 3, not 3c. What this arm adds is the *displace* side,
                // which no other invariant states, and a message that names the
                // rule rather than the deletion.
                //
                // `Unsupported` is judged by the same two lines and not left in
                // an empty arm beside them, which is where it sat until fix
                // round 2. `displaces` gives the two rules the identical
                // condition, and `make_opaque` — a PDF header saved over a note
                // — reaches this one on every run, so the arm was silent about
                // a verdict the generator produces rather than about one it
                // cannot. Its keep side stays unreachable here for a reason
                // worth naming rather than hiding: it needs a build that *lost*
                // a reader, and no sidecar in this file models one. The
                // deterministic pair does
                // (`a_file_no_reader_can_take_keeps_its_document_when_only_the_rule_changed`).
                // `Malformed` and `Encrypted` are judged by the same two lines
                // because `displaces` gives them the same condition, and they
                // are placed here rather than in the empty arm below on
                // purpose: an empty arm accepts both behaviours, which is how a
                // rule gets onto the wrong side of `displaces` without a single
                // seed going red.
                //
                // **Dormant today, and named as dormant rather than left to be
                // discovered.** No operation in this generator produces either
                // rule — nothing in this build sends those wire strings, so the
                // real worker cannot answer with them — and a reader that
                // refuses a truncated or locked file is what makes this arm
                // start firing. It is written now so that the reader arrives to
                // an assertion instead of to an empty arm.
                SkipRule::NotText
                | SkipRule::Unsupported
                | SkipRule::Malformed
                | SkipRule::Encrypted
                | SkipRule::NoTextLayer => match last.hash.as_deref() {
                    // The worker saw exactly the bytes the index was built
                    // from. The rule changed, the file did not.
                    Some(sha) if sha == held => {
                        if after.get(relative) != Some(held) {
                            self.fail(format!(
                                "invariant 3c — {relative} was refused as {rule:?} and lost \
                                 {held}, but its bytes are byte-identical to what that \
                                 document was built from. Nothing about the file changed, \
                                 so a release that classifies it differently has deleted \
                                 text that is still on disk.\n  it now holds: {:?}",
                                after.get(relative)
                            ));
                        }
                    }
                    // Different bytes: the path holds something else now, and
                    // what the index answers with belongs to a file that is
                    // gone.
                    Some(_) if after.get(relative) == Some(held) => {
                        self.fail(format!(
                            "invariant 3c — {relative} was refused as {rule:?}, and the \
                             index still answers for it with {held}. Those bytes are \
                             something else now, so every citation of that document names \
                             a file whose text it no longer contains"
                        ));
                    }
                    Some(_) => {}
                    // The harness could not hash the file at the moment of the
                    // call, so it cannot say which of the two cases this was
                    // and asserts neither.
                    None => {}
                },
                // Unconditionally false in `displaces`, and the whole of why
                // the rule is separate from `NotText`.
                SkipRule::BinaryTail => {
                    if after.get(relative) != Some(held) {
                        self.fail(format!(
                            "invariant 3c — {relative} was refused as a binary tail and \
                             the index stopped holding {held} under it. The file opened \
                             as text and stopped; the prose the index had is still on \
                             disk in front of the damage, and deleting it loses text \
                             that is readable nowhere else.\n  it now holds: {:?}",
                            after.get(relative)
                        ));
                    }
                }
                // Refused from `stat`, without the file being opened — so the
                // product has no reading of the content and this harness does.
                // One direction only, and the doc comment above says why the
                // other one is owed elsewhere.
                SkipRule::TooLarge => {
                    if last.hash.as_deref().is_some_and(|sha| sha != held)
                        && after.get(relative) == Some(held)
                    {
                        self.fail(format!(
                            "invariant 3c — {relative} was refused for being over the size \
                             ceiling, and the index still answers for it with {held}. The \
                             file no longer holds those bytes, and it will be refused from \
                             `stat` on every later walk too, so nothing is coming to \
                             correct it"
                        ));
                    }
                }
                // The environment faults, and **only** them: `displaces` answers
                // an unconditional `false` for all four, and this harness has
                // nothing to add about a worker that crashed.
                //
                // 🔴 `NoTextLayer` used to sit in this list and does not belong
                // in it. `displaces` gives it the *same* condition as the four
                // content rules above — `content.is_none_or(|sha| sha !=
                // recorded.document_id)` (`crates/mnema-ingest/src/lib.rs`) —
                // while this arm asserted nothing at all about it, and an empty
                // arm accepts both behaviours. That is the failure this very
                // function's comment warns about, three lines up, about a
                // different rule. It went unnoticed because no generator here
                // could produce the rule: a document that opens and holds no
                // words needed a reader that opens documents, and until this
                // cycle there was none. The class and the assertion for it
                // arrived in opposite orders.
                SkipRule::Crash | SkipRule::Timeout | SkipRule::Memory | SkipRule::Unreadable => {}
            }
        }
    }

    /// **3d. A page that has text again leaves no row saying it has none.**
    ///
    /// Per-page journal rows are a class of their own and this harness had never
    /// seen one. They are written from the numbers in
    /// `Frame::Summary.skipped_pages`, they live in the same `skip` table as the
    /// file-level verdicts, and they are cleared by a **separate** path —
    /// `Db::forget_page_skips` — because `forget_skip` deliberately leaves them
    /// alone. Two independent call sites maintain them
    /// (`crates/mnema-ingest/src/lib.rs:741,843`), and a plan that saw only one
    /// is how the class arrived with a hole in it.
    ///
    /// What a stale row costs is not abstract: it is a line in the journal
    /// telling someone that page 3 of a document could not be read, while the
    /// index holds page 3's text and answers searches with it. Nothing else in
    /// this file would notice — the document is complete, every marker is
    /// findable, and the row sits beside it saying otherwise.
    ///
    /// Scoped to a call that **actually read the file**. `Unchanged` and
    /// `AlreadyIndexed` never open it, so rows about its pages are still true;
    /// it is the fresh index that owes the clean-up.
    ///
    /// **Both directions.** A document whose reader really did skip a page must
    /// *have* the rows — otherwise the assertion below is satisfied by a build
    /// that writes no page rows at all, and the whole class would go untested
    /// while reading as covered.
    fn check_a_page_that_has_text_again_leaves_no_row(&self) {
        let Some(relative) = self.calling.as_deref() else {
            return;
        };
        let Some(last) = self.last.get(relative) else {
            return;
        };
        // **Two entrances, and the plan that added this class saw one.** A
        // fresh index rewrites the rows (`journal_skipped_pages`); a refusal
        // that leaves no `path` row clears them outright, because `repoint` —
        // the only other place that maintains them — never runs for this path
        // again and the rows would go on naming a missing page of a document
        // the index does not hold. Judging only the first left the second
        // measurable-but-unmeasured: mutating it away kept every seed green.
        let refused_and_gone =
            matches!(last.verdict, Verdict::Skipped(_)) && !self.paths_now().contains_key(relative);
        if !last.indexed && !refused_and_gone {
            return;
        }
        let Some(state) = self.files.get(relative) else {
            return;
        };
        let rows: i64 = self
            .db
            .conn()
            .query_row(
                "SELECT count(*) FROM skipped WHERE watched_root_id = ?1 AND relative_path = ?2 \
                 AND page_no IS NOT NULL",
                (self.root_id, relative),
                |r| r.get(0),
            )
            .unwrap();

        // A refused path holds no document at all, so the only honest claim is
        // that no page of it is still being reported as missing.
        if refused_and_gone {
            if rows > 0 {
                self.fail(format!(
                    "invariant 3d — {relative} was refused and the index holds no document \
                     for it, and the journal still holds {rows} row(s) about a page of one. \
                     The window answering \"why is this not in my index?\" names a missing \
                     page of a document that is not there either"
                ));
            }
            return;
        }

        match state.shape {
            Shape::Gappy(_, pages) => {
                if rows == 0 {
                    self.fail(format!(
                        "invariant 3d — {relative} was just indexed and its reader could not \
                         read one of the pages it declares, and the journal holds no row for \
                         it. The page is missing from the index and nothing anywhere says so"
                    ));
                }
                self.check_the_gap_is_at_the_number_it_is_at(relative, pages);
            }
            _ => {
                if rows > 0 {
                    self.fail(format!(
                        "invariant 3d — {relative} was just indexed and every page it \
                         declares was read, and the journal still holds {rows} row(s) saying \
                         a page of it could not be. The index answers with that page's text \
                         while the journal tells someone it is missing"
                    ));
                }
            }
        }
    }

    /// **3e. The page that could not be read is the one reported, and every
    /// other page holds its own text.**
    ///
    /// 🔴 **This is the half of Task 11's class that counting rows cannot
    /// reach.** That cycle found two defects on one book, and they are not the
    /// same defect: an `<itemref/>` with no `idref` made the entry *vanish* and
    /// shifted every later chapter up by one, and a "sensible" fix for it put
    /// **one chapter's text under another chapter's number**. Invariant 3d
    /// catches the first — a shifted book reports no gap, and `rows == 0`
    /// fires. Nothing caught the second: the row count is right, every marker
    /// is findable, invariant 4 asks only that a marker match *some* chunk of
    /// the *same* document, and the citation quietly names the wrong chapter.
    ///
    /// `gappy_body` puts the gap **first** for exactly this reason, and
    /// `epub_of` already writes down the answer — "its position in this slice
    /// is that number minus one" — so the expected numbers are known without
    /// re-deriving anything: the gap is page 1, and the readable pages are
    /// 2..=pages+1, each carrying the marker it was built with.
    ///
    /// Both directions, and they fail differently. The **numbers** catch a
    /// renumbering that drops the gap; the **text on each page** catches a
    /// renumbering that keeps the count and slides the content along it.
    fn check_the_gap_is_at_the_number_it_is_at(&self, relative: &str, pages: usize) {
        let skipped: Vec<i64> = self
            .db
            .conn()
            .prepare(
                "SELECT page_no FROM skipped WHERE watched_root_id = ?1 AND relative_path = ?2 \
                 AND page_no IS NOT NULL ORDER BY page_no",
            )
            .unwrap()
            .query_map((self.root_id, relative), |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        if skipped != vec![1] {
            self.fail(format!(
                "invariant 3e — {relative} was built with its unreadable page **first**, so \
                 page 1 is the one that cannot be read, and the journal names {skipped:?}. \
                 A reader that renumbers what came back reports a gap that is not where the \
                 gap is, and every citation after it names the wrong page"
            ));
        }

        let Some(document) = self.paths_now().get(relative).cloned() else {
            return;
        };
        let Some(state) = self.files.get(relative) else {
            return;
        };
        // Page 1 is the gap, so the markers start at page 2 in the order they
        // were written.
        for (i, expected) in state.markers.iter().enumerate() {
            let page_no = i as i64 + 2;
            let text: Option<String> = self
                .db
                .conn()
                .query_row(
                    "SELECT group_concat(b.text, ' ') FROM block b JOIN page p ON b.page_id = p.id \
                     WHERE p.document_id = ?1 AND p.page_no = ?2",
                    (&document, page_no),
                    |r| r.get(0),
                )
                .unwrap_or(None);
            let Some(text) = text else {
                self.fail(format!(
                    "invariant 3e — {relative} declares {pages} readable pages after its gap, \
                     and the index holds no page {page_no} for the document it built. The \
                     chapter is gone and the page numbering closed over the hole"
                ));
            };
            if !text.contains(expected.as_str()) {
                self.fail(format!(
                    "invariant 3e — page {page_no} of {relative} should hold {expected:?} and \
                     holds {text:?}. The text of one chapter is stored under another \
                     chapter's number: every marker is still findable, the row count is \
                     still right, and the citation names a chapter that does not contain it"
                ));
            }
        }

        // **The same member named twice is two pages, and the second is not the
        // first.** An epub spine may reference one chapter more than once, and
        // the generator always ends a gappy book that way. What this asserts is
        // that the repeat arrived as a page of its own carrying chapter 0's
        // text — a reader that silently collapsed it would lose a page the
        // spine declares, and one that mislabelled it would put chapter 0's
        // words under a number that is not chapter 0's either.
        if matches!(state.shape, Shape::Gappy(Format::Epub, _))
            && let Some(first) = state.markers.first()
        {
            let repeat_at = pages as i64 + 2;
            let text: Option<String> = self
                .db
                .conn()
                .query_row(
                    "SELECT group_concat(b.text, ' ') FROM block b JOIN page p ON b.page_id = p.id \
                     WHERE p.document_id = ?1 AND p.page_no = ?2",
                    (&document, repeat_at),
                    |r| r.get(0),
                )
                .unwrap_or(None);
            match text {
                Some(text) if text.contains(first.as_str()) => {}
                other => self.fail(format!(
                    "invariant 3e — the spine of {relative} names its first chapter a second \
                     time, so page {repeat_at} should hold {first:?} again, and it holds \
                     {other:?}. A page the spine declares has been dropped or renumbered"
                )),
            }
        }
    }

    /// **3b. What phase 3 could tell was gone, it actually removed.**
    ///
    /// Invariant 3 only ever proves the negative — a path row still standing
    /// does not lie about content the disk has moved past. It cannot prove
    /// the positive, because most calls in this file have no [`WalkReport`]
    /// to consult and cannot tell "no evidence yet" from "evidence refused."
    /// `RunWalk` has one: `report.frozen` is phase 3's own account of every
    /// prefix it declined to trust — D33's ambiguity between an unmounted
    /// share and a deletion, read back here rather than re-derived. A
    /// `before` path that is off the disk, or newly excluded by a rule
    /// (§5 — "I excluded that folder" has to mean it), and sits under none
    /// of those prefixes has nothing left standing between it and deletion,
    /// so a row that survives it anyway is phase 3 failing its own stated
    /// contract.
    ///
    /// This is the invariant the task brief asked for as two — "a vanished
    /// file leaves no path row" and "an excluded file is not findable" — folded
    /// into one, because both are the same claim from phase 3's point of
    /// view (a `known` path this walk has no reason to keep) and because the
    /// literal, unconditional form of either one is false against correct
    /// code: a directory a user empties without deleting reads exactly like
    /// an unmounted share and is *frozen*, not removed (D33, `walk.rs`'s own
    /// doc comment on phase 3) — and a copy of a file that is still findable
    /// through some OTHER, non-excluded path is not a counterexample to
    /// "excluded is unfindable" at all. Reading `report.frozen` back is what
    /// keeps this from re-deriving `resolve_ancestor`'s own logic to guess
    /// at the first, and scoping every check to the ONE path phase 3 was
    /// actually asked to remove — never to whatever marker its bytes carry —
    /// is what avoids the second without having to track which other paths
    /// share a hash.
    ///
    /// Scoped to `report.stopped == StopReason::Completed`: any other stop
    /// means phase 3 never ran at all (`walk_root`'s own gate,
    /// `crates/mnema-ingest/src/walk.rs`), and every `before` path is
    /// untouched by construction — asserting removal there would be
    /// asserting a contract phase 3 was never asked to keep.
    fn check_walk_removed_what_it_could(
        &self,
        before: &BTreeMap<String, String>,
        after: &BTreeMap<String, String>,
        report: &WalkReport,
    ) {
        if report.stopped != StopReason::Completed {
            return;
        }
        for relative in before.keys() {
            let gone_from_disk = self.hash_on_disk(relative).is_none();
            let newly_excluded = self.excluded.contains(relative);
            if !gone_from_disk && !newly_excluded {
                continue;
            }
            // Deliberately NOT an equality check as well, and the branch
            // review measured why the obvious reasoning for adding one is
            // wrong. A `known` path equal to a frozen prefix IS reachable:
            // `enumerate` skips directories (`mnema-walk`'s `is_dir`
            // continue), so a path that currently exists as a directory is
            // in neither `found` nor `skipped` and therefore never in
            // `seen` — while every ancestor-climb prefix is by construction
            // an existing directory. A file indexed under `mnt/share`,
            // replaced by a directory, reaches exactly that state, and
            // phase 3 then deletes the row. **That deletion is correct**:
            // the file really is gone. An assertion here that the equality
            // never arises would fail a run on a state correct code
            // produces — and could not fire today anyway, since every
            // generated path carries an extension while the only
            // directories are `docs` and `backup`.
            if report.frozen.iter().any(|f| under(relative, &f.prefix)) {
                continue;
            }
            if after.contains_key(relative) {
                self.fail(format!(
                    "invariant 3b — {relative} is {}, phase 3 froze nothing that covers it \
                     ({:?}), and it still has a path row after a walk that completed",
                    if gone_from_disk {
                        "gone from disk"
                    } else {
                        "excluded by a rule"
                    },
                    report.frozen,
                ));
            }
        }
    }

    /// **4. Nothing is stored but unfindable.**
    ///
    /// Every word of a document the index calls finished must answer to a
    /// search — under D29 the lexical arm is the only private way into this
    /// index, so a chunk that is stored and not findable is a chunk that does
    /// not exist for the user.
    ///
    /// Marker by marker, and every page carries one, so this is also the check
    /// that a document is **complete**: a job torn between two of the write
    /// loop's transaction slices leaves pages 21..n missing, and their markers
    /// are missing with them. Nothing here models the extractor to know that —
    /// it only knows which words it wrote into the file.
    ///
    /// Scoped to documents whose chunking is recorded as finished. A document
    /// half-written by an interrupted job is legitimately incomplete; that it
    /// must not *stay* that way is invariant 6's business, and the sequence
    /// ending in [`World::settle`] is where it is collected.
    /// **3f. A document answers a search whole, or does not answer at all —
    /// never with part of itself.**
    ///
    /// The other half of invariant 4, and the half nothing stated.
    /// `check_stored_is_findable` **skips** a document that is not settled
    /// (`is_settled`: `status == indexed && stage == done`), which is right —
    /// a document half-written has no obligation to be findable. What follows
    /// from that and was never written down is the opposite obligation: while
    /// it is half-written it must be findable **not at all**, and this is where
    /// the two halves meet.
    ///
    /// It is D61's whole content. A rebuild puts `document.status` back to
    /// `pending` precisely so that the document stops answering while its
    /// chunks are being replaced; without that, a rebuild cut between slices
    /// leaves a document answering with the chunks that landed — a search
    /// returning half a contract, with nothing anywhere saying it is half.
    ///
    /// **Scoped to hits that belong to this document.** The same marker can
    /// legitimately answer from somewhere else — a copy of the file at another
    /// path, indexed and finished — and that is not this document answering.
    /// `document_of_chunk` is what tells them apart; without it this invariant
    /// would fail on an ordinary `copy` and would then be "fixed" by weakening
    /// it, which is how a real one gets lost.
    fn check_an_unfinished_document_answers_nothing(
        &self,
        after: &BTreeMap<String, String>,
        documents: &BTreeMap<String, (String, Option<String>)>,
    ) {
        for (relative, document) in after {
            if self.is_settled(documents, document) {
                continue;
            }
            let Some(state) = self.files.get(relative) else {
                continue;
            };
            let Some((status, stage)) = documents.get(document) else {
                continue;
            };
            for marker in sample(&state.markers) {
                for chunk_id in self.db.search_lexical(marker, 20).unwrap() {
                    if self.document_of_chunk(chunk_id).as_deref() == Some(document.as_str()) {
                        self.fail(format!(
                            "invariant 3f — {relative} names a document that is not finished \
                             (status {status:?}, stage {stage:?}) and chunk {chunk_id} of that \
                             same document already answers a search for {marker:?}. A document \
                             being written must answer whole or not at all; this one answers \
                             with the part that happened to land"
                        ));
                    }
                }
            }
        }
    }

    fn check_stored_is_findable(
        &self,
        after: &BTreeMap<String, String>,
        documents: &BTreeMap<String, (String, Option<String>)>,
    ) {
        for (relative, document) in after {
            if self.hash_on_disk(relative).as_deref() != Some(document.as_str()) {
                continue;
            }
            if !self.is_settled(documents, document) {
                continue;
            }
            let Some(state) = self.files.get(relative) else {
                continue;
            };
            for marker in sample(&state.markers) {
                let hits = self.db.search_lexical(marker, 20).unwrap();
                let found = hits
                    .iter()
                    .filter_map(|id| self.document_of_chunk(*id))
                    .any(|id| &id == document);
                if !found {
                    self.fail(format!(
                        "invariant 4 — {relative} is indexed as document {document}, its \
                         chunking is recorded as finished, and the word {marker:?} that \
                         is in the file right now answers to no chunk of it. Either the \
                         document is missing part of itself or its search rows are.\n  \
                         hits for it elsewhere: {hits:?}"
                    ));
                }
            }
        }
    }

    /// **4b. …and the storage under the search agrees with itself.**
    ///
    /// `chunk` and `chunk_search` are written as one transaction and
    /// `chunk_fts` is kept in step by triggers, including through a cascade. A
    /// chunk with no search row is citable and permanently unfindable, with no
    /// error anywhere; a search row with no chunk answers a query with a
    /// citation that cannot be built.
    fn check_chunks_are_searchable(&self) {
        let unsearchable = self.count(
            "SELECT count(*) FROM chunk c
              WHERE NOT EXISTS (SELECT 1 FROM chunk_search s WHERE s.chunk_id = c.id)",
        );
        if unsearchable != 0 {
            self.fail(format!(
                "invariant 4b — {unsearchable} chunk(s) have no chunk_search row: stored, \
                 citable, and unfindable by any query"
            ));
        }
        let dangling = self.count(
            "SELECT count(*) FROM chunk_search s
              WHERE NOT EXISTS (SELECT 1 FROM chunk c WHERE c.id = s.chunk_id)",
        );
        if dangling != 0 {
            self.fail(format!(
                "invariant 4b — {dangling} chunk_search row(s) name no chunk: a search \
                 would answer with a hit that cannot be turned into a citation"
            ));
        }
        let searchable = self.count("SELECT count(*) FROM chunk_search");
        let indexed = self.count("SELECT count(*) FROM chunk_fts");
        if searchable != indexed {
            self.fail(format!(
                "invariant 4b — {searchable} search rows against {indexed} rows in \
                 chunk_fts: the triggers that keep the two in step missed a delete or an \
                 update, which is exactly the case a cascade produces"
            ));
        }
    }

    /// **4c. A document's chunks are its whole sequence, with no gap.**
    ///
    /// `ord` is unique per document and the writer carries it across pages and
    /// across transaction slices. A document whose ords are not `0..n` has lost
    /// chunks out of its middle — invisible to a search for anything that is
    /// still there, and the shape a partial rebuild leaves.
    fn check_ord_is_dense(&self) {
        let rows: Vec<(String, i64, i64, i64)> = self
            .db
            .conn()
            .prepare(
                "SELECT document_id, count(*), min(ord), max(ord) FROM chunk GROUP BY document_id",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for (document, count, low, high) in rows {
            if low != 0 || high != count - 1 {
                self.fail(format!(
                    "invariant 4c — document {document} has {count} chunks numbered \
                     {low}..={high}: the sequence has a hole in it, so part of the \
                     document is gone while the rest still answers"
                ));
            }
        }
    }

    /// **5. The two checkpoints never disagree.**
    ///
    /// `ingest_stage` says whether the chunking finished; `document.status`
    /// says whether the document may be searched. They are written as one
    /// transaction precisely so that no interruption can separate them, and the
    /// state this rules out is the expensive one: a stage that says `done` over
    /// a status that says `pending` short-circuits the cheap arm on every
    /// future walk, so the document is never re-indexed and never repaired —
    /// for the life of the index.
    ///
    /// Scoped to documents that exist. A stage row surviving a document it
    /// belonged to is a different question, and it is the one the trace below
    /// will reach if it is a live defect.
    fn check_checkpoints_agree(&self, documents: &BTreeMap<String, (String, Option<String>)>) {
        for (id, (status, stage)) in documents {
            let finished = stage.as_deref() == Some("done");
            let searchable = status == "indexed";
            if finished != searchable {
                // The two facts that tell the two causes apart, and they are
                // worth carrying in the message rather than reconstructing by
                // hand: a checkpoint torn in half leaves a document nothing has
                // seen before, while a stage row that outlived an **earlier**
                // document of the same content leaves one whose hash the index
                // has settled once already.
                let returning = self.settled_before.contains(id);
                let chunks: i64 = self
                    .db
                    .conn()
                    .query_row(
                        "SELECT count(*) FROM chunk WHERE document_id = ?1",
                        [id],
                        |r| r.get(0),
                    )
                    .unwrap();
                self.fail(format!(
                    "invariant 5 — document {id} has stage {stage:?} and status \
                     {status:?}. Those two are written as one transaction, so nothing \
                     should be able to separate them; a `done` stage over a `pending` \
                     status is permanent, because every future walk short-circuits on \
                     the stage before it can repair the status.\n  chunks now: \
                     {chunks}\n  this content hash was settled earlier in this run and \
                     has come back: {returning}"
                ));
            }
        }
    }

    /// **6. Citations locate.**
    ///
    /// Every span a citation carries is read back out of the block it claims to
    /// quote, at the character offset it claims. This is the product's answer to
    /// the one thing the server cannot do — point at the quote it is showing —
    /// and a wrong offset is invisible in the citation's own text, which reads
    /// perfectly either way.
    ///
    /// Every offset that reaches the database is a **character** offset, and
    /// the fixtures are Ukrainian throughout, so a byte-offset implementation
    /// cannot pass this by accident.
    ///
    /// A sample rather than all of them, taken by rowid stride so it is the
    /// same sample for the same seed.
    fn check_citations_locate(&self) {
        let ids: Vec<i64> = self
            .db
            .conn()
            .prepare("SELECT id FROM chunk ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for id in stride(&ids, 8) {
            let Some(citation) = self.db.citation(*id).unwrap() else {
                self.fail(format!(
                    "invariant 6 — chunk {id} is in the index and has no citation at all"
                ));
            };
            if citation.spans.is_empty() {
                self.fail(format!(
                    "invariant 6 — chunk {id} cites nothing, so it can be shown and not \
                     located"
                ));
            }
            for span in &citation.spans {
                let Some(block) = self.db.block_text(span.block_id).unwrap() else {
                    self.fail(format!(
                        "invariant 6 — chunk {id} cites block {} which is not in the \
                         index: the block was deleted out from under a chunk that \
                         survived",
                        span.block_id
                    ));
                };
                let quoted: String = citation
                    .text
                    .chars()
                    .skip(span.start as usize)
                    .take((span.end - span.start) as usize)
                    .collect();
                let from: String = block.chars().skip(span.block_start as usize).collect();
                if !from.starts_with(&quoted) {
                    self.fail(format!(
                        "invariant 6 — chunk {id}: block {} from character {} does not \
                         begin with the {} characters the citation says came from \
                         there.\n  block:  {block:?}\n  quoted: {quoted:?}",
                        span.block_id,
                        span.block_start,
                        quoted.chars().count(),
                    ));
                }
            }
        }
    }
}

/// Up to six of a slice, always including the first and the last.
///
/// The first and the last are the two that matter: a document torn between two
/// transaction slices keeps its opening pages and loses its closing ones.
fn sample(items: &[String]) -> Vec<&String> {
    if items.len() <= 6 {
        return items.iter().collect();
    }
    let mut out = Vec::with_capacity(6);
    for k in 0..5 {
        out.push(&items[k * (items.len() - 1) / 5]);
    }
    out.push(&items[items.len() - 1]);
    out
}

fn stride<T>(items: &[T], most: usize) -> Vec<&T> {
    if items.len() <= most {
        return items.iter().collect();
    }
    let step = items.len() / most;
    items.iter().step_by(step.max(1)).take(most).collect()
}

/// Whether `relative` names something inside the subtree `prefix` names —
/// the same rule `mnema_ingest::walk::under` decides phase 3's deletions
/// with, duplicated rather than imported because it is private to that crate
/// and three lines long. Prefix-plus-separator, not a bare string prefix, so
/// `"linked_dirs/x"` does not match against a frozen prefix `"linked_dir"`.
fn under(relative: &str, prefix: &str) -> bool {
    relative
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('/'))
}

// ============================================================= the operations

impl World {
    /// One person-sized thing happening to the folder, and whatever walk
    /// follows it.
    fn step(&mut self, n: usize) {
        self.note(format!("step {n}:"));
        // Nothing has been called yet in this step, so invariant 3c has no
        // verdict of its own to judge until something makes one.
        self.calling = None;
        self.draw();
        // Every invariant again at the end of the step, including for a step
        // that never called `ingest_file` at all.
        //
        // `before` is taken **now**, which makes invariant 3 a no-op here, and
        // that is deliberate rather than a hole: it is a property of one call —
        // "this call removed content the disk still holds" — and comparing
        // across a whole step is strictly weaker. A step can legitimately
        // change a file's bytes and repoint its row, and the removal that
        // matters may be a second call's collateral damage inside the same
        // step. Measured: with the comparison at step granularity, a rebuild
        // deleting another copy's `path` row was invisible in exactly the
        // sequence written to catch it, because the row it destroyed had been
        // created earlier in the same step.
        let now = self.paths_now();
        self.check(&now);
    }

    fn draw(&mut self) {
        // Every operation that needs a file re-rolls to `create` when the
        // folder is empty, which is also how a run starts.
        let choice = if self.files.is_empty() {
            0
        } else {
            self.rng.below(30)
        };
        match choice {
            0 => self.create(),
            1 => self.edit_keeping_length(),
            2 => self.edit_changing_length(),
            3 => self.touch(),
            4 => self.copy(),
            5 => self.rename(),
            6 => self.delete_one(),
            7 => self.delete_every_copy(),
            8 => self.make_opaque(),
            9 => self.grow_past_the_ceiling(),
            10 => self.rewrite_small(),
            11 => self.reingest(),
            12 => self.lower_the_ceiling(),
            13 => self.babbling_sidecar(),
            14 => self.deadline_nothing_can_meet(),
            // Two slots, and it is the only weighting in this table. Everything
            // that reaches the rebuild in step 3 needs a document that exists
            // and is not finished, and this is the only operation that can
            // leave one — so at one slot in eighteen the whole recovery path
            // was being exercised roughly once per two hundred steps.
            15 | 16 => self.database_refuses_a_write(),
            17 => self.toggle_exclusion(),
            18 => self.simulate_ejected_volume(),
            // The two refusals by content, one slot each. They are the only
            // operations here that make `displaces` answer differently for two
            // files that are both refused on their own bytes.
            19 => self.overwrite_with_a_photo(),
            20 => self.interrupt_an_append(),
            21 => self.stricter_rule_over_an_unchanged_folder(),
            // The two the clock made unreachable. Everything above writes at a
            // tick nothing has used before, so no sequence drawn from this
            // table could return a file to a `(size, mtime)` pair the index or
            // the journal already knew, and no sequence could rewrite a file in
            // place under a ceiling that had moved. Both are ordinary things to
            // do to a folder, and both were invisible here by construction.
            22 => self.restore_a_previous_version(),
            23 => self.rewrite_in_place_under_a_lowered_ceiling(),
            // The three refusals the five readers of this cycle added. One slot
            // between them, because each is the same claim on `displaces` and
            // the corpus-coverage assertion at the end of the file is what says
            // whether one slot was enough to reach all three.
            24 => self.refuse_by_reader(),
            // The per-page journal rows. Their own slot, because they are the
            // one class in this file whose evidence lives in a *second* table
            // and is cleared by a path of its own.
            25 => self.document_with_an_unreadable_page(),
            // The rebuild machinery. Two slots, and the weighting is the same
            // argument `database_refuses_a_write` makes for its two: everything
            // downstream of a rebuild needs a rebuild to have happened, and one
            // slot in twenty-eight put the whole path at roughly once per
            // corpus.
            26 => self.the_build_learned_to_read_better(),
            27 => self.an_interrupted_rebuild_is_finished_by_the_next_pass(),
            28 => self.a_format_changes_hands(),
            _ => self.run_walk(),
        }
    }

    /// With four chances in five the walk reaches this file straight away; with
    /// the fifth it does not, and the index is left legitimately behind — which
    /// is the state invariant 2 has to tell apart from a stale answer.
    ///
    /// No exclusion check here — Task 13, fix round 2 moved it down into
    /// `ingest_measured`, the one place every caller in this file (this one
    /// included) actually reaches `ingest_file` through, rather than
    /// repeating it at each of the eight call sites a systematic probe
    /// found. See that function's own doc comment.
    fn maybe_ingest(&mut self, relative: &str) {
        if self.rng.chance(80) {
            self.ingest(relative);
        } else {
            self.note(format!("    (not walked yet: {relative})"));
        }
    }

    fn a_file(&mut self) -> String {
        let names: Vec<String> = self.files.keys().cloned().collect();
        self.rng.pick(&names).clone()
    }

    /// Moves a file's modification time without touching its bytes.
    ///
    /// Every operation that is about what happens *during* extraction has to do
    /// this first, or the file never reaches extraction: an untouched file is
    /// answered by the cheap arm from the `path` row, and a fault injected past
    /// that point is never reached.
    fn retouch(&mut self, relative: &str) {
        let at = self.next_tick();
        if let Ok(file) = std::fs::File::options()
            .write(true)
            .open(self.absolute(relative))
        {
            file.set_modified(at).unwrap();
            if let Some(state) = self.files.get_mut(relative) {
                state.mtime = at;
                // The journal's row is keyed on size and mtime, so moving the
                // mtime is enough to make the second cheap arm miss and offer
                // the file to a worker again.
                state.refused_by_content = false;
            }
        }
    }

    /// A new file, of one of the six shapes a reader can take.
    ///
    /// **The weighting is deliberate and is not uniform.** Text and markdown
    /// keep half the draws between them because they are what every other
    /// operation in this file was written against — the cheap arm's two
    /// branches, the restore, the in-place rewrite — and thinning them out would
    /// buy the new formats coverage by taking it from the old. The other half
    /// rotates through the four container readers, so a run of any length meets
    /// each of them.
    fn create(&mut self) {
        let n = self.next_counter();
        let relative = match self.rng.below(8) {
            0..=2 => format!("docs/handbook-{n}.md"),
            3 | 4 => format!("docs/file-{n}.txt"),
            5 => format!("docs/page-{n}.html"),
            6 => format!("docs/book-{n}.epub"),
            7 if self.rng.chance(50) => format!("docs/agreement-{n}.docx"),
            _ => format!("docs/budget-{n}.xlsx"),
        };
        let units = self.ordinary_units(&relative);
        let content = self.body_for(&relative, units);
        let at = self.next_tick();
        self.note(format!("  create {relative} ({units} units)"));
        self.write_at(&relative, content, at);
        self.maybe_ingest(&relative);
    }

    /// An edit that keeps the file exactly as long as it was, so the size
    /// column cannot see it and the modification time is the whole of the cheap
    /// arm's evidence.
    fn edit_keeping_length(&mut self) {
        let relative = self.a_file();
        let units = match self.files[&relative].shape {
            Shape::Text(n) | Shape::Markdown(n) | Shape::Rich(_, n) => n,
            // A file that is currently unreadable bytes has no unit count to
            // preserve; rewriting it is a different operation. A photo and a
            // zeroed tail are the same case for the same reason — neither has
            // paragraphs or sections to keep the length of, and neither does a
            // document refused for holding no words.
            // `Gappy` is here rather than beside `Rich` on purpose: its byte
            // count depends on how many degenerate entries were drawn, not on
            // its page count alone, so it has no length an edit could reproduce.
            Shape::Opaque
            | Shape::NotText
            | Shape::BinaryTail
            | Shape::Refused(_)
            | Shape::Gappy(_, _) => {
                return self.rewrite_small();
            }
        };
        let was = self.on_disk(&relative).map(|b| b.len());
        let content = self.body_for(&relative, units);
        let now = content.bytes.len();
        assert_eq!(
            was,
            Some(now),
            "the fixture generator must produce the same length for the same shape, or \
             this operation is silently the next one"
        );
        let at = self.next_tick();
        self.note(format!(
            "  edit {relative} in place, same {now} bytes, later mtime"
        ));
        self.write_at(&relative, content, at);
        self.maybe_ingest(&relative);
    }

    /// An edit that changes the length and puts the modification time **back**
    /// to what the index recorded.
    ///
    /// Not a contrived shape: `cp -p`, `rsync -a` and every archive restore
    /// carry the original modification time onto content of a different size,
    /// and the size column is the only thing that catches them.
    fn edit_changing_length(&mut self) {
        let relative = self.a_file();
        let units = 1 + self.rng.below(7);
        let content = self.body_for(&relative, units);
        if self.on_disk(&relative).map(|b| b.len()) == Some(content.bytes.len()) {
            // The draw landed on the same length; that is the operation above,
            // and running it under this name would make the trace lie.
            self.note(format!(
                "  (edit of {relative} drew its own length; skipped)"
            ));
            return;
        }

        // The modification time is preserved only when the **index's** recorded
        // size differs from the new one, and that condition is the whole of what
        // keeps this harness honest.
        //
        // The cheap arm compares against the `path` row, not against the last
        // thing this file wrote, and those two part company after two
        // mtime-preserving edits in a row: the second can land back on exactly
        // the size the index recorded, and then the file has changed while
        // nothing the product is allowed to look at has. `ingest_file`
        // answering `Unchanged` there is correct and documented — it is what
        // `slice.rs::an_unchanged_file_is_not_read_a_second_time`'s second
        // phase asserts.
        //
        // Generating that state and then calling it a stale answer is a false
        // report, and this harness produced exactly one (seed 6000011). It is
        // fixed here rather than by weakening invariant 2, because the
        // alternative is to teach the invariant what the cheap arm compares —
        // which is the product's logic re-derived in the test, and the whole
        // point of the model above is not to do that.
        //
        // **What this dodge costs is a whole state, and an unwritten dodge reads
        // like coverage, so here it is in words.** Moving the clock forward
        // exactly when the new length would have matched the recorded one is a
        // refusal to generate the third ghost: a file whose `(size, mtime)` pair
        // is what the `path` row records while its bytes are something else, so
        // the **first** cheap arm answers `Unchanged` over content it has never
        // seen. That is the same residual `displaces` names for the size ceiling
        // and `BinaryTail` names for its own head window — the product trusts
        // that pair, and every trust of it has the same hole — and this harness
        // does not test it, in either direction. It is not merely undrawn, it is
        // stepped around on purpose, because the harness has no way to tell that
        // state from the legitimate one beside it without re-deriving the arm it
        // is checking. Closing it needs something this file does not have: a
        // model of what the index was *told*, separate from what it holds.
        let recorded = self.db.path_entry(self.root_id, &relative).unwrap();
        let invisible = recorded.is_some_and(|row| row.size_bytes == content.bytes.len() as i64);
        let at = if invisible {
            self.next_tick()
        } else {
            self.files[&relative].mtime
        };
        self.note(format!(
            "  edit {relative}, {units} units, modification time {}",
            if invisible {
                "moved — the new length is the one the index recorded"
            } else {
                "preserved"
            }
        ));
        self.write_at(&relative, content, at);
        self.maybe_ingest(&relative);
    }

    fn touch(&mut self) {
        let relative = self.a_file();
        let at = self.next_tick();
        let path = self.absolute(&relative);
        if let Ok(file) = std::fs::File::options().write(true).open(&path) {
            file.set_modified(at).unwrap();
            let state = self.files.get_mut(&relative).unwrap();
            state.mtime = at;
            // Exactly what `retouch` does one screen up, and it was missing
            // here — measured on 2 paths of 200 seeds: a file refused by a
            // content rule, then touched, went on carrying a flag that says
            // "the journal still matches", while the journal row it names is
            // keyed on the mtime this line just moved.
            state.refused_by_content = false;
        }
        self.note(format!("  touch {relative}"));
        self.maybe_ingest(&relative);
    }

    /// A second copy of one file. Content addressing makes the two one
    /// document, which is what makes every deletion below interesting.
    fn copy(&mut self) {
        let relative = self.a_file();
        if let Some(copy) = self.copy_of(&relative) {
            self.maybe_ingest(&copy);
        }
    }

    /// Puts a second copy of one file's bytes at a new path and returns where,
    /// without walking either. The walk is the caller's to decide, because when
    /// it happens is the interesting part.
    fn copy_of(&mut self, relative: &str) -> Option<String> {
        let bytes = self.on_disk(relative)?;
        let n = self.next_counter();
        let copy = format!("backup/copy-{n}.{}", Self::extension_of(relative));
        let state = &self.files[relative];
        let content = Content {
            bytes,
            markers: state.markers.clone(),
            shape: state.shape,
        };
        let at = self.next_tick();
        self.note(format!("  copy {relative} -> {copy}"));
        self.write_at(&copy, content, at);
        Some(copy)
    }

    fn rename(&mut self) {
        let relative = self.a_file();
        let n = self.next_counter();
        let renamed = format!("docs/renamed-{n}.{}", Self::extension_of(&relative));
        if std::fs::rename(self.absolute(&relative), self.absolute(&renamed)).is_err() {
            return;
        }
        let mut state = self.files.remove(&relative).unwrap();
        // The flag does not travel. It says "the skip journal still refuses
        // this path", and the journal is keyed on the path: the new name has
        // no row at all, so a walk offers it to a worker like any other file.
        // Measured on 3 paths of 200 seeds, each one a renamed file the settle
        // loop then declined to check.
        state.refused_by_content = false;
        // And neither does the copy of it an earlier version carries, for the
        // same reason: restoring that version under the *new* name meets a
        // journal keyed on a path that has no row at all.
        if let Some(previous) = state.previous.as_mut() {
            previous.refused_by_content = false;
        }
        self.files.insert(renamed.clone(), state);
        self.note(format!("  rename {relative} -> {renamed}"));
        self.maybe_ingest(&renamed);
        // The old name is where a walk finds nothing, and the index still has a
        // row for it. Offering it to `ingest_file` is what a re-walk does.
        if self.rng.chance(50) {
            self.ingest(&relative);
        }
    }

    fn delete_one(&mut self) {
        let relative = self.a_file();
        let _ = std::fs::remove_file(self.absolute(&relative));
        self.files.remove(&relative);
        self.note(format!("  delete {relative}"));
        self.maybe_ingest(&relative);
    }

    /// Every path holding one file's bytes goes at once, which is the only way
    /// to make a document genuinely unreachable from the disk.
    fn delete_every_copy(&mut self) {
        let relative = self.a_file();
        let Some(hash) = self.hash_on_disk(&relative) else {
            return;
        };
        let doomed: Vec<String> = self
            .files
            .keys()
            .filter(|name| self.hash_on_disk(name).as_ref() == Some(&hash))
            .cloned()
            .collect();
        self.note(format!("  delete every copy of {relative}: {doomed:?}"));
        for name in &doomed {
            let _ = std::fs::remove_file(self.absolute(name));
            self.files.remove(name);
        }
        for name in doomed {
            self.maybe_ingest(&name);
        }
    }

    /// Someone saves a PDF over a text file. The worker reads the bytes and
    /// declines them: there is no PDF reader yet, so this is a determination
    /// about the content and the index must stop answering under that name.
    fn make_opaque(&mut self) {
        let relative = self.a_file();
        let content = self.opaque_body();
        let at = self.next_tick();
        self.note(format!(
            "  replace {relative} with bytes no reader can take"
        ));
        self.write_at(&relative, content, at);
        self.maybe_ingest(&relative);
    }

    /// A file replaced by a document some of whose pages its reader cannot
    /// read.
    ///
    /// The point is not the document — it is the **journal rows** it leaves: one
    /// per skipped page, in the same table as the file-level verdicts, cleared
    /// only by `Db::forget_page_skips`. Every later operation in the sequence
    /// then runs over a path that has them, which is how the class gets mixed
    /// into edits, copies, renames and deletions rather than tested on its own.
    fn document_with_an_unreadable_page(&mut self) {
        let relative = self.a_file();
        // **Drawn, not taken from the path's extension.** Both formats that can
        // skip a page are identified by *content*, so an epub written over
        // `file-3.txt` is still read as an epub — and keying on the extension
        // meant a gappy book needed an `.epub` path to exist first, which made
        // the whole epub half of this class rare. Measured: the mutation that
        // reverses chapter order stayed green because the conjunction it needs
        // — a gappy epub, of more than one chapter, that got indexed — did not
        // come up in the corpus at all.
        let format = if self.rng.chance(50) {
            Format::Epub
        } else {
            Format::Xlsx
        };
        // **At least two**, because a one-chapter book cannot show a chapter in
        // the wrong place: with a single page, every ordering is the same
        // ordering, and an invariant about position has nothing to bite on.
        let pages = 2 + self.rng.below(2);
        let content = self.gappy_body(format, pages);
        let at = self.next_tick();
        self.note(format!(
            "  replace {relative} with a {format:?} of {pages} readable pages and a gap"
        ));
        self.write_at(&relative, content, at);
        self.maybe_ingest(&relative);
    }

    /// A file replaced by a document that **opens** and is refused on what is
    /// inside it.
    ///
    /// The three rules here are the ones the five readers of this cycle added,
    /// and they are one operation rather than three because they differ only in
    /// which reader says no: `displaces` gives all three the identical condition
    /// (`content.is_none_or(|sha| sha != recorded.document_id)`), so what
    /// invariant 3c has to check is the same sentence three times.
    ///
    /// **Distinct from `make_opaque`, and the difference is the whole reason
    /// both exist.** That one writes a container no reader recognises —
    /// `Unsupported`, "no reader implemented yet", the rule a *release* changes.
    /// This one writes files the readers do open and then decline: a book of
    /// plates, a document of empty paragraphs, a workbook with no rows, a
    /// document cut mid-element, a PDF with a password. Neither is corrupt bytes
    /// in the sense the old generator meant, and two of the three are
    /// **structurally valid and degenerate in content** — the class both of Task
    /// 11's defects lived in, which a truncated archive cannot produce.
    fn refuse_by_reader(&mut self) {
        let relative = self.a_file();
        let refusal =
            *self
                .rng
                .pick(&[Refusal::NoTextLayer, Refusal::Malformed, Refusal::Encrypted]);
        let content = self.refused_body(refusal);
        let at = self.next_tick();
        self.note(format!("  replace {relative} with {refusal:?} content"));
        self.write_at(&relative, content, at);
        self.maybe_ingest(&relative);
    }

    /// A text file replaced by a photo — the refusal that **removes**.
    ///
    /// Distinct from `make_opaque`, which writes a PDF header and earns
    /// `Unsupported`, but not for the reason this comment used to give. It said
    /// that rule "leaves the earlier document alone", so that until this
    /// operation existed "nothing in this harness ever drove a file down the
    /// branch of `record_skip` that deletes a path row". Wrong twice, and one
    /// `grep` apart: the row is deleted by `db.delete_path` under the condition
    /// `displaces`, which is not a branch of `record_skip` at all — and
    /// `Unsupported` answers that condition, unconditionally when this was
    /// written and on changed bytes now. `make_opaque` writes different bytes,
    /// so it had been exercising that deletion all along.
    ///
    /// What is new here is the rule that reaches it: `NotText`, decided on the
    /// file's own bytes rather than on a format nobody has written a reader
    /// for.
    fn overwrite_with_a_photo(&mut self) {
        let relative = self.a_file();
        let content = self.not_text_body();
        let at = self.next_tick();
        self.note(format!("  replace {relative} with a photo"));
        self.write_at(&relative, content, at);
        self.maybe_ingest(&relative);
    }

    /// The power goes out while a note is being appended to: the prose that was
    /// already on disk stays, the tail comes back zeroed.
    ///
    /// The other side of `overwrite_with_a_photo`, and the reason the two are
    /// separate operations rather than one: both are refusals decided on the
    /// file's own bytes, and they owe the index opposite answers.
    fn interrupt_an_append(&mut self) {
        let relative = self.a_file();
        // **Only prose is kept as the prefix, and this cost a red run to
        // learn.** The shape being modelled is "a note whose append the power
        // cut short": text on disk, then zeros, refused as `BinaryTail` and
        // never displacing what the index already holds. Zeros appended to a
        // *container* are not that at all — a zip's directory is still found by
        // scanning back from the end, so the archive parses, the reader reads
        // it, and the file is indexed. The harness said `BinaryTail` and the
        // product said "an epub"; the product was right, and the model was
        // asserting a fact about a file it had not actually produced.
        //
        // Falling through to `None` makes the body bring its own prose, which is
        // what this operation has always done for a file too short to clear the
        // head window.
        let keeping = match self.files[&relative].shape {
            Shape::Text(_) | Shape::Markdown(_) => self.on_disk(&relative),
            _ => None,
        };
        let content = self.interrupted_append_body(keeping);
        let at = self.next_tick();
        self.note(format!(
            "  interrupt an append to {relative}: {} bytes, tail zeroed",
            content.bytes.len()
        ));
        self.write_at(&relative, content, at);
        self.maybe_ingest(&relative);
    }

    fn grow_past_the_ceiling(&mut self) {
        let relative = self.a_file();
        let units = Self::oversized_units(&relative);
        let content = self.body_for(&relative, units);
        assert!(
            content.bytes.len() as u64 > CEILING,
            "the oversized fixture must actually be oversized"
        );
        let at = self.next_tick();
        self.note(format!(
            "  grow {relative} to {} bytes, past the {CEILING}-byte ceiling",
            content.bytes.len()
        ));
        self.write_at(&relative, content, at);
        self.maybe_ingest(&relative);
    }

    /// Ordinary content again — the file shrinks back under the ceiling, or
    /// stops being bytes nothing can read.
    fn rewrite_small(&mut self) {
        let relative = self.a_file();
        let units = self.ordinary_units(&relative);
        let content = self.body_for(&relative, units);
        let at = self.next_tick();
        self.note(format!("  rewrite {relative} small again ({units} units)"));
        self.write_at(&relative, content, at);
        self.maybe_ingest(&relative);
    }

    fn reingest(&mut self) {
        let relative = self.a_file();
        self.note(format!("  re-walk {relative}, unchanged"));
        self.ingest(&relative);
    }

    /// A previous version of a file put back **with its own modification
    /// time** — `cp -p`, `rsync -a`, `tar -xp`, `unzip`, Time Machine, and a
    /// cloud client's "restore previous version".
    ///
    /// The one operation here that moves a file's clock backwards, and that is
    /// the whole of what it is for. [`World::next_tick`] is strictly monotonic,
    /// so before this every sequence this file could draw left each path on a
    /// `(size, mtime)` pair nothing had ever seen before. Both of
    /// `ingest_file`'s cheap arms key on exactly that pair — the first against
    /// the `path` row, the second against the skip journal — so the states
    /// where a *remembered* answer meets bytes it was not reached on were
    /// unreachable by construction, in the same way as task 8's UTF-16 tail.
    /// D47: the generator was the thing that was wrong, not the invariants.
    ///
    /// What it reaches, in three steps a person does without thinking: a file
    /// is refused on its content, something else is saved over that name and
    /// indexed, and then the refused version is restored from a backup that
    /// kept its timestamp. The journal's row matches the disk again, and what
    /// the index answers with under that name is a document built from bytes
    /// that are no longer there.
    fn restore_a_previous_version(&mut self) {
        let relative = self.a_file();
        let Some(previous) = self.files[&relative].previous.clone() else {
            self.note(format!("  (no earlier version of {relative} to restore)"));
            return;
        };
        self.note(format!(
            "  restore the previous version of {relative} ({} bytes) with its own \
             modification time",
            previous.bytes.len()
        ));
        let content = Content {
            bytes: previous.bytes,
            markers: previous.markers,
            shape: previous.shape,
        };
        self.write_at(&relative, content, previous.mtime);
        // Restored bytes at their restored time: whatever the journal recorded
        // against that pair applies again, so the flag comes back with them.
        if let Some(state) = self.files.get_mut(&relative) {
            state.refused_by_content = previous.refused_by_content;
        }
        self.maybe_ingest(&relative);
    }

    /// A file rewritten in place, keeping its exact length, under a ceiling
    /// that has moved below it.
    ///
    /// The size on disk is what `displaces` had to decide the ceiling's case
    /// on, and this is the sequence that size alone cannot answer: the file is
    /// the length the index recorded, because it is the same document rewritten
    /// rather than a longer one, and it is over the ceiling now only because
    /// the setting moved. Neither `grow_past_the_ceiling` (a different length)
    /// nor `lower_the_ceiling` (the same bytes) produces it, and between them
    /// they were the whole of what this file could say about the ceiling.
    fn rewrite_in_place_under_a_lowered_ceiling(&mut self) {
        let relative = self.a_file();
        let units = match self.files[&relative].shape {
            // The same restriction `edit_keeping_length` carries, for the same
            // reason: only a shape with a unit count has a length that can be
            // reproduced. `Rich` has one — and its archives are STORED exactly
            // so that reproducing it reproduces the byte count too.
            Shape::Text(n) | Shape::Markdown(n) | Shape::Rich(_, n) => n,
            Shape::Opaque
            | Shape::NotText
            | Shape::BinaryTail
            | Shape::Refused(_)
            | Shape::Gappy(_, _) => return,
        };
        let Some(was) = self.on_disk(&relative).map(|b| b.len()) else {
            return;
        };
        if was == 0 {
            return;
        }
        let content = self.body_for(&relative, units);
        assert_eq!(
            was,
            content.bytes.len(),
            "the fixture generator must produce the same length for the same shape, or \
             this operation is silently a file that grew"
        );
        let at = self.next_tick();
        self.note(format!(
            "  rewrite {relative} in place, same {was} bytes, then walk it under a \
             ceiling of {}",
            was - 1
        ));
        self.write_at(&relative, content, at);
        let config = PoolConfig {
            max_bytes: (was - 1) as u64,
            ..Self::config()
        };
        self.ingest_with(&relative, config, " [lowered ceiling]");
    }

    /// A ceiling lowered under a file whose bytes did not change — but whose
    /// modification time did, because that is the only way to reach the pool at
    /// all.
    ///
    /// The touch is what stops the cheap arm answering first, and since fix
    /// round 2 it is also what the answer turns on: from a refusal made without
    /// opening the file, a touch and a same-length rewrite in place are the same
    /// two numbers, so this operation now legitimately loses the document.
    /// Invariant 3 carries the narrow exception that says so, and
    /// `rewrite_in_place_under_a_lowered_ceiling` below is the case it is losing
    /// it for.
    fn lower_the_ceiling(&mut self) {
        let relative = self.a_file();
        let Some(size) = self.on_disk(&relative).map(|b| b.len() as u64) else {
            return;
        };
        if size == 0 {
            return;
        }
        self.retouch(&relative);
        self.note(format!(
            "  walk {relative} under a ceiling of {} — the file is unchanged, the \
             setting is not",
            size - 1
        ));
        let config = PoolConfig {
            max_bytes: size - 1,
            ..Self::config()
        };
        self.ingest_with(&relative, config, " [lowered ceiling]");
    }

    /// A sidecar that is not the worker, answering every request with bytes
    /// that are not UTF-8.
    ///
    /// The failure this models is environmental and applies to every file in
    /// the walk alike, which is why it is drawn against files that are
    /// otherwise perfectly readable.
    /// A release whose content rule got stricter, walking a folder that did
    /// not change. **Deliberately no `retouch`** — that is the whole question,
    /// and it is the one operation here for which touching the file first
    /// would destroy the case rather than enable it.
    ///
    /// This was measured with a throwaway probe during task 7 and left
    /// uncommitted, because it made this harness red against the code as it
    /// then stood: `displaces` answered `true` for `NotText` unconditionally,
    /// so a file whose bytes had never moved lost its document the first time
    /// anything — a `touch`, a `cp -p`, a restore, a sync client — pushed its
    /// mtime past the cheap arm. Invariant 3 caught it. Task 10 made the rule
    /// conditional on the digest, and this operation is that fix's standing
    /// witness.
    fn stricter_rule_over_an_unchanged_folder(&mut self) {
        #[cfg(unix)]
        {
            // **A file the index currently holds, and touched first.** The
            // keep side of invariant 3c is reached only when the worker reports
            // a digest byte-identical to what the document was built from —
            // "the rule changed, the file did not" — and two things kept that
            // from happening reliably: an arbitrary file may hold no document
            // at all, and an untouched one is answered by a cheap arm before
            // any worker runs (measured by an earlier round at 72 refusals in
            // 186 calls). Touching moves the mtime and leaves the bytes, which
            // is exactly the shape being modelled. Rotating the rule alone was
            // not enough — `malformed` and `encrypted` stayed unreached — and a
            // rule the corpus cannot reach looks like a rule with no defects.
            let indexed: Vec<String> = self.paths_now().into_keys().collect();
            let relative = if indexed.is_empty() {
                self.a_file()
            } else {
                self.rng.pick(&indexed).clone()
            };
            self.retouch(&relative);
            // **All four content rules, not just `not_text`.** This is the only
            // operation in the file that produces "the rule changed and the
            // file did not" — a refusal whose digest is byte-identical to what
            // the index built its document from — and that is precisely the
            // branch of invariant 3c that says the document must be **kept**.
            // With one rule here, three of `displaces`'s five conditional arms
            // were judged on their displace side only: measured, mutating
            // `NoTextLayer`, `Malformed` and `Encrypted` to `true` left the
            // whole harness green. Writing new bytes cannot reach it — a new
            // body has a new digest — so the rule has to move while the file
            // stands still.
            // **Rotated, not drawn.** A one-in-four pick left `malformed`
            // unreached across the whole default corpus — measured, its
            // mutation stayed green while the other three reddened — and a rule
            // the corpus happens not to draw looks exactly like a rule with no
            // defects. The rotation is per run and deterministic, so four calls
            // to this operation cover all four rules and a seed still decides
            // which files they land on.
            const STRICTER: [&str; 4] = ["not_text", "no_text_layer", "malformed", "encrypted"];
            let rule = STRICTER[self.stricter_rotation % STRICTER.len()];
            self.stricter_rotation += 1;
            let stricter = stricter_worker(self.dir.path(), rule);
            self.note(format!(
                "  walk {relative} past a STRICTER content rule ({rule})"
            ));
            let verdict =
                self.ingest_with(&relative, PoolConfig::new(&stricter), " [stricter rule]");
            // The refusal is journalled against this file's current size and
            // mtime, and `SkipRule::NotText.is_about_content()` is true — so a
            // later walk answers from the journal without asking a worker, and
            // no number of clean passes brings this path back on its own.
            // Recorded so that `settle` does not expect it to.
            //
            // Two things do bring it back, and the flag deliberately does not
            // model either — `FileState::refused_by_content` has why: a live
            // `path` row the rule would remove sends the file to a worker
            // anyway, and a successful index clears the journal row outright.
            //
            // **On the verdict this call returned, not on the condition that
            // it was offered.** The stricter worker is only reached when the
            // walk gets that far, and `ingest_file` answers most of these
            // offers from a cheap arm first: measured over 200 seeds, of 186
            // calls here only 72 were a refusal by content — 77 were
            // `Unchanged`, 15 were answered from an older journal row, 22 were
            // excluded. The flag was being set on all of them, and `settle`
            // steps over every path carrying it, so the settling check was
            // switched off on 29 paths that had never been refused at all —
            // ordinary indexed text files among them, which is the case this
            // harness exists to check.
            // On whichever of the four actually came back, and on nothing else:
            // all four answer `is_about_content`, so all four freeze the path
            // the same way.
            if matches!(
                verdict,
                Some(Verdict::Skipped(
                    SkipRule::NotText
                        | SkipRule::NoTextLayer
                        | SkipRule::Malformed
                        | SkipRule::Encrypted
                ))
            ) && let Some(state) = self.files.get_mut(&relative)
            {
                state.refused_by_content = true;
            }
        }
        #[cfg(not(unix))]
        self.reingest();
    }

    /// **The build learned to read better**: the same file, the same digest, a
    /// different reader version.
    ///
    /// This is the operation the whole task exists for. `ingest_file` compares
    /// the reader and version recorded on the `path` row against the one the
    /// worker just announced (`crates/mnema-ingest/src/lib.rs`'s `stale_reading`),
    /// and when they differ it **rebuilds** the document rather than answering
    /// `AlreadyIndexed` — clearing the old chunks, putting `document.status`
    /// back to `pending` so the document answers no search while it is being
    /// written, and passing `ingest_stage.status` through `rebuilding`.
    ///
    /// Nothing in this file could reach that before: a rebuild needs a worker
    /// that **succeeds**, and every sidecar here refused.
    ///
    /// Restricted to prose, and that is a property of the sidecar rather than
    /// of the product: it re-emits the file's own lines as JSON, so a format
    /// whose bytes are markup or an archive would need escaping this shell
    /// script has no business doing. The rebuild machinery is per-document and
    /// does not care which reader produced it.
    fn the_build_learned_to_read_better(&mut self) {
        #[cfg(unix)]
        {
            let candidates: Vec<String> = self
                .paths_now()
                .into_keys()
                .filter(|relative| {
                    matches!(
                        self.files.get(relative).map(|state| state.shape),
                        Some(Shape::Text(_)) | Some(Shape::Markdown(_))
                    )
                })
                .collect();
            if candidates.is_empty() {
                return;
            }
            let relative = self.rng.pick(&candidates).clone();
            // The mtime has to move or the first cheap arm answers `Unchanged`
            // from the `path` row and no worker runs at all — the version can
            // only be compared by a pass that got as far as asking one.
            self.retouch(&relative);
            // **Two ways for a reading to be stale, and the harness has to
            // reach both.** `stale_reading` is `reader != recorded.reader ||
            // reader_version != recorded.reader_version`, and a corpus that
            // only ever changes the version judges half of it: measured, the
            // mutation removing the *name* comparison stayed green, because
            // every pass that changed the name changed the version too.
            //
            // A markdown file offered by the *text* reader **at version 1** is
            // the name half on its own, and it is not a contrivance — it is what
            // `.html` did inside this cycle when it left the text reader for the
            // html one.
            // Never 1: a version equal to the recorded one is not a better
            // reader, it is the same one. The **name** half of `stale_reading`
            // is `a_format_changes_hands`, which has to build its own file to
            // reach it — see there.
            // **Its own counter, advanced here.** This read `stricter_rotation`
            // — a counter owned by the stricter-rule operation and never
            // advanced by this one — so within a run the offered version stood
            // still until that unrelated operation happened to fire. Two draws
            // over one file then offered a version already recorded, answered
            // `AlreadyIndexed`, and the operation quietly did nothing. Two
            // independent operations were tied together through a shared field,
            // and nothing said so.
            self.rebuild_rotation += 1;
            let (reader, version, state) =
                ("text", 2 + (self.rebuild_rotation % 3) as u32, "rebuilt");
            let better = better_reader_worker(self.dir.path(), reader, version);
            self.note(format!(
                "  walk {relative} past a build whose {reader} reader is at version {version}"
            ));
            let verdict = self.ingest_with(
                &relative,
                PoolConfig::new(&better),
                &format!(" [{reader} reader v{version}]"),
            );
            // **Asserted, not assumed.** The point of the operation is that the
            // same bytes under a new reader version are *rebuilt*; if this ever
            // answers `AlreadyIndexed` the operation is a no-op and every
            // invariant below it is judging an ordinary walk.
            // **`Indexed`, not `Settled`** — and the difference is the whole
            // operation. `Verdict::Settled` folds `Indexed`, `Unchanged` and
            // `AlreadyIndexed` together, so recording the class on it marked
            // the rebuild as reached even when the pass had *confirmed* the
            // document instead of rebuilding it. Measured: the mutation that
            // makes the sidecar announce version 1 — no version change, no
            // rebuild, by construction — left the corpus assertion green.
            // A class recorded when it did not happen is worse than one that is
            // never recorded, because it reports coverage rather than absence.
            // `verdict.is_some()` first: an excluded path is never offered, and
            // `self.last` then still holds **an earlier call's** entry for it —
            // which is how this could record a rebuild the operation never
            // performed.
            if verdict.is_some() && self.last.get(&relative).is_some_and(|last| last.indexed) {
                self.reached.states.insert(state);
            }
        }
    }

    /// **A format changes hands**: the same file, the same version, a different
    /// *reader*.
    ///
    /// `stale_reading` is two comparisons — `reader != recorded.reader ||
    /// reader_version != recorded.reader_version` — and until this operation
    /// existed the corpus only ever moved the second. Measured: the mutation
    /// that deletes the **name** comparison stayed green, because every pass
    /// that changed the name changed the version with it.
    ///
    /// It is not a contrivance either. `.html` did exactly this inside this
    /// cycle: it was read by the text reader, was recorded as `text@1` in every
    /// index built before, and moved to a reader of its own. The version did not
    /// have to change for the reading to be stale.
    ///
    /// **Builds its own file, and that is the whole reason it is a separate
    /// operation.** The name half is only reachable when the recorded version
    /// equals the offered one, and a file this corpus has already rebuilt is
    /// recorded at 2 or above — offering version 1 over *that* is a version
    /// change again, and the case tests nothing. A fresh markdown file indexed
    /// by the real worker is recorded `markdown@1` exactly.
    fn a_format_changes_hands(&mut self) {
        #[cfg(unix)]
        {
            let n = self.next_counter();
            let relative = format!("docs/handover-{n}.md");
            let units = 1 + self.rng.below(3);
            let content = self.markdown_body(units);
            let at = self.next_tick();
            self.note(format!("  create {relative} for a reader handover"));
            self.write_at(&relative, content, at);
            self.ingest(&relative);
            if !self.paths_now().contains_key(&relative) {
                // Excluded, or never offered; there is nothing recorded to go
                // stale against.
                return;
            }

            self.retouch(&relative);
            // The text reader at the version markdown is already recorded at, so
            // the **only** difference is which reader read it.
            let better = better_reader_worker(self.dir.path(), "text", 1);
            self.note(format!(
                "  walk {relative} past a build where .md is read by text@1"
            ));
            let verdict =
                self.ingest_with(&relative, PoolConfig::new(&better), " [text reader v1]");
            if verdict.is_some() && self.last.get(&relative).is_some_and(|last| last.indexed) {
                self.reached.states.insert("reader-changed-hands");
            }
        }
    }

    /// A rebuild that is cut off **between two slices**, and the pass after it.
    ///
    /// `PAGES_PER_TRANSACTION` is 20 and the sidecar emits one page per line, so
    /// a file of more than twenty paragraphs is written in more than one
    /// transaction. Aborting the second leaves the document half-written: the
    /// two invariants this task exists for are that such a document answers no
    /// search **at all** rather than with the half that landed, and that the
    /// next pass finishes it rather than reading the leftover state as
    /// `Unchanged`.
    ///
    /// Both already cost a Critical once, around the `AlreadyIndexed` branch and
    /// slice 0's commit.
    fn an_interrupted_rebuild_is_finished_by_the_next_pass(&mut self) {
        #[cfg(unix)]
        {
            // A document long enough to be cut, written fresh so that the pass
            // below is a rebuild of something the index already holds.
            let n = self.next_counter();
            let relative = format!("docs/long-{n}.txt");
            let units = mnema_ingest::PAGES_PER_TRANSACTION + 2 + self.rng.below(4);
            let content = self.text_body(units);
            let at = self.next_tick();
            self.note(format!(
                "  create {relative} ({units} paragraphs) for a cut rebuild"
            ));
            self.write_at(&relative, content, at);
            self.ingest(&relative);

            self.retouch(&relative);
            self.rebuild_rotation += 1;
            let version = 2 + (self.rebuild_rotation % 3) as u32;
            let better = better_reader_worker(self.dir.path(), "text", version);

            // The abort lands on a page the *second* slice writes, so slice 0 is
            // committed and the document is genuinely half-written — which is
            // the state, not merely a failed write.
            self.db
                .conn()
                .execute_batch(
                    "CREATE TRIGGER forced_failure BEFORE INSERT ON page                      WHEN new.page_no > 20 BEGIN                          SELECT RAISE(ABORT, 'forced failure');                      END;",
                )
                .unwrap();
            self.note(format!(
                "  rebuild {relative} at v{version}, cut between slices"
            ));
            let cut = self.ingest_with(
                &relative,
                PoolConfig::new(&better),
                &format!(" [text reader v{version}, cut]"),
            );
            self.db
                .conn()
                .execute_batch("DROP TRIGGER forced_failure")
                .unwrap();

            // **The pass after the interruption.** It must read the file again
            // and finish the document; answering `Unchanged` would mean the
            // interrupted pass left behind a `path` row the cheap arm trusts,
            // and the half-written document would stay half-written for ever.
            let verdict = self.ingest_with(
                &relative,
                PoolConfig::new(&better),
                &format!(" [text reader v{version}, resuming]"),
            );
            if verdict.is_none() {
                // Excluded between the two passes; nothing was offered, so
                // there is nothing to assert about what an offer answered.
                return;
            }

            // **Only when the cut pass really was cut.** The `else` below
            // accuses the product of leaving an `Unchanged`-able state, and
            // that accusation is only true if there was an interruption to
            // leave one — if the trigger never fired, the rebuild simply
            // finished and `AlreadyIndexed` is the correct answer to the next
            // offer. Measured: two mutations that stop the interruption
            // (the trigger's page number, the document's length) reddened
            // through this `else`, reporting a half-written document when
            // nothing had been half-written. The honest signal for those is
            // the corpus assertion finding `rebuild-resumed` unreached, and
            // this guard is what leaves it to say so.
            if cut != Some(Verdict::Failed) {
                return;
            }

            if self.last.get(&relative).is_some_and(|last| last.indexed) {
                // **This document, not any document.** The first version asked
                // whether *some* row in the index was finished, which the
                // corpus almost always has — so `!finished` was never true and
                // the message reported on something else entirely. An assertion
                // satisfied by an unrelated row, inside the instrument built to
                // catch exactly that.
                let document = self.paths_now().get(&relative).cloned();
                let finished = document.as_deref().is_some_and(|id| {
                    self.documents_now().get(id).is_some_and(|(status, stage)| {
                        status == "indexed" && stage.as_deref() == Some("done")
                    })
                });
                if !finished {
                    self.fail(format!(
                        "D64 invariant 2 — the pass after an interrupted rebuild of {relative} \
                         settled and the document it names ({document:?}) is still not \
                         finished. An interrupted pass must not leave a state the next walk \
                         reads as done"
                    ));
                }
                self.reached.states.insert("rebuild-resumed");
            } else {
                self.fail(format!(
                    "D64 invariant 2 — the rebuild of {relative} was cut between slices and \
                     the pass after it did not read the file again ({verdict:?}). An \
                     interrupted pass must not leave a state the next walk answers \
                     `Unchanged` from: the document would stay half-written for ever, and \
                     every later walk would agree it was fine"
                ));
            }
        }
    }

    fn babbling_sidecar(&mut self) {
        #[cfg(unix)]
        {
            let relative = self.a_file();
            self.retouch(&relative);
            let broken = babbling_worker(self.dir.path());
            self.note(format!(
                "  walk {relative} past a worker that is not the worker"
            ));
            self.ingest_with(&relative, PoolConfig::new(&broken), " [babbling sidecar]");
        }
        #[cfg(not(unix))]
        self.reingest();
    }

    /// A deadline no process can meet, which is a fact about how loaded the
    /// machine is rather than about the file.
    fn deadline_nothing_can_meet(&mut self) {
        let relative = self.a_file();
        self.retouch(&relative);
        self.note(format!(
            "  walk {relative} against a deadline of one nanosecond"
        ));
        let config = PoolConfig {
            timeout: Duration::from_nanos(1),
            ..Self::config()
        };
        self.ingest_with(&relative, config, " [impossible deadline]");
    }

    /// A write that fails, at a point in the sequence the draw chooses.
    ///
    /// The conditional faults are the interesting ones. `new.ord >= k` lets a
    /// document's chunks land and then stops — and when `k` falls past the
    /// first transaction slice, what is left behind is a **committed** partial
    /// document, which no unconditional trigger can produce because it rolls
    /// slice 0 back along with everything else.
    fn database_refuses_a_write(&mut self) {
        let relative = self.a_file();

        // The content comes first, because which faults can even fire depends
        // on the document about to be written, and drawing them independently
        // wastes most of the draws. `INSERT page WHEN new.page_no > 20` on a
        // one-page text file is not a fault at all — the trigger never fires
        // and the whole step is an ordinary successful walk.
        //
        // The file also has to be given *new bytes* rather than a new
        // modification time, and that was measured rather than reasoned: a
        // touched file still hashes to the document the index holds, so step 3
        // answers `AlreadyIndexed` and returns before a row of a document is
        // written. With only a touch here the rebuild path was reached once in
        // sixteen hundred steps.
        let mut units = 0;
        if self.rng.chance(80) {
            units = if relative.ends_with(".md") && self.rng.chance(50) {
                // Deliberately past `PAGES_PER_TRANSACTION`, so the write loop
                // cuts this document into more than one transaction and a
                // failure can land between two of them.
                mnema_ingest::PAGES_PER_TRANSACTION + 2 + self.rng.below(6)
            } else {
                self.ordinary_units(&relative)
            };
            let content = self.body_for(&relative, units);
            let at = self.next_tick();
            self.write_at(&relative, content, at);
        } else {
            // The remaining fifth only touches it, because the short path
            // through step 3 has writes of its own worth breaking: `repoint`
            // moves a `path` row even when nothing else changes.
            self.retouch(&relative);
        }

        // Two tables, and the split is the point.
        //
        // Only the **torn** ones can leave a document committed and unfinished,
        // which is the one state the rest of this file cannot produce and the
        // one every rebuild sequence needs. The others abort inside a
        // transaction and roll the whole document back — the easy case, kept
        // because `record_skip`'s atomicity and `repoint`'s rollback are worth
        // breaking too, but not what this operation is mainly for.
        //
        // A document of more than one slice admits the two conditional faults,
        // whose threshold is then placed inside the range that document really
        // covers rather than guessed. A document of one slice admits only the
        // broken checkpoint: everything before it is a single transaction, so
        // any failure inside takes the whole document with it.
        let multi_slice = units > mnema_ingest::PAGES_PER_TRANSACTION;
        let torn: Vec<(&str, &str, String)> = if multi_slice {
            let past = mnema_ingest::PAGES_PER_TRANSACTION;
            vec![
                (
                    "INSERT",
                    "chunk",
                    format!("new.ord >= {}", past + self.rng.below(3)),
                ),
                (
                    "INSERT",
                    "page",
                    format!("new.page_no > {}", past + self.rng.below(3)),
                ),
                ("UPDATE", "document", String::new()),
            ]
        } else {
            vec![("UPDATE", "document", String::new())]
        };
        let rolled_back: [(&str, &str, String); 6] = [
            ("INSERT", "chunk", String::new()),
            ("INSERT", "block", String::new()),
            ("INSERT", "chunk_search", String::new()),
            ("INSERT", "path", String::new()),
            ("DELETE", "path", String::new()),
            ("DELETE", "page", String::new()),
        ];
        let (event, table, when) = if self.rng.chance(70) {
            self.rng.pick(&torn).clone()
        } else {
            self.rng.pick(&rolled_back).clone()
        };
        self.note(format!(
            "  walk {relative} against a database that refuses a write"
        ));
        self.ingest_with_broken_database(&relative, event, table, &when);

        // What happens next is the whole value of this operation. Both
        // follow-ups matter and they are mutually exclusive, which is why they
        // are drawn together rather than each rolled separately:
        //
        // * walking the same file again is the recovery, and the common case;
        // * **copying it first** is the one that puts a second `path` row on an
        //   unfinished document — and the rebuild then has to clear that
        //   document's content without taking the other copy's row with it,
        //   through a foreign key that cascades. A harness that always repaired
        //   first can never build that state, which is what the first version
        //   of this file did.
        //
        match self.rng.below(10) {
            0..=3 => self.ingest(&relative),
            4..=8 => {
                let copy = self.copy_of(&relative);
                if let Some(copy) = copy {
                    // Either end of the pair may be the one the walk reaches
                    // first, and the rebuild runs on whichever it is.
                    if self.rng.chance(50) {
                        self.ingest(&copy);
                    } else {
                        self.ingest(&relative);
                    }
                }
            }
            _ => {}
        }
    }

    /// RunWalk: `walk_root` itself, over the real folder and the real rules —
    /// enumerate, ingest, reconcile, exactly as the product runs it.
    ///
    /// Every other operation in this file offers `ingest_file` one path at a
    /// time, which is a model of what a walk does to a single file but has
    /// never once called the function that enumerates a folder or the one
    /// that deletes from it. This is the operation that closes that gap: the
    /// gap between one file's measurement and its own ingest that Task 5
    /// closed is exercised for real here too, because `walk_root`'s own two
    /// phases open exactly that gap and close it exactly the way the product
    /// does — nothing in this file re-derives it any more.
    fn run_walk(&mut self) {
        self.note(format!("  RunWalk over {}", self.root.display()));
        let before = self.paths_now();
        let cancel = AtomicBool::new(false);
        let report = walk_root(
            &self.pool,
            &self.db,
            self.root_id,
            &self.root,
            &self.rules,
            &cancel,
            &mut |_| {},
        )
        .unwrap();
        self.note(format!(
            "    walk_root -> found {} indexed {} unchanged {} skipped {} refused {} \
             removed {} frozen {} complete {} stopped {:?}",
            report.found,
            report.indexed,
            report.unchanged,
            report.skipped,
            report.refused,
            report.removed,
            report.frozen.len(),
            report.complete,
            report.stopped,
        ));
        if report.stopped == StopReason::Completed {
            self.record_settlement_from_walk();
        }
        let after = self.paths_now();
        // `walking` brackets only this check — see its own doc comment on
        // `World` for why invariant 3's exclusion exception must not outlive
        // it.
        self.walking = true;
        self.check(&before);
        self.check_walk_removed_what_it_could(&before, &after, &report);
        self.walking = false;
        self.remember_settled();
    }

    /// After a walk whose phase 2 ran to completion, every file this run
    /// still believes exists — other than one a rule excludes, which the
    /// walk never offered to `ingest_file` at all — genuinely was offered to
    /// it, exactly once, with its own measurement. Unlike every other caller
    /// in this file, `walk_root` returns no per-path outcome to record — only
    /// the aggregate counts on [`WalkReport`] — so `self.last` is brought up
    /// to date by reading the index back afterwards rather than by keeping a
    /// return value, which is safe here specifically because nothing in this
    /// single-threaded harness can change a file's bytes while `walk_root` is
    /// running: whatever `hash_on_disk` reads immediately after the call is
    /// the same bytes phase 2 just measured.
    fn record_settlement_from_walk(&mut self) {
        let after = self.paths_now();
        let documents = self.documents_now();
        let names: Vec<String> = self.files.keys().cloned().collect();
        for relative in names {
            if self.excluded.contains(&relative) {
                // The walk never offered this path to `ingest_file` at all —
                // leaving `self.last` exactly as it was is what stops
                // invariant 2b from reading a rule's own removal as data
                // loss (`toggle_exclusion` marks it `Skipped` at the moment
                // of exclusion for the same reason).
                continue;
            }
            let Some(hash) = self.hash_on_disk(&relative) else {
                continue;
            };
            let verdict = match after.get(&relative) {
                Some(document) if document == &hash && self.is_settled(&documents, document) => {
                    Verdict::Settled
                }
                _ => Verdict::Unoffered,
            };
            self.last.insert(
                relative,
                LastCall {
                    hash: Some(hash),
                    verdict,
                    // Synthesised by the harness rather than answered by a
                    // call: nothing read the file, so nothing owed the
                    // per-page rows a clean-up.
                    indexed: false,
                },
            );
        }
    }

    /// Excludes one currently-included file by its own exact path, or lifts
    /// an exclusion already in force — "I excluded that folder" tested at the
    /// single-file grain (§5), which is all `WalkRules` needs handed a
    /// well-formed prefix: a whole relative path is as valid a rule as a
    /// directory one, and it never reaches into whatever else its own
    /// directory holds the way excluding `docs` outright would.
    fn toggle_exclusion(&mut self) {
        if !self.excluded.is_empty() && self.rng.chance(50) {
            let choices: Vec<String> = self.excluded.iter().cloned().collect();
            let relative = self.rng.pick(&choices).clone();
            self.excluded.remove(&relative);
            self.note(format!("  stop excluding {relative}"));
            // What the index says about it is unknown again until a walk
            // offers it — removing it from `excluded` alone changed nothing
            // on disk or in the database.
            self.last.remove(&relative);
        } else {
            let relative = self.a_file();
            self.note(format!("  exclude {relative}"));
            // The exclusion takes effect on the NEXT walk (§5's own words),
            // not this instant — until then the index may still legitimately
            // answer for it. Marked `Skipped` rather than left alone: an
            // entry still reading `Settled` here would make invariant 2b
            // read the walk that later removes it as data loss, because as
            // far as that invariant can tell nothing about the file changed.
            if let Some(hash) = self.hash_on_disk(&relative) {
                self.last.insert(
                    relative.clone(),
                    LastCall {
                        hash: Some(hash),
                        verdict: Verdict::Unoffered,
                        indexed: false,
                    },
                );
            }
            self.excluded.insert(relative);
        }
        self.rebuild_rules();
    }

    /// The unmount signature (D33): every entry directly under the watched
    /// root gone from a raw listing, with the index still holding paths
    /// under it.
    ///
    /// Nothing else in this file can produce that shape on its own. Every
    /// other operation only ever adds or removes a *file* — `docs/` and
    /// `backup/` are created once by [`World::new`] and nothing here ever
    /// removes them, so the root's own top-level listing never empties by
    /// itself, and `walk_root`'s own guard against this (`root_is_empty`,
    /// `crates/mnema-ingest/src/walk.rs`) would otherwise go the whole run
    /// unexercised. This operation is the generator fix Task 13's own brief
    /// calls for when a targeted mutation does not go red on its own: it
    /// moves the watched root aside, walks an empty directory standing in
    /// for it, and puts everything straight back before any invariant runs —
    /// as close as one process can come to an external drive being
    /// unplugged and reconnected between two walks of the same folder.
    ///
    /// Deliberately does not assert `report.stopped` or `report.removed`
    /// directly: `self.check(&before)` below, run once the root is back and
    /// every file on it reads exactly as it did before this ran, is what
    /// invariant 3 is for — a path row missing here over content the disk
    /// (now restored) still holds is exactly the shape invariant 3 already
    /// exists to catch, so this operation only has to put the harness back
    /// into a state that invariant can see, not repeat what it checks.
    ///
    /// Does not call `check_walk_removed_what_it_could` (invariant 3b) the
    /// way `run_walk` does, and that is not an oversight: by the time
    /// `self.check` below runs, `before` and the restored disk agree on
    /// every path again, so nothing in `before` reads as "gone from disk" —
    /// invariant 3b would have nothing to say. This operation's own claim is
    /// the over-deletion direction (invariant 3), never the under-deletion
    /// one 3b is for.
    fn simulate_ejected_volume(&mut self) {
        let before = self.paths_now();
        let parked = self.dir.path().join("parked");
        self.note(format!(
            "  eject the volume: {} reads empty for one walk",
            self.root.display()
        ));
        std::fs::rename(&self.root, &parked).unwrap();
        std::fs::create_dir_all(&self.root).unwrap();
        let cancel = AtomicBool::new(false);
        let report = walk_root(
            &self.pool,
            &self.db,
            self.root_id,
            &self.root,
            &self.rules,
            &cancel,
            &mut |_| {},
        )
        .unwrap();
        std::fs::remove_dir(&self.root).unwrap();
        std::fs::rename(&parked, &self.root).unwrap();
        self.note(format!(
            "  reconnect it: walk_root answered removed {} stopped {:?}",
            report.removed, report.stopped
        ));
        self.walking = true;
        self.check(&before);
        self.walking = false;
    }
}

// ============================================================== the aftermath

impl World {
    /// Walks the folder with nothing broken until the index stops changing, and
    /// then asks for everything.
    ///
    /// This is where the properties that are allowed to be false *for a while*
    /// are collected. A torn write may leave a half-built document; a skip may
    /// leave the index behind the disk. None of that may be **permanent**: a
    /// walk over an undamaged machine has to bring every readable file up to
    /// date, and a state that survives this one is a state the product can
    /// never leave.
    ///
    /// It is the check that catches a document parked at `pending` under a
    /// `done` stage — the cheap arm answers `Unchanged` for it every time, so
    /// nothing here would notice except by asking whether it is finished.
    fn settle(&mut self) {
        let names: Vec<String> = self.files.keys().cloned().collect();
        for pass in 0..3 {
            self.note(format!("settling, pass {pass}:"));
            for name in &names.clone() {
                if self.excluded.contains(name) {
                    // A rule, not a fact about the file's own bytes —
                    // `ingest_file` has no notion of it at all, so calling it
                    // here would index straight through the exclusion this
                    // run drew on purpose. Reconciling it is `run_walk`'s
                    // business, below.
                    continue;
                }
                self.ingest(name);
            }
            // Individual `ingest_file` calls, above, never remove a path row
            // for a reason other than its own content (§7 is `walk_root`'s
            // alone) — so a file this run deleted or excluded and never
            // walked again stays in the index until a real walk reconciles
            // it. One clean `RunWalk` per pass is what a person leaving the
            // window open would eventually get for free, and what makes the
            // final loop below able to expect an excluded file gone.
            self.run_walk();
        }

        let after = self.paths_now();
        let documents = self.documents_now();
        for name in &names {
            if self.excluded.contains(name) {
                // `contains_key`, not a match against the current hash: a
                // stale row still pointing at some OLD document would pass
                // the hash comparison (fix round 1, Minor) and hide behind
                // invariant 3b, which already backstops this exact claim —
                // this check should not depend on it to be right.
                if after.contains_key(name) {
                    self.fail(format!(
                        "after settling — {name} is excluded by a rule and still has a path \
                         row: three clean walks did not remove what the rule says must not be \
                         findable"
                    ));
                }
                continue;
            }
            let Some(hash) = self.hash_on_disk(name) else {
                continue;
            };
            let size = self.on_disk(name).map(|b| b.len()).unwrap_or(0) as u64;
            let state = &self.files[name];
            // A path carrying a content refusal the journal still matches is
            // outside this loop's question in **both** directions, so nothing
            // is asserted about it here rather than a weaker thing being
            // asserted. It may legitimately be absent — the second cheap arm
            // answers from the journal and no walk asks a worker again — and
            // it may legitimately be present, because since task 10 a refusal
            // on unchanged bytes keeps the document it already had. What
            // happened at the moment the rule fired is invariant 3c's
            // business, and that ran then, with the digest in hand.
            if state.refused_by_content {
                continue;
            }
            // Four kinds of file are legitimately not in the index at the end:
            // one no reader can take, a photo, a note whose append was
            // interrupted, and one over the ceiling. All four are journalled.
            //
            // `== Some(&hash)` and not `is_some()`, and the difference is the
            // whole reason `Shape::BinaryTail` can be in this list: that file's
            // path row legitimately still names the document it named *before*
            // the damage. What must not happen is the index holding the
            // damaged bytes themselves as a document, which is what comparing
            // against `hash` says. Invariant 3c is what checks the other half
            // — that the earlier document is still there.
            let refused = match state.shape {
                Shape::Opaque => Some("a container no reader here recognises"),
                Shape::NotText => Some("a photo"),
                Shape::BinaryTail => Some("text that stops being text partway through"),
                Shape::Refused(Refusal::NoTextLayer) => Some("a document holding no words"),
                Shape::Refused(Refusal::Malformed) => Some("a document whose structure is damaged"),
                Shape::Refused(Refusal::Encrypted) => Some("a password-protected document"),
                // A gappy document *is* indexed — the pages it could read are
                // there, and the ones it could not are journalled by number.
                Shape::Text(_) | Shape::Markdown(_) | Shape::Rich(_, _) | Shape::Gappy(_, _) => {
                    None
                }
            };
            if refused.is_some() || size > CEILING {
                if after.get(name) == Some(&hash) {
                    self.fail(format!(
                        "after settling — {name} is in the index although it is \
                         {} and must have been journalled instead",
                        refused.unwrap_or("over the size ceiling")
                    ));
                }
                continue;
            }
            match after.get(name) {
                Some(document) if document == &hash => {
                    if !self.is_settled(&documents, document) {
                        self.fail(format!(
                            "after settling — {name} is indexed as {document}, and that \
                             document is {:?} with stage {:?}. Three clean walks did not \
                             finish it, so nothing ever will: the cheap arm answers from \
                             the stage before the repair in step 3 can be reached",
                            documents.get(document).map(|d| d.0.clone()),
                            documents.get(document).and_then(|d| d.1.clone()),
                        ));
                    }
                    for marker in sample(&state.markers) {
                        let hits = self.db.search_lexical(marker, 20).unwrap();
                        let found = hits
                            .iter()
                            .filter_map(|id| self.document_of_chunk(*id))
                            .any(|id| &id == document);
                        if !found {
                            self.fail(format!(
                                "after settling — the word {marker:?} is in {name} and \
                                 answers to no chunk of the document the index holds for \
                                 it. Three clean walks did not restore it"
                            ));
                        }
                    }
                }
                other => self.fail(format!(
                    "after settling — {name} is a readable file under the ceiling and \
                     the index holds {other:?} for it rather than {hash}. A folder that \
                     is walked to a standstill must contain no file the index has \
                     forgotten"
                )),
            }
        }
    }
}

// ==================================================================== the run

fn setting(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

fn run(seed: u64, steps: usize) -> Reached {
    let mut world = World::new(seed);
    for n in 1..=steps {
        world.step(n);
    }
    world.settle();
    world.reached
}

/// What a run actually produced, as opposed to what its generator can produce.
///
/// **This exists because a class the generator never reaches looks exactly like
/// a class with no defects.** The whole of `Unsupported` sat unreachable behind
/// a `%PDF-` stub for two cycles while every seed stayed green, and nothing in
/// this file could have said so: the invariants only ever judge what happened.
/// A run that meets no workbook is not evidence about workbooks, and after this
/// it cannot be mistaken for evidence about workbooks.
///
/// It is an assertion about the **corpus**, not about the product, so it is
/// checked once over all seeds rather than per seed — a single run of forty
/// steps has no business meeting every format.
#[derive(Default)]
struct Reached {
    /// Every `Shape` the generator wrote to disk, by its own name.
    shapes: BTreeSet<&'static str>,
    /// Every rule a worker actually answered with, by the string the journal
    /// stores — `SkipRule::as_str`, not `Debug`, so the coverage list names the
    /// same value the `skip` table holds and a renamed variant cannot quietly
    /// turn this into an assertion that can never pass.
    rules: BTreeSet<&'static str>,
    /// Every reader that produced a document the index kept.
    readers: BTreeSet<String>,
    /// Which states of the **rebuild** machinery this run actually drove the
    /// product through.
    ///
    /// The dimension Task 14's corpus assertion did not have, and the reason
    /// this is a task rather than a line in a report: `shapes`, `rules` and
    /// `readers` all passed at full strength while containing no entry for the
    /// riskiest machinery on the branch. A check written to expose an unreached
    /// class was reporting success over one.
    states: BTreeSet<&'static str>,
    /// How many times a settled call found the journal holding a per-page row
    /// **under this run's watched root** — an observation count, not a set of
    /// page numbers, and the doc comment said the wrong one of those.
    ///
    /// Only ever read as `> 0`, and it must stay that way: the same row is
    /// counted again by every later settled call, so the total is a number
    /// about the run's shape rather than about the index. Anything sharper —
    /// "exactly three pages were skipped" — belongs in invariant 3e, which
    /// reads the numbers themselves.
    page_skips: usize,
}

impl Reached {
    fn merge(&mut self, other: Reached) {
        self.shapes.extend(other.shapes);
        self.rules.extend(other.rules);
        self.readers.extend(other.readers);
        self.states.extend(other.states);
        self.page_skips += other.page_skips;
    }
}

/// The harness.
///
/// **What the default run covers.** Twelve seeds of `MNEMA_FUZZ_STEPS` steps each,
/// from a fixed base, so the default is the same corpus on every machine and in
/// CI rather than a lottery that is green until it is not. Each step draws one
/// of the operations in `draw`, applies it, walks whatever it touched — or, for
/// `RunWalk` and `SimulateEjectedVolume`, runs a real `walk_root` over the
/// whole folder — and checks every invariant in this file after **each call**,
/// not merely at the end of the step. About three hundred steps and rather
/// more calls, ending in a settle that insists nothing was left permanently
/// broken. Two and a half seconds, which is what makes it a test rather than a
/// nightly job.
///
/// It is a corpus chosen to catch things, not a round number: twelve seeds is
/// where the rebuild-with-two-copies sequence appears on this base, and eight
/// was one seed short of it.
///
/// **What a longer run costs.** `MNEMA_FUZZ_RUNS` and `MNEMA_FUZZ_STEPS` scale
/// it linearly in both directions; `MNEMA_FUZZ_BASE` moves the corpus to seeds
/// nothing has drawn yet. Raising the step count deepens sequences — a world
/// with more files in it, and more history behind each — while raising the base
/// widens them, and the two find different things. `MNEMA_FUZZ_TRACE=1` prints
/// each operation as it happens, which is the only way to see what a passing
/// run actually did.
///
/// **A failure prints one number.** `MNEMA_FUZZ_SEED=<n>` runs exactly that
/// seed and nothing else, and reproduces it exactly: the operations, the bytes,
/// and the modification times are all drawn from it.
#[test]
fn random_sequences_do_not_lose_data() {
    // **40, and the number was measured rather than chosen.** It stood at 24
    // until the two worst defects of the refuse-by-content cycle were found by
    // a reviewer instead of by this harness, and a re-review then measured why:
    // with both of their guards removed, 12 seeds × 24 steps is **green**, and
    // 12 × 40 is red on seed 1592590339. The states in question need a file to
    // be refused, re-indexed, and then handed back its earlier size and mtime —
    // a history too long to appear inside 24 steps. So the gate that runs on
    // every `cargo test` could not have caught either one.
    //
    // The cost of closing that is 2.2 seconds: 2.40 s at 24 steps against
    // 4.63 s at 40. Depth is what this dimension buys — `MNEMA_FUZZ_RUNS`
    // widens the corpus and finds different things, and neither substitutes for
    // the other.
    let steps = setting("MNEMA_FUZZ_STEPS", 40);
    if let Ok(seed) = std::env::var("MNEMA_FUZZ_SEED") {
        let seed = seed.parse().expect("MNEMA_FUZZ_SEED is a number");
        eprintln!("replaying seed {seed} for {steps} steps");
        let reached = run(seed, steps);
        // **A single seed asserts nothing about the corpus and now says so out
        // loud.** One run of forty steps has no business meeting every format,
        // so the coverage assertion below is deliberately skipped here — but
        // that left the only check on generator rot silent in exactly the mode
        // a person debugs in. Printing what this seed reached costs nothing and
        // means a replay can still show that the class being chased was never
        // generated at all.
        eprintln!(
            "seed {seed} reached:\n  shapes:  {:?}\n  readers: {:?}\n  rules:   {:?}\n  \
             states:  {:?}\n  per-page rows seen: {}",
            reached.shapes, reached.readers, reached.rules, reached.states, reached.page_skips
        );
        return;
    }
    let base = setting("MNEMA_FUZZ_BASE", 0x5EED_0000) as u64;
    let runs = setting("MNEMA_FUZZ_RUNS", 12);
    let mut reached = Reached::default();
    for i in 0..runs as u64 {
        reached.merge(run(base + i, steps));
    }

    // **The corpus has to have met what it claims to cover.** Everything above
    // judges what happened; this judges what was allowed to happen, and it is
    // the only assertion in the file that fails when the *generator* rots rather
    // than the product. Both directions are here on purpose: a missing entry is
    // a class nothing measured, and the list is written out rather than counted,
    // because a count is a definition that goes stale one format later.
    // **Compared as sets, in both directions, and every dimension at once.**
    //
    // Set equality rather than containment: a class that stops being generated
    // fails, and a class that starts being generated without anyone deciding it
    // should be also fails. The first version asserted only the first half, and
    // had the hole already — thirteen shape labels and twelve listed.
    //
    // **All four reported together rather than one `assert_eq!` after another**,
    // because a mutation that removes an operation from the draw table shifts
    // the whole RNG sequence and therefore the corpus: a case aimed at `states`
    // was reddening on `shapes` instead, and the log a reader sees named the
    // wrong claim. It was still a sound case — checked by neutralising the
    // assertions in front of it — but "which assertion fired" is the evidence
    // this branch runs on, and a louder neighbour answering first destroys it.
    let want_states: BTreeSet<&str> = ["rebuilt", "rebuild-resumed", "reader-changed-hands"]
        .into_iter()
        .collect();
    let want_shapes: BTreeSet<&str> = Shape::EVERY_LABEL.into_iter().collect();
    let want_readers: BTreeSet<String> = ["text", "markdown", "html", "epub", "docx", "xlsx"]
        .into_iter()
        .map(String::from)
        .collect();
    let want_rules: BTreeSet<&str> = EVERY_RULE
        .into_iter()
        .filter(|rule| must_the_corpus_reach(*rule))
        .map(SkipRule::as_str)
        .collect();

    let mut wrong = String::new();
    if reached.shapes != want_shapes {
        let _ = write!(
            wrong,
            "\n  shapes:  missing {:?}, unexpected {:?}",
            want_shapes.difference(&reached.shapes).collect::<Vec<_>>(),
            reached.shapes.difference(&want_shapes).collect::<Vec<_>>()
        );
    }
    if reached.readers != want_readers {
        let _ = write!(
            wrong,
            "\n  readers: missing {:?}, unexpected {:?}",
            want_readers
                .difference(&reached.readers)
                .collect::<Vec<_>>(),
            reached
                .readers
                .difference(&want_readers)
                .collect::<Vec<_>>()
        );
    }
    if reached.rules != want_rules {
        let _ = write!(
            wrong,
            "\n  rules:   missing {:?}, unexpected {:?}",
            want_rules.difference(&reached.rules).collect::<Vec<_>>(),
            reached.rules.difference(&want_rules).collect::<Vec<_>>()
        );
    }
    if reached.states != want_states {
        let _ = write!(
            wrong,
            "\n  states:  missing {:?}, unexpected {:?}",
            want_states.difference(&reached.states).collect::<Vec<_>>(),
            reached.states.difference(&want_states).collect::<Vec<_>>()
        );
    }
    assert!(
        wrong.is_empty(),
        "the corpus of {runs} seeds × {steps} steps did not contain exactly what this file \
         claims to cover. A class it did not reach is a class every invariant judges \
         vacuously; a class it reached and does not list is one nobody decided to add. \
         **PDF is deliberately absent from `readers`** — see `Format`'s doc — and `Memory` \
         from `rules`, by `must_the_corpus_reach`, which is exhaustive so a new `SkipRule` \
         cannot arrive without that decision being made.{wrong}"
    );

    assert!(
        reached.page_skips > 0,
        "no reader reported a skipped page in this corpus, so the per-page journal rows          Task 9 added — and the path that removes them — are untested"
    );
}

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
fn stricter_worker(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("stricter-worker");
    std::fs::write(
        &path,
        r#"#!/bin/sh
while read -r line; do
  file=$(printf '%s' "$line" | sed -n 's/.*"path":"\([^"]*\)".*/\1/p')
  sha=$(shasum -a 256 "$file" 2>/dev/null | cut -d' ' -f1)
  printf '{"frame":"refused","rule":"not_text","reason":"the threshold moved","sha256":"%s"}\n' "$sha"
done
"#,
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
}

/// Bytes and the words that must be findable in them once they are indexed.
struct Content {
    bytes: Vec<u8>,
    markers: Vec<String>,
    shape: Shape,
}

/// What the harness believes is at a path, which is only what the product
/// cannot know: the shape it wrote, the words it put there, and the
/// modification time it chose.
struct FileState {
    shape: Shape,
    markers: Vec<String>,
    mtime: SystemTime,
    /// A content rule refused this path, and the journal still remembers it
    /// against the file's current size and mtime.
    ///
    /// While that is true the file cannot come back to the index no matter how
    /// many clean walks run over it: `ingest_file`'s **second** cheap arm
    /// answers from the skip journal for any rule where
    /// `SkipRule::is_about_content()` holds, so no worker is ever asked again.
    /// That is D51's accepted price, written down — the only lever that clears
    /// such a verdict is `INDEX_FORMAT_VERSION`, and a walk does not move it.
    ///
    /// Cleared by anything that moves the file's size or modification time,
    /// because the journal row is keyed on those: once they differ, the cheap
    /// arm misses and the file is offered to a worker again.
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
        World {
            db,
            pool,
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
                // New bytes at a new time: whatever the journal remembers about
                // this path no longer matches what a walk will stat.
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

    fn opaque_body(&self) -> Content {
        Content {
            bytes: b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n".to_vec(),
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
    fn body_for(&mut self, relative: &str, units: usize) -> Content {
        if relative.ends_with(".md") {
            self.markdown_body(units)
        } else {
            self.text_body(units)
        }
    }

    /// A number of units that keeps the file comfortably under the ceiling,
    /// and that crosses `PAGES_PER_TRANSACTION` often enough for the write
    /// loop's second slice to be reached.
    fn ordinary_units(&mut self, relative: &str) -> usize {
        if relative.ends_with(".md") && self.rng.chance(30) {
            mnema_ingest::PAGES_PER_TRANSACTION + 2
        } else {
            1 + self.rng.below(5)
        }
    }

    /// Enough units to go over [`CEILING`].
    fn oversized_units(relative: &str) -> usize {
        if relative.ends_with(".md") { 80 } else { 100 }
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
    fn record(
        &mut self,
        relative: &str,
        hash: Option<String>,
        outcome: Result<Ingested, mnema_ingest::IngestError>,
        how: &str,
    ) {
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
        self.calling = Some(relative.to_string());
        self.last
            .insert(relative.to_string(), LastCall { hash, verdict });
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
    fn ingest_with(&mut self, relative: &str, config: PoolConfig, how: &str) {
        if self.excluded.contains(relative) {
            self.note(format!("    (excluded, not offered: {relative}{how})"));
            return;
        }
        let pool = Pool::new(config).unwrap();
        let before = self.paths_now();
        let hash = self.hash_on_disk(relative);
        let absolute = self.absolute(relative);
        let on_disk = mnema_walk::stat(&absolute);
        let outcome = ingest_file(&pool, &self.db, self.root_id, &absolute, relative, on_disk);
        self.record(relative, hash, outcome, how);
        self.check(&before);
        self.remember_settled();
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
        self.check_stored_is_findable(&after, &documents);
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
    /// (`crates/mnema-index/src/write.rs:172`, the query that decides
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
    /// Only the two content rules are named, and deliberately not `TooLarge`,
    /// whose answer is conditional on a size comparison rather than on the rule
    /// — restating that here would re-derive `displaces` inside the tool meant
    /// to check it. `Crash`, `Timeout`, `Memory` and `Unreadable` are readings
    /// of the environment and keep, which invariant 3 already covers whenever
    /// the bytes did not move.
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
                // contract, not a relaxed one; the keep side is a new claim
                // that nothing asserted before.
                SkipRule::NotText => match last.hash.as_deref() {
                    // The worker saw exactly the bytes the index was built
                    // from. The rule changed, the file did not.
                    Some(sha) if sha == held => {
                        if after.get(relative) != Some(held) {
                            self.fail(format!(
                                "invariant 3c — {relative} was refused as not text and lost \
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
                            "invariant 3c — {relative} was refused as not text, and the \
                             index still answers for it with {held}. Those bytes are a \
                             photo now, so every citation of that document names a file \
                             whose text it no longer contains"
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
                SkipRule::Crash
                | SkipRule::Timeout
                | SkipRule::Memory
                | SkipRule::Unsupported
                | SkipRule::NoTextLayer
                | SkipRule::Unreadable
                | SkipRule::TooLarge => {}
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
            self.rng.below(23)
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

    fn create(&mut self) {
        let n = self.next_counter();
        let relative = if self.rng.chance(35) {
            format!("docs/handbook-{n}.md")
        } else {
            format!("docs/file-{n}.txt")
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
            Shape::Text(n) | Shape::Markdown(n) => n,
            // A file that is currently unreadable bytes has no unit count to
            // preserve; rewriting it is a different operation. A photo and a
            // zeroed tail are the same case for the same reason — neither has
            // paragraphs or sections to keep the length of.
            Shape::Opaque | Shape::NotText | Shape::BinaryTail => return self.rewrite_small(),
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
            self.files.get_mut(&relative).unwrap().mtime = at;
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
        let extension = if relative.ends_with(".md") {
            "md"
        } else {
            "txt"
        };
        let copy = format!("backup/copy-{n}.{extension}");
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
        let extension = if relative.ends_with(".md") {
            "md"
        } else {
            "txt"
        };
        let renamed = format!("docs/renamed-{n}.{extension}");
        if std::fs::rename(self.absolute(&relative), self.absolute(&renamed)).is_err() {
            return;
        }
        let state = self.files.remove(&relative).unwrap();
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

    /// A text file replaced by a photo — the refusal that **removes**.
    ///
    /// Distinct from `make_opaque`, which writes a PDF header and earns
    /// `Unsupported`: that rule leaves the earlier document alone, so until
    /// this operation existed nothing in this harness ever drove a file down
    /// the branch of `record_skip` that deletes a path row and can orphan the
    /// document behind it.
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
        let keeping = self.on_disk(&relative);
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

    /// A ceiling lowered under a file that did not change.
    ///
    /// The size on disk is what tells this apart from a file that *grew* past a
    /// fixed ceiling, and only one of the two may lose what the index holds. The
    /// touch is what stops the cheap arm answering before the pool is asked.
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
            let relative = self.a_file();
            let stricter = stricter_worker(self.dir.path());
            self.note(format!("  walk {relative} past a STRICTER content rule"));
            self.ingest_with(&relative, PoolConfig::new(&stricter), " [stricter rule]");
            // The refusal is journalled against this file's current size and
            // mtime, and `SkipRule::NotText.is_about_content()` is true — so
            // every later walk answers from the journal without asking a
            // worker, and no number of clean passes brings this path back.
            // Recorded so that `settle` does not expect it to.
            if !self.excluded.contains(&relative)
                && let Some(state) = self.files.get_mut(&relative)
            {
                state.refused_by_content = true;
            }
        }
        #[cfg(not(unix))]
        self.reingest();
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
                Shape::Opaque => Some("bytes no reader can take"),
                Shape::NotText => Some("a photo"),
                Shape::BinaryTail => Some("text that stops being text partway through"),
                Shape::Text(_) | Shape::Markdown(_) => None,
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

fn run(seed: u64, steps: usize) {
    let mut world = World::new(seed);
    for n in 1..=steps {
        world.step(n);
    }
    world.settle();
}

/// The harness.
///
/// **What the default run covers.** Twelve seeds of twenty-four steps each,
/// from a fixed base, so the default is the same corpus on every machine and in
/// CI rather than a lottery that is green until it is not. Each step draws one
/// of the nineteen operations, applies it, walks whatever it touched — or, for
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
    let steps = setting("MNEMA_FUZZ_STEPS", 24);
    if let Ok(seed) = std::env::var("MNEMA_FUZZ_SEED") {
        let seed = seed.parse().expect("MNEMA_FUZZ_SEED is a number");
        eprintln!("replaying seed {seed} for {steps} steps");
        run(seed, steps);
        return;
    }
    let base = setting("MNEMA_FUZZ_BASE", 0x5EED_0000) as u64;
    let runs = setting("MNEMA_FUZZ_RUNS", 12);
    for i in 0..runs as u64 {
        run(base + i, steps);
    }
}

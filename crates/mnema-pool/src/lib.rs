//! The supervised extraction pool: it hands files to worker processes and
//! survives every way a worker can end.
//!
//! Extraction runs in a separate process because file parsers consume untrusted
//! input, and a malformed PDF can fault the C++ library reading it — killing the
//! process outright, where no guard written in Rust helps (D40, D35). This crate
//! is the parent side of that boundary. It never links `mnema-extract`: the
//! frames it parses live in `mnema_core::wire`, so the crate that binds Pdfium
//! stays out of the application's dependency graph entirely.
//!
//! A worker's life ends in one of eight ways, and the supervisor owes a
//! different answer to each:
//!
//! | how it ended                          | what the file gets       |
//! |---------------------------------------|--------------------------|
//! | answered `Refused` (no reader)        | [`Failure::Unsupported`] |
//! | answered `Refused` (over the ceiling) | [`Failure::TooLarge`]    |
//! | answered `Refused` (not text)         | [`Failure::NotText`]     |
//! | answered `Refused` (text, then not)   | [`Failure::BinaryTail`]  |
//! | answered `Failed` (I/O)               | [`Failure::Unreadable`]  |
//! | said nothing before the deadline      | [`Failure::Timeout`]     |
//! | died on a signal                      | [`Failure::Crash`]       |
//! | killed by the out-of-memory killer    | [`Failure::Memory`]      |
//!
//! Three properties of this design were measured before it was written, and they
//! are the reasons it has the shape it has rather than a simpler one:
//!
//! - **The child's stderr goes to a file, never to a pipe.** Pipe capacity on
//!   this platform is exactly 65,536 bytes, and a parent that drains only stdout
//!   deadlocks the moment the child writes past it — while the library that reads
//!   PDFs writes diagnostics to stderr on precisely the malformed input this
//!   boundary exists for.
//! - **A worker takes a batch, not one file.** Spawning a process, binding the
//!   library and opening a document costs about 4 ms here: nothing across 500
//!   documents, and dominant across thousands of small source files (D28, D40).
//! - **The batch counter is reset only by success.** Any other outcome retires
//!   the worker, so a bad file never shares a process with the next one — the
//!   guarantee batching would otherwise weaken.
//!
//! And one rule that is not an optimisation: **a file that killed a worker is
//! never handed to another one.** Without it a single malformed document loops
//! for as long as the job runs.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mnema_core::manifest::Manifest;
use mnema_core::wire::{Frame, Request, from_line, to_request_line};
use mnema_core::{Block, SourceKind};
use mnema_index::SkipRule;

/// How many stdout lines the reader thread may run ahead of the parent.
///
/// Bounded rather than unbounded so that the frames of a large document cannot
/// pile up in a channel *and* in the block vector at the same time; when the
/// parent falls behind, the backpressure reaches the worker through its own
/// stdout write. D40 chose NDJSON over whole-document JSON to keep peak memory
/// near the document's own size, and an unbounded channel would have given that
/// back.
const READ_AHEAD: usize = 64;

// ---------------------------------------------------------------- what fails

/// Declares [`Failure`] and, from the same list, the slice
/// [`Failure::every`] hands out — the form `declare_skip_rules` uses in
/// `mnema-index` for `SkipRule`, and here for the same measured reason.
///
/// `every_failure_maps_onto_its_own_skip_rule` in `tests/supervision.rs` used
/// to carry its own list of variants, with a comment saying it was "written out
/// rather than derived, so that a future variant added to either enum has to
/// face this list". The first variant added after that comment was
/// [`Failure::BinaryTail`], and it did not face the list: the list stayed seven
/// long, and mapping `BinaryTail` to the wrong rule left the whole of this
/// crate green — the only test that reddened was in `mnema-ingest`, a crate
/// away from the line that owns the mapping.
///
/// A list written beside an enum cannot say anything about a variant that is
/// not in it. Generating it from the declaration is what makes the promise in
/// that comment true.
macro_rules! declare_failures {
    ($($(#[$attr:meta])* $variant:ident,)+) => {
        /// Why a file did not make it into the index. Maps onto
        /// [`SkipRule`](mnema_index::SkipRule), the vocabulary the journal
        /// records, but is a smaller set: it names only the ways a *whole
        /// document* can fail, where `SkipRule` also covers a single PDF page
        /// with no text layer.
        ///
        /// **The mapping lives in this crate**, as `impl From<Failure> for
        /// SkipRule`, for three reasons. The pool is the only code that
        /// observes all eight outcomes — a worker that dies on a signal reports
        /// nothing itself, so no other crate could name that case.
        /// `mnema-extract` may not depend on `mnema-index` at all (a worker
        /// that links the database library it is forbidden from opening would
        /// undo the boundary it exists to draw, D26/D40), which is why the
        /// worker reports its rule as a plain string and something has to
        /// translate. And this crate already runs inside the application, where
        /// the database library is linked anyway, so the dependency costs
        /// nothing that was not already paid. The direction matters: a
        /// journal's vocabulary must not depend on the supervisor that happens
        /// to feed it.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Failure {
            $($(#[$attr])* $variant,)+
        }

        impl Failure {
            /// Every variant, in declaration order, generated from the list
            /// that declares them. Read [`every`](Failure::every) instead.
            const ALL: &[Failure] = &[$(Failure::$variant,)+];
        }
    };
}

impl Failure {
    /// Every variant, in declaration order.
    ///
    /// `pub` for the reason [`mnema_index::SkipRule::every`] is: the test that
    /// needs it is an integration test, which links this crate the way any
    /// other caller does and cannot see a `pub(crate)` or a `#[cfg(test)]`
    /// item. What it buys is that "every failure" in a test means every
    /// failure, including the one added tomorrow.
    pub fn every() -> impl Iterator<Item = Self> {
        Failure::ALL.iter().copied()
    }
}

declare_failures! {
    /// The worker died on a signal without answering — a parser fault.
    Crash,
    /// The worker said nothing within the deadline and was killed.
    Timeout,
    /// The worker was killed by a signal this pool did not send, which is the
    /// out-of-memory killer's signature.
    Memory,
    /// The worker declined the file's format: no reader exists for it yet,
    /// including `typing::Reader::Unrecognized`, which until now led nowhere.
    ///
    /// The size ceiling used to arrive here too and no longer does — see
    /// [`Failure::TooLarge`], which the parent has to be able to tell apart.
    Unsupported,
    /// The file is over the configured `max_bytes` and was refused from `stat`
    /// without being opened.
    ///
    /// Not folded into [`Failure::Unsupported`], although both arrive as
    /// `Frame::Refused`: `mnema-ingest` removes what the index holds under a
    /// path when a worker **read** a file and declined its content, and this
    /// branch never read a byte, so the refusal itself says nothing at all
    /// about whether the content changed.
    ///
    /// That is the distinction this variant exists for, and it is **not** the
    /// stronger claim this comment used to make. "Lowering a setting must not
    /// delete indexed content" was true of the rule as it then stood and is no
    /// longer true as written. What holds is narrower: lowering the ceiling does
    /// not, *on its own*, delete anything — an untouched file matches on size,
    /// mtime and stage, so the cheap arm answers `Unchanged` and no worker is
    /// ever started. Once either of those numbers has moved, the parent has only
    /// them to go on, and it displaces. The whole of that, including what it
    /// costs and what is left over,
    /// is [`SkipRule::TooLarge`](mnema_index::SkipRule::TooLarge) and
    /// `mnema_ingest`'s `displaces`.
    TooLarge,
    /// The file could not be read at all: missing, not a regular file, refused
    /// by permissions, or a path this protocol cannot carry.
    Unreadable,
    /// The worker looked at the bytes and they are not text (D51):
    /// `Frame::Refused { rule: "not_text" }`.
    NotText,
    /// The worker looked at the bytes and they are text at first and binary
    /// after that (D51): `Frame::Refused { rule: "binary_tail" }`.
    ///
    /// Not folded into [`Failure::NotText`], for the reason that split
    /// [`Failure::TooLarge`] off [`Failure::Unsupported`]: the parent decides
    /// whether to remove what the index holds under the path, and these two
    /// want opposite answers. A photo replacing a note means the note's text is
    /// gone; a note whose append was interrupted still holds its prose, and
    /// deleting the document would lose it.
    /// [`SkipRule::BinaryTail`](mnema_index::SkipRule::BinaryTail) carries the
    /// rest.
    BinaryTail,
}

impl From<Failure> for SkipRule {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Crash => SkipRule::Crash,
            Failure::Timeout => SkipRule::Timeout,
            Failure::Memory => SkipRule::Memory,
            Failure::Unsupported => SkipRule::Unsupported,
            Failure::Unreadable => SkipRule::Unreadable,
            Failure::TooLarge => SkipRule::TooLarge,
            Failure::NotText => SkipRule::NotText,
            Failure::BinaryTail => SkipRule::BinaryTail,
        }
    }
}

/// One file's failure, in the two parts `Db::record_skip` needs: the rule that
/// fired and a sentence a person can read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skip {
    pub failure: Failure,
    pub reason: String,
    /// The sha256 of the bytes this verdict was reached on, when the worker
    /// had them.
    ///
    /// `None` for every outcome the worker did not decide by reading: the size
    /// ceiling (refused from `stat`), a crash, a timeout, an out-of-memory
    /// kill, an unreadable path, and any answer this pool synthesised without
    /// a worker at all. The parent uses it to tell a file that *changed* into
    /// something unindexable from a file that did not change while the rule
    /// under it did — see `mnema_ingest`'s `displaces`, which is where the
    /// difference costs a document.
    pub sha256: Option<String>,
}

/// One page of an extracted document: what its [`Frame::Page`] announced, and
/// the blocks that arrived under it.
///
/// `page_no` is the reader's own numbering rather than this page's index in the
/// vector, and the two can differ — a reader that drops a page it cannot read
/// leaves a gap, which `Summary::skipped_pages` counts and this preserves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedPage {
    pub page_no: u32,
    pub section_title: Option<String>,
    pub blocks: Vec<Block>,
}

/// A document as the worker reported it: the header's facts, its pages with
/// their blocks in reading order, and the summary's.
///
/// Carries more than the blocks because the wire already does, and the schema
/// needs all of it — `document.mime`, `page.text_source`, and the count of pages
/// a reader dropped mid-document. Returning only the blocks would mean
/// extracting the file twice.
///
/// The header's page *count* is not kept: it has been checked against
/// `pages.len()` by the time this is built (see `run_one`), so keeping both
/// would leave two numbers that could disagree and a caller free to trust the
/// wrong one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub sha256: String,
    pub mime: String,
    pub source_kind: SourceKind,
    /// Which reader produced this document, and which version of it — carried
    /// through from the header unchanged. The pool does not interpret either:
    /// it is the parent that stores them beside the path and later compares
    /// them against a worker's manifest to decide whether the file must be read
    /// again. Carried here rather than re-derived from `mime` because the pool
    /// is deliberately the side of the boundary that knows no formats.
    pub reader: String,
    pub reader_version: u32,
    pub pages: Vec<ExtractedPage>,
    pub skipped_pages: u32,
    pub text_source: String,
}

/// What one call to [`Pool::extract`] settled. Both arms are ordinary results
/// for a multi-hour job over a folder nobody curated: a skip is recorded and the
/// walk continues. Only [`PoolError`] means "stop".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Extracted(Document),
    Skipped(Skip),
}

/// The pool itself is broken, or the worker binary is not the one this pool
/// speaks to. Distinct from [`Skip`] on purpose: a skip costs one file, one of
/// these costs the job, and confusing them either stops a run over one bad
/// document or records ten thousand files as damaged when the real fault is a
/// half-finished install.
#[derive(Debug, thiserror::Error)]
pub enum PoolError {
    #[error("a pool needs at least one worker and a batch of at least one file: {0}")]
    Config(String),
    #[error(
        "a memory ceiling was asked for, but this platform has none this crate can impose: {reason}"
    )]
    MemoryCeilingUnavailable { reason: &'static str },
    #[error("could not open the worker diagnostics file {path:?}: {source}")]
    Diagnostics { path: PathBuf, source: io::Error },
    #[error("could not start the extraction worker {worker:?}: {source}")]
    Spawn { worker: PathBuf, source: io::Error },
    #[error(
        "a running worker could not be given its request ({source}); it had already gone. \
         The file was not read and is not recorded as skipped"
    )]
    WorkerUnreachable { source: io::Error },
    #[error(
        "the worker does not speak this pool's protocol ({detail}) — the two binaries do not \
         match. Line: {line:?}"
    )]
    Protocol { line: String, detail: String },
    #[error("waiting on a worker process failed: {source}")]
    Wait { source: io::Error },
    #[error("a request could not be encoded: {source}")]
    Encode { source: serde_json::Error },
}

// ------------------------------------------------------- the memory ceiling

/// How, on this platform, a ceiling is imposed on a child's memory.
///
/// The case this exists for is a small archive that expands to gigabytes, and no
/// language-level guard closes it: by the time the parent could notice, the
/// child has already asked the kernel for the memory. The limit therefore has to
/// be set *on the child*, between fork and exec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryCeiling {
    /// `setrlimit(RLIMIT_AS)` in a `pre_exec` hook.
    AddressSpaceRlimit,
    /// This platform has no mechanism this crate implements. The string says
    /// what is missing, because a gap that reads like a feature is worse than
    /// no ceiling at all.
    Unavailable(&'static str),
}

/// What [`MemoryCeiling`] this build can actually impose.
///
/// Deliberately decided at compile time per platform rather than probed at
/// runtime: the only way to probe `setrlimit` is to call it, and calling it on
/// ourselves would limit the application. The macOS answer is measured, not
/// assumed — `crates/mnema-pool/tests/supervision.rs` pins it, so the day the
/// platform grows the call, a test goes red instead of a comment going stale.
pub fn memory_ceiling() -> MemoryCeiling {
    #[cfg(target_os = "linux")]
    return MemoryCeiling::AddressSpaceRlimit;

    #[cfg(target_os = "macos")]
    return MemoryCeiling::Unavailable(
        "macOS rejects setrlimit for RLIMIT_AS, RLIMIT_DATA and RLIMIT_RSS with EINVAL \
         (measured 2026-07-26 on Darwin 25.5.0/arm64; `ulimit -v` and `ulimit -d` report the \
         same), so there is no per-process address-space ceiling to set. A parent that sampled \
         the child's resident size and killed it would be a different mechanism with its own \
         measurements to make, and is not implemented",
    );

    #[cfg(windows)]
    return MemoryCeiling::Unavailable(
        "Windows has no setrlimit; the equivalent is a job object with \
         JOB_OBJECT_LIMIT_PROCESS_MEMORY, which is not implemented — there is no Windows \
         measurement rig on this project yet, and an untested ceiling is worse than a named gap",
    );

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    return MemoryCeiling::Unavailable(
        "no memory ceiling is implemented for this platform; only Linux's \
         setrlimit(RLIMIT_AS) is",
    );
}

/// Applies the ceiling to `command`'s child, between fork and exec.
#[cfg(target_os = "linux")]
fn apply_memory_ceiling(command: &mut Command, bytes: u64) {
    use std::os::unix::process::CommandExt;

    // SAFETY: `pre_exec` runs in the forked child, after fork and before exec,
    // where only async-signal-safe work is allowed — no allocation, no locks.
    // This closure does one `setrlimit` syscall on a stack value and returns;
    // `bytes` was copied into it before the fork.
    unsafe {
        command.pre_exec(move || {
            let limit = libc::rlimit {
                rlim_cur: bytes as libc::rlim_t,
                rlim_max: bytes as libc::rlim_t,
            };
            if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

// ------------------------------------------------------------------- config

/// How the pool is set up. Public fields, so a caller writes what it means to
/// change and inherits the rest:
///
/// ```no_run
/// # use std::time::Duration;
/// # use mnema_pool::PoolConfig;
/// let config = PoolConfig {
///     timeout: Duration::from_secs(30),
///     ..PoolConfig::new("/path/to/mnema-extract-worker")
/// };
/// ```
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// The worker executable. Resolved by the caller — the application knows
    /// where its own binaries are installed and this crate does not.
    pub worker: PathBuf,

    /// How many workers may run at once.
    ///
    /// **Provisional: 2.** D40 says "2–3 workers", reasoned from the 4 ms
    /// process cost and from four workers putting peak memory at 1.7 GB against
    /// the 3–4 GB budget §2.3 leaves the application — not from a throughput
    /// measurement. What is still unmeasured: the number on a floor-spec machine
    /// (where extraction competes with the interface for two cores) and on
    /// Windows, where process creation is dearer. 2 is the low end of a range
    /// someone reasoned about, not a measured optimum.
    pub workers: usize,

    /// How many *successful* files one worker may read before it is retired.
    ///
    /// **Provisional: 100.** D40's figure, chosen to amortise the 4 ms process
    /// cost (at 100 files it is 40 µs a file). The counter is reset only by
    /// success. What is unmeasured: whether a long-lived worker's memory grows
    /// across a hundred documents — the reason to retire at all — which needs a
    /// run against a real corpus with the real readers, not the text reader
    /// alone.
    pub batch: usize,

    /// How long one document may take before its worker is killed.
    ///
    /// **Provisional: 120 s.** Unmeasured in both directions, and both hurt: too
    /// short silently skips a legitimately slow document (a thousand-page PDF),
    /// too long stalls a job on a file that will never finish. D40 puts this on
    /// the measurement list, and notes it is also the answer to a Cancel that
    /// cannot interrupt the file in flight. A ceiling that scales with the
    /// file's size is the obvious refinement and is not implemented.
    pub timeout: Duration,

    /// A ceiling on the child's memory, in bytes, or `None` for no ceiling.
    ///
    /// [`Pool::new`] **refuses** a `Some` on a platform whose ceiling
    /// [`memory_ceiling`] reports as unavailable, rather than accepting the
    /// number and quietly not applying it.
    pub memory_limit: Option<u64>,

    /// The largest file the worker may read; anything above is refused from
    /// `stat`, before a byte is loaded.
    ///
    /// **Provisional: 64 MiB.** D40 records that this ceiling "is required and
    /// does not exist anywhere today" without naming a number. 64 MiB is
    /// derived, not measured: the worker holds a document's bytes and its blocks
    /// at once, so a handful of workers on files this size stays inside the
    /// 3–4 GB §2.3 leaves the application. The number a real corpus justifies is
    /// unknown.
    pub max_bytes: u64,

    /// Where the workers' stderr goes. **Never a pipe** — see the module doc for
    /// the 65,536-byte deadlock that rules one out.
    ///
    /// `None` sends it to the null device, which costs the diagnostics a parser
    /// writes on exactly the malformed files this boundary exists for, so an
    /// application should give it a path. Several workers append to one file and
    /// their lines interleave; that is accepted for diagnostics.
    pub diagnostics: Option<PathBuf>,
}

impl PoolConfig {
    /// The provisional defaults, for the named worker executable. Every default
    /// is documented on its field, including which are measured and which are
    /// not.
    pub fn new(worker: impl Into<PathBuf>) -> Self {
        Self {
            worker: worker.into(),
            workers: 2,
            batch: 100,
            timeout: Duration::from_secs(120),
            memory_limit: None,
            max_bytes: 64 << 20,
            diagnostics: None,
        }
    }
}

// --------------------------------------------------------------------- pool

/// A fixed set of worker slots, filled on demand.
#[derive(Debug)]
pub struct Pool {
    config: PoolConfig,
    /// Opened once, cloned per spawn: several children share one append-mode
    /// descriptor.
    diagnostics: Option<File>,
    /// One entry per permitted worker; `None` is a permit with no process behind
    /// it yet. Popping is checking a permit out, so the vector's length is the
    /// number of workers not currently working.
    slots: Mutex<Vec<Option<Worker>>>,
    free: Condvar,
    /// Files that killed a worker, and what they were recorded as. Never handed
    /// to a second process.
    ///
    /// **Keyed on the path and nothing else, which is strictly weaker than the
    /// skip journal one level up, and is safe only because of who builds a
    /// `Pool`.** The journal at least compares `(size, mtime,
    /// format_version)` before it trusts a remembered verdict
    /// (`mnema_ingest::ingest_file`'s second cheap arm); this map is read at the
    /// top of `extract`, before anything has looked at the file, so it carries
    /// no size, no modification time, no digest and no expiry. Replace the file
    /// entirely and this still answers for it.
    ///
    /// What makes that harmless today is a fact about the caller, not about this
    /// type: `src-tauri/src/walk_job.rs` builds a **new** `Pool` for every walk
    /// job, and phase 2 offers each path to it once. So an entry can never
    /// outlive the pass that made it. Measured on two pools over one index: the
    /// poisoned one answers `Skipped { Crash }` without asking a worker and the
    /// old prose stays findable, while a pool without the entry reads the file,
    /// says `NotText` and displaces.
    ///
    /// That is a **contract**, and this comment is the only place it is written
    /// down — `walk_root` takes `&Pool` and `Pool` is public, so a live watcher
    /// that re-walks on change against one long-lived pool would make it a
    /// defect rather than a note. Whoever builds that has to key this map on
    /// something the file can change, or drop entries when a walk ends.
    ///
    /// One mitigation is real and worth having: a refusal **by content** never
    /// poisons. Only `Answer::Ended` does (below), which is the worker dying —
    /// so the rules that displace a document are not the ones this map can
    /// answer for.
    poisoned: Mutex<HashMap<PathBuf, Skip>>,
    spawned: AtomicUsize,
    live: Arc<AtomicUsize>,
}

impl Pool {
    pub fn new(config: PoolConfig) -> Result<Self, PoolError> {
        if config.workers == 0 {
            return Err(PoolError::Config("workers is 0".to_string()));
        }
        if config.batch == 0 {
            return Err(PoolError::Config("batch is 0".to_string()));
        }
        if config.memory_limit.is_some()
            && let MemoryCeiling::Unavailable(reason) = memory_ceiling()
        {
            return Err(PoolError::MemoryCeilingUnavailable { reason });
        }

        // Opened here rather than per spawn so that an unwritable path fails
        // when the pool is built, not on the first file of a multi-hour job.
        let diagnostics = match &config.diagnostics {
            Some(path) => Some(
                OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(path)
                    .map_err(|source| PoolError::Diagnostics {
                        path: path.clone(),
                        source,
                    })?,
            ),
            None => None,
        };

        let slots = (0..config.workers).map(|_| None).collect();
        Ok(Self {
            config,
            diagnostics,
            slots: Mutex::new(slots),
            free: Condvar::new(),
            poisoned: Mutex::new(HashMap::new()),
            spawned: AtomicUsize::new(0),
            live: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// Reads one file, blocking until a worker is free.
    ///
    /// `Ok(Outcome::Skipped)` is an ordinary answer: record it and carry on.
    /// `Err` means the pool or the worker binary is broken and the job should
    /// stop.
    pub fn extract(&self, path: &Path) -> Result<Outcome, PoolError> {
        // The request carries the path as JSON, so a path that is not UTF-8
        // cannot be expressed. `Path::display()` would substitute a replacement
        // character and ask the worker to read a file that does not exist —
        // reporting the honest reason costs a process instead of wasting one.
        let Some(text) = path.to_str() else {
            return Ok(Outcome::Skipped(Skip {
                failure: Failure::Unreadable,
                reason: format!(
                    "{} is not valid UTF-8, and the extraction request cannot carry it",
                    path.display()
                ),
                // No worker ran, so nothing read the bytes.
                sha256: None,
            }));
        };

        if let Some(recorded) = self.poisoned().get(path) {
            return Ok(Outcome::Skipped(recorded.clone()));
        }

        let mut lease = self.checkout();

        // At most two attempts, and the second is only ever reached when the
        // first proved that the request never left this process — see the
        // `Unsendable` arm below. Every other outcome returns from inside the
        // loop, so falling out of it means both attempts were unsendable.
        //
        // A worker taken back out of a slot has been idle since its last file,
        // and "idle" is not "alive": where no memory ceiling can be imposed, the
        // out-of-memory killer chooses its victim by size rather than by what a
        // process is doing, so a worker can die after it answered and before it
        // is asked again. Asking `try_wait` first, ahead of the write, would
        // catch that — and it is **not** what this does, because measured on
        // this platform it catches strictly less: a worker can also be alive and
        // no longer listening (its end of the request pipe closed), which no
        // amount of asking whether it has exited will predict. Retrying on a
        // failed handover covers both, and the write into a departed worker's
        // pipe fails reliably rather than being quietly buffered. That last
        // point is a platform property rather than a guarantee of the language,
        // so it is pinned by a test that asserts the outcome — a dead idle
        // worker costs the next file nothing — and not the mechanism. On a
        // platform where such a write succeeds, that test fails and the
        // pre-flight check is what closes it.
        for _ in 0..2 {
            if lease.worker.is_none() {
                lease.worker = Some(self.spawn()?);
            }
            let fresh = lease.worker.as_ref().is_some_and(|w| w.successes == 0);
            let worker = lease.worker.as_mut().expect("a worker was just put here");

            match run_one(worker, text, &self.config) {
                // The stream is out of step; this worker cannot be trusted with
                // another file, and the caller has to hear about it.
                Err(error) => {
                    lease.retire();
                    return Err(error);
                }
                // The worker was alive a syscall ago and gone by the time it was
                // written to, so the request never reached a parser. Retrying it
                // requeues nothing: the no-requeue rule protects a file that
                // *killed* a worker, and this file was never read. Bounded at
                // one attempt, and only on a worker that had already done work
                // — a brand new process that cannot be written to means the
                // environment is broken, not that a worker aged out.
                Ok(Answer::Unsendable(source)) => {
                    lease.retire();
                    if fresh {
                        return Err(PoolError::WorkerUnreachable { source });
                    }
                }
                Ok(Answer::Document(document)) => {
                    let worker = lease.worker.as_mut().expect("the worker answered");
                    worker.successes += 1;
                    if worker.successes >= self.config.batch {
                        lease.retire();
                    }
                    return Ok(Outcome::Extracted(document));
                }
                // Refusals and I/O failures retire the worker too: the batch
                // counter is reset only by success, so that a file which upset a
                // parser never shares a process with the next one. The cost is
                // one process per refused file — 4 ms — which a folder of
                // unsupported formats pays in full, and which is the deliberate
                // side of this trade rather than an oversight.
                Ok(Answer::Skipped {
                    failure,
                    reason,
                    sha256,
                }) => {
                    lease.retire();
                    return Ok(Outcome::Skipped(Skip {
                        failure,
                        reason,
                        sha256,
                    }));
                }
                Ok(Answer::Ended { ending, note }) => {
                    lease.retire();
                    let (failure, reason) = classify(
                        &ending,
                        self.config.memory_limit,
                        self.config.timeout,
                        note.as_deref(),
                    );
                    // A worker that died, timed out or was killed never
                    // reported a digest — and must not be given one here: the
                    // whole point of the poison record is that these bytes were
                    // never successfully read.
                    let skip = Skip {
                        failure,
                        reason,
                        sha256: None,
                    };
                    // This is the rule that keeps a multi-hour job moving: the
                    // file that killed a worker is remembered, not requeued.
                    self.poisoned().insert(path.to_path_buf(), skip.clone());
                    return Ok(Outcome::Skipped(skip));
                }
            }
        }

        // Both attempts ended with a request that could not be handed over, the
        // second to a process this pool had just started. Nothing at all is
        // known about the file, so nothing is recorded against it.
        Err(PoolError::WorkerUnreachable {
            source: io::Error::other(
                "two workers in a row could not be given the request, the second one freshly \
                 spawned",
            ),
        })
    }

    /// How many worker processes this pool has started, ever. A file answered
    /// from the poisoned record, or by a worker still inside its batch, does not
    /// move this.
    pub fn worker_generation(&self) -> usize {
        self.spawned.load(Ordering::SeqCst)
    }

    /// How many worker processes exist right now — never more than
    /// `config.workers`.
    pub fn live_workers(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }

    /// The `workers` this pool was configured with — fixed for the pool's
    /// whole lifetime, unlike [`live_workers`](Self::live_workers), which is
    /// zero until the first file asks for a process and stays below the
    /// configured count the rest of the time whenever fewer files are in
    /// flight than there are slots. A caller that wants to reason about the
    /// pool's *capacity* — how many processes a genuinely broken batch could
    /// touch — needs this one; `live_workers` answers a different question
    /// ("how many processes exist at this instant") that happens to be 0 at
    /// the moment a caller has not yet extracted anything.
    pub fn configured_workers(&self) -> usize {
        self.config.workers
    }

    /// What this pool's worker says its readers are: which reader takes which
    /// extension, and at what version.
    ///
    /// A process rather than a constant, and that is D40 rather than a
    /// preference: the parent may not link `mnema-extract`, so the only way to
    /// learn what this build reads is to ask the binary. It is asked of
    /// `config.worker` — the same executable every file goes to — which is the
    /// property the parent's freshness check rests on: the manifest and the
    /// headers come from one binary, or from neither.
    ///
    /// Outside the NDJSON protocol and outside the slots entirely. `--manifest`
    /// prints one line and exits, so there is no worker to lease, no batch to
    /// count against and nothing to poison; a caller asks once per walk, before
    /// deciding which files to send. Not cached here either: a `Pool` may
    /// outlive the walk that built it (see `poisoned`), and a cached manifest
    /// would be a second thing that could then answer for a binary that has
    /// since been replaced.
    ///
    /// The pipe this one *does* use is safe where the ones in `spawn` are not:
    /// `Command::output` drains stdout and stderr concurrently, so the
    /// 65,536-byte deadlock the module doc describes has nothing to fill. The
    /// worker's own stderr is folded into the error below rather than sent to
    /// the diagnostics file, because a binary that cannot state its readers is
    /// a fault the caller is about to be told about and its complaint belongs
    /// in that sentence.
    ///
    /// An `Err` stops the job, and it is the same class as a `Protocol` error
    /// on a frame: a binary that cannot state its readers is not the one this
    /// parent speaks to. There is deliberately no fallback — an empty manifest,
    /// or the parent's own idea of what the defaults are, would decide the
    /// freshness of every file in the index from a value nothing measured, and
    /// it would do it silently.
    pub fn manifest(&self) -> Result<Manifest, PoolError> {
        let out = Command::new(&self.config.worker)
            .arg("--manifest")
            // Nothing is written to this child: it answers an argument, not a
            // request. Null rather than inherited, so a worker built to read
            // stdin cannot sit waiting on the parent's own terminal.
            .stdin(Stdio::null())
            .output()
            .map_err(|source| PoolError::Spawn {
                worker: self.config.worker.clone(),
                source,
            })?;
        if !out.status.success() {
            return Err(protocol(
                &String::from_utf8_lossy(&out.stderr),
                &format!(
                    "{:?} --manifest ended with {} instead of stating this build's readers",
                    self.config.worker, out.status
                ),
            ));
        }
        serde_json::from_slice(&out.stdout).map_err(|source| {
            protocol(
                &String::from_utf8_lossy(&out.stdout),
                &format!("--manifest did not answer with a reader manifest: {source}"),
            )
        })
    }

    fn poisoned(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Skip>> {
        self.poisoned
            .lock()
            .expect("the poisoned-file record is only held for a map operation")
    }

    /// Takes a permit, waiting for one if every worker is busy. The permit
    /// returns when the [`Lease`] drops, including if the caller panics.
    fn checkout(&self) -> Lease<'_> {
        let mut slots = self
            .slots
            .lock()
            .expect("the slot list is only held while a slot changes hands");
        while slots.is_empty() {
            slots = self
                .free
                .wait(slots)
                .expect("the slot list is only held while a slot changes hands");
        }
        let worker = slots.pop().expect("a non-empty list yields a slot");
        drop(slots);
        Lease { pool: self, worker }
    }

    fn checkin(&self, worker: Option<Worker>) {
        let mut slots = self
            .slots
            .lock()
            .expect("the slot list is only held while a slot changes hands");
        slots.push(worker);
        drop(slots);
        self.free.notify_one();
    }

    fn spawn(&self) -> Result<Worker, PoolError> {
        let mut command = Command::new(&self.config.worker);
        command.stdin(Stdio::piped()).stdout(Stdio::piped());

        // Never `Stdio::piped()`: the measured deadlock at 65,536 bytes is what
        // this line prevents. A file, or the null device.
        let stderr = match &self.diagnostics {
            Some(file) => {
                Stdio::from(file.try_clone().map_err(|source| PoolError::Diagnostics {
                    path: self.config.diagnostics.clone().unwrap_or_default(),
                    source,
                })?)
            }
            None => Stdio::null(),
        };
        command.stderr(stderr);

        // On every other platform `memory_limit` is `None` — `Pool::new`
        // refuses to accept one it cannot impose — so there is nothing to apply.
        #[cfg(target_os = "linux")]
        if let Some(bytes) = self.config.memory_limit {
            apply_memory_ceiling(&mut command, bytes);
        }

        let mut child = command.spawn().map_err(|source| PoolError::Spawn {
            worker: self.config.worker.clone(),
            source,
        })?;
        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");

        let (lines, receiver) = sync_channel(READ_AHEAD);
        let reader = std::thread::spawn(move || read_lines(stdout, lines));

        self.spawned.fetch_add(1, Ordering::SeqCst);
        self.live.fetch_add(1, Ordering::SeqCst);
        Ok(Worker {
            child,
            stdin: Some(stdin),
            lines: Some(receiver),
            reader: Some(reader),
            successes: 0,
            live: Arc::clone(&self.live),
        })
    }
}

/// A checked-out permit, and the worker behind it if there is one. Returning the
/// permit is `Drop`'s job so that a panic between checkout and checkin cannot
/// shrink the pool.
struct Lease<'pool> {
    pool: &'pool Pool,
    worker: Option<Worker>,
}

impl Lease<'_> {
    /// Ends the worker's life now. The permit itself survives; the next file
    /// through this slot gets a fresh process.
    fn retire(&mut self) {
        self.worker = None;
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.pool.checkin(self.worker.take());
    }
}

// ------------------------------------------------------------------- worker

#[derive(Debug)]
struct Worker {
    child: Child,
    /// `Option` only so that `Drop` can close it before killing: a worker
    /// reading stdin ends on its own when the pipe closes.
    stdin: Option<ChildStdin>,
    /// `Option` for a sharper reason — see `Drop`. The reading end has to be
    /// closed *before* the thread holding the writing end is joined.
    lines: Option<Receiver<io::Result<String>>>,
    reader: Option<JoinHandle<()>>,
    /// Successful files this process has read. Reset by nothing — a worker that
    /// fails once is retired, not forgiven.
    successes: usize,
    live: Arc<AtomicUsize>,
}

/// How a worker's process ended, with enough context to name it.
#[derive(Debug, Clone, Copy)]
struct Ending {
    status: ExitStatus,
    /// This pool's own `kill` is what ended it.
    we_killed: bool,
    /// …and it was killed because the deadline passed, rather than because its
    /// stdout had closed while it was still running.
    at_deadline: bool,
}

impl Worker {
    /// Ends the process and reports how it ended.
    ///
    /// Checks first whether it has already exited, so a worker that died of its
    /// own accord a moment before the deadline is reported as what killed it and
    /// not as a timeout. After a `kill`, "did we do this" is answered from the
    /// wait status rather than from having called `kill`: the two differ in
    /// exactly the race this ordering exists to catch.
    fn stop(&mut self, at_deadline: bool) -> Result<Ending, PoolError> {
        if let Some(status) = self
            .child
            .try_wait()
            .map_err(|source| PoolError::Wait { source })?
        {
            return Ok(Ending {
                status,
                we_killed: false,
                at_deadline: false,
            });
        }
        // SIGKILL is safe here without ceremony, and that is the point of the
        // whole boundary: the worker holds no database handle, writes nothing
        // but frames, and flushes each one as it is produced. There is no state
        // for a graceful shutdown to save.
        let _ = self.child.kill();
        let status = self
            .child
            .wait()
            .map_err(|source| PoolError::Wait { source })?;
        let ours = killed_by_us(status);
        Ok(Ending {
            status,
            we_killed: ours,
            at_deadline: at_deadline && ours,
        })
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing stdin lets a healthy worker exit on its own; the kill covers
        // one that will not. Both are no-ops on a process already reaped —
        // `Child` caches its exit status — so this is safe after `stop`.
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Closing the reading end *before* the join, and this order is
        // load-bearing rather than tidy. A worker retired mid-document — on a
        // protocol error, say — leaves its reader thread blocked handing over a
        // line the parent has stopped taking, and a bounded channel does not
        // release that thread until the receiver is gone. Joining first
        // deadlocks inside `Drop`, where no deadline applies and no error can be
        // returned: the whole job stops silently. Dropping the receiver turns
        // the blocked `send` into an error, which is how the thread learns
        // nobody is listening. `tests/supervision.rs` pins it.
        drop(self.lines.take());
        // Joining is what keeps a long job from accumulating one thread per
        // retired worker.
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(unix)]
fn killed_by_us(status: ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;
    // The only signal this pool sends is SIGKILL, so anything else came from
    // elsewhere — a parser fault, or the out-of-memory killer.
    status.signal() == Some(libc::SIGKILL)
}

#[cfg(not(unix))]
fn killed_by_us(_status: ExitStatus) -> bool {
    // `Child::kill` is `TerminateProcess` here, which leaves an ordinary exit
    // code indistinguishable from one the process chose. Having just called it,
    // the honest answer is yes.
    true
}

/// Reads the worker's stdout line by line until it closes.
///
/// Its own thread because the parent needs a deadline, and a blocking read has
/// none. Killing the worker closes this pipe, which is what ends this thread.
fn read_lines(stdout: ChildStdout, lines: SyncSender<io::Result<String>>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                // An error here means the parent has dropped the receiver: the
                // worker is being retired and nobody is listening.
                if lines.send(Ok(line)).is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = lines.send(Err(error));
                break;
            }
        }
    }
}

// ----------------------------------------------------------- one file's turn

enum Answer {
    Document(Document),
    /// The worker answered with a refusal or an I/O failure: one frame, no
    /// document, and a rule already chosen.
    Skipped {
        failure: Failure,
        reason: String,
        sha256: Option<String>,
    },
    /// The worker produced no answer: it died, or it was killed. `note` carries
    /// anything the wait status cannot express — today, that its output was not
    /// text.
    Ended {
        ending: Ending,
        note: Option<String>,
    },
    Unsendable(io::Error),
}

/// Sends one request and reads until the worker has answered it, the deadline
/// passes, or the process ends.
fn run_one(worker: &mut Worker, path: &str, config: &PoolConfig) -> Result<Answer, PoolError> {
    let request = to_request_line(&Request {
        path: path.to_string(),
        max_bytes: config.max_bytes,
    })
    .map_err(|source| PoolError::Encode { source })?;

    let stdin = worker.stdin.as_mut().expect("a live worker has stdin");
    if let Err(error) = stdin
        .write_all(request.as_bytes())
        .and_then(|()| stdin.flush())
    {
        return Ok(Answer::Unsendable(error));
    }

    // One deadline for the whole document, not per frame: a worker that emits a
    // block a second forever is as stuck as one that emits nothing.
    let deadline = Instant::now() + config.timeout;
    let mut header: Option<Frame> = None;
    let mut pages: Vec<ExtractedPage> = Vec::new();

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let lines = worker
            .lines
            .as_ref()
            .expect("a worker is only read from before it is dropped");
        let line = match lines.recv_timeout(remaining) {
            Ok(Ok(line)) => line,
            // The worker's stdout could not be read — `read_line` refuses bytes
            // that are not UTF-8. This is **not** a protocol disagreement, and
            // saying so would be the wrong accusation: the whole reason this
            // process boundary exists is that a C++ library misbehaves on
            // malformed input, and one that writes raw bytes to stdout rather
            // than to stderr arrives exactly here. Telling the user their
            // binaries do not match, and stopping a job of forty thousand files,
            // would be a worse answer than the truth — this one file's worker
            // produced unusable output. So it is an ending like any other, and
            // the file gets a skip.
            Ok(Err(error)) => {
                return Ok(Answer::Ended {
                    ending: worker.stop(false)?,
                    note: Some(format!("the worker's output could not be read: {error}")),
                });
            }
            Err(RecvTimeoutError::Timeout) => {
                return Ok(Answer::Ended {
                    ending: worker.stop(true)?,
                    note: None,
                });
            }
            // stdout closed: the process has ended, or is about to.
            Err(RecvTimeoutError::Disconnected) => {
                return Ok(Answer::Ended {
                    ending: worker.stop(false)?,
                    note: None,
                });
            }
        };

        let frame = from_line(&line).map_err(|error| PoolError::Protocol {
            line: line.clone(),
            detail: format!("a line was not a frame: {error}"),
        })?;

        match frame {
            Frame::Header { ref reader, .. } => {
                if header.is_some() {
                    return Err(protocol(&line, "a second header inside one document"));
                }
                // `reader` being a required field stops a header that omits it,
                // and stops nothing else: `""` is a perfectly good `String` and
                // parses without complaint. That leaves the placeholder the
                // required field exists to prevent reachable by another road —
                // and `NOT NULL` on the column that stores it will not catch an
                // empty string either.
                //
                // What it would cost is not a bad name in a row. The parent
                // compares this value against a worker's manifest to decide
                // whether a file must be read again; no manifest ever names the
                // empty reader, so every document from such a worker mismatches
                // for ever and is re-extracted on every run — a job that never
                // settles, over a folder nobody is watching. Refused here,
                // where the other "this worker does not speak our protocol"
                // checks live, rather than deeper in, so that no document is
                // built from it at all.
                if reader.trim().is_empty() {
                    return Err(protocol(&line, "a header naming no reader"));
                }
                header = Some(frame);
            }
            Frame::Page {
                page_no,
                section_title,
            } => {
                if header.is_none() {
                    return Err(protocol(&line, "a page before any header"));
                }
                pages.push(ExtractedPage {
                    page_no,
                    section_title,
                    blocks: Vec::new(),
                });
            }
            Frame::Block(block) => {
                if header.is_none() {
                    return Err(protocol(&line, "a block before any header"));
                }
                // Every reader opens a page before its first block, plain text
                // included, so a block arriving with no page open is a worker
                // that does not speak this protocol — not a block belonging to
                // an implied page 1. Inventing that page here is how the
                // application would end up holding a thousand-page document
                // with every block on one page and nothing anywhere saying so.
                let Some(page) = pages.last_mut() else {
                    return Err(protocol(&line, "a block before any page"));
                };
                page.blocks.push(block);
            }
            Frame::Summary {
                skipped_pages,
                text_source,
            } => {
                let Some(Frame::Header {
                    sha256,
                    mime,
                    source_kind,
                    reader,
                    reader_version,
                    pages: promised,
                }) = header
                else {
                    return Err(protocol(&line, "a summary before any header"));
                };
                // A worker that does not agree with this pool about the
                // protocol, caught for free — the same accusation
                // `PoolError::Protocol` makes about a line that is not a
                // frame, and the same remedy: stop the job rather than record
                // ten thousand files against a binary mismatch.
                //
                // **Not a truncation check**, which is what it looks like. A
                // worker killed part-way through a document loses its summary
                // too and never reaches here; it is classified by how it died.
                // And the real worker takes the header's count and the page
                // frames from one vector (`bin/worker.rs`), so this cannot
                // fire for a correct reader at all.
                if promised as usize != pages.len() {
                    return Err(protocol(
                        &line,
                        &format!(
                            "the header promised {promised} pages and {} arrived",
                            pages.len()
                        ),
                    ));
                }
                return Ok(Answer::Document(Document {
                    sha256,
                    mime,
                    source_kind,
                    reader,
                    reader_version,
                    pages,
                    skipped_pages,
                    text_source,
                }));
            }
            // A refusal or an I/O failure is the whole answer, so one arriving
            // after a header means the two binaries disagree about the
            // protocol.
            Frame::Refused {
                rule,
                reason,
                sha256,
            } => {
                if header.is_some() {
                    return Err(protocol(&line, "a refusal after a header"));
                }
                let failure = match rule.as_str() {
                    "unsupported" => Failure::Unsupported,
                    "not_text" => Failure::NotText,
                    "binary_tail" => Failure::BinaryTail,
                    "unreadable" => Failure::Unreadable,
                    "too_large" => Failure::TooLarge,
                    // Strict on purpose. A rule this pool does not know means
                    // the worker is from another release, and answering ten
                    // thousand files with a guess would bury that.
                    other => {
                        return Err(protocol(
                            &line,
                            &format!("it refused the file under an unknown rule {other:?}"),
                        ));
                    }
                };
                return Ok(Answer::Skipped {
                    failure,
                    reason,
                    sha256,
                });
            }
            Frame::Failed { message } => {
                if header.is_some() {
                    return Err(protocol(&line, "an I/O failure after a header"));
                }
                return Ok(Answer::Skipped {
                    failure: Failure::Unreadable,
                    reason: message,
                    // `Failed` means the worker could not obtain the bytes at
                    // all, so there is nothing to have hashed.
                    sha256: None,
                });
            }
        }
    }
}

fn protocol(line: &str, detail: &str) -> PoolError {
    PoolError::Protocol {
        line: line.to_string(),
        detail: detail.to_string(),
    }
}

/// Names a worker's death.
///
/// The one case this cannot tell apart is worth stating plainly: under an
/// address-space ceiling, an allocation that exceeds it makes `malloc` fail, and
/// Rust's response to a failed allocation is `abort` — a SIGABRT, exactly what a
/// C++ parser faulting on a malformed document produces. So a file that
/// exhausted the ceiling is reported as a crash, and the reason says a ceiling
/// was in force. [`Failure::Memory`] is kept for the death the platform *does*
/// report unambiguously: a SIGKILL this pool did not send, which is what the
/// out-of-memory killer sends and which a ceiling never does.
///
/// `note` is prepended to the reason when the caller knows something the wait
/// status cannot say — today, that the worker's output was not text at all.
fn classify(
    ending: &Ending,
    ceiling: Option<u64>,
    timeout: Duration,
    note: Option<&str>,
) -> (Failure, String) {
    let (failure, reason) = name_the_ending(ending, ceiling, timeout);
    match note {
        Some(note) => (failure, format!("{note}; {reason}")),
        None => (failure, reason),
    }
}

fn name_the_ending(ending: &Ending, ceiling: Option<u64>, timeout: Duration) -> (Failure, String) {
    if ending.at_deadline {
        return (
            Failure::Timeout,
            format!("the worker did not answer within {timeout:?} and was killed"),
        );
    }
    if ending.we_killed {
        return (
            Failure::Crash,
            "the worker's stdout closed while the process was still running, so it was killed"
                .to_string(),
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = ending.status.signal() {
            if signal == libc::SIGKILL {
                return (
                    Failure::Memory,
                    "the worker was killed by SIGKILL, which this pool did not send: that is \
                     the out-of-memory killer's signature"
                        .to_string(),
                );
            }
            let under_ceiling = match ceiling {
                Some(bytes) => format!(
                    " (a {bytes}-byte address-space ceiling was in force, so this may equally \
                     be an allocation the ceiling refused)"
                ),
                None => String::new(),
            };
            return (
                Failure::Crash,
                format!("the worker died on signal {signal} without answering{under_ceiling}"),
            );
        }
    }

    match ending.status.code() {
        Some(0) => (
            Failure::Crash,
            "the worker exited cleanly without answering".to_string(),
        ),
        Some(code) => (
            Failure::Crash,
            format!("the worker exited with status {code} without answering"),
        ),
        None => (
            Failure::Crash,
            "the worker ended without an exit status".to_string(),
        ),
    }
}

#[cfg(all(test, unix))]
mod classification {
    //! The wait statuses a worker can end with, built by hand.
    //!
    //! Here rather than in `tests/supervision.rs` because `classify` is private
    //! and should stay so, and because provoking a real out-of-memory kill in a
    //! test suite is not something to do — this is the one branch no
    //! behavioural test can reach.

    use super::*;
    use std::os::unix::process::ExitStatusExt;

    /// A status for a process that died on `signal`: the low seven bits of a
    /// wait status are the signal that ended it.
    fn signalled(signal: i32) -> ExitStatus {
        ExitStatus::from_raw(signal)
    }

    /// …and the next eight bits are the exit code of one that exited.
    fn exited(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code << 8)
    }

    fn ending(status: ExitStatus, we_killed: bool, at_deadline: bool) -> Ending {
        Ending {
            status,
            we_killed,
            at_deadline,
        }
    }

    #[test]
    fn a_worker_killed_at_the_deadline_is_a_timeout() {
        let (failure, reason) = classify(
            &ending(signalled(libc::SIGKILL), true, true),
            None,
            Duration::from_secs(7),
            None,
        );
        assert_eq!(failure, Failure::Timeout);
        assert!(
            reason.contains('7'),
            "the reason names the deadline: {reason}"
        );
    }

    #[test]
    fn a_sigkill_this_pool_did_not_send_is_a_memory_kill() {
        let (failure, _) = classify(
            &ending(signalled(libc::SIGKILL), false, false),
            None,
            ONE_MIN,
            None,
        );
        assert_eq!(
            failure,
            Failure::Memory,
            "an unsolicited SIGKILL is the out-of-memory killer, and it is the only memory \
             death the platform names"
        );
    }

    #[test]
    fn any_other_signal_is_a_crash() {
        for signal in [libc::SIGSEGV, libc::SIGABRT, libc::SIGBUS, libc::SIGILL] {
            let (failure, reason) = classify(
                &ending(signalled(signal), false, false),
                None,
                ONE_MIN,
                None,
            );
            assert_eq!(failure, Failure::Crash, "signal {signal}");
            assert!(reason.contains(&signal.to_string()), "{reason}");
        }
    }

    #[test]
    fn a_crash_under_a_ceiling_says_the_ceiling_was_in_force() {
        // The honest half of a distinction the platform does not draw: an
        // allocation the ceiling refused arrives as SIGABRT, the same as a
        // parser fault, so the reason has to carry what the wait status cannot.
        let (failure, reason) = classify(
            &ending(signalled(libc::SIGABRT), false, false),
            Some(512 << 20),
            ONE_MIN,
            None,
        );
        assert_eq!(failure, Failure::Crash);
        assert!(reason.contains("536870912"), "{reason}");
    }

    #[test]
    fn a_worker_that_exited_without_answering_is_a_crash() {
        for status in [exited(0), exited(3)] {
            let (failure, _) = classify(&ending(status, false, false), None, ONE_MIN, None);
            assert_eq!(failure, Failure::Crash);
        }
    }

    #[test]
    fn a_stdout_that_closed_under_a_living_worker_is_not_a_timeout() {
        let (failure, reason) = classify(
            &ending(signalled(libc::SIGKILL), true, false),
            None,
            ONE_MIN,
            None,
        );
        assert_eq!(failure, Failure::Crash);
        assert!(reason.contains("stdout"), "{reason}");
    }

    #[test]
    fn a_note_leads_the_reason_without_changing_the_rule() {
        // What the wait status cannot say has to reach the journal, and it has
        // to reach it first: "the worker exited cleanly without answering" is
        // true and useless on its own when the real story is that its output
        // was not text.
        let (failure, reason) = classify(
            &ending(exited(0), false, false),
            None,
            ONE_MIN,
            Some("the worker's output could not be read: stream did not contain valid UTF-8"),
        );
        assert_eq!(failure, Failure::Crash);
        assert!(
            reason.starts_with("the worker's output could not be read"),
            "{reason}"
        );
        assert!(reason.contains("exited cleanly"), "{reason}");
    }

    const ONE_MIN: Duration = Duration::from_secs(60);
}

//! `walk_root`, end to end: a folder becomes what is findable, through the
//! real worker process, the real supervisor, and the real database.
//!
//! Phase 3 — reconciling what the walk found against what the index already
//! holds, and deleting what is genuinely gone — is exercised here too: the
//! section below marked "reconciliation" calls `f.remove` and then walks
//! again, which is the only way anything in this crate deletes a `path` row.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mnema_index::{Db, open};
use mnema_ingest::{FrozenReason, StopReason, WalkReport, walk_root};
use mnema_pool::{Pool, PoolConfig};
use mnema_walk::WalkRules;
use sha2::{Digest, Sha256};

// `worker` and `wrong_worker` live here, shared with `slice.rs` — see that
// module's own doc comment for why a second, per-file resolver is exactly
// the divergence this project has already paid for once.
mod support;

// -------------------------------------------------------------------- fixture

/// A temporary root, an index and a real pool over the real worker binary.
///
/// The worker is the built one rather than a stub: this subsystem's failures
/// live in the seam between a walk and a process, and a stub has no seam.
struct Fixture {
    dir: tempfile::TempDir,
    _index: tempfile::TempDir,
    index_path: PathBuf,
    db: Db,
    pool: Pool,
    root: i64,
}

impl Fixture {
    fn new() -> Self {
        Self::with_config(PoolConfig::new(support::worker()))
    }

    /// A pool built from a caller-chosen config over the real worker binary —
    /// for the one test that needs a lowered `max_bytes`
    /// (`a_run_of_oversized_files_does_not_look_like_a_broken_worker`).
    fn with_config(config: PoolConfig) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let index = tempfile::tempdir().unwrap();
        Self::build(dir, index, config)
    }

    /// A worker that answers every request with bytes that are not valid
    /// UTF-8, which the pool turns into `Failure::Crash` for every single
    /// file it is given (`tests/support/mod.rs`'s `wrong_worker` has the
    /// reasoning). Every file then reports `Crash`, which is exactly the
    /// shape D44 says a narrow, per-file fix cannot tell from a genuinely bad
    /// document — the counter `walk_root` owns is the general answer, and
    /// `a_worker_that_answers_nothing_useful_stops_the_walk` is what proves it
    /// actually fires rather than only proving it does not mis-fire.
    #[cfg(unix)]
    fn with_broken_worker() -> Self {
        Self::with_broken_worker_and_workers(2) // `PoolConfig::new`'s own default
    }

    /// The same broken worker, with a caller-chosen `workers` count — for the
    /// one test that needs `Pool::configured_workers()` and
    /// `Pool::live_workers()` to genuinely disagree
    /// (`the_threshold_reads_the_configured_worker_count_not_the_live_one`).
    #[cfg(unix)]
    fn with_broken_worker_and_workers(workers: usize) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let index = tempfile::tempdir().unwrap();
        // Written into the index's own scratch directory, never `dir`: `dir`
        // is about to become the watched root, and `enumerate` would list the
        // script itself as a found file if it lived there.
        let worker = support::wrong_worker(index.path(), r"printf '\377\376\n'");
        Self::build(
            dir,
            index,
            PoolConfig {
                workers,
                ..PoolConfig::new(worker)
            },
        )
    }

    fn build(dir: tempfile::TempDir, index: tempfile::TempDir, config: PoolConfig) -> Self {
        let index_path = index.path().join("index.db");
        let db = open(&index_path).unwrap();
        let root = db
            .insert_watched_root(&dir.path().display().to_string())
            .unwrap();
        let pool = Pool::new(config).unwrap();
        Self {
            dir,
            _index: index,
            index_path,
            db,
            pool,
            root,
        }
    }

    fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// Writes `contents` at `relative` inside the watched root and returns
    /// where.
    fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.dir().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    }

    /// Deletes a file the fixture previously wrote, the way a user deleting
    /// it from their own disk would — nothing about this touches the index;
    /// only the next `walk()` does.
    fn remove(&self, relative: &str) {
        std::fs::remove_file(self.dir().join(relative)).unwrap();
    }

    /// `document.id` is the sha256 of the file's bytes
    /// (`crates/mnema-extract/src/bin/worker.rs`), read straight off disk
    /// rather than out of the index — asking the index what it thinks a
    /// document's id is would be the index marking its own work.
    fn document_id_of(&self, relative: &str) -> String {
        use std::fmt::Write as _;
        let bytes = std::fs::read(self.dir().join(relative)).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        hasher
            .finalize()
            .iter()
            .fold(String::with_capacity(64), |mut s, b| {
                let _ = write!(s, "{b:02x}");
                s
            })
    }

    /// Spawns the pool's worker processes before the caller does anything
    /// timing-sensitive, and returns once they have answered.
    ///
    /// The three write-contention tests below race a lock held for a fixed
    /// number of seconds against the walk reaching its first write. Everything
    /// between those two points is meant to be the retry budget under test —
    /// but a pool that has never run pays a first-execution cost there too,
    /// and on a fresh macOS CI runner that cost is large: the worker binary
    /// was just built, is unsigned, and the system checks it on its first
    /// exec. Measured on `macos-14`, that pushed the walk past an 18-second
    /// window and inverted all three assertions at once — the contended file
    /// was simply reached after the lock had already been released, so it
    /// behaved like any other file.
    ///
    /// `extract` on a path that does not exist is the cheapest way to pay it:
    /// a worker starts, answers `Unreadable`, and nothing is written to the
    /// index. Doing this **before** the lock is taken puts the variance
    /// outside the window instead of inside it, which is what makes the
    /// number under test the retry budget rather than the runner's mood.
    fn warm_pool(&self) {
        let _ = self.pool.extract(&self.dir().join("no-such-file-warmup"));
    }

    /// Proves the precondition all three write-contention tests rest on and
    /// none of them used to state: that a second connection really is refused
    /// the write lock right now.
    ///
    /// Every one of those tests asserts a *consequence* of contention — a walk
    /// that gave up, a skip that was journalled, a cancellation noticed
    /// between retries — while assuming contention happened at all. When
    /// `macos-14` failed all three at once, the values said the assumption was
    /// what broke: a cancelled walk came back `Completed` and the write that
    /// was meant to be blocked landed, both of which mean the walk was never
    /// refused anything. An assertion about a consequence, with the premise
    /// unchecked, is the same shape as an assertion satisfied by zero.
    ///
    /// `BEGIN IMMEDIATE` is what the walk's own writers take, so this asks the
    /// exact question the walk will ask, on the exact connection it will use,
    /// and it answers within the index's busy timeout rather than hanging.
    fn assert_write_lock_is_contended(&self) {
        let refused = self.db.conn().execute_batch("BEGIN IMMEDIATE").is_err();
        if !refused {
            let _ = self.db.conn().execute_batch("ROLLBACK");
        }
        assert!(
            refused,
            "the window's BEGIN IMMEDIATE did not lock out the walk's own connection, \
             so nothing in this test is measuring contention"
        );
    }

    fn walk(&self) -> WalkReport {
        self.walk_with(&WalkRules::none())
    }

    fn walk_with(&self, rules: &WalkRules) -> WalkReport {
        walk_root(
            &self.pool,
            &self.db,
            self.root,
            self.dir(),
            rules,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap()
    }
}

// ---------------------------------------------------------------- the walk

/// The whole point of the subsystem in one test: point at a folder, and its
/// contents become findable.
#[test]
fn a_walk_indexes_what_the_folder_holds() {
    let f = Fixture::new();
    f.write("a.txt", "the quick brown fox");
    f.write("sub/b.md", "# heading\n\nsecond document");

    let report = f.walk();

    assert_eq!(report.found, 2);
    assert_eq!(report.indexed, 2);
    assert!(!f.db.search_lexical("fox", 10).unwrap().is_empty());
    assert!(!f.db.search_lexical("second", 10).unwrap().is_empty());
}

/// A second walk over an untouched folder must open nothing. The cheap arm is
/// what makes a watched folder affordable at all: 5,249 files enumerate in
/// 21.5 ms, against 3.98 ms per file if each were re-read (D40).
#[test]
fn a_second_walk_over_an_untouched_folder_reads_nothing() {
    let f = Fixture::new();
    f.write("a.txt", "hello");
    f.walk();

    let before = f.pool.worker_generation();
    let report = f.walk();

    assert_eq!(report.unchanged, 1);
    assert_eq!(report.indexed, 0);
    assert_eq!(
        f.pool.worker_generation(),
        before,
        "a worker was started for an untouched file"
    );
}

/// Progress carries a denominator, which is the whole reason the walk has two
/// phases: a multi-hour job that can only say "412 files" and not "of 5,249"
/// cannot draw a bar. And the total has to be there on the FIRST callback —
/// a window that only learns it after the first file is already read could
/// not have drawn a bar for however long that first file took.
#[test]
fn progress_knows_the_total_before_the_first_file_is_read() {
    let f = Fixture::new();
    for i in 0..5 {
        f.write(&format!("f{i}.txt"), "x");
    }
    let mut progress = Vec::new();

    let report = walk_root(
        &f.pool,
        &f.db,
        f.root,
        f.dir(),
        &WalkRules::none(),
        &AtomicBool::new(false),
        &mut |p| progress.push(p),
    )
    .unwrap();

    assert_eq!(report.indexed, 5);
    assert!(!progress.is_empty());
    assert!(progress.iter().all(|p| p.total == 5));
    assert_eq!(
        progress[0].done, 0,
        "the first callback must arrive before any file is read"
    );
}

/// Cancellation is checked between files. The file in flight is not
/// interrupted — that is the worker timeout's job, and D40 records it as open.
#[test]
fn cancellation_stops_the_walk_between_files() {
    let f = Fixture::new();
    for i in 0..20 {
        f.write(&format!("f{i}.txt"), "x");
    }
    let cancel = AtomicBool::new(false);
    let mut seen = 0;

    let report = walk_root(
        &f.pool,
        &f.db,
        f.root,
        f.dir(),
        &WalkRules::none(),
        &cancel,
        &mut |_| {
            seen += 1;
            if seen == 2 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
    )
    .unwrap();

    assert_eq!(report.stopped, StopReason::Cancelled);
    assert!(report.indexed < 20);
}

// ------------------------------------------------ rules that failed to apply

/// `walk_root` must never enter phase 2 when the exclusion rules failed to
/// combine into a working pattern set (`Walked::rules_applied`, in
/// `mnema-walk`). `rules_applied == false` means the rules may have silently
/// stopped applying for this one walk, so `found` may hold files the user
/// asked to exclude.
///
/// Nothing indexed **today** leaves this machine — there is no embedding call
/// site anywhere yet. The guard is here for the version this is being built
/// toward: D29 ships v1 with no local models, so once the embedding stage
/// lands, indexing a file means sending it to a third-party provider, and an
/// exclusion rule is the user's only way to keep a file away from that.
/// Retrofitting this onto code already written to assume it is safe is the
/// expensive order to do it in.
///
/// The recipe that flips `rules_applied` to `false` is `mnema-walk`'s own
/// (`rules_applied_is_false_when_the_combined_rule_set_is_too_large`,
/// `crates/mnema-walk/tests/rules.rs`): five prefixes, each comfortably under
/// the per-prefix size limit alone, that overflow the pattern engine once
/// combined into one override set.
#[test]
fn rules_not_applied_stops_before_any_file_is_read() {
    let f = Fixture::new();
    f.write("kept.txt", "hello");
    let huge_prefixes: Vec<String> = (0..5)
        .map(|i| format!("{}_{i}", "?".repeat(100_000)))
        .collect();
    let rules = WalkRules::new(false, false, huge_prefixes)
        .expect("each prefix alone is well under the per-prefix size limit");

    let before = f.pool.worker_generation();
    let report = f.walk_with(&rules);

    assert_eq!(report.stopped, StopReason::RulesNotApplied);
    assert_eq!(
        report.found, 1,
        "phase 1 still ran, and its count is still owed"
    );
    assert_eq!(report.indexed, 0);
    assert_eq!(
        f.pool.worker_generation(),
        before,
        "a worker was started even though the exclusion rules may not have applied"
    );
}

// ----------------------------------------------------- the broken-worker counter

/// `TooLarge` is a fact about `PoolConfig::max_bytes`, a setting, not about
/// the worker process or the machine
/// (`SkipRule::suggests_broken_environment`'s own doc comment in
/// `mnema-index` carries the reasoning) — a folder that happens to hold
/// several large files in a row must not be mistaken for a dying worker.
/// `broken_after` is `(pool.configured_workers() * 2).max(8)`, which is 8 for
/// the default two-worker pool this fixture builds — well past three.
#[test]
fn a_run_of_oversized_files_does_not_look_like_a_broken_worker() {
    let f = Fixture::with_config(PoolConfig {
        max_bytes: 4,
        ..PoolConfig::new(support::worker())
    });
    for i in 0..3 {
        f.write(
            &format!("big{i}.bin"),
            "far more than four bytes of content",
        );
    }

    let report = f.walk();

    assert_eq!(report.stopped, StopReason::Completed);
    assert_eq!(report.found, 3);
    assert_eq!(report.skipped, 3);
    assert_eq!(report.indexed, 0);
}

/// The other half of D44's asymmetry, against the real worker rather than
/// the broken one below: a folder the product genuinely cannot read, file
/// after file, must run to completion — a hundred correct "no reader for
/// this" verdicts are not a broken machine.
///
/// Not an unrecognised extension: `identify_plain_text`'s default arm
/// (`crates/mnema-extract/src/typing.rs`) treats an extension it does not
/// know as plain text and indexes it, so a `.unknownext` file earns
/// `indexed`, not a skip, from the real worker — measured directly before
/// writing this test, not assumed. What the worker actually refuses with
/// `SkipRule::Unsupported` is a format whose *magic bytes* it recognises but
/// has no reader for: the five bytes `%PDF-` are matched ahead of the
/// extension (`identify`, same module) and land on `Reader::Pdf`, which has
/// no `Vec<Block>` reader in this crate yet (`crates/mnema-extract/src/bin/worker.rs`)
/// — the file need not be a well-formed PDF beyond that signature, since the
/// worker refuses it before parsing any further.
///
/// Twenty files, not the brief's fifty: `broken_after` is 8 for this
/// fixture's default two-worker pool, so twenty clears it comfortably
/// without paying for thirty files' worth of worker round-trips the suite
/// does not need — this file already runs to about 18 s.
#[test]
fn a_run_of_unsupported_files_does_not_look_like_a_broken_worker() {
    let f = Fixture::new();
    for i in 0..20 {
        f.write(
            &format!("f{i}.pdf"),
            "%PDF-1.4\nnot a real pdf, just the magic bytes",
        );
    }

    let report = f.walk();

    assert_eq!(report.stopped, StopReason::Completed);
    assert_eq!(report.found, 20);
    assert_eq!(report.skipped, 20);
    assert_eq!(report.indexed, 0);

    // The counts alone do not say WHICH rule fired, and this test's whole
    // claim is about one particular rule being on the harmless side of
    // `suggests_broken_environment`. Without this the test passes just as
    // well on `NoTextLayer` or `TooLarge`, neither of which is what the
    // name promises to exercise.
    let rules: std::collections::BTreeSet<String> =
        f.db.skips_for_root(f.root)
            .unwrap()
            .into_iter()
            .map(|s| s.rule)
            .collect();
    assert_eq!(
        rules,
        ["unsupported".to_string()].into_iter().collect(),
        "the run must be Unsupported specifically, not merely some non-environmental rule"
    );
}

/// The counter this walk owns (D44): consecutive skips that are evidence
/// about the environment, not about any one file's bytes, stop the walk
/// rather than spend a worker process on every remaining file to learn the
/// same thing again. A worker that answers every request with bytes
/// `read_line` cannot parse reports `Crash` for every file alike — modelling
/// exactly the half-finished install or mismatched release D44 names.
///
/// Nine files, not three: `broken_after` is
/// `(pool.configured_workers() * 2).max(8)`, which is 8 for the default pool
/// `with_broken_worker` builds (`workers: 2`), so the threshold needs eight
/// consecutive `Crash` skips to trip, not two — deriving it from the live
/// worker count used to make it 2 regardless of configuration, which is
/// exactly what let two ordinary unlucky files abort a real walk.
#[cfg(unix)]
#[test]
fn a_worker_that_answers_nothing_useful_stops_the_walk() {
    let f = Fixture::with_broken_worker();
    for i in 0..9 {
        f.write(&format!("f{i}.txt"), "x");
    }

    let report = f.walk();

    assert_eq!(report.stopped, StopReason::BrokenWorker);
    assert_eq!(report.indexed, 0);
}

/// Below the threshold, a run of genuine environmental skips must not stop
/// the walk on its own — three is comfortably under 8, but was already past
/// the old `pool.live_workers().max(1) * 2` threshold (2, read before phase 2
/// had started anything, regardless of `PoolConfig::workers`), which is
/// exactly what let two ordinary unlucky files abort a walk with nothing
/// actually broken.
#[cfg(unix)]
#[test]
fn a_few_consecutive_crashes_do_not_alone_stop_the_walk() {
    let f = Fixture::with_broken_worker();
    for i in 0..3 {
        f.write(&format!("f{i}.txt"), "x");
    }

    let report = f.walk();

    assert_eq!(report.stopped, StopReason::Completed);
    assert_eq!(report.skipped, 3);
    assert_eq!(report.indexed, 0);
}

/// `configured_workers()` and `live_workers()` genuinely disagree here, which
/// the default `workers: 2` fixture above cannot show: `live_workers()` is
/// read before phase 2 touches anything and is always 0 at that point, so the
/// old formula gave a threshold of `0.max(1) * 2 = 2` no matter how the pool
/// was configured. At `workers: 2` the new formula's `.max(8)` floor hides
/// the difference too (`configured * 2 = 4`, still swallowed by the floor).
/// `workers: 8` is where the floor stops mattering: `(8 * 2).max(8) = 16`,
/// nowhere near what `live_workers()` would have given. Ten consecutive
/// `Crash` skips sit strictly between the two thresholds — past the old one,
/// short of the new one — so this only stays `Completed` if the threshold
/// really is reading the configured count.
#[cfg(unix)]
#[test]
fn the_threshold_reads_the_configured_worker_count_not_the_live_one() {
    let f = Fixture::with_broken_worker_and_workers(8);
    for i in 0..10 {
        f.write(&format!("f{i}.txt"), "x");
    }

    let report = f.walk();

    assert_eq!(report.stopped, StopReason::Completed);
    assert_eq!(report.skipped, 10);
    assert_eq!(report.indexed, 0);
}

/// A busy-exhausted file must not count toward the broken-worker threshold —
/// write contention is evidence about whoever else is holding the write
/// lock, not about the worker. One contended file plus seven genuine
/// `Crash` skips stays under the threshold (8) only because the contended
/// one was not counted; counting it would tip the eighth file over and abort
/// the walk.
#[cfg(unix)]
#[test]
fn a_busy_exhausted_file_does_not_count_toward_the_broken_worker_threshold() {
    let f = Fixture::with_broken_worker();
    // Sorts first: `enumerate` orders `found` by relative path, so this is
    // the file the walk reaches while the lock below is still held.
    f.write("a_contended.txt", "hello");
    for i in 0..7 {
        f.write(&format!("b{i}.txt"), "x");
    }

    f.warm_pool();
    let window = open(&f.index_path).unwrap();
    window.conn().execute_batch("BEGIN IMMEDIATE").unwrap();
    window.insert_watched_root("/Volumes/Second").unwrap();
    // Before the hold begins, not inside it: the probe waits out a full
    // busy-timeout window of its own, and spending that from the margin is
    // what reproduced the CI failure here.
    f.assert_write_lock_is_contended();
    let holder = std::thread::spawn(move || {
        std::thread::sleep(HOLD);
        window.conn().execute_batch("COMMIT").unwrap();
    });

    let report = f.walk();
    holder.join().unwrap();

    assert_eq!(report.stopped, StopReason::Completed);
    assert_eq!(
        report.skipped, 8,
        "the contended file plus seven genuine crashes"
    );
    assert_eq!(report.indexed, 0);
}

// --------------------------------------------------------------- write contention

/// How long the window holds the write lock in the contention tests below.
///
/// **This number sits in a corridor with a wall on each side, and only one of
/// them was written down before `macos-14` found the other.** The walk retries
/// `BUSY_RETRIES` (3) times, each waiting out the index's own five-second
/// `busy_timeout`, so the hold must outlast about fifteen seconds or the
/// retries never exhaust. It must also end *before* twenty, because the walk's
/// next act is to journal the skip — itself a write — and that write gets one
/// five-second wait of its own. Too short and the file is simply indexed; too
/// long and the walk returns `Err(Busy)` instead of a report. Eighteen is the
/// middle of a five-second corridor, not a value with slack in it.
///
/// Which is why nothing slow may sit between the lock being taken and the
/// walk's first write. That is what `macos-14` had and this machine did not:
/// a pool whose worker processes had never run, paying a first-execution cost
/// inside the corridor. Reproduced here by spending one busy-timeout window on
/// something else before the walk — the retries then finish after the hold,
/// the lock is free by the third one, and the write the test expects to be
/// refused succeeds, inverting all three assertions exactly as CI showed.
/// `Fixture::warm_pool` and running the contention probe before the hold
/// begins are what keep the corridor empty.
const HOLD: Duration = Duration::from_secs(18);

/// `IngestError::Busy` means "come back to this file," not "drop it" — its
/// own doc comment in `mnema-ingest/src/lib.rs` leaves that decision to
/// whoever owns the walk. `walk_root` retries a bounded number of times and,
/// once every attempt still finds the index busy, records a skip — so the
/// file's absence from `indexed` has a reason, instead of just being a gap
/// between `found` and every other counter that nothing explains.
///
/// The window holds the write lock for eighteen seconds. Three retry
/// attempts each wait out the index's own five-second `busy_timeout`
/// (`crates/mnema-index/src/open.rs`) before `ingest_file` answers `Busy`
/// again, so every attempt is genuinely exhausted by around the
/// fifteen-second mark; the remaining three seconds is margin for process and
/// scheduling overhead, not part of the number under test. The lock is
/// released only after that, so the walk's own attempt to record the skip —
/// itself a write — finds the lock free well inside its own five-second
/// wait, rather than exhausting a fourth window too.
#[test]
fn a_file_still_busy_after_every_retry_is_skipped_not_lost() {
    let f = Fixture::new();
    f.write("contract.txt", "hello");

    // The window's connection, holding the write lock the way `open_index` +
    // a folder being added would (mirrors
    // `a_walk_that_meets_the_window_holding_the_write_lock_is_told_to_retry`
    // in `tests/slice.rs`, held for long enough to exhaust every retry
    // instead of just the first one).
    f.warm_pool();
    let window = open(&f.index_path).unwrap();
    window.conn().execute_batch("BEGIN IMMEDIATE").unwrap();
    window.insert_watched_root("/Volumes/Second").unwrap();
    // Before the hold begins, not inside it: the probe waits out a full
    // busy-timeout window of its own, and spending that from the margin is
    // what reproduced the CI failure here.
    f.assert_write_lock_is_contended();
    let holder = std::thread::spawn(move || {
        std::thread::sleep(HOLD);
        window.conn().execute_batch("COMMIT").unwrap();
    });

    let report = f.walk();
    holder.join().unwrap();

    assert_eq!(
        report.indexed, 0,
        "the write never actually landed under contention"
    );
    assert_eq!(
        report.skipped, 1,
        "still busy after every retry, but not silently lost"
    );
    assert_eq!(
        report.stopped,
        StopReason::Completed,
        "one contended file among one must not look like a broken worker"
    );
    let skips = f.db.skips_for_root(f.root).unwrap();
    assert_eq!(skips.len(), 1);
    assert_eq!(skips[0].relative_path, "contract.txt");
    assert_eq!(skips[0].rule, "unreadable");
}

/// Cancellation is checked between busy retries too, not only between files
/// in the outer loop — a single contended file can span up to three
/// five-second attempts, and a cancel that arrives partway through must not
/// have to wait out every remaining one first.
///
/// Two assertions, because one mutation each catches and the other misses.
/// `stopped == Cancelled` is what actually fails when the per-attempt check
/// is removed: without it, the retry that runs once the lock is released at
/// 8 s just succeeds — the file gets indexed, the walk finishes normally,
/// and it does so in ~8.16 s, comfortably under the 10 s bound below, so
/// elapsed time alone would have missed this exact regression (measured).
/// The elapsed bound exists for the other shape of the same bug: a check
/// that is present but only runs somewhere infrequent — after every attempt
/// is exhausted, say — would still eventually report `Cancelled`, just after
/// the full ~15 s three attempts need, which `stopped` alone would not
/// notice and the bound below would.
#[test]
fn cancellation_is_checked_between_busy_retries_not_only_between_files() {
    let f = Fixture::new();
    f.write("contract.txt", "hello");

    f.warm_pool();
    let window = open(&f.index_path).unwrap();
    window.conn().execute_batch("BEGIN IMMEDIATE").unwrap();
    window.insert_watched_root("/Volumes/Second").unwrap();
    let cancel = AtomicBool::new(false);

    // Held past the point cancellation should already have stopped the walk
    // (one busy-timeout window, ~5 s) and released well short of what
    // exhausting all three retries would need (~15 s) — see the two
    // assertions below for what each half of that window is for.
    let elapsed = std::thread::scope(|scope| {
        scope.spawn(move || {
            std::thread::sleep(HOLD);
            window.conn().execute_batch("COMMIT").unwrap();
        });
        scope.spawn(|| {
            std::thread::sleep(Duration::from_secs(2));
            cancel.store(true, Ordering::SeqCst);
        });

        let started = Instant::now();
        let report = walk_root(
            &f.pool,
            &f.db,
            f.root,
            f.dir(),
            &WalkRules::none(),
            &cancel,
            &mut |_| {},
        )
        .unwrap();
        let elapsed = started.elapsed();
        assert_eq!(report.stopped, StopReason::Cancelled);
        elapsed
    });

    assert!(
        elapsed < Duration::from_secs(10),
        "cancellation should be noticed after the first exhausted retry attempt (~5 s), \
         not only after all three (~15 s); took {elapsed:?}"
    );
}

// ------------------------------------------------------- counters and totals

/// `indexed + unchanged + skipped` must equal `found`, and `refused` must
/// equal how many files phase 1 refused before any worker was asked — every
/// file the walk saw lands in exactly one bucket, never zero and never two.
/// A review probe found this broken: one ordinary file and three symlinks
/// produced `indexed: 1, skipped: 3` against `found: 1`, and the very first
/// progress callback claimed 300% done before a byte was read.
#[cfg(unix)]
#[test]
fn the_report_accounts_for_every_file_the_walk_saw() {
    use std::os::unix::fs::symlink;

    let f = Fixture::with_config(PoolConfig {
        max_bytes: 10,
        ..PoolConfig::new(support::worker())
    });
    f.write("unchanged.txt", "same");
    f.walk(); // seeds the index so the second walk answers Unchanged for it

    f.write("indexed.txt", "new");
    f.write("too_large.bin", "far more than ten bytes of content");
    symlink(f.dir().join("indexed.txt"), f.dir().join("a_symlink.txt")).unwrap();

    let report = f.walk();

    assert_eq!(report.found, 3, "unchanged.txt, indexed.txt, too_large.bin");
    assert_eq!(report.refused, 1, "the symlink, refused before any worker");
    assert_eq!(
        report.indexed + report.unchanged + report.skipped,
        report.found,
        "every found file must land in exactly one bucket"
    );
    assert_eq!(report.indexed, 1);
    assert_eq!(report.unchanged, 1);
    assert_eq!(report.skipped, 1);
}

/// `total` in `WalkProgress` must count every file phase 1 saw, including the
/// ones it refused before any worker was asked — not only the ones handed to
/// phase 2 — and `done` must reach `total` once a walk completes.
#[cfg(unix)]
#[test]
fn progress_total_includes_files_phase_1_refused() {
    use std::os::unix::fs::symlink;

    let f = Fixture::new();
    f.write("a.txt", "hello");
    symlink(f.dir().join("a.txt"), f.dir().join("a_symlink.txt")).unwrap();
    let mut progress = Vec::new();

    let report = walk_root(
        &f.pool,
        &f.db,
        f.root,
        f.dir(),
        &WalkRules::none(),
        &AtomicBool::new(false),
        &mut |p| progress.push(p),
    )
    .unwrap();

    assert_eq!(report.found, 1);
    assert_eq!(report.refused, 1);
    assert!(
        progress.iter().all(|p| p.total == 2),
        "total must count the refused symlink too, not only the found file: {progress:?}"
    );
    assert!(
        progress.iter().all(|p| p.refused == 1),
        "WalkProgress::refused must mirror WalkReport::refused: {progress:?}"
    );
    assert_eq!(
        progress.last().unwrap().done,
        2,
        "done must reach total on a completed walk, refused files included"
    );
}

// --------------------------------------------------------------- completeness

/// An unreadable subdirectory is, from `found` alone, indistinguishable from
/// an empty one — `WalkReport::complete` is what tells them apart, and a
/// reconciliation that deletes rows for paths absent from `found` must refuse
/// to run when it is `false` (`Walked::complete`'s own doc comment in
/// `mnema-walk` has the reasoning in full).
#[cfg(unix)]
#[test]
fn complete_is_false_when_a_subdirectory_could_not_be_read() {
    use std::os::unix::fs::PermissionsExt;

    let f = Fixture::new();
    f.write("kept.txt", "hello");
    let locked = f.dir().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("inside.txt"), "secret").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    // Root reads through any permission bits, which would make this test
    // pass for the wrong reason (or not at all) — mirrors
    // `an_unreadable_directory_marks_the_walk_incomplete` in
    // `crates/mnema-walk/tests/enumerate.rs`.
    let root_can_still_read = std::fs::read_dir(&locked).is_ok();
    if root_can_still_read {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!(
            "skipped complete_is_false_when_a_subdirectory_could_not_be_read: \
             running as root, chmod 000 has no effect"
        );
        return;
    }

    let report = f.walk();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        !report.complete,
        "an unreadable subdirectory must not look like a complete walk"
    );
    assert_eq!(
        report.stopped,
        StopReason::Completed,
        "the walk still finished; it just did not see everything"
    );
}

/// A root that is gone entirely — an ejected external drive, a folder deleted
/// since the last walk — must be told apart from an ordinary empty folder,
/// and it is not a fact about any one *file*, so it must not be journalled as
/// one under the root's own absolute path (see the comment on the pre-skip
/// loop in `src/walk.rs`).
#[test]
fn a_missing_root_is_named_apart_from_an_empty_folder() {
    let f = Fixture::new();
    let missing = f.dir().join("does-not-exist");

    let report = walk_root(
        &f.pool,
        &f.db,
        f.root,
        &missing,
        &WalkRules::none(),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.stopped, StopReason::RootUnavailable);
    assert!(!report.complete);
    assert_eq!(report.found, 0);
    assert_eq!(report.refused, 0);
    assert!(
        f.db.skips_for_root(f.root).unwrap().is_empty(),
        "a missing root is not a fact about any one file, so it must not be journalled as one"
    );
}

// ------------------------------------------------------------ reconciliation

/// A file the user deleted stops answering. Nothing else does.
#[test]
fn a_vanished_file_leaves_no_path_row() {
    let f = Fixture::new();
    f.write("gone.txt", "gone content");
    f.write("stays.txt", "stays content");
    f.walk();

    f.remove("gone.txt");
    let report = f.walk();

    assert_eq!(report.removed, 1);
    assert!(f.db.path_entry(f.root, "gone.txt").unwrap().is_none());
    assert!(f.db.path_entry(f.root, "stays.txt").unwrap().is_some());
    assert!(f.db.search_lexical("gone", 10).unwrap().is_empty());
    assert!(!f.db.search_lexical("stays", 10).unwrap().is_empty());
}

/// Two copies of one file are one document (content addressing). Deleting one
/// copy must not take the document with it — the invariant the randomised
/// harness states as "a document disappears only when its last path did".
#[test]
fn deleting_one_copy_keeps_the_document() {
    let f = Fixture::new();
    f.write("a.txt", "same bytes");
    f.write("b.txt", "same bytes");
    f.walk();

    f.remove("a.txt");
    f.walk();

    assert_eq!(f.db.path_count(&f.document_id_of("b.txt")).unwrap(), 1);
    assert!(!f.db.search_lexical("same", 10).unwrap().is_empty());
}

/// An unmounted volume and a mass delete look identical from here, and D33 says
/// the answer is a pause. Zero files under a root the index holds paths for is
/// the unmount signature — a fact, not a threshold, chosen deliberately so that
/// no invented fraction decides whether a user loses their index.
#[test]
fn an_empty_root_deletes_nothing() {
    let f = Fixture::new();
    f.write("a.txt", "content");
    f.write("b.txt", "content two");
    f.walk();

    f.remove("a.txt");
    f.remove("b.txt");
    let report = f.walk();

    assert_eq!(report.removed, 0);
    assert_eq!(report.stopped, StopReason::VolumeMissing);
    assert_eq!(f.db.paths_under_root(f.root).unwrap().len(), 2);
}

/// A walk that was cancelled has not seen the whole folder, so its list of
/// "files that were not there" is not evidence of anything.
#[test]
fn a_cancelled_walk_deletes_nothing() {
    let f = Fixture::new();
    for i in 0..10 {
        f.write(&format!("f{i}.txt"), "x");
    }
    f.walk();
    f.remove("f0.txt");

    let cancel = AtomicBool::new(false);
    let mut seen = 0;
    let report = walk_root(
        &f.pool,
        &f.db,
        f.root,
        f.dir(),
        &WalkRules::none(),
        &cancel,
        &mut |_| {
            seen += 1;
            if seen == 2 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
    )
    .unwrap();

    assert_eq!(report.removed, 0);
    assert!(f.db.path_entry(f.root, "f0.txt").unwrap().is_some());
}

/// A rule that newly excludes an indexed file removes it. Otherwise "I excluded
/// that folder" does not mean "it is no longer findable" (§5).
#[test]
fn a_newly_excluding_rule_removes_what_it_excludes() {
    let f = Fixture::new();
    f.write("private/secret.txt", "confidential text");
    f.walk_with(&WalkRules::none());
    assert!(!f.db.search_lexical("confidential", 10).unwrap().is_empty());

    f.walk_with(&WalkRules::new(false, false, vec!["private".into()]).unwrap());

    assert!(f.db.search_lexical("confidential", 10).unwrap().is_empty());
}

/// An incomplete walk has not seen the whole folder — an unreadable
/// subdirectory is, from `found` alone, indistinguishable from an empty one —
/// so a file that genuinely vanished elsewhere under the same root must not
/// be deleted on the strength of a walk that did not finish looking.
/// `complete_is_false_when_a_subdirectory_could_not_be_read` (above) pins
/// `WalkReport::complete` itself; this pins that phase 3 actually reads it.
#[cfg(unix)]
#[test]
fn an_incomplete_walk_deletes_nothing() {
    use std::os::unix::fs::PermissionsExt;

    let f = Fixture::new();
    f.write("gone.txt", "gone content");
    let locked = f.dir().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("inside.txt"), "secret").unwrap();
    f.walk();

    f.remove("gone.txt");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let root_can_still_read = std::fs::read_dir(&locked).is_ok();
    if root_can_still_read {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!(
            "skipped an_incomplete_walk_deletes_nothing: running as root, chmod 000 has no effect"
        );
        return;
    }

    let report = f.walk();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        !report.complete,
        "the locked subdirectory must break completeness"
    );
    assert_eq!(
        report.removed, 0,
        "an incomplete walk must delete nothing, even a file it is certain vanished"
    );
    assert!(
        f.db.path_entry(f.root, "gone.txt").unwrap().is_some(),
        "gone.txt must survive an incomplete walk despite being gone from disk"
    );
}

/// A pre-skip that carries a path is presence, not absence
/// (`PreSkipRule::NotMaterialised`'s own doc comment in `mnema-walk` states
/// the obligation in those words, and it binds every pre-skip rule that
/// carries a path, not only that one). A symlink is the easiest of them to
/// construct in a test: once a previously-indexed name is taken over by a
/// symlink to a file, the walk still names it — as a `NotAFile` pre-skip,
/// never as `found` — and reconciliation must read that as "still there,
/// untouched", not as "gone".
#[cfg(unix)]
#[test]
fn a_symlink_over_a_former_file_does_not_delete_it() {
    use std::os::unix::fs::symlink;

    let f = Fixture::new();
    f.write("elsewhere.txt", "elsewhere content");
    f.write("doc.txt", "docmarker text");
    f.walk();
    assert!(!f.db.search_lexical("docmarker", 10).unwrap().is_empty());

    f.remove("doc.txt");
    symlink(f.dir().join("elsewhere.txt"), f.dir().join("doc.txt")).unwrap();
    let report = f.walk();

    assert_eq!(
        report.removed, 0,
        "doc.txt is now a NotAFile pre-skip with a path, not an absence"
    );
    assert!(f.db.path_entry(f.root, "doc.txt").unwrap().is_some());
    assert!(!f.db.search_lexical("docmarker", 10).unwrap().is_empty());
}

/// A symlink to a directory is a whole subtree the walk never enters
/// (`PreSkipRule::NotAFileSubtree`). Its `relative` names only the symlink,
/// so a `known` path that used to sit under a real directory of the same
/// name — before it became this symlink — carries no evidence either way,
/// and deleting it would be deleting on absence-of-evidence, which §7
/// forbids. Distinct from the test above: there `seen` names the exact path;
/// here nothing does, and only prefix treatment protects it.
#[cfg(unix)]
#[test]
fn a_directory_symlink_protects_what_used_to_be_inside_it() {
    use std::os::unix::fs::symlink;

    let f = Fixture::new();
    f.write("linked/inner.txt", "inner content");
    f.walk();
    assert!(!f.db.search_lexical("inner", 10).unwrap().is_empty());

    std::fs::remove_dir_all(f.dir().join("linked")).unwrap();
    let elsewhere = f.dir().join("elsewhere_dir");
    std::fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, f.dir().join("linked")).unwrap();
    let report = f.walk();

    assert_eq!(
        report.removed, 0,
        "linked/inner.txt has no evidence either way, so it must not be deleted"
    );
    assert!(
        f.db.path_entry(f.root, "linked/inner.txt")
            .unwrap()
            .is_some()
    );
    assert!(!f.db.search_lexical("inner", 10).unwrap().is_empty());

    // Freezing must not be silent — `removed: 0` alone cannot be told
    // apart from a walk where nothing happened at all.
    assert_eq!(report.frozen.len(), 1);
    assert_eq!(report.frozen[0].prefix, "linked");
    assert_eq!(report.frozen[0].why, FrozenReason::SymlinkedSubtree);
}

/// `vec0` cannot be the target of a foreign key (spec §7 item 2), so nothing
/// under `document`'s `ON DELETE CASCADE` reaches a vector table — a document
/// that reconciliation deletes must have `Db::delete_vectors_for_document`
/// called for it explicitly, or its vectors outlive their chunks silently.
///
/// Nothing in the product writes a vector yet under D29 (no embedder exists),
/// which is exactly why `delete_document` never had to reach them before this
/// task. Executable now rather than an assertion that only turns red once an
/// embedder exists: builds a real space, inserts a real vector against a real
/// chunk this walk produced, deletes the file, and reads the vector table
/// back directly.
#[test]
fn reconciliation_deletes_the_documents_vectors_too() {
    let f = Fixture::new();
    f.write("vectored.txt", "vectored content");
    // `stays.txt` is what keeps the root from looking empty once
    // `vectored.txt` is gone — with only one file, deleting it would trip
    // the unmount signature (`root_is_empty`, in `src/walk.rs`) and phase 3
    // would correctly refuse to run at all, which is a different behaviour
    // than the one this test means to pin.
    f.write("stays.txt", "stays content");
    f.walk();

    let document_id = f.document_id_of("vectored.txt");
    let chunk_ids: Vec<i64> =
        f.db.conn()
            .prepare("SELECT id FROM chunk WHERE document_id = ?1")
            .unwrap()
            .query_map([document_id.as_str()], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
    assert!(
        !chunk_ids.is_empty(),
        "the file must have produced at least one chunk to make this test mean anything"
    );

    let cfg =
        f.db.create_model_config("test-model", "openrouter", None, "baai/bge-m3", 4)
            .unwrap();
    let space = f.db.create_space(cfg, 4, "chunker-v1").unwrap();
    for (i, chunk_id) in chunk_ids.iter().enumerate() {
        f.db.insert_vector(space, *chunk_id, &[1.0, 0.0, 0.0, i as f32])
            .unwrap();
    }
    let table = format!("vec_emb_{space}");
    let count_before: i64 =
        f.db.conn()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
    assert_eq!(count_before, chunk_ids.len() as i64);

    f.remove("vectored.txt");
    f.walk();

    let count_after: i64 =
        f.db.conn()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
    assert_eq!(count_after, 0, "the document's vectors must not outlive it");
}

/// Vector cleanup has to run wherever a document can lose its last path, and
/// reconciliation is not the only place that happens — an ordinary edit is,
/// by far the more common of the two. `ingest_file`'s `repoint` displaces
/// whatever document used to sit under an edited path through the same
/// `forget_if_unnamed` phase 3 calls; testing only phase 3's own door would
/// leave this one — a user saving a file — uncovered, and a later change
/// that inlined `forget_if_unnamed` back into phase 3, or added a third
/// caller, would bring the original defect back with
/// `reconciliation_deletes_the_documents_vectors_too` still green.
///
/// Measured: `repoint` is reached here through `ingest_file`'s own
/// displacement path (`crates/mnema-ingest/src/lib.rs`), not through
/// `walk_root`'s phase 3 — this walk never deletes a `path` row at all, it
/// only repoints one from an old document to a new one.
#[test]
fn an_edit_that_displaces_a_document_deletes_its_vectors_too() {
    let f = Fixture::new();
    f.write("edited.txt", "original content");
    f.walk();

    let old_document_id = f.document_id_of("edited.txt");
    let chunk_ids = chunk_ids_of(&f.db, &old_document_id);
    assert!(
        !chunk_ids.is_empty(),
        "the file must have produced at least one chunk to make this test mean anything"
    );

    let cfg =
        f.db.create_model_config("test-model", "openrouter", None, "baai/bge-m3", 4)
            .unwrap();
    let space = f.db.create_space(cfg, 4, "chunker-v1").unwrap();
    for (i, chunk_id) in chunk_ids.iter().enumerate() {
        f.db.insert_vector(space, *chunk_id, &[1.0, 0.0, 0.0, i as f32])
            .unwrap();
    }
    let table = format!("vec_emb_{space}");
    let count_before: i64 =
        f.db.conn()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
    assert_eq!(count_before, chunk_ids.len() as i64);

    // The edit: same path, different bytes, so a different content hash —
    // `edited.txt`'s only path is the one it already has, so repointing it
    // to the new document is exactly what displaces the old one.
    f.write("edited.txt", "a completely different replacement body");
    f.walk();

    let new_document_id = f.document_id_of("edited.txt");
    assert_ne!(
        new_document_id, old_document_id,
        "the edit must have actually changed the content hash, or this test proves nothing"
    );
    assert!(
        !f.db.document_exists(&old_document_id).unwrap(),
        "the old document must be gone: no path names it any more"
    );
    let count_after: i64 =
        f.db.conn()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
    assert_eq!(
        count_after, 0,
        "the displaced document's vectors must not outlive it either"
    );
}

/// The `NotAFileSubtree` prefix must protect the skip journal the same way
/// it protects `path` — not only a row that exactly matches the symlink's
/// own name (already covered by `seen`), but a STALE row for a file that
/// used to live under the directory before it became a symlink. Measured:
/// without this, replacing `linked/` with a directory symlink took a skip
/// row for `linked/skipped.pdf` straight out of the journal on the very
/// next walk, even though the walk never descended into `linked/` again to
/// re-confirm it either way — and unlike an ordinarily pruned row, nothing
/// ever re-creates it, because the walk never visits that name again.
#[cfg(unix)]
#[test]
fn a_directory_symlink_protects_journal_rows_too() {
    use std::os::unix::fs::symlink;

    let f = Fixture::new();
    // A format nothing reads earns a skip row rather than an index entry —
    // same magic-bytes trick as
    // `a_run_of_unsupported_files_does_not_look_like_a_broken_worker`.
    f.write(
        "linked/skipped.pdf",
        "%PDF-1.4\nnot a real pdf, just the magic bytes",
    );
    f.walk();
    assert_eq!(f.db.skips_for_root(f.root).unwrap().len(), 1);

    std::fs::remove_dir_all(f.dir().join("linked")).unwrap();
    let elsewhere = f.dir().join("elsewhere_dir");
    std::fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, f.dir().join("linked")).unwrap();
    f.walk();

    let remaining: Vec<String> =
        f.db.skips_for_root(f.root)
            .unwrap()
            .into_iter()
            .map(|s| s.relative_path)
            .collect();
    assert!(
        remaining.contains(&"linked/skipped.pdf".to_string()),
        "the stale skip row under the frozen subtree must survive: {remaining:?}"
    );
}

/// `WalkReport::complete` and the root-level unmount signature both miss the
/// same ambiguity one directory down: a mounted share or drive nested under
/// the watched root (not at it) can unmount and leave a readable, empty
/// directory in its place. `complete` stays true — the empty directory reads
/// fine — and the root itself is not empty, because `keep.txt` is still
/// there, so neither existing guard fires. Without a check of its own, this
/// looked identical to two files having been deleted.
#[test]
fn an_unmounted_nested_share_deletes_nothing() {
    let f = Fixture::new();
    f.write("keep.txt", "keep content");
    f.write("mnt/one.txt", "one content");
    f.write("mnt/two.txt", "two content");
    f.walk();
    assert!(!f.db.search_lexical("one", 10).unwrap().is_empty());
    assert!(!f.db.search_lexical("two", 10).unwrap().is_empty());

    // The unmount itself: both files are gone, but `mnt/` — the mountpoint
    // — is still there, readable, and now empty. Nothing here deletes the
    // directory, because a real unmount does not remove the mountpoint.
    f.remove("mnt/one.txt");
    f.remove("mnt/two.txt");
    let report = f.walk();

    assert_eq!(report.stopped, StopReason::Completed);
    assert_eq!(
        report.removed, 0,
        "an unmounted nested share must not look like two deleted files"
    );
    assert!(f.db.path_entry(f.root, "mnt/one.txt").unwrap().is_some());
    assert!(f.db.path_entry(f.root, "mnt/two.txt").unwrap().is_some());
    assert!(!f.db.search_lexical("one", 10).unwrap().is_empty());
    assert!(!f.db.search_lexical("two", 10).unwrap().is_empty());

    // Freezing must not be silent — a caller with only `removed` to look at
    // cannot tell "two files are frozen behind an ambiguity" apart from
    // "nothing happened," and this is the field that tells them apart.
    assert_eq!(report.frozen.len(), 1);
    assert_eq!(report.frozen[0].prefix, "mnt");
    assert_eq!(report.frozen[0].why, FrozenReason::EmptyDirectory);
}

/// The test above is the SHALLOW case: the missing files' own parent (`mnt`)
/// is the directory that unmounted, so the probed directory and the one
/// `resolve_ancestor` finds empty are the same string. This is the DEEP
/// case, where they are not: the missing files live two levels under the
/// mountpoint, so the directory `resolve_ancestor` is asked about
/// (`mnt/share/2024`) does not exist on disk at all — only `mnt`, the
/// ancestor it climbs to past two `NotFound`s, does. Measured directly
/// (compiling `walk_root` verbatim and running it against this exact tree):
/// the report named `mnt/share/2024` as the frozen prefix while only `mnt`
/// existed on disk, which sent a person checking the file manager to a path
/// that was never there.
#[test]
fn an_unmounted_share_freezes_the_ancestor_that_actually_exists() {
    let f = Fixture::new();
    f.write("keep.txt", "keep content");
    f.write("mnt/share/2024/one.txt", "one content");
    f.write("mnt/share/2024/two.txt", "two content");
    f.walk();
    assert!(!f.db.search_lexical("one", 10).unwrap().is_empty());
    assert!(!f.db.search_lexical("two", 10).unwrap().is_empty());

    // The unmount: the whole `mnt/share/2024` subtree is gone from disk —
    // not merely emptied — and the mountpoint reverts to a fresh, empty
    // directory at `mnt`. This is what a real unmount looks like one level
    // deeper than the shallow test above: `mnt/share` and `mnt/share/2024`
    // do not exist at all; only `mnt` does.
    std::fs::remove_dir_all(f.dir().join("mnt")).unwrap();
    std::fs::create_dir(f.dir().join("mnt")).unwrap();
    let report = f.walk();

    assert_eq!(report.stopped, StopReason::Completed);
    assert_eq!(
        report.removed, 0,
        "an unmounted nested share must not look like two deleted files"
    );
    assert!(
        f.db.path_entry(f.root, "mnt/share/2024/one.txt")
            .unwrap()
            .is_some()
    );
    assert!(
        f.db.path_entry(f.root, "mnt/share/2024/two.txt")
            .unwrap()
            .is_some()
    );

    // The protection itself covers the right files either way — this is
    // the reporting half. `report.frozen[0].prefix` must name the directory
    // the evidence actually belongs to (`mnt`, still present and readable),
    // not the probed-but-nonexistent `mnt/share/2024`.
    assert_eq!(report.frozen.len(), 1);
    assert_eq!(report.frozen[0].prefix, "mnt");
    assert!(
        f.dir().join(&report.frozen[0].prefix).is_dir(),
        "the reported prefix must name a directory that exists on disk"
    );
    assert_eq!(report.frozen[0].why, FrozenReason::EmptyDirectory);
}

/// The evidence points the opposite way from the test above, and telling the
/// two apart is the whole of `resolve_ancestor`'s job. An unmounted share
/// leaves its mountpoint PRESENT and empty; a directory removed outright is
/// ABSENT — `read_dir` on it returns `Err(NotFound)`, not `Ok` at zero
/// entries. A version of the freeze that could not tell the two apart
/// treated `NotFound` exactly like an empty directory and froze here too:
/// measured, `removed: 0` with both `path` rows still searchable after
/// `gone/` was deleted along with everything in it.
#[test]
fn a_deleted_subdirectory_removes_its_documents() {
    let f = Fixture::new();
    f.write("keep.txt", "keep content");
    f.write("gone/one.txt", "one content");
    f.write("gone/two.txt", "two content");
    f.walk();
    assert!(!f.db.search_lexical("one", 10).unwrap().is_empty());
    assert!(!f.db.search_lexical("two", 10).unwrap().is_empty());

    std::fs::remove_dir_all(f.dir().join("gone")).unwrap();
    let report = f.walk();

    assert_eq!(report.stopped, StopReason::Completed);
    assert_eq!(
        report.removed, 2,
        "a subdirectory removed outright is evidence of deletion, not the unmount ambiguity"
    );
    assert!(f.db.path_entry(f.root, "gone/one.txt").unwrap().is_none());
    assert!(f.db.path_entry(f.root, "gone/two.txt").unwrap().is_none());
    assert!(f.db.search_lexical("one", 10).unwrap().is_empty());
    assert!(f.db.search_lexical("two", 10).unwrap().is_empty());
    assert!(
        report.frozen.is_empty(),
        "a genuine deletion must not also report an ambiguity: {:?}",
        report.frozen
    );
}

/// Every chunk id a document owns, ordered — used to tell "the same rows"
/// apart from "a document that looks the same but was rebuilt from scratch,"
/// which `document_id_of` alone cannot: content addressing gives a rebuild
/// the same `document.id`, but `chunk.id` is `INTEGER PRIMARY KEY` without
/// `AUTOINCREMENT` (`crates/mnema-index/src/schema.sql`), so a rebuild does
/// not even keep its own old ids.
fn chunk_ids_of(db: &Db, document_id: &str) -> Vec<i64> {
    db.conn()
        .prepare("SELECT id FROM chunk WHERE document_id = ?1 ORDER BY id")
        .unwrap()
        .query_map([document_id], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Phase 2 must run before phase 3, and nothing before this test pinned that
/// ordering directly — a rename is the single most common action a watched
/// folder sees, and it depends on it entirely. Same bytes, new name: phase 2
/// re-ingests under the new name first, content addressing hands back the
/// same `document.id`, `path_count` rises to 2 — and only then does phase 3
/// remove the old path row, leaving 1. Reversed, `path_count` would hit 0
/// first, phase 3 would destroy the document, and the second walk would
/// rebuild it from nothing under a fresh set of chunk ids: every citation
/// into it invalidated for a rename, which is not a deletion at all.
///
/// `third.txt` is the load-bearing one, and the test is wrong without it:
/// `chunk.id` is `INTEGER PRIMARY KEY` **without** `AUTOINCREMENT`
/// (`crates/mnema-index/src/schema.sql`), so on a database holding only the
/// one renamed document a wrongly-rebuilt chunk can land back on the very
/// id the deleted one just freed — SQLite's own "reuse the highest free
/// rowid" rule, coincidentally matching `chunk_ids_before` for a reason
/// that has nothing to do with this test being right. A chunk still alive
/// at a HIGHER id than the renamed file's own — `third.txt`, written and
/// walked after `old.txt` — forces a genuine rebuild to land above it
/// instead, so the comparison below fails for the actual bug rather than
/// passing by accident (measured: this test with only `old.txt` in it
/// stayed green under the phase-3-before-phase-2 mutation described below;
/// with `third.txt` present it does not). The assertion just after
/// `chunk_ids_before` pins that property directly, so reordering the
/// writes — `third.txt` before `old.txt`, say — cannot silently restore
/// the coincidence by giving `third.txt` the lower id instead.
///
/// `first.txt` carries no such property; it is here only so the walk
/// ingests more than one document before the rename, and its own search
/// assertion at the end is incidental, not load-bearing.
#[test]
fn a_rename_keeps_the_document_and_its_chunks() {
    let f = Fixture::new();
    f.write("first.txt", "first content");
    f.walk();
    f.write("old.txt", "rename content");
    f.walk();
    f.write("third.txt", "third content");
    f.walk();

    let document_id = f.document_id_of("old.txt");
    let chunk_ids_before = chunk_ids_of(&f.db, &document_id);
    assert!(
        !chunk_ids_before.is_empty(),
        "the file must have produced at least one chunk to make this test mean anything"
    );

    // The property the whole test depends on, pinned directly rather than
    // left implicit: `third.txt`'s chunk must sit at a higher id than every
    // chunk `old.txt` produced, or a wrongly-rebuilt chunk could land back
    // on `old.txt`'s own freed id and this test would pass for the wrong
    // reason again.
    let third_chunk_ids = chunk_ids_of(&f.db, &f.document_id_of("third.txt"));
    assert!(
        !third_chunk_ids.is_empty(),
        "third.txt must have produced at least one chunk to make this test mean anything"
    );
    assert!(
        third_chunk_ids.iter().min() > chunk_ids_before.iter().max(),
        "third.txt's chunk ids {third_chunk_ids:?} must all exceed old.txt's {chunk_ids_before:?}, \
         or a rebuilt chunk could coincidentally land back on old.txt's own freed id"
    );

    std::fs::rename(f.dir().join("old.txt"), f.dir().join("new.txt")).unwrap();
    let report = f.walk();

    assert_eq!(report.removed, 1, "the old path row, and only it");
    assert!(f.db.path_entry(f.root, "old.txt").unwrap().is_none());
    assert!(f.db.path_entry(f.root, "new.txt").unwrap().is_some());
    assert_eq!(f.db.path_count(&document_id).unwrap(), 1);
    assert_eq!(
        chunk_ids_of(&f.db, &document_id),
        chunk_ids_before,
        "the same document must keep the same chunk rows, not be rebuilt from scratch"
    );
    assert!(!f.db.search_lexical("rename", 10).unwrap().is_empty());
    assert!(!f.db.search_lexical("first", 10).unwrap().is_empty());
    assert!(!f.db.search_lexical("third", 10).unwrap().is_empty());
}

//! `walk_root`, end to end: a folder becomes what is findable, through the
//! real worker process, the real supervisor, and the real database.
//!
//! Phase 3 — reconciling what the walk found against what the index already
//! holds, and deleting what is genuinely gone — is not this crate's yet
//! (spec §6, reserved for a later task). Nothing here writes a test that
//! needs it, and nothing here calls anything that deletes a `path` row.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mnema_index::{Db, open};
use mnema_ingest::{StopReason, WalkReport, walk_root};
use mnema_pool::{Pool, PoolConfig};
use mnema_walk::WalkRules;

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
/// `mnema-walk`). Under D29 there are no local models in this product, so
/// indexing a file means sending it to a third-party embedding provider — an
/// exclusion rule is the user's only way to keep a file away from that, and
/// `rules_applied == false` means the rules may have silently stopped
/// applying for this one walk. Indexing under that condition would be sending
/// files the user explicitly excluded to a third party, which is why this
/// refuses rather than proceeding on the assumption that nothing was excluded
/// anyway.
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
/// has no reader for: the four bytes `%PDF-` are matched ahead of the
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

    let window = open(&f.index_path).unwrap();
    window.conn().execute_batch("BEGIN IMMEDIATE").unwrap();
    window.insert_watched_root("/Volumes/Second").unwrap();
    let holder = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(18));
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
    let window = open(&f.index_path).unwrap();
    window.conn().execute_batch("BEGIN IMMEDIATE").unwrap();
    window.insert_watched_root("/Volumes/Second").unwrap();
    let holder = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(18));
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
            std::thread::sleep(Duration::from_secs(8));
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

//! `walk_root`, end to end: a folder becomes what is findable, through the
//! real worker process, the real supervisor, and the real database.
//!
//! Phase 3 — reconciling what the walk found against what the index already
//! holds, and deleting what is genuinely gone — is not this crate's yet
//! (spec §6, reserved for a later task). Nothing here writes a test that
//! needs it, and nothing here calls anything that deletes a `path` row.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

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
        let dir = tempfile::tempdir().unwrap();
        let index = tempfile::tempdir().unwrap();
        // Written into the index's own scratch directory, never `dir`: `dir`
        // is about to become the watched root, and `enumerate` would list the
        // script itself as a found file if it lived there.
        let worker = support::wrong_worker(index.path(), r"printf '\377\376\n'");
        Self::build(dir, index, PoolConfig::new(worker))
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
/// `broken_after` is `pool.live_workers().max(1) * 2`, which is 2 for a pool
/// that has not yet started a worker, so three consecutive `TooLarge` files
/// is already past the threshold a miscount would have tripped at the
/// second one.
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

/// The counter this walk owns (D44): consecutive skips that are evidence
/// about the environment, not about any one file's bytes, stop the walk
/// rather than spend a worker process on every remaining file to learn the
/// same thing again. A worker that answers every request with bytes
/// `read_line` cannot parse reports `Crash` for every file alike — modelling
/// exactly the half-finished install or mismatched release D44 names.
#[cfg(unix)]
#[test]
fn a_worker_that_answers_nothing_useful_stops_the_walk() {
    let f = Fixture::with_broken_worker();
    for i in 0..5 {
        f.write(&format!("f{i}.txt"), "x");
    }

    let report = f.walk();

    assert_eq!(report.stopped, StopReason::BrokenWorker);
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

//! A walk of one watched root: enumerate, ingest, reconcile.
//!
//! Two phases rather than one streaming pass, decided on a measurement:
//! enumerating 5,249 files costs 21.5 ms warm against 3.98 ms per file of
//! indexing (D40), so the extra metadata pass is 0.1% of the work and buys the
//! denominator a multi-hour job needs (spec §3).
//!
//! Phase 3 — reconciling `Walked::found` against what the index already holds
//! and deleting what is genuinely gone — is not built here. `removed` on
//! [`WalkReport`] stays `0` until it is.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use mnema_index::{Db, SkipRule};
use mnema_pool::Pool;
use mnema_walk::{Found, PreSkipRule, WalkRules, enumerate};

use crate::{IngestError, Ingested, ingest_file};

/// How many times [`ingest_with_busy_retry`] asks `ingest_file` again after a
/// `Busy` answer, before it gives up on the file for this pass and journals a
/// skip instead.
///
/// Not a sleep-and-retry of this crate's own devising: every attempt already
/// spends up to the index's own `busy_timeout` — five seconds
/// (`crates/mnema-index/src/open.rs`) — inside SQLite's busy handler before it
/// returns `Busy` at all, so stacking a manual wait on top would only be
/// waiting to wait. Three attempts is three separate five-second windows for
/// whatever else holds the write lock to finish and let go — measured at
/// 5.19 s for one ordinary write in
/// `a_walk_that_meets_the_window_holding_the_write_lock_is_told_to_retry`
/// (`crates/mnema-ingest/tests/slice.rs`). Bounded rather than unbounded
/// because a lock held for the better part of a minute is no longer the
/// "a window added a folder" case D46 was written for, and a walk of tens of
/// thousands of files cannot afford to find that out one retry at a time on
/// every single one of them.
const BUSY_RETRIES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Completed,
    Cancelled,
    /// Consecutive skips that are evidence about the environment rather than
    /// about any one file's bytes said the worker, the machine or the volume
    /// is broken rather than the files. D44 named this counter and said it
    /// belongs to whoever owns the walk. This is that owner.
    BrokenWorker,
    /// The exclusion rules failed to combine into a working pattern set for
    /// this walk (`Walked::rules_applied` in `mnema-walk`), which means
    /// `Walked::found` may hold files the user asked to exclude. Under D29
    /// there are no local models in this product, so indexing a file sends it
    /// to a third-party embedding provider — an exclusion rule is the user's
    /// only mechanism for keeping a file away from that. Proceeding on the
    /// hope that the rules did not matter for this particular folder would be
    /// exactly the silent failure `Walked::rules_applied` exists to name, so
    /// phase 2 never starts: nothing is read, nothing is sent anywhere. Not
    /// excessive caution — the alternative is a setting that fails open.
    RulesNotApplied,
}

#[derive(Debug, Clone, Copy)]
pub struct WalkProgress {
    pub done: u64,
    pub total: u64,
    pub skipped: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkReport {
    pub found: u64,
    pub indexed: u64,
    pub unchanged: u64,
    pub skipped: u64,
    pub removed: u64,
    pub stopped: StopReason,
}

pub fn walk_root(
    pool: &Pool,
    db: &Db,
    root_id: i64,
    root: &Path,
    rules: &WalkRules,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(WalkProgress),
) -> Result<WalkReport, IngestError> {
    // Phase 1. The only look at the disk.
    let walked = enumerate(root, rules);
    let total = walked.found.len() as u64;
    let mut report = WalkReport {
        found: total,
        indexed: 0,
        unchanged: 0,
        skipped: 0,
        removed: 0,
        stopped: StopReason::Completed,
    };

    // Files the walk refused before any worker: they belong in the journal,
    // so that "why is this file not in my index?" has an answer. Every
    // `PreSkipRule` maps to `SkipRule::Unreadable` here — none of the five is
    // a statement about the file's own bytes, and every one of them may stop
    // being true on the very next walk (a permission fixed, a file
    // re-materialised, a symlink replaced), which is exactly what
    // `Unreadable` means on the `SkipRule` side. Written as an exhaustive
    // `match` so that a sixth `PreSkipRule` has to be placed here by whoever
    // adds it, rather than silently falling through to a shared reason that
    // stops being true the moment the sixth variant is not equally described
    // by it.
    for pre in &walked.skipped {
        let reason = match pre.rule {
            PreSkipRule::UnrepresentableName => {
                "the file name is not valid UTF-8, so it cannot be named the way the rest of \
                 the index compares paths"
            }
            PreSkipRule::NotMaterialised => {
                "the file is a cloud placeholder that has not been downloaded to this disk"
            }
            PreSkipRule::Unreadable => {
                "the walker could not read this entry: permission denied, it vanished mid-walk, \
                 or its size does not fit"
            }
            PreSkipRule::NotAFile => {
                "not a regular file or a directory: a symlink, a dangling symlink, a FIFO, a \
                 socket or a device"
            }
            PreSkipRule::NotAFileSubtree => {
                "a symlink to a directory; the walk does not follow it, so nothing under it was \
                 visited"
            }
        };
        // `pre.relative` is `None` only when no `String` at all can name the
        // failure in the `/`-separated form the rest of the journal keys on
        // (see `PreSkip`'s own doc comment in `mnema-walk`) — a walker error
        // with no path to peel, or a name that is not valid UTF-8, which is
        // `UnrepresentableName`'s whole point. `pre.detail` is recorded in
        // that case instead, purely so a person reading the journal has
        // something to look at; it can never equal a `Found::relative` a
        // later walk produces, so a reconciliation that matches the skip
        // journal against what the walk found will never see this row as
        // resolved, and will re-touch it as "still missing" on every future
        // walk. That is a known, accepted limitation of this row, not a bug
        // in the reconciliation that reads it.
        let key = pre.relative.as_deref().unwrap_or(&pre.detail);
        db.record_skip(root_id, key, None, reason, SkipRule::Unreadable, None)?;
        report.skipped += 1;
    }

    // The exclusion rules may not have applied to this walk at all — see
    // `StopReason::RulesNotApplied`'s own doc comment. Checked after the
    // pre-skip journal above, not before it: recording those rows sends
    // nothing anywhere — each one is a local sqlite write about a file the
    // walk never opened — so it carries none of the risk `rules_applied`
    // exists to guard against, whether or not this particular file would
    // have been excluded by a working rule set. What must not happen next is
    // asking a worker to read a file, which is why this gate sits here,
    // before phase 2 rather than before phase 1.
    if !walked.rules_applied {
        report.stopped = StopReason::RulesNotApplied;
        return Ok(report);
    }

    // Phase 2.
    let mut consecutive_environmental = 0usize;
    let broken_after = pool.live_workers().max(1) * 2;

    // Emitted before the loop so a caller learns `total` — and that nothing
    // has been read yet — before the first file is opened, not only after
    // it. A window drawing a progress bar from the first callback needs a
    // `done: 0` sample to draw from; without this one, the first sample it
    // would ever see already has one file behind it.
    on_progress(WalkProgress {
        done: 0,
        total,
        skipped: report.skipped,
    });

    for found in &walked.found {
        if cancel.load(Ordering::SeqCst) {
            report.stopped = StopReason::Cancelled;
            return Ok(report);
        }

        match ingest_with_busy_retry(pool, db, root_id, found)? {
            Ingested::Indexed { .. } | Ingested::AlreadyIndexed { .. } => {
                report.indexed += 1;
                consecutive_environmental = 0;
            }
            Ingested::Unchanged { .. } => {
                report.unchanged += 1;
                consecutive_environmental = 0;
            }
            Ingested::Skipped { rule } => {
                report.skipped += 1;
                if rule.suggests_broken_environment() {
                    consecutive_environmental += 1;
                    if consecutive_environmental >= broken_after {
                        report.stopped = StopReason::BrokenWorker;
                        return Ok(report);
                    }
                } else {
                    // A folder of scans, or of files over the size ceiling,
                    // is not a broken worker.
                    consecutive_environmental = 0;
                }
            }
        }

        on_progress(WalkProgress {
            done: report.indexed + report.unchanged,
            total,
            skipped: report.skipped,
        });
    }

    // Phase 3 lands in a later task.
    Ok(report)
}

/// Calls [`ingest_file`], retrying up to [`BUSY_RETRIES`] times while the
/// index answers [`IngestError::Busy`]. If every attempt still finds it busy,
/// the file is journalled as an environmental skip rather than left uncounted
/// — [`IngestError::Busy`]'s own doc comment leaves "come back to the file"
/// to whoever owns the walk, and this is that: a bounded number of comebacks,
/// then an honest record instead of a silent gap between `found` and every
/// other counter.
///
/// Recorded with [`SkipRule::Unreadable`] — the closest existing member of
/// the closed vocabulary to "this file could not be read on this pass, for a
/// reason that says nothing about its bytes and may not repeat next time",
/// which is also exactly what a database busy under someone else's write is.
/// `Unreadable` never displaces what a path already names (`displaces`, in
/// this crate's `lib.rs`), so there is nothing to look up here first: a busy
/// database says nothing about whether the file's content changed, only that
/// this pass could not find out.
///
/// If recording the skip *also* meets contention — the same write lock, still
/// held — that failure is not retried again here and propagates out as
/// `IngestError::Busy`, ending the walk. Two independent writes both meeting
/// sustained contention is no longer the "a window added a folder" shape
/// `BUSY_RETRIES` is sized for; surfacing it as an explicit error the caller
/// can retry the whole walk over is more honest than a second unbounded retry
/// loop layered on the first.
fn ingest_with_busy_retry(
    pool: &Pool,
    db: &Db,
    root_id: i64,
    found: &Found,
) -> Result<Ingested, IngestError> {
    let mut last_busy = None;
    for attempt in 1..=BUSY_RETRIES {
        match ingest_file(
            pool,
            db,
            root_id,
            &found.absolute,
            &found.relative,
            Some(found.on_disk),
        ) {
            Err(IngestError::Busy(err)) => {
                last_busy = Some(err);
                if attempt < BUSY_RETRIES {
                    continue;
                }
            }
            other => return other,
        }
    }
    let last_busy = last_busy
        .expect("the loop above only runs out of attempts by taking the Busy arm every time");
    let reason = format!(
        "the index was still busy after {BUSY_RETRIES} attempts to write to it: {last_busy}"
    );
    db.record_skip(
        root_id,
        &found.relative,
        None,
        &reason,
        SkipRule::Unreadable,
        None,
    )?;
    Ok(Ingested::Skipped {
        rule: SkipRule::Unreadable,
    })
}

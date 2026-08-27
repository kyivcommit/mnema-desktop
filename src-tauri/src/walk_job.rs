//! The real walk, run as a job — `start_probe_job`'s shape, over
//! `mnema_ingest::walk_root` instead of forty units of nothing.
//!
//! Kept apart from `bridge.rs` rather than folded into it because this one
//! command pulls in three crates (`mnema-core`, `mnema-pool`, `mnema-walk`)
//! none of the other commands need, and because translating a `WalkReport`
//! into what the window reads is enough logic on its own to want a file that
//! is *only* that translation.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use mnema_ingest::{FrozenReason, StopReason, WalkReport, walk_root};
use mnema_pool::{Pool, PoolConfig};
use mnema_walk::WalkRules;
use tauri::State;
use tauri::ipc::Channel;

use crate::error::Error;
use crate::job::{self, EndReason, Ended, Frozen, JobEvent, Progress};
use crate::state::AppState;

/// `(async)` for the reason given on [`crate::bridge::open_index`]: unlike
/// `start_probe_job`, this command reads the root's path through
/// `with_index` before it ever spawns a thread, and every other caller of
/// `with_index` in this crate is `(async)` for exactly that reason — a
/// window-issued command that can wait on the same mutex must not be the one
/// left free to run inline on the main thread. Claiming the slot and
/// spawning the walk's own OS thread are cheap either way; what moved this
/// off `start_probe_job`'s blocking shape is the lookup in front of them.
///
/// Every fallible step below runs **before** [`AppState::claim_job`], not
/// after: an unknown `root_id` (a folder removed by a second window, a stale
/// id a reloaded page still has) or a `Pool` that refuses its own config
/// must be refused without ever taking the slot. Claiming it first and
/// releasing it on the first `?` gives the same end state one command later,
/// but for as long as this call runs `job_status` would report a job
/// running for a call that was always going to fail — a page polling it at
/// the wrong moment sees a lie, however short-lived.
#[tauri::command(async)]
pub fn start_walk_job(
    state: State<'_, AppState>,
    root_id: i64,
    on_progress: Channel<JobEvent>,
) -> Result<(), Error> {
    let root = state
        .with_index(|db| db.watched_root_path(root_id))?
        .ok_or(Error::UnknownWatchedRoot(root_id))?;
    let root = PathBuf::from(root);

    // No exclusion-rule command exists yet — nothing in this task adds one —
    // so every watched folder walks with the built-in list and `.gitignore`
    // on and no user prefixes. **Provisional**: the smallest default that
    // lets a walk run at all, not a stand-in for the settings UI the
    // interface spec still owes.
    // `.expect` rather than `?`: `validate_prefix` only ever runs over
    // `user_prefixes`, which is empty here, so `RulesError` cannot be
    // produced by this call — not "should not", cannot, by inspection of
    // `WalkRules::new`'s own loop.
    let rules = WalkRules::new(true, true, Vec::new())
        .expect("no user prefixes are passed, so validate_prefix never runs");

    // `Pool::new` never touches the worker path — it opens the diagnostics
    // file, if any, and allocates empty slots. A worker that does not exist
    // at `state.worker_path()` is discovered on the first `extract()` call,
    // inside `walk_root`, and surfaces as an ordinary `IngestError` handled
    // below, not here.
    let pool = Pool::new(PoolConfig::new(state.worker_path()))
        // Reuses `mnema_ingest::IngestError`'s own `Pool` variant rather than
        // adding a second `Error` case that would say the same thing in
        // different words: a pool that refuses its config and a pool that
        // dies mid-walk are both "the extraction pool cannot continue," which
        // is exactly that variant's own message.
        .map_err(mnema_ingest::IngestError::Pool)?;

    let slot = state.claim_job()?;

    // The job's own connection, not the window's — see `AppState::
    // open_job_index`'s own doc comment. A walk is a sequence of writes that
    // can run for hours; the window must keep answering searches while it
    // does. After `claim_job`, unlike everything above: releasing a slot
    // this command already holds on the way out is `JobSlot::drop`'s job,
    // not a second thing to get right here.
    let job_db = state.open_job_index()?;

    std::thread::spawn(move || {
        // The last count, and the last total, the window was actually shown —
        // read on every failure path below, not only the panic one. Updated
        // together, and only after a successful send, for the same reason
        // `start_probe_job` records `reported` only then: `Ended::failed`
        // promises what the window *saw*, not the loop's internal position.
        let reported = AtomicU64::new(0);
        let last_total = AtomicU64::new(0);
        let started = Instant::now();
        // Throttling state for the progress closure below, mirroring
        // `job::run_probe`'s own `last_report` exactly — see the `due` check
        // inside the closure for why. A plain local rather than another
        // atomic: `walk_root` calls this closure synchronously, on this one
        // thread, so nothing else can race it.
        let mut last_report: Option<Instant> = None;

        // `AssertUnwindSafe`: unlike the probe, this closure's body reaches
        // into `pool` and `job_db` as well as the channel and two atomics —
        // but all four are used only inside this one thread, for only as
        // long as this call runs, and every one of them is dropped the
        // moment the thread ends, caught panic or not. Nothing downstream
        // ever observes them again in whatever state an unwind left them in.
        // `mnema-extract` is what this thread calls through FFI by way of
        // the pool's worker process, which is the panic this exists for —
        // the probe cannot panic at all.
        let caught = catch_unwind(AssertUnwindSafe(|| {
            walk_root(
                &pool,
                &job_db,
                root_id,
                &root,
                &rules,
                slot.cancel_flag(),
                &mut |progress| {
                    let done = progress.done;
                    let total = progress.total;

                    // `walk_root` calls this once per file — `job::
                    // REPORT_INTERVAL`'s own doc comment names this exact
                    // shape: "a folder of a hundred thousand files would put
                    // a hundred thousand messages through the IPC to move a
                    // bar the user reads four times a second anyway." The
                    // probe already avoids this inside `run_probe`; nothing
                    // inside `walk_root` throttles on its caller's behalf, so
                    // this closure is where it has to happen for a real
                    // walk — `job::progress_is_due` is the same rule
                    // `run_probe`'s own loop uses, pulled out so both share
                    // one tested definition of "due" rather than two.
                    let now = Instant::now();
                    // `0` refused: a walk gives nothing up for good. Its own
                    // `WalkProgress::refused` is a file phase 1 declined to
                    // open, merged into `skipped` below, and a different fact
                    // from the one `job::Progress::refused` carries — that
                    // field's own doc comment is where the two are told apart.
                    if !job::progress_is_due(last_report, now, job::REPORT_INTERVAL, done, 0, total)
                    {
                        return;
                    }
                    last_report = Some(now);

                    if on_progress
                        .send(JobEvent::Progress(Progress {
                            done,
                            total,
                            // `WalkProgress` counts a phase-1 refusal
                            // (`refused`) apart from a phase-2 skip
                            // (`skipped`) because the two are refused for
                            // different reasons — but the live bar draws one
                            // number, and the itemised difference is what
                            // `skips` (`bridge.rs`) reads from the journal,
                            // not what the bar is for. Nothing is lost: the
                            // journal, not the bar, is the record.
                            skipped: progress.skipped + progress.refused,
                            // Not `progress.refused`, which is already inside
                            // `skipped` one line up. See
                            // `job::Progress::refused`.
                            refused: 0,
                            seconds_left: job::seconds_left(done, total, started.elapsed()),
                        }))
                        .is_ok()
                    {
                        reported.store(done, Ordering::Relaxed);
                        last_total.store(total, Ordering::Relaxed);
                    }
                },
            )
        }));

        let ending = match caught {
            Ok(Ok(report)) => ended_from_report(&report),
            // Neither arm below has a `WalkReport` to read `frozen` or the
            // counters from — `walk_root` returned `Err`, or never returned
            // at all — so both fall back to the same "last count the window
            // was shown" `Ended::failed` already gives a panic, which is what
            // an unexplained stop **is** from the window's side regardless of
            // which of the two produced it. Each still has its own text,
            // though: `IngestError`'s `Display` for the one that returned an
            // error, and the caught payload — via `job::panic_message` — for
            // the one that unwound. Without either, `reason: "failed"` is all
            // a window can say, which is the gap task 12's review named
            // first: a missing worker binary, a broken pool and a panic all
            // arriving as the same bare word.
            Ok(Err(ingest_error)) => job::Ended::failed(
                reported.load(Ordering::Relaxed),
                last_total.load(Ordering::Relaxed),
                ingest_error.to_string(),
            ),
            Err(panic) => job::Ended::failed(
                reported.load(Ordering::Relaxed),
                last_total.load(Ordering::Relaxed),
                job::panic_message(&*panic),
            ),
        };
        // Dropped **before** the send, not left to the end of this closure:
        // `JobSlot::drop` is what clears `AppState::running`, and a window is
        // free to re-enable Start inside the very handler that receives this
        // `Ended` message. A click landing in the gap between
        // the send and an implicit end-of-scope drop races a slot this
        // thread still holds. Measured directly before this line existed: a
        // second `start_walk_job` issued the instant the first `Ended`
        // arrived was refused with `a job is already running`, even though
        // the window had just been told the first one was over. The
        // ordering is enough on its own — `on_progress.send` happens after
        // `slot`'s drop in this same thread's program order, and the
        // receiving thread's `recv` synchronises with that send, so
        // whatever this thread observes about `running` by the time it
        // drops `slot` is what the receiving thread sees too.
        drop(slot);
        let _ = on_progress.send(JobEvent::Ended(ending));
        // `pool` and `job_db` are dropped here, at the end of the closure:
        // the pool's workers are asked to exit and the job's own connection
        // closes. Neither gates a second job's ability to start — only the
        // slot does — so there is no reason to hurry them the way `slot`
        // was hurried above.
    });

    Ok(())
}

/// Translates a finished walk into what the channel sends.
///
/// `total` and `done` are recomputed from the report's own counters rather
/// than carried from the last progress event, because `WalkReport` makes them
/// derivable for every `StopReason` including the one no progress event is
/// ever sent for: `RootUnavailable` returns before phase 1 runs at all, with
/// `found` and `refused` both `0`, so the same formula gives `0/0` there too
/// without a special case.
///
/// `found + refused` **is** `total`: the pre-skip loop in `walk_root` that
/// increments `refused` runs unconditionally right after phase 1, before any
/// of the early returns below phase 1 can fire, so `refused` always equals
/// the number of phase-1 refusals by the time this reads it. `indexed +
/// unchanged + skipped + refused` is `done` for the same reason — each term
/// only ever grows by exactly the work phase 2 finished before the walk
/// stopped, cancelled, broken worker, or completed alike.
///
/// `report.complete` crosses unchanged, deliberately not folded into
/// `reason` or dropped: it is the one field that tells a `Completed` walk
/// that saw everything apart from a `Completed` walk that did not — see
/// `Ended::complete`'s own doc comment for the exact shape (an unreadable
/// subdirectory) that would otherwise reach the window looking identical to
/// a clean walk. A review round found this field read from `WalkReport` and
/// never written to `Ended` at all; the unit tests below pin every field
/// this function reads, not only the ones a first pass happened to wire up.
///
/// `report.indexed` and `report.unchanged` cross the same way, separately
/// from each other and from `done`: `done` merges them with `skipped` and
/// `refused` (the total count a bar advances by), which is the right number
/// for a bar and the wrong one for the sentence "added 12 documents, 30
/// unchanged" — a sentence `done` alone cannot produce no matter how it is
/// worded, because the two counts it merges are already gone by the time it
/// is computed.
///
/// `message` is always `None` here: this function only ever runs on
/// `Ok(Ok(report))`, the one outcome where the walk itself decided the
/// ending rather than failing unexplained, so there is no failure text to
/// carry — see `Ended::message`'s own doc comment for where one comes from.
fn ended_from_report(report: &WalkReport) -> Ended {
    let total = report.found + report.refused;
    let done = report.indexed + report.unchanged + report.skipped + report.refused;
    let reason = match report.stopped {
        StopReason::Completed => EndReason::Completed,
        StopReason::Cancelled => EndReason::Cancelled,
        StopReason::BrokenWorker => EndReason::BrokenWorker,
        StopReason::RulesNotApplied => EndReason::RulesNotApplied,
        StopReason::RootUnavailable => EndReason::RootUnavailable,
        StopReason::VolumeMissing => EndReason::VolumeMissing,
    };
    let frozen = report
        .frozen
        .iter()
        .map(|f| Frozen {
            prefix: f.prefix.clone(),
            reason: frozen_reason(f.why),
        })
        .collect();
    Ended {
        reason,
        done,
        total,
        // Same merge `Progress` makes for the same reason — see the comment
        // where the live progress event is built, above.
        skipped: report.skipped + report.refused,
        // A walk gives no unit up for good; `report.refused` is a file it
        // declined to open, and it is already inside `skipped`.
        refused: 0,
        complete: report.complete,
        frozen,
        indexed: report.indexed,
        unchanged: report.unchanged,
        removed: report.removed,
        message: None,
    }
}

/// Maps `mnema_ingest`'s own `FrozenReason` onto [`job::FrozenReason`] — the
/// closed vocabulary crosses; the sentence a person reads about it does not.
/// See [`job::FrozenReason`]'s own doc comment for why the words moved to
/// the window rather than staying here as `Ended.frozen[_].why`, which is
/// what this function replaced.
fn frozen_reason(why: FrozenReason) -> job::FrozenReason {
    match why {
        FrozenReason::SymlinkedSubtree => job::FrozenReason::SymlinkedSubtree,
        FrozenReason::EmptyDirectory => job::FrozenReason::EmptyDirectory,
        FrozenReason::UnreadableDirectory => job::FrozenReason::UnreadableDirectory,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `WalkReport` with values distinct enough from each other and from
    /// their own defaults that a swapped field, not only a dropped one,
    /// would show up in a failing assertion.
    fn report(stopped: StopReason) -> WalkReport {
        WalkReport {
            found: 8,
            indexed: 5,
            unchanged: 1,
            skipped: 2,
            refused: 3,
            removed: 4,
            frozen: Vec::new(),
            complete: true,
            stopped,
        }
    }

    /// Pins the one thing a review round found missing entirely by mutation:
    /// with every `StopReason` arm below collapsed to `EndReason::Completed`
    /// and `frozen` replaced by `Vec::new()`, all seventeen tests in
    /// `tests/commands.rs` still passed, because nothing exercised any walk
    /// that stopped for a reason other than `Completed`. This test needs no
    /// filesystem, no fixture, and no real walk to fail on that mutation —
    /// only `ended_from_report` itself.
    #[test]
    fn every_stop_reason_becomes_its_own_end_reason() {
        let cases = [
            (StopReason::Completed, EndReason::Completed),
            (StopReason::Cancelled, EndReason::Cancelled),
            (StopReason::BrokenWorker, EndReason::BrokenWorker),
            (StopReason::RulesNotApplied, EndReason::RulesNotApplied),
            (StopReason::RootUnavailable, EndReason::RootUnavailable),
            (StopReason::VolumeMissing, EndReason::VolumeMissing),
        ];
        for (stopped, expected) in cases {
            assert_eq!(
                ended_from_report(&report(stopped)).reason,
                expected,
                "StopReason::{stopped:?} did not become EndReason::{expected:?}"
            );
        }
    }

    /// The critical case: `WalkReport::complete` must cross to `Ended.
    /// complete` unchanged, in both directions — not defaulted to `true` (the
    /// value that reads as "reconciliation is trustworthy") for a walk that
    /// never claimed it.
    #[test]
    fn completeness_crosses_the_seam_unchanged() {
        let mut walked = report(StopReason::Completed);
        walked.complete = true;
        assert!(ended_from_report(&walked).complete);

        walked.complete = false;
        assert!(
            !ended_from_report(&walked).complete,
            "an incomplete walk must not report as one that saw everything, \
             even when it otherwise stopped `Completed`"
        );
    }

    #[test]
    fn frozen_prefixes_cross_the_seam_with_a_discriminant_the_window_can_translate() {
        let mut walked = report(StopReason::Completed);
        walked.frozen = vec![mnema_ingest::Frozen {
            prefix: "mnt/share".to_string(),
            why: FrozenReason::EmptyDirectory,
        }];

        let ended = ended_from_report(&walked);
        assert_eq!(ended.frozen.len(), 1);
        assert_eq!(ended.frozen[0].prefix, "mnt/share");
        // The exact variant, not merely `Some`: `each_frozen_reason_maps_to_
        // its_own_discriminant` below is what pins `frozen_reason` itself,
        // but this is what proves `ended_from_report` actually calls it with
        // the RIGHT `FrozenReason` for this entry — a weaker check here
        // would pass even if a review round swapped `SymlinkedSubtree`'s and
        // `EmptyDirectory`'s own mappings at the source.
        assert_eq!(
            ended.frozen[0].reason,
            frozen_reason(FrozenReason::EmptyDirectory)
        );
    }

    /// The mapping itself, pinned pairwise: three arms that all mapped to the
    /// same `job::FrozenReason` would pass a test that only checked "some
    /// variant came back," which would send a person with a symlinked
    /// subtree looking for an unmounted share exactly the way the free-text
    /// version of this bug (fixed alongside this change) could have.
    #[test]
    fn each_frozen_reason_maps_to_its_own_discriminant() {
        let symlink = frozen_reason(FrozenReason::SymlinkedSubtree);
        let empty = frozen_reason(FrozenReason::EmptyDirectory);
        let unreadable = frozen_reason(FrozenReason::UnreadableDirectory);

        assert_eq!(symlink, job::FrozenReason::SymlinkedSubtree);
        assert_eq!(empty, job::FrozenReason::EmptyDirectory);
        assert_eq!(unreadable, job::FrozenReason::UnreadableDirectory);
        assert_ne!(symlink, empty);
        assert_ne!(symlink, unreadable);
        assert_ne!(empty, unreadable);
    }

    /// Gap 2 from the task-12 review: `done` merges `indexed` and `unchanged`
    /// with `skipped` and `refused`, so a window that only had `done` could
    /// not write "added 12, 30 unchanged" — it had nothing to derive either
    /// number from. This is what proves both cross separately, with values
    /// distinct enough from `done` and from each other that a dropped or
    /// swapped field would show up here rather than in `done` alone.
    #[test]
    fn indexed_and_unchanged_cross_the_seam_separately_from_done() {
        let ended = ended_from_report(&report(StopReason::Completed));
        assert_eq!(ended.indexed, 5);
        assert_eq!(ended.unchanged, 1);
        assert_ne!(ended.indexed, ended.done);
        assert_ne!(ended.unchanged, ended.done);
    }

    /// `WalkReport::removed` used to be computed by phase 3 and then dropped
    /// at exactly this seam: `Ended` had no field for it, so a walk that
    /// deleted four hundred `path` rows and a walk that deleted none reached
    /// the window identically. This is what proves `removed` crosses like
    /// `indexed` and `unchanged` do, with a value distinct from every other
    /// field on `report(..)` so a swap — not only a drop — would fail here.
    #[test]
    fn removed_crosses_the_seam_separately_from_done() {
        let ended = ended_from_report(&report(StopReason::Completed));
        assert_eq!(ended.removed, 4);
        assert_ne!(ended.removed, ended.done);
    }

    /// `message` is the field `ended_from_report` never sets — it belongs to
    /// the two `Ended::failed` call sites in `start_walk_job`, which have
    /// actual failure text to give it. A walk the core itself decided the
    /// ending for has none to invent.
    #[test]
    fn a_walk_reported_by_ended_from_report_carries_no_failure_message() {
        assert_eq!(
            ended_from_report(&report(StopReason::Completed)).message,
            None
        );
    }

    #[test]
    fn done_and_total_include_phase_one_refusals_and_skipped_merges_both_kinds() {
        let ended = ended_from_report(&report(StopReason::Completed));
        // found: 8, refused: 3
        assert_eq!(ended.total, 11);
        // indexed: 5, unchanged: 1, skipped: 2, refused: 3
        assert_eq!(ended.done, 11);
        // skipped: 2, refused: 3 — the same merge `Progress` makes
        assert_eq!(ended.skipped, 5);
    }

    /// `RootUnavailable` is the one `StopReason` `walk_root` returns before
    /// phase 1 ever runs, with `found` and `refused` both `0` — the formula
    /// above must not need a special case for it.
    #[test]
    fn root_unavailable_reports_zero_of_zero() {
        let mut walked = report(StopReason::RootUnavailable);
        walked.found = 0;
        walked.indexed = 0;
        walked.unchanged = 0;
        walked.skipped = 0;
        walked.refused = 0;

        let ended = ended_from_report(&walked);
        assert_eq!(ended.done, 0);
        assert_eq!(ended.total, 0);
        assert_eq!(ended.reason, EndReason::RootUnavailable);
    }
}

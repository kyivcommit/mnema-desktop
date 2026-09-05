//! What a real walk produced, translated into the shell's own vocabulary —
//! [`Ended`] and [`job::FrozenReason`] — for [`crate::scan_job`]'s reading
//! pass to build its per-folder outcome from.
//!
//! ⚠️ **This file no longer runs a walk itself.** Until Task 3b it was also
//! `start_walk_job`, the window-driven command one folder at a time —
//! `#[tauri::command(async)] pub fn start_walk_job(state, root_id,
//! on_progress: Channel<JobEvent>)`, claiming one job slot per folder and
//! chaining the embedding pass over a channel the window listened on. It was
//! unregistered from `invoke_handler!` when [`crate::scan_job`]'s reading
//! pass took over every watched folder under one claim (that file's own
//! header has the argument — a Stop lost in the gap between two claims), and
//! deleted here once the ~48 test functions across `tests/commands.rs`,
//! `tests/model_commands.rs` and `tests/mask_differential.rs` that had driven
//! it as a fixture moved onto the scan job instead. `ui/` still names it
//! until Task 6 rewrites the window side. What is left is the translation
//! this file was always more than half of — [`ended_from_report`] and
//! [`frozen_reason`] — which `scan_job.rs` still calls for every folder it
//! reads.

use mnema_ingest::{FrozenReason, StopReason, WalkReport};

use crate::job::{self, EndReason, Ended, Frozen};

/// Translates a finished walk into the shell's own [`Ended`], which
/// [`crate::scan_job::root_outcome`] then folds into one folder's row.
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
///
/// ⚠️ **No late-Stop rewrite here.** The command this function was written
/// for, `start_walk_job`, used to read the cancellation flag itself after
/// `walk_root` returned and report a completed walk as `Cancelled` when a Stop
/// had landed too late for the walk to see it on its own — one folder was one
/// job, and the slot changed hands the moment it answered, so that read was
/// the only place left to catch it. The scan job reads every folder under ONE
/// claim instead, and holds the SAME property one level up: `scan_job.rs`'s
/// D-h rewrites the whole PASS's reason at the boundary after the last
/// folder's report, not this function's per-folder one, because a Stop
/// landing after one folder's report still leaves folders unread that the
/// pass's own top-of-loop check will catch on the next iteration. A per-folder
/// rewrite here would be the wrong layer twice over: too early for a Stop
/// landing after this folder but before the next one starts, and redundant
/// with D-h for a Stop landing after the very last folder.
///
/// `contended` is the one counter that is **not** in the report and cannot be:
/// `WalkReport` has no such field, because contention is announced once,
/// through the progress callback, at the moment the last busy retry is refused.
/// Every caller of that callback throttles it, so the caller is the only place
/// the number survives — see [`Ended::contended`] for the rule and
/// `crate::scan_job::RootProgress` for the counter that keeps it.
pub(crate) fn ended_from_report(report: &WalkReport, contended: u64) -> Ended {
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
        contended,
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
pub(crate) fn frozen_reason(why: FrozenReason) -> job::FrozenReason {
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

    /// The contention count reaches the ending from the caller, and only from
    /// the caller.
    ///
    /// The pair of states this separates is "a walk that met a held write lock"
    /// from "a walk that met none", on two reports that are otherwise the same
    /// byte for byte — `WalkReport` carries nothing about contention, so the
    /// only thing that can differ is the argument. An implementation that
    /// dropped the argument and wrote `0` passes the second assertion and fails
    /// the first; one that wrote `skipped` there passes the first and fails the
    /// second.
    #[test]
    fn contention_reaches_the_ending_from_the_caller_and_not_from_the_report() {
        assert_eq!(
            ended_from_report(&report(StopReason::Completed), 2).contended,
            2,
            "the count the caller kept was not carried into the ending"
        );
        assert_eq!(
            ended_from_report(&report(StopReason::Completed), 0).contended,
            0,
            "a walk that met no lock must report none"
        );
        // The rule `Ended::contended` states, on the fixture that can break it:
        // `report(..)` skips 2 and refuses 3, so the ending's `skipped` is 5.
        let ended = ended_from_report(&report(StopReason::Completed), 5);
        assert!(
            ended.contended <= ended.skipped,
            "contended {} is above skipped {}, so a surface adding the two \
             would count files twice",
            ended.contended,
            ended.skipped
        );
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
                ended_from_report(&report(stopped), 0).reason,
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
        assert!(ended_from_report(&walked, 0).complete);

        walked.complete = false;
        assert!(
            !ended_from_report(&walked, 0).complete,
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

        let ended = ended_from_report(&walked, 0);
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
        let ended = ended_from_report(&report(StopReason::Completed), 0);
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
        let ended = ended_from_report(&report(StopReason::Completed), 0);
        assert_eq!(ended.removed, 4);
        assert_ne!(ended.removed, ended.done);
    }

    /// `message` is the field `ended_from_report` never sets — it belongs to
    /// callers that have actual failure text to give it, like
    /// `scan_job::failed_root`'s `..Ended::failed(0, 0, message)`. A walk the
    /// core itself decided the ending for has none to invent.
    #[test]
    fn a_walk_reported_by_ended_from_report_carries_no_failure_message() {
        assert_eq!(
            ended_from_report(&report(StopReason::Completed), 0).message,
            None
        );
    }

    #[test]
    fn done_and_total_include_phase_one_refusals_and_skipped_merges_both_kinds() {
        let ended = ended_from_report(&report(StopReason::Completed), 0);
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

        let ended = ended_from_report(&walked, 0);
        assert_eq!(ended.done, 0);
        assert_eq!(ended.total, 0);
        assert_eq!(ended.reason, EndReason::RootUnavailable);
    }
}

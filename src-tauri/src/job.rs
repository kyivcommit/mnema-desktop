//! The execution model for a job that runs far longer than a command may.
//!
//! Nothing here is a Tauri type. The shell's job is to carry these values across
//! the boundary, and a loop that can only be observed through a webview cannot
//! be observed at all.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

/// One progress report, as the webview receives it.
///
/// `seconds_left` is `Option` because "not known yet" is a real state and must
/// not render as `0`. `skipped` is separate from `done` because a run that
/// skipped half the folder and one that indexed it are not the same run, and a
/// single counter cannot tell the user which one they got.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    pub skipped: u64,
    pub seconds_left: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    /// How many units were finished when the stop was noticed.
    ///
    /// The count is here because "stopped after 412 of 5000" is what a window
    /// has to say, and because the last report sent is not that number — it is
    /// whatever the throttle last let through.
    Cancelled {
        done: u64,
    },
}

/// The ways a job stops.
///
/// `Failed` is here because the guarantee below says *however* the job ended,
/// and a panic is one of the ways. Reporting a panic as a cancellation would be
/// telling the user they did something they did not do.
///
/// The four variants after `Failed` are not the probe's: they are
/// `mnema_ingest::walk::StopReason`'s own `BrokenWorker`, `RulesNotApplied`,
/// `RootUnavailable` and `VolumeMissing`, carried across by name rather than
/// folded into `Failed`. A walk that stops for one of these has not
/// malfunctioned — it made a decision `walk_root`'s own doc comments argue for
/// at length — and reporting it as `Failed` would tell the user something
/// broke when instead a folder is unreadable, an exclusion rule did not take,
/// or a volume may have gone missing. `walk_job.rs` is the only writer of
/// these four; the probe never produces them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EndReason {
    Completed,
    Cancelled,
    Failed,
    BrokenWorker,
    RulesNotApplied,
    RootUnavailable,
    VolumeMissing,
}

/// One folder reconciliation declined to touch, and why — the webview's
/// counterpart to `mnema_ingest::walk::Frozen`, translated to a string
/// (`walk_job::frozen_reason_text`) rather than carrying `FrozenReason`
/// itself: `FrozenReason` has no `Serialize`, and giving it one would put a
/// UI-facing rename on a type whose only other reader today compares it by
/// value (`crates/mnema-ingest/tests/walk.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frozen {
    pub prefix: String,
    pub why: String,
}

/// The last message on the channel, always sent, however the job ended.
///
/// Without it a webview cannot tell a finished job from a cancelled one, or
/// either from a job whose reports simply stopped arriving — and a page that has
/// to guess ends up asserting what it hopes happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ended {
    pub reason: EndReason,
    pub done: u64,
    pub total: u64,
    /// Always `0` for the probe. For a walk, mirrors `Progress::skipped`'s own
    /// merge of `WalkProgress::skipped` and `WalkProgress::refused` — the
    /// final counts, not only the ones a throttled progress event happened to
    /// carry last. Without this, a page reading only the ending (the common
    /// case: `ui/main.js` overwrites the progress line with the ending's text)
    /// had no way to say how many files were skipped, only how many were not.
    pub skipped: u64,
    /// Always `true` for the probe, which has no subtree to fail to read.
    /// For a walk, mirrors `WalkReport::complete` — **the field a walk that
    /// stops `Completed` does not imply `true` for**, per that field's own
    /// doc comment. An unreadable subdirectory leaves `reason: Completed`
    /// (phase 2 finished everything phase 1 could hand it) with `complete:
    /// false` (phase 1 did not see the whole tree), which is the one
    /// combination that must reach the window: it is what tells "reconciled,
    /// and there was nothing to reconcile" apart from "reconciliation never
    /// ran, and whatever the user deleted under that unreadable folder is
    /// still searchable." `false` on [`Ended::failed`]'s two callers, for the
    /// same reason `frozen` is empty there: neither has a `WalkReport` to
    /// read `complete` from, and a job that stopped for an unexplained reason
    /// has not earned the claim that it saw everything.
    pub complete: bool,
    /// Always empty for the probe, which reconciles nothing. For a walk,
    /// mirrors `WalkReport::frozen` — see that field's own doc comment for why
    /// `removed == 0` alone cannot say whether anything was silently left
    /// untouched, which is exactly the question this answers.
    pub frozen: Vec<Frozen>,
}

impl Ended {
    /// `Completed` carries no count because completing means `done == total`;
    /// this is where that equivalence is written down rather than assumed by
    /// whoever draws the bar.
    pub fn of(outcome: Outcome, total: u64) -> Self {
        match outcome {
            Outcome::Completed => Self {
                reason: EndReason::Completed,
                done: total,
                total,
                skipped: 0,
                complete: true,
                frozen: Vec::new(),
            },
            Outcome::Cancelled { done } => Self {
                reason: EndReason::Cancelled,
                done,
                total,
                skipped: 0,
                complete: true,
                frozen: Vec::new(),
            },
        }
    }

    /// The job panicked, or — for a walk — stopped on an error `StopReason`
    /// has no variant for at all (`mnema_ingest::IngestError`, e.g. a broken
    /// pool). `done` is the last count the window was *shown*, not the loop's
    /// internal position: those differ by whatever the throttle dropped, and
    /// a number the user never saw is a worse answer than the one they did.
    /// `frozen` is empty and `complete` is `false` because both callers reach
    /// this with no `WalkReport` to read either from — the job stopped before
    /// producing one, and `false` is the side that costs nothing to be wrong
    /// about: it can only make a page more cautious about trusting what it
    /// saw, never less.
    pub fn failed(done: u64, total: u64) -> Self {
        Self {
            reason: EndReason::Failed,
            done,
            total,
            skipped: 0,
            complete: false,
            frozen: Vec::new(),
        }
    }
}

/// What the webview receives on a job's channel.
///
/// Tagged, in Tauri's documented `event`/`data` shape, because a progress report
/// and an ending are different things and a page that has to infer which it got
/// from the presence of a field will infer wrong.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum JobEvent {
    Progress(Progress),
    Ended(Ended),
}

/// The floor on how often progress is reported.
///
/// The cadence is fixed by the core, not chosen by the webview: at one report
/// per indexed document a folder of a hundred thousand files would put a
/// hundred thousand messages through the IPC to move a bar the user reads four
/// times a second anyway.
pub const REPORT_INTERVAL: Duration = Duration::from_millis(250);

/// The probe does forty units of nothing, a quarter-second each.
///
/// Ten seconds is long enough to watch the bar move, cancel it, and see it
/// stop — which is the whole of what this job is for.
pub const PROBE_UNITS: u64 = 40;
pub const PROBE_UNIT: Duration = Duration::from_millis(250);

/// Seconds still to go, from the rate measured so far.
///
/// `None` before anything is done: with nothing measured the honest answer is
/// "unknown", and a webview showing nothing is better than one showing a number
/// derived from a constant that will not survive the first real document.
pub fn seconds_left(done: u64, total: u64, elapsed: Duration) -> Option<u64> {
    if done == 0 {
        return None;
    }
    // Saturating: a job that reports more done than it planned — a folder that
    // grew while it was walked — has nothing left, not a negative amount.
    let remaining = total.saturating_sub(done);
    let estimate = elapsed.as_secs_f64() / done as f64 * remaining as f64;
    if !estimate.is_finite() {
        return None;
    }
    // Saturating, not wrapping: a float-to-integer cast in Rust clamps to the
    // range rather than wrapping, so an absurd estimate reads as absurd.
    Some(estimate.round() as u64)
}

/// Whether a progress report should go out now: nothing has been sent yet,
/// `interval` has elapsed since the last one, or this report is the one that
/// reaches `total` — always sent regardless of timing, because a bar that
/// stops one short of the end looks like a hang.
///
/// Shared by [`run_probe`]'s own loop and `walk_job::start_walk_job`'s
/// progress closure. `walk_root` (`mnema-ingest`) calls its progress
/// callback once per file with no throttle of its own — [`REPORT_INTERVAL`]'s
/// own doc comment names the shape that produces: "a folder of a hundred
/// thousand files would put a hundred thousand messages through the IPC" —
/// so whoever owns the channel on the other end of that callback has to
/// apply this rule, or flood it.
pub fn progress_is_due(
    last: Option<Instant>,
    now: Instant,
    interval: Duration,
    done: u64,
    total: u64,
) -> bool {
    done == total || last.is_none_or(|last| now.duration_since(last) >= interval)
}

/// Runs the probe job, reporting to `on_progress` and stopping when `cancel` is
/// raised.
///
/// Reports are throttled to `report_interval` with one exception: the report
/// that reaches `total` is always sent, because a bar that stops at 39 of 40
/// looks like a hang.
///
/// `on_progress` is called on this thread. It must not block for long — a slow
/// sink slows the job.
pub fn run_probe<F>(
    total: u64,
    unit: Duration,
    report_interval: Duration,
    cancel: &AtomicBool,
    on_progress: F,
) -> Outcome
where
    F: Fn(Progress),
{
    let started = Instant::now();
    let mut last_report: Option<Instant> = None;
    let mut done = 0u64;

    while done < total {
        // Checked before the unit, not after: cancelling means "stop", and a
        // check at the bottom of the loop always does one more unit first.
        if cancel.load(Ordering::SeqCst) {
            return Outcome::Cancelled { done };
        }
        if !unit.is_zero() {
            std::thread::sleep(unit);
        }
        done += 1;

        let now = Instant::now();
        if progress_is_due(last_report, now, report_interval, done, total) {
            last_report = Some(now);
            on_progress(Progress {
                done,
                total,
                skipped: 0,
                seconds_left: seconds_left(done, total, started.elapsed()),
            });
        }
    }

    Outcome::Completed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn a_completed_job_ends_at_the_total() {
        assert_eq!(
            Ended::of(Outcome::Completed, 40),
            Ended {
                reason: EndReason::Completed,
                done: 40,
                total: 40,
                skipped: 0,
                complete: true,
                frozen: Vec::new(),
            }
        );
    }

    #[test]
    fn a_cancelled_job_ends_where_it_stopped() {
        assert_eq!(
            Ended::of(Outcome::Cancelled { done: 7 }, 40),
            Ended {
                reason: EndReason::Cancelled,
                done: 7,
                total: 40,
                skipped: 0,
                complete: true,
                frozen: Vec::new(),
            }
        );
    }

    #[test]
    fn a_failure_is_not_reported_as_a_cancellation() {
        // The distinction the `reason` field exists for. A user who is told
        // their job was cancelled looks for what they did wrong.
        let failed = Ended::failed(7, 40);
        assert_eq!(failed.reason, EndReason::Failed);
        assert_eq!(failed.done, 7);
        assert_ne!(
            failed.reason,
            Ended::of(Outcome::Cancelled { done: 7 }, 40).reason
        );
    }

    #[test]
    fn a_report_is_due_when_none_has_been_sent_yet() {
        assert!(progress_is_due(
            None,
            Instant::now(),
            Duration::from_millis(250),
            1,
            100
        ));
    }

    #[test]
    fn a_report_is_not_due_before_the_interval_elapses() {
        let last = Instant::now();
        let now = last + Duration::from_millis(100);
        assert!(!progress_is_due(
            Some(last),
            now,
            Duration::from_millis(250),
            5,
            100
        ));
    }

    #[test]
    fn a_report_is_due_once_the_interval_elapses() {
        let last = Instant::now();
        let now = last + Duration::from_millis(300);
        assert!(progress_is_due(
            Some(last),
            now,
            Duration::from_millis(250),
            5,
            100
        ));
    }

    #[test]
    fn the_report_that_reaches_total_is_always_due() {
        // One millisecond after the last one — nowhere near the interval —
        // and still due, because `done == total` overrides the timer. Pins
        // the exact exception `walk_job.rs`'s progress closure leans on to
        // avoid a bar that stops one file short of the end.
        let last = Instant::now();
        let now = last + Duration::from_millis(1);
        assert!(progress_is_due(
            Some(last),
            now,
            Duration::from_millis(250),
            100,
            100
        ));
    }

    #[test]
    fn nothing_measured_yet_means_no_estimate() {
        assert_eq!(seconds_left(0, 10, Duration::from_secs(5)), None);
    }

    #[test]
    fn the_estimate_comes_from_the_measured_rate() {
        // Two seconds bought one unit of five, so four units are two seconds
        // each: eight.
        assert_eq!(seconds_left(1, 5, Duration::from_secs(2)), Some(8));
    }

    #[test]
    fn a_finished_job_has_nothing_left() {
        assert_eq!(seconds_left(10, 10, Duration::from_secs(90)), Some(0));
        assert_eq!(seconds_left(11, 10, Duration::from_secs(90)), Some(0));
    }

    #[test]
    fn every_unit_is_reported_when_the_interval_is_zero() {
        let seen = Mutex::new(Vec::new());
        let cancel = AtomicBool::new(false);
        let outcome = run_probe(5, Duration::ZERO, Duration::ZERO, &cancel, |p| {
            seen.lock().unwrap().push(p)
        });

        assert_eq!(outcome, Outcome::Completed);
        let seen = seen.into_inner().unwrap();
        assert_eq!(
            seen.iter().map(|p| p.done).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
        assert!(seen.iter().all(|p| p.total == 5));
    }

    #[test]
    fn progress_is_throttled_to_the_report_interval() {
        // A hundred units with a report interval no run can outlast: the first
        // report is due because nothing has been reported yet, and the last is
        // forced because it completes the job. Everything between is dropped.
        let seen = Mutex::new(Vec::new());
        let cancel = AtomicBool::new(false);
        run_probe(
            100,
            Duration::ZERO,
            Duration::from_secs(3600),
            &cancel,
            |p| seen.lock().unwrap().push(p),
        );

        let seen = seen.into_inner().unwrap();
        assert_eq!(
            seen.iter().map(|p| p.done).collect::<Vec<_>>(),
            vec![1, 100],
            "an unthrottled loop would have reported all hundred units"
        );
    }

    #[test]
    fn a_job_cancelled_before_it_starts_does_no_work() {
        let seen = Mutex::new(Vec::new());
        let cancel = AtomicBool::new(true);
        let outcome = run_probe(5, Duration::ZERO, Duration::ZERO, &cancel, |p| {
            seen.lock().unwrap().push(p)
        });

        // The count, not just the absence of reports. Silence is also what a
        // loop that did a unit of work and then noticed the flag would produce,
        // and those are not the same thing.
        assert_eq!(outcome, Outcome::Cancelled { done: 0 });
        assert!(seen.into_inner().unwrap().is_empty());
    }

    #[test]
    fn cancelling_part_way_through_stops_the_loop() {
        // The sink raises the flag, so the stop is observed at a known unit
        // rather than at whatever moment a sleeping thread happened to reach.
        let seen = Mutex::new(Vec::new());
        let cancel = AtomicBool::new(false);
        let outcome = run_probe(1_000_000, Duration::ZERO, Duration::ZERO, &cancel, |p| {
            seen.lock().unwrap().push(p.done);
            if p.done == 3 {
                cancel.store(true, Ordering::SeqCst);
            }
        });

        assert_eq!(outcome, Outcome::Cancelled { done: 3 });
        assert_eq!(seen.into_inner().unwrap(), vec![1, 2, 3]);
    }
}

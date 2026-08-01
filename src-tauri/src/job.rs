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
/// counterpart to `mnema_ingest::walk::Frozen`. `reason` carries
/// [`FrozenReason`], not `mnema_ingest::walk::FrozenReason` itself — see
/// that type's own doc comment for why a second enum exists — so
/// `walk_job::frozen_reason` is where the core's vocabulary is translated
/// into this one's, and `crates/mnema-ingest/tests/walk.rs`'s comparisons
/// against the core enum are untouched by anything here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frozen {
    pub prefix: String,
    pub reason: FrozenReason,
}

/// The webview's counterpart to `mnema_ingest::walk::FrozenReason` — a
/// second enum, not a `Serialize` impl on the core one, and for the same
/// reason the sentences used to live in `walk_job::frozen_reason_text`
/// rather than in `mnema_ingest`: the core crate decides *that* a folder is
/// frozen, not the words a person reads about it. Before this type existed,
/// the words crossed instead of the decision — `walk_job.rs` built the
/// sentence and `Frozen.why` carried it as free English, which left a window
/// with nothing to group or translate by except string-matching that
/// sentence. A discriminant is a value every reader can switch on; a
/// sentence is a value only one reader — the one who already knows what
/// English to expect — can use for anything but printing verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FrozenReason {
    SymlinkedSubtree,
    EmptyDirectory,
    UnreadableDirectory,
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
    /// Always `0` for the probe, which indexes nothing real. For a walk,
    /// mirrors `WalkReport::indexed` — kept apart from `unchanged` because a
    /// run that wrote a hundred new documents and a run that found a hundred
    /// documents already there are not the same run, and `done` (which merges
    /// both of these with `skipped` and `refused`, for the reason given on
    /// `walk_job::ended_from_report`'s own doc comment) cannot answer which
    /// one happened.
    pub indexed: u64,
    /// Always `0` for the probe. For a walk, mirrors `WalkReport::unchanged` —
    /// see [`Ended::indexed`]'s own doc comment for why this does not fold
    /// into it.
    pub unchanged: u64,
    /// Always `0` for the probe, which reconciles nothing. For a walk,
    /// mirrors `WalkReport::removed` — how many `path` rows phase 3 actually
    /// deleted, as opposed to `frozen`, which is every prefix it refused to.
    /// `removed == 0` alone cannot say whether phase 3 ran and found nothing
    /// gone, or never ran at all — [`Ended::complete`] and `reason` are what
    /// answer that, the same way they already do for `frozen`. Before this
    /// field existed, `WalkReport::removed` was computed and then dropped at
    /// this exact seam: a person who moved four hundred files out of a
    /// watched folder read "12 unchanged" and nothing else, with no way to
    /// tell four hundred deletions from a walk that touched nothing.
    pub removed: u64,
    /// Set only when `reason` is `Failed` — the text a broken pool, a missing
    /// worker binary, or a panic each leave behind. `None` for every other
    /// `reason`, which already has a sentence a window can write without one:
    /// `Completed`, `Cancelled`, and the four `StopReason` variants
    /// `walk_job.rs` carries across all say what happened on their own.
    ///
    /// Before this field existed, all three of `Failed`'s causes reached the
    /// window as the single word `"failed"` — indistinguishable from each
    /// other, and indistinguishable in particular from the shape that is the
    /// **known, current state of any packaged build**: no `externalBin` is
    /// wired yet (`paths::worker_path`'s own doc comment says so), so a
    /// shipped application has no worker binary at the path it looks for one,
    /// and every walk over it fails this way. A window that could only say
    /// "failed after N of M" had nothing to tell a person about the one
    /// failure they were most likely to see.
    pub message: Option<String>,
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
                indexed: 0,
                unchanged: 0,
                removed: 0,
                message: None,
            },
            Outcome::Cancelled { done } => Self {
                reason: EndReason::Cancelled,
                done,
                total,
                skipped: 0,
                complete: true,
                frozen: Vec::new(),
                indexed: 0,
                unchanged: 0,
                removed: 0,
                message: None,
            },
        }
    }

    /// The job panicked, or — for a walk — stopped on an error `StopReason`
    /// has no variant for at all (`mnema_ingest::IngestError`, e.g. a broken
    /// pool). `done` is the last count the window was *shown*, not the loop's
    /// internal position: those differ by whatever the throttle dropped, and
    /// a number the user never saw is a worse answer than the one they did.
    /// `frozen`, `indexed`, `unchanged` and `removed` are empty or zero and
    /// `complete` is `false` because both callers reach this with no
    /// `WalkReport` to read any of them from — the job stopped before
    /// producing one, and
    /// `false` is the side that costs nothing to be wrong about: it can only
    /// make a page more cautious about trusting what it saw, never less.
    ///
    /// `message` is not optional here the way it is on [`Ended`] itself:
    /// every caller of this constructor is on the `Failed` path, so every
    /// caller has *something* to say — `mnema_ingest::IngestError`'s own
    /// `Display`, or [`panic_message`] — and a blank field on the one
    /// `reason` this crosses for is a worse answer than making the caller
    /// supply it.
    pub fn failed(done: u64, total: u64, message: impl Into<String>) -> Self {
        Self {
            reason: EndReason::Failed,
            done,
            total,
            skipped: 0,
            complete: false,
            frozen: Vec::new(),
            indexed: 0,
            unchanged: 0,
            removed: 0,
            message: Some(message.into()),
        }
    }
}

/// Turns a caught panic's payload into the text [`Ended::failed`] carries to
/// the window.
///
/// `catch_unwind` hands back `Box<dyn Any + Send>`. A bare `panic!("...")`
/// payload downcasts to `&str`; one built with `format!`-style arguments —
/// which is what `.unwrap()`, `.expect(...)` and every assertion macro
/// produce — downcasts to `String`. Between the two is every panic this
/// codebase or an ordinary dependency raises. Anything else — `panic_any`
/// with a caller-defined payload type — has no text to extract, so this says
/// that plainly rather than fabricating a sentence a panic never wrote.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "the job panicked with no text message".to_string()
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
                indexed: 0,
                unchanged: 0,
                removed: 0,
                message: None,
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
                indexed: 0,
                unchanged: 0,
                removed: 0,
                message: None,
            }
        );
    }

    #[test]
    fn a_failure_is_not_reported_as_a_cancellation() {
        // The distinction the `reason` field exists for. A user who is told
        // their job was cancelled looks for what they did wrong.
        let failed = Ended::failed(7, 40, "the extraction pool cannot continue");
        assert_eq!(failed.reason, EndReason::Failed);
        assert_eq!(failed.done, 7);
        assert_ne!(
            failed.reason,
            Ended::of(Outcome::Cancelled { done: 7 }, 40).reason
        );
    }

    /// Gap 1 from the task-12 review: a missing worker binary, a broken pool
    /// and a panic used to arrive as the same bare word, `"failed"`. This is
    /// what carries the difference — pinned here on the constructor itself,
    /// since `walk_job.rs`'s two call sites only ever pass through what they
    /// are given.
    #[test]
    fn a_failed_job_carries_the_message_it_is_given() {
        let failed = Ended::failed(3, 10, "could not start the extraction worker");
        assert_eq!(
            failed.message.as_deref(),
            Some("could not start the extraction worker")
        );
    }

    /// The one field `Ended::failed` does not leave to its caller: a job that
    /// stopped for an unexplained reason has not earned the claim that it saw
    /// everything, and `complete: true` is the value that reads as "trust
    /// what this run reconciled."
    #[test]
    fn a_failed_job_never_claims_completeness() {
        assert!(!Ended::failed(0, 0, "boom").complete);
    }

    #[test]
    fn a_bare_panic_string_is_read_back_verbatim() {
        // `panic!("literal")` boxes a `&'static str`, never a `String` — this
        // is the shape that would fail to downcast against the wrong branch.
        let payload: Box<dyn std::any::Any + Send> = Box::new("deliberate panic: literal");
        assert_eq!(panic_message(&*payload), "deliberate panic: literal");
    }

    #[test]
    fn a_formatted_panic_message_is_read_back_verbatim() {
        // `.expect(...)`, `.unwrap()` and `assert_eq!` all box a `String`, not
        // a `&str` — the shape `panic!("{}", "formatted")`, `format!(...)`
        // arguments, or an interpolated `panic!` also produce.
        let payload: Box<dyn std::any::Any + Send> =
            Box::new(format!("deliberate panic: {}", "formatted"));
        assert_eq!(panic_message(&*payload), "deliberate panic: formatted");
    }

    #[test]
    fn a_non_string_panic_payload_gets_an_honest_fallback() {
        // `std::panic::panic_any(42)` is the shape neither branch matches —
        // this is what a window sees instead of an empty string or a debug
        // dump of an opaque `Any`.
        let payload: Box<dyn std::any::Any + Send> = Box::new(42i32);
        assert_eq!(
            panic_message(&*payload),
            "the job panicked with no text message"
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

    /// The canonical list of `EndReason` discriminants, in the exact
    /// camelCase spelling `#[serde(rename_all = "camelCase")]` produces —
    /// pinned identically, by hand, in `ui/render.test.js`'s own `END_
    /// REASONS`. Neither side can see the other's source, so nothing forces
    /// them to agree; what this test forces is narrower and still real: the
    /// `match` below has no wildcard arm, so a variant added to `EndReason`
    /// without a matching line here fails to **compile**, which is what
    /// makes a maintainer look at this list at all rather than letting it go
    /// silently stale. Whoever touches it is the one who has to remember the
    /// JS copy — this test cannot do that part, but it makes the Rust half
    /// of "cannot drift apart silently" true rather than aspirational.
    ///
    /// `serde_json::to_value` alongside the hand-written match, not instead
    /// of it: the match is a second, independently typed opinion about the
    /// spelling, so a `#[serde(rename = "...")]` typo on one variant that
    /// happened to also be typo'd the same way here would still be caught —
    /// the two have to agree with each other, not just with themselves.
    #[test]
    fn every_end_reason_has_its_camel_case_spelling_pinned() {
        let discriminant = |reason: EndReason| -> &'static str {
            match reason {
                EndReason::Completed => "completed",
                EndReason::Cancelled => "cancelled",
                EndReason::Failed => "failed",
                EndReason::BrokenWorker => "brokenWorker",
                EndReason::RulesNotApplied => "rulesNotApplied",
                EndReason::RootUnavailable => "rootUnavailable",
                EndReason::VolumeMissing => "volumeMissing",
            }
        };
        for reason in [
            EndReason::Completed,
            EndReason::Cancelled,
            EndReason::Failed,
            EndReason::BrokenWorker,
            EndReason::RulesNotApplied,
            EndReason::RootUnavailable,
            EndReason::VolumeMissing,
        ] {
            assert_eq!(
                serde_json::to_value(reason).unwrap().as_str().unwrap(),
                discriminant(reason),
                "{reason:?} serialized differently than this test's own spelling of it"
            );
        }
    }

    /// Same pairing as the test above, for `FrozenReason` — mirrored in
    /// `ui/render.test.js`'s `FROZEN_REASONS`.
    #[test]
    fn every_frozen_reason_has_its_camel_case_spelling_pinned() {
        let discriminant = |reason: FrozenReason| -> &'static str {
            match reason {
                FrozenReason::SymlinkedSubtree => "symlinkedSubtree",
                FrozenReason::EmptyDirectory => "emptyDirectory",
                FrozenReason::UnreadableDirectory => "unreadableDirectory",
            }
        };
        for reason in [
            FrozenReason::SymlinkedSubtree,
            FrozenReason::EmptyDirectory,
            FrozenReason::UnreadableDirectory,
        ] {
            assert_eq!(
                serde_json::to_value(reason).unwrap().as_str().unwrap(),
                discriminant(reason),
                "{reason:?} serialized differently than this test's own spelling of it"
            );
        }
    }
}

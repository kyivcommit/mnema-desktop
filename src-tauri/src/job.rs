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
    /// How many units this run has given up on for good. Always `0` for the
    /// probe and for a walk; written only by `embed_job.rs`.
    ///
    /// **Not `mnema_ingest::WalkProgress::refused`**, which is a *file* phase 1
    /// declined to open and which `walk_job.rs` merges into [`Progress::
    /// skipped`]. This is a *chunk the provider refused*, and the difference is
    /// the whole reason it is a field of its own rather than another thing
    /// added into `skipped`: a skipped unit is one nobody has got to, and these
    /// are the ones nobody ever will — the chunk leaves the embedding queue and
    /// is not offered again until its text changes, so vector search stops
    /// answering for it while the document still shows it and lexical search
    /// still finds it. It is the number that makes that rule defensible instead
    /// of silent.
    ///
    /// **It counts this run, not the space.** The cumulative number for the
    /// active space is `crate::models::IndexRead::failed_chunks`, read from the
    /// database for the settings screen; a second run starts this one again at
    /// zero while that one goes on growing. Two numbers about two scopes, and
    /// whichever surface shows them owes each its own words rather than letting
    /// either be read as the other. Neither is on screen today — `ui/src/
    /// settings/Settings.svelte` is a stub and the settings surfaces are PR 7's.
    pub refused: u64,
    /// How many files this run has so far found the index locked by another
    /// writer on, after every busy retry was refused
    /// (`mnema_ingest::WalkProgress::contended`). Always `0` for the probe and
    /// for an embedding pass; written only by `walk_job.rs`.
    ///
    /// **Not [`Progress::refused`]**, which is a chunk the provider refused and
    /// will not be offered again, and **not `mnema_ingest::WalkProgress::
    /// refused`**, which is a file phase 1 declined to open — the two are told
    /// apart above. This is neither a refusal nor a property of the file at
    /// all: it is evidence about whoever else holds the write lock, and the
    /// same file walked a second later, with the lock free, is indexed like any
    /// other (`mnema-ingest`'s `a_file_left_unwritten_by_a_busy_index_is_
    /// indexed_by_the_next_walk`).
    ///
    /// 🔴 **`contended <= skipped` in every event after the file is
    /// journalled**, because the file counted here is journalled as a skip
    /// immediately afterwards and counted there too. A surface showing both
    /// must therefore explain part of `skipped` with this number and must never
    /// add the two: that would count one file twice. The one event in which the
    /// inequality does not yet hold is the contended file's own — the callback
    /// fires before the skip write — and it is not an exception to the rule, it
    /// is the moment before the rule applies.
    ///
    /// **It says nothing about the file having been recorded.** The skip write
    /// meets the same lock and can fail too, ending the walk and leaving the
    /// file in neither the index nor the journal (`mnema-ingest`'s
    /// `a_skip_write_that_meets_the_same_lock_leaves_the_file_in_neither_
    /// place`), so whatever a surface says about this number may promise the
    /// next scan and nothing more.
    pub contended: u64,
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
    /// carry last. Without this, a page reading only the ending — the common
    /// case, since an ending overwrites whatever the progress line last said —
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
    /// How many units the run gave up on for good — always `0` for the probe
    /// and for a walk, and written only by `embed_job.rs`. See
    /// [`Progress::refused`]'s own doc comment for what it is, what it is not
    /// (`WalkProgress::refused`), and why it is a run-scoped number that the
    /// settings screen's cumulative `failed_chunks` must not be read as.
    ///
    /// On [`Ended::failed`]'s two callers it is `0` for the same reason
    /// `indexed` and `removed` are: neither has anything to read it from. The
    /// embedding job's own failure path does have something — the rows are
    /// already written — and fills it in with `..Ended::failed(..)`, which is
    /// the one place this field is set on a `Failed` ending.
    pub refused: u64,
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
    /// other. A missing worker binary is no longer the expected shape of a
    /// packaged build — `bundle.externalBin` stages one now, and
    /// `scripts/verify-bundle.sh` is what keeps it there — but it is still
    /// one of the three causes this field exists to tell apart: a broken
    /// pool, a panic, or a future bundle that fails to carry the worker in
    /// all still reach a walk the same way, and a window that could only say
    /// "failed after N of M" had nothing to tell a person about which one
    /// they hit.
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
                refused: 0,
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
                refused: 0,
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
    /// `frozen`, `indexed`, `unchanged`, `removed` and `refused` are empty or
    /// zero and `complete` is `false` because both callers reach this with no
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
            refused: 0,
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
///
/// ⚠️ **Does not know about `refused`, unlike its neighbour.**
/// [`progress_is_due`], below and fed from the same report, was repaired to
/// treat a unit as resolved at `done + refused >= total`. This
/// function was not: `remaining` below is `total - done` alone, so on the run
/// that repair exists for — one ending in refusals — the forced final report
/// carries an estimate that still counts the refused chunks as work to come.
/// Left rather than fixed, because it self-corrects within that one message:
/// the ending text replaces this line the moment the window renders it. See
/// `embed_job::progress_from`'s doc for the separate, existing reason `done`
/// alone (not `done + refused`) is also the right choice for the *rate* half
/// of this estimate — that argument still holds; it is only the `remaining`
/// half this note is about.
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
/// **resolves the last unit** — always sent regardless of timing, because a bar
/// that stops one short of the end looks like a hang.
///
/// Shared by [`run_probe`]'s own loop, `walk_job::start_walk_job`'s progress
/// closure and `embed_job::start_embed_job`'s. `walk_root` (`mnema-ingest`)
/// calls its progress callback once per file (twice for a file whose busy
/// retries were all refused, and once before the loop) with no throttle of
/// its own —
/// [`REPORT_INTERVAL`]'s own doc comment names the shape that produces: "a
/// folder of a hundred thousand files would put a hundred thousand messages
/// through the IPC" — so whoever owns the channel on the other end of that
/// callback has to apply this rule, or flood it.
///
/// ⚠️ **`refused` is in the condition, and that is a repair rather than a
/// generalisation.** The arm used to read `done == total`, which silently never
/// fires for a run that ends with refusals — `mnema_embed::EmbedProgress`'s own
/// doc comment says so in as many words: "a run with `failed > 0` never reaches
/// `done == total` at all". So the unthrottled last report was missing on
/// exactly the runs whose numbers matter most, and the promise above held only
/// for the two callers that cannot refuse anything. A resolved unit is one that
/// was done *or* given up on, which is what the sum says; both existing callers
/// pass `0` and keep the behaviour they had, and
/// `progress_is_throttled_to_the_report_interval` is what still pins it.
///
/// `>=` rather than `==` for the reason the old `==` failed: a condition that
/// has to be hit exactly stops firing altogether the moment something makes the
/// left side overshoot, and it does it in silence. Nothing can overshoot today
/// (`done + failed <= total` is `EmbedProgress`'s own invariant, `total` being
/// the queue measured once at the start of a run that is the only writer), so
/// this costs nothing and cannot fail that way later.
pub fn progress_is_due(
    last: Option<Instant>,
    now: Instant,
    interval: Duration,
    done: u64,
    refused: u64,
    total: u64,
) -> bool {
    done + refused >= total || last.is_none_or(|last| now.duration_since(last) >= interval)
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
        // `0` refused: the probe does forty units of nothing and has nothing to
        // give up on, so the arm above reduces to the `done == total` it was
        // before `refused` existed.
        if progress_is_due(last_report, now, report_interval, done, 0, total) {
            last_report = Some(now);
            on_progress(Progress {
                done,
                total,
                skipped: 0,
                refused: 0,
                // `0` contended for the same reason: the probe writes to no
                // index, so there is no lock for anyone to be holding against
                // it.
                contended: 0,
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
                refused: 0,
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
                refused: 0,
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
            0,
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
            0,
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
            0,
            100
        ));
    }

    #[test]
    fn the_report_that_reaches_total_is_always_due() {
        // One millisecond after the last one — nowhere near the interval —
        // and still due, because reaching `total` overrides the timer. Pins
        // the exact exception `walk_job.rs`'s progress closure leans on to
        // avoid a bar that stops one file short of the end.
        let last = Instant::now();
        let now = last + Duration::from_millis(1);
        assert!(progress_is_due(
            Some(last),
            now,
            Duration::from_millis(250),
            100,
            0,
            100
        ));
    }

    /// The arm as `done == total` never fires for a run that ends with
    /// refusals, which is exactly the run whose numbers a person needs: an
    /// embedding pass that gave up on three chunks stops at 97 done of 100 and
    /// the last report is left to the throttle, so the bar can sit on whatever
    /// number the timer last let through while the true one is never sent.
    /// `mnema_embed::EmbedProgress`'s own doc comment states the arithmetic
    /// this test is the other end of.
    #[test]
    fn the_report_that_resolves_the_last_unit_is_due_even_when_some_were_refused() {
        let last = Instant::now();
        let now = last + Duration::from_millis(1);
        assert!(
            progress_is_due(Some(last), now, Duration::from_millis(250), 97, 3, 100),
            "97 embedded and 3 refused resolves all 100, and the report saying so was withheld"
        );
    }

    /// The other direction, without which the test above is satisfied by a
    /// function that answers `true` to everything — and so is the whole
    /// throttle. One unit still unresolved, a millisecond after the last
    /// report: not due.
    #[test]
    fn a_report_that_leaves_a_unit_unresolved_is_still_throttled() {
        let last = Instant::now();
        let now = last + Duration::from_millis(1);
        assert!(
            !progress_is_due(Some(last), now, Duration::from_millis(250), 96, 3, 100),
            "96 embedded and 3 refused leaves one unit unaccounted for, so nothing forces \
             this report out ahead of the interval"
        );
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
    /// camelCase spelling `#[serde(rename_all = "camelCase")]` produces.
    /// **No window copy exists to pin it against today**: `ui/src/lib/ipc.ts`
    /// mirrors the ask, search, tree and source wire, plus a deliberately narrow
    /// read of `model_settings` — and none of the job events, so this list has
    /// one side only until PR 7 gives the indexing surface its own. What this
    /// test forces is narrower and still real: the `match` below has no
    /// wildcard arm, so a variant added to `EndReason` without a matching line
    /// here fails to **compile**, which is what
    /// makes a maintainer look at this list at all rather than letting it go
    /// silently stale. Whoever touches it is the one who has to carry it
    /// across to whatever window comes to read it — this test cannot do that
    /// part, but it makes the Rust half of "cannot drift apart silently" true
    /// rather than aspirational.
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

    /// Same pairing as the test above, for `FrozenReason`, and with the same
    /// note: there is no window copy to mirror it against yet (PR 7).
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

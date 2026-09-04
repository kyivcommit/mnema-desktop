//! The embedding pass, run as a job — `start_walk_job`'s shape, over
//! `mnema_embed::run` instead of `walk_root`.
//!
//! Kept apart from `bridge.rs` for the reason `walk_job.rs` is: this one
//! command is the only thing in the shell that reaches `mnema-embed`, and
//! translating what that crate reports into what the window reads is enough
//! logic on its own to want a file that is *only* that translation.
//!
//! **The translation is the point of this file.** `mnema_embed::EmbedProgress`
//! and [`crate::job::Progress`] are two types with deliberately different
//! names, and `EmbedProgress`'s own doc comment says why: the name is taken
//! twice over, and two types called the same thing would make the step between
//! them invisible at exactly the place it is easiest to forget. That step is
//! [`progress_from`], and the ending's is [`ended_from_tally`]; both are
//! ordinary functions with unit tests below rather than closures inside the
//! thread, so that what crosses the seam is something a test can hold.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use mnema_embed::{EmbedProgress, EmbedTally};
use tauri::State;
use tauri::ipc::Channel;

use crate::error::Error;
use crate::job::{self, EndReason, Ended, JobEvent, Progress};
use crate::state::AppState;

/// How many chunks go to the provider in one request.
///
/// ⚠️ **Nobody has measured this, and the spec says so** (§8, "the batch size —
/// not measured; the default goes into the plan as an assumption, the live run
/// names the number"). It is an assumption with an argument behind it, not a
/// measurement, and the acceptance run is what replaces it:
///
/// - **Above one, and that is load-bearing rather than a preference.**
///   `mnema_embed::one_at_a_time`'s corroboration rule attributes a refusal to a
///   text only once some *other* text in the same split has embedded, and at a
///   batch of one there is no split to corroborate anything — that crate's own
///   doc comment names the gap and accepts it precisely because the production
///   batch is wider.
/// - **Small enough that a bad batch is cheap.** A refusal that can be about the
///   texts is re-sent one text at a time, so the worst case is this many extra
///   round trips for one bad chunk.
/// - **Small enough that Stop is prompt.** `cancel` is asked between batches
///   (and, inside a split, between single calls), so a person waits out at most
///   one request.
///
/// What it is not chosen from: any published limit, any measured throughput, and
/// any token budget — the provider states no batch limit this build has read,
/// and the one number anybody measured about long inputs is D25's observation
/// that an over-long input to `bge-m3` returns `200` with a third of the text
/// silently dropped, which is about one text and not about how many.
const BATCH: usize = 32;

/// `(async)` for the reason given on [`crate::bridge::open_index`] and repeated
/// by [`crate::walk_job::start_walk_job`]: this command reads the credential
/// store before it spawns anything, and a window-issued command that can block —
/// on macOS the store can put an authorisation dialog on screen — must not be
/// the one left running inline on the main thread.
///
/// **The key is read before [`AppState::claim_job`], and that ordering is a
/// decision rather than a line that happened to come first.** It is
/// `start_walk_job`'s own rule — every fallible step before the claim, so that a
/// call which was always going to fail never has `job_status` reporting a job
/// that is running — and it bites harder here, because the fallible step is a
/// dialog somebody may leave unanswered for a minute. Claiming first would
/// disable Start and refuse a walk for that whole minute, for a call that then
/// fails with `NoKey`.
///
/// The absence of a key is [`Error::NoKey`] and never a panic: `models::key` is
/// where that split is made, and that variant's own doc comment is about what it
/// tells the person to do next as against [`Error::Secrets`], which is a store
/// that would not answer at all.
///
/// **What is deliberately *not* checked here:** whether a model has been chosen.
/// `mnema_embed::run` reads `meta.active_space` itself and refuses with
/// `NoActiveSpace` when there is none, and asking the same question here first
/// would be a second measurement of one fact — the shape `set_embedding_model`'s
/// own doc comment argues against for the credential store. The refusal reaches
/// the window as an ending carrying that crate's sentence, which is the same
/// route every other failure of the pass takes.
#[tauri::command(async)]
pub fn start_embed_job(
    state: State<'_, AppState>,
    on_progress: Channel<JobEvent>,
) -> Result<(), Error> {
    let key = crate::models::key(&state)?;
    let base = state.provider_base().to_string();

    let slot = state.claim_job()?;

    // The job's own connection, for the reason `AppState::open_job_index`'s own
    // doc comment gives: this is a sequence of writes that can run for hours,
    // and the window has to keep answering searches while it does.
    let job_db = state.open_job_index()?;

    std::thread::spawn(move || {
        // The last two counts the window was actually *shown*, in the sense
        // `Ended::failed` promises: written only after a send that succeeded,
        // and read only on the paths where the pass produced no tally to read
        // instead.
        let reported_done = AtomicU64::new(0);
        let reported_refused = AtomicU64::new(0);
        // The size of the queue this run started against. Written on every
        // report, sent or not, because unlike the two above it is not a claim
        // about what the window saw: `mnema_embed` measures it once, before it
        // takes anything out of the queue, and carries the same value on every
        // report — so this is a fact about the run, and the honest denominator
        // for an ending even when every send failed.
        //
        // ⚠️ **`0` when the pass reported nothing at all, and that stands for
        // two different states.** One is an empty queue, where the total really
        // is zero: the loop breaks before the first report and `0 of 0` is the
        // truth. The other is a Stop landing in the first instant —
        // `mnema_embed::run` asks `cancel()` *before* its first batch, so a run
        // stopped there has measured a queue and reported none of it, and `0`
        // here is "not known" wearing a number's clothes.
        //
        // Not repaired by reading the queue again from this side: that would be
        // a second measurement of a number the pass already has, taken after it
        // stopped, and it would disagree with the one the run actually used.
        // What is repaired is the sentence — `reason` tells the two apart, so
        // the surface that shows this can say "stopped before anything was
        // embedded" for the cancelled one rather than claiming a total nobody
        // measured. That surface is PR 7's; the fact it needs is here already.
        let queue_total = AtomicU64::new(0);
        let started = Instant::now();
        // Throttling state, a plain local rather than an atomic for the reason
        // `walk_job.rs` gives: `mnema_embed::run` calls this closure
        // synchronously, on this one thread.
        let mut last_report: Option<Instant> = None;

        // `AssertUnwindSafe` for `walk_job.rs`'s reason: everything this closure
        // reaches — the channel, the atomics, the database connection — is used
        // only on this thread, only for as long as this call runs, and dropped
        // when the thread ends however it ends. Nothing downstream ever sees any
        // of it again. Unlike the walk there is no FFI here, so a panic would be
        // this code's own; it is caught all the same, because the window has to
        // be told the job is over or Start stays disabled for the life of the
        // page.
        let caught = catch_unwind(AssertUnwindSafe(|| {
            mnema_embed::run(
                &job_db,
                &base,
                &key,
                BATCH,
                &|| slot.cancel_flag().load(Ordering::SeqCst),
                &mut |progress| {
                    queue_total.store(progress.total, Ordering::Relaxed);

                    // `mnema_embed::run` calls this once per batch, unthrottled,
                    // and once per single call inside a split — its own doc
                    // comment says the throttle belongs to whoever owns the
                    // channel, which is here. `progress_is_due` is the same rule
                    // the probe and the walk use, and `refused` is in it because
                    // a run that ends with refusals never reaches `done ==
                    // total` at all; without that the last report — the one
                    // carrying the numbers that matter most — would be left to
                    // the timer.
                    let now = Instant::now();
                    if !job::progress_is_due(
                        last_report,
                        now,
                        job::REPORT_INTERVAL,
                        progress.done,
                        progress.failed,
                        progress.total,
                    ) {
                        return;
                    }
                    last_report = Some(now);

                    // A failed send means the webview is gone — reloaded, or
                    // closed while the job runs. The job deliberately continues:
                    // the work is the point, and it is paid for.
                    if on_progress
                        .send(JobEvent::Progress(progress_from(
                            progress,
                            started.elapsed(),
                        )))
                        .is_ok()
                    {
                        reported_done.store(progress.done, Ordering::Relaxed);
                        reported_refused.store(progress.failed, Ordering::Relaxed);
                    }
                },
            )
        }));

        // Read once, after the pass has returned, and it is the only thing that
        // can tell a run that emptied the queue from one that was stopped:
        // `mnema_embed::run` answers `Ok(tally)` to both and says nothing about
        // which. See `ended_from_tally` for what the narrow race costs and why
        // it is broken this way round.
        //
        // This is also the symmetrical half of `walk_job.rs`'s post-walk read,
        // and it was already here — but it carries less weight, and the
        // difference is worth naming rather than leaving to be rediscovered.
        // **This ending has no successor.** Nothing chains a further pass off
        // an embedding pass: `jobs.ts` chains only off a `walk` (`if (pass ===
        // 'walk' && chainsEmbedPass(...))`), so a Stop lost here would cost a
        // mislabelled report and nothing else, where a Stop lost at the end of
        // a WALK sends the person's text to a provider they had just told the
        // application not to send it to. If a pass is ever chained after this
        // one, that asymmetry ends and this read becomes load-bearing.
        let cancelled = slot.cancel_flag().load(Ordering::SeqCst);
        let total = queue_total.load(Ordering::Relaxed);
        let ending = match caught {
            Ok(Ok(tally)) => ended_from_tally(tally, total, cancelled),
            // Both failing paths report the last counts the window was shown
            // rather than anything read back from the database, which is
            // `Ended::failed`'s own promise. It costs nothing in accuracy here:
            // `mnema_embed::run` reports before it propagates an error — the
            // vectors of the failing batch are already written when it does —
            // so the last number sent is already true of the index.
            Ok(Err(refusal)) => failed_ending(
                reported_done.load(Ordering::Relaxed),
                reported_refused.load(Ordering::Relaxed),
                total,
                refusal.to_string(),
            ),
            Err(panic) => failed_ending(
                reported_done.load(Ordering::Relaxed),
                reported_refused.load(Ordering::Relaxed),
                total,
                job::panic_message(&*panic),
            ),
        };
        // Dropped **before** the send, for the reason measured on
        // `walk_job.rs`: `JobSlot::drop` is what clears `AppState::running`, and
        // the window re-enables its buttons inside the handler that receives
        // this message. A press landing between the send and an implicit
        // end-of-scope drop races a slot this thread still holds.
        drop(slot);
        let _ = on_progress.send(JobEvent::Ended(ending));
    });

    Ok(())
}

/// One report from the pass, as the window receives it.
///
/// `failed` becomes `refused` and nothing else moves. The rename is the whole
/// visible content of this function and it is deliberate: `job::Progress`
/// already carries `skipped`, and a reader who found a second "failed" beside it
/// would have to guess which of the two a number belongs to. `refused` says who
/// did the refusing — the provider — and [`job::Progress::refused`]'s own doc
/// comment is where it is told apart from `WalkProgress::refused`, which is a
/// *file* and is merged into `skipped`.
///
/// `skipped` is `0` and not a rearrangement of the other two. An embedding run
/// passes nothing over: every chunk it takes out of the queue is embedded,
/// refused, or left exactly where it was for the next run to find.
///
/// `seconds_left` is measured from `done` alone rather than from `done +
/// failed`, which is [`job::seconds_left`]'s existing contract — the rate is
/// what the work has actually cost so far, and a refusal costs a round trip like
/// anything else.
fn progress_from(progress: EmbedProgress, elapsed: Duration) -> Progress {
    Progress {
        done: progress.done,
        total: progress.total,
        skipped: 0,
        refused: progress.failed,
        // `0` contended: an embedding pass takes the index's write lock for its
        // own writes and can wait on it, but it has no per-file retry budget to
        // exhaust and nothing to report when it does — `contended` is a walk's
        // fact about the files it could not write.
        contended: 0,
        seconds_left: job::seconds_left(progress.done, progress.total, elapsed),
    }
}

/// A finished pass, as the window receives it.
///
/// **The counts come from the tally and not from the last report**, the same
/// choice `walk_job::ended_from_report` makes and for the same reason: on this
/// path the pass itself decided the ending, and `EmbedTally` is what it counted
/// as it wrote. The last report is whatever the throttle let through, and on a
/// run whose sends were failing it is not even that.
///
/// `total` is the queue as it stood when the run began — carried from the
/// reports rather than re-read from the database, because a second measurement
/// after the run would be a different number (the queue has just been emptied)
/// standing where the first one's denominator belongs.
///
/// **`done + refused < total` is a normal ending, not a broken one.** A chunk
/// whose text changed while its own request was in flight leaves the queue
/// counted as neither — `upsert_vector_for_text` answers `false` and the pass
/// records nothing — and a cancelled run leaves whatever it had not reached. The
/// window must not read the remainder as an error; it is what the next run will
/// find.
///
/// ⚠️ **`cancelled` is read from the flag, and there is a race it settles on
/// purpose.** `mnema_embed::run` answers `Ok` both to a queue it emptied and to
/// a stop it was asked for, so the flag is the only witness — and a Stop pressed
/// in the instant between the pass's last check and this read reports
/// `Cancelled` for a run that had in fact just finished. That is the direction
/// this must be wrong in if it is wrong at all: "stopped after 9000 of 9000" is
/// odd and harmless, while "finished" told to somebody who pressed Stop is a
/// claim that the archive is fully embedded when the person has every reason to
/// believe it is not.
///
/// `complete` is `true` for the same reason it is on the probe: it is
/// [`Ended::complete`]'s walk-reconciliation question, and an embedding run has
/// no tree to have failed to read. `frozen`, `indexed`, `unchanged` and
/// `removed` are empty or zero because they are a walk's counts and this is not
/// one.
fn ended_from_tally(tally: EmbedTally, total: u64, cancelled: bool) -> Ended {
    Ended {
        reason: if cancelled {
            EndReason::Cancelled
        } else {
            EndReason::Completed
        },
        done: tally.embedded,
        total,
        skipped: 0,
        refused: tally.failed,
        complete: true,
        frozen: Vec::new(),
        indexed: 0,
        unchanged: 0,
        removed: 0,
        message: None,
    }
}

/// A pass that stopped for a reason it could not report itself — an error from
/// `mnema_embed`, or a panic — with the one number [`Ended::failed`] cannot
/// know.
///
/// `..Ended::failed(..)` rather than a fourth parameter on that constructor:
/// every other field it sets is set for a reason written on it, and three `u64`s
/// in a row at a call site is a place to pass two of them in the wrong order.
/// `refused` is the exception because the rows are already in the database when
/// this runs — `mnema_embed::run` writes a refusal before it propagates anything
/// — so unlike `indexed` or `removed` there is something real to report.
fn failed_ending(done: u64, refused: u64, total: u64, message: String) -> Ended {
    Ended {
        refused,
        ..Ended::failed(done, total, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values distinct from each other and from their own defaults, so a
    /// swapped field shows up rather than only a dropped one.
    const TALLY: EmbedTally = EmbedTally {
        embedded: 7,
        failed: 2,
    };

    /// The number the whole task exists for: it crosses from the pass's own
    /// tally into the ending, separately from `done`, and is not folded into
    /// `skipped` on the way. Without it a run that gave up on two chunks and a
    /// run that finished cleanly reach the window as the same message.
    #[test]
    fn refusals_cross_the_seam_separately_from_what_was_embedded() {
        let ended = ended_from_tally(TALLY, 10, false);
        assert_eq!(ended.refused, 2);
        assert_eq!(ended.done, 7);
        assert_eq!(
            ended.skipped, 0,
            "a refusal folded into `skipped` reads as a chunk nobody has got to yet, which is \
             the one thing it is not"
        );
    }

    /// The same for a live report. `EmbedProgress::failed` is the field
    /// `job::Progress::refused` is the other end of, and the translation is
    /// this file's whole reason to exist.
    #[test]
    fn a_report_carries_the_refusals_as_well_as_the_count() {
        let progress = progress_from(
            EmbedProgress {
                done: 7,
                total: 10,
                failed: 2,
            },
            Duration::from_secs(7),
        );
        assert_eq!(progress.done, 7);
        assert_eq!(progress.total, 10);
        assert_eq!(progress.refused, 2);
        assert_eq!(progress.skipped, 0);
    }

    /// A run that was stopped and a run that finished are not the same run, and
    /// the flag is the only thing that can tell them apart — `mnema_embed::run`
    /// answers `Ok` to both.
    #[test]
    fn a_stopped_run_is_not_reported_as_a_finished_one() {
        assert_eq!(
            ended_from_tally(TALLY, 10, true).reason,
            EndReason::Cancelled
        );
        assert_eq!(
            ended_from_tally(TALLY, 10, false).reason,
            EndReason::Completed
        );
    }

    /// Both ways round, because this is the field a person reads as "how much of
    /// my archive is done": a stopped run keeps what it wrote, so its counts are
    /// the tally's and not zero, and its denominator is the queue it started
    /// against and not what it reached.
    #[test]
    fn a_stopped_run_still_reports_what_it_managed() {
        let ended = ended_from_tally(TALLY, 10, true);
        assert_eq!(ended.done, 7);
        assert_eq!(ended.refused, 2);
        assert_eq!(ended.total, 10);
    }

    /// The failure path has the counts the window last *saw*, and the refusals
    /// with them — the rows are already written by the time the pass gives up,
    /// so reporting `0` there would take a number off the screen that the
    /// database still holds.
    #[test]
    fn a_failed_run_still_says_how_many_were_refused() {
        let ended = failed_ending(7, 2, 10, "the provider stopped answering".into());
        assert_eq!(ended.reason, EndReason::Failed);
        assert_eq!(ended.done, 7);
        assert_eq!(ended.refused, 2);
        assert_eq!(ended.total, 10);
        assert_eq!(
            ended.message.as_deref(),
            Some("the provider stopped answering")
        );
    }

    /// An empty queue is a real ending and not a defect: nothing was waiting,
    /// so nothing was reported, and `0 of 0` is what the run actually did. It is
    /// the state a second press produces the moment the first one finished.
    #[test]
    fn a_run_with_nothing_queued_ends_at_zero_of_zero() {
        let ended = ended_from_tally(
            EmbedTally {
                embedded: 0,
                failed: 0,
            },
            0,
            false,
        );
        assert_eq!(ended.reason, EndReason::Completed);
        assert_eq!(ended.done, 0);
        assert_eq!(ended.total, 0);
        assert_eq!(ended.refused, 0);
    }
}

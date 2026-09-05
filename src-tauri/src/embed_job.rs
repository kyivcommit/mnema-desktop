//! What `mnema_embed::run` produced, translated into the shell's own
//! vocabulary — [`Progress`] and [`Ended`] — for [`crate::scan_job`]'s
//! embedding phase to report from.
//!
//! **The translation is the point of this file.** `mnema_embed::EmbedProgress`
//! and [`crate::job::Progress`] are two types with deliberately different
//! names, and `EmbedProgress`'s own doc comment says why: the name is taken
//! twice over, and two types called the same thing would make the step between
//! them invisible at exactly the place it is easiest to forget. That step is
//! [`progress_from`], and the ending's is [`ended_from_tally`]; both are
//! ordinary functions with unit tests below rather than closures inside the
//! thread, so that what crosses the seam is something a test can hold.
//!
//! ⚠️ **This file no longer runs an embedding pass itself.** Until Task 3b it
//! was also `start_embed_job`, the window-driven command that ran the whole
//! index's queue as its own job — `#[tauri::command(async)] pub fn
//! start_embed_job(state, on_progress: Channel<JobEvent>)`, reading the
//! credential store before it claimed the slot and chaining nothing after
//! it. It was unregistered from `invoke_handler!` when [`crate::scan_job`]'s
//! embedding phase took over (that file's own `embed_after` is the
//! replacement, deliberately reordered — D-g — to claim the slot BEFORE the
//! key is read, the opposite of this command's own rule above), and deleted
//! here once the test functions that had driven it directly as a fixture,
//! across `tests/commands.rs`, `tests/model_commands.rs` and
//! `tests/mask_differential.rs`, moved onto the scan job's `Entry::EmbedOnly`
//! instead. `ui/` still names it until Task 6 rewrites the window side. What
//! is left is the translation this file was always more than half of —
//! [`progress_from`], [`ended_from_tally`], [`failed_ending`] and [`BATCH`]
//! — which `scan_job.rs` still calls.

use std::time::Duration;

use mnema_embed::{EmbedProgress, EmbedTally};

use crate::job::{self, EndReason, Ended, Progress};

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
pub(crate) const BATCH: usize = 32;

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
pub(crate) fn progress_from(progress: EmbedProgress, elapsed: Duration) -> Progress {
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
pub(crate) fn ended_from_tally(tally: EmbedTally, total: u64, cancelled: bool) -> Ended {
    Ended {
        reason: if cancelled {
            EndReason::Cancelled
        } else {
            EndReason::Completed
        },
        done: tally.embedded,
        total,
        // `0` for the reason `Progress::contended` gives: an embedding pass
        // takes no index write lock a walk could find held against it.
        //
        // Written ABOVE `skipped` rather than in field order, because two
        // mutation cases in `scripts/mutations/embedding.sh` quote `skipped`,
        // `refused` and `complete` as one adjacent block, on both sides of
        // their substitution. A field inserted anywhere inside that block
        // leaves both cases proving nothing while still reporting green.
        contended: 0,
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
pub(crate) fn failed_ending(done: u64, refused: u64, total: u64, message: String) -> Ended {
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

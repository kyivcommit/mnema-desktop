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
#[tauri::command(async)]
pub fn start_walk_job(
    state: State<'_, AppState>,
    root_id: i64,
    on_progress: Channel<JobEvent>,
) -> Result<(), Error> {
    let slot = state.claim_job()?;

    let root = state
        .with_index(|db| db.watched_root_path(root_id))?
        .ok_or(Error::UnknownWatchedRoot(root_id))?;
    let root = PathBuf::from(root);

    // The job's own connection, not the window's — see `AppState::
    // open_job_index`'s own doc comment. A walk is a sequence of writes that
    // can run for hours; the window must keep answering searches while it
    // does.
    let job_db = state.open_job_index()?;

    // No exclusion-rule command exists yet — nothing in this task adds one —
    // so every watched folder walks with the built-in list and `.gitignore`
    // on and no user prefixes. **Provisional**, in the same sense
    // `paths::worker_path` is: the smallest default that lets a walk run at
    // all, not a stand-in for the settings UI the interface spec still owes.
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

    std::thread::spawn(move || {
        // The last count, and the last total, the window was actually shown —
        // read on every failure path below, not only the panic one. Updated
        // together, and only after a successful send, for the same reason
        // `start_probe_job` records `reported` only then: `Ended::failed`
        // promises what the window *saw*, not the loop's internal position.
        let reported = AtomicU64::new(0);
        let last_total = AtomicU64::new(0);
        let started = Instant::now();

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
            // which of the two produced it.
            Ok(Err(_ingest_error)) => job::Ended::failed(
                reported.load(Ordering::Relaxed),
                last_total.load(Ordering::Relaxed),
            ),
            Err(_panic) => job::Ended::failed(
                reported.load(Ordering::Relaxed),
                last_total.load(Ordering::Relaxed),
            ),
        };
        let _ = on_progress.send(JobEvent::Ended(ending));
        // `slot`, `pool` and `job_db` are dropped here: the job slot frees,
        // the pool's workers are asked to exit, and the job's own connection
        // closes — whether the walk finished, was cancelled, or panicked.
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
            why: frozen_reason_text(f.why).to_string(),
        })
        .collect();
    Ended {
        reason,
        done,
        total,
        frozen,
    }
}

/// A sentence for each `FrozenReason`. `FrozenReason` itself has no
/// `Serialize` — see [`crate::job::Frozen`]'s own doc comment for why — so
/// this is where the closed vocabulary becomes the free text `Ended.frozen`
/// actually carries to the window.
fn frozen_reason_text(why: FrozenReason) -> &'static str {
    match why {
        FrozenReason::SymlinkedSubtree => {
            "a symlink to a directory; the walk does not follow it, so it has no evidence \
             about what used to be there before it became a symlink"
        }
        FrozenReason::EmptyDirectory => {
            "this folder now reads as empty, and an unmounted share looks exactly like a \
             folder emptied by hand from here — resolve it by hand"
        }
        FrozenReason::UnreadableDirectory => {
            "this folder could not be read, most likely a permissions problem"
        }
    }
}

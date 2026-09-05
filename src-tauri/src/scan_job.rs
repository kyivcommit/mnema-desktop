//! The scanning job: every watched folder read under ONE slot, and what each
//! folder said.
//!
//! **Why one job rather than the chain it replaces.** `walk_job` walked one
//! folder, told the window, and the window decided what to do next — walk
//! another, start the embedding pass, or nothing. Every one of those decisions
//! sat between two `claim_job` calls, and `claim_job` clears the cancellation
//! flag as it takes the slot: a Stop pressed in a gap was not late, it was
//! erased, and the person's text went to the provider anyway. The former
//! `start_walk_job`'s own long comment above its `stopped_late` flag — both
//! gone since Task 3b deleted the command that owned them — is where that was
//! measured and where this file was promised; D-h, below, is where the
//! argument now lives. One claim, held across every folder, is what removes
//! the gaps rather than narrowing them.
//!
//! **What it produces, and where each half lives.** A scan writes two values.
//! [`crate::scan_state::ReadingOutcome`] is what the folders said and outlives
//! the job that asked them, so it goes on [`crate::scan_state::ScanState::
//! last_reading`]; [`ScanReport`] is how this particular job ended and is
//! replaced by the next one, so it goes in the snapshot. D-e is that split, and
//! `ScanReport`'s own doc comment has the reason a window needs both.
//!
//! Nothing here is a Tauri type but the command wrapper's `State`, for
//! [`crate::job`]'s reason: a job that can only be observed through a webview
//! cannot be observed by the tray, by a second window, or by a test.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use mnema_index::Db;
use mnema_ingest::{WalkProgress, walk_root};
use mnema_pool::{Pool, PoolConfig};
use mnema_walk::WalkRules;
use tauri::State;

use crate::error::Error;
use crate::job::{self, EndReason, Ended, Progress};
use crate::scan_state::{
    EmbedOutcome, EndedIn, Entry, Phase, ReadingOutcome, RootOutcome, ScanReport, SkipWhy, Terminal,
};
use crate::state::{AppState, JobSlot};
use crate::walk_job::ended_from_report;

/// One watched folder, with everything the walk needs to read it, decided
/// before the job thread exists.
///
/// The rules are built here rather than inside the loop because building them
/// can FAIL — a stored exclusion prefix that `WalkRules::new` refuses — and a
/// refusal is something the person asking for a scan has to be told about, not
/// something a thread discovers three folders in. [`read_roots`]'s own comment
/// on the same call, below, has the whole argument for refusing rather than
/// walking with the rule silently absent — the argument the deleted
/// `start_walk_job` used to make in its own words before Task 3b.
pub(crate) struct PreparedRoot {
    pub(crate) id: i64,
    pub(crate) path: PathBuf,
    pub(crate) rules: WalkRules,
}

/// What [`read_roots`] answers with: every watched folder, in the order the
/// index lists them, each already carrying its rules.
pub(crate) type Roots = Vec<PreparedRoot>;

/// The two things the embedding phase reaches the outside world through.
///
/// Injected rather than called directly so that the embedding phase can be
/// driven from a test without a credential store and without a provider: the
/// store can put an authorisation dialog on a developer's screen, and the
/// provider costs money. [`ScanDeps::production`] is the one place the real
/// pair is named.
///
/// 🔴 **Owned `Arc`s and not `&'a dyn`, because this crosses `thread::spawn`.**
/// The embedding phase runs on the job's own thread, and a borrowed pair would
/// tie that thread's lifetime to the caller's stack frame — the command returns
/// the moment the thread is spawned, so there is no frame left to borrow from.
/// The `Arc` is cloned into the thread and dropped with it.
#[derive(Clone)]
pub(crate) struct ScanDeps {
    pub(crate) key: std::sync::Arc<KeyFn>,
    pub(crate) embed: std::sync::Arc<EmbedFn>,
}

/// The credential store, as one question: the key for this installation, or
/// `None` when the person has not entered one.
///
/// It takes **nothing**, and that is what makes it callable from the job
/// thread: `AppState` does not cross into the thread (see [`start_inner`]), so
/// a signature taking `&AppState` could only be called before the spawn — which
/// is the one place D-g says it must not be called from.
/// [`ScanDeps::production`] captures the credential reference instead.
pub(crate) type KeyFn = dyn Fn() -> Result<Option<String>, Error> + Send + Sync;

/// `mnema_embed::run`, with its own signature rather than a narrowed one: the
/// point of the seam is that the embedding phase cannot tell the difference.
pub(crate) type EmbedFn = dyn Fn(
        &Db,
        &str,
        &str,
        &dyn Fn() -> bool,
        &mut dyn FnMut(mnema_embed::EmbedProgress),
    ) -> Result<mnema_embed::EmbedTally, mnema_embed::Error>
    + Send
    + Sync;

impl ScanDeps {
    /// The real credential store and the real embedding pass.
    ///
    /// The credential reference is read from the state **here**, on the calling
    /// thread, and moved into the closure — the closure itself is then free of
    /// `AppState` and can be called from the job thread, which is where D-g
    /// puts the key read so that a keychain dialog is waited for under the
    /// `Embedding` phase rather than in front of the claim.
    pub(crate) fn production(state: &AppState) -> Self {
        let credential_ref = state.credential_ref().to_string();
        Self {
            key: std::sync::Arc::new(move || Ok(mnema_secrets::load(&credential_ref)?)),
            embed: std::sync::Arc::new(|db, base, key, cancel, on_progress| {
                mnema_embed::run(db, base, key, crate::embed_job::BATCH, cancel, on_progress)
            }),
        }
    }
}

/// Reads every watched folder, in order, under one job slot.
///
/// `(async)` for the reason given on [`crate::bridge::open_index`], once
/// repeated by the deleted `walk_job::start_walk_job` too: this command reads
/// the index before it spawns anything, and a window-issued command that can
/// wait on the same mutex must not be the one left running inline on the main
/// thread.
#[tauri::command(async)]
pub fn start_scan_job(state: State<'_, AppState>, entry: Entry) -> Result<(), Error> {
    start(&state, entry)
}

/// The command, reachable from Rust — the shape `run_walk_and_capture_ending`
/// already uses for the walk, and what the tray will call.
pub(crate) fn start(state: &AppState, entry: Entry) -> Result<(), Error> {
    let deps = ScanDeps::production(state);
    start_inner(state, entry, deps)
}

/// The key the index carries while a scan is under way, and the one it keeps if
/// the scan never gets to the end.
///
/// A named constant rather than the literal twice over: it is written by the
/// reading pass and read by [`crate::models::read_settings`], and two literals
/// one file apart is how a marker comes to be written under one spelling and
/// asked for under another.
pub(crate) const SCAN_INCOMPLETE: &str = "scan.incomplete";

/// 🔴 **The order of the first three steps is the decision this file is about**
/// (D-f), and it is not the order that reads most naturally.
///
/// `claim_job` comes FIRST, before the index is read — the opposite of the
/// deleted `walk_job::start_walk_job`, where every fallible step ran before
/// the claim so that a call which was always going to fail never had
/// `job_status` reporting a running job. That rule is right for a command that
/// reads one folder's row and is wrong here, because what this reads is the
/// **whole list of folders**, and the list is what another command is free to
/// change. Between an unclaimed read and the walk that acts on it, a folder can
/// be removed and another added: SQLite hands the new row the id the old one
/// gave up, and the walk then writes the first folder's files into the second
/// folder's `path` rows — an index whose citations name files that are not
/// there, produced by a scan that reports `completed`. Taking the slot first is
/// what makes the removal command refuse (`Error::JobAlreadyRunning`) instead
/// of racing.
///
/// The cost is the one the deleted `start_walk_job` used to avoid: a scan
/// that fails on a stored exclusion prefix has held the slot for the length
/// of one index read. It is paid deliberately, and the ending is not silent
/// — the slot drops with the phase still `Reading`, so [`crate::state::
/// JobSlot::drop`]'s policy writes `Ended { Failed, "the job ended without a
/// report" }` and the window is told something went wrong rather than being
/// left to notice an idle application.
///
/// `open_job_index` is between the two: a scan is a sequence of writes that
/// can run for hours and the window has to go on answering searches, so the
/// job gets a connection of its own — the same reason the deleted
/// `start_walk_job` gave its own walk one.
pub(crate) fn start_inner(state: &AppState, entry: Entry, deps: ScanDeps) -> Result<(), Error> {
    // Resolved on this thread for the reason `ScanDeps::production` gives about
    // the credential reference: `AppState` does not cross into the job.
    let base = state.provider_base().to_string();

    // 🔴 **D-f: the entry point branches BEFORE any preflight.** `EmbedOnly` is
    // a resumption of a reading pass that already finished, so everything the
    // reading phase does first — reading the list of folders, building the
    // rules, counting the roots — is work whose answer this entry has no use
    // for. Worse than useless: a stored exclusion prefix that `WalkRules::new`
    // refuses would refuse the resumption too, leaving a person whose chunks
    // are waiting unable to embed them until they fix a rule that this run was
    // never going to apply. `last_reading` is untouched for the same reason —
    // this run reads no folder, so it has nothing to say about one.
    if entry == Entry::EmbedOnly {
        let slot = state.claim_job(
            Phase::Embedding {
                counts: Progress::default(),
            },
            true,
        )?;
        let job_db = state.open_job_index()?;
        std::thread::spawn(move || embed_after(slot, job_db, deps, base));
        return Ok(());
    }

    // Zero folders and an empty path: what is true at the moment the slot is
    // taken, since the list has not been read yet. The `update` below replaces
    // it with the real count before the thread starts, so nothing draws this
    // for longer than one index read — but it is the honest value here, and an
    // invented `root_count` would be a number the window could show.
    let slot = state.claim_job(
        Phase::Reading {
            root_index: 0,
            root_count: 0,
            root_path: String::new(),
            counts: Progress::default(),
        },
        true,
    )?;

    let job_db = state.open_job_index()?;
    let roots = read_roots(state)?;
    let root_count = roots.len() as u64;

    slot.update(Phase::Reading {
        root_index: 0,
        root_count,
        root_path: String::new(),
        counts: Progress::default(),
    });

    // Resolved on this thread, not inside the job: `AppState` does not cross
    // into the thread, and the worker's path is the only thing from it the
    // reading pass still needs.
    let worker = state.worker_path().to_path_buf();
    std::thread::spawn(move || read_every_root(slot, job_db, worker, roots, deps, base));
    Ok(())
}

/// Every watched folder and the rules it walks under, read under ONE
/// `with_index` lock.
///
/// One lock rather than one per folder: `Db::delete_watched_root` runs as a
/// single transaction that cascades a root's exclusion rows away with it, so a
/// path read before it and an exclusion list read after it describe two
/// different indexes — the same reason the deleted `start_walk_job` read its
/// own root this way, before there was more than one to read at once. The
/// masks join the same read not because they belong to a root —
/// they belong to none (D-c) — but because they are part of the same one
/// question, "the rules this scan runs under".
///
/// The rules are built AFTER the guard is released. Nothing about
/// `WalkRules::new` needs the database, and holding the index's mutex across
/// work that does not need it is what keeps a window from answering searches.
pub(crate) fn read_roots(state: &AppState) -> Result<Roots, Error> {
    let (rows, masks) = state.with_index(|db| {
        let masks = db.list_masks()?;
        let mut rows = Vec::new();
        for root in db.list_watched_roots()? {
            let prefixes = db.list_path_exclusions(root.id)?;
            rows.push((root.id, root.absolute_path, prefixes));
        }
        Ok((rows, masks))
    })?;

    // 🔴 Here, and nowhere else (D-l). The hook models a second command
    // arriving in the window between the read and the walk, so it has to run
    // after the guard above is released — a hook that took the index's mutex
    // while this function still held it would deadlock rather than race — and
    // before this function returns, so that moving this whole call above
    // `claim_job` moves the hook with it. That is the mutant the swap-race test
    // exists to kill, and a hook installed at the call site instead would stay
    // put while the call moved.
    test_hook(state);

    let mut prepared = Vec::with_capacity(rows.len());
    for (id, path, prefixes) in rows {
        // The same fixed defaults and the same refusal the deleted
        // `start_walk_job` used, and the refusal is the point: under D29 an
        // indexed file is a file
        // whose text is sent to a third-party provider, so a rule that will not
        // apply stops the scan rather than being silently absent. `with_masks`
        // REPLACES the mask set, so the whole stored set goes in one call.
        let rules = WalkRules::new(true, true, prefixes)?.with_masks(masks.clone())?;
        prepared.push(PreparedRoot {
            id,
            path: PathBuf::from(path),
            rules,
        });
    }
    Ok(prepared)
}

/// The reading pass itself: every folder in turn, on the job's own thread, and
/// the embedding phase after it.
fn read_every_root(
    slot: JobSlot,
    job_db: Db,
    worker: PathBuf,
    roots: Roots,
    deps: ScanDeps,
    base: String,
) {
    let root_count = roots.len() as u64;
    let mut outcome = ReadingOutcome {
        root_count,
        ..ReadingOutcome::default()
    };
    // What ended the pass, when something did — the message belongs to the
    // folder that stopped it, and is carried out rather than looked up
    // afterwards, because `roots.last()` is the wrong folder whenever the pass
    // stopped before a folder answered at all.
    let mut message = None;

    // Before the first walk, so that a scan interrupted by a crash or a power
    // cut leaves a mark rather than an index that looks finished. Task 3 is
    // what clears it, at the end of a scan that got all the way through; a scan
    // that ends here leaves it set, which is exactly what an unfinished scan
    // should leave behind. A failure to write it is not a reason to refuse to
    // scan: the flag is a hint for the next run, and the run in front of it is
    // the one the person asked for.
    let _ = job_db.meta_set(SCAN_INCOMPLETE, "1");

    for (index, root) in roots.iter().enumerate() {
        // Asked before each folder rather than only inside the walk: a Stop
        // that lands between two folders is a Stop, and `walk_root` reads the
        // flag only while it is running.
        if slot.cancel_flag().load(Ordering::SeqCst) {
            outcome.reason = EndReason::Cancelled;
            break;
        }

        let root_path = root.path.display().to_string();
        let contended_seen = AtomicU64::new(0);

        // 🔴 A fresh `Pool` per folder, INSIDE the loop. `Pool::poisoned` is
        // keyed on the path alone — no size, no modification time, no expiry —
        // so an entry made while reading one folder answers for whatever is at
        // that path while reading the next one, without a worker ever being
        // asked. Two watched folders can overlap, and a file poisoned under the
        // first would then be skipped under the second without evidence.
        // `mnema_ingest::walk_root`'s own doc comment states the obligation:
        // the pool handed to a walk must not outlive it.
        let pool = match Pool::new(PoolConfig::new(&worker)) {
            Ok(pool) => pool,
            Err(refusal) => {
                let refusal = mnema_ingest::IngestError::Pool(refusal);
                outcome.absorb(failed_root(root_path, refusal.to_string(), 0));
                outcome.reason = EndReason::Failed;
                message = Some(refusal.to_string());
                break;
            }
        };

        let mut ticker = RootProgress::new(Instant::now());
        let caught = catch_unwind(AssertUnwindSafe(|| {
            walk_root(
                &pool,
                &job_db,
                root.id,
                &root.path,
                &root.rules,
                slot.cancel_flag(),
                &mut |progress| {
                    if let Some(counts) = ticker.observe(&progress, Instant::now(), &contended_seen)
                    {
                        slot.update(Phase::Reading {
                            // One-based: "3 of 7" is what a person reads, and
                            // `root_index` is the folder being read now rather
                            // than how many are behind it.
                            root_index: index as u64 + 1,
                            root_count,
                            root_path: root_path.clone(),
                            counts,
                        });
                    }
                },
            )
        }));

        let contended = contended_seen.load(Ordering::Relaxed);
        let done = match caught {
            // `ended_from_report` no longer takes a late-Stop flag at all
            // (`walk_job.rs`'s own doc comment on it is where that moved): this
            // pass reads the cancel flag itself, at the top of the next
            // iteration, and a folder that finished everything it was given
            // DID complete. Rewriting its own reason to `Cancelled` here would
            // lose the one fact the per-folder row is for. D-h, below, is
            // where a Stop landing after the very last folder's report is
            // still caught — at the PASS's boundary, not this folder's row.
            Ok(Ok(report)) => root_outcome(root_path, &ended_from_report(&report, contended)),
            Ok(Err(refusal)) => failed_root(root_path, refusal.to_string(), contended),
            Err(panic) => failed_root(root_path, job::panic_message(&*panic), contended),
        };

        // 🔴 Counted BEFORE the decision to stop, and that ordering is the
        // whole of D-i. A folder that answered has told the pass something —
        // how many files it indexed before the worker broke, how far it got
        // before the Stop — and a `break` taken first throws exactly those
        // numbers away. The folder that ended the scan is the one a person most
        // wants the counts for.
        let stop = after_root(done.reason);
        let stop_message = done.message.clone();
        outcome.absorb(done);
        if let Some(reason) = stop {
            outcome.reason = reason;
            message = stop_message;
            break;
        }
    }

    // 🔴 The window the boundary hook models: every folder has answered, and
    // the pass has not yet asked whether somebody pressed Stop while it was
    // absorbing the last one. Installed here rather than at a call site for
    // [`test_hook`]'s reason — a hook that moved with the call site would stay
    // put while the lines it separates moved.
    boundary_hook();

    // 🔴 **D-h: a Stop at the boundary is a Stop.** Every folder can have
    // answered `Completed` and a person still have pressed Stop — during the
    // last folder's phase 3, or while this pass was absorbing its report. The
    // loop's own check is at the TOP of an iteration, so on the last folder
    // there is no next iteration to ask, and without this line the pass would
    // report a scan that finished and then hand a person's text to the provider
    // in the phase below.
    //
    // 🔴 **It overwrites `Completed` and only `Completed`**, and the condition
    // is the finding rather than a tidiness. A pass that ended `Failed`,
    // `BrokenWorker` or `RulesNotApplied` broke out of the loop carrying the
    // MESSAGE of the folder that stopped it, and `Cancelled` written over that
    // leaves a diagnostic sentence about a broken worker filed under a reason
    // that says a person pressed Stop — with `resume_for` then offering
    // `Some(Full)` where `RulesNotApplied` owes `None`, a button that spends the
    // time and fails the same way. The Stop is not lost: the folders' own rows
    // and the report's `message` still say what happened, and the phase below
    // is skipped either way because the reason is not `Completed`.
    if outcome.reason == EndReason::Completed && slot.cancel_flag().load(Ordering::SeqCst) {
        outcome.reason = EndReason::Cancelled;
    }

    // 🔴 **D-j: cleared when every folder was VISITED, which is not the same
    // question as whether the archive was fully seen.** The marker answers "did
    // a scan get all the way round the folders, or did it stop half-way and
    // leave the index describing a state nothing finished writing" — so a pass
    // that read every folder and met an unreadable subdirectory in one of them
    // clears it, while `last_reading.complete` stays `false` and is what says
    // the archive is not fully accounted for. A pass over zero folders clears
    // it too: it visited every folder there was.
    //
    // Before `mark_reading_done`, and that ordering is what the announcement
    // makes matter: an observer woken by the pass ending reads the index for
    // itself, and would otherwise find the marker still claiming a scan is
    // half-done over a `last_reading` that says it finished.
    if outcome.roots_read == outcome.root_count && outcome.reason == EndReason::Completed {
        let _ = job_db.meta_set(SCAN_INCOMPLETE, "0");
    }

    slot.mark_reading_done(outcome.clone());

    let reason = outcome.reason;
    if reason != EndReason::Completed {
        // `ok()`, not `unwrap_or(0)`: a count that could not be read is "the
        // last count still stands", which is what `JobSlot::finish` does with
        // `None`. A zero would tell a window the index had emptied itself.
        let files = job_db.indexed_file_count().ok();
        slot.finish(
            Terminal::Ended {
                report: ScanReport {
                    // The phase was never entered, which is a different answer
                    // from "it ran and embedded nothing" — see
                    // [`EmbedOutcome::NotReached`].
                    embedding: EmbedOutcome::NotReached,
                    ended_in: EndedIn::Reading,
                    reason,
                    message,
                    resume: resume_for(reason, EndedIn::Reading),
                },
            },
            files,
        );
        return;
    }

    embed_after(slot, job_db, deps, base);
}

/// The embedding phase, and the ending of the whole job.
///
/// One function for both entry points, called from the same place in both: the
/// `EmbedOnly` resumption and the tail of a reading pass run **the same code**,
/// so a rule that holds for one cannot come to hold only for one. It takes the
/// slot by value because it is what finishes the job.
///
/// 🔴 **The order of the first three steps is D-g and it is not the order that
/// reads most naturally.**
///
/// 1. The phase is announced BEFORE the key is read. On macOS the credential
///    store can put an authorisation dialog on screen and wait for a person to
///    answer it, and a phase announced afterwards would leave every surface
///    drawing `Reading` — a folder name and a progress bar — for the whole of
///    that wait, over a pass that had finished reading folders.
/// 2. The key is read on THIS thread. That is the whole reason it is not read
///    in the command: a dialog waited for in front of `claim_job` is a minute
///    with Start disabled and no job to show for it.
/// 3. The Stop check comes between the read and anything the key is used for,
///    and it wins **whatever the store answered** — including the answers that
///    look like they settle the matter on their own. A person who pressed Stop
///    while the dialog was up asked for this job to end, and an ending that
///    said `Completed` because there happened to be no key would be reporting a
///    scan that finished to somebody who stopped it.
fn embed_after(slot: JobSlot, job_db: Db, deps: ScanDeps, base: String) {
    slot.update(Phase::Embedding {
        counts: Progress::default(),
    });

    let answer = (deps.key)();

    if slot.cancel_flag().load(Ordering::SeqCst) {
        finish_embedding(
            slot,
            &job_db,
            EmbedOutcome::NotReached,
            EndReason::Cancelled,
            None,
        );
        return;
    }

    let key = match answer {
        Ok(Some(key)) => key,
        // Nobody has entered one. An ordinary state — a fresh installation is
        // in it — and so the JOB completed: it read every folder and had
        // nothing it was allowed to do next.
        Ok(None) => {
            skip_embedding(slot, &job_db, SkipWhy::NoKey);
            return;
        }
        // The store would not answer at all: a locked keychain, an absent
        // Secret Service session. Kept apart from `NoKey` because the two tell
        // a person to do opposite things — enter a key, or unlock a store that
        // may already hold one — and `mnema_secrets::load` is the layer that
        // can still tell them apart.
        Err(Error::Secrets(refusal)) => {
            skip_embedding(
                slot,
                &job_db,
                SkipWhy::StoreUnavailable {
                    message: refusal.to_string(),
                },
            );
            return;
        }
        // Anything else is this build going wrong rather than the machine, and
        // is reported as the failure it is.
        Err(other) => {
            finish_embedding(
                slot,
                &job_db,
                EmbedOutcome::NotReached,
                EndReason::Failed,
                Some(other.to_string()),
            );
            return;
        }
    };

    match job_db.active_space() {
        // No model has been chosen, so there is nothing to embed INTO. Asked
        // here rather than left to `mnema_embed::run`'s own `NoActiveSpace`
        // refusal, which `start_embed_job` did leave it to: that refusal is an
        // ending carrying a sentence, and this is a state a person acts on by
        // choosing a model. A closed reason is something a window can offer
        // that button for.
        Ok(None) => {
            skip_embedding(slot, &job_db, SkipWhy::NoModel);
            return;
        }
        Ok(Some(_)) => {}
        Err(refusal) => {
            finish_embedding(
                slot,
                &job_db,
                EmbedOutcome::NotReached,
                EndReason::Failed,
                Some(refusal.to_string()),
            );
            return;
        }
    }

    // The last counts a surface was actually SHOWN, for the reason
    // `embed_job.rs` gives on the same pair: they are read only on the paths
    // where the pass produced no tally to read instead.
    let reported_done = AtomicU64::new(0);
    let reported_refused = AtomicU64::new(0);
    // The size of the queue this run started against, written on every report
    // whether or not the throttle published it — `mnema_embed` measures it once,
    // before it takes anything out of the queue, so it is a fact about the run
    // and the honest denominator for an ending. `0` here (a run stopped in its
    // first instant, before any report) is deliberately not repaired by
    // reading the queue again from this side: that would be a second
    // measurement, taken after the pass stopped, and it could disagree with
    // the one the run actually used — the deleted `start_embed_job` made the
    // same choice for the same reason.
    let queue_total = AtomicU64::new(0);
    let started = Instant::now();
    // A plain local rather than an atomic: the pass calls this closure
    // synchronously, on this one thread.
    let mut last_report: Option<Instant> = None;

    // `AssertUnwindSafe` for `embed_job.rs`'s reason: everything this closure
    // reaches is used only on this thread and dropped when it ends. A panic is
    // caught all the same, because the slot owes an ending — without one,
    // `JobSlot::drop` writes "the job ended without a report" and the phase it
    // names is the one this function announced.
    let caught = catch_unwind(AssertUnwindSafe(|| {
        (deps.embed)(
            &job_db,
            &base,
            &key,
            &|| slot.cancel_flag().load(Ordering::SeqCst),
            &mut |progress| {
                queue_total.store(progress.total, Ordering::Relaxed);

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
                reported_done.store(progress.done, Ordering::Relaxed);
                reported_refused.store(progress.failed, Ordering::Relaxed);

                slot.update(Phase::Embedding {
                    counts: crate::embed_job::progress_from(progress, started.elapsed()),
                });
            },
        )
    }));

    // Read once, after the pass has returned, and it is the only thing that can
    // tell a run that emptied the queue from one that was stopped:
    // `mnema_embed::run` answers `Ok(tally)` to both and says nothing about
    // which. `ended_from_tally` is where the narrow race that leaves is settled,
    // and which direction it is settled in.
    let cancelled = slot.cancel_flag().load(Ordering::SeqCst);
    let total = queue_total.load(Ordering::Relaxed);
    let ended = match caught {
        Ok(Ok(tally)) => crate::embed_job::ended_from_tally(tally, total, cancelled),
        Ok(Err(refusal)) => crate::embed_job::failed_ending(
            reported_done.load(Ordering::Relaxed),
            reported_refused.load(Ordering::Relaxed),
            total,
            refusal.to_string(),
        ),
        Err(panic) => crate::embed_job::failed_ending(
            reported_done.load(Ordering::Relaxed),
            reported_refused.load(Ordering::Relaxed),
            total,
            job::panic_message(&*panic),
        ),
    };

    finish_embedding(
        slot,
        &job_db,
        EmbedOutcome::Ran {
            done: ended.done,
            total: ended.total,
            refused: ended.refused,
        },
        ended.reason,
        ended.message,
    );
}

/// A phase that was reached and declined to run.
///
/// 🔴 **`Completed`, and `resume: None`.** Every one of these is a state a
/// person changes by doing something — entering a key, unlocking a store,
/// choosing a model — and none of them is changed by running the same scan
/// again. An ending that said `Failed` would be a defect report for a fresh
/// installation, and a `resume` would be a button that spends the time and
/// reaches exactly here again. What went wrong is [`SkipWhy`], on the outcome,
/// where a window can act on it rather than string-match a sentence.
fn skip_embedding(slot: JobSlot, job_db: &Db, why: SkipWhy) {
    finish_embedding(
        slot,
        job_db,
        EmbedOutcome::Skipped { why },
        EndReason::Completed,
        None,
    );
}

/// The one ending every path through [`embed_after`] leaves by, so that
/// `ended_in` and the resumption table cannot come to disagree between two of
/// them.
fn finish_embedding(
    slot: JobSlot,
    job_db: &Db,
    embedding: EmbedOutcome,
    reason: EndReason,
    message: Option<String>,
) {
    // `ok()` for the reason the reading pass's own read gives: a count that
    // could not be read leaves the last one standing.
    let files = job_db.indexed_file_count().ok();
    slot.finish(
        Terminal::Ended {
            report: ScanReport {
                embedding,
                ended_in: EndedIn::Embedding,
                reason,
                message,
                resume: resume_for(reason, EndedIn::Embedding),
            },
        },
        files,
    );
}

/// Whether the pass carries on after a folder that ended with `reason`, and
/// with what if it does not.
///
/// The split is between an ending that is **about the folder** and one that is
/// **about the run**. A folder that is not there (`RootUnavailable`) or that may
/// be an unmounted volume (`VolumeMissing`) says nothing about the next folder,
/// and stopping there would leave six readable folders unread because of one
/// ejected disk. A broken worker, exclusion rules that would not apply and a
/// Stop are all facts about this machine, this build or this person's
/// intention, and carrying on would repeat the same failure per folder — or,
/// for `RulesNotApplied`, index a folder with a rule the person believes is
/// keeping their files out of a provider's hands.
pub(crate) fn after_root(reason: EndReason) -> Option<EndReason> {
    match reason {
        EndReason::Completed | EndReason::RootUnavailable | EndReason::VolumeMissing => None,
        EndReason::Cancelled
        | EndReason::Failed
        | EndReason::BrokenWorker
        | EndReason::RulesNotApplied => Some(reason),
    }
}

/// What the next scan should be, having ended this way — or `None` when there
/// is nothing left for a next scan to pick up.
///
/// Two questions and not one, which is why `ended_in` is here: `Cancelled` in
/// the reading phase leaves folders unread, so the next run is a whole scan;
/// `Cancelled` in the embedding phase leaves only chunks unembedded, and a
/// whole scan would re-read every folder to find that out.
///
/// The `None` rows are the ones where running again changes nothing on its own.
/// `Completed` is finished. `RulesNotApplied` needs the person to fix a stored
/// rule first, and a resumption that simply ran again would fail the same way.
/// `RootUnavailable` and `VolumeMissing` need a folder to come back — a disk to
/// be plugged in — and, for `VolumeMissing` especially, the whole point of the
/// pause (D33) is that the application does NOT act on its own until somebody
/// has looked.
pub(crate) fn resume_for(reason: EndReason, ended_in: EndedIn) -> Option<Entry> {
    match reason {
        EndReason::Completed
        | EndReason::RulesNotApplied
        | EndReason::RootUnavailable
        | EndReason::VolumeMissing => None,
        EndReason::Cancelled | EndReason::Failed | EndReason::BrokenWorker => {
            Some(match ended_in {
                EndedIn::Reading => Entry::Full,
                EndedIn::Embedding => Entry::EmbedOnly,
            })
        }
    }
}

/// One folder's row, translated from the ending the walk produced.
fn root_outcome(root_path: String, ended: &Ended) -> RootOutcome {
    RootOutcome {
        root_path,
        reason: ended.reason,
        complete: ended.complete,
        message: ended.message.clone(),
        done: ended.done,
        total: ended.total,
        indexed: ended.indexed,
        unchanged: ended.unchanged,
        skipped: ended.skipped,
        removed: ended.removed,
        contended: ended.contended,
        frozen: ended.frozen.clone(),
    }
}

/// A folder that answered with an error or a panic instead of a report.
///
/// `Ended::failed` is what decides every field but the contention count: there
/// is no `WalkReport` to read `indexed`, `frozen` or `complete` from, and
/// `false` for `complete` is the side that costs nothing to be wrong about.
/// `contended` survives because it never came from the report in the first
/// place — see [`crate::job::Ended::contended`].
fn failed_root(root_path: String, message: String, contended: u64) -> RootOutcome {
    root_outcome(
        root_path,
        &Ended {
            contended,
            ..Ended::failed(0, 0, message)
        },
    )
}

impl ReadingOutcome {
    /// Adds one folder's answer to the pass.
    ///
    /// `complete` is an AND and every counter is a sum, both over the folders
    /// that ANSWERED — this function is the only writer of either, and a folder
    /// the pass never reached never gets here. So a pass stopped at the second
    /// of seven leaves `complete` describing the one folder it read, `true` if
    /// that folder was fine. That is the honest reading of "every folder was
    /// read completely and reconciled" over a set of one, and it is why
    /// `roots_read` against `root_count` is the separate question a window has
    /// to ask as well; [`crate::scan_state::ReadingOutcome::complete`] says so
    /// on the field itself.
    ///
    /// 🔴 **Two conditions per folder, not one.** A folder counts towards
    /// `complete` only if it was read whole AND it ended `Completed`, because
    /// those are two different failures with one consequence. `RootOutcome::
    /// complete` is about phase 1 — what the walk SAW — and the ending is what
    /// says whether phase 3 ran. `VolumeMissing` is the case that needs both:
    /// the walk saw the whole folder (`walk.rs` returns `walked.complete`,
    /// which is `true`), and then stopped before reconciling, so rows for files
    /// that are no longer there stay in the index and stay searchable.
    /// `complete: root.complete` alone would call that pass complete.
    fn absorb(&mut self, root: RootOutcome) {
        self.roots_read += 1;
        self.complete &= root.complete && root.reason == EndReason::Completed;
        self.done += root.done;
        self.total += root.total;
        self.indexed += root.indexed;
        self.unchanged += root.unchanged;
        self.skipped += root.skipped;
        self.removed += root.removed;
        self.contended += root.contended;
        self.roots.push(root);
    }
}

/// The throttle and the contention counter behind one folder's progress
/// callback.
///
/// A type rather than three locals inside the closure, because the rule it
/// carries is exactly the one a closure hides: what happens BEFORE the throttle
/// and what happens after. A test can hand this a sequence of callbacks and ask
/// what survived; it cannot ask that of a closure inside a spawned thread.
pub(crate) struct RootProgress {
    started: Instant,
    last_report: Option<Instant>,
}

impl RootProgress {
    pub(crate) fn new(started: Instant) -> Self {
        Self {
            started,
            last_report: None,
        }
    }

    /// Records what this callback said, and answers with the counts to publish
    /// — or `None` when the throttle drops this report.
    ///
    /// 🔴 **`contended` is stored before the `progress_is_due` check and
    /// everything else after it.** `walk_root` calls this once per file, and
    /// `REPORT_INTERVAL`'s own doc comment has what publishing all of them
    /// costs; but contention is announced ONCE, in the callback for the file
    /// whose last busy retry was refused, and that callback is as likely to be
    /// dropped as any other. Stored after the check, the number would be
    /// whatever the last *published* report happened to carry — nothing, for
    /// any folder read inside one report interval. It cannot be recovered from
    /// the report either: `WalkReport` has no such field.
    pub(crate) fn observe(
        &mut self,
        progress: &WalkProgress,
        now: Instant,
        contended_seen: &AtomicU64,
    ) -> Option<Progress> {
        contended_seen.store(progress.contended, Ordering::Relaxed);

        // `0` refused, for the reason `walk_job::ended_from_report`'s own
        // construction of `Ended` still gives: a walk gives no file up for
        // good, and `WalkProgress::refused` is merged into `skipped` below
        // rather than carried as the different fact `Progress::refused`
        // names.
        if !job::progress_is_due(
            self.last_report,
            now,
            job::REPORT_INTERVAL,
            progress.done,
            0,
            progress.total,
        ) {
            return None;
        }
        self.last_report = Some(now);

        Some(Progress {
            done: progress.done,
            total: progress.total,
            // The same merge `walk_job::ended_from_report` still makes: the
            // bar draws one number, and the itemised difference is what the
            // skip journal is for.
            skipped: progress.skipped + progress.refused,
            refused: 0,
            contended: progress.contended,
            seconds_left: job::seconds_left(
                progress.done,
                progress.total,
                now.duration_since(self.started),
            ),
        })
    }
}

/// What a test installs to be called from inside [`read_roots`], between the
/// index read and the walk that acts on it. It is handed the state, so that a
/// hook can issue the command it is modelling through the same slot everything
/// else goes through.
#[cfg(test)]
type Hook = std::sync::Arc<dyn Fn(&AppState) + Send + Sync>;

#[cfg(test)]
static TEST_HOOK: std::sync::Mutex<Option<Hook>> = std::sync::Mutex::new(None);

#[cfg(test)]
fn set_test_hook(hook: Option<Hook>) {
    *TEST_HOOK.lock().unwrap_or_else(|e| e.into_inner()) = hook;
}

/// Cloned out of its mutex before it is called, so the hook may take any lock
/// it likes — including the index's — without meeting this one.
#[cfg(test)]
fn test_hook(state: &AppState) {
    let hook = TEST_HOOK.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(hook) = hook {
        hook(state);
    }
}

#[cfg(not(test))]
#[inline]
fn test_hook(_state: &AppState) {}

/// What a test installs to be called at the phase boundary — after the last
/// folder's report has been absorbed and before the pass asks whether somebody
/// pressed Stop.
///
/// It is handed nothing: the only thing a test can usefully do in that window is
/// raise the cancellation flag, and the flag it must raise is the one the job
/// already holds, reached through the `AppState` the test itself built. A hook
/// taking the state would suggest there is something else to reach.
///
/// A second hook rather than a parameter on [`TEST_HOOK`], because the two model
/// different races and a test that wanted one would have to no-op the other.
/// They share one turn all the same — see [`SCAN_TURN`], which is held by every
/// test here that starts a scan and not only by the ones that arm a hook.
#[cfg(test)]
type BoundaryHook = std::sync::Arc<dyn Fn() + Send + Sync>;

#[cfg(test)]
static BOUNDARY_HOOK: std::sync::Mutex<Option<BoundaryHook>> = std::sync::Mutex::new(None);

#[cfg(test)]
fn set_boundary_hook(hook: Option<BoundaryHook>) {
    *BOUNDARY_HOOK.lock().unwrap_or_else(|e| e.into_inner()) = hook;
}

/// 🔴 **The turn belongs to the SCAN, not to the hook.** Every test in this
/// module that starts a scan takes it, whether or not it installs anything.
///
/// [`TEST_HOOK`] and [`BOUNDARY_HOOK`] are one per binary, and **every** scan
/// calls both — not only the scan of the test that armed them. So an armed hook
/// fires inside a sibling test's job, on that job's thread, and acts on the
/// state IT was armed for. Measured: with the turn guarding only the installers,
/// `a_stop_after_the_last_root_report_still_ends_cancelled_with_resume_full`
/// failed 7 times in 30 parallel runs of this module, reporting a folder that
/// ended `Cancelled` with `done: 0, total: 2` — its Stop raised by another
/// test's scan while its own walk had not read a file yet, which the boundary
/// hook cannot do from where it is called.
///
/// Guarding the installers alone closes only the window where two hooks
/// overlap, and leaves the wider one where an armed hook overlaps an ordinary
/// scan — nineteen of the twenty-two tests here. A turn that is a **parameter**
/// of [`run_scan`] is what makes the rule mechanical rather than remembered:
/// there is no way to start a scan in this module without one in hand.
///
/// `prefs.rs`'s own `HOOK_TURN` is the shape this copies, not the instance —
/// they guard different hooks and must not serialise against each other.
#[cfg(test)]
static SCAN_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
#[must_use = "a scan started without holding the turn can be reached by another test's hook"]
#[allow(dead_code)] // held for its `Drop`, the guard itself is never read
struct ScanTurn(std::sync::MutexGuard<'static, ()>);

#[cfg(test)]
impl Drop for ScanTurn {
    /// Both hooks, unconditionally: idempotent, and it covers the panic path of
    /// a test that armed one.
    fn drop(&mut self) {
        set_test_hook(None);
        set_boundary_hook(None);
    }
}

/// The turn, with nothing armed — what a test that only runs a scan needs.
///
/// Poisoning is absorbed: a test that panicked must not also poison the next
/// one's turn.
#[cfg(test)]
fn take_scan_turn() -> ScanTurn {
    let turn = SCAN_TURN.lock().unwrap_or_else(|e| e.into_inner());
    set_test_hook(None);
    set_boundary_hook(None);
    ScanTurn(turn)
}

/// The turn, with [`TEST_HOOK`] armed. Armed **after** the lock is held, which
/// is the half of the ordering that matters: armed before it, the hook is
/// reachable by whatever scan is running now.
#[cfg(test)]
fn take_scan_turn_reading(hook: Hook) -> ScanTurn {
    let turn = take_scan_turn();
    set_test_hook(Some(hook));
    turn
}

/// The turn, with [`BOUNDARY_HOOK`] armed.
#[cfg(test)]
fn take_scan_turn_boundary(hook: BoundaryHook) -> ScanTurn {
    let turn = take_scan_turn();
    set_boundary_hook(Some(hook));
    turn
}

/// Cloned out of its mutex before it is called, for [`test_hook`]'s reason.
#[cfg(test)]
fn boundary_hook() {
    let hook = BOUNDARY_HOOK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(not(test))]
#[inline]
fn boundary_hook() {}

#[cfg(test)]
mod tests {
    use super::*;
    use mnema_ingest::{StopReason, WalkReport};
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    /// The extraction worker binary, derived from this test binary's own
    /// location.
    ///
    /// `tests/support/mod.rs::worker` is where this comes from, and the two are
    /// deliberately the same derivation: a unit test here and an integration
    /// test there run from the same `<target>/<profile>/deps` directory, and a
    /// second, different resolver drifting from the first is the shape that
    /// makes one of them silently walk with a worker from somebody else's
    /// build. It is copied rather than shared because `tests/` is not reachable
    /// from `src/`.
    fn worker_path() -> PathBuf {
        // 🔴 Built ONCE per test binary. Every test below that walks a folder
        // wants this path, `cargo test` runs them on several threads at once,
        // and two `cargo build`s of the same binary relink it underneath each
        // other: measured here as a pool that could not start a worker whose
        // path exists — "No such file or directory" for a file `cargo` had just
        // reported building. The lock is what makes the build one event rather
        // than one per test.
        static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        BUILT.get_or_init(build_the_worker).clone()
    }

    fn build_the_worker() -> PathBuf {
        let exe = std::env::current_exe().expect("a test binary knows its own path");
        let profile_dir = exe
            .parent()
            .and_then(Path::parent)
            .expect("a test binary sits in <target>/<profile>/deps");
        let target_dir = profile_dir
            .parent()
            .expect("<target>/<profile> sits inside <target>");
        let profile = profile_dir
            .file_name()
            .and_then(|name| name.to_str())
            .expect("the profile directory is named");
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(1)
            .expect("src-tauri sits one level below the workspace root");

        let mut cargo = std::process::Command::new(env!("CARGO"));
        cargo
            .args([
                "build",
                "-p",
                "mnema-extract",
                "--bin",
                "mnema-extract-worker",
            ])
            .arg("--manifest-path")
            .arg(workspace.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(target_dir);
        if profile != "debug" {
            cargo.args(["--profile", profile]);
        }
        assert!(
            cargo.status().expect("cargo runs").success(),
            "the extraction worker did not build, so this test is unanswered rather \
             than passing"
        );

        let path = profile_dir.join(format!(
            "mnema-extract-worker{}",
            std::env::consts::EXE_SUFFIX
        ));
        assert!(
            path.exists(),
            "cargo reported success but {} is not there",
            path.display()
        );
        path
    }

    /// An `AppState` over a temporary data directory, with its index open.
    fn state_in(data_dir: &Path) -> AppState {
        let state = AppState::new(
            data_dir.to_path_buf(),
            worker_path(),
            // Nothing here calls the provider, and an address that refuses
            // instantly is how a test that starts to finds out at once.
            "http://127.0.0.1:1".to_string(),
            format!("mnema-desktop-scan-job-test-{}", data_dir.display()),
        );
        state.open_index().expect("the index would not open");
        state
    }

    /// Blocks until the job slot is free, or fails saying it never was.
    fn wait_for_the_slot(state: &AppState) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while state.job_is_running() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!state.job_is_running(), "the scan never released the slot");
    }

    fn walk_progress(done: u64, total: u64, skipped: u64, contended: u64) -> WalkProgress {
        WalkProgress {
            done,
            total,
            skipped,
            refused: 0,
            contended,
        }
    }

    // ------------------------------------------------- the embedding phase

    /// The width every fake model here is adopted at. Nothing asks the provider
    /// for it — no test below reaches one — so it is a number that only has to
    /// be legal.
    const A_WIDTH: i64 = 8;

    /// A state whose credential store is this process's own in-memory one.
    ///
    /// `Arc`, because an observer has to reach the state to read what it was
    /// announced about, and the observer is stored INSIDE the state — see
    /// [`run_scan_watching`], which holds a `Weak` for exactly that reason.
    fn app_in(data_dir: &Path) -> Arc<AppState> {
        mnema_secrets::test_store::register();
        Arc::new(state_in(data_dir))
    }

    /// Adds `path` as a watched folder, straight through the index: the command
    /// that does it is `bridge`'s and is not what any of these tests is about.
    fn watch(state: &AppState, path: &Path) -> i64 {
        state
            .with_index(|db| db.insert_watched_root(&path.display().to_string()))
            .expect("adding a watched folder")
    }

    /// A folder holding one indexable file per name, each with text of its own —
    /// distinct, because content addressing makes two files with the same bytes
    /// one document and every count below would then be about the wrong thing.
    fn dir_holding(names: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temp dir for the scan fixture");
        for name in names {
            std::fs::write(
                dir.path().join(name),
                format!("the text of {name}, and nothing else"),
            )
            .expect("writing a scan fixture file");
        }
        dir
    }

    /// Points the index at a model, so that `active_space` answers `Some`.
    ///
    /// Through `Db::adopt_embedding_model` rather than by writing the meta row,
    /// for the reason `tests/support/fixture.rs` gives about its own adoption: a
    /// fixture that writes rows its own way can build a database the product
    /// cannot.
    fn adopt_a_model(state: &AppState) {
        state
            .with_index(|db| db.adopt_embedding_model("a-model", A_WIDTH, "a-ref", "chunker-v1"))
            .expect("adopting a model");
    }

    /// The marker as the index holds it: `Some("1")` while a scan is under way,
    /// `Some("0")` after one that got all the way round, `None` before any scan
    /// has ever run.
    fn marker(state: &AppState) -> Option<String> {
        state
            .with_index(|db| db.meta_get(SCAN_INCOMPLETE))
            .expect("reading the marker")
    }

    /// A key read that answers with a key.
    fn a_key() -> Result<Option<String>, Error> {
        Ok(Some("a-key".to_string()))
    }

    /// A key read that answers "nobody has entered one".
    fn no_key() -> Result<Option<String>, Error> {
        Ok(None)
    }

    /// A credential store that will not answer at all.
    ///
    /// The empty reference is `tests/support/fixture.rs`'s own trick, used here
    /// for the same reason: `mnema_secrets::entry` refuses an empty reference
    /// **before** it installs or consults any store, so this is a store failure
    /// reached in microseconds and touching nothing. It stands in for a locked
    /// keychain and an absent Secret Service session, neither of which a test
    /// can arrange and both of which arrive here the same way — as an `Err` from
    /// `load`, which `?` turns into [`Error::Secrets`].
    fn a_store_that_will_not_answer() -> Result<Option<String>, Error> {
        Ok(mnema_secrets::load("")?)
    }

    /// What a fake embedding pass is, once the two things a test actually
    /// varies are the only parameters left: whether the job has been asked to
    /// stop, and where its reports go. The database, the provider address and
    /// the key belong to the seam rather than to the fake, and
    /// [`deps_counting_embeds`] is what supplies them.
    type FakePass = dyn Fn(
            &dyn Fn() -> bool,
            &mut dyn FnMut(mnema_embed::EmbedProgress),
        ) -> Result<mnema_embed::EmbedTally, mnema_embed::Error>
        + Send
        + Sync;

    /// A `ScanDeps` over this test's own store and provider, and the counter
    /// that says how many times the embedding pass was ENTERED.
    ///
    /// 🔴 **The counter is the assertion, not the absence of an HTTP request**
    /// (D-n). `mnema_embed::run` marks the space `Building` and only *then* asks
    /// `cancel`, so a run that was entered and stopped itself makes no request
    /// and is indistinguishable, from the provider's side, from a run that was
    /// never entered at all. A test that asserted only "nothing was sent" would
    /// stay green with every shell-side guard removed.
    fn deps_counting_embeds(
        key: impl Fn() -> Result<Option<String>, Error> + Send + Sync + 'static,
        answer: impl Fn(
            &dyn Fn() -> bool,
            &mut dyn FnMut(mnema_embed::EmbedProgress),
        ) -> Result<mnema_embed::EmbedTally, mnema_embed::Error>
        + Send
        + Sync
        + 'static,
    ) -> (ScanDeps, Arc<AtomicU64>) {
        let calls = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&calls);
        let deps = ScanDeps {
            key: Arc::new(key),
            embed: Arc::new(move |_db, _base, _key, cancel, on_progress| {
                counted.fetch_add(1, Ordering::SeqCst);
                answer(cancel, on_progress)
            }),
        };
        (deps, calls)
    }

    /// An embedding pass that reported one batch and embedded `done` of them.
    ///
    /// It reports before it answers, which is what the real one does and what
    /// makes the counted `Embedding` announcement below something the phase had
    /// to publish rather than something it could have invented from the tally.
    fn a_pass_that_embeds(done: u64) -> Box<FakePass> {
        Box::new(move |_cancel, on_progress| {
            on_progress(mnema_embed::EmbedProgress {
                done,
                total: done,
                failed: 0,
            });
            Ok(mnema_embed::EmbedTally {
                embedded: done,
                failed: 0,
            })
        })
    }

    /// An embedding pass nothing should ever reach. Fails the test from inside
    /// the thread rather than answering, so a guard that let it through is
    /// reported at the moment it was crossed.
    fn a_pass_that_must_not_run() -> Box<FakePass> {
        Box::new(|_cancel, _on_progress| panic!("the embedding pass was entered"))
    }

    /// Runs one whole scan and answers with what an observer FOUND each time the
    /// job slot changed hands, followed by the state it settled in.
    ///
    /// The same shape `tests/commands.rs::run_scan_capturing_snapshots` uses,
    /// and for the same reason: a scan has no channel, so recording what was
    /// READ at each announcement is recording exactly what a tray or a reopened
    /// window would have drawn.
    ///
    /// 🔴 The `turn` is a parameter and is never taken inside, so that no test
    /// in this module can start a scan without holding it — see [`SCAN_TURN`]
    /// for the race that reaches a scan running without one, and for how it was
    /// measured. It is borrowed rather than taken because a test that arms a
    /// hook takes the same turn to arm it, and one lock cannot be taken twice.
    fn run_scan(
        turn: &ScanTurn,
        state: &Arc<AppState>,
        entry: Entry,
        deps: ScanDeps,
    ) -> (
        Vec<crate::scan_state::ScanState>,
        crate::scan_state::ScanState,
    ) {
        run_scan_watching(turn, state, entry, deps, |_, _| {})
    }

    /// [`run_scan`] with a hand at the announcement: `watcher` runs on whichever
    /// thread announced, which for a reading report is the job's own thread
    /// inside `walk_root`. That is what makes the interleavings below built
    /// rather than waited for.
    fn run_scan_watching(
        turn: &ScanTurn,
        state: &Arc<AppState>,
        entry: Entry,
        deps: ScanDeps,
        watcher: impl Fn(&AppState, &crate::scan_state::ScanState) + Send + Sync + 'static,
    ) -> (
        Vec<crate::scan_state::ScanState>,
        crate::scan_state::ScanState,
    ) {
        let seen: Arc<std::sync::Mutex<Vec<crate::scan_state::ScanState>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        // `Weak`, not `Arc`: the observer is stored on the state, and an `Arc`
        // back to it would be a cycle that outlives the test.
        let weak = Arc::downgrade(state);
        state.set_job_observer(Box::new(move || {
            let Some(state) = weak.upgrade() else { return };
            let now = state.scan_state();
            recorder
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(now.clone());
            watcher(&state, &now);
        }));

        // Read, so that the borrow is a real one: the turn is proof the caller
        // holds it, and a parameter nothing touches is a parameter a future
        // edit deletes. Borrowed and never moved — dropping it here would
        // release the lock this very scan is running under.
        let _held: &std::sync::MutexGuard<'static, ()> = &turn.0;
        start_inner(state, entry, deps).expect("the scan would not start");
        wait_for_the_slot(state);
        let settled = state.scan_state();
        let snapshots = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
        (snapshots, settled)
    }

    /// The report a finished scan settled on, or a failure naming what was found
    /// instead.
    fn report_of(settled: &crate::scan_state::ScanState) -> ScanReport {
        match &settled.snapshot {
            crate::scan_state::ScanSnapshot::Ended { report } => report.clone(),
            other => panic!("the scan did not end with a report: {other:?}"),
        }
    }

    /// Every phase a surface was shown while the job ran, in order — the log the
    /// ordering assertions below are made against.
    fn phases(snapshots: &[crate::scan_state::ScanState]) -> Vec<Phase> {
        snapshots
            .iter()
            .filter_map(|state| match &state.snapshot {
                crate::scan_state::ScanSnapshot::Running { phase, .. } => Some(phase.clone()),
                _ => None,
            })
            .collect()
    }

    /// Whether a phase is the empty `Embedding` announcement D-g puts in front
    /// of the key read.
    fn is_embedding_with_nothing_counted(phase: &Phase) -> bool {
        matches!(phase, Phase::Embedding { counts } if *counts == Progress::default())
    }

    /// 🔴 The contention count survives the throttle that drops the report
    /// carrying it.
    ///
    /// The pair of states this separates is "one file found the index locked"
    /// from "no file did", on a folder read inside a single report interval —
    /// which is every small folder. Contention is announced ONCE, in the
    /// callback for the file whose last busy retry was refused, and that
    /// callback is as droppable as any other; `WalkReport` has no such field to
    /// recover it from. An implementation that stored the count after the
    /// `progress_is_due` check reports zero here and cannot be told from a
    /// folder nothing held a lock on.
    ///
    /// The mirror is in the same test: the same sequence with nothing contended
    /// must report nothing, so an implementation that hardcoded a number to
    /// satisfy the first half fails the second.
    #[test]
    fn a_contended_report_the_throttle_drops_is_still_counted() {
        let started = Instant::now();
        let mut ticker = RootProgress::new(started);
        let seen = AtomicU64::new(0);

        // The report `walk_root` makes before its phase-2 loop: nothing has
        // been published yet, so this one goes out.
        assert!(
            ticker
                .observe(&walk_progress(0, 3, 0, 0), started, &seen)
                .is_some(),
            "the first report of a folder is never throttled"
        );

        // The contended file's own callback, a millisecond later — far inside
        // `REPORT_INTERVAL`, so the throttle drops it.
        let a_moment_later = started + Duration::from_millis(1);
        assert!(
            ticker
                .observe(&walk_progress(1, 3, 0, 1), a_moment_later, &seen)
                .is_none(),
            "this report must be throttled away, or the test is not about the \
             throttle at all"
        );
        assert_eq!(
            seen.load(Ordering::Relaxed),
            1,
            "the only report that ever names contention was dropped, and the \
             count with it"
        );

        // The folder's ending, built from the count the throttle could not
        // reach. `skipped` is 1 because the contended file is journalled as a
        // skip immediately after it is counted.
        let ended = ended_from_report(
            &WalkReport {
                found: 3,
                indexed: 2,
                unchanged: 0,
                skipped: 1,
                refused: 0,
                removed: 0,
                frozen: Vec::new(),
                complete: true,
                stopped: StopReason::Completed,
            },
            seen.load(Ordering::Relaxed),
        );
        let root = root_outcome("/somewhere".to_string(), &ended);
        assert_eq!(root.contended, 1, "{root:?}");
        assert!(
            ended.contended <= ended.skipped,
            "contended {} is above skipped {}, so a surface adding the two would \
             count one file twice",
            ended.contended,
            ended.skipped
        );

        // The other direction, on the same code path: nothing held a lock.
        let quiet = AtomicU64::new(0);
        let mut quiet_ticker = RootProgress::new(started);
        assert!(
            quiet_ticker
                .observe(&walk_progress(0, 3, 0, 0), started, &quiet)
                .is_some()
        );
        assert!(
            quiet_ticker
                .observe(&walk_progress(1, 3, 0, 0), a_moment_later, &quiet)
                .is_none()
        );
        assert_eq!(
            quiet.load(Ordering::Relaxed),
            0,
            "a folder nothing held a lock on must report no contention"
        );
    }

    /// Which endings are about the folder and which are about the run.
    ///
    /// The pair it separates is "one ejected disk costs one folder" from "one
    /// ejected disk costs the whole scan", and its mirror: a broken install
    /// must NOT be carried into the next folder to be discovered again. Both
    /// lists are stated positively and in full, so a variant that changed sides
    /// fails rather than falling through a catch-all.
    #[test]
    fn only_the_endings_that_are_about_the_run_stop_the_pass() {
        for reason in [
            EndReason::Completed,
            EndReason::RootUnavailable,
            EndReason::VolumeMissing,
        ] {
            assert_eq!(
                after_root(reason),
                None,
                "{reason:?} is a fact about one folder and must not stop the pass"
            );
        }
        for reason in [
            EndReason::Cancelled,
            EndReason::Failed,
            EndReason::BrokenWorker,
            EndReason::RulesNotApplied,
        ] {
            assert_eq!(
                after_root(reason),
                Some(reason),
                "{reason:?} is a fact about the run and must stop the pass, saying so"
            );
        }
    }

    /// Every row of the resumption table, both which rows have an entry point
    /// and which one each of them names.
    ///
    /// The pair it separates is "the next run picks up where this one stopped"
    /// from "the next run re-reads the archive to find out there was nothing to
    /// read" — which is what an embedding phase resumed as `Full` does. The
    /// `None` rows are asserted just as explicitly: an ending that offers a
    /// resumption which cannot help is a button that spends an hour and changes
    /// nothing.
    #[test]
    fn every_ending_names_what_the_next_scan_should_be_or_that_there_is_nothing() {
        let table = [
            (EndReason::Completed, EndedIn::Reading, None),
            (EndReason::Completed, EndedIn::Embedding, None),
            (EndReason::Cancelled, EndedIn::Reading, Some(Entry::Full)),
            (
                EndReason::Cancelled,
                EndedIn::Embedding,
                Some(Entry::EmbedOnly),
            ),
            (EndReason::Failed, EndedIn::Reading, Some(Entry::Full)),
            (
                EndReason::Failed,
                EndedIn::Embedding,
                Some(Entry::EmbedOnly),
            ),
            (EndReason::BrokenWorker, EndedIn::Reading, Some(Entry::Full)),
            (
                EndReason::BrokenWorker,
                EndedIn::Embedding,
                Some(Entry::EmbedOnly),
            ),
            (EndReason::RulesNotApplied, EndedIn::Reading, None),
            (EndReason::RulesNotApplied, EndedIn::Embedding, None),
            (EndReason::RootUnavailable, EndedIn::Reading, None),
            (EndReason::RootUnavailable, EndedIn::Embedding, None),
            (EndReason::VolumeMissing, EndedIn::Reading, None),
            (EndReason::VolumeMissing, EndedIn::Embedding, None),
        ];
        for (reason, ended_in, expected) in table {
            assert_eq!(
                resume_for(reason, ended_in),
                expected,
                "{reason:?} in the {ended_in:?} phase should resume as {expected:?}"
            );
        }
    }

    /// The pass sums the folders it READ, and is complete only if every one of
    /// them was.
    ///
    /// 🔴 Three pairs, all on the same counters, because there are three ways a
    /// pass can be incomplete and only one of them is about what the walk SAW.
    ///
    /// - two folders read whole and reconciled → complete;
    /// - one of them only partly seen (`RootOutcome::complete` false, the
    ///   unreadable subtree) → not complete;
    /// - one of them seen whole and never reconciled (`complete` TRUE, ending
    ///   `VolumeMissing`) → not complete, and this is the pair an aggregation
    ///   written as `self.complete &= root.complete` gets wrong: the folder
    ///   really was read to the end, phase 3 simply never ran, and the rows for
    ///   files that are gone stay searchable.
    ///
    /// `roots_read` is asserted beside them because a pass that counted the
    /// folders it was GIVEN rather than the ones that answered reports the same
    /// number here and a wrong one the moment a pass stops early.
    #[test]
    fn a_pass_is_complete_only_when_every_folder_it_read_was_whole_and_reconciled() {
        let folder = |complete: bool, reason: EndReason| RootOutcome {
            root_path: "/somewhere".to_string(),
            reason,
            complete,
            message: None,
            done: 2,
            total: 2,
            indexed: 1,
            unchanged: 1,
            skipped: 0,
            removed: 3,
            contended: 1,
            frozen: Vec::new(),
        };
        let pass_over = |roots: [RootOutcome; 2]| {
            let mut outcome = ReadingOutcome {
                root_count: 2,
                ..ReadingOutcome::default()
            };
            for root in roots {
                outcome.absorb(root);
            }
            outcome
        };

        let both_seen = pass_over([
            folder(true, EndReason::Completed),
            folder(true, EndReason::Completed),
        ]);
        assert!(both_seen.complete, "{both_seen:?}");

        let one_partly = pass_over([
            folder(false, EndReason::Completed),
            folder(true, EndReason::Completed),
        ]);
        assert!(
            !one_partly.complete,
            "one folder that was not fully seen must make the pass not fully \
             seen: {one_partly:?}"
        );

        let one_unreconciled = pass_over([
            folder(true, EndReason::VolumeMissing),
            folder(true, EndReason::Completed),
        ]);
        assert!(
            !one_unreconciled.complete,
            "a folder the walk saw whole and never reconciled leaves rows for \
             files that are gone, and the pass claims it is complete: \
             {one_unreconciled:?}"
        );
        let one_absent = pass_over([
            folder(false, EndReason::RootUnavailable),
            folder(true, EndReason::Completed),
        ]);
        assert!(!one_absent.complete, "{one_absent:?}");

        // Identical in every counter, which is what makes these pairs rather
        // than four different passes.
        for other in [&one_partly, &one_unreconciled, &one_absent] {
            assert_eq!(both_seen.done, other.done);
            assert_eq!(both_seen.indexed, other.indexed);
            assert_eq!(both_seen.removed, other.removed);
            assert_eq!(both_seen.contended, other.contended);
            assert_eq!(both_seen.roots_read, other.roots_read);
        }
        assert_eq!(both_seen.roots_read, 2);
        assert_eq!(both_seen.done, 4, "the counters are sums, not the last row");
    }

    /// 🔴 **A folder swapped between the read and the walk is not walked under
    /// its successor's id.**
    ///
    /// The race this kills: the scan reads the list of watched folders, and
    /// between that read and the walk that acts on it, folder A is removed and
    /// folder B is added. SQLite hands the new row the id the old one gave up,
    /// so the walk — still holding A's path and A's id — writes A's files into
    /// B's `path` rows. Every citation into them then names a file that is not
    /// under that folder, and the scan reports `completed`.
    ///
    /// What closes it is the ORDER of two lines in `start_inner`: `claim_job`
    /// before `read_roots`. With the slot taken, the removal is refused with
    /// `Error::JobAlreadyRunning` and there is no swap to race.
    ///
    /// The hook fires inside `read_roots`, which is the moment between the read
    /// and the walk (D-l), and it calls `bridge::remove_watched_root` itself —
    /// Task 4's real removal, not a model of it. Against correct code the scan
    /// is already holding the slot by the time `read_roots` runs, so this call
    /// is refused with `Error::JobAlreadyRunning`, `removed` stays `false`, and
    /// there is no swap. Moving `read_roots` above `claim_job` (the mutant
    /// below) is what lets this same call succeed: the slot is still free when
    /// the hook fires, so the removal goes through, folder B is inserted under
    /// A's old id, and the walk — already holding `roots` from the read that
    /// preceded the swap — writes A's files into what is now B's row.
    ///
    /// The invariant asserted is about the index and not about the ordering:
    /// every `path` row under root 1 names a file that exists under root 1's
    /// CURRENT directory. It holds for correct code and fails for the mutant
    /// that moves `read_roots` above `claim_job`, with no test code changed.
    #[test]
    fn a_root_swapped_between_the_read_and_the_walk_is_not_walked_under_its_successors_id() {
        let data = tempfile::tempdir().expect("a data directory");
        let folder_a = tempfile::tempdir().expect("folder A");
        let folder_b = tempfile::tempdir().expect("folder B");
        std::fs::write(folder_a.path().join("A.txt"), "the text that is under A").unwrap();
        std::fs::write(folder_b.path().join("B.txt"), "the text that is under B").unwrap();

        let state = state_in(data.path());
        let id = state
            .with_index(|db| db.insert_watched_root(&folder_a.path().display().to_string()))
            .expect("adding folder A");
        assert_eq!(
            id, 1,
            "this test is about the id a removed folder gives up, so folder A \
             has to be holding it"
        );

        let fired = Arc::new(AtomicBool::new(false));
        let removed = Arc::new(AtomicBool::new(false));
        let a_path = folder_a.path().to_path_buf();
        let b_path = folder_b.path().to_path_buf();
        // This hook calls the real `bridge::remove_watched_root`, which is
        // guarded by its OWN hook and its OWN turn — a global this binary's
        // tests share the same way they share `TEST_HOOK`. Taking it here,
        // with nothing to install, is what `take_remove_hook_turn`'s own doc
        // says every caller of that function must do.
        let _remove_turn = crate::bridge::take_remove_hook_turn(Arc::new(|_: &AppState| {}));
        let _turn = take_scan_turn_reading(Arc::new({
            let fired = Arc::clone(&fired);
            let removed = Arc::clone(&removed);
            move |state: &AppState| {
                fired.store(true, Ordering::SeqCst);

                // Task 4's real removal command, called exactly as
                // `bridge::remove_watched_folder` calls it — see this test's
                // own doc comment for what each ordering of `start_inner`
                // makes of this call.
                let deleted =
                    crate::bridge::remove_watched_root(state, 1, &a_path.display().to_string());
                if deleted.is_ok() {
                    removed.store(true, Ordering::SeqCst);
                    let _ = state
                        .with_index(|db| db.insert_watched_root(&b_path.display().to_string()));
                }
            }
        }));

        start_inner(&state, Entry::Full, ScanDeps::production(&state))
            .expect("the scan would not start");
        wait_for_the_slot(&state);

        assert!(
            fired.load(Ordering::SeqCst),
            "the hook never ran, so this test asserted nothing about the race \
             it exists for"
        );
        let (root_path, rows) = state
            .with_index(|db| Ok((db.watched_root_path(1)?, db.paths_under_root(1)?)))
            .expect("reading root 1 back");
        let current = PathBuf::from(root_path.expect("root 1 no longer exists at all"));
        assert!(
            !rows.is_empty(),
            "the scan wrote nothing under root 1, so the invariant below holds \
             by being about nothing"
        );
        for row in &rows {
            assert!(
                current.join(row).exists(),
                "the index holds {row:?} under root 1, and there is no such file \
                 under {} — the folder was walked under another folder's id",
                current.display()
            );
        }

        // Last, and deliberately so: the invariant above is what this test is
        // for, and an ordering assertion placed in front of it would be the one
        // that killed the mutant — leaving the invariant asserted by a line
        // that never had to hold. This one says WHY the invariant held.
        assert!(
            !removed.load(Ordering::SeqCst),
            "the removal went through while a scan held the job slot"
        );
    }

    /// 🔴 The whole phase, end to end: a reading pass that finished, a key, a
    /// model, and a pass that embedded something.
    ///
    /// The pair it separates is "the scan read the folders and then embedded
    /// what they queued" from "the scan read the folders and stopped", which is
    /// what this job did before this commit and which a window cannot tell from
    /// the first by looking at the index alone — an archive with no vectors and
    /// an archive nobody has embedded yet hold the same rows.
    ///
    /// The announcements are asserted as a SEQUENCE and not as a set, because
    /// the empty `Embedding` in front of the counted one is D-g's first step: it
    /// is what a surface draws while the credential store is putting a dialog on
    /// screen, and a phase announced after the key read would leave a folder
    /// name and a reading bar on screen for the whole of that wait.
    ///
    /// `last_reading` is asserted after the embedding for the same reason it is
    /// a separate field at all (D-e): the embedding phase writes a report and
    /// must not touch what the folders said.
    #[test]
    fn a_scan_that_read_its_folders_goes_on_to_embed_what_they_queued() {
        let turn = take_scan_turn();
        let data = tempfile::tempdir().expect("a data directory");
        let folder = dir_holding(&["a1.txt", "a2.txt"]);
        let state = app_in(data.path());
        watch(&state, folder.path());
        adopt_a_model(&state);

        let (deps, calls) = deps_counting_embeds(a_key, a_pass_that_embeds(3));
        let (snapshots, settled) = run_scan(&turn, &state, Entry::Full, deps);

        let report = report_of(&settled);
        assert_eq!(
            report.embedding,
            EmbedOutcome::Ran {
                done: 3,
                total: 3,
                refused: 0
            },
            "{report:?}"
        );
        assert_eq!(report.ended_in, EndedIn::Embedding, "{report:?}");
        assert_eq!(report.reason, EndReason::Completed, "{report:?}");
        assert_eq!(
            report.resume, None,
            "a scan that read everything and embedded everything has nothing \
             left for a next one: {report:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "{report:?}");
        assert_eq!(
            marker(&state).as_deref(),
            Some("0"),
            "the scan visited every folder and the marker still says otherwise"
        );

        let reading = settled
            .last_reading
            .clone()
            .expect("the scan recorded no reading pass");
        assert!(
            reading.complete,
            "the embedding phase overwrote what the folders said: {reading:?}"
        );
        assert_eq!(
            reading.indexed, 2,
            "the reading's own count did not survive the embedding: {reading:?}"
        );

        let phases = phases(&snapshots);
        let last_reading_at = phases
            .iter()
            .rposition(|phase| matches!(phase, Phase::Reading { .. }))
            .unwrap_or_else(|| panic!("no folder was ever announced: {phases:?}"));
        let empty_embedding_at = phases
            .iter()
            .position(is_embedding_with_nothing_counted)
            .unwrap_or_else(|| {
                panic!("the embedding phase was never announced before it counted: {phases:?}")
            });
        let counted_embedding_at = phases
            .iter()
            .position(|phase| matches!(phase, Phase::Embedding { counts } if counts.done == 3))
            .unwrap_or_else(|| {
                panic!("what the pass reported never reached a surface: {phases:?}")
            });
        assert!(
            last_reading_at < empty_embedding_at,
            "the embedding was announced before the reading had finished: {phases:?}"
        );
        assert!(
            empty_embedding_at < counted_embedding_at,
            "the phase was announced only once it had something to count, so a \
             surface drew a reading bar for the whole of the key read: {phases:?}"
        );
    }

    /// 🔴 A store that will not answer is not a store with no key in it.
    ///
    /// The pair, on one fixture and one code path: `Ok(None)` is a person who
    /// has not entered a key, and `Err` is a store that could not be asked —
    /// commonly a locked keychain, which may well be holding the key already.
    /// The first tells them to enter one; the second tells them to unlock
    /// something. Folded together, somebody with a locked keychain is invited to
    /// type in a key they have already entered.
    ///
    /// Both halves end the JOB the same way — `Completed`, no resumption — and
    /// that is deliberate rather than an oversight: neither is changed by
    /// running the same scan again, and both leave the reading exactly as it
    /// was.
    #[test]
    fn a_store_that_will_not_answer_is_reported_as_unavailable_not_as_no_key() {
        let turn = take_scan_turn();
        let data = tempfile::tempdir().expect("a data directory");
        let folder = dir_holding(&["a1.txt"]);
        let state = app_in(data.path());
        watch(&state, folder.path());
        adopt_a_model(&state);

        let (deps, calls) =
            deps_counting_embeds(a_store_that_will_not_answer, a_pass_that_must_not_run());
        let (_, settled) = run_scan(&turn, &state, Entry::Full, deps);

        let report = report_of(&settled);
        let message = match &report.embedding {
            EmbedOutcome::Skipped {
                why: SkipWhy::StoreUnavailable { message },
            } => message.clone(),
            other => panic!(
                "a store that refused to answer was reported as {other:?}, so a person with a \
                 locked keychain is being told to enter a key they may already have"
            ),
        };
        assert!(
            !message.is_empty(),
            "the one skip reason that carries a diagnostic carried an empty one"
        );
        assert_eq!(report.reason, EndReason::Completed, "{report:?}");
        assert_eq!(report.ended_in, EndedIn::Embedding, "{report:?}");
        assert_eq!(report.resume, None, "{report:?}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "the pass ran without a key"
        );
        assert_eq!(marker(&state).as_deref(), Some("0"), "{report:?}");
        assert!(
            settled
                .last_reading
                .as_ref()
                .is_some_and(|reading| reading.complete && reading.roots_read == 1),
            "the reading pass was disturbed by an embedding phase that never \
             ran: {settled:?}"
        );

        // The other half, same fixture, same code path: nobody entered a key.
        let data = tempfile::tempdir().expect("a second data directory");
        let state = app_in(data.path());
        watch(&state, folder.path());
        adopt_a_model(&state);
        let (deps, calls) = deps_counting_embeds(no_key, a_pass_that_must_not_run());
        let (_, settled) = run_scan(&turn, &state, Entry::Full, deps);

        let report = report_of(&settled);
        assert_eq!(
            report.embedding,
            EmbedOutcome::Skipped {
                why: SkipWhy::NoKey
            },
            "an empty store must be reported as an empty store: {report:?}"
        );
        assert_eq!(report.reason, EndReason::Completed, "{report:?}");
        assert_eq!(report.ended_in, EndedIn::Embedding, "{report:?}");
        assert_eq!(report.resume, None, "{report:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(marker(&state).as_deref(), Some("0"));
    }

    /// 🔴 A Stop during the key read wins whatever the store answers.
    ///
    /// The pair it separates is "the person stopped this job" from "the job had
    /// nothing to do anyway", and the three store answers are what make it a
    /// pair rather than one case: two of them — no key, and a store that would
    /// not answer — end `Completed` on their own, so a cancellation check placed
    /// after the classification reports a scan that FINISHED to somebody who
    /// pressed Stop while a keychain dialog was on their screen. Only the third,
    /// where a key really was returned, would fail such an implementation.
    ///
    /// The empty `Embedding` announcement is asserted in the same test, between
    /// the last `Reading` and the ending, because it is the other half of the
    /// same decision: the dialog the person is answering has to be waited for
    /// under the phase it belongs to.
    ///
    /// `embed_calls` and not "no request was made" (D-n): `mnema_embed::run`
    /// marks the space `Building` before it asks `cancel`, so it would answer
    /// this question with silence whether or not the guard above it survived.
    #[test]
    fn a_stop_during_the_key_read_wins_whatever_the_store_answers() {
        let turn = take_scan_turn();
        for (which, answer, model) in [
            (
                "a key",
                a_key as fn() -> Result<Option<String>, Error>,
                true,
            ),
            ("no key", no_key, true),
            (
                "a store that will not answer",
                a_store_that_will_not_answer,
                true,
            ),
            // 🔴 The fourth arm, and the only one where a model is NOT adopted.
            // Without it nothing holds the cancel check ABOVE `active_space()`:
            // the three arms above all have a model, so the check they cross is
            // the last thing between the key and the engine, and a phase that
            // asked "is there a model to embed into?" before putting a keychain
            // dialog on screen — a natural refactor, with a good reason behind
            // it — would pass all three. It would tell a person who has entered
            // no model and pressed Stop at that dialog `Skipped{NoModel}`,
            // `Completed`, `resume: None`: a scan that says it finished, to
            // somebody who stopped it, with no way to carry on.
            ("a key, with no model adopted", a_key, false),
        ] {
            let data = tempfile::tempdir().expect("a data directory");
            let folder = dir_holding(&["a1.txt"]);
            let state = app_in(data.path());
            watch(&state, folder.path());
            if model {
                adopt_a_model(&state);
            }

            // The Stop lands while the store is being asked — the shape of a
            // person pressing Stop with an authorisation dialog on screen. The
            // closure also records the phase the application was in AT THAT
            // MOMENT, which is the only way to ask the question D-g's first
            // step is about: an announcement made after the read still appears
            // in the log, just later, so a test reading only the log cannot
            // tell the two orders apart.
            let under: Arc<std::sync::Mutex<Option<Phase>>> = Arc::new(std::sync::Mutex::new(None));
            let recorder = Arc::clone(&under);
            let stopping = Arc::downgrade(&state);
            let (deps, calls) = deps_counting_embeds(
                move || {
                    if let Some(state) = stopping.upgrade() {
                        if let crate::scan_state::ScanSnapshot::Running { phase, .. } =
                            state.scan_state().snapshot
                        {
                            *recorder.lock().unwrap_or_else(|e| e.into_inner()) = Some(phase);
                        }
                        state.cancel_job();
                    }
                    answer()
                },
                a_pass_that_must_not_run(),
            );
            let (snapshots, settled) = run_scan(&turn, &state, Entry::Full, deps);

            // 🔴 The ending FIRST, and the phase the store was asked under
            // second. The ending is the harm — a scan that tells somebody who
            // pressed Stop that it completed — and an ordering assertion placed
            // in front of it would be the line that killed the mutant, leaving
            // the harm asserted by a line that never had to hold.
            let report = report_of(&settled);
            assert_eq!(
                report.reason,
                EndReason::Cancelled,
                "the store answered {which} and the Stop was forgotten: {report:?}"
            );
            assert_eq!(
                report.ended_in,
                EndedIn::Embedding,
                "the job was stopped in the embedding phase and says otherwise \
                 ({which}): {report:?}"
            );
            assert_eq!(
                report.resume,
                Some(Entry::EmbedOnly),
                "a scan stopped after its folders were read must not make a \
                 person read them again ({which}): {report:?}"
            );
            assert_eq!(
                report.embedding,
                EmbedOutcome::NotReached,
                "({which}): {report:?}"
            );
            assert_eq!(
                calls.load(Ordering::SeqCst),
                0,
                "the pass was entered after a Stop ({which})"
            );

            let under = under.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let under = under.unwrap_or_else(|| {
                panic!(
                    "the credential store was never asked at all, so nothing was \
                     waited for under any phase ({which})"
                )
            });
            assert!(
                is_embedding_with_nothing_counted(&under),
                "the credential store was asked under {under:?}, so a person \
                 answering an authorisation dialog watches a folder name and a \
                 reading bar for the whole of it ({which})"
            );

            let phases = phases(&snapshots);
            let last_reading_at = phases
                .iter()
                .rposition(|phase| matches!(phase, Phase::Reading { .. }))
                .unwrap_or_else(|| panic!("no folder was announced ({which}): {phases:?}"));
            let empty_embedding_at = phases
                .iter()
                .position(is_embedding_with_nothing_counted)
                .unwrap_or_else(|| {
                    panic!(
                        "the store was asked under the reading phase, so a surface drew a \
                         folder name and a progress bar for the whole of the wait ({which}): \
                         {phases:?}"
                    )
                });
            assert!(
                last_reading_at < empty_embedding_at,
                "({which}): {phases:?}"
            );
        }
    }

    /// 🔴 A Stop raised after the last folder's report still ends the scan.
    ///
    /// The window this is about is the one the loop cannot ask about: its
    /// cancellation check is at the TOP of an iteration, so for the last folder
    /// of a scan there is no next iteration in which to notice. Every folder
    /// reports `Completed`, the person presses Stop while the pass is absorbing
    /// the last of them, and without D-h's check the scan carries on into the
    /// embedding phase and sends their text to the provider they had just told
    /// it not to.
    ///
    /// The pair: `Cancelled` with `resume: Full` against `Completed` with the
    /// embedding phase entered. The hook fires in exactly that window, and
    /// `fired` is asserted so a hook that stopped being called cannot leave this
    /// test passing about nothing.
    ///
    /// ⚠️ **Every folder ending `Completed` is a PREMISE here, not a result**,
    /// and it is the line that caught [`SCAN_TURN`]'s race: with the turn
    /// guarding only the hook installers, a sibling test's scan called this
    /// test's hook and raised this test's Stop while its own walk was still in
    /// phase 2, so the folder answered `Cancelled` with `done: 0` and the
    /// premise failed. A folder that was itself stopped proves nothing about
    /// the boundary, which is why the assertion is worth its own message rather
    /// than being folded into the one below it.
    #[test]
    fn a_stop_after_the_last_root_report_still_ends_cancelled_with_resume_full() {
        let data = tempfile::tempdir().expect("a data directory");
        let folder = dir_holding(&["a1.txt", "a2.txt"]);
        let state = app_in(data.path());
        watch(&state, folder.path());
        adopt_a_model(&state);

        let fired = Arc::new(AtomicBool::new(false));
        let stopping = Arc::downgrade(&state);
        let turn = take_scan_turn_boundary(Arc::new({
            let fired = Arc::clone(&fired);
            move || {
                fired.store(true, Ordering::SeqCst);
                if let Some(state) = stopping.upgrade() {
                    state.cancel_job();
                }
            }
        }));

        let (deps, calls) = deps_counting_embeds(a_key, a_pass_that_must_not_run());
        let (_, settled) = run_scan(&turn, &state, Entry::Full, deps);

        assert!(
            fired.load(Ordering::SeqCst),
            "the hook never ran, so this test asserted nothing about the window \
             it exists for"
        );
        let reading = settled
            .last_reading
            .clone()
            .expect("the scan recorded no reading pass");
        assert_eq!(
            reading.reason,
            EndReason::Cancelled,
            "every folder completed and the Stop between the last one and the \
             embedding was lost: {reading:?}"
        );
        assert!(
            reading
                .roots
                .iter()
                .all(|root| root.reason == EndReason::Completed),
            "this test is about a pass whose folders all completed; they did \
             not, so it is about something else: {reading:?}"
        );

        let report = report_of(&settled);
        assert_eq!(report.reason, EndReason::Cancelled, "{report:?}");
        assert_eq!(report.ended_in, EndedIn::Reading, "{report:?}");
        assert_eq!(report.embedding, EmbedOutcome::NotReached, "{report:?}");
        assert_eq!(
            report.resume,
            Some(Entry::Full),
            "a scan stopped before the reading was declared done owes a whole \
             scan next, not a resumption that reads nothing: {report:?}"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "the embedding phase ran after a Stop"
        );
        assert_eq!(
            marker(&state).as_deref(),
            Some("1"),
            "a scan that was stopped left the index looking finished"
        );
    }

    /// A reading pass that was stopped never enters the embedding phase.
    ///
    /// The pair it separates is "the scan stopped when it was asked to" from
    /// "the scan stopped reading and sent the person's text to the provider
    /// anyway" — which is the whole reason this job exists (D29 makes an
    /// indexed file one whose text goes to a third party), and which no
    /// assertion about the reading alone can see.
    ///
    /// ⚠️ **It is not a test of D-h**, though the Stop lands in the same
    /// instant: measured by removing D-h's check, this test stays green,
    /// because `walk_root` reads the flag itself while it is still running and
    /// the folder answers `Cancelled` on its own. What it does guard is the
    /// early return after `mark_reading_done` — remove that and the embedding
    /// phase runs over a pass nobody finished. `a_stop_after_the_last_root_
    /// report_still_ends_cancelled_with_resume_full` is the one that holds
    /// D-h, and it needs the hook precisely because a real Stop this late is
    /// usually caught one layer down.
    #[test]
    fn a_stop_raised_in_the_last_progress_event_ends_cancelled_and_never_embeds() {
        let turn = take_scan_turn();
        let data = tempfile::tempdir().expect("a data directory");
        let folder = dir_holding(&["a1.txt", "a2.txt", "a3.txt"]);
        let state = app_in(data.path());
        watch(&state, folder.path());
        adopt_a_model(&state);

        let (deps, calls) = deps_counting_embeds(a_key, a_pass_that_must_not_run());
        let stopping = Arc::downgrade(&state);
        let (_, settled) = run_scan_watching(&turn, &state, Entry::Full, deps, move |_, now| {
            if let crate::scan_state::ScanSnapshot::Running {
                phase: Phase::Reading { counts, .. },
                ..
            } = &now.snapshot
                && counts.total >= 3
                && counts.done == counts.total
                && let Some(state) = stopping.upgrade()
            {
                state.cancel_job();
            }
        });

        let report = report_of(&settled);
        assert_eq!(report.reason, EndReason::Cancelled, "{report:?}");
        assert_eq!(
            report.embedding,
            EmbedOutcome::NotReached,
            "a stopped reading pass must not report an embedding phase it never \
             entered: {report:?}"
        );
        // 🔴 The two fields the early return decides, and the reason they are
        // here rather than left implicit. A pass that fell through into the
        // embedding phase would be caught by ITS cancel check and would still
        // report `cancelled` and `notReached` — measured, by removing the
        // return — and would name the wrong phase and offer the wrong
        // resumption: `embedOnly`, which reads no folder, over an archive whose
        // folders were never finished.
        assert_eq!(
            report.ended_in,
            EndedIn::Reading,
            "a pass stopped while reading folders says it stopped somewhere \
             else: {report:?}"
        );
        assert_eq!(
            report.resume,
            Some(Entry::Full),
            "a scan stopped with folders left to read must offer to read them, \
             not to embed what it never got: {report:?}"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a Stop landing on the last report was overtaken by the embedding \
             phase"
        );
    }

    /// The marker answers "were the folders visited", and nothing else.
    ///
    /// The pair it separates is "a scan got all the way round" from "a scan
    /// stopped half-way and left an index nothing finished writing" — which is
    /// what a crash or a power cut leaves, since the snapshot is a process's own
    /// memory and starts empty. Both folders are visited here and there is no
    /// key, so the embedding phase is skipped: the marker must still clear,
    /// because it is not about the embedding.
    #[test]
    fn a_reading_phase_that_visited_every_root_clears_the_marker_even_without_a_key() {
        let turn = take_scan_turn();
        let data = tempfile::tempdir().expect("a data directory");
        let first = dir_holding(&["a1.txt"]);
        let second = dir_holding(&["b1.txt"]);
        let state = app_in(data.path());
        watch(&state, first.path());
        watch(&state, second.path());

        // 🔴 What the marker said AT the announcement that told everyone the
        // reading had ended. The marker is written to the index and the pass's
        // conclusions to the snapshot, and the two are read by the same
        // observer in the same instant: an observer woken by `read_seq` moving
        // that then found the marker still set would be reading a half-done
        // scan beside a `last_reading` saying it finished. The write order is
        // the only thing that closes that, and nothing else can see it.
        let at_the_announcement: Arc<std::sync::Mutex<Option<Option<String>>>> =
            Arc::new(std::sync::Mutex::new(None));
        let recorder = Arc::clone(&at_the_announcement);
        let (deps, _) = deps_counting_embeds(no_key, a_pass_that_must_not_run());
        let (_, settled) =
            run_scan_watching(&turn, &state, Entry::Full, deps, move |state, now| {
                if now.read_seq == 1 {
                    let mut slot = recorder.lock().unwrap_or_else(|e| e.into_inner());
                    if slot.is_none() {
                        *slot = Some(marker(state));
                    }
                }
            });

        assert_eq!(
            settled.last_reading.as_ref().map(|r| r.roots_read),
            Some(2),
            "{settled:?}"
        );
        assert_eq!(
            marker(&state).as_deref(),
            Some("0"),
            "every folder was visited and the index still says a scan is \
             half-done"
        );

        let at_the_announcement = at_the_announcement
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .expect("the end of the reading pass was never announced");
        assert_eq!(
            at_the_announcement.as_deref(),
            Some("0"),
            "the pass announced that it had finished reading while the index \
             still said a scan was half-done, so an observer that reads both \
             sees two answers to one question"
        );
    }

    /// The other direction on the same marker: a pass stopped at the first of
    /// two folders leaves it set.
    ///
    /// Without this the test above is satisfied by a marker that is always
    /// cleared, which is the same as no marker at all.
    #[test]
    fn a_reading_phase_stopped_at_the_first_of_two_folders_leaves_the_marker_set() {
        let turn = take_scan_turn();
        let data = tempfile::tempdir().expect("a data directory");
        let first = dir_holding(&["a1.txt", "a2.txt", "a3.txt"]);
        let second = dir_holding(&["b1.txt"]);
        let state = app_in(data.path());
        watch(&state, first.path());
        watch(&state, second.path());

        let (deps, calls) = deps_counting_embeds(a_key, a_pass_that_must_not_run());
        let stopping = Arc::downgrade(&state);
        let (_, settled) = run_scan_watching(&turn, &state, Entry::Full, deps, move |_, now| {
            if let crate::scan_state::ScanSnapshot::Running {
                phase:
                    Phase::Reading {
                        root_index: 1,
                        counts,
                        ..
                    },
                ..
            } = &now.snapshot
                && counts.total >= 3
                && counts.done == counts.total
                && let Some(state) = stopping.upgrade()
            {
                state.cancel_job();
            }
        });

        let reading = settled
            .last_reading
            .clone()
            .expect("the scan recorded no reading pass");
        assert_eq!(reading.roots_read, 1, "{reading:?}");
        assert_eq!(reading.root_count, 2, "{reading:?}");
        assert_eq!(
            marker(&state).as_deref(),
            Some("1"),
            "one folder of two was read and the index says the scan got all the \
             way round: {reading:?}"
        );
        assert_eq!(report_of(&settled).resume, Some(Entry::Full), "{settled:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    /// An index with no watched folders visited every folder there was.
    ///
    /// The corner the `roots_read == root_count` comparison decides on its own,
    /// and the one an implementation written as "at least one folder was read"
    /// gets wrong: a fresh installation would carry the marker for ever and
    /// every settings screen would report an unfinished scan that had in fact
    /// finished.
    #[test]
    fn a_scan_over_no_folders_at_all_still_clears_the_marker() {
        let turn = take_scan_turn();
        let data = tempfile::tempdir().expect("a data directory");
        let state = app_in(data.path());

        let (deps, _) = deps_counting_embeds(no_key, a_pass_that_must_not_run());
        let (_, settled) = run_scan(&turn, &state, Entry::Full, deps);

        assert_eq!(
            settled.last_reading.as_ref().map(|r| r.root_count),
            Some(0),
            "{settled:?}"
        );
        assert_eq!(
            marker(&state).as_deref(),
            Some("0"),
            "a scan with nothing to visit visited everything there was"
        );
    }

    /// 🔴 `embedOnly` reads no folder at all.
    ///
    /// The pair it separates is "the resumption embedded what was waiting" from
    /// "the resumption re-read the archive first" — minutes to hours of work for
    /// somebody who asked for the cheap half, and the reason [`Entry`] has two
    /// variants rather than being a preference. Asserted on the announcements
    /// (no `Reading` was ever drawn) and on the state (`last_reading` and
    /// `read_seq` are untouched), because either alone can be satisfied by a
    /// pass that read folders and said nothing about it.
    #[test]
    fn an_embed_only_entry_never_reads_a_folder() {
        let turn = take_scan_turn();
        let data = tempfile::tempdir().expect("a data directory");
        let folder = dir_holding(&["a1.txt"]);
        let state = app_in(data.path());
        watch(&state, folder.path());

        let before = state.scan_state().read_seq;
        let (deps, calls) = deps_counting_embeds(no_key, a_pass_that_must_not_run());
        let (snapshots, settled) = run_scan(&turn, &state, Entry::EmbedOnly, deps);

        let report = report_of(&settled);
        assert_eq!(
            report.embedding,
            EmbedOutcome::Skipped {
                why: SkipWhy::NoKey
            },
            "{report:?}"
        );
        assert_eq!(
            report.ended_in,
            EndedIn::Embedding,
            "a resumption that reads no folder cannot have ended in the reading \
             phase: {report:?}"
        );
        assert_eq!(report.reason, EndReason::Completed, "{report:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        assert!(
            !phases(&snapshots)
                .iter()
                .any(|phase| matches!(phase, Phase::Reading { .. })),
            "a folder was announced by a run that is not supposed to read one: \
             {snapshots:?}"
        );
        assert_eq!(
            settled.read_seq, before,
            "the resumption recorded a reading pass it never made"
        );
        assert!(settled.last_reading.is_none(), "{settled:?}");
        assert_eq!(
            marker(&state),
            None,
            "the resumption touched a marker that is the reading pass's to write"
        );
        assert!(
            state
                .with_index(|db| db.paths_under_root(1))
                .expect("reading what the index holds under the folder")
                .is_empty(),
            "the resumption indexed files, so it walked the folder after all"
        );
    }

    /// A resumption leaves the reading pass before it exactly as it was.
    ///
    /// The pair: a person who stopped an embedding pass and pressed Resume must
    /// find the same account of their folders afterwards. `read_seq` is the
    /// field a consumer watches to know new documents have arrived, so a
    /// resumption that moved it would send every surface back to the index for
    /// nothing.
    #[test]
    fn an_embed_only_run_over_pending_chunks_leaves_the_reading_alone() {
        let turn = take_scan_turn();
        let data = tempfile::tempdir().expect("a data directory");
        let folder = dir_holding(&["a1.txt", "a2.txt"]);
        let state = app_in(data.path());
        watch(&state, folder.path());
        adopt_a_model(&state);

        // A reading pass that got round every folder, and then a marker set by
        // hand: the state a scan stopped during its EMBEDDING leaves, where the
        // folders were visited and the index is still owed vectors.
        let (deps, _) = deps_counting_embeds(no_key, a_pass_that_must_not_run());
        let (_, first) = run_scan(&turn, &state, Entry::Full, deps);
        let reading_before = first
            .last_reading
            .clone()
            .expect("the first scan recorded no reading pass");
        let seq_before = first.read_seq;
        state
            .with_index(|db| db.meta_set(SCAN_INCOMPLETE, "1"))
            .expect("setting the marker by hand");

        let (deps, calls) = deps_counting_embeds(a_key, a_pass_that_embeds(2));
        let (snapshots, settled) = run_scan(&turn, &state, Entry::EmbedOnly, deps);

        let report = report_of(&settled);
        assert_eq!(
            report.embedding,
            EmbedOutcome::Ran {
                done: 2,
                total: 2,
                refused: 0
            },
            "{report:?}"
        );
        assert_eq!(report.ended_in, EndedIn::Embedding, "{report:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(
            !phases(&snapshots)
                .iter()
                .any(|phase| matches!(phase, Phase::Reading { .. })),
            "{snapshots:?}"
        );
        assert_eq!(
            settled.last_reading.as_ref(),
            Some(&reading_before),
            "the resumption rewrote what the folders had said"
        );
        assert_eq!(
            settled.read_seq, seq_before,
            "the resumption counted itself as a reading pass"
        );
        assert_eq!(
            marker(&state).as_deref(),
            Some("1"),
            "the resumption cleared a marker only a reading pass may clear"
        );
    }

    /// 🔴 `embedOnly` runs under a stored exclusion prefix that refuses a whole
    /// scan.
    ///
    /// Both directions in one test, because the pair is the point: the same
    /// index, the same moment, refuses `Full` and accepts `EmbedOnly`. A stored
    /// rule that `WalkRules::new` will not build stops a scan — deliberately, so
    /// that nothing is indexed under a rule a person believes is keeping their
    /// files out of a provider's hands — and there is no reason for it to stop a
    /// pass that reads no folder. Branching after the preflight instead of
    /// before it (D-f) is exactly what would strand somebody's queued chunks
    /// behind a rule this run was never going to apply.
    #[test]
    fn embed_only_runs_under_a_stored_prefix_that_refuses_full() {
        let turn = take_scan_turn();
        let data = tempfile::tempdir().expect("a data directory");
        let folder = dir_holding(&["a1.txt"]);
        let state = app_in(data.path());
        let root = watch(&state, folder.path());
        adopt_a_model(&state);
        state
            .with_index(|db| db.add_path_exclusion(root, ".."))
            .expect("writing an unvalidated prefix straight to the index");
        state
            .with_index(|db| db.meta_set(SCAN_INCOMPLETE, "1"))
            .expect("setting the marker by hand");

        let (deps, calls) = deps_counting_embeds(a_key, a_pass_that_embeds(4));
        start_inner(&state, Entry::Full, deps.clone())
            .expect_err("a whole scan started under a stored prefix that cannot become a rule");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "the refused scan embedded something"
        );

        let (_, settled) = run_scan(&turn, &state, Entry::EmbedOnly, deps);
        let report = report_of(&settled);
        assert_eq!(
            report.embedding,
            EmbedOutcome::Ran {
                done: 4,
                total: 4,
                refused: 0
            },
            "a resumption was refused over a rule it was never going to apply: \
             {report:?}"
        );
        assert_eq!(report.reason, EndReason::Completed, "{report:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            marker(&state).as_deref(),
            Some("1"),
            "the resumption cleared a marker only a reading pass may clear"
        );
    }

    /// An index nobody has chosen a model for skips the embedding and says
    /// which state it is in.
    ///
    /// The pair it separates is "no model has been chosen" from "no key has
    /// been entered": both leave an archive unembedded, and the button a window
    /// offers is a different one for each. The mirror is on the same fixture —
    /// with a model adopted, the same scan embeds — so a phase that skipped
    /// everything would fail the second half.
    #[test]
    fn an_index_with_no_model_skips_the_embedding_rather_than_asking_the_provider() {
        let turn = take_scan_turn();
        let data = tempfile::tempdir().expect("a data directory");
        let folder = dir_holding(&["a1.txt"]);
        let state = app_in(data.path());
        watch(&state, folder.path());

        let (deps, calls) = deps_counting_embeds(a_key, a_pass_that_must_not_run());
        let (_, settled) = run_scan(&turn, &state, Entry::Full, deps);
        let report = report_of(&settled);
        assert_eq!(
            report.embedding,
            EmbedOutcome::Skipped {
                why: SkipWhy::NoModel
            },
            "{report:?}"
        );
        assert_eq!(report.reason, EndReason::Completed, "{report:?}");
        assert_eq!(report.ended_in, EndedIn::Embedding, "{report:?}");
        assert_eq!(report.resume, None, "{report:?}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "the pass was asked to embed into a space nobody has chosen"
        );

        // The mirror: the one thing that was missing, supplied.
        adopt_a_model(&state);
        let (deps, calls) = deps_counting_embeds(a_key, a_pass_that_embeds(1));
        let (_, settled) = run_scan(&turn, &state, Entry::Full, deps);
        assert_eq!(
            report_of(&settled).embedding,
            EmbedOutcome::Ran {
                done: 1,
                total: 1,
                refused: 0
            },
            "{settled:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// 🔴 A Stop at the boundary does not overwrite a reading pass that had
    /// already failed.
    ///
    /// The mirror of `a_stop_after_the_last_root_report_still_ends_cancelled_
    /// with_resume_full`, on the same hook and the same line of code: there,
    /// every folder completed and the Stop is what ended the scan; here the
    /// pass had already broken out of its loop carrying the message of the
    /// folder that stopped it, and the Stop must not take that ending's place.
    ///
    /// The pair it separates is "the extraction worker could not be started"
    /// from "somebody pressed Stop" — reported to the same person, about the
    /// same run. An unconditional overwrite files the pool's own diagnostic
    /// sentence under a reason that says a person stopped the scan, which is a
    /// bug report nobody can act on, and for the endings whose `resume_for` is
    /// `None` it also offers a button that spends the time and fails
    /// identically.
    ///
    /// The worker path is a name with nothing behind it, which is what
    /// `Pool::new` refuses — the same failure a broken installation produces,
    /// reached without one.
    #[test]
    fn a_stop_at_the_boundary_does_not_overwrite_a_reading_that_had_already_failed() {
        let data = tempfile::tempdir().expect("a data directory");
        let folder = dir_holding(&["a1.txt"]);
        let state = Arc::new(AppState::new(
            data.path().to_path_buf(),
            data.path().join("no-such-extraction-worker"),
            "http://127.0.0.1:1".to_string(),
            format!(
                "mnema-desktop-scan-job-test-broken-{}",
                data.path().display()
            ),
        ));
        state.open_index().expect("the index would not open");
        watch(&state, folder.path());
        adopt_a_model(&state);

        let fired = Arc::new(AtomicBool::new(false));
        let stopping = Arc::downgrade(&state);
        let turn = take_scan_turn_boundary(Arc::new({
            let fired = Arc::clone(&fired);
            move || {
                fired.store(true, Ordering::SeqCst);
                if let Some(state) = stopping.upgrade() {
                    state.cancel_job();
                }
            }
        }));

        let (deps, calls) = deps_counting_embeds(a_key, a_pass_that_must_not_run());
        let (_, settled) = run_scan(&turn, &state, Entry::Full, deps);

        assert!(
            fired.load(Ordering::SeqCst),
            "the hook never ran, so no Stop landed at the boundary and this test \
             asserted nothing"
        );
        let reading = settled
            .last_reading
            .clone()
            .expect("the scan recorded no reading pass");
        // The premise, on the FOLDER's own row — the one thing the boundary
        // overwrite cannot reach. Asserted there rather than on the pass, so a
        // failure below says "the ending was overwritten" and never "the
        // fixture did not break the pool".
        assert_eq!(
            reading.roots.first().map(|root| root.reason),
            Some(EndReason::Failed),
            "the fixture was supposed to break the extraction pool and did not, \
             so nothing below is about a failure being overwritten: {reading:?}"
        );
        assert_eq!(
            reading.reason,
            EndReason::Failed,
            "the folder said the extraction pool could not start and the pass \
             reports that a person pressed Stop: {reading:?}"
        );

        let report = report_of(&settled);
        assert_eq!(
            report.reason,
            EndReason::Failed,
            "a Stop at the boundary took the place of a real failure, so the \
             message below is filed under a reason nobody can act on: {report:?}"
        );
        assert!(
            report
                .message
                .as_ref()
                .is_some_and(|message| message.contains("extraction worker")),
            "the ending kept a reason and lost the sentence that explains it: \
             {report:?}"
        );
        assert_eq!(report.ended_in, EndedIn::Reading, "{report:?}");
        assert_eq!(report.embedding, EmbedOutcome::NotReached, "{report:?}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a pass that never read a folder went on to embed"
        );
    }

    /// A reading pass stopped over an index with nothing to embed into is still
    /// a Stop.
    ///
    /// The pair: "the person stopped it" against "there was nothing to embed
    /// anyway". A scan that reported `Completed` because no model was chosen
    /// would be telling somebody who pressed Stop that their scan finished, and
    /// offering them no way to carry on — `Skipped` resumes as `None`.
    ///
    /// ⚠️ **It says nothing about the ORDER inside the embedding phase**, and an
    /// earlier version of this comment claimed it did. The Stop is raised on a
    /// `Reading` announcement, so `read_every_root`'s own early return ends the
    /// job and `embed_after` is never entered: the line that decides this
    /// outcome is that return, not the cancel check in front of
    /// `active_space()`. The fourth arm of
    /// `a_stop_during_the_key_read_wins_whatever_the_store_answers` is what
    /// holds that ordering, and it needs the Stop to land during the KEY READ
    /// to reach it at all.
    #[test]
    fn a_reading_stopped_over_an_index_with_no_model_is_still_a_stop() {
        let turn = take_scan_turn();
        let data = tempfile::tempdir().expect("a data directory");
        let folder = dir_holding(&["a1.txt", "a2.txt", "a3.txt"]);
        let state = app_in(data.path());
        watch(&state, folder.path());

        let (deps, calls) = deps_counting_embeds(a_key, a_pass_that_must_not_run());
        let stopping = Arc::downgrade(&state);
        let (_, settled) = run_scan_watching(&turn, &state, Entry::Full, deps, move |_, now| {
            if let crate::scan_state::ScanSnapshot::Running {
                phase: Phase::Reading { counts, .. },
                ..
            } = &now.snapshot
                && counts.total >= 3
                && counts.done == counts.total
                && let Some(state) = stopping.upgrade()
            {
                state.cancel_job();
            }
        });

        let report = report_of(&settled);
        assert_eq!(
            report.reason,
            EndReason::Cancelled,
            "a Stop was reported as a scan that had nothing to do: {report:?}"
        );
        assert_eq!(report.embedding, EmbedOutcome::NotReached, "{report:?}");
        assert_eq!(
            report.resume,
            Some(Entry::Full),
            "a stopped scan owes a way to carry on: {report:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    /// A Stop inside the embedding pass resumes as the cheap half.
    ///
    /// The pair `resume` exists for: a scan stopped in the embedding phase has
    /// folders that were all read, so the next run has only chunks to embed. A
    /// resumption as `Full` would re-read the whole archive to discover there
    /// was nothing to read.
    #[test]
    fn a_stop_during_the_embedding_resumes_as_embed_only() {
        let turn = take_scan_turn();
        let data = tempfile::tempdir().expect("a data directory");
        let folder = dir_holding(&["a1.txt"]);
        let state = app_in(data.path());
        watch(&state, folder.path());
        adopt_a_model(&state);

        let stopping = Arc::downgrade(&state);
        let (deps, calls) = deps_counting_embeds(a_key, move |_cancel, on_progress| {
            if let Some(state) = stopping.upgrade() {
                state.cancel_job();
            }
            on_progress(mnema_embed::EmbedProgress {
                done: 2,
                total: 5,
                failed: 0,
            });
            Ok(mnema_embed::EmbedTally {
                embedded: 2,
                failed: 0,
            })
        });
        let (_, settled) = run_scan(&turn, &state, Entry::Full, deps);

        let report = report_of(&settled);
        assert_eq!(report.reason, EndReason::Cancelled, "{report:?}");
        assert_eq!(report.ended_in, EndedIn::Embedding, "{report:?}");
        assert_eq!(
            report.resume,
            Some(Entry::EmbedOnly),
            "a person stopped half-way through the embedding and is being asked \
             to read every folder again: {report:?}"
        );
        assert_eq!(
            report.embedding,
            EmbedOutcome::Ran {
                done: 2,
                total: 5,
                refused: 0
            },
            "a stopped pass must still report what it managed: {report:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// 🔴 A folder that was only partly seen stays partly seen after a
    /// successful embedding.
    ///
    /// The pair it separates is "the archive was fully read and embedded" from
    /// "the embedding finished over an archive that was never fully read". They
    /// end identically — `Completed`, `Ran`, `resume: None` — and the only thing
    /// that tells them apart is `last_reading.complete`, which the embedding
    /// phase must leave alone. A person whose subdirectory could not be read is
    /// owed that warning after their vectors arrive, not instead of them.
    #[cfg(unix)]
    #[test]
    fn a_partly_read_root_stays_partly_read_after_a_successful_embedding() {
        let turn = take_scan_turn();
        use std::os::unix::fs::PermissionsExt;

        let data = tempfile::tempdir().expect("a data directory");
        let folder = dir_holding(&["seen.txt"]);
        let locked = folder.path().join("locked");
        std::fs::create_dir(&locked).expect("creating the subdirectory");
        std::fs::write(locked.join("hidden.txt"), "text nothing will read")
            .expect("writing inside the subdirectory");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .expect("making the subdirectory unreadable");

        let state = app_in(data.path());
        watch(&state, folder.path());
        adopt_a_model(&state);

        let (deps, calls) = deps_counting_embeds(a_key, a_pass_that_embeds(1));
        let (_, settled) = run_scan(&turn, &state, Entry::Full, deps);

        // Restored before anything can fail below, so the temporary directory
        // can still be cleaned up when it does.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .expect("restoring the subdirectory");

        let report = report_of(&settled);
        assert_eq!(
            report.embedding,
            EmbedOutcome::Ran {
                done: 1,
                total: 1,
                refused: 0
            },
            "{report:?}"
        );
        assert_eq!(report.reason, EndReason::Completed, "{report:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let reading = settled
            .last_reading
            .clone()
            .expect("the scan recorded no reading pass");
        assert!(
            !reading.complete,
            "a successful embedding was allowed to overwrite the warning that \
             part of the archive was never read: {reading:?}"
        );
        assert_eq!(
            reading.reason,
            EndReason::Completed,
            "the pass finished everything it could, which is the other question: \
             {reading:?}"
        );
        assert_eq!(
            marker(&state).as_deref(),
            Some("0"),
            "the folder was visited, so the marker clears even though the \
             archive was not fully seen"
        );
    }

    /// 🔴 A resumption keeps the warning the reading pass left, and the scan
    /// after it is what lifts it.
    ///
    /// Three jobs, because the pair needs the third to mean anything:
    ///
    /// 1. a whole scan over a tree with something unreadable in it, stopped in
    ///    its embedding phase — `complete: false`, one reading pass recorded;
    /// 2. `embedOnly`, which finishes the embedding — and must leave that
    ///    `false` exactly where it was, counters and all, without recording a
    ///    reading pass of its own;
    /// 3. a whole scan over the same tree, now readable — `complete: true`, and
    ///    one more reading pass.
    ///
    /// Without the third, "the resumption left it alone" is satisfied by a field
    /// that is `false` for ever.
    #[cfg(unix)]
    #[test]
    fn an_embed_only_run_keeps_the_last_readings_warning() {
        let turn = take_scan_turn();
        use std::os::unix::fs::PermissionsExt;

        let data = tempfile::tempdir().expect("a data directory");
        let folder = dir_holding(&["seen.txt"]);
        let locked = folder.path().join("locked");
        std::fs::create_dir(&locked).expect("creating the subdirectory");
        std::fs::write(locked.join("hidden.txt"), "text nothing will read")
            .expect("writing inside the subdirectory");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .expect("making the subdirectory unreadable");

        let state = app_in(data.path());
        watch(&state, folder.path());
        adopt_a_model(&state);

        // 1 — read the folder, then stop inside the embedding.
        let stopping = Arc::downgrade(&state);
        let (deps, _) = deps_counting_embeds(a_key, move |_cancel, _on_progress| {
            if let Some(state) = stopping.upgrade() {
                state.cancel_job();
            }
            Ok(mnema_embed::EmbedTally {
                embedded: 0,
                failed: 0,
            })
        });
        let (_, stopped) = run_scan(&turn, &state, Entry::Full, deps);
        let after_reading = stopped
            .last_reading
            .clone()
            .expect("the first scan recorded no reading pass");
        assert!(
            !after_reading.complete,
            "the fixture was supposed to leave part of the archive unread, and \
             did not, so nothing below is about a warning: {after_reading:?}"
        );
        assert_eq!(
            report_of(&stopped).resume,
            Some(Entry::EmbedOnly),
            "{stopped:?}"
        );
        let seq_after_reading = stopped.read_seq;

        // 2 — the resumption the report just named.
        let (deps, calls) = deps_counting_embeds(a_key, a_pass_that_embeds(1));
        let (_, resumed) = run_scan(&turn, &state, Entry::EmbedOnly, deps);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            resumed.last_reading.as_ref(),
            Some(&after_reading),
            "the resumption rewrote the reading pass's own account of the \
             folders — the warning, the counters, or both"
        );
        assert_eq!(
            resumed.read_seq, seq_after_reading,
            "the resumption counted itself as a reading pass, so every surface \
             watching for new documents went back to the index for nothing"
        );
        let report = report_of(&resumed);
        assert_eq!(report.reason, EndReason::Completed, "{report:?}");
        assert_eq!(report.ended_in, EndedIn::Embedding, "{report:?}");

        // 3 — the same tree, now readable: the warning is lifted by a reading
        // pass and by nothing else.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .expect("restoring the subdirectory");
        let (deps, _) = deps_counting_embeds(a_key, a_pass_that_embeds(1));
        let (_, whole) = run_scan(&turn, &state, Entry::Full, deps);
        let after_second_reading = whole
            .last_reading
            .clone()
            .expect("the third scan recorded no reading pass");
        assert!(
            after_second_reading.complete,
            "the tree is readable now and the pass still says otherwise, so \
             `complete` is not measuring anything: {after_second_reading:?}"
        );
        assert_eq!(
            whole.read_seq,
            seq_after_reading + 1,
            "a whole scan is one reading pass more than the resumption before it"
        );
    }
}

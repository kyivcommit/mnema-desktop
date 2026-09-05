//! The scanning job: every watched folder read under ONE slot, and what each
//! folder said.
//!
//! **Why one job rather than the chain it replaces.** `walk_job` walked one
//! folder, told the window, and the window decided what to do next — walk
//! another, start the embedding pass, or nothing. Every one of those decisions
//! sat between two `claim_job` calls, and `claim_job` clears the cancellation
//! flag as it takes the slot: a Stop pressed in a gap was not late, it was
//! erased, and the person's text went to the provider anyway. `start_walk_job`'s
//! own long comment above `stopped_late` is where that was measured and where
//! this file was promised. One claim, held across every folder, is what removes
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
    EmbedOutcome, EndedIn, Entry, Phase, ReadingOutcome, RootOutcome, ScanReport, Terminal,
};
use crate::state::{AppState, JobSlot};
use crate::walk_job::ended_from_report;

/// One watched folder, with everything the walk needs to read it, decided
/// before the job thread exists.
///
/// The rules are built here rather than inside the loop because building them
/// can FAIL — a stored exclusion prefix that `WalkRules::new` refuses — and a
/// refusal is something the person asking for a scan has to be told about, not
/// something a thread discovers three folders in. `start_walk_job`'s own
/// comment on the same call has the whole argument for refusing rather than
/// walking with the rule silently absent.
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
/// Injected rather than called directly so that Task 3's phase can be driven
/// from a test without a credential store and without a provider: the store can
/// put an authorisation dialog on a developer's screen, and the provider costs
/// money. `ScanDeps::production` is the one place the real pair is named.
pub(crate) struct ScanDeps<'a> {
    pub(crate) key: &'a KeyFn,
    pub(crate) embed: &'a EmbedFn,
}

/// The credential store, as one question: the key for this installation, or
/// `None` when the person has not entered one. Takes the state because that is
/// where the credential reference lives — see [`AppState`]'s own field for why
/// it is not a constant.
pub(crate) type KeyFn = dyn Fn(&AppState) -> Result<Option<String>, Error> + Send + Sync;

/// `mnema_embed::run`, with its own signature rather than a narrowed one: the
/// point of the seam is that Task 3's phase cannot tell the difference.
pub(crate) type EmbedFn = dyn Fn(
        &Db,
        &str,
        &str,
        &dyn Fn() -> bool,
        &mut dyn FnMut(mnema_embed::EmbedProgress),
    ) -> Result<mnema_embed::EmbedTally, mnema_embed::Error>
    + Send
    + Sync;

/// The real credential store.
fn production_key(state: &AppState) -> Result<Option<String>, Error> {
    Ok(mnema_secrets::load(state.credential_ref())?)
}

/// The real embedding pass.
fn production_embed(
    db: &Db,
    base: &str,
    key: &str,
    cancel: &dyn Fn() -> bool,
    on_progress: &mut dyn FnMut(mnema_embed::EmbedProgress),
) -> Result<mnema_embed::EmbedTally, mnema_embed::Error> {
    mnema_embed::run(db, base, key, crate::embed_job::BATCH, cancel, on_progress)
}

/// Named `static`s rather than `&production_key` written inline, so that the
/// references are `'static` by construction instead of by whether constant
/// promotion happens to apply to a function item this year.
type KeyPointer = fn(&AppState) -> Result<Option<String>, Error>;
type EmbedPointer = fn(
    &Db,
    &str,
    &str,
    &dyn Fn() -> bool,
    &mut dyn FnMut(mnema_embed::EmbedProgress),
) -> Result<mnema_embed::EmbedTally, mnema_embed::Error>;

static PRODUCTION_KEY: KeyPointer = production_key;
static PRODUCTION_EMBED: EmbedPointer = production_embed;

impl ScanDeps<'static> {
    pub(crate) fn production() -> Self {
        Self {
            key: &PRODUCTION_KEY,
            embed: &PRODUCTION_EMBED,
        }
    }
}

/// Reads every watched folder, in order, under one job slot.
///
/// `(async)` for the reason given on [`crate::bridge::open_index`] and repeated
/// by [`crate::walk_job::start_walk_job`]: this command reads the index before
/// it spawns anything, and a window-issued command that can wait on the same
/// mutex must not be the one left running inline on the main thread.
#[tauri::command(async)]
pub fn start_scan_job(state: State<'_, AppState>, entry: Entry) -> Result<(), Error> {
    start(&state, entry)
}

/// The command, reachable from Rust — the shape `run_walk_and_capture_ending`
/// already uses for the walk, and what the tray will call.
pub(crate) fn start(state: &AppState, entry: Entry) -> Result<(), Error> {
    start_inner(state, entry, &ScanDeps::production())
}

/// 🔴 **The order of the first three steps is the decision this file is about**
/// (D-f), and it is not the order that reads most naturally.
///
/// `claim_job` comes FIRST, before the index is read — the opposite of
/// [`crate::walk_job::start_walk_job`], where every fallible step runs before
/// the claim so that a call which was always going to fail never has
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
/// The cost is the one `start_walk_job` avoids: a scan that fails on a stored
/// exclusion prefix has held the slot for the length of one index read. It is
/// paid deliberately, and the ending is not silent — the slot drops with the
/// phase still `Reading`, so [`crate::state::JobSlot::drop`]'s policy writes
/// `Ended { Failed, "the job ended without a report" }` and the window is told
/// something went wrong rather than being left to notice an idle application.
///
/// `open_job_index` is between the two, for `start_walk_job`'s own reason: a
/// walk is a sequence of writes that can run for hours and the window has to go
/// on answering searches, so the job gets a connection of its own.
pub(crate) fn start_inner(state: &AppState, entry: Entry, deps: &ScanDeps) -> Result<(), Error> {
    // Task 3 is what reads these; Task 2 only carries them. Bound to names
    // rather than silenced with `#[allow(dead_code)]`, so nothing has to
    // remember to take an allowance away once the embedding phase arrives.
    let (_key, _embed) = (deps.key, deps.embed);

    if entry == Entry::EmbedOnly {
        return Err(Error::EmbeddingPhaseNotWired);
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
    std::thread::spawn(move || read_every_root(slot, job_db, worker, roots));
    Ok(())
}

/// Every watched folder and the rules it walks under, read under ONE
/// `with_index` lock.
///
/// One lock rather than one per folder, for the reason `start_walk_job`'s own
/// read gives at greater length: `Db::delete_watched_root` runs as a single
/// transaction that cascades a root's exclusion rows away with it, so a path
/// read before it and an exclusion list read after it describe two different
/// indexes. The masks join the same read not because they belong to a root —
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
        // The same fixed defaults and the same refusal `start_walk_job` uses,
        // and the refusal is the point: under D29 an indexed file is a file
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

/// The reading pass itself: every folder in turn, on the job's own thread.
fn read_every_root(slot: JobSlot, job_db: Db, worker: PathBuf, roots: Roots) {
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
    let _ = job_db.meta_set("scan.incomplete", "1");

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
            // `false` for `stopped_late`, deliberately: this pass reads the
            // flag itself, at the top of the next iteration, and a folder that
            // finished everything it was given DID complete. Rewriting its own
            // reason to `Cancelled` would lose the one fact the per-folder row
            // is for — `start_walk_job` had to do it because its job ended
            // there and the slot changed hands; this one does not.
            Ok(Ok(report)) => {
                root_outcome(root_path, &ended_from_report(&report, false, contended))
            }
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

    slot.mark_reading_done(outcome.clone());

    let reason = outcome.reason;
    // `ok()`, not `unwrap_or(0)`: a count that could not be read is "the last
    // count still stands", which is what `JobSlot::finish` does with `None`. A
    // zero would tell a window the index had emptied itself.
    let files = job_db.indexed_file_count().ok();
    slot.finish(
        Terminal::Ended {
            report: ScanReport {
                // Task 3 replaces this with what the embedding phase did. Until
                // then a scan really does stop after the reading, and saying so
                // is what keeps a window from drawing an archive as searchable.
                embedding: EmbedOutcome::NotReached,
                ended_in: EndedIn::Reading,
                reason,
                message,
                resume: resume_for(reason, EndedIn::Reading),
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
    /// that were READ and not over the folders that exist: a pass that stopped
    /// at the second of seven has nothing to say about the other five, and
    /// counting them as complete is the claim that would draw a person a
    /// finished scan over an index missing most of their archive.
    fn absorb(&mut self, root: RootOutcome) {
        self.roots_read += 1;
        self.complete &= root.complete;
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

        // `0` refused, for `start_walk_job`'s own reason: a walk gives no file
        // up for good, and `WalkProgress::refused` is merged into `skipped`
        // below rather than carried as the different fact `Progress::refused`
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
            // The same merge `start_walk_job` makes: the bar draws one number,
            // and the itemised difference is what the skip journal is for.
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

/// Turn on [`TEST_HOOK`]. The slot is one per binary, so two tests that install
/// a hook cannot overlap: otherwise one's teardown clears the other's hook
/// before that other's reader has had a chance to run it. `prefs.rs`'s own
/// `HOOK_TURN` is the shape this copies, not the instance — they guard
/// different hooks and must not serialise against each other.
#[cfg(test)]
static HOOK_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
#[must_use = "the hook is cleared when this is dropped"]
#[allow(dead_code)] // held for its `Drop`, the guard itself is never read
struct HookTurn(std::sync::MutexGuard<'static, ()>);

#[cfg(test)]
impl Drop for HookTurn {
    fn drop(&mut self) {
        set_test_hook(None); // idempotent; covers the panic path
    }
}

#[cfg(test)]
fn take_hook_turn(hook: Hook) -> HookTurn {
    // Poisoning is absorbed: a test that panicked must not also poison the next
    // one's turn.
    let turn = HOOK_TURN.lock().unwrap_or_else(|e| e.into_inner());
    set_test_hook(Some(hook));
    HookTurn(turn)
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
            false,
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
    /// The pair it separates is "two folders, one of them partly seen" from
    /// "two folders, both seen": the counters are identical and only `complete`
    /// differs, which is the whole reason `complete` is not derived from
    /// `reason`. `roots_read` is asserted beside it because a pass that counted
    /// the folders it was GIVEN rather than the ones that answered reports the
    /// same number here and a wrong one the moment a pass stops early.
    #[test]
    fn a_pass_is_complete_only_when_every_folder_it_read_was() {
        let folder = |complete: bool| RootOutcome {
            root_path: "/somewhere".to_string(),
            reason: EndReason::Completed,
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

        let mut both_seen = ReadingOutcome {
            root_count: 2,
            ..ReadingOutcome::default()
        };
        both_seen.absorb(folder(true));
        both_seen.absorb(folder(true));
        assert!(both_seen.complete, "{both_seen:?}");

        let mut one_partly = ReadingOutcome {
            root_count: 2,
            ..ReadingOutcome::default()
        };
        one_partly.absorb(folder(false));
        one_partly.absorb(folder(true));
        assert!(
            !one_partly.complete,
            "one folder that was not fully seen must make the pass not fully \
             seen: {one_partly:?}"
        );

        // Identical in everything but `complete`, which is what makes the pair
        // above a pair rather than two different passes.
        assert_eq!(both_seen.done, one_partly.done);
        assert_eq!(both_seen.indexed, one_partly.indexed);
        assert_eq!(both_seen.removed, one_partly.removed);
        assert_eq!(both_seen.contended, one_partly.contended);
        assert_eq!(both_seen.roots_read, 2);
        assert_eq!(one_partly.roots_read, 2);
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
    /// and the walk (D-l), and it models the removal **as Task 4 will perform
    /// it**: claim the slot, delete, give the slot back. That is deliberate and
    /// it is what this test can and cannot prove at this commit — see the note
    /// below.
    ///
    /// ⚠️ **What it proves here.** `bridge::remove_watched_folder` does not
    /// take the job slot yet; Task 4 is what makes it. So the hook cannot
    /// simply call that command — it would delete the row whichever order the
    /// production lines are in, and the test would fail against correct code.
    /// Calling `state.with_index(|db| db.delete_watched_root(1))` would be the
    /// same mistake one layer down. What the hook does instead is exactly what
    /// Task 4's command will do, so this test asserts the protocol the scan
    /// depends on: that a removal which asks for the slot first cannot get it
    /// while a scan holds it. It does NOT assert that today's
    /// `remove_watched_folder` asks — that is Task 4's own test to write.
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
        let b_path = folder_b.path().to_path_buf();
        let _turn = take_hook_turn(Arc::new({
            let fired = Arc::clone(&fired);
            let removed = Arc::clone(&removed);
            move |state: &AppState| {
                fired.store(true, Ordering::SeqCst);

                // Task 4's removal command, modelled line for line: the slot
                // first, then the delete, then the slot back.
                let root_path = state
                    .with_index(|db| db.watched_root_path(1))
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let Ok(slot) = state.claim_job(Phase::Removing { root_path }, false) else {
                    return;
                };
                let deleted = state.with_index(|db| db.delete_watched_root(1));
                slot.finish(Terminal::Idle, None);
                if deleted.is_ok() {
                    removed.store(true, Ordering::SeqCst);
                    let _ = state
                        .with_index(|db| db.insert_watched_root(&b_path.display().to_string()));
                }
            }
        }));

        start_inner(&state, Entry::Full, &ScanDeps::production())
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
}

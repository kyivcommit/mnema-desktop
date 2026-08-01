//! A walk of one watched root: enumerate, ingest, reconcile.
//!
//! Two phases rather than one streaming pass, decided on a measurement:
//! enumerating 5,249 files costs 21.5 ms warm against 3.98 ms per file of
//! indexing (D40), so the extra metadata pass is 0.1% of the work and buys the
//! denominator a multi-hour job needs (spec §3).
//!
//! Phase 3 reconciles what the index already holds under the watched root
//! against what a *complete* walk just saw, and deletes what is genuinely
//! gone — a document only when its last path is, and its vectors with it
//! (§7), through the same `forget_if_unnamed` an ordinary edit already used
//! before this phase existed. It runs only once phase 1 and phase 2 both
//! finished cleanly; see the gate at the top of phase 3, below, for exactly
//! what that means.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use mnema_index::{Db, SkipRule};
use mnema_pool::Pool;
use mnema_walk::{Found, PreSkipRule, WalkRules, enumerate};

use crate::{IngestError, Ingested, ingest_file};

/// How many times [`ingest_with_busy_retry`] asks `ingest_file` again after a
/// `Busy` answer, before it gives up on the file for this pass and journals a
/// skip instead.
///
/// Not a sleep-and-retry of this crate's own devising: every attempt already
/// spends up to the index's own `busy_timeout` — five seconds
/// (`crates/mnema-index/src/open.rs`) — inside SQLite's busy handler before it
/// returns `Busy` at all, so stacking a manual wait on top would only be
/// waiting to wait. Three attempts is three separate five-second windows for
/// whatever else holds the write lock to finish and let go — measured at
/// 5.19 s for one ordinary write in
/// `a_walk_that_meets_the_window_holding_the_write_lock_is_told_to_retry`
/// (`crates/mnema-ingest/tests/slice.rs`). Bounded rather than unbounded
/// because a lock held for the better part of a minute is no longer the
/// "a window added a folder" case D46 was written for, and a walk of tens of
/// thousands of files cannot afford to find that out one retry at a time on
/// every single one of them.
const BUSY_RETRIES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Completed,
    Cancelled,
    /// Consecutive skips that are evidence about the environment rather than
    /// about any one file's bytes said the worker, the machine or the volume
    /// is broken rather than the files. D44 named this counter and said it
    /// belongs to whoever owns the walk. This is that owner.
    BrokenWorker,
    /// The exclusion rules failed to combine into a working pattern set for
    /// this walk (`Walked::rules_applied` in `mnema-walk`), which means
    /// `Walked::found` may hold files the user asked to exclude. Under D29
    /// there are no local models in this product, so indexing a file sends it
    /// to a third-party embedding provider — an exclusion rule is the user's
    /// only mechanism for keeping a file away from that. Proceeding on the
    /// hope that the rules did not matter for this particular folder would be
    /// exactly the silent failure `Walked::rules_applied` exists to name, so
    /// phase 2 never starts: nothing is read, nothing is sent anywhere. Not
    /// excessive caution — the alternative is a setting that fails open.
    RulesNotApplied,
    /// The root itself could not be entered at all — an ejected external
    /// drive, a deleted folder. Named apart from an ordinary empty folder
    /// (D33: a folder that disappears is a pause, not a deletion) and apart
    /// from the per-file skip journal: the root not existing is not a fact
    /// about any *file* under it, so nothing is written to `skipped` for it —
    /// see the comment where this is checked, below.
    RootUnavailable,
    /// Phase 3's own refusal, distinct from `RootUnavailable`: the root
    /// itself was fine — phase 1 walked it and phase 2 ran every file it was
    /// handed — but the walk named no file at all under a root the index
    /// still holds paths for, which is indistinguishable from here from a
    /// detached volume mounted empty at the same path (D33). Deletion is
    /// refused rather than guessed at; the comment where this fires, in
    /// `walk_root`, has the exact signature.
    VolumeMissing,
}

/// Why reconciliation refused to delete a `known` path the walk did not
/// find — always a statement that the walk has no evidence OF deletion,
/// never a statement that the file is confirmed to still be there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrozenReason {
    /// A symlink to a directory: the walk never entered it
    /// (`PreSkipRule::NotAFileSubtree`), so it has no evidence at all about
    /// what used to live under that name before it became a symlink.
    SymlinkedSubtree,
    /// A directory that read as holding nothing — the same ambiguity D33
    /// names for the watched root itself (`StopReason::VolumeMissing`),
    /// one level down: an unmounted share and a folder emptied by hand are
    /// the same shape from here, and this product answers both by pausing
    /// rather than guessing. Does not self-heal on its own: a folder
    /// emptied, not deleted, reads exactly the same on every later walk
    /// too, because nothing here re-examines a shape that never changes.
    EmptyDirectory,
    /// `read_dir` on the directory failed for a reason other than "it does
    /// not exist" — most likely permission denied. Treated the same as
    /// `EmptyDirectory`: the conservative side, chosen over guessing.
    UnreadableDirectory,
}

/// One prefix reconciliation refused to delete anything under, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frozen {
    /// Relative to the watched root, `/`-separated — the same form
    /// `Found::relative` and `path.relative_path` use. Every `known` path
    /// starting with this prefix followed by `/` was left alone
    /// (`should_delete`'s own `under` check, below).
    ///
    /// Deliberately NOT "or equal to this": a `known` path is always a
    /// FILE — one row in the `path` table — while every `prefix` here names
    /// something `resolve_ancestor` (or the symlink producer) found
    /// currently readable as a directory or a non-file entry. A `known`
    /// path cannot be both at once *this walk*: whatever currently exists
    /// at a given relative path, in any representable form, is exactly what
    /// `seen` is built from, and `seen` is `should_delete`'s first,
    /// short-circuiting check — so a path this exact string could ever
    /// equal is already excluded before the `frozen` comparison runs at
    /// all. This used to be claimed here without being true of
    /// `should_delete`, which checks `under` only; the randomised harness's
    /// own invariant 3b (`tests/randomised.rs`) now asserts the equality
    /// case never arises, rather than silently tolerating it.
    pub prefix: String,
    pub why: FrozenReason,
}

#[derive(Debug, Clone, Copy)]
pub struct WalkProgress {
    pub done: u64,
    pub total: u64,
    /// Files phase 2 asked about and did not index — mirrors
    /// `WalkReport::skipped` exactly, and excludes phase-1 refusals for the
    /// same reason that field does.
    pub skipped: u64,
    /// Files phase 1 refused before any worker was asked — mirrors
    /// `WalkReport::refused`. Without this, a window rendering "N skipped"
    /// mid-walk under-reports by however many files phase 1 already turned
    /// away, even though `done` already counts them: the bar would be in the
    /// right place and the label next to it would be lying.
    pub refused: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkReport {
    /// How many files phase 1 handed to phase 2 — the ones that became a
    /// worker request, a cheap-arm hit, or a phase-2 skip. Does **not**
    /// include `refused`, which counts files phase 1 never handed over.
    ///
    /// `indexed + unchanged + skipped == found` **only when `stopped ==
    /// Completed`.** Any early stop leaves the remainder of `found` untouched
    /// and the three counts short of it — measured, thirty files against a
    /// broken worker give `found: 30` with the three summing to 8. This
    /// sentence used to state the equality with no condition on it, which is
    /// the more dangerous half of the same trap `complete` carries: a walk
    /// that stopped early still reports every file phase 1 saw.
    pub found: u64,
    pub indexed: u64,
    pub unchanged: u64,
    /// Files phase 2 asked about and did not index — a worker refusal, or the
    /// index staying busy through every retry (`ingest_with_busy_retry`).
    pub skipped: u64,
    /// Files phase 1 refused before any worker was asked: an unrepresentable
    /// name, a cloud placeholder, a symlink, an unreadable entry. Kept apart
    /// from `skipped` because a caller summing "how many files did this walk
    /// even look at" needs `found`, which by definition excludes these — a
    /// dashboard that folded them into `skipped` and then computed
    /// `(done + skipped) / total` without a matching `total` change would
    /// show more than 100% before phase 2 read a single byte, which is
    /// exactly what a review probe measured here.
    pub refused: u64,
    /// How many `path` rows phase 3 deleted because the file they named was
    /// genuinely gone. Stays `0` whenever phase 3 refuses to run — an
    /// incomplete walk, an early stop, or the unmount signature
    /// (`StopReason::VolumeMissing`) — never as a sign nothing vanished.
    pub removed: u64,
    /// Every prefix phase 3 refused to delete under, and why — the answer
    /// to "why does this file, gone from my disk, still show up in
    /// search?" for exactly the case `removed` alone cannot answer,
    /// because `removed` counts only what phase 3 DID delete. Always empty
    /// whenever phase 3 refused to run at all (mirrors `removed`'s own
    /// rule) — but also empty on an ordinary `Completed` walk that found
    /// nothing ambiguous, which `removed == 0` alone cannot be told apart
    /// from: a walk that silently freezes five hundred documents and a
    /// walk where nothing happened both report `removed: 0, stopped:
    /// Completed`. This field is what tells them apart, and it is the
    /// whole reason it exists — a caller with nothing better to show a
    /// person than that silence had already lost the one question this
    /// product is named after answering: what can still answer with text
    /// the file no longer contains.
    pub frozen: Vec<Frozen>,
    /// False if any entry under the root could not be read — an unreadable
    /// subdirectory, most commonly. From `found` alone, an unreadable
    /// subdirectory is indistinguishable from an empty one, so a
    /// reconciliation that deletes rows for paths absent from `found` must
    /// refuse to run when this is `false` (`Walked::complete`'s own doc
    /// comment in `mnema-walk` has the reasoning in full). Mirrors that field
    /// exactly; `stopped == Completed` does **not** imply this is `true` — a
    /// walk can process every file it found and still have missed a whole
    /// subtree along the way.
    pub complete: bool,
    pub stopped: StopReason,
}

/// Walks `root`: enumerates it, ingests what changed, reconciles what
/// vanished.
///
/// The three run in that order, and the order between the last two is part
/// of this function's contract, not an implementation detail a later edit
/// is free to rearrange: **phase 2 (ingest) must run before phase 3
/// (reconcile).** A rename is same bytes under a new name, and depends on
/// it entirely — phase 2 re-ingests the new name first, content addressing
/// hands back the SAME `document.id`, and `path_count` for it rises to 2
/// before phase 3 ever runs; only then does phase 3 removing the old path
/// row leave 1. Run phase 3 first and `path_count` hits 0 instead:
/// `forget_if_unnamed` destroys the document there and then, and phase 2
/// running second rebuilds it from nothing under a fresh set of chunk ids
/// — every citation into it invalidated over what was only a rename.
/// Measured directly, by moving phase 3 above phase 2:
/// `a_rename_keeps_the_document_and_its_chunks` (`tests/walk.rs`) failed
/// with the document rebuilt onto different chunk ids rather than kept.
pub fn walk_root(
    pool: &Pool,
    db: &Db,
    root_id: i64,
    root: &Path,
    rules: &WalkRules,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(WalkProgress),
) -> Result<WalkReport, IngestError> {
    // The root itself may be gone entirely: an ejected external drive, a
    // folder deleted since the last walk. `enumerate` already covers PART of
    // this gap with its own `!root.is_dir()` guard at the top of
    // `mnema-walk/src/lib.rs`, which is exactly what this check duplicates —
    // deliberately, so it runs before `enumerate` is even called, rather
    // than trusting a caller to notice `enumerate`'s answer meant this.
    // `enumerate`'s guard covers only the common shape (root already gone
    // when the walk starts); the pre-skip loop's own comment, below, has the
    // narrower gap neither guard closes — a root that vanishes in the moment
    // between them. Whichever guard catches it, `enumerate` can only express
    // "the root itself" as an ordinary `PreSkip` keyed on the root's own
    // absolute path, and the pre-skip loop below journals every `PreSkip` it
    // is handed under `relative_path`, a column that means "relative to the
    // root". The root not existing is not a fact about any one file under
    // it, so the common case earns a `StopReason` of its own and never
    // reaches `enumerate`, let alone the journal, at all.
    if !root.is_dir() {
        return Ok(WalkReport {
            found: 0,
            indexed: 0,
            unchanged: 0,
            skipped: 0,
            refused: 0,
            removed: 0,
            frozen: Vec::new(),
            complete: false,
            stopped: StopReason::RootUnavailable,
        });
    }

    // Phase 1. The only look at the disk.
    let walked = enumerate(root, rules);
    let total = walked.found.len() as u64 + walked.skipped.len() as u64;
    let mut report = WalkReport {
        found: walked.found.len() as u64,
        indexed: 0,
        unchanged: 0,
        skipped: 0,
        refused: 0,
        removed: 0,
        frozen: Vec::new(),
        complete: walked.complete,
        stopped: StopReason::Completed,
    };

    // Files the walk refused before any worker: they belong in the journal,
    // so that "why is this file not in my index?" has an answer. Every
    // `PreSkipRule` maps to `SkipRule::Unreadable` here — none of the five is
    // a statement about the file's own bytes, and every one of them may stop
    // being true on the very next walk (a permission fixed, a file
    // re-materialised, a symlink replaced), which is exactly what
    // `Unreadable` means on the `SkipRule` side. Written as an exhaustive
    // `match` so that a sixth `PreSkipRule` has to be placed here by whoever
    // adds it, rather than silently falling through to a shared reason that
    // stops being true the moment the sixth variant is not equally described
    // by it.
    for pre in &walked.skipped {
        let reason = match pre.rule {
            PreSkipRule::UnrepresentableName => {
                "the file name is not valid UTF-8, so it cannot be named the way the rest of \
                 the index compares paths"
            }
            PreSkipRule::NotMaterialised => {
                "the file is a cloud placeholder that has not been downloaded to this disk"
            }
            PreSkipRule::Unreadable => {
                "the walker could not read this entry: permission denied, it vanished mid-walk, \
                 or its size does not fit"
            }
            PreSkipRule::NotAFile => {
                "not a regular file or a directory: a symlink, a dangling symlink, a FIFO, a \
                 socket or a device"
            }
            PreSkipRule::NotAFileSubtree => {
                "a symlink to a directory; the walk does not follow it, so nothing under it was \
                 visited"
            }
        };
        // `pre.relative` is `None` for three reasons on the `mnema-walk`
        // side, only two of which still reach this loop. A walker error with
        // no path to peel, and a name that is not valid UTF-8
        // (`UnrepresentableName`), both still land here. The third — the
        // root itself failing `enumerate`'s own `!root.is_dir()` check — no
        // longer does, because the guard above this function's phase 1 now
        // intercepts that case before `enumerate` is ever called. What the
        // guard above does NOT close is the narrow race inside it:
        // `enumerate` re-checks the root once more when its walker actually
        // reaches the depth-0 entry, and a root that vanishes in the gap
        // between this function's check and that one still surfaces here,
        // with `pre.detail` holding the root's own absolute path. `pre.detail`
        // is recorded in all these `None` cases, purely so a person reading
        // the journal has something to look at; it can never equal a
        // `Found::relative` a later walk produces, so a reconciliation that
        // matches the skip journal against what the walk found will never
        // see this row as resolved, and will re-touch it as "still missing"
        // on every future walk. That is a known, accepted limitation of this
        // row, not a bug in the reconciliation that reads it.
        let key = pre.relative.as_deref().unwrap_or(&pre.detail);
        db.record_skip(root_id, key, None, reason, SkipRule::Unreadable, None)?;
        report.refused += 1;
    }

    // The exclusion rules may not have applied to this walk at all — see
    // `StopReason::RulesNotApplied`'s own doc comment. Checked after the
    // pre-skip journal above, not before it: recording those rows sends
    // nothing anywhere — each one is a local sqlite write about a file the
    // walk never opened — so it carries none of the risk `rules_applied`
    // exists to guard against, whether or not this particular file would
    // have been excluded by a working rule set. What must not happen next is
    // asking a worker to read a file, which is why this gate sits here,
    // before phase 2 rather than before phase 1.
    if !walked.rules_applied {
        report.stopped = StopReason::RulesNotApplied;
        return Ok(report);
    }

    // Phase 2.
    let mut consecutive_environmental = 0usize;
    // Derived from the CONFIGURED worker count, not `live_workers()`: the
    // live count is 0 until the first file asks for a process, so reading it
    // here — before phase 2 has touched anything — always gave 2 regardless
    // of how the pool was actually sized, which is a threshold two ordinary
    // unlucky files can cross by coincidence. `max(…, 8)` puts a floor under
    // it for the same reason: this counter exists to tell "these files
    // happen to be bad" from "this machine is broken", and two — or even
    // four, for the smallest configured pool — in a row is still well inside
    // what an unremarkable folder can produce (D44's own two big PDFs in a
    // row is exactly that shape).
    let broken_after = (pool.configured_workers() * 2).max(8);

    // Emitted before the loop so a caller learns `total` — and how much of it
    // is already resolved by phase 1's own refusals — before the first file
    // in `found` is opened. `done` here is `report.refused`, not `0`: those
    // files are already accounted for, and a window drawing a bar from this
    // callback needs that reflected immediately, or it would show 0/`total`
    // even though `refused` of `total` is already behind it.
    on_progress(WalkProgress {
        done: report.refused,
        total,
        skipped: report.skipped,
        refused: report.refused,
    });

    for found in &walked.found {
        if cancel.load(Ordering::SeqCst) {
            report.stopped = StopReason::Cancelled;
            return Ok(report);
        }

        match ingest_with_busy_retry(pool, db, root_id, found, cancel)? {
            Retried::Cancelled => {
                report.stopped = StopReason::Cancelled;
                return Ok(report);
            }
            Retried::StillBusy => {
                report.skipped += 1;
                // Write contention is not evidence about the worker, the
                // machine or the volume at all — it is evidence about
                // whatever else opened a write transaction, most often a
                // window the user is looking at. Deliberately NOT touching
                // `consecutive_environmental` in either direction: not
                // incrementing it, because two contended files in a row must
                // not read as a dying worker, and not resetting it either,
                // because a contended file sitting in the middle of a
                // genuine run of `Crash`/`Timeout`/`Memory` skips says
                // nothing about whether that run is over.
            }
            Retried::Settled(Ingested::Indexed { .. } | Ingested::AlreadyIndexed { .. }) => {
                report.indexed += 1;
                consecutive_environmental = 0;
            }
            Retried::Settled(Ingested::Unchanged { .. }) => {
                report.unchanged += 1;
                consecutive_environmental = 0;
            }
            Retried::Settled(Ingested::Skipped { rule }) => {
                report.skipped += 1;
                if rule.suggests_broken_environment() {
                    consecutive_environmental += 1;
                    if consecutive_environmental >= broken_after {
                        report.stopped = StopReason::BrokenWorker;
                        return Ok(report);
                    }
                } else {
                    // A folder of scans, or of files over the size ceiling,
                    // is not a broken worker.
                    consecutive_environmental = 0;
                }
            }
        }

        on_progress(WalkProgress {
            done: report.indexed + report.unchanged + report.skipped + report.refused,
            total,
            skipped: report.skipped,
            refused: report.refused,
        });
    }

    // Phase 3. Deletions are applied only after a complete enumeration, and
    // never during one: a walk that stopped half-way has not seen the whole
    // folder, so what it did not see is not evidence of anything (§7). Must
    // run after phase 2, not only after phase 1 — see this function's own
    // doc comment for why that ordering is a contract rather than a matter
    // of taste, and the mutation that proves it.
    //
    // Gated on BOTH `walked.complete` and `report.stopped` — two different
    // questions, and neither implies the other. `complete` says whether
    // phase 1 read every entry under the root; `stopped` says whether phase 2
    // was allowed to run to the end of what phase 1 handed it. Every early
    // `return` above already makes `stopped == Completed` true by the time
    // this line runs — this `match` re-derives that instead of trusting it,
    // so a `StopReason` added later, or an early `return` moved past this
    // point, has to be placed in this `match` rather than silently reopening
    // deletion under a stop that does not mean "the walk finished, having
    // seen the whole root".
    let stopped_cleanly = match report.stopped {
        StopReason::Completed => true,
        StopReason::Cancelled
        | StopReason::BrokenWorker
        | StopReason::RulesNotApplied
        | StopReason::RootUnavailable
        | StopReason::VolumeMissing => false,
    };
    if !walked.complete || !stopped_cleanly {
        return Ok(report);
    }

    let known = db.paths_under_root(root_id)?;

    // The unmount signature (D33): a detached disk and a mass delete are
    // indistinguishable from here, so the answer is a pause. Stated as a
    // fact about `root` itself, not as a fraction — a fraction would be a
    // number nothing here measured. `!known.is_empty()` is checked first
    // because it is free and `root_is_empty` is not — one `opendir`, worth
    // skipping when there is nothing under this root the walk could
    // possibly disagree with anyway.
    //
    // Deliberately a RAW listing, taken with no exclusion rule applied at
    // all, and not `walked.found` or anything else `Walked` carries.
    // Measured directly: a rule that newly excludes the watched root's only
    // file produces the identical `Walked` (found empty, skipped empty,
    // complete true) as a root with genuinely nothing left on disk — an
    // override-excluded entry is invisible to `enumerate` in exactly the
    // same way a nonexistent one is, because the walker never descends into
    // it either way. `a_newly_excluding_rule_removes_what_it_excludes` needs
    // that walk to delete; `an_empty_root_deletes_nothing` needs the
    // opposite call on the same `Walked` shape — so nothing inside `Walked`
    // can be the fact this decision turns on, and this line exists because
    // an earlier version of it read `walked.found.is_empty()` and failed the
    // exclusion-rule test for exactly that reason.
    //
    // What differs between the two cases is what is actually sitting at
    // `root`, read without any rule deciding what counts. A directory that
    // legitimately holds nothing — an ejected volume's stub, a folder
    // emptied by hand — has an empty top-level listing, full stop; a
    // directory whose only content was just excluded still has an entry
    // there, the walker simply declined to descend into it. A read failure
    // counts as empty too: the conservative direction, a pause rather than a
    // guess, for the same root that already earned `RootUnavailable` above
    // had it vanished before phase 1 instead of during phase 3.
    if !known.is_empty() && root_is_empty(root) {
        report.stopped = StopReason::VolumeMissing;
        return Ok(report);
    }

    // Presence, not absence — `PreSkipRule::NotMaterialised`'s own doc
    // comment in `mnema-walk` states the obligation in those words.
    // `NotMaterialised`, `NotAFile` and `NotAFileSubtree` do not clear
    // `Walked::complete`, because declining a placeholder or a symlink is a
    // decision, not a read failure — but the file is still there. Built from
    // every pre-skip that carries a path, not filtered by which of the three
    // rules produced it: entries with no path (`UnrepresentableName`, a
    // walker error with nothing to peel) contribute nothing to a set keyed on
    // path, so including them costs nothing, and excluding them by name would
    // be a second place this list has to be kept in step with
    // `mnema-walk`'s.
    let seen: HashSet<&str> = walked
        .found
        .iter()
        .map(|f| f.relative.as_str())
        .chain(walked.skipped.iter().filter_map(|s| s.relative.as_deref()))
        .collect();

    // A symlink to a directory is a whole subtree the walk never entered —
    // `PreSkipRule::NotAFileSubtree`'s own doc comment defers exactly this
    // call to whoever reconciles. Its `relative` names the symlink itself,
    // not anything beyond it, so about what used to live under that name —
    // before it became a symlink — the walk has no evidence at all: not
    // "absent", not "present". Treated as a path PREFIX rather than an exact
    // path for that reason: a `known` path that predates the symlink has
    // exactly that shape, and deleting it would be deleting on
    // absence-of-evidence, which is precisely what §7 forbids.
    //
    // The first of two producers of `frozen`. The second — an unmounted
    // NESTED subtree, below — earns exactly the same prefix treatment for
    // the same reason, so this starts the collection rather than standing
    // apart from it.
    let mut frozen: Vec<Frozen> = walked
        .skipped
        .iter()
        .filter(|s| s.rule == PreSkipRule::NotAFileSubtree)
        .filter_map(|s| s.relative.as_deref())
        .map(|prefix| Frozen {
            prefix: prefix.to_string(),
            why: FrozenReason::SymlinkedSubtree,
        })
        .collect();

    // The root's own unmount signature, one directory down. A mounted share
    // or drive under (not at) the watched root can unmount exactly the way
    // the root itself can, leaving a readable, empty directory behind —
    // `Walked::complete` stays true, an empty directory reads fine, and the
    // root-level check above stays false, because whatever else is under
    // the root is still there. Without this, phase 3 read the emptied share
    // as every file under it having been deleted: measured directly against
    // that exact shape — a watched root holding `keep.txt` at the top and
    // `mnt/one.txt`, `mnt/two.txt` under a share that then unmounted gave
    // `removed: 2, stopped: Completed`, `keep.txt` the only path left.
    //
    // `resolve_ancestor` is what tells that shape apart from a subdirectory
    // deleted outright, which must NOT be frozen — an earlier version of
    // this check answered "does `read_dir` on the immediate parent fail?"
    // with a single bool, which cannot tell "the directory exists and is
    // empty" (D33's ambiguity) apart from "the directory does not exist at
    // all" (evidence OF deletion, the opposite conclusion). Measured: with
    // that version, `rm -rf`-ing `gone/` entirely — two indexed files,
    // deleted along with their directory — gave `removed: 0` and left both
    // `path` rows searchable forever, because `std::fs::read_dir` on a
    // missing directory returns `Err(NotFound)`, which the old check
    // treated exactly like `Ok` with zero entries. `resolve_ancestor`'s own
    // doc comment has the three states this replaces that one bool with.
    //
    // `missing` is computed HERE — after the symlink producer above has
    // already populated `frozen`, before the ancestor-climb producer below
    // extends it further — and that ordering is deliberate, not incidental,
    // for a reason that is about the QUALITY of `frozen` rather than about
    // which paths end up deleted (`should_delete`'s own final check below
    // re-reads the fully-populated `frozen` regardless of when `missing` was
    // built, so the delete/keep answer cannot change). What the ordering
    // buys: a `known` path already excused by a `NotAFileSubtree` prefix is
    // filtered out of `missing` before `resolve_ancestor` ever runs on it,
    // so the climb never independently re-examines what is on the far side
    // of a symlink this walk deliberately never followed. Skipping that
    // shadowing would not silently delete anything it should not — it would
    // silently REPORT the wrong reason, or two contradictory ones for the
    // same prefix: `resolve_ancestor` reads real directory entries by
    // following the symlink (`read_dir` follows symlinks), so a symlink
    // whose target happens to be empty would additionally freeze under
    // `EmptyDirectory`, alongside the accurate `SymlinkedSubtree` entry
    // already there for a reason that has nothing to do with what the
    // symlink happens to point at today.
    //
    // Checked per distinct PARENT DIRECTORY of a `known` path the walk did
    // not find — not for every directory under the root, and not once per
    // missing path sharing one, since `resolve_ancestor` caches every
    // directory it visits — so the cost stays proportional to what
    // actually went missing rather than to the size of the tree. A `known`
    // path already excused by `seen` or by a `NotAFileSubtree` prefix needs
    // no second look; a top-level path (no `/` in it) has no directory of
    // its own below the root to freeze, and its absence is already the
    // root-level check's business, above.
    //
    // Excusing the `NotAFileSubtree` prefixes here is an OPTIMISATION and
    // nothing more, which is worth stating because two readings of this
    // filter — one per reviewer — both got it wrong in different
    // directions before it was measured. Replacing this filter with a bare
    // `!seen.contains(p)` changes no deletion and no report: `frozen` only
    // ever grows, and `should_delete` is monotone in it, so probing more
    // parents can never delete more; and the report stays identical because
    // the duplicate-and-contradiction guard inside the loop below is what
    // keeps `frozen` clean, not this filter. What this line buys is the
    // climbs that guard would have thrown away anyway.
    //
    // The cost of getting this right: a directory a user EMPTIES without
    // deleting — every file inside removed, the folder itself left behind —
    // now reads exactly like an unmounted share and is frozen the same way,
    // where before this fix it correctly reconciled (`removed` included
    // those files). That is D33's own tradeoff, applied one directory down
    // rather than only at the root, not a new one invented here: pausing
    // on an ambiguity a person could resolve in a second is preferred over
    // guessing wrong and deleting content that is still on the disk. The
    // cost is real, and it does not self-heal: those files stay frozen on
    // every future walk too, for as long as the empty directory itself
    // remains, because nothing here re-examines a shape that never changes
    // on its own — an unmounted mountpoint and an emptied folder are the
    // same directory to `read_dir` (an unmounted mountpoint reverts to the
    // underlying filesystem), so no cleverer rule closes this gap; a person
    // has to. `WalkReport::frozen` is what tells that person there is an
    // ambiguity to resolve at all, rather than leaving `removed: 0,
    // stopped: Completed` indistinguishable from a walk where nothing
    // happened.
    let missing: Vec<&str> = known
        .iter()
        .map(String::as_str)
        .filter(|p| should_delete(p, &seen, &frozen))
        .collect();
    let mut ancestor_cache: HashMap<&str, Option<(&str, FrozenReason)>> = HashMap::new();
    for relative in missing {
        let Some(parent) = relative.rfind('/').map(|i| &relative[..i]) else {
            continue;
        };
        // Already covered by an entry the symlink producer wrote, or by an
        // ancestor a previous iteration of this very loop already resolved
        // and pushed — skip the climb (and the duplicate report entry it
        // would otherwise produce for the same prefix) rather than pay for
        // it and dedupe after the fact.
        if frozen
            .iter()
            .any(|f| f.prefix == parent || under(parent, &f.prefix))
        {
            continue;
        }
        // `prefix` here is `resolve_ancestor`'s own answer, not `parent` —
        // when `parent` does not exist on disk, they are two different
        // directories, and `parent` is the one that is NOT there. See that
        // function's own doc comment for the measured case this fixes.
        if let Some((prefix, why)) = resolve_ancestor(root, parent, &mut ancestor_cache) {
            frozen.push(Frozen {
                prefix: prefix.to_string(),
                why,
            });
        }
    }

    for relative in &known {
        if !should_delete(relative, &seen, &frozen) {
            continue;
        }
        let Some(entry) = db.path_entry(root_id, relative)? else {
            continue;
        };
        // The path's removal and whatever `forget_if_unnamed` decides about
        // its document — and, through that, its vectors, see that
        // function's own doc comment — land together or not at all: a
        // crash between them must not leave a document with zero paths,
        // which is exactly what the randomised harness's first invariant
        // forbids. Deliberately NOT one transaction for the whole loop — a
        // large root would then hold the write lock for as long as the
        // entire reconciliation takes, and `IngestError::Busy` exists
        // because five seconds of that already measured as a problem for
        // one ordinary write elsewhere in this crate.
        db.transaction(|_| {
            db.delete_path(root_id, relative)?;
            // The document goes when its last path does, and only then —
            // the same decision an ordinary edit's displacement already
            // made, through the same function, so there is exactly one
            // place that makes it rather than two that could disagree.
            crate::forget_if_unnamed(db, &entry.document_id)
        })?;
        report.removed += 1;
    }
    // Journal rows for paths that no longer exist go with them, or the
    // journal grows in the one dimension Task 6 did not close. Same `seen`
    // and `frozen` as the loop above: a pre-skip with a path, or a path
    // under a frozen prefix, is not evidence the file is gone, so its
    // journal row must not be forgotten either.
    let frozen_prefixes: Vec<&str> = frozen.iter().map(|f| f.prefix.as_str()).collect();
    db.forget_skips_not_in(root_id, &seen, &frozen_prefixes)?;

    report.frozen = frozen;
    Ok(report)
}

/// The deletion rule, stated once: a `known` path is deleted if and only if
/// it is not something the walk saw or explicitly declined to touch this
/// pass (`seen`), and no frozen prefix covers it. Everything else that has
/// to hold for phase 3 to reach a single path at all — the walk completed,
/// it stopped cleanly, the root itself is not the unmount signature — is
/// checked once, above, before `known` is even read; reaching this
/// function at all already means those held, so this decides only the
/// part that varies path by path.
///
/// Called twice with two different snapshots of `frozen` by design, not by
/// accident: once while `frozen` holds only the symlink producer's entries
/// (to build `missing`, the candidate list for the ancestor-climb
/// producer), and once after both producers have run (the final
/// delete/keep decision in the loop below). See the comment where
/// `missing` is computed for why that first, partial call is safe — the
/// second, complete call is always what actually decides a path's fate.
fn should_delete(relative: &str, seen: &HashSet<&str>, frozen: &[Frozen]) -> bool {
    !seen.contains(relative) && !frozen.iter().any(|f| under(relative, &f.prefix))
}

/// A raw top-level listing of the watched root, with no exclusion rule
/// applied at all — deliberately independent of `Walked`, which cannot tell
/// an excluded entry apart from a nonexistent one. `walk_root`'s only use of
/// this is the root itself, checked once at the top of phase 3; a candidate
/// directory further down uses `resolve_ancestor` instead, which needs a
/// THIRD answer this function cannot give — see that function's own doc
/// comment for why "true on any read failure" is wrong one level down.
///
/// `true` on a read failure too, same as an empty directory: the
/// conservative direction for a check whose whole job is choosing a pause
/// over a guess. Safe here specifically because the watched root's own
/// existence is already established before phase 1 even runs
/// (`!root.is_dir()`, at the top of `walk_root`) — a read failure on it
/// during phase 3 is a race, not the ordinary case `resolve_ancestor` has
/// to tell apart from an ordinary deletion.
fn root_is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true)
}

/// Walks up from `dir` — a candidate directory that may not exist at all —
/// to the shallowest ancestor that does, and says whether reconciliation
/// must freeze everything under `dir` rather than trust its absence as
/// evidence — and, if so, WHICH directory the evidence actually belongs to
/// and why, so the caller can put an accurate prefix and reason on
/// `WalkReport::frozen` rather than a bare bool, or worse, `dir` itself —
/// the returned directory is not always `dir`; see the `NotFound` arm below.
///
/// Four states, where an earlier version of this check had room for only
/// two — folding `NotFound` into "read failure, so empty, so freeze" is
/// exactly the bug this replaces, measured directly: a subdirectory removed
/// outright (`rm -rf`) froze its own contents forever, because `read_dir`
/// on a path that no longer exists returns `Err(NotFound)`, indistinguishable
/// from `Ok` at zero entries under the old check.
///
/// - The directory exists and `read_dir` returns at least one entry: the
///   walk saw real content there, so a `known` path missing from `seen`
///   under it is genuinely gone. Not frozen — `None`.
/// - The directory exists and `read_dir` returns nothing: D33's ambiguity,
///   the same one `root_is_empty` names for the watched root itself, one
///   level down. Frozen — `Some((dir, FrozenReason::EmptyDirectory))`: `dir`
///   itself is the evidence here, since it is the one that read empty.
/// - The directory does not exist (`io::ErrorKind::NotFound`): this is
///   evidence OF deletion, not an absence of evidence, so THIS level
///   answers nothing on its own — the walk continues to the parent
///   directory one level up, and returns THAT call's verdict verbatim,
///   `&str` and all. A caller that freezes `dir` on this arm's answer would
///   name a directory that may not exist on disk at all — measured, probing
///   `mnt/share/2024` for a share unmounted at `mnt` climbed two `NotFound`s
///   to find `mnt` empty, and a version of this function that returned only
///   the `FrozenReason` left the caller with no way to say anything but
///   `mnt/share/2024`, a path nobody could go check by hand. The recursion
///   always terminates: the watched root is the last possible ancestor, and
///   by the time phase 3 reaches this code it is already known to exist and
///   read non-empty (the whole-root unmount check above returned early
///   otherwise), so running out of `/` to split on resolves to "not frozen"
///   directly rather than recursing into a check that would only confirm
///   what is already known.
/// - `read_dir` fails for any other reason (permission denied, some other
///   IO error): frozen — `Some((dir, FrozenReason::UnreadableDirectory))`,
///   the conservative side, same as the old behaviour, kept for exactly the
///   cases that are not "not found." This arm is expected to be rare: a
///   permission failure anywhere under the watched root would normally
///   already have cleared `Walked::complete` during phase 1 and stopped
///   phase 3 from running at all (the gate at the top of phase 3), so
///   reaching it here at all means phase 1 saw the entry as readable and
///   something changed in the moment since.
///
/// `cache` is populated for every directory visited along the way — not
/// only the one finally resolved — to the SAME verdict, so a second
/// candidate that shares any part of this walk-up chain (a sibling file
/// under the same unmounted share, another file under the same deleted
/// tree) resolves from the cache without a second `read_dir` call. This is
/// what keeps the cost proportional to how many distinct directories are
/// actually involved in what went missing, not to how deep any one of them
/// happens to be nested. The cache key is still the directory ASKED about,
/// even though the verdict's own `&str` may name a shallower ancestor —
/// that mismatch is exactly the point: a second candidate under the same
/// nonexistent `mnt/share/2024` hits the cache under that key and gets back
/// `mnt`, without climbing again.
fn resolve_ancestor<'a>(
    root: &Path,
    dir: &'a str,
    cache: &mut HashMap<&'a str, Option<(&'a str, FrozenReason)>>,
) -> Option<(&'a str, FrozenReason)> {
    if let Some(&verdict) = cache.get(dir) {
        return verdict;
    }
    let verdict = match std::fs::read_dir(root.join(dir)) {
        Ok(mut entries) => entries
            .next()
            .is_none()
            .then_some((dir, FrozenReason::EmptyDirectory)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => match dir.rfind('/') {
            Some(i) => resolve_ancestor(root, &dir[..i], cache),
            None => None,
        },
        Err(_) => Some((dir, FrozenReason::UnreadableDirectory)),
    };
    cache.insert(dir, verdict);
    verdict
}

/// Whether `relative` names something inside the subtree `prefix` names —
/// prefix-plus-separator, not a bare string prefix: `"linked_dirs/x"` must not
/// match against `"linked_dir"`.
fn under(relative: &str, prefix: &str) -> bool {
    relative
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// What one call to [`ingest_with_busy_retry`] settled.
enum Retried {
    /// `ingest_file` returned an ordinary outcome, on the first attempt or a
    /// later one.
    Settled(Ingested),
    /// Every attempt still found the index busy; the file was journalled as
    /// a skip without ever reaching `ingest_file`'s content-facing logic.
    /// Kept distinct from `Settled(Ingested::Skipped { .. })` because a
    /// caller counting consecutive environmental skips must not treat this
    /// one as evidence about the worker — see the call site in `walk_root`.
    StillBusy,
    /// Cancelled while waiting out a retry, rather than between files.
    Cancelled,
}

/// Calls [`ingest_file`], retrying up to [`BUSY_RETRIES`] times while the
/// index answers [`IngestError::Busy`]. If every attempt still finds it busy,
/// the file is journalled as an environmental skip rather than left uncounted
/// — [`IngestError::Busy`]'s own doc comment leaves "come back to the file"
/// to whoever owns the walk, and this is that: a bounded number of comebacks,
/// then an honest record instead of a silent gap between `found` and every
/// other counter.
///
/// `cancel` is checked between attempts, not only between files in the
/// caller's own loop: three attempts can span up to fifteen seconds of
/// waiting on one file, and a cancellation that arrived partway through that
/// window should not have to wait it out just because the outer loop only
/// looks between files.
///
/// Recorded with [`SkipRule::Unreadable`] — the closest existing member of
/// the closed vocabulary to "this file could not be read on this pass, for a
/// reason that says nothing about its bytes and may not repeat next time",
/// which is also exactly what a database busy under someone else's write is.
/// `Unreadable` never displaces what a path already names (`displaces`, in
/// this crate's `lib.rs`), so there is nothing to look up here first: a busy
/// database says nothing about whether the file's content changed, only that
/// this pass could not find out.
///
/// If recording the skip *also* meets contention — the same write lock, still
/// held — that failure is not retried again here and propagates out as
/// `IngestError::Busy`, ending the walk. Two independent writes both meeting
/// sustained contention is no longer the "a window added a folder" shape
/// `BUSY_RETRIES` is sized for; surfacing it as an explicit error the caller
/// can retry the whole walk over is more honest than a second unbounded retry
/// loop layered on the first.
fn ingest_with_busy_retry(
    pool: &Pool,
    db: &Db,
    root_id: i64,
    found: &Found,
    cancel: &AtomicBool,
) -> Result<Retried, IngestError> {
    let mut last_busy = None;
    for attempt in 1..=BUSY_RETRIES {
        match ingest_file(
            pool,
            db,
            root_id,
            &found.absolute,
            &found.relative,
            Some(found.on_disk),
        ) {
            Ok(ingested) => return Ok(Retried::Settled(ingested)),
            Err(IngestError::Busy(err)) => {
                last_busy = Some(err);
                if attempt == BUSY_RETRIES {
                    break;
                }
                if cancel.load(Ordering::SeqCst) {
                    return Ok(Retried::Cancelled);
                }
            }
            Err(other) => return Err(other),
        }
    }
    let last_busy = last_busy
        .expect("the loop above only runs out of attempts by taking the Busy arm every time");
    let reason = format!(
        "the index was still busy after {BUSY_RETRIES} attempts to write to it: {last_busy}"
    );
    db.record_skip(
        root_id,
        &found.relative,
        None,
        &reason,
        SkipRule::Unreadable,
        None,
    )?;
    Ok(Retried::StillBusy)
}

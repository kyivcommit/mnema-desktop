// The scan the settings window is watching, as ONE value: what the application
// is doing, what the last scan came to, and what a person may press next.
// DOM-free, the way `launcher/state.ts` is — the sentences are the component's,
// the states are here.
//
// 🔴 **A snapshot, not a stream of edges.** Until this commit the controller
// rebuilt the state from channel events it may never have heard, and everything
// that wanted to draw the scan had a different answer: a window reopened
// mid-job, a section mounted half-way through, and a job that had never run
// were indistinguishable. `scan_state.rs` moved that state into the core; this
// file now does two things only — it keeps the newest `ScanState` it has been
// shown, and it asks the backend to start or stop a scan.
//
// **Where this lives is still a decision, not an accident.** `Settings.svelte`
// creates exactly one of these, above every section, and hands it down: a
// controller created inside a section is destroyed when that section is,
// taking the subscription and the Stop button with it. Three of the four
// sections are destroyed by the next nav click; the fourth, the folders panel,
// is kept mounted and hidden by F10 (Task 10e), and that is the window's
// decision about one section rather than a property this controller may lean
// on — which is why it is created above all four and not inside the one that
// happens to survive today.
import { get, writable, type Readable } from 'svelte/store';
import {
  cancelJob, jobStatus, listenScanProgress, startScanJob,
  type Counts, type Entry, type IndexRead, type OtherJob, type Phase, type ScanState,
} from '../lib/ipc';
import type { Key } from '../i18n/catalog';

/// The state a window holds before anything has told it otherwise, and the
/// exact shape `ScanState::default()` serialises to: idle, nothing counted,
/// nothing read.
///
/// `revision: 0` is the DEFAULT state's own revision, and not a floor that
/// anything can equal: `apply` needs strictly greater, so a state arriving at
/// revision 0 is dropped as stale. That is safe because of a fact about the
/// core rather than about this line — every write to `ScanState` bumps the
/// revision before announcing (`state.rs`) — so no state but the default one
/// can ever carry 0. A core that announced at 0 would be invisible here.
///
/// Not exported: a test asserting against this very constant would agree with
/// the code by construction. `jobs.test.ts` writes the same shape itself.
const NO_SCAN: ScanState = {
  revision: 0,
  files: 0,
  readSeq: 0,
  jobsDone: 0,
  lastReading: null,
  snapshot: { kind: 'idle' },
};

/// What can honestly be said about how far along a run is.
///
/// `total: 0` is not an edge case: a reading pass reports it before phase 1 has
/// counted anything, and a root that could not be entered reports zero of zero
/// for good (`walk_job.rs`). "0 of 0" reads as "nothing to do" while a run is
/// under way, and any expression dividing by it is worse. Nothing here divides.
export type ProgressShape =
  | { kind: 'countingUp'; done: number }
  | { kind: 'ratio'; done: number; total: number };

export function progressShape(counts: Counts): ProgressShape {
  if (counts.total === 0) return { kind: 'countingUp', done: counts.done };
  return { kind: 'ratio', done: counts.done, total: counts.total };
}

/// What a running phase is called, decided once here so `ScanProgress.svelte`
/// (the `<progress>` and its own visible line) and `JobStrip.svelte`'s
/// disclosure summary — two readers of the SAME `phase` — cannot each answer
/// it differently (`jobs.ts` already keeps `continueAction` this way for the
/// strip and the section). The key alone, not the rendered string: this file
/// has no `t()` and does not track `$locale`, so the caller formats it.
///
/// Task 5. `other` used to have no words at all (`scan_state::OtherJob`
/// covers the probe and a model adoption, and nobody asked to start either) —
/// but a disclosure needs a summary line even while one of those holds the
/// slot, so both now get their own sentence rather than an empty clickable
/// row.
export type PhaseLabel = { key: Key; params?: Record<string, unknown> };

const OTHER_JOB_LABEL: Record<OtherJob, Key> = {
  probe: 'indexing_probe_running',
  modelAdoption: 'indexing_model_adoption_running',
};

export function phaseLabel(phase: Phase): PhaseLabel {
  switch (phase.kind) {
    case 'reading':
      // One-based on the wire (`scan_job.rs`: "3 of 7" is what a person
      // reads), so nothing here adds or subtracts one.
      return {
        key: 'indexing_reading_root',
        params: { rootIndex: phase.rootIndex, rootCount: phase.rootCount, rootPath: phase.rootPath },
      };
    case 'embedding':
      return { key: 'indexing_embed_running' };
    case 'removing':
      return { key: 'indexing_removing', params: { rootPath: phase.rootPath } };
    case 'other':
      return { key: OTHER_JOB_LABEL[phase.job] };
  }
}

/// The scan as this window holds it, plus whatever the last command said back.
///
/// `note` is a backend sentence VERBATIM: a rejection crosses the IPC as text
/// (`error.rs`), so nothing here branches on a kind or matches on the words. It
/// is deliberately not part of `ScanState` — the core has no opinion about a
/// command this window sent and had refused.
export type JobState = { scan: ScanState; note: string | null };

export type JobController = {
  state: Readable<JobState>;
  /// Starts one scan from one of its two entry points. `full` reads every
  /// watched folder and then embeds; `embedOnly` is the resumption for a scan
  /// whose reading pass already finished (`scan_state::Entry`).
  scan(entry: Entry): Promise<void>;
  cancel(): Promise<void>;
  /// Opens the subscription and takes the first snapshot. Synchronous, and the
  /// `destroy` it returns is synchronous too, because Svelte's `onMount` will
  /// take either but only calls a returned function — an `async` mount returns
  /// a promise, which Svelte would keep and never call.
  mount(): () => void;
};

/// Which of two states is the newer one, and NOTHING else. Pure, so the rule
/// can be tested without a controller at all.
///
/// 🔴 An equal revision returns `current` **by identity**, which is what lets a
/// caller recognise "nothing newer arrived" with `===` — the observer can
/// announce the same revision twice, because two announcements race and either
/// may arrive first (`state::JobObserver`). Identity is the SIGNAL and not the
/// remedy: what actually spares the subscribers is `absorb` below, which does
/// not write the store at all when this returns what it was given.
///
/// Compared by revision and never field by field: two reads equal in every
/// field are not evidence that nothing happened in between, which is the whole
/// reason `ScanState::revision` exists.
export function apply(current: ScanState, incoming: ScanState): ScanState {
  return incoming.revision > current.revision ? incoming : current;
}

/// What a person may press to carry on, and WHERE the offer belongs.
///
/// `where` is not decoration. The strip is the window's status line and speaks
/// about the job that just ended; the section speaks about the state the index
/// is in. A state that is both — a cancelled scan whose report names its
/// resumption, over an index still carrying the marker — owes ONE offer, and
/// this function is the single place that decides which.
export type ContinueAction = { entry: Entry; where: 'strip' | 'section'; label: 'resume' | 'retry' };

/// D-m's table, pure, in one place so that the strip and the section cannot
/// each answer it and disagree.
///
/// The order of the arms is the decision:
///
/// - A running scan offers nothing. The index's markers are still set while a
///   scan is under way — they are cleared by the pass that finishes, not by the
///   one that starts — so a table reading them first would offer a second scan
///   over the one already going.
/// - A report that NAMES its resumption wins over the markers. It is the
///   narrower fact: `scan_job::resume_for` decided it from the phase the scan
///   actually ended in, and the marker only says something is owed.
/// - `scanIncomplete` outranks the queue. `embedOnly` over a half-read archive
///   would embed what is there and leave the unread half invisible while the
///   window said the work was done.
///
/// `label` follows the reason and not the entry point: a person who pressed
/// Stop is resuming, a person whose scan failed is retrying, and one word for
/// both makes a failure read as their own doing.
export function continueAction(state: ScanState, read: IndexRead | null): ContinueAction | null {
  const snapshot = state.snapshot;
  if (snapshot.kind === 'running') return null;
  if (snapshot.kind === 'ended' && snapshot.report.resume !== null) {
    return {
      entry: snapshot.report.resume,
      where: 'strip',
      label: snapshot.report.reason === 'cancelled' ? 'resume' : 'retry',
    };
  }
  if (read === null) return null;
  if (read.scanIncomplete) return { entry: 'full', where: 'section', label: 'resume' };
  if (read.pendingChunks > 0) return { entry: 'embedOnly', where: 'section', label: 'resume' };
  return null;
}

const sentenceOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function createJobController(): JobController {
  const store = writable<JobState>({ scan: NO_SCAN, note: null });

  // 🔴 The store is not written AT ALL when nothing is newer, and returning the
  // same object from `update` is not the same thing: Svelte's `safe_not_equal`
  // treats every object as changed, so a store handed back its own value still
  // wakes every subscriber. The read and the write are one step — there is no
  // `await` between them and nothing else on this thread to interleave.
  //
  // 🔴 **`fromEvent` decides whether the standing sentence dies with this
  // state, and the two callers are not interchangeable.** `note` is written by
  // every rejection and used to be cleared in exactly one place — the top of
  // `scan()` — so a refused Stop sat on the strip beneath a live
  // `indexing_reading_root` line until the person pressed Scan, and a refused first `jobStatus`
  // sat there for the life of the window while every state after it arrived
  // correctly. `Settings.svelte` states the opposite rule for its own banner in
  // this same window, and paid a fix round for it: a successful read takes the
  // failure sentence away, because a sentence that outlives the state it
  // describes is this project's own dominant late-PR class.
  //
  // It cannot simply be cleared on every applied state. `scan()`'s error path
  // says its sentence and then re-reads `job_status` for itself, and that
  // re-read applies a newer state immediately — so a blanket clear would delete
  // the sentence about the press before anybody could read it, which is what
  // `a re-read refused after a refused scan keeps the sentence about the press`
  // exists to protect. The distinction is whose statement the state is: an
  // EVENT is the core speaking about the world, and it postdates whatever was
  // refused; a re-read is this window answering its own question.
  //
  // Cleared only when the state is actually taken, so «nothing newer arrived»
  // stays one fact rather than two: an event carrying a revision this window
  // has already seen says nothing new and takes nothing away.
  const absorb = (incoming: ScanState, fromEvent = false) => {
    const current = get(store);
    const scan = apply(current.scan, incoming);
    if (scan === current.scan) return;
    store.set({ ...current, scan, note: fromEvent ? null : current.note });
  };

  const say = (e: unknown) => store.update((s) => ({ ...s, note: sentenceOf(e) }));

  function mount(): () => void {
    // Everything below is guarded by this rather than by unsubscribing: the
    // subscription and the snapshot are both in flight when `destroy` can first
    // be called, and neither can be recalled once asked for.
    let destroyed = false;
    let unlisten: (() => void) | null = null;

    const destroy = () => {
      destroyed = true;
      const fn = unlisten;
      unlisten = null;
      if (fn !== null) fn();
    };

    // `async` so that a wrapper which throws SYNCHRONOUSLY — one that is
    // `undefined`, or that answers with something that is not a promise — comes
    // back as a rejection this `catch` can turn into a sentence. Called bare,
    // that throw would escape `mount`, and `mount` is what `onMount` calls: the
    // whole window would fail to render over a boundary that only failed to
    // answer.
    const readSnapshot = () => {
      void (async () => {
        const state = await jobStatus();
        if (!destroyed) absorb(state);
      })().catch((e) => { if (!destroyed) say(e); });
    };

    // 🔴 **Three things reach this controller, and each covers what the one
    // before it cannot.**
    //
    // The FIRST read, started here and waiting on nothing, is the fast paint: a
    // window opened mid-scan draws the run without waiting for a dynamic import
    // to resolve. It is started independently of the subscription because
    // `listenScanProgress` awaits that import, and a promise that never settles
    // is neither a rejection nor a resolution — chained behind it, this read
    // would never happen at all, and the window would say nothing whatever
    // about a scan that is running, with no timeout and nothing else that asks.
    //
    // The SECOND read, inside the `.then` below, closes the gap that
    // independence opens — and the gap is real. `apply` sorts two states that
    // both ARRIVE; `scan-progress` is a fire-and-forget `handle.emit`
    // (`lib.rs`) with no replay and no retained last value, so an emission
    // landing between the first read and the moment the listener finishes
    // registering is delivered to nobody and is in no reply either. During a run
    // the next progress tick corrects that. The LAST emission of a job does not:
    // a window opened just as a scan ends would keep a running strip with a Stop
    // that `cancel_job` will refuse, and the sections would never take their
    // ending re-read. The duplicate costs nothing — the second answer is either
    // newer, or the same revision and dropped by `apply` on identity.
    //
    // The EVENTS cover everything after that.
    //
    // This is where the controller parts company with `bootLocale`
    // (`i18n/index.ts`), which registers before it asks and asks once: a locale
    // reply carries no version, so order is the only thing that can say which of
    // two answers is newer, and deferring that read is free because a window
    // with no locale yet has not painted. Neither holds here.
    readSnapshot();

    void listenScanProgress((incoming) => {
      if (destroyed) return;
      // `true`: this is the core speaking, and it postdates any rejection still
      // on screen. See `absorb`.
      absorb(incoming, true);
    })
      .then((fn) => {
        // `destroy` may already have run, with nothing to call. The unlisten is
        // used the moment it arrives instead, so a section switch during boot
        // does not leave a listener on the window for the life of the process —
        // and a window that has gone has nothing to re-read for.
        if (destroyed) {
          fn();
          return;
        }
        unlisten = fn;
        readSnapshot();
      })
      // Trailing, so it covers the handler above as well as the subscription
      // itself. A subscription that could not be registered is a sentence like
      // any other rejection.
      .catch((e) => { if (!destroyed) say(e); });

    return destroy;
  }

  async function scan(entry: Entry) {
    // The sentence a previous press earned is about that press. Cleared before
    // this one is sent, so a refusal left on screen cannot be read as this
    // scan's own answer.
    store.update((s) => ({ ...s, note: null }));
    try {
      await startScanJob(entry);
    } catch (e) {
      say(e);
      // 🔴 The decision comes from the re-read, never from the sentence. A
      // rejection is text (`error.rs`), and the commonest reason this command
      // is refused is that another job holds the slot — that job's state is
      // exactly what this window must go on drawing, and the event for it may
      // already have gone by.
      //
      // A re-read that is itself refused is swallowed on purpose: the sentence
      // a person needs is the one about the press they made, and replacing it
      // with a sentence about a question nobody asked would take the reason the
      // scan would not start off the screen.
      try {
        absorb(await jobStatus());
      } catch {
        /* the sentence above stands */
      }
    }
  }

  async function cancel() {
    try {
      await cancelJob();
    } catch (e) {
      say(e);
    }
    // Nothing is written on success: the job reports its own stop through the
    // observer, and a state invented here would be a claim about a slot this
    // window has not read.
  }

  return { state: { subscribe: store.subscribe }, scan, cancel, mount };
}

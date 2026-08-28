// The job the settings window is watching, as a value: what a walk or an
// embedding pass is doing, what it ended as, and what the window may say about
// it. DOM-free, the way `launcher/state.ts` is — the sentences are the
// component's, the states are here.
//
// **Where this lives is a decision, not an accident.** The channel a job
// reports on belongs to the page that started it (`bridge.rs`), so a controller
// created inside a section is destroyed the moment somebody clicks another one
// — taking the counters AND the Cancel button with it. `cancel_job` needs no
// channel at all, so that Cancel would be lost for nothing. `Settings.svelte`
// creates exactly one of these, above every section, and hands it down.
import { get, writable, type Readable } from 'svelte/store';
import {
  cancelJob, jobStatus, modelSettings, startEmbedJob, startWalkJob,
  type Frozen, type JobEnded, type JobEvent,
} from '../lib/ipc';

/// Which pass reported. The two are not interchangeable on screen: a walk reads
/// ONE folder, an embedding pass covers the whole index and takes no root
/// (`embed_job.rs`), so a sentence written for one lies about the other.
export type JobPass = 'walk' | 'embed';

/// What a job ended as — eight, not seven, and the extra one is the whole point
/// of reading `Ended.complete`.
///
/// The four after `failed` are carried across by name because they are not
/// malfunctions: `job.rs` says reporting them as `failed` tells a person
/// something broke when instead a folder is unreadable, an exclusion rule did
/// not take, or a volume may have gone missing.
export type JobOutcome =
  | { kind: 'completed' }
  /// `reason: completed` with `complete: false`: phase 2 finished everything
  /// phase 1 could hand it, but phase 1 never saw the whole tree — so whatever
  /// the person deleted under the unreadable part is STILL searchable
  /// (`job.rs`). "Done" is the one word this must not be.
  | { kind: 'partlyRead' }
  | { kind: 'cancelled' }
  /// A panic, a broken pool, a worker binary that would not start. `message` is
  /// the text that tells those apart; `null` is a shape the wire permits
  /// (`Option<String>`) and the screen has to survive.
  | { kind: 'failed'; message: string | null }
  | { kind: 'brokenWorker' }
  | { kind: 'rulesNotApplied' }
  | { kind: 'rootUnavailable' }
  | { kind: 'volumeMissing' };

export type OutcomeKind = JobOutcome['kind'];

/// Every outcome, for the tables that must cover all of them. A closed set,
/// unlike an API surface: it is exactly the seven wire reasons plus the split
/// `complete` makes in one of them.
export const OUTCOME_KINDS = [
  'completed', 'partlyRead', 'cancelled', 'failed',
  'brokenWorker', 'rulesNotApplied', 'rootUnavailable', 'volumeMissing',
] as const satisfies readonly OutcomeKind[];

/// No wildcard arm: a variant added to `EndReason` fails to type-check here,
/// which is what `job.rs`'s own pinning test asks the window side to do.
///
/// `complete` splits `completed` and NOTHING else. `Ended::failed` sets
/// `complete: false` on every failure — it has no `WalkReport` to read it from
/// — so a reducer keying the partly-read state off that field alone would
/// report a broken pass as a half-read folder.
export function outcomeOf(ended: JobEnded): JobOutcome {
  switch (ended.reason) {
    case 'completed': return ended.complete ? { kind: 'completed' } : { kind: 'partlyRead' };
    case 'cancelled': return { kind: 'cancelled' };
    case 'failed': return { kind: 'failed', message: ended.message };
    case 'brokenWorker': return { kind: 'brokenWorker' };
    case 'rulesNotApplied': return { kind: 'rulesNotApplied' };
    case 'rootUnavailable': return { kind: 'rootUnavailable' };
    case 'volumeMissing': return { kind: 'volumeMissing' };
  }
}

/// An ending with the counts kept apart the way the wire keeps them: `indexed`
/// and `unchanged` are not one number (a run that wrote a hundred documents and
/// a run that found a hundred already there are not the same run), and `frozen`
/// is here because `removed == 0` alone cannot say whether anything was
/// silently left untouched (`job.rs`).
export type Ending = {
  outcome: JobOutcome;
  done: number; total: number; skipped: number; refused: number;
  indexed: number; unchanged: number; removed: number;
  frozen: Frozen[];
};

export function endingOf(ended: JobEnded): Ending {
  return {
    outcome: outcomeOf(ended),
    done: ended.done, total: ended.total, skipped: ended.skipped, refused: ended.refused,
    indexed: ended.indexed, unchanged: ended.unchanged, removed: ended.removed,
    frozen: ended.frozen,
  };
}

/// Whether a walk that ended this way is worth embedding.
///
/// `partlyRead` chains: the part that WAS read is real work, and refusing to
/// embed it because a subfolder could not be opened would leave the person with
/// documents in the index that content search cannot answer for. The partly-read
/// sentence stays on screen alongside — the chaining decision does not soften
/// what the walk reported.
export function chainsEmbedPass(kind: OutcomeKind): boolean {
  return kind === 'completed' || kind === 'partlyRead';
}

export type Counts = {
  done: number; total: number; skipped: number; refused: number;
  /// `Option<u64>` on the Rust side, so `null` for the whole of an ordinary
  /// run's beginning: "not known yet" is a real state and must not render as 0.
  secondsLeft: number | null;
};

/// What can honestly be said about how far along a run is.
///
/// `total: 0` is not an edge case: a walk reports it before phase 1 has counted
/// anything, and a root that could not be entered reports zero of zero for good
/// (`walk_job.rs`). "0 of 0" reads as "nothing to do" while a run is under way,
/// and any expression dividing by it is worse. Nothing here divides.
export type ProgressShape =
  | { kind: 'countingUp'; done: number }
  | { kind: 'ratio'; done: number; total: number };

export function progressShape(counts: Counts): ProgressShape {
  if (counts.total === 0) return { kind: 'countingUp', done: counts.done };
  return { kind: 'ratio', done: counts.done, total: counts.total };
}

export type JobPhase =
  | { kind: 'idle' }
  /// `job_status` said a job is running and this window has no channel for it —
  /// the settings window was reopened mid-run, or another section took the slot
  /// (`set_embedding_model` holds it without ever sending an ending). There are
  /// no counts to draw and none will arrive, so this is deliberately NOT a
  /// progress line: a bar fed from a boolean is one that never finishes.
  /// Cancel is still offered, because `cancel_job` needs no channel.
  | { kind: 'runningUnobserved' }
  | { kind: 'starting'; pass: JobPass }
  | { kind: 'running'; pass: JobPass; counts: Counts }
  | { kind: 'ended'; pass: JobPass; ending: Ending };

/// Something the window has to say beside the phase. `noKey`/`noModel` are the
/// window's own pre-check, read from `model_settings`: the walk still ran
/// — text search needs neither — and the section says in words which one is
/// absent. `rejected` carries a backend sentence VERBATIM: a rejection crosses
/// the IPC as text (`error.rs`), so nothing here branches on a kind or matches
/// on the words.
export type JobNote =
  | { kind: 'noKey' }
  | { kind: 'noModel' }
  | { kind: 'rejected'; sentence: string };

/// `walk` is held apart from `phase` on purpose: when a read folder chains the
/// embedding pass, the phase moves on to that pass while the walk's own ending
/// — including a partly-read one — must stay on screen.
export type JobState = { phase: JobPhase; walk: Ending | null; note: JobNote | null };

export type JobController = {
  state: Readable<JobState>;
  /// Runs the walk for ONE watched root, then chains the embedding pass if the
  /// folder was read and both preconditions hold.
  scan(rootId: number): Promise<void>;
  cancel(): Promise<void>;
  /// Asks the backend whether a job is running. Only ever writes over `idle` or
  /// `runningUnobserved` — see the guard's own comment.
  syncFromStatus(): Promise<void>;
};

const sentenceOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function createJobController(): JobController {
  const store = writable<JobState>({ phase: { kind: 'idle' }, walk: null, note: null });

  function onEvent(pass: JobPass, event: JobEvent) {
    if (event.event === 'progress') {
      store.update((s) => ({ ...s, phase: { kind: 'running', pass, counts: event.data } }));
      return;
    }
    const ending = endingOf(event.data);
    store.update((s) => ({
      ...s,
      phase: { kind: 'ended', pass, ending },
      walk: pass === 'walk' ? ending : s.walk,
    }));
    if (pass === 'walk' && chainsEmbedPass(ending.outcome.kind)) void chain();
  }

  // The window checks BOTH preconditions itself and names the one that is
  // absent, so on the ordinary path neither of the backend's own refusals is
  // reached. `start_embed_job` rejects on a missing key before it claims the
  // slot; a missing MODEL it does not check at all, and that refusal arrives as
  // an ending carrying a sentence. Those two are the second line, for the state
  // that changed between this read and the call — both still happen, and both
  // still reach the screen.
  async function chain() {
    let settings;
    try {
      settings = await modelSettings();
    } catch (e) {
      store.update((s) => ({ ...s, note: { kind: 'rejected', sentence: sentenceOf(e) } }));
      return;
    }
    if (settings.key.kind !== 'present') {
      store.update((s) => ({ ...s, note: { kind: 'noKey' } }));
      return;
    }
    // `typeof === 'string'` rather than a truthiness or null check: a field
    // renamed away on the wire reads as `undefined`, and the safe side of that
    // mistake is "no model chosen", never "chosen".
    if (settings.index.kind !== 'read' || typeof settings.index.embeddingModel !== 'string') {
      store.update((s) => ({ ...s, note: { kind: 'noModel' } }));
      return;
    }
    store.update((s) => ({ ...s, phase: { kind: 'starting', pass: 'embed' } }));
    try {
      await startEmbedJob((event) => onEvent('embed', event));
    } catch (e) {
      store.update((s) => ({
        ...s,
        phase: s.phase.kind === 'starting' ? { kind: 'idle' } : s.phase,
        note: { kind: 'rejected', sentence: sentenceOf(e) },
      }));
    }
  }

  async function scan(rootId: number) {
    store.set({ phase: { kind: 'starting', pass: 'walk' }, walk: null, note: null });
    try {
      await startWalkJob(rootId, (event) => onEvent('walk', event));
    } catch (e) {
      // Only if nothing has reported yet: a refused command claimed no slot, but
      // a job that has already sent an event owns the phase.
      store.update((s) => ({
        ...s,
        phase: s.phase.kind === 'starting' ? { kind: 'idle' } : s.phase,
        note: { kind: 'rejected', sentence: sentenceOf(e) },
      }));
      // 🔴 The line above has just destroyed `runningUnobserved`, and the
      // commonest reason this command is refused is that the very job that
      // state described still holds the slot. Nothing else ever restores it —
      // `syncFromStatus` is called once, at mount — so a single press would
      // otherwise cost a person the Cancel button for the life of the window,
      // which is the one failure they cannot work around. The re-read writes
      // only over `idle`/`runningUnobserved`, so it cannot clobber a pass this
      // window is actually watching.
      await syncFromStatus();
    }
  }

  async function cancel() {
    try {
      await cancelJob();
    } catch (e) {
      store.update((s) => ({ ...s, note: { kind: 'rejected', sentence: sentenceOf(e) } }));
      return;
    }
    // A job this window is watching reports its own stop on the channel, and
    // that ending carries counts nothing here could invent. A job it cannot
    // hear will report to nobody, so its state has to be asked for.
    if (get(store).phase.kind === 'runningUnobserved') await syncFromStatus();
  }

  async function syncFromStatus() {
    // Reads only where it cannot destroy something better. A window watching a
    // pass of its own has live counts and a working Cancel; overwriting those
    // with a boolean — on every remount, which is every section switch — is the
    // mutation that costs a person the button they cannot work around.
    const observing = (s: JobState) => s.phase.kind === 'idle' || s.phase.kind === 'runningUnobserved';
    if (!observing(get(store))) return;
    let running: boolean;
    try {
      running = (await jobStatus()).running;
    } catch (e) {
      store.update((s) => ({ ...s, note: { kind: 'rejected', sentence: sentenceOf(e) } }));
      return;
    }
    // Asked again after the await: a scan may have started while it was in
    // flight, and this answer is then already about the wrong moment.
    store.update((s) => (observing(s)
      ? { ...s, phase: running ? { kind: 'runningUnobserved' } : { kind: 'idle' } }
      : s));
  }

  return { state: { subscribe: store.subscribe }, scan, cancel, syncFromStatus };
}

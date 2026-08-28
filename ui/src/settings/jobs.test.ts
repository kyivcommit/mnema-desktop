import { expect, test, vi, beforeEach } from 'vitest';
import { get } from 'svelte/store';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { END_REASONS, FROZEN_REASONS, type JobEnded, type JobEvent } from '../lib/ipc';
import {
  createJobController, outcomeOf, endingOf, chainsEmbedPass, progressShape,
  OUTCOME_KINDS, type OutcomeKind,
} from './jobs';

// Only Tauri's own module is faked: the real `ipc.ts` wrappers run, so the
// command names, the `rootId` argument and the channel plumbing are exercised
// here rather than mocked away. `jobs.ts` never touches the DOM, and neither
// does this file — the way `launcher/state.ts` is tested.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...a: unknown[]) => invoke(...a),
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

// ---------------------------------------------------------------------------
// Real payloads. EVERY envelope below was printed by a temporary Rust test that
// built the `Ended` through `walk_job.rs`'s own `ended_from_report` — one per
// `StopReason` — and through `job::Ended::failed`, then serialized
// `JobEvent::Ended(..)`. Nothing here was written from a document: a
// hand-written type and a hand-written fixture can carry the same mistake and
// agree with each other while the real event lands in a branch neither
// describes.
// ---------------------------------------------------------------------------
const REAL: Record<string, JobEvent> = {
  completed: { event: 'ended', data: { reason: 'completed', done: 11, total: 11, skipped: 5, complete: true, frozen: [], indexed: 5, unchanged: 1, refused: 0, removed: 4, message: null } },
  cancelled: { event: 'ended', data: { reason: 'cancelled', done: 11, total: 11, skipped: 5, complete: true, frozen: [], indexed: 5, unchanged: 1, refused: 0, removed: 4, message: null } },
  brokenWorker: { event: 'ended', data: { reason: 'brokenWorker', done: 11, total: 11, skipped: 5, complete: true, frozen: [], indexed: 5, unchanged: 1, refused: 0, removed: 4, message: null } },
  rulesNotApplied: { event: 'ended', data: { reason: 'rulesNotApplied', done: 11, total: 11, skipped: 5, complete: true, frozen: [], indexed: 5, unchanged: 1, refused: 0, removed: 4, message: null } },
  rootUnavailable: { event: 'ended', data: { reason: 'rootUnavailable', done: 11, total: 11, skipped: 5, complete: true, frozen: [], indexed: 5, unchanged: 1, refused: 0, removed: 4, message: null } },
  volumeMissing: { event: 'ended', data: { reason: 'volumeMissing', done: 11, total: 11, skipped: 5, complete: true, frozen: [], indexed: 5, unchanged: 1, refused: 0, removed: 4, message: null } },
  partlyRead: { event: 'ended', data: { reason: 'completed', done: 11, total: 11, skipped: 5, complete: false, frozen: [{ prefix: 'notes/archive', reason: 'unreadableDirectory' }, { prefix: 'notes/link', reason: 'symlinkedSubtree' }, { prefix: 'notes/void', reason: 'emptyDirectory' }], indexed: 5, unchanged: 1, refused: 0, removed: 4, message: null } },
  failedWithMessage: { event: 'ended', data: { reason: 'failed', done: 7, total: 11, skipped: 0, complete: false, frozen: [], indexed: 0, unchanged: 0, refused: 0, removed: 0, message: 'the worker binary could not be started' } },
  failedNoMessage: { event: 'ended', data: { reason: 'failed', done: 7, total: 11, skipped: 0, complete: false, frozen: [], indexed: 0, unchanged: 0, refused: 0, removed: 0, message: null } },
  progress: { event: 'progress', data: { done: 3, total: 8, skipped: 1, refused: 0, secondsLeft: null } },
};

function ended(name: keyof typeof REAL): JobEnded {
  const e = REAL[name];
  if (e.event !== 'ended') throw new Error(`${name} is not an ending`);
  return e.data;
}

// ---------------------------------------------------------------------------
// The mirror: derived from the Rust source at test time, never hand-copied.
// Lifted from `Models.test.ts`'s `rustEnumVariants`, including its fix —
// comments are stripped BEFORE the brace walk, or a `}` inside a doc comment
// truncates the enum body and the test reports green having never seen the
// variants past it. `job.rs`'s `EndReason` doc comment is exactly that shape:
// it names `walk_job.rs` and carries prose full of punctuation.
// ---------------------------------------------------------------------------
const HERE = dirname(fileURLToPath(import.meta.url));
const JOB_RS = readFileSync(join(HERE, '../../../src-tauri/src/job.rs'), 'utf8');

function rustEnumVariants(rawSource: string, enumName: string): string[] {
  const source = rawSource.split('\n').map((line) => line.replace(/\/\/.*$/, '')).join('\n');
  const m = new RegExp(`pub enum ${enumName}\\s*\\{`).exec(source);
  if (!m) throw new Error(`enum ${enumName} not found in job.rs — has it moved or been renamed?`);
  let depth = 1;
  let i = m.index + m[0].length;
  const start = i;
  while (depth > 0) {
    if (source[i] === '{') depth++;
    else if (source[i] === '}') depth--;
    i++;
    if (i > source.length) throw new Error(`ran off the end of job.rs looking for the closing brace of ${enumName}`);
  }
  const body = source.slice(start, i - 1);
  if (/#\[serde\([^)]*\brename\s*=/.test(body)) {
    throw new Error(
      `${enumName} now carries an explicit #[serde(rename = "…")] on a variant. This mirror derives ` +
      'wire names with serde\'s CamelCase rule alone and cannot express a rename.',
    );
  }
  const variants: string[] = [];
  let d = 0;
  let cur = '';
  for (const ch of body) {
    if (ch === '{') d++;
    if (ch === '}') d--;
    if (ch === ',' && d === 0) {
      if (cur.trim()) variants.push(cur.trim());
      cur = '';
    } else {
      cur += ch;
    }
  }
  if (cur.trim()) variants.push(cur.trim());
  return variants.map((v) => {
    const name = /^([A-Za-z0-9_]+)/.exec(v.trim());
    if (!name) throw new Error(`could not parse a variant name out of: ${v}`);
    return name[1];
  });
}

const camelOf = (pascal: string) => pascal.charAt(0).toLowerCase() + pascal.slice(1);

test('the parser reads a variant hidden behind a doc comment carrying a lone brace', () => {
  const fixture = `
/// A doc comment with a lone } in it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Sample {
    First,
    /// A brace } here truncated the body before comments were stripped first.
    Second,
}
`;
  expect(rustEnumVariants(fixture, 'Sample')).toEqual(['First', 'Second']);
});

// Both directions in one comparison: a variant Rust gains and this window has
// never heard of fails here, and so does one this window still lists after Rust
// has dropped it.
test('END_REASONS is exactly what job.rs defines, in the spelling serde sends', () => {
  expect([...END_REASONS].sort()).toEqual(rustEnumVariants(JOB_RS, 'EndReason').map(camelOf).sort());
  expect(END_REASONS.length).toBe(7);
});

test('FROZEN_REASONS is exactly what job.rs defines, in the spelling serde sends', () => {
  expect([...FROZEN_REASONS].sort()).toEqual(rustEnumVariants(JOB_RS, 'FrozenReason').map(camelOf).sort());
});

// ---------------------------------------------------------------------------
// The reducer.
// ---------------------------------------------------------------------------

// The cardinality claim. It is NOT the claim that the screen says seven
// different things — that one belongs to the component matrix, because a
// reducer returning `{ kind: 'failed', reason }` for the four variants after
// `failed` yields distinct objects and still collapses four sentences into one.
test('each of the seven wire reasons becomes an outcome of its own', () => {
  const kinds = END_REASONS.map((reason) => outcomeOf({ ...ended('completed'), reason }).kind);
  expect(kinds).toEqual(['completed', 'cancelled', 'failed', 'brokenWorker', 'rulesNotApplied', 'rootUnavailable', 'volumeMissing']);
  expect(new Set(kinds).size).toBe(END_REASONS.length);
});

test('`completed` with complete:false is a different outcome from `completed` with complete:true', () => {
  expect(outcomeOf(ended('completed')).kind).toBe('completed');
  expect(outcomeOf(ended('partlyRead')).kind).toBe('partlyRead');
});

// The other direction of the same rule, and the one a fixture that always sets
// the two together cannot see: `Ended::failed` sets `complete: false` on every
// failure, so a reducer keying `partlyRead` off `complete` alone would report a
// broken pass as a partly-read folder.
test('complete:false on a reason other than `completed` is not a partly-read folder', () => {
  expect(ended('failedWithMessage').complete).toBe(false);
  expect(outcomeOf(ended('failedWithMessage')).kind).toBe('failed');
  expect(outcomeOf({ ...ended('brokenWorker'), complete: false }).kind).toBe('brokenWorker');
});

test('a failure carries its message, and carries its absence too', () => {
  const withText = outcomeOf(ended('failedWithMessage'));
  expect(withText).toEqual({ kind: 'failed', message: 'the worker binary could not be started' });
  expect(outcomeOf(ended('failedNoMessage'))).toEqual({ kind: 'failed', message: null });
});

test('an ending keeps the counts the window has to report, not only `done`', () => {
  expect(endingOf(ended('completed'))).toEqual({
    outcome: { kind: 'completed' },
    done: 11, total: 11, skipped: 5, refused: 0, indexed: 5, unchanged: 1, removed: 4, frozen: [],
  });
  // `frozen` crosses whole: `removed == 0` alone cannot say whether anything
  // was silently left untouched (job.rs), and this is the field that can.
  expect(endingOf(ended('partlyRead')).frozen).toEqual([
    { prefix: 'notes/archive', reason: 'unreadableDirectory' },
    { prefix: 'notes/link', reason: 'symlinkedSubtree' },
    { prefix: 'notes/void', reason: 'emptyDirectory' },
  ]);
});

// Exhaustive over the outcome kinds, both directions: the two that chain and
// the six that do not, named rather than counted.
test('only a folder that was read — in full or in part — chains the embedding pass', () => {
  const chaining: OutcomeKind[] = [];
  const notChaining: OutcomeKind[] = [];
  for (const kind of OUTCOME_KINDS) {
    (chainsEmbedPass(kind) ? chaining : notChaining).push(kind);
  }
  expect(chaining).toEqual(['completed', 'partlyRead']);
  expect(notChaining).toEqual(['cancelled', 'failed', 'brokenWorker', 'rulesNotApplied', 'rootUnavailable', 'volumeMissing']);
});

// `total: 0` is the first thing a real folder breaks — a walk reports it before
// phase 1 has counted anything, and `RootUnavailable` reports zero of zero for
// good. Nothing here divides by it; the shape says which sentence can be told.
test('a run with nothing counted yet states no ratio, and one with a total does', () => {
  expect(progressShape({ done: 0, total: 0, skipped: 0, refused: 0, secondsLeft: null }))
    .toEqual({ kind: 'countingUp', done: 0 });
  expect(progressShape({ done: 4, total: 0, skipped: 0, refused: 0, secondsLeft: null }))
    .toEqual({ kind: 'countingUp', done: 4 });
  expect(progressShape({ done: 3, total: 8, skipped: 1, refused: 0, secondsLeft: null }))
    .toEqual({ kind: 'ratio', done: 3, total: 8 });
});

// ---------------------------------------------------------------------------
// The controller: the job state, and where it lives.
// ---------------------------------------------------------------------------

const KEY_AND_MODEL = {
  key: { kind: 'present' },
  index: { kind: 'read', embeddingModel: 'openai/text-embedding-3-small', embeddedChunks: 4, embeddedChunksEverywhere: 4, searchTextArm: true, searchContentArm: true },
  platform: 'mac',
};

type Replies = {
  model_settings?: unknown;
  job_status?: unknown;
  start_walk_job?: unknown;
  start_embed_job?: unknown;
  cancel_job?: unknown;
};

// Per-command replies, so a rejection can be aimed at ONE command. A blanket
// `mockResolvedValue` cannot express "the walk started and the embed pass was
// refused", which is half of what this file has to say.
function replies(r: Replies) {
  invoke.mockImplementation((cmd: string) => {
    const reply = r[cmd as keyof Replies];
    if (reply instanceof Error) return Promise.reject(reply);
    return Promise.resolve(reply);
  });
}

// The channel handed to the last call of `cmd`, so a test can send the events a
// real job would.
function channelOf(cmd: string): (event: JobEvent) => void {
  const call = [...invoke.mock.calls].reverse().find((c) => c[0] === cmd);
  if (!call) throw new Error(`${cmd} was never invoked`);
  const channel = (call[1] as { onProgress: { onmessage: (e: JobEvent) => void } }).onProgress;
  return (event) => channel.onmessage(event);
}

const calls = (cmd: string) => invoke.mock.calls.filter((c) => c[0] === cmd);

beforeEach(() => {
  invoke.mockReset();
  replies({ model_settings: KEY_AND_MODEL, job_status: { running: false } });
});

test('a scan starts the walk for the root it was given', async () => {
  const jobs = createJobController();

  await jobs.scan(7);

  expect(calls('start_walk_job').map((c) => (c[1] as { rootId: number }).rootId)).toEqual([7]);
  expect(get(jobs.state).phase.kind).toBe('starting');
});

test('a progress report becomes a running state carrying every count, `secondsLeft` included', async () => {
  const jobs = createJobController();
  await jobs.scan(7);

  channelOf('start_walk_job')(REAL.progress);

  const phase = get(jobs.state).phase;
  expect(phase).toEqual({
    kind: 'running',
    pass: 'walk',
    counts: { done: 3, total: 8, skipped: 1, refused: 0, secondsLeft: null },
  });
});

// A rejection is a sentence, never a kind (error.rs): nothing here reads its
// shape, and the phase goes back to idle because no job was ever claimed.
test('a refused walk shows the sentence the backend sent and claims no job', async () => {
  replies({ start_walk_job: new Error('another job is already running') });
  const jobs = createJobController();

  await jobs.scan(7);

  expect(get(jobs.state).note).toEqual({ kind: 'rejected', sentence: 'another job is already running' });
  expect(get(jobs.state).phase.kind).toBe('idle');
});

test('a walk that read the folder chains the embedding pass and keeps its own ending', async () => {
  const jobs = createJobController();
  await jobs.scan(7);

  channelOf('start_walk_job')(REAL.completed);
  await vi.waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));

  const state = get(jobs.state);
  expect(state.walk?.outcome.kind).toBe('completed');
  expect(state.phase).toEqual({ kind: 'starting', pass: 'embed' });
  // The pass covers the whole index and takes no root.
  expect(Object.keys(calls('start_embed_job')[0][1] as object)).toEqual(['onProgress']);
});

// Both halves of the partly-read ruling: the read part is real work, so it is
// embedded — AND the folder was only partly read, so that ending stays.
test('a partly read folder is embedded anyway, and stays reported as partly read', async () => {
  const jobs = createJobController();
  await jobs.scan(7);

  channelOf('start_walk_job')(REAL.partlyRead);
  await vi.waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));

  expect(get(jobs.state).walk?.outcome.kind).toBe('partlyRead');
});

test('a walk that stopped for any other reason embeds nothing', async () => {
  for (const name of ['cancelled', 'failedWithMessage', 'brokenWorker', 'rulesNotApplied', 'rootUnavailable', 'volumeMissing'] as const) {
    invoke.mockReset();
    replies({ model_settings: KEY_AND_MODEL, job_status: { running: false } });
    const jobs = createJobController();
    await jobs.scan(7);

    channelOf('start_walk_job')(REAL[name]);
    await vi.waitFor(() => expect(get(jobs.state).phase.kind).toBe('ended'));

    expect(calls('start_embed_job'), name).toHaveLength(0);
  }
});

// D-c's own rule: the window checks BOTH preconditions itself, from
// `model_settings`, and says which one is absent — so on the ordinary path
// neither of the backend's two refusal routes is reached at all.
test('no provider key: the pass is not started, and the window says which is missing', async () => {
  replies({ model_settings: { ...KEY_AND_MODEL, key: { kind: 'absent' } }, job_status: { running: false } });
  const jobs = createJobController();
  await jobs.scan(7);

  channelOf('start_walk_job')(REAL.completed);
  await vi.waitFor(() => expect(get(jobs.state).note).toEqual({ kind: 'noKey' }));

  expect(calls('start_embed_job')).toHaveLength(0);
});

test('no embedding model chosen: the pass is not started, and the missing one is named apart from the key', async () => {
  replies({
    model_settings: { ...KEY_AND_MODEL, index: { ...KEY_AND_MODEL.index, embeddingModel: null } },
    job_status: { running: false },
  });
  const jobs = createJobController();
  await jobs.scan(7);

  channelOf('start_walk_job')(REAL.completed);
  await vi.waitFor(() => expect(get(jobs.state).note).toEqual({ kind: 'noModel' }));

  expect(calls('start_embed_job')).toHaveLength(0);
});

// The second line, for the state that changed between the window's own read and
// the call: `start_embed_job` reads the key and rejects BEFORE claiming the job
// slot, so this route is a rejected command, not an ending.
test('a key that vanished after the check reaches the window as the command`s own sentence', async () => {
  replies({ model_settings: KEY_AND_MODEL, start_embed_job: new Error('no provider key is stored') });
  const jobs = createJobController();
  await jobs.scan(7);

  channelOf('start_walk_job')(REAL.completed);
  await vi.waitFor(() => expect(get(jobs.state).note).toEqual({ kind: 'rejected', sentence: 'no provider key is stored' }));
});

// The other second-line route, and it is a different shape entirely: a missing
// model is NOT checked by `start_embed_job` (embed_job.rs says so), so the
// command is accepted and the refusal arrives as an ending carrying a sentence.
test('a model that vanished after the check arrives as an ending, not as a rejection', async () => {
  const jobs = createJobController();
  await jobs.scan(7);
  channelOf('start_walk_job')(REAL.completed);
  await vi.waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));

  channelOf('start_embed_job')({
    ...REAL.failedWithMessage,
    data: { ...ended('failedWithMessage'), message: 'the index has no active vector space' },
  } as JobEvent);

  const phase = get(jobs.state).phase;
  expect(phase.kind).toBe('ended');
  expect(phase.kind === 'ended' && phase.pass).toBe('embed');
  expect(phase.kind === 'ended' && phase.ending.outcome).toEqual({
    kind: 'failed', message: 'the index has no active vector space',
  });
  // The walk's own ending is still there beside it: two passes, two results.
  expect(get(jobs.state).walk?.outcome.kind).toBe('completed');
});

test('a job the window has no channel for is a state of its own, not a progress line', async () => {
  replies({ job_status: { running: true } });
  const jobs = createJobController();

  await jobs.syncFromStatus();

  expect(get(jobs.state).phase).toEqual({ kind: 'runningUnobserved' });
});

test('no job running leaves the window idle', async () => {
  const jobs = createJobController();

  await jobs.syncFromStatus();

  expect(get(jobs.state).phase).toEqual({ kind: 'idle' });
});

// The mutation this kills is the one that costs a person their Cancel button:
// re-reading `job_status` on every remount and writing whatever it says would
// throw away the live counts of a job this window is watching.
test('a status re-read never overwrites a pass this window is watching', async () => {
  replies({ model_settings: KEY_AND_MODEL, job_status: { running: true } });
  const jobs = createJobController();
  await jobs.scan(7);
  channelOf('start_walk_job')(REAL.progress);

  await jobs.syncFromStatus();

  expect(get(jobs.state).phase.kind).toBe('running');
});

test('a status re-read clears a job that has since finished elsewhere', async () => {
  replies({ job_status: { running: true } });
  const jobs = createJobController();
  await jobs.syncFromStatus();
  expect(get(jobs.state).phase.kind).toBe('runningUnobserved');

  replies({ job_status: { running: false } });
  await jobs.syncFromStatus();

  expect(get(jobs.state).phase.kind).toBe('idle');
});

test('cancelling asks the backend to stop and needs no channel to do it', async () => {
  const jobs = createJobController();
  await jobs.scan(7);
  channelOf('start_walk_job')(REAL.progress);

  await jobs.cancel();

  expect(calls('cancel_job')).toHaveLength(1);
  expect(calls('cancel_job')[0]).toHaveLength(1); // the command name alone
  // The job itself reports the stop; nothing here guesses at it.
  expect(get(jobs.state).phase.kind).toBe('running');
  channelOf('start_walk_job')(REAL.cancelled);
  const phase = get(jobs.state).phase;
  expect(phase.kind === 'ended' && phase.ending.outcome.kind).toBe('cancelled');
});

// A job with no channel of its own sends no ending, so the state it leaves
// behind has to be re-read rather than waited for.
test('cancelling a job the window cannot hear re-reads the status afterwards', async () => {
  replies({ job_status: { running: true } });
  const jobs = createJobController();
  await jobs.syncFromStatus();

  replies({ job_status: { running: false }, cancel_job: undefined });
  await jobs.cancel();

  expect(calls('cancel_job')).toHaveLength(1);
  expect(get(jobs.state).phase.kind).toBe('idle');
});

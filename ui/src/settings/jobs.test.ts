import { expect, test, vi, beforeEach } from 'vitest';
import { get } from 'svelte/store';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  END_REASONS, ENDED_IN, ENTRIES, FROZEN_REASONS, SKIP_WHY_KINDS,
  type Counts, type IndexRead, type ScanReport, type ScanState, type SkipWhy,
} from '../lib/ipc';
import { camelOf, rustEnumVariants } from '../lib/rust-enum';
import { apply, continueAction, createJobController, progressShape } from './jobs';

// Only Tauri's own modules are faked: the real `ipc.ts` wrappers run, so the
// command names, the `entry` argument and the event name are exercised here
// rather than mocked away. `jobs.ts` never touches the DOM, and neither does
// this file — the way `launcher/state.ts` is tested.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...a: unknown[]) => invoke(...a),
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

// `listen` is the whole of what this controller hears. It is faked at the
// module boundary rather than through `ipc.ts`, so `listenScanProgress`'s own
// event NAME and its unwrapping of `e.payload` are exercised too — a wrapper
// that listened to the wrong event, or handed the envelope on instead of the
// payload, passes every test that mocks `ipc.ts` itself.
const listen = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({
  listen: (...a: unknown[]) => listen(...a),
}));

// ---------------------------------------------------------------------------
// Fixtures. Every shape is the one `scan_state.rs` pins as JSON
// (`every_snapshot_has_its_wire_shape_pinned`); the field names are checked
// against that file by the three mirror tests below rather than trusted.
// ---------------------------------------------------------------------------
const COUNTS: Counts = { done: 3, total: 8, skipped: 1, refused: 0, contended: 0, secondsLeft: null };

const IDLE: ScanState = {
  revision: 0, files: 0, readSeq: 0, jobsDone: 0, lastReading: null, snapshot: { kind: 'idle' },
};

const idleAt = (revision: number): ScanState => ({ ...IDLE, revision });

const runningAt = (revision: number): ScanState => ({
  ...IDLE,
  revision,
  snapshot: {
    kind: 'running',
    cancellable: true,
    phase: {
      kind: 'reading', rootIndex: 0, rootCount: 2, rootPath: '/home/a/notes', counts: COUNTS,
    },
  },
});

const report = (over: Partial<ScanReport> = {}): ScanReport => ({
  embedding: { kind: 'notReached' },
  endedIn: 'reading',
  reason: 'completed',
  message: null,
  resume: null,
  ...over,
});

const endedAt = (revision: number, over: Partial<ScanReport> = {}): ScanState => ({
  ...IDLE,
  revision,
  snapshot: { kind: 'ended', report: report(over) },
});

// The `read` arm of `model_settings`, whole — `continueAction` reads two of its
// fields and the type requires the rest, which is the point of them being
// required (`ipc.ts`).
const indexRead = (over: Partial<IndexRead> = {}): IndexRead => ({
  kind: 'read', embeddingModel: 'openai/text-embedding-3-small', chatModel: null,
  embeddedChunks: 4, embeddedChunksEverywhere: 4, totalChunks: 4,
  failedChunks: 0, pendingChunks: 0, indexedFiles: 2, lastIndexedAt: 1_700_000_000,
  scanIncomplete: false, searchTextArm: true, searchContentArm: true,
  ...over,
});

const calls = (cmd: string) => invoke.mock.calls.filter((c) => c[0] === cmd);

type Replies = {
  job_status?: unknown;
  start_scan_job?: unknown;
  cancel_job?: unknown;
};

// Per-command replies, so a rejection can be aimed at ONE command. A blanket
// `mockResolvedValue` cannot express "the scan was refused and the status read
// answered", which is half of what this file has to say.
function replies(r: Replies) {
  invoke.mockImplementation((cmd: string) => {
    const reply = r[cmd as keyof Replies];
    if (reply instanceof Error) return Promise.reject(reply);
    return Promise.resolve(reply);
  });
}

// The `scan-progress` handler the controller registered, as the emitter would
// call it: with an envelope whose `payload` is the whole state.
function emit(state: ScanState) {
  const call = listen.mock.calls.at(-1);
  if (!call) throw new Error('nothing is listening to scan-progress');
  (call[1] as (e: { payload: ScanState }) => void)({ payload: state });
}

// `listen` resolves with the unlisten function, which is what `destroy` owes.
let unlisten: ReturnType<typeof vi.fn>;

beforeEach(() => {
  invoke.mockReset();
  listen.mockReset();
  unlisten = vi.fn();
  listen.mockResolvedValue(unlisten);
  replies({ job_status: IDLE });
});

// Both halves in place: the snapshot has been asked for and the subscription is
// registered. They are started independently, so neither alone says the
// controller is ready to be driven — waiting on `job_status` used to imply the
// listener existed and no longer does.
const started = () => vi.waitFor(() => {
  expect(calls('job_status').length).toBeGreaterThanOrEqual(1);
  expect(listen).toHaveBeenCalled();
});

async function mounted(controller = createJobController()) {
  const destroy = controller.mount();
  await started();
  return { jobs: controller, destroy };
}

// ---------------------------------------------------------------------------
// The mirror: derived from the Rust source at test time, never hand-copied.
// The reader is `lib/rust-enum.ts`, which carries its own tests.
//
// ⚠️ This is HALF of the guard, and the half that cannot see its own blind
// spot. `rustEnumVariants` reads variant NAMES and `camelOf` applies serde's
// camelCase rule to them; neither reads the enum's `#[serde(rename_all = …)]`,
// so switching that attribute to `snake_case` leaves every test in this file
// green while the wire spelling changes underneath. The other half is Rust's
// own pinning tests — `every_end_reason_has_its_camel_case_spelling_pinned`
// (`job.rs`) and `every_snapshot_has_its_wire_shape_pinned` /
// `the_two_entry_points_survive_the_round_trip_under_the_names_the_window_sends`
// (`scan_state.rs`) — which serialize each variant and pin the string. Neither
// closes the gap alone: the pair does.
// ---------------------------------------------------------------------------
const HERE = dirname(fileURLToPath(import.meta.url));
const JOB_RS = readFileSync(join(HERE, '../../../src-tauri/src/job.rs'), 'utf8');
const SCAN_STATE_RS = readFileSync(join(HERE, '../../../src-tauri/src/scan_state.rs'), 'utf8');

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

// 🔴 `Entry` is the one type in this direction — the window SENDS it. A
// spelling this window invented is not drawn wrongly, it is REFUSED by serde as
// an unknown variant, so a resumption becomes an error message
// (`the_two_entry_points_survive_the_round_trip_under_the_names_the_window_sends`).
test('ENTRIES is exactly what scan_state.rs defines, in the spelling serde accepts', () => {
  expect([...ENTRIES].sort()).toEqual(rustEnumVariants(SCAN_STATE_RS, 'Entry').map(camelOf).sort());
  expect(ENTRIES.length).toBe(2);
});

test('ENDED_IN is exactly what scan_state.rs defines, in the spelling serde sends', () => {
  expect([...ENDED_IN].sort()).toEqual(rustEnumVariants(SCAN_STATE_RS, 'EndedIn').map(camelOf).sort());
});

// The runtime list against Rust, and the TypeScript union against the runtime
// list. The `Record` is the second half: a variant added to `SkipWhy` here and
// left out of `SKIP_WHY_KINDS` fails to compile, so the list cannot fall behind
// the union it is supposed to enumerate.
test('SKIP_WHY_KINDS is exactly what scan_state.rs defines, and covers the union', () => {
  expect([...SKIP_WHY_KINDS].sort()).toEqual(rustEnumVariants(SCAN_STATE_RS, 'SkipWhy').map(camelOf).sort());
  const covered: Record<SkipWhy['kind'], true> = { noKey: true, noModel: true, storeUnavailable: true };
  expect(Object.keys(covered).sort()).toEqual([...SKIP_WHY_KINDS].sort());
});

// ---------------------------------------------------------------------------
// `apply` — which of two states is the newer one.
// ---------------------------------------------------------------------------

// The pair: «the state I hold is stale» against «the state that just arrived
// is». Both directions, because a reducer that always took the incoming value
// passes the first assertion and is exactly the defect — a `job_status` reply
// that was true when it was asked lands after an event about a later moment.
test('a higher revision wins and a lower one is ignored', () => {
  const older = idleAt(5);
  const newer = runningAt(7);

  expect(apply(older, newer)).toBe(newer);
  expect(apply(newer, older)).toBe(newer);
});

// The pair: «nothing changed» against «something changed». The emitter can fire
// the same revision twice — two announcements can race (`lib.rs`) — and a
// reducer that returned a fresh object for an unchanged state would wake every
// subscriber on the window for a redraw of what it already holds.
test('an equal revision returns the state that was already held, by identity', () => {
  const held = runningAt(7);
  const twin = runningAt(7);

  expect(apply(held, twin)).toBe(held);
  expect(apply(held, twin)).not.toBe(twin);
});

// ---------------------------------------------------------------------------
// `continueAction` — what a person may press next, and WHERE it belongs.
//
// The table is D-m's, row by row. `where` is not decoration: the strip is the
// window's status line and speaks about the job that just ended, the section
// speaks about the state the index is in — and the last row is the one that
// says an offer must never appear in both places at once.
// ---------------------------------------------------------------------------

// The pair: «a job is running» against «a job has ended». Both directions with
// the SAME markers set, so a table that read the index's markers before the
// snapshot would offer a person a second scan over the one already going.
test('nothing is offered while a scan is running, whatever the index says', () => {
  const marked = indexRead({ scanIncomplete: true, pendingChunks: 9 });

  expect(continueAction(runningAt(3), marked)).toBeNull();
  expect(continueAction(idleAt(3), marked)).toEqual({ entry: 'full', where: 'section', label: 'resume' });
});

// The pair: «the scan was stopped» against «the scan broke». Both end with a
// resumption to offer, and the WORD differs — a person who pressed Stop is
// resuming, a person whose scan failed is retrying, and one word for both makes
// a failure read as their own doing.
test('a report that names its resumption is offered on the strip, worded by the reason', () => {
  expect(continueAction(endedAt(4, { reason: 'cancelled', resume: 'full' }), null))
    .toEqual({ entry: 'full', where: 'strip', label: 'resume' });

  expect(continueAction(endedAt(4, { reason: 'failed', resume: 'embedOnly', endedIn: 'embedding' }), null))
    .toEqual({ entry: 'embedOnly', where: 'strip', label: 'retry' });
});

// The pair: «the report has a resumption» against «it has none, and the index
// carries a marker». The second is the state a tray Stop leaves behind after
// the report has been replaced, and the marker is the only thing that still
// says so.
test('a report with nothing to resume falls through to the index marker', () => {
  const incomplete = indexRead({ scanIncomplete: true });

  expect(continueAction(endedAt(6, { resume: null }), incomplete))
    .toEqual({ entry: 'full', where: 'section', label: 'resume' });
  expect(continueAction(endedAt(6, { reason: 'cancelled', resume: 'full' }), incomplete))
    .toEqual({ entry: 'full', where: 'strip', label: 'resume' });
});

// The pair: «the reading finished and the embedding was refused» against «there
// is nothing queued». A skipped embedding leaves chunks in the queue and no
// resumption in the report, because the pass never ran — the queue count is
// what says the work is still owed.
test('a queue left behind by a skipped embedding is offered in the section', () => {
  const ended = endedAt(8, {
    endedIn: 'embedding',
    embedding: { kind: 'skipped', why: { kind: 'storeUnavailable', message: 'the keychain would not answer' } },
    resume: null,
  });

  expect(continueAction(ended, indexRead({ pendingChunks: 12 })))
    .toEqual({ entry: 'embedOnly', where: 'section', label: 'resume' });
  expect(continueAction(ended, indexRead({ pendingChunks: 0 }))).toBeNull();
});

// The pair: «the archive was only half read» against «it was read and only the
// embedding is owed». The marker wins, and it has to: `embedOnly` over a
// half-read archive would embed what is there and leave the unread half
// invisible while the window says the work is done.
test('an unfinished reading outranks a queue when the index carries both', () => {
  expect(continueAction(idleAt(2), indexRead({ scanIncomplete: true, pendingChunks: 12 })))
    .toEqual({ entry: 'full', where: 'section', label: 'resume' });
  expect(continueAction(idleAt(2), indexRead({ scanIncomplete: false, pendingChunks: 12 })))
    .toEqual({ entry: 'embedOnly', where: 'section', label: 'resume' });
});

// The pair: «there is something owed» against «there is nothing owed». An idle
// index with neither marker offers nothing at all — a Continue button standing
// on a finished index is an invitation to redo work that is done.
test('an idle index owing nothing offers nothing, and one owing something offers it', () => {
  expect(continueAction(idleAt(2), indexRead())).toBeNull();
  expect(continueAction(idleAt(2), null)).toBeNull();
  expect(continueAction(idleAt(2), indexRead({ pendingChunks: 1 })))
    .toEqual({ entry: 'embedOnly', where: 'section', label: 'resume' });
});

// 🔴 The row the whole `where` field exists for. A report that names its
// resumption AND an index carrying the marker are the ordinary state after a
// cancelled reading pass: both are true, and one offer is owed, not two. The
// second assertion is what says the answer is a single place — a table
// returning a list, or the section deciding for itself, would put the same
// button under the strip and inside the section at once.
test('a resumption in the report and a marker in the index are one offer, on the strip', () => {
  const both = continueAction(
    endedAt(9, { reason: 'cancelled', resume: 'full' }),
    indexRead({ scanIncomplete: true, pendingChunks: 12 }),
  );

  expect(both).toEqual({ entry: 'full', where: 'strip', label: 'resume' });
  expect(both?.where).not.toBe('section');
});

// ---------------------------------------------------------------------------
// The controller.
// ---------------------------------------------------------------------------

// 🔴 The two are STARTED independently, and this is the state that says so: a
// `listen` that never settles — no rejection, no resolution, nothing to catch —
// must not take the snapshot with it. Chained behind it, the window would show
// nothing at all about a scan that is running, with no timeout and nothing else
// to ask.
//
// This is where the controller parts company with `bootLocale` (`i18n/index.ts`),
// which must register before it asks because a locale reply carries no version
// and only the order can say which of the two is newer. A `ScanState` carries
// its revision, so `apply` sorts them however they arrive, and the ordering buys
// nothing to pay for with this failure mode.
test('a subscription that never settles does not stop the snapshot being read', async () => {
  listen.mockReturnValue(new Promise<() => void>(() => {}));
  replies({ job_status: runningAt(4) });
  const jobs = createJobController();

  jobs.mount();

  await vi.waitFor(() => expect(get(jobs.state).scan.revision).toBe(4));
  expect(get(jobs.state).scan.snapshot.kind).toBe('running');
  expect(get(jobs.state).note).toBeNull();
});

// 🔴 The gap the independent start opens, and the read that closes it.
//
// `apply` sorts two states that both ARRIVE. It cannot sort one that never
// arrives — and `scan-progress` is a fire-and-forget `handle.emit` (`lib.rs`)
// with no replay and no retained last value, so an emission landing between the
// snapshot read and the moment the listener finishes registering is delivered
// to nobody and is in no reply either.
//
// During a running scan the next progress tick corrects it. The LAST emission
// of a job does not: a person opening the settings window just as a scan ends
// would keep a running strip with a Stop that `cancel_job` will refuse, while
// the sections never take their ending re-read — and nothing asks again until
// the next job.
//
// The fixture builds exactly that window: the first reply is `running@3`, the
// scan ends while the listener is still being registered, and **no event is
// ever delivered**. Only a second read, taken after the listener exists, can
// reach `ended@4`.
test('a state that changed in the mount gap is read again once the listener exists', async () => {
  let register!: (fn: () => void) => void;
  listen.mockReturnValue(new Promise<() => void>((resolve) => { register = resolve; }));
  replies({ job_status: runningAt(3) });
  const jobs = createJobController();

  jobs.mount();
  await vi.waitFor(() => expect(get(jobs.state).scan.revision).toBe(3));

  // The scan ends here — after the window read the state, before it is
  // listening. Nothing is emitted to this window, ever.
  replies({ job_status: endedAt(4) });
  register(unlisten);

  await vi.waitFor(() => expect(get(jobs.state).scan.revision).toBe(4));
  expect(get(jobs.state).scan.snapshot.kind).toBe('ended');
  expect(calls('job_status')).toHaveLength(2);
});

// Both directions on the count: twice, and then it stops. Once leaves the gap
// above open; a read per event, or a read that re-triggers itself, would put an
// IPC round trip behind every progress tick of every scan.
test('a mount reads the snapshot twice and then stops asking', async () => {
  await mounted();

  await vi.waitFor(() => expect(calls('job_status')).toHaveLength(2));
  // A macrotask boundary, not a fixed number of microtask ticks: whatever else
  // the mount had queued is given every chance to run before this is believed.
  await new Promise((resolve) => { setTimeout(resolve, 0); });

  expect(calls('job_status')).toHaveLength(2);
});

// The other shape a broken boundary takes, and the one that reaches this code as
// a value rather than as a rejection: a wrapper that is `undefined`, or one that
// answers with something that is not a promise. It throws SYNCHRONOUSLY inside
// `mount`, which returns no promise to reject — so without a guard it escapes
// `onMount` and takes the whole window down with it.
test('a snapshot read that throws instead of rejecting is a sentence, not a broken window', async () => {
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'job_status') throw new Error('the job_status wrapper is not a function');
    return Promise.resolve(undefined);
  });
  const jobs = createJobController();

  expect(() => jobs.mount()).not.toThrow();

  await vi.waitFor(() => expect(get(jobs.state).note).toBe('the job_status wrapper is not a function'));
});

// 🔴 The race the revision exists for. The reply was true when it was asked and
// is about an earlier moment by the time it lands; written over the event it
// would take a running scan off the screen along with its Stop, and nothing
// would put it back until the next progress tick.
test('a late job_status reply cannot overwrite a newer event', async () => {
  let answer!: (state: ScanState) => void;
  invoke.mockImplementation((cmd: string) => (cmd === 'job_status'
    ? new Promise<ScanState>((resolve) => { answer = resolve; })
    : Promise.resolve(undefined)));
  const jobs = createJobController();
  jobs.mount();
  await started();

  emit(runningAt(3));
  answer(idleAt(2));
  await vi.waitFor(() => expect(get(jobs.state).scan.revision).toBe(3));

  expect(get(jobs.state).scan.snapshot.kind).toBe('running');
});

// The same rule in the direction the events themselves can break it: two
// announcements can race and arrive in either order (`state::JobObserver`), so
// the older one must not undo the ending. A report replaced by a progress line
// is a finished scan a person never gets told about.
test('an ended snapshot is not overwritten by an older progress event', async () => {
  const { jobs } = await mounted();

  emit(endedAt(9));
  emit(runningAt(8));

  expect(get(jobs.state).scan.revision).toBe(9);
  expect(get(jobs.state).scan.snapshot.kind).toBe('ended');
});

// The other direction of the late reply: the snapshot really is the newer of
// the two, and a controller that refused every reply once an event had arrived
// would sit on the older one for the life of the window.
test('a snapshot newer than the event that preceded it is applied', async () => {
  let answer!: (state: ScanState) => void;
  invoke.mockImplementation((cmd: string) => (cmd === 'job_status'
    ? new Promise<ScanState>((resolve) => { answer = resolve; })
    : Promise.resolve(undefined)));
  const jobs = createJobController();
  jobs.mount();
  await started();

  emit(runningAt(2));
  answer(endedAt(4));

  await vi.waitFor(() => expect(get(jobs.state).scan.revision).toBe(4));
  expect(get(jobs.state).scan.snapshot.kind).toBe('ended');
});

// The emitter may fire the same revision twice — two announcements racing is
// harmless BECAUSE of this (`lib.rs` says so on the emit). A store that wrote a
// fresh object anyway would wake every section of the window to redraw what it
// already has.
test('a repeated revision does not wake the subscribers', async () => {
  const { jobs } = await mounted();
  let notifications = 0;
  const stop = jobs.state.subscribe(() => { notifications += 1; });
  const seeded = notifications;

  emit(runningAt(5));
  const afterFirst = notifications;
  emit(runningAt(5));

  expect(afterFirst).toBe(seeded + 1);
  expect(notifications).toBe(afterFirst);
  stop();
});

test('a scan asks for the entry point it was given, and clears the standing sentence', async () => {
  replies({ job_status: IDLE, start_scan_job: new Error('another job is already running') });
  const { jobs } = await mounted();
  await jobs.scan('full');
  expect(get(jobs.state).note).toBe('another job is already running');

  replies({ job_status: IDLE, start_scan_job: undefined });
  await jobs.scan('embedOnly');

  expect(calls('start_scan_job').map((c) => (c[1] as { entry: string }).entry)).toEqual(['full', 'embedOnly']);
  expect(get(jobs.state).note).toBeNull();
});

// 🔴 A rejection is a sentence and never a kind (`error.rs`), so nothing here
// reads its shape — and the sentence alone cannot say what the slot now holds.
// The re-read is what answers that: the commonest reason this command is
// refused is that another job holds the slot, and its state is exactly what the
// window must go on drawing.
test('a refused scan shows the sentence verbatim and asks the state again', async () => {
  replies({ job_status: IDLE, start_scan_job: new Error('another job is already running') });
  const { jobs } = await mounted();
  // Two, not one: a mount reads the snapshot once for a fast first paint and
  // again once the listener exists — see `a mount reads the snapshot twice`.
  await vi.waitFor(() => expect(calls('job_status')).toHaveLength(2));

  replies({ job_status: runningAt(4), start_scan_job: new Error('another job is already running') });
  await jobs.scan('full');

  expect(get(jobs.state).note).toBe('another job is already running');
  expect(calls('job_status')).toHaveLength(3);
  expect(get(jobs.state).scan.snapshot.kind).toBe('running');
});

// The sentence a person needs is the one about the press. A re-read that is
// itself refused would otherwise replace it with a sentence about a question
// nobody asked, and the reason the scan would not start would be gone.
test('a re-read refused after a refused scan keeps the sentence about the press', async () => {
  const { jobs } = await mounted();

  replies({
    job_status: new Error('the index could not be opened'),
    start_scan_job: new Error('another job is already running'),
  });
  await jobs.scan('full');

  expect(get(jobs.state).note).toBe('another job is already running');
});

// Both directions on `cancel`: an accepted stop says nothing — the job reports
// its own ending through the event, and a note invented here would be a claim
// about a state nothing has read — and a refused one says the sentence.
test('cancelling an idle application says nothing, and a refused stop says the sentence', async () => {
  const { jobs } = await mounted();

  await jobs.cancel();

  expect(calls('cancel_job')).toHaveLength(1);
  expect(calls('cancel_job')[0]).toHaveLength(1); // the command name alone
  expect(get(jobs.state).note).toBeNull();

  replies({ job_status: IDLE, cancel_job: new Error('nothing is running') });
  await jobs.cancel();

  expect(get(jobs.state).note).toBe('nothing is running');
});

// 🔴 The other half of the sentence's life, and the half that was missing: a
// rejection is about a moment, and the next thing the CORE says postdates it.
//
// The sequence is an ordinary one. A person presses Stop as the job is ending;
// the command is refused («nothing is running»); the next scan starts. Before
// this, that sentence sat on the strip underneath a live «Індексація теки 1 з
// 2» until the person happened to press Scan or Continue — the one place `note`
// was cleared.
//
// Asserted with the state as well as the sentence, because a controller that
// dropped the event entirely would also show `note` as null.
test('a refused stop\'s sentence dies with the next state the core sends', async () => {
  const { jobs } = await mounted();

  replies({ job_status: IDLE, cancel_job: new Error('nothing is running') });
  await jobs.cancel();
  expect(get(jobs.state).note).toBe('nothing is running');

  emit(runningAt(4));

  expect(get(jobs.state).note).toBeNull();
  expect(get(jobs.state).scan.revision).toBe(4);
  expect(get(jobs.state).scan.snapshot.kind).toBe('running');
});

// The same rule at the other rejection this controller can carry for the life
// of a window: the first `job_status` is refused, and every state after it
// arrives correctly. `a refused initial snapshot is a sentence, and the
// subscription still delivers` (below) asserts the states arrive and says
// nothing about the sentence, which is how it stood for the life of the window.
test('a refused first snapshot\'s sentence dies with the first event', async () => {
  replies({ job_status: new Error('the index could not be opened') });
  const { jobs } = await mounted();
  await vi.waitFor(() => expect(get(jobs.state).note).toBe('the index could not be opened'));

  emit(runningAt(3));

  expect(get(jobs.state).note).toBeNull();
});

// The mirror, and it is what keeps the clear narrow: an event carrying nothing
// newer is not the core saying anything, so it takes nothing away. Without this
// the rule would be "any event clears", and a duplicate delivery — which
// `a repeated revision does not wake the subscribers` shows the core can
// send — would silently swallow a sentence a person had not read yet.
test('an event with nothing newer in it leaves the standing sentence alone', async () => {
  const { jobs } = await mounted();

  emit(runningAt(5));
  replies({ job_status: IDLE, cancel_job: new Error('nothing is running') });
  await jobs.cancel();
  expect(get(jobs.state).note).toBe('nothing is running');

  emit(runningAt(5));

  expect(get(jobs.state).note).toBe('nothing is running');
  expect(get(jobs.state).scan.revision).toBe(5);
});

// 🔴 `mount` returns before `listen` resolves, so `destroy` can be called with
// no unlisten function to call yet — a section switch during boot is exactly
// that. Both halves: the unlisten arrives and is used ONCE, and the handler
// registered before it does nothing afterwards. A controller that only
// remembered the function would leave a live listener on the window for good.
test('destroying before the subscription resolves still unsubscribes, exactly once', async () => {
  let register!: (fn: () => void) => void;
  listen.mockReturnValue(new Promise<() => void>((resolve) => { register = resolve; }));
  const jobs = createJobController();
  const destroy = jobs.mount();

  destroy();
  destroy();
  register(unlisten);
  await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));

  emit(runningAt(7));

  expect(unlisten).toHaveBeenCalledTimes(1);
  expect(get(jobs.state).scan).toEqual(IDLE);
  // ONE read, not two. The second read exists to close the gap between the
  // first one and the listener; a window that has gone has no gap left to
  // close, and an IPC round trip fired for it is work nobody will ever read.
  expect(calls('job_status')).toHaveLength(1);
});

// The reply is in flight when the section goes. Applied, it would write to a
// store nothing is reading and — worse — would be the state a remount then
// starts from without ever having asked.
test('destroying while the snapshot is in flight drops the reply', async () => {
  let answer!: (state: ScanState) => void;
  invoke.mockImplementation((cmd: string) => (cmd === 'job_status'
    ? new Promise<ScanState>((resolve) => { answer = resolve; })
    : Promise.resolve(undefined)));
  const jobs = createJobController();
  const destroy = jobs.mount();
  await started();

  destroy();
  answer(runningAt(6));
  await Promise.resolve();
  await Promise.resolve();

  expect(get(jobs.state).scan).toEqual(IDLE);
  expect(unlisten).toHaveBeenCalledTimes(1);
});

// 🔴 The subscription survives a refused snapshot, and that is the whole of
// this test. A controller that gave up on the listener when the first read
// failed would leave a window that says why it could not read the state and
// then never notices the state changing.
test('a refused initial snapshot is a sentence, and the subscription still delivers', async () => {
  replies({ job_status: new Error('the index could not be opened') });
  const { jobs } = await mounted();
  await vi.waitFor(() => expect(get(jobs.state).note).toBe('the index could not be opened'));

  emit(runningAt(3));

  expect(get(jobs.state).scan.revision).toBe(3);
  expect(get(jobs.state).scan.snapshot.kind).toBe('running');
});

// The listener itself can fail to register. It is a sentence like any other
// rejection, and the snapshot is still asked for — a window with no live
// updates and no state at all would say nothing about a scan that is running.
test('a subscription that could not be registered is a sentence, and the snapshot is still read', async () => {
  listen.mockRejectedValue(new Error('the event system is unavailable'));
  replies({ job_status: runningAt(2) });
  const { jobs } = await mounted();

  await vi.waitFor(() => expect(get(jobs.state).note).toBe('the event system is unavailable'));
  expect(get(jobs.state).scan.revision).toBe(2);
  expect(get(jobs.state).scan.snapshot.kind).toBe('running');
});

// The event name and the envelope, exercised through the real wrapper: a
// listener registered on another name hears nothing, and one that stores the
// envelope instead of its payload holds a state with no revision at all.
test('the controller listens to scan-progress and reads the payload out of the envelope', async () => {
  const { jobs } = await mounted();

  expect(listen.mock.calls.at(-1)?.[0]).toBe('scan-progress');
  emit(endedAt(3, { reason: 'volumeMissing', message: 'the volume is not mounted' }));

  const snapshot = get(jobs.state).scan.snapshot;
  expect(snapshot.kind === 'ended' && snapshot.report.message).toBe('the volume is not mounted');
});

// ---------------------------------------------------------------------------
// `progressShape` — unchanged by this task, and still the only thing that says
// which sentence about progress can honestly be told.
// ---------------------------------------------------------------------------

// `total: 0` is the first thing a real folder breaks — a walk reports it before
// phase 1 has counted anything, and `rootUnavailable` reports zero of zero for
// good. Nothing here divides by it; the shape says which sentence can be told.
test('a run with nothing counted yet states no ratio, and one with a total does', () => {
  expect(progressShape({ done: 0, total: 0, skipped: 0, refused: 0, contended: 0, secondsLeft: null }))
    .toEqual({ kind: 'countingUp', done: 0 });
  expect(progressShape({ done: 4, total: 0, skipped: 0, refused: 0, contended: 0, secondsLeft: null }))
    .toEqual({ kind: 'countingUp', done: 4 });
  expect(progressShape({ done: 3, total: 8, skipped: 1, refused: 0, contended: 0, secondsLeft: null }))
    .toEqual({ kind: 'ratio', done: 3, total: 8 });
});

import { render, screen, cleanup, waitFor, fireEvent } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import { tick } from 'svelte';
import Indexing from './Indexing.svelte';
import Settings from './Settings.svelte';
import { createJobController } from './jobs';
import { setLocale } from '../i18n';
import type { ModelSettings, ScanReport, ScanState } from '../lib/ipc';

// The typed wrappers, not the raw `invoke` — the shape `Models.test.ts` uses.
const modelSettings = vi.fn();
const providerModels = vi.fn();
const listTree = vi.fn();
const listMasks = vi.fn();
const startScanJob = vi.fn();
const cancelJob = vi.fn();
const jobStatus = vi.fn();
const listenScanProgress = vi.fn();
const unlisten = vi.fn();
// The `scan-progress` handler the controller registered, kept so a test can
// deliver the states a real scan would.
let deliver: ((state: ScanState) => void) | null = null;
vi.mock('../lib/ipc', () => ({
  modelSettings: (...a: unknown[]) => modelSettings(...a),
  providerModels: (...a: unknown[]) => providerModels(...a),
  listTree: (...a: unknown[]) => listTree(...a),
  listMasks: (...a: unknown[]) => listMasks(...a),
  setKey: vi.fn(),
  forgetKey: vi.fn(),
  setChatModel: vi.fn(),
  setEmbeddingModel: vi.fn(),
  maskPreview: vi.fn(),
  addMask: vi.fn(),
  removeMask: vi.fn(),
  addWatchedFolder: vi.fn(),
  removeWatchedFolder: vi.fn(),
  // The controller imports all four from this module. Left out, each wrapper is
  // `undefined` and every call becomes a TypeError swallowed by a catch — the
  // lesson `Models.test.ts`'s own mock records.
  startScanJob: (...a: unknown[]) => startScanJob(...a),
  cancelJob: (...a: unknown[]) => cancelJob(...a),
  jobStatus: (...a: unknown[]) => jobStatus(...a),
  listenScanProgress: (...a: unknown[]) => listenScanProgress(...a),
}));

// 🔴 Annotated `ModelSettings`, and that is the point of the annotation rather
// than tidiness. Task 3 made `indexedFiles`, `lastIndexedAt` and `failedChunks`
// REQUIRED fields of the `read` arm, and every inline fixture in this project's
// UI suites sat behind an untyped mock where the compiler never looked. A
// fixture here that forgets one is a `npm run check` error, not a section that
// renders `undefined` in front of a person.
function settings(over: Partial<ModelSettings> = {}): ModelSettings {
  return {
    key: { kind: 'present' },
    index: {
      kind: 'read',
      embeddingModel: 'openai/text-embedding-3-small', chatModel: null,
      embeddedChunks: 12, embeddedChunksEverywhere: 12, totalChunks: 12,
      failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null,
      searchTextArm: true, searchContentArm: true,
    },
    platform: 'linux',
    ...over,
  };
}

// A `read` arm with only the fields a case cares about restated. Spelled as a
// helper rather than by hand so the three required numbers cannot be dropped by
// an override that was only trying to change one of them.
type IndexRead = Extract<ModelSettings['index'], { kind: 'read' }>;
const read = (over: Partial<IndexRead> = {}): ModelSettings =>
  settings({ index: { ...(settings().index as IndexRead), ...over } });

const HOUR_AGO = () => Math.floor(Date.now() / 1000) - 3600;

// Computed here, never written out: the test machine's zone is not the CI
// machine's, and the section formats in the machine's own zone on purpose.
//
// The trailing-stop strip mirrors `formatIndexedDate` (F1): ICU's own `uk`
// long-date form ends in «р.», and the sentence these composed assertions
// build supplies its own final stop — `recency.test.ts` covers the strip
// itself in full, so this oracle only needs to agree with production on the
// shape the composed sentence is checked against.
const dateIn = (loc: string, at: number) =>
  new Intl.DateTimeFormat(loc, { dateStyle: 'long' }).format(new Date(at * 1000)).replace(/\.$/, '');

const EMPTY_CATALOGUE = { entries: [], unreadable: 0, unreadableRecords: [] };

beforeEach(() => {
  modelSettings.mockReset();
  providerModels.mockReset();
  listTree.mockReset();
  listMasks.mockReset();
  cancelJob.mockReset();
  jobStatus.mockReset();
  modelSettings.mockResolvedValue(settings());
  providerModels.mockResolvedValue(EMPTY_CATALOGUE);
  listTree.mockResolvedValue({ roots: [], recents: [] });
  listMasks.mockResolvedValue([]);
  cancelJob.mockResolvedValue(undefined);
  startScanJob.mockReset();
  startScanJob.mockResolvedValue(undefined);
  listenScanProgress.mockReset();
  unlisten.mockReset();
  deliver = null;
  revision = 0;
  jobStatus.mockResolvedValue(IDLE_SCAN);
  listenScanProgress.mockImplementation((cb: (state: ScanState) => void) => {
    deliver = cb;
    return Promise.resolve(unlisten);
  });
  setLocale('uk');
});

afterEach(() => {
  cleanup();
  setLocale('en');
});

// What a person reads, with the markup's own indentation collapsed the way a
// browser collapses it (`JobStrip.test.ts:79` normalises the same way).
const visible = (el: Element | null) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();
const pageText = () => visible(document.body);

// 🔴 The controller is MOUNTED, not merely created: `Settings.svelte` is what
// mounts it in the window, and until it is mounted nothing is listening for the
// states these tests deliver. The teardown is left to `cleanup`, which unmounts
// the component; the subscription itself is the window's, not this section's.
function renderSection(jobs = createJobController()) {
  jobs.mount();
  return { jobs, ...render(Indexing, { props: { jobs } }) };
}

// Every state is newer than the one before it, because that is the only thing
// the controller compares (`apply`): a fixture that reused a revision would be
// dropped as stale and the test would be asserting about the state before it.
let revision = 0;
const IDLE_SCAN: ScanState = {
  revision: 0, files: 0, readSeq: 0, lastReading: null, snapshot: { kind: 'idle' },
};

const runningScan = (): ScanState => ({
  ...IDLE_SCAN,
  revision: (revision += 1),
  snapshot: {
    kind: 'running',
    cancellable: true,
    phase: {
      kind: 'embedding',
      counts: { done: 1, total: 4, skipped: 0, refused: 0, contended: 0, secondsLeft: null },
    },
  },
});

// `cancelled` by default: the ordinary resting state after a stop, and the one
// that leaves work still owed for the queue line below to be about.
const endedScan = (over: Partial<ScanReport> = {}): ScanState => ({
  ...IDLE_SCAN,
  revision: (revision += 1),
  snapshot: {
    kind: 'ended',
    report: {
      embedding: { kind: 'notReached' },
      endedIn: 'reading',
      reason: 'cancelled',
      message: null,
      resume: null,
      ...over,
    },
  },
});

// Delivers one state the way the core's own observer does, and lets Svelte draw.
async function emit(state: ScanState) {
  if (deliver === null) throw new Error('nothing is listening to scan-progress');
  deliver(state);
  await tick();
}

// ---------------------------------------------------------------------------
// What the index holds, and when it last grew (§9.3, D-e).
// ---------------------------------------------------------------------------

test('a filled index says how many files it holds, the date it last grew, and how long ago that was', async () => {
  const at = HOUR_AGO();
  modelSettings.mockResolvedValue(read({ indexedFiles: 12, lastIndexedAt: at }));

  renderSection();

  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('В індексі 12 файлів.');
  // The date, in the active locale and the machine's own zone.
  expect(visible(screen.getByTestId('indexing-index-date')))
    .toBe(`Останнє оновлення: ${dateIn('uk', at)}.`);
  // …and the relative phrase beside it. Two lines, because they answer two
  // different questions and one cannot stand in for the other. A sentence with
  // its own subject and its own full stop, not the bare phrase: the Recents
  // card can render «1 годину тому» alone because a filename sits beside it
  // supplying the subject, and nothing here does.
  expect(visible(screen.getByTestId('indexing-index-ago'))).toBe('Це було 1 годину тому.');
  expect(screen.queryByTestId('indexing-index-never')).toBeNull();
});

test('the date follows the language, not the machine', async () => {
  const at = HOUR_AGO();
  modelSettings.mockResolvedValue(read({ indexedFiles: 12, lastIndexedAt: at }));

  renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-index-date')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-date')))
    .toBe(`Останнє оновлення: ${dateIn('uk', at)}.`);

  setLocale('en');
  await tick();

  // Both halves move: the sentence around the date and the date itself. A
  // `$derived` without its `void $locale` anchor keeps the Ukrainian one here.
  expect(visible(screen.getByTestId('indexing-index-date')))
    .toBe(`Last updated: ${dateIn('en', at)}.`);
  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('The index holds 12 files.');
  expect(visible(screen.getByTestId('indexing-index-ago'))).toBe('That was 1 hour ago.');
  expect(dateIn('en', at)).not.toBe(dateIn('uk', at));
});

test('an index nothing has ever finished indexing says so, and draws no time at all', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 0, lastIndexedAt: null }));

  renderSection();

  await waitFor(() => expect(screen.getByTestId('indexing-index-never')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-never'))).toBe('Ще нічого не проіндексовано.');
  // Both directions. `null` is a statement, and every wrong way to render it
  // still produces a line that looks like a date.
  expect(screen.queryByTestId('indexing-index-date')).toBeNull();
  expect(screen.queryByTestId('indexing-index-ago')).toBeNull();
  const text = pageText();
  expect(text).not.toContain('Invalid Date');
  expect(text).not.toContain('null');
  expect(text).not.toContain('1970'); // the epoch, which `?? 0` renders as a real date
  expect(text).not.toContain('щойно'); // "just now", which `?? 0` would give the phrase
  // The count is still drawn: zero files is a measured number, unlike the time.
  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('В індексі 0 файлів.');
});

// ---------------------------------------------------------------------------
// The index that could not be read (§10: branch on `kind`, never on the text).
// ---------------------------------------------------------------------------

const TOKEN = 'REASON-TOKEN-9f2c';

test('an index that is not open says so, and shows the backend reason verbatim', async () => {
  modelSettings.mockResolvedValue(settings({
    index: { kind: 'unreadable', cause: 'notOpen', reason: `could not open the index: ${TOKEN}` },
  }));

  renderSection();

  await waitFor(() => expect(screen.getByTestId('indexing-index-unreadable')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-unreadable')))
    .toBe('Не вдалося прочитати індекс: він не відкритий.');
  // 🔴 Deliberately the OPPOSITE of the Models section's rule
  // (`Models.test.ts:115-120` asserts the reason is never shown there). §9.3 is
  // where a person is told what is wrong with their index, and
  // `IndexSettings::Unreadable`'s own doc says `reason` stays verbatim for
  // showing (`models.rs:932`).
  expect(visible(screen.getByTestId('indexing-index-unreadable-reason')))
    .toBe(`Програма повідомила: could not open the index: ${TOKEN}`);
  // Neither sentence stands in for the other.
  expect(pageText()).not.toContain('спроба читання не вдалася');
  // …and nothing is read out of the arm that is not there.
  expect(screen.queryByTestId('indexing-index-files')).toBeNull();
  expect(screen.queryByTestId('indexing-index-date')).toBeNull();
  expect(screen.queryByTestId('indexing-index-never')).toBeNull();
  expect(pageText()).not.toContain('undefined');
});

test('an index whose read failed gets its own sentence, not the not-open one', async () => {
  modelSettings.mockResolvedValue(settings({
    index: { kind: 'unreadable', cause: 'readFailed', reason: `disk I/O error: ${TOKEN}` },
  }));

  renderSection();

  await waitFor(() => expect(screen.getByTestId('indexing-index-unreadable')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-unreadable')))
    .toBe('Не вдалося прочитати індекс: спроба читання не вдалася.');
  expect(visible(screen.getByTestId('indexing-index-unreadable-reason')))
    .toBe(`Програма повідомила: disk I/O error: ${TOKEN}`);
  expect(pageText()).not.toContain('він не відкритий');
  expect(screen.queryByTestId('indexing-index-files')).toBeNull();
});

// ---------------------------------------------------------------------------
// The two scopes of "given up on", owed since PR 7.
// ---------------------------------------------------------------------------

const SPACE_SENTENCE = 'У цьому індексі провайдер відхилив 3 фрагменти за весь час.'
  + ' Їх більше не пропонують, доки не зміниться їхній текст:'
  + ' пошук за змістом їх не знаходить, пошук по словах — знаходить.';
const RUN_SENTENCE = 'Останній прохід вбудовування відхилив 1 фрагмент.';

test('chunks the provider gave up on are named for the index, and for what that costs', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, failedChunks: 3 }));

  renderSection();

  await waitFor(() => expect(screen.getByTestId('indexing-index-failed-chunks')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-failed-chunks'))).toBe(SPACE_SENTENCE);
});

test('an index the provider refused nothing in says nothing about refusals', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, failedChunks: 0 }));

  renderSection();

  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(screen.queryByTestId('indexing-index-failed-chunks')).toBeNull();
  expect(pageText()).not.toContain('відхилив');
});

// 🔴 The other direction of the RUN's own sentence, and the one a suite that
// only ever ends scans with `Ran` cannot see. `notReached` and `skipped` are
// not a refusal count of zero: no chunk was offered to a provider at all
// (`scan_state::EmbedOutcome`), so there is no run to report on. A build that
// read a number off those arms would tell a person their last scan gave up on
// chunks that were never sent.
//
// The pair is stated in one test because it is one fixture moved: the same
// ended scan, once with the embedding phase never reached and once with it
// having run and refused.
test('an ending whose embedding never ran reports no refusals, and one that ran reports its own', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, failedChunks: 3 }));
  renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-index-failed-chunks')).toBeTruthy());

  await emit(endedScan({ reason: 'cancelled', embedding: { kind: 'notReached' } }));
  await waitFor(() => expect(modelSettings.mock.calls.length).toBeGreaterThan(1));
  expect(screen.queryByTestId('indexing-index-refused-run')).toBeNull();

  await emit(endedScan({
    reason: 'completed', endedIn: 'embedding',
    embedding: { kind: 'skipped', why: { kind: 'noKey' } },
  }));
  await tick();
  expect(screen.queryByTestId('indexing-index-refused-run')).toBeNull();

  await emit(endedScan({
    reason: 'completed', endedIn: 'embedding',
    embedding: { kind: 'ran', done: 3, total: 4, refused: 1 },
  }));

  await waitFor(() => expect(screen.getByTestId('indexing-index-refused-run')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-refused-run'))).toBe(RUN_SENTENCE);
});

// 🔴 The state a suite without it cannot read: one sentence and two sentences
// look the same until both scopes are on the screen at once. `job.rs:38-44`
// says these are two numbers about two scopes, and whichever surface shows them
// owes each its own words.
test('a run that gave up on chunks and an index that already had some show two sentences, each about its own subject', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, failedChunks: 3 }));
  renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-index-failed-chunks')).toBeTruthy());

  await emit(endedScan({
    reason: 'completed', endedIn: 'embedding',
    embedding: { kind: 'ran', done: 3, total: 4, refused: 1 },
  }));

  await waitFor(() => expect(screen.getByTestId('indexing-index-refused-run')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-refused-run'))).toBe(RUN_SENTENCE);
  expect(visible(screen.getByTestId('indexing-index-failed-chunks'))).toBe(SPACE_SENTENCE);
  // Two different sentences, not one key drawn twice with two counts.
  expect(RUN_SENTENCE).not.toBe(SPACE_SENTENCE);
});

// 🔴 The pairing the two sentences have to survive apart, decided rather than
// left undecided (review, Minor 5). The sequence is the ordinary one: a pass
// ends, the ending triggers the re-read, and the re-read comes back
// `Unreadable`. The run's report is then the only surviving account of what
// just happened, so it stays; the cumulative sentence goes, because that one is
// about the index and there is no `read` arm to take it from.
test('an index that stops being readable still says what the pass that just ended gave up on', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, failedChunks: 3 }));
  renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-index-failed-chunks')).toBeTruthy());

  // What the ending's own re-read finds.
  modelSettings.mockResolvedValue(settings({
    index: { kind: 'unreadable', cause: 'notOpen', reason: `gone mid-pass: ${TOKEN}` },
  }));
  await emit(endedScan({
    reason: 'completed', endedIn: 'embedding',
    embedding: { kind: 'ran', done: 3, total: 4, refused: 1 },
  }));

  await waitFor(() => expect(screen.getByTestId('indexing-index-unreadable')).toBeTruthy());
  // Kept: its subject is the pass, and the pass really did refuse them.
  expect(visible(screen.getByTestId('indexing-index-refused-run'))).toBe(RUN_SENTENCE);
  // Gone: its subject is the index, and the index no longer answers.
  expect(screen.queryByTestId('indexing-index-failed-chunks')).toBeNull();
  expect(screen.queryByTestId('indexing-index-files')).toBeNull();
});

// ---------------------------------------------------------------------------
// F4 (spec §9.3, amended 2026-09-04): the embedding queue. A tray Stop on the
// embedding pass, then a restart, left thousands of chunks un-embedded with
// nothing on any screen saying so — the only resume was «Сканувати» beside the
// right folder happening to chain into an embed. `IndexRead.pendingChunks` is
// the queue itself; this section says how many chunks are owed and offers to
// resume, but only while no run is already under way — the strip above owns
// that state once one starts.
// ---------------------------------------------------------------------------

const PENDING_SENTENCE = 'Ще не вбудовано 5 фрагментів.';

test('an index with a queue and no run under way names it and offers to resume', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));

  renderSection();

  await waitFor(() => expect(screen.getByTestId('indexing-index-pending-chunks')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-pending-chunks'))).toBe(PENDING_SENTENCE);
  expect(screen.getByTestId('indexing-resume-embedding')).toBeTruthy();
});

// Minor 4, review: both `pendingLine` and `resumeEmbeddingLabel` read
// `void $locale` inside their `$derived.by`, the same anchor every other
// string on this section carries — and until this test, nothing here drove a
// language switch while a queue was on screen, so removing either anchor left
// the suite green. Both directions, both testids, the way `the date follows
// the language, not the machine` already covers the two lines above these.
test('the pending-queue line and its button follow the language, not the machine', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));

  renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-index-pending-chunks')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-pending-chunks'))).toBe(PENDING_SENTENCE);
  expect(visible(screen.getByTestId('indexing-resume-embedding'))).toBe('Продовжити вбудовування');

  setLocale('en');
  await tick();

  expect(visible(screen.getByTestId('indexing-index-pending-chunks')))
    .toBe('5 chunks are not embedded yet.');
  expect(visible(screen.getByTestId('indexing-resume-embedding'))).toBe('Continue embedding');
});

test('an empty queue says nothing and offers nothing', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 0 }));

  renderSection();

  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(screen.queryByTestId('indexing-index-pending-chunks')).toBeNull();
  expect(screen.queryByTestId('indexing-resume-embedding')).toBeNull();
  expect(pageText()).not.toContain('вбудовано');
});

// A queue does not vanish the moment a run starts on it — the count on the
// screen is stale until the next re-read — so the section hides the line and
// the button itself rather than trusting the backend to zero the count first.
// The strip above already owns "a pass is running" for every section; a
// second button offering to start ANOTHER embed here would race it.
test('a queue is not offered again while a run is already under way', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));
  renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-index-pending-chunks')).toBeTruthy());

  await emit(runningScan());

  expect(screen.queryByTestId('indexing-index-pending-chunks')).toBeNull();
  expect(screen.queryByTestId('indexing-resume-embedding')).toBeNull();
});

// ⚠️ The test that stood here — «a queue is not offered while the pass is still
// starting» — asserted a state that no longer exists. `jobs.ts` had a
// `starting` phase it wrote itself, in anticipation of a job it had asked for
// and not yet heard from; the snapshot has no such state, because the core
// announces the claim and nothing else may invent one. The window between the
// press and that announcement is one IPC round trip, during which this section
// still offers the button; a second press is refused by `claim_job` and the
// refusal reaches the strip as a sentence. **Task 8** rewrites this gate on
// `continueAction`, which is where that decision belongs.

// The settings window reopened while a scan started elsewhere — from the tray,
// or before this window existed — is still going. It is the same F4 scenario
// one step later: a person who stopped a run, closed the window, reopened it,
// and started another pass from elsewhere before checking back. The queue count
// on screen is a moment-old read and does not shrink as that run works through
// it, so the line and its button step aside.
//
// Delivered as the SNAPSHOT rather than as a boolean, which is the whole of
// what this task changed: a window that never started the job hears about it in
// exactly the same words as one that did.
test('a queue is not offered while a scan this window did not start is running', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));
  renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-resume-embedding')).toBeTruthy());

  await emit(runningScan());

  expect(screen.queryByTestId('indexing-index-pending-chunks')).toBeNull();
  expect(screen.queryByTestId('indexing-resume-embedding')).toBeNull();
});

// 🔴 (final review, C-M7) …and it must not stay hidden for the life of the
// window — which is exactly what this task fixed rather than worked around.
//
// The defect this test was written for: the running state came from a
// `job_status` read taken once at the window's mount, so a pass this window had
// no channel for could END and nothing here would ever learn. The strip went on
// saying a pass was running and F4's queue line and its Continue button stayed
// suppressed; the only recovery was pressing Cancel, which nobody would guess.
// The old fix asked again on every mount of this section, so a person parked
// here saw nothing change until they navigated away and back.
//
// The pair this now separates is «the section learns on the next visit» from
// «the section learns when it happens». NOTHING is remounted below: the window
// holds one subscription to `scan-progress` and the ending arrives on it.
test('a scan this window could not hear ends, and the section offers the queue again without a revisit', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));
  // The settings window was opened while a scan started elsewhere was running.
  jobStatus.mockResolvedValue({ ...IDLE_SCAN, ...runningScan() });
  const { container } = render(Settings);
  const panel = () => container.querySelector('.spane');
  await fireEvent.click(screen.getByTestId('settings-nav-indexing'));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(screen.queryByTestId('indexing-resume-embedding')).toBeNull();

  await emit(endedScan());

  await waitFor(() => expect(screen.getByTestId('indexing-resume-embedding')).toBeTruthy());
  expect(visible(panel())).toContain('Ще не вбудовано 5 фрагментів.');
});

// Through the controller, exactly the way `Models.svelte`'s own `reembed`
// argues: the pass this button starts belongs on the window's strip, where its
// progress and its Stop stay reachable from every section, not to a listener
// this component alone can hear.
//
// 🔴 Both directions on the ENTRY POINT, which is what replaced the old
// «never a walk» assertion. There is one command now and the argument is the
// whole difference: `full` would re-read every watched folder to reach the same
// queue, and this button's own sentence promises only the chunks that are
// already owed.
test('the resume button asks for the embedding entry point through the controller', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));
  renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-resume-embedding')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('indexing-resume-embedding'));

  await waitFor(() => expect(startScanJob).toHaveBeenCalledTimes(1));
  expect(startScanJob).toHaveBeenCalledWith('embedOnly');
  expect(startScanJob).not.toHaveBeenCalledWith('full');
});

// The `ended` half of the gate (Important 1, review) — the state the button
// was written FOR. `refresh()` fires on an ended snapshot and nothing
// afterwards moves it back to `idle` — an ending is a STATE that stands until
// the next job claims the slot (`scan_state.rs`) — so a scan stopped from the
// window's own strip with chunks still owed lands here and stays here: this
// is the ordinary resting state after a stop, not a transient one. The queue
// stays positive across the ending's own re-read on purpose — a completed
// pass that did not clear the whole queue, or a cancelled one — so this test
// is told apart from "the empty queue hid it" the other `ended` test already
// covers.
test('an ended pass with chunks still owed still shows the line and the button', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));
  renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-resume-embedding')).toBeTruthy());

  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));
  await emit(endedScan({ reason: 'completed' }));

  await waitFor(() => expect(screen.getByTestId('indexing-index-pending-chunks')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-pending-chunks'))).toBe(PENDING_SENTENCE);
  expect(screen.getByTestId('indexing-resume-embedding')).toBeTruthy();
});

// The line and the button re-derive once the pass they started ends, the same
// re-read every other ending on this section already triggers
// (`refresh()` on an ended snapshot) — reusing the state fixtures and the
// `modelSettings` call-count pattern the refresh tests below pin.
test('once the resumed pass ends, the section re-reads and the queue reflects what is left', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));
  renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-resume-embedding')).toBeTruthy());

  // What this pins is the ONE further read the ending itself triggers — the
  // same `refresh()` on an ended snapshot every other ending on this section
  // already causes.
  const beforeEnding = modelSettings.mock.calls.length;
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 0 }));
  await emit(endedScan({ reason: 'completed' }));

  await waitFor(() => expect(modelSettings.mock.calls.length).toBe(beforeEnding + 1));
  await waitFor(() => expect(screen.queryByTestId('indexing-index-pending-chunks')).toBeNull());
  expect(screen.queryByTestId('indexing-resume-embedding')).toBeNull();
});

// ---------------------------------------------------------------------------
// The refresh, its trigger, and its lifetime.
// ---------------------------------------------------------------------------

test('an ending re-reads the index, and each further ending re-reads it again', async () => {
  renderSection();
  await waitFor(() => expect(modelSettings).toHaveBeenCalledTimes(1));

  await emit(endedScan());
  await waitFor(() => expect(modelSettings).toHaveBeenCalledTimes(2));

  await emit(endedScan());
  await waitFor(() => expect(modelSettings).toHaveBeenCalledTimes(3));
});

// The mirror. A subscriber that re-fetches on every store emission passes the
// test above and fails here — and it is the only thing that tells the two apart.
test('a progress report is not an ending and re-reads nothing', async () => {
  renderSection();
  await waitFor(() => expect(modelSettings).toHaveBeenCalledTimes(1));

  await emit(runningScan());
  await emit(runningScan());
  await tick();

  expect(modelSettings).toHaveBeenCalledTimes(1);
});

// 🔴 Counted, not "at least once". A section that never unsubscribes answers
// three times here and satisfies every looser assertion.
test('a section left behind by a nav change stops listening — three mounts, one ending, one re-read', async () => {
  const jobs = createJobController();
  jobs.mount();
  render(Indexing, { props: { jobs } }).unmount();
  render(Indexing, { props: { jobs } }).unmount();
  render(Indexing, { props: { jobs } });
  await waitFor(() => expect(modelSettings).toHaveBeenCalledTimes(3));

  await emit(endedScan());

  await waitFor(() => expect(modelSettings).toHaveBeenCalledTimes(4));
  await tick();
  expect(modelSettings).toHaveBeenCalledTimes(4);
});

// ---------------------------------------------------------------------------
// Two reads in flight, and the older one answering last.
// ---------------------------------------------------------------------------

function deferredPromise<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

test('an older read that settles last does not repaint over the newer one', async () => {
  const queue: ReturnType<typeof deferredPromise<ModelSettings>>[] = [];
  modelSettings.mockImplementation(() => {
    const d = deferredPromise<ModelSettings>();
    queue.push(d);
    return d.promise;
  });

  renderSection();
  await waitFor(() => expect(queue).toHaveLength(1)); // the mount's read
  await emit(endedScan());
  await waitFor(() => expect(queue).toHaveLength(2)); // the ending's read

  // Newer first, older last — the order the network is free to choose.
  queue[1].resolve(read({ indexedFiles: 99, lastIndexedAt: null }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  queue[0].resolve(read({ indexedFiles: 7, lastIndexedAt: null }));
  await tick();
  await tick();

  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('В індексі 99 файлів.');
  expect(pageText()).not.toContain('В індексі 7 файлів.');
});

// The same race, the other exit. An older read can REJECT after a newer one has
// already repainted the screen, and an unstamped catch then puts a failure
// sentence over numbers that were read successfully — the mirror of the case
// above, and the only thing that makes the stamp in the catch load-bearing.
test('an older read that is refused last does not put a failure over the newer numbers', async () => {
  const queue: ReturnType<typeof deferredPromise<ModelSettings>>[] = [];
  modelSettings.mockImplementation(() => {
    const d = deferredPromise<ModelSettings>();
    queue.push(d);
    return d.promise;
  });

  renderSection();
  await waitFor(() => expect(queue).toHaveLength(1));
  await emit(endedScan());
  await waitFor(() => expect(queue).toHaveLength(2));

  queue[1].resolve(read({ indexedFiles: 42, lastIndexedAt: null }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  queue[0].reject(new Error('STALE-REJECTION'));
  await tick();
  await tick();

  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('В індексі 42 файли.');
  expect(screen.queryByTestId('indexing-index-load-failed')).toBeNull();
  expect(pageText()).not.toContain('STALE-REJECTION');
});

// ---------------------------------------------------------------------------
// A rejected read (§10: a rejection arrives as a sentence, never as a kind).
// ---------------------------------------------------------------------------

test('a refused read shows the backend sentence and draws no numbers', async () => {
  const SENTENCE = 'the settings window could not reach the index';
  modelSettings.mockRejectedValue(new Error(SENTENCE));

  renderSection();

  await waitFor(() => expect(screen.getByTestId('indexing-index-load-failed')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-load-failed')))
    .toBe('Не вдалося прочитати стан індексу.');
  expect(visible(screen.getByTestId('indexing-index-load-error'))).toBe(SENTENCE);
  expect(screen.queryByTestId('indexing-index-files')).toBeNull();
  expect(screen.queryByTestId('indexing-index-date')).toBeNull();
  expect(screen.queryByTestId('indexing-index-never')).toBeNull();
  expect(screen.queryByTestId('indexing-index-failed-chunks')).toBeNull();
});

// 🔴 Both directions of the sentence's own lifetime. A failure handled at the
// mount alone leaves the section silent when a LATER re-read is refused; a
// failure never cleared leaves that sentence standing over numbers a later read
// confirmed. The numbers survive the failed re-read on purpose — `Tree.svelte`'s
// ruling: a count true a moment ago probably still is, and what the sentence
// adds is that it is no longer confirmed.
test('a re-read that is refused says so beside the numbers it could not confirm, and stops saying it once one succeeds', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 12, lastIndexedAt: null }));
  renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(screen.queryByTestId('indexing-index-load-failed')).toBeNull();

  const SENTENCE = 'the index went away mid-session';
  modelSettings.mockRejectedValue(new Error(SENTENCE));
  await emit(endedScan());

  await waitFor(() => expect(screen.getByTestId('indexing-index-load-failed')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-load-error'))).toBe(SENTENCE);
  // Kept, not blanked: what was true a moment ago probably still is.
  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('В індексі 12 файлів.');

  modelSettings.mockResolvedValue(read({ indexedFiles: 13, lastIndexedAt: null }));
  await emit(endedScan());

  await waitFor(() => expect(visible(screen.getByTestId('indexing-index-files'))).toBe('В індексі 13 файлів.'));
  expect(screen.queryByTestId('indexing-index-load-failed')).toBeNull();
  expect(screen.queryByTestId('indexing-index-load-error')).toBeNull();
});

// ---------------------------------------------------------------------------
// The whole window, read as a person reads it. A card that renders the right
// numbers under the wrong labels satisfies every testid assertion above.
// ---------------------------------------------------------------------------

test('a person who opens Indexing in the settings window reads what the index holds, in one breath', async () => {
  const at = HOUR_AGO();
  modelSettings.mockResolvedValue(read({ indexedFiles: 12, lastIndexedAt: at, failedChunks: 3 }));
  const { container } = render(Settings);
  const panel = () => container.querySelector('.spane');

  await fireEvent.click(screen.getByTestId('settings-nav-indexing'));

  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  // Equality over the whole panel, not containment: the heading already sits in
  // the nav, so a `toContain` over the page is satisfied by the nav alone and
  // never notices an empty panel or a line drawn under the wrong label.
  expect(visible(panel())).toBe(
    'Індексація В індексі 12 файлів.'
    + ` Останнє оновлення: ${dateIn('uk', at)}.`
    + ' Це було 1 годину тому.'
    + ` ${SPACE_SENTENCE}`,
  );
  // (review, Important 1) A `not.toContain` against `settings_section_not_ready`'s
  // old sentence stood here — Task 8 removed that key from the catalogue, so
  // the string can no longer be produced by anything and the assertion could
  // not fail. The equality check above is strictly stronger: it is exact over
  // the whole panel, not a containment claim over one page, so a placeholder
  // sentence appearing anywhere in the panel would already break it.
});

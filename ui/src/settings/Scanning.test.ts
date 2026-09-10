import { render, screen, cleanup, waitFor, fireEvent } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import { tick } from 'svelte';
import Scanning from './Scanning.svelte';
import { createJobController } from './jobs';
import { setLocale, t } from '../i18n';
import type { ModelSettings, ScanReport, ScanState } from '../lib/ipc';

// §9.3 — the Scanning SECTION (renamed from `Indexing.svelte` by Task 8): what
// the index holds, the ONE «Сканувати» control, and the continue row
// `continueAction` (`jobs.ts`) offers.
//
// 🔴 This file used to mock `modelSettings` and drive this section's OWN
// mount/refresh/subscription through it — Task 8 deleted all three from
// `Scanning.svelte`. What this section reads is now a PROP (`settings`,
// `loadError`), handed down by `Settings.svelte`'s single reader, so every
// test below builds that prop directly rather than answering a mocked IPC
// call. The re-read machinery itself (the `readSeq`/`ended` trigger, the
// `settingsSeq` stamp against two reads racing) moved with it — see
// `Settings.test.ts` for its own fixtures now.
//
// The job commands are still mocked at the wrapper boundary: this section
// still takes `jobs` to draw the «Сканувати»/«Продовжити» buttons through the
// REAL controller, and to read the running phase it gates the buttons on.
const startScanJob = vi.fn();
const cancelJob = vi.fn();
const jobStatus = vi.fn();
const listenScanProgress = vi.fn();
const unlisten = vi.fn();
// The `scan-progress` handler the controller registered, kept so a test can
// deliver the states a real scan would.
let deliver: ((state: ScanState) => void) | null = null;
vi.mock('../lib/ipc', () => ({
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
const dateIn = (loc: string, at: number) =>
  new Intl.DateTimeFormat(loc, { dateStyle: 'long' }).format(new Date(at * 1000)).replace(/\.$/, '');

beforeEach(() => {
  cancelJob.mockReset();
  jobStatus.mockReset();
  startScanJob.mockReset();
  startScanJob.mockResolvedValue(undefined);
  cancelJob.mockResolvedValue(undefined);
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
// browser collapses it.
const visible = (el: Element | null) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();
const pageText = () => visible(document.body);

// 🔴 The controller is MOUNTED, not merely created: `Settings.svelte` is what
// mounts it in the window, and until it is mounted nothing is listening for the
// states these tests deliver. `settings`/`loadError` are `Settings.svelte`'s
// own read, handed down directly — nothing here answers an IPC call for them.
function renderSection(
  settingsValue: ModelSettings | null = null,
  loadErrorValue: string | null = null,
  jobs = createJobController(),
) {
  jobs.mount();
  return {
    jobs,
    ...render(Scanning, { props: { jobs, settings: settingsValue, loadError: loadErrorValue } }),
  };
}

// Every state is newer than the one before it, because that is the only thing
// the controller compares (`apply`): a fixture that reused a revision would be
// dropped as stale and the test would be asserting about the state before it.
let revision = 0;
const IDLE_SCAN: ScanState = {
  revision: 0, files: 0, readSeq: 0, jobsDone: 0, lastReading: null, snapshot: { kind: 'idle' },
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

// `cancelled` by default: the ordinary resting state after a stop.
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
// What the index holds, and when it last grew (§9.3, D-e). Unchanged in
// substance from before Task 8 — only the wiring that hands `read` these
// numbers has moved.
// ---------------------------------------------------------------------------

test('a filled index says how many files it holds, the date it last grew, and how long ago that was', async () => {
  const at = HOUR_AGO();
  renderSection(read({ indexedFiles: 12, lastIndexedAt: at }));

  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('В індексі 12 файлів.');
  expect(visible(screen.getByTestId('indexing-index-date')))
    .toBe(`Останнє оновлення: ${dateIn('uk', at)}.`);
  expect(visible(screen.getByTestId('indexing-index-ago'))).toBe('Це було 1 годину тому.');
  // The statcard's own "updated" cell states the date too, never the
  // never-sentence, once there is one to state.
  expect(screen.queryByText('Ще нічого не проіндексовано.')).toBeNull();
});

test('the date follows the language, not the machine', async () => {
  const at = HOUR_AGO();
  renderSection(read({ indexedFiles: 12, lastIndexedAt: at }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-date')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-date')))
    .toBe(`Останнє оновлення: ${dateIn('uk', at)}.`);

  setLocale('en');
  await tick();

  expect(visible(screen.getByTestId('indexing-index-date')))
    .toBe(`Last updated: ${dateIn('en', at)}.`);
  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('The index holds 12 files.');
  expect(visible(screen.getByTestId('indexing-index-ago'))).toBe('That was 1 hour ago.');
  expect(dateIn('en', at)).not.toBe(dateIn('uk', at));
});

test('an index nothing has ever finished indexing says so, and draws no time at all', async () => {
  renderSection(read({ indexedFiles: 0, lastIndexedAt: null }));

  // Task 6, review round 1: the standalone paragraph is gone — the
  // statcard's own "updated" cell is the one and only place this sentence
  // renders now, so a duplicate would be the regression this line guards.
  await waitFor(() => expect(screen.getByTestId('indexing-statcard')).toBeTruthy());
  expect(screen.getAllByText('Ще нічого не проіндексовано.')).toHaveLength(1);
  expect(screen.queryByTestId('indexing-index-date')).toBeNull();
  expect(screen.queryByTestId('indexing-index-ago')).toBeNull();
  const text = pageText();
  expect(text).not.toContain('Invalid Date');
  expect(text).not.toContain('null');
  expect(text).not.toContain('1970');
  expect(text).not.toContain('щойно');
  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('В індексі 0 файлів.');
});

// ---------------------------------------------------------------------------
// Task 6 — the statcard: the same numbers, drawn as two labelled cells
// instead of a sentence. Never a decorative number: every fixture below reads
// its value from `read` or from the existing never-sentence, exactly as the
// sentences above already do.
// ---------------------------------------------------------------------------

function statcardValues(): (string | null)[] {
  return [...screen.getByTestId('indexing-statcard').querySelectorAll('dd')]
    .map((dd) => dd.textContent?.trim() ?? null);
}

test('the statcard states zero documents rather than an empty cell', async () => {
  const at = HOUR_AGO();
  renderSection(read({ indexedFiles: 0, lastIndexedAt: at }));

  await waitFor(() => expect(screen.getByTestId('indexing-statcard')).toBeTruthy());
  expect(statcardValues()).toEqual(['0', dateIn('uk', at)]);
});

test('the statcard\'s updated cell states the never sentence when nothing has ever grown the index', async () => {
  renderSection(read({ indexedFiles: 5, lastIndexedAt: null }));

  await waitFor(() => expect(screen.getByTestId('indexing-statcard')).toBeTruthy());
  expect(statcardValues()).toEqual(['5', 'Ще нічого не проіндексовано.']);
});

test('an unreadable index draws no statcard, only the error it could not get past', async () => {
  renderSection(settings({
    index: { kind: 'unreadable', cause: 'notOpen', reason: `could not open the index: ${TOKEN}` },
  }));

  await waitFor(() => expect(screen.getByTestId('indexing-index-unreadable')).toBeTruthy());
  expect(screen.queryByTestId('indexing-statcard')).toBeNull();
});

test('a failed re-read keeps the statcard\'s old numbers, with the error stated before them', async () => {
  const at = HOUR_AGO();
  renderSection(read({ indexedFiles: 7, lastIndexedAt: at }), 'the settings window could not reach the index');

  await waitFor(() => expect(screen.getByTestId('indexing-statcard')).toBeTruthy());
  const load = screen.getByTestId('indexing-index-load-failed');
  const card = screen.getByTestId('indexing-statcard');
  expect(!!(load.compareDocumentPosition(card) & Node.DOCUMENT_POSITION_FOLLOWING)).toBe(true);
  expect(statcardValues()).toEqual(['7', dateIn('uk', at)]);
});

// ---------------------------------------------------------------------------
// The index that could not be read (§10: branch on `kind`, never on the text).
// ---------------------------------------------------------------------------

const TOKEN = 'REASON-TOKEN-9f2c';

test('an index that is not open says so, and shows the backend reason verbatim', async () => {
  renderSection(settings({
    index: { kind: 'unreadable', cause: 'notOpen', reason: `could not open the index: ${TOKEN}` },
  }));

  await waitFor(() => expect(screen.getByTestId('indexing-index-unreadable')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-unreadable')))
    .toBe('Не вдалося прочитати індекс: він не відкритий.');
  expect(visible(screen.getByTestId('indexing-index-unreadable-reason')))
    .toBe(`Програма повідомила: could not open the index: ${TOKEN}`);
  expect(pageText()).not.toContain('спроба читання не вдалася');
  expect(screen.queryByTestId('indexing-index-files')).toBeNull();
  expect(screen.queryByTestId('indexing-index-date')).toBeNull();
  expect(screen.queryByTestId('indexing-statcard')).toBeNull();
  expect(pageText()).not.toContain('undefined');
});

test('an index whose read failed gets its own sentence, not the not-open one', async () => {
  renderSection(settings({
    index: { kind: 'unreadable', cause: 'readFailed', reason: `disk I/O error: ${TOKEN}` },
  }));

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
  renderSection(read({ indexedFiles: 5, failedChunks: 3 }));

  await waitFor(() => expect(screen.getByTestId('indexing-index-failed-chunks')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-failed-chunks'))).toBe(SPACE_SENTENCE);
});

test('an index the provider refused nothing in says nothing about refusals', async () => {
  renderSection(read({ indexedFiles: 5, failedChunks: 0 }));

  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(screen.queryByTestId('indexing-index-failed-chunks')).toBeNull();
  expect(pageText()).not.toContain('відхилив');
});

// 🔴 The other direction of the RUN's own sentence, and the one a suite that
// only ever ends scans with `Ran` cannot see.
test('an ending whose embedding never ran reports no refusals, and one that ran reports its own', async () => {
  renderSection(read({ indexedFiles: 5, failedChunks: 3 }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-failed-chunks')).toBeTruthy());

  await emit(endedScan({ reason: 'cancelled', embedding: { kind: 'notReached' } }));
  await tick();
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

test('a run that gave up on chunks and an index that already had some show two sentences, each about its own subject', async () => {
  renderSection(read({ indexedFiles: 5, failedChunks: 3 }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-failed-chunks')).toBeTruthy());

  await emit(endedScan({
    reason: 'completed', endedIn: 'embedding',
    embedding: { kind: 'ran', done: 3, total: 4, refused: 1 },
  }));

  await waitFor(() => expect(screen.getByTestId('indexing-index-refused-run')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-refused-run'))).toBe(RUN_SENTENCE);
  expect(visible(screen.getByTestId('indexing-index-failed-chunks'))).toBe(SPACE_SENTENCE);
  expect(RUN_SENTENCE).not.toBe(SPACE_SENTENCE);
});

// 🔴 Task 8's own version of the pairing (final review, Minor 5, carried
// forward): the run's own report outlives an index this window cannot read AT
// ALL, not only one whose re-read failed — there is no re-read here any more,
// `settings` is simply handed down `Unreadable` throughout. The subject is a
// pass, not the index, so the sentence stands on its own; the cumulative
// sentence beside it does not, because that one IS about the index and has no
// arm to be read from.
test('a pass that gave up on chunks says so even when the index cannot be read at all', async () => {
  renderSection(settings({
    index: { kind: 'unreadable', cause: 'notOpen', reason: `gone mid-pass: ${TOKEN}` },
  }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-unreadable')).toBeTruthy());

  await emit(endedScan({
    reason: 'completed', endedIn: 'embedding',
    embedding: { kind: 'ran', done: 3, total: 4, refused: 1 },
  }));

  await waitFor(() => expect(screen.getByTestId('indexing-index-refused-run')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-refused-run'))).toBe(RUN_SENTENCE);
  // Gone: its subject is the index, and the index cannot be read at all.
  expect(screen.queryByTestId('indexing-index-failed-chunks')).toBeNull();
  expect(screen.queryByTestId('indexing-index-files')).toBeNull();
});

// ---------------------------------------------------------------------------
// Task 8 — one «Сканувати» control, and the continue row `continueAction`
// (`jobs.ts`) offers. The `showPending`/«Продовжити вбудовування»-era tests
// that stood here are rewritten onto it below; `continueAction` is now the
// ONLY gate, and it already answers `null` while a run is under way.
// ---------------------------------------------------------------------------

// idle + `scanIncomplete` + a queue: the marker wins over the queue
// (`continueAction`'s own ordering, `jobs.ts`), so exactly one «Продовжити» —
// the incomplete sentence, not the queue line — and it resumes a full scan.
test('an incomplete scan outranks a waiting queue: one «Продовжити», the incomplete sentence, and it resumes the walk', async () => {
  renderSection(read({ scanIncomplete: true, pendingChunks: 10 }));

  await waitFor(() => expect(screen.getByTestId('scanning-continue')).toBeTruthy());
  expect(screen.queryAllByTestId('scanning-continue')).toHaveLength(1);
  expect(visible(screen.getByTestId('scanning-continue'))).toBe(t('indexing_resume'));
  expect(screen.getByTestId('scanning-incomplete')).toBeTruthy();
  expect(screen.queryByTestId('indexing-index-pending-chunks')).toBeNull();

  await fireEvent.click(screen.getByTestId('scanning-continue'));
  await waitFor(() => expect(startScanJob).toHaveBeenCalledTimes(1));
  expect(startScanJob).toHaveBeenCalledWith('full');
});

// idle + a queue alone: the queue line and «Продовжити вбудовування» resume
// embedding only — F2 (Task 10 live run): this arm's own key
// (`scanning_continue_embedding`), never `indexing_resume`, which is the
// marker arm's own promise to resume a half-read archive.
test('a waiting queue with no incomplete marker offers to resume embedding, not a full scan', async () => {
  renderSection(read({ scanIncomplete: false, pendingChunks: 10 }));

  await waitFor(() => expect(screen.getByTestId('scanning-continue')).toBeTruthy());
  expect(visible(screen.getByTestId('scanning-continue'))).toBe(t('scanning_continue_embedding'));
  expect(visible(screen.getByTestId('indexing-index-pending-chunks')))
    .toBe(t('indexing_index_pending_chunks', { count: 10 }));
  expect(screen.queryByTestId('scanning-incomplete')).toBeNull();

  await fireEvent.click(screen.getByTestId('scanning-continue'));
  await waitFor(() => expect(startScanJob).toHaveBeenCalledTimes(1));
  expect(startScanJob).toHaveBeenCalledWith('embedOnly');
});

// idle, neither marker: the scan button alone, no continue row of any kind.
test('idle with neither marker nor queue shows the scan button alone', async () => {
  renderSection(read({ scanIncomplete: false, pendingChunks: 0 }));

  await waitFor(() => expect(screen.getByTestId('scanning-scan')).toBeTruthy());
  expect(screen.queryByTestId('scanning-continue')).toBeNull();
  expect(screen.queryByTestId('scanning-incomplete')).toBeNull();
  expect(screen.queryByTestId('indexing-index-pending-chunks')).toBeNull();
});

// running: the strip owns Stop, so the section offers nothing at all — not the
// scan button, not a continue row, not the queue line — even though the same
// markers that would otherwise offer one are still set on the index.
test('a run under way hides the scan button and the continue row both', async () => {
  renderSection(read({ scanIncomplete: true, pendingChunks: 10 }));
  await waitFor(() => expect(screen.getByTestId('scanning-scan')).toBeTruthy());

  await emit(runningScan());

  expect(screen.queryByTestId('scanning-scan')).toBeNull();
  expect(screen.queryByTestId('scanning-continue')).toBeNull();
  expect(screen.queryByTestId('scanning-incomplete')).toBeNull();
  expect(screen.queryByTestId('indexing-index-pending-chunks')).toBeNull();
});

// Review round 1, Important 3. Task 5 gave this section its own
// `<ScanProgress>`, off the same `jobs.state` snapshot `showScanButton`
// already reads — nothing above named it directly, and the whole suite
// stayed green with the render deleted (only `JobStrip.test.ts`'s
// slot-contention test scopes its own query through `within`, which asks
// nothing about whether a SECOND projection exists at all). This is that
// direct assertion.
test('the section shows the same running-phase projection the strip does', async () => {
  renderSection(read());
  await waitFor(() => expect(screen.getByTestId('scanning-scan')).toBeTruthy());

  await emit(runningScan());

  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває вбудовування всього індексу.');
  expect(visible(screen.getByTestId('indexing-counts'))).toBe('Опрацьовано 1 з 4. Пропущено: 0. Відхилено: 0.');
});

// `ended` + `report.resume: null` + `scanIncomplete: true` — the
// embedOnly-from-Models-after-restart case: a re-embed started from
// `Models.svelte` reads no folder, so its report never names a resumption, and
// only the index's own marker says the earlier walk never finished.
test('an ended report naming no resumption still offers to continue when the index marks a walk incomplete', async () => {
  renderSection(read({ scanIncomplete: true, pendingChunks: 0 }));
  await waitFor(() => expect(screen.getByTestId('scanning-scan')).toBeTruthy());

  await emit(endedScan({ reason: 'completed', endedIn: 'embedding', resume: null }));

  await waitFor(() => expect(screen.getByTestId('scanning-continue')).toBeTruthy());
  expect(screen.getByTestId('scanning-incomplete')).toBeTruthy();
  await fireEvent.click(screen.getByTestId('scanning-continue'));
  await waitFor(() => expect(startScanJob).toHaveBeenCalledTimes(1));
  expect(startScanJob).toHaveBeenCalledWith('full');
});

// `ended` + `embedding: skipped{storeUnavailable}` + `resume: null` +
// `pendingChunks > 0` — a credential store that did not answer left the queue
// untouched, and the report names no resumption of its own either.
test('an ended report whose embedding was skipped for an unresponsive store still offers to resume the queue', async () => {
  renderSection(read({ scanIncomplete: false, pendingChunks: 7 }));
  await waitFor(() => expect(screen.getByTestId('scanning-scan')).toBeTruthy());

  await emit(endedScan({
    reason: 'completed', endedIn: 'embedding', resume: null,
    embedding: { kind: 'skipped', why: { kind: 'storeUnavailable', message: 'timed out' } },
  }));

  await waitFor(() => expect(screen.getByTestId('scanning-continue')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-pending-chunks')))
    .toBe(t('indexing_index_pending_chunks', { count: 7 }));
  await fireEvent.click(screen.getByTestId('scanning-continue'));
  await waitFor(() => expect(startScanJob).toHaveBeenCalledTimes(1));
  expect(startScanJob).toHaveBeenCalledWith('embedOnly');
});

test('the scan button starts a full scan, once', async () => {
  renderSection(read({ scanIncomplete: false, pendingChunks: 0 }));
  await waitFor(() => expect(screen.getByTestId('scanning-scan')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('scanning-scan'));

  await waitFor(() => expect(startScanJob).toHaveBeenCalledTimes(1));
  expect(startScanJob).toHaveBeenCalledWith('full');
  await fireEvent.click(screen.getByTestId('scanning-scan'));
  await waitFor(() => expect(startScanJob).toHaveBeenCalledTimes(2));
});

// D130: the section's own new labels follow the language, not the machine —
// `scanning_scan`, `scanning_incomplete` and the continue button's
// `indexing_resume`, all in the one state where all three are on screen at
// once (the incomplete marker still leaves the scan button showing).
test('the scan button, the incomplete sentence and the continue button all follow a language switch', async () => {
  renderSection(read({ scanIncomplete: true, pendingChunks: 10 }));
  await waitFor(() => expect(screen.getByTestId('scanning-continue')).toBeTruthy());
  expect(visible(screen.getByTestId('scanning-scan'))).toBe('Сканувати');
  expect(visible(screen.getByTestId('scanning-incomplete'))).toBe('Попереднє сканування не завершило індексацію тек.');
  expect(visible(screen.getByTestId('scanning-continue'))).toBe('Продовжити');

  setLocale('en');
  await tick();

  expect(visible(screen.getByTestId('scanning-scan'))).toBe('Scan');
  expect(visible(screen.getByTestId('scanning-incomplete')))
    .toBe('The previous scan did not finish indexing the folders.');
  expect(visible(screen.getByTestId('scanning-continue'))).toBe('Resume');
});

// The queue row's own wording, held apart from the test above because it is a
// DIFFERENT branch of `continueAction` (`entry: 'embedOnly'`, not `'full'`)
// with its own catalogue key.
test('the queue row follows the language too, when the offer is to resume embedding', async () => {
  renderSection(read({ scanIncomplete: false, pendingChunks: 5 }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-pending-chunks')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-pending-chunks'))).toBe('Ще не вбудовано 5 фрагментів.');

  setLocale('en');
  await tick();

  expect(visible(screen.getByTestId('indexing-index-pending-chunks')))
    .toBe('5 chunks are not embedded yet.');
});

// F2 (Task 10 live run): both `where: 'section'` arms, by their OWN visible
// text, held in one test so a fix that merged the two keys back together
// would fail one of the two assertions. The marker arm (`full`) says
// «Продовжити» — the same word the strip's own resume button says — and the
// queue arm (`embedOnly`) says «Продовжити вбудовування», because it resumes
// only the embedding half, never a folder that still needs reading.
test('the marker arm says «Продовжити», the queue arm says «Продовжити вбудовування», never the other one\'s word', async () => {
  renderSection(read({ scanIncomplete: true, pendingChunks: 0 }));
  await waitFor(() => expect(screen.getByTestId('scanning-continue')).toBeTruthy());
  expect(visible(screen.getByTestId('scanning-continue'))).toBe('Продовжити');
  cleanup();

  renderSection(read({ scanIncomplete: false, pendingChunks: 5 }));
  await waitFor(() => expect(screen.getByTestId('scanning-continue')).toBeTruthy());
  expect(visible(screen.getByTestId('scanning-continue'))).toBe('Продовжити вбудовування');

  setLocale('en');
  await tick();
  expect(visible(screen.getByTestId('scanning-continue'))).toBe('Continue embedding');
});

// A rejected read (§10: a rejection arrives as a sentence, never as a kind).
// `loadError` is a prop now, not a state this section fetches into — so this
// pins only that the section still shows it verbatim, and does not gate the
// scan button on it (an unreadable index or a failed read are reasons to try
// scanning, not reasons to hide the one button that starts one).
test('a failed read of the index is shown verbatim, and does not hide the scan button', async () => {
  const SENTENCE = 'the settings window could not reach the index';
  renderSection(null, SENTENCE);

  await waitFor(() => expect(screen.getByTestId('indexing-index-load-failed')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-load-failed')))
    .toBe('Не вдалося прочитати стан індексу.');
  expect(visible(screen.getByTestId('indexing-index-load-error'))).toBe(SENTENCE);
  expect(screen.queryByTestId('indexing-index-files')).toBeNull();
  expect(screen.getByTestId('scanning-scan')).toBeTruthy();
});

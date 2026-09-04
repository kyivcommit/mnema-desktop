import { render, screen, cleanup, waitFor, fireEvent } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import { tick } from 'svelte';
import Indexing from './Indexing.svelte';
import Settings from './Settings.svelte';
import { createJobController } from './jobs';
import { setLocale } from '../i18n';
import type { ModelSettings, JobEnded, JobEvent } from '../lib/ipc';

// The typed wrappers, not the raw `invoke` — the shape `Models.test.ts` uses.
const modelSettings = vi.fn();
const providerModels = vi.fn();
const listTree = vi.fn();
const listMasks = vi.fn();
const startWalkJob = vi.fn();
const startEmbedJob = vi.fn();
const cancelJob = vi.fn();
const jobStatus = vi.fn();
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
  startWalkJob: (...a: unknown[]) => startWalkJob(...a),
  startEmbedJob: (...a: unknown[]) => startEmbedJob(...a),
  cancelJob: (...a: unknown[]) => cancelJob(...a),
  jobStatus: (...a: unknown[]) => jobStatus(...a),
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
      failedChunks: 0, pendingChunks: 0, indexedFiles: 0, lastIndexedAt: null,
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
  startWalkJob.mockReset();
  startEmbedJob.mockReset();
  cancelJob.mockReset();
  jobStatus.mockReset();
  modelSettings.mockResolvedValue(settings());
  providerModels.mockResolvedValue(EMPTY_CATALOGUE);
  listTree.mockResolvedValue({ roots: [], recents: [] });
  listMasks.mockResolvedValue([]);
  startWalkJob.mockResolvedValue(undefined);
  startEmbedJob.mockResolvedValue(undefined);
  cancelJob.mockResolvedValue(undefined);
  jobStatus.mockResolvedValue({ running: false });
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

const renderSection = (jobs = createJobController()) =>
  ({ jobs, ...render(Indexing, { props: { jobs } }) });

// A real ending, in the shape `walk_job.rs`'s `ended_from_report` prints.
const ENDING: JobEnded = {
  reason: 'cancelled', done: 4, total: 4, skipped: 0, complete: false, frozen: [],
  indexed: 0, unchanged: 0, refused: 0, removed: 0, message: null,
};
const ended = (over: Partial<JobEnded> = {}): JobEvent =>
  ({ event: 'ended', data: { ...ENDING, ...over } });
const progress = (): JobEvent => ({
  event: 'progress',
  data: { done: 1, total: 4, skipped: 0, refused: 0, contended: 0, secondsLeft: null },
});

// Drives the controller the way the window does, and takes the channel back.
// `cancelled` by default so the walk does NOT chain the embedding pass: a chain
// reads `model_settings` for its own preconditions, and every count below would
// then be counting two different things.
async function walkChannel(jobs: ReturnType<typeof createJobController>) {
  void jobs.scan(1);
  await waitFor(() => expect(startWalkJob).toHaveBeenCalled());
  const send = startWalkJob.mock.calls[0][1] as (e: JobEvent) => void;
  return async (event: JobEvent) => { send(event); await tick(); };
}

// The embedding pass, for the one case whose ending has to carry `refused`.
// `job::Progress::refused` is written only by `embed_job.rs` — a walk's ending
// carries `0` there for good — so this is the only channel on which that
// sentence can honestly arrive. An embed ending chains nothing, so the `ended`
// phase it leaves is the last word.
async function embedChannel(jobs: ReturnType<typeof createJobController>) {
  void jobs.embed();
  await waitFor(() => expect(startEmbedJob).toHaveBeenCalled());
  const send = startEmbedJob.mock.calls[0][0] as (e: JobEvent) => void;
  return async (event: JobEvent) => { send(event); await tick(); };
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

// 🔴 The state a suite without it cannot read: one sentence and two sentences
// look the same until both scopes are on the screen at once. `job.rs:38-44`
// says these are two numbers about two scopes, and whichever surface shows them
// owes each its own words.
test('a run that gave up on chunks and an index that already had some show two sentences, each about its own subject', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, failedChunks: 3 }));
  const { jobs } = renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-index-failed-chunks')).toBeTruthy());

  const send = await embedChannel(jobs);
  await send(ended({ reason: 'completed', complete: true, refused: 1 }));

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
  const { jobs } = renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-index-failed-chunks')).toBeTruthy());
  const send = await embedChannel(jobs);

  // What the ending's own re-read finds.
  modelSettings.mockResolvedValue(settings({
    index: { kind: 'unreadable', cause: 'notOpen', reason: `gone mid-pass: ${TOKEN}` },
  }));
  await send(ended({ reason: 'completed', complete: true, refused: 1 }));

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
  const { jobs } = renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-index-pending-chunks')).toBeTruthy());

  const send = await embedChannel(jobs);
  await send(progress());

  expect(screen.queryByTestId('indexing-index-pending-chunks')).toBeNull();
  expect(screen.queryByTestId('indexing-resume-embedding')).toBeNull();
});

// The other `starting`/`running`/`runningUnobserved` arm review found
// unasserted (Important 1): a press has been made and `chain`'s own
// precondition read of `model_settings` may still be in flight, or
// `startEmbedJob` itself has not yet called back — the window's own opening
// answer (`jobs.ts`'s `store.set` before either await), and the callback
// `onEvent` reacts to has not fired even once. `deferredPromise` (below)
// leaves `startEmbedJob`'s own promise unsettled so the phase cannot advance
// past `starting` on its own.
test('a queue is not offered while the pass is still starting', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));
  const { jobs } = renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-resume-embedding')).toBeTruthy());

  const deferred = deferredPromise<void>();
  startEmbedJob.mockReturnValue(deferred.promise);
  void jobs.embed();
  await waitFor(() => expect(startEmbedJob).toHaveBeenCalled());
  await tick();

  expect(screen.queryByTestId('indexing-index-pending-chunks')).toBeNull();
  expect(screen.queryByTestId('indexing-resume-embedding')).toBeNull();
});

// `runningUnobserved` — the settings window reopened while a pass this
// window has no channel for is still going (`jobs.ts:344-357`,
// `syncFromStatus`), which is the same F4 scenario one step later: a person
// who stopped a run, closed the window, reopened it, and started another
// pass from elsewhere before checking back. No counts are drawn for it and
// none arrive, so the queue's own line must not either.
test('a queue is not offered while a job is running unobserved', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));
  const { jobs } = renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-resume-embedding')).toBeTruthy());

  jobStatus.mockResolvedValue({ running: true });
  await jobs.syncFromStatus();
  await tick();

  expect(screen.queryByTestId('indexing-index-pending-chunks')).toBeNull();
  expect(screen.queryByTestId('indexing-resume-embedding')).toBeNull();
});

// Through the controller, exactly the way `Models.svelte:468`'s own `reembed`
// argues: the pass this button starts belongs on the window's strip, where its
// progress and its Cancel stay reachable from every section, not to a listener
// this component alone can hear. `scan` is asserted never-called specifically
// because the folder-scan chain is the OTHER way to reach this same pass, and
// a button that quietly called the wrong one would still leave the queue
// embedding — just from underneath a walk nobody asked for.
test('the resume button starts the embedding pass through the controller, never a scan', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));
  renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-resume-embedding')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('indexing-resume-embedding'));

  await waitFor(() => expect(startEmbedJob).toHaveBeenCalledTimes(1));
  expect(startWalkJob).not.toHaveBeenCalled();
});

// The `ended` half of the gate (Important 1, review) — the state the button
// was written FOR. `refresh()` fires on `phase.kind === 'ended'` and nothing
// afterwards moves the phase back to `idle`, so a pass cancelled from the
// window's own strip with chunks still owed lands here and stays here: this
// is the ordinary resting state after a stop, not a transient one. The queue
// stays positive across the ending's own re-read on purpose — a completed
// pass that did not clear the whole queue, or a cancelled one — so this test
// is told apart from "the empty queue hid it" the other `ended` test already
// covers.
test('an ended pass with chunks still owed still shows the line and the button', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));
  const { jobs } = renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-resume-embedding')).toBeTruthy());

  const send = await embedChannel(jobs);
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));
  await send(ended({ reason: 'completed', complete: true }));

  await waitFor(() => expect(screen.getByTestId('indexing-index-pending-chunks')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-pending-chunks'))).toBe(PENDING_SENTENCE);
  expect(screen.getByTestId('indexing-resume-embedding')).toBeTruthy();
});

// The line and the button re-derive once the pass they started ends, the same
// re-read every other ending on this section already triggers
// (`refresh()` on `phase.kind === 'ended'`) — reusing the phase fixtures and
// the `modelSettings` call-count pattern the refresh tests below pin.
test('once the resumed pass ends, the section re-reads and the queue reflects what is left', async () => {
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 5 }));
  const { jobs } = renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-resume-embedding')).toBeTruthy());

  // Taken AFTER `embedChannel` rather than after the mount: starting an embed
  // makes its own precondition read of `model_settings` (`jobs.ts`'s `chain`),
  // which is not the read this test is about. What it pins is the ONE further
  // read the ending itself triggers — the same `refresh()` on `phase.kind ===
  // 'ended'` every other ending on this section already causes.
  const send = await embedChannel(jobs);
  const beforeEnding = modelSettings.mock.calls.length;
  modelSettings.mockResolvedValue(read({ indexedFiles: 5, pendingChunks: 0 }));
  await send(ended({ reason: 'completed', complete: true }));

  await waitFor(() => expect(modelSettings.mock.calls.length).toBe(beforeEnding + 1));
  await waitFor(() => expect(screen.queryByTestId('indexing-index-pending-chunks')).toBeNull());
  expect(screen.queryByTestId('indexing-resume-embedding')).toBeNull();
});

// ---------------------------------------------------------------------------
// The refresh, its trigger, and its lifetime.
// ---------------------------------------------------------------------------

test('an ending re-reads the index, and each further ending re-reads it again', async () => {
  const { jobs } = renderSection();
  await waitFor(() => expect(modelSettings).toHaveBeenCalledTimes(1));
  const send = await walkChannel(jobs);

  await send(ended());
  await waitFor(() => expect(modelSettings).toHaveBeenCalledTimes(2));

  await send(ended());
  await waitFor(() => expect(modelSettings).toHaveBeenCalledTimes(3));
});

// The mirror. A subscriber that re-fetches on every store emission passes the
// test above and fails here — and it is the only thing that tells the two apart.
test('a progress report is not an ending and re-reads nothing', async () => {
  const { jobs } = renderSection();
  await waitFor(() => expect(modelSettings).toHaveBeenCalledTimes(1));
  const send = await walkChannel(jobs);

  await send(progress());
  await send(progress());
  await tick();

  expect(modelSettings).toHaveBeenCalledTimes(1);
});

// 🔴 Counted, not "at least once". A section that never unsubscribes answers
// three times here and satisfies every looser assertion.
test('a section left behind by a nav change stops listening — three mounts, one ending, one re-read', async () => {
  const jobs = createJobController();
  render(Indexing, { props: { jobs } }).unmount();
  render(Indexing, { props: { jobs } }).unmount();
  render(Indexing, { props: { jobs } });
  await waitFor(() => expect(modelSettings).toHaveBeenCalledTimes(3));
  const send = await walkChannel(jobs);

  await send(ended());

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

  const { jobs } = renderSection();
  await waitFor(() => expect(queue).toHaveLength(1)); // the mount's read
  const send = await walkChannel(jobs);
  await send(ended());
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

  const { jobs } = renderSection();
  await waitFor(() => expect(queue).toHaveLength(1));
  const send = await walkChannel(jobs);
  await send(ended());
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
  const { jobs } = renderSection();
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(screen.queryByTestId('indexing-index-load-failed')).toBeNull();
  const send = await walkChannel(jobs);

  const SENTENCE = 'the index went away mid-session';
  modelSettings.mockRejectedValue(new Error(SENTENCE));
  await send(ended());

  await waitFor(() => expect(screen.getByTestId('indexing-index-load-failed')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-load-error'))).toBe(SENTENCE);
  // Kept, not blanked: what was true a moment ago probably still is.
  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('В індексі 12 файлів.');

  modelSettings.mockResolvedValue(read({ indexedFiles: 13, lastIndexedAt: null }));
  await send(ended());

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

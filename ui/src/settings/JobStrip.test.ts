import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import { tick } from 'svelte';
import Settings from './Settings.svelte';
import { setLocale, type Loc } from '../i18n';
import { END_REASONS } from '../lib/ipc';
import type { Counts, EndReason, ModelSettings, ScanReport, ScanState } from '../lib/ipc';

// ⚠️ **This file was cut down by PR 9b Task 6, and Task 7 is what restores it.**
//
// The strip used to be drawn from a channel of job events, and this file was
// its whole matrix: the per-outcome sentences with their result lines, the
// frozen-subtree rows, the embedding pass's own block, the `noKey`/`noModel`
// notes, the «a job we cannot hear» state, and the chained-pass races. Task 6
// replaced the channel with one versioned `ScanState` and rewrote the strip to
// the SMALLEST thing that can be drawn from it, so the tests for what it no
// longer draws are gone rather than weakened.
//
// What went, and where it comes back — all of it **Task 7**, by name:
//   - the reading outcome from `scan.lastReading`: the per-root rows, the
//     frozen prefixes, the folders-read count, the partly-read sentence;
//   - the embedding block from `report.embedding` (`notReached` / `skipped`
//     with its three reasons / `ran` with its counts);
//   - the removal and probe phases, which draw no line here at all today;
//   - the continue button from `continueAction`, and the resume/retry wording.
// The controller-side races those tests used to reach through the strip —
// supersession, a late reply against a newer event, destroy during a read — are
// `jobs.test.ts`'s now, where they are about the reducer rather than about a
// screen that happened to show it.
//
// Only Tauri's own modules are faked. The whole settings window renders — the
// nav, the sections and the strip — because the claim this file makes is about
// what a person READS on that window, and every previous round of this project
// that pinned a testid or a count instead shipped a defect a screenshot found
// in a minute.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...a: unknown[]) => invoke(...a),
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

// The window's one subscription to the scan. Faked at the module boundary so
// `listenScanProgress`'s own event name and its unwrapping of `e.payload` are
// exercised rather than described.
const listen = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({
  listen: (...a: unknown[]) => listen(...a),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

// Two roots, and their ids are NOT their positions: a component that sends the
// index of the row it drew, or always the first id, cannot pass.
const ROOTS = {
  roots: [
    { rootId: 1, absolutePath: '/home/a/notes', name: 'notes', files: [] },
    { rootId: 4, absolutePath: '/home/a/papers', name: 'papers', files: [] },
  ],
  recents: [],
};

// 🔴 Annotated `ModelSettings`, and the annotation is the guard rather than
// documentation. This fixture crosses an UNTYPED mock (`invoke` answers
// `unknown`), so without it a required field added to the `read` arm reaches
// the §9.3 section as `undefined` and every test that stands on the Indexing
// nav item dies somewhere unrelated.
const READY_SETTINGS: ModelSettings = {
  key: { kind: 'present' },
  index: {
    kind: 'read', embeddingModel: 'openai/text-embedding-3-small', chatModel: null,
    embeddedChunks: 12, embeddedChunksEverywhere: 12, totalChunks: 12,
    failedChunks: 0, pendingChunks: 0, indexedFiles: 9, lastIndexedAt: 1_700_000_000,
    scanIncomplete: false, searchTextArm: true, searchContentArm: true,
  },
  platform: 'linux',
};

const EMPTY_CATALOGUE = { entries: [], unreadable: 0, unreadableRecords: [] };

// ---------------------------------------------------------------------------
// The states. Every shape is the one `scan_state.rs` pins as JSON
// (`every_snapshot_has_its_wire_shape_pinned`), and every one is newer than the
// last, because the revision is the only thing the controller compares.
// ---------------------------------------------------------------------------
let revision = 0;
const IDLE_SCAN: ScanState = {
  revision: 0, files: 0, readSeq: 0, lastReading: null, snapshot: { kind: 'idle' },
};

const COUNTS: Counts = { done: 3, total: 8, skipped: 1, refused: 0, contended: 0, secondsLeft: null };

const reading = (counts: Partial<Counts> = {}, cancellable = true): ScanState => ({
  ...IDLE_SCAN,
  revision: (revision += 1),
  snapshot: {
    kind: 'running',
    cancellable,
    phase: {
      kind: 'reading', rootIndex: 0, rootCount: 2, rootPath: '/home/a/notes',
      counts: { ...COUNTS, ...counts },
    },
  },
});

const embedding = (counts: Partial<Counts> = {}): ScanState => ({
  ...IDLE_SCAN,
  revision: (revision += 1),
  snapshot: {
    kind: 'running',
    cancellable: true,
    phase: { kind: 'embedding', counts: { ...COUNTS, ...counts } },
  },
});

const removing = (): ScanState => ({
  ...IDLE_SCAN,
  revision: (revision += 1),
  snapshot: {
    kind: 'running',
    cancellable: false,
    phase: { kind: 'removing', rootPath: '/home/a/papers' },
  },
});

const ended = (over: Partial<ScanReport> = {}): ScanState => ({
  ...IDLE_SCAN,
  revision: (revision += 1),
  snapshot: {
    kind: 'ended',
    report: {
      embedding: { kind: 'notReached' },
      endedIn: 'reading',
      reason: 'completed',
      message: null,
      resume: null,
      ...over,
    },
  },
});

type Replies = Record<string, unknown>;
let replies: Replies = {};

function reply(extra: Replies = {}) {
  replies = {
    list_tree: ROOTS,
    model_settings: READY_SETTINGS,
    provider_models: EMPTY_CATALOGUE,
    job_status: IDLE_SCAN,
    start_scan_job: undefined,
    cancel_job: undefined,
    app_prefs: {
      hotkey: { shortcut: 'Alt+Space', status: { kind: 'registered' } },
      autostart: { kind: 'disabled' },
      version: '0.0.0',
      platform: 'linux',
    },
    set_hotkey: undefined,
    set_autostart: undefined,
    ...extra,
  };
}

let deliver: ((state: ScanState) => void) | null = null;
const unlisten = vi.fn();

beforeEach(() => {
  invoke.mockReset();
  listen.mockReset();
  unlisten.mockReset();
  deliver = null;
  revision = 0;
  reply();
  invoke.mockImplementation((cmd: string) => {
    const r = replies[cmd];
    if (r instanceof Error) return Promise.reject(r);
    return Promise.resolve(r);
  });
  listen.mockImplementation((_name: string, cb: (e: { payload: ScanState }) => void) => {
    deliver = (state: ScanState) => cb({ payload: state });
    return Promise.resolve(unlisten);
  });
  setLocale('uk');
});

afterEach(() => {
  cleanup();
  setLocale('en');
});

const calls = (cmd: string) => invoke.mock.calls.filter((c) => c[0] === cmd);

// What a person reads, with the markup's own indentation collapsed the way a
// browser collapses it. Nobody sees the newline between two <span>s.
const visible = (el: Element | null) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();

// The whole window, with its one subscription open.
async function openWindow(loc: Loc = 'uk') {
  setLocale(loc);
  const rendered = render(Settings);
  await waitFor(() => expect(deliver).not.toBeNull());
  return rendered;
}

// One state, the way the core's own observer sends it.
async function emit(state: ScanState) {
  if (deliver === null) throw new Error('nothing is listening to scan-progress');
  deliver(state);
  await tick();
}

const strip = () => screen.queryByTestId('indexing');

// ---------------------------------------------------------------------------
// When the strip is there at all.
// ---------------------------------------------------------------------------

// Both directions. A strip that is always there, saying nothing, is noise on a
// window somebody opened to change a model; one that never appears is a running
// scan a person cannot stop.
test('nothing on screen until something has happened, and the strip the moment it has', async () => {
  await openWindow();
  expect(strip()).toBeNull();

  await emit(reading());

  expect(strip()).not.toBeNull();
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває читання теки.');
});

// ⚠️ Task 7 gives the removal its own sentence («we are emptying this folder»)
// and the probe none. Until then this build has no words for either, so it
// draws no line rather than borrowing the reading's — which would tell a person
// their documents are being read while a folder is being deleted.
//
// The pair: a phase with words against a phase without. `removing` is also not
// cancellable (`bridge.rs` claims it with `false`), so nothing at all is owed
// and the strip stays away.
test('a phase this build has no words for draws no line rather than another phase`s', async () => {
  await openWindow();

  await emit(removing());
  expect(strip()).toBeNull();

  await emit(embedding());
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває вбудовування всього індексу.');
});

// ---------------------------------------------------------------------------
// A running scan: what it says, and what it offers.
// ---------------------------------------------------------------------------

test('the running line reads as words, with the counts in them', async () => {
  await openWindow();

  await emit(reading({ done: 3, total: 8, skipped: 1, refused: 0 }));

  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває читання теки.');
  expect(visible(screen.getByTestId('indexing-counts')))
    .toBe('Опрацьовано 3 з 8. Пропущено: 1. Відхилено: 0.');
});

// `total: 0` is not an edge case: a reading pass reports it before phase 1 has
// counted anything, and a folder that could not be entered reports zero of zero
// for good. "0 of 0" reads as "nothing to do" while a run is under way.
test('a run with nothing counted yet says so instead of reading as nothing to do', async () => {
  await openWindow();

  await emit(reading({ done: 4, total: 0 }));

  expect(visible(screen.getByTestId('indexing-counts')))
    .toBe('Опрацьовано 4. Скільки їх усього, поки не відомо. Пропущено: 1. Відхилено: 0.');
});

// Both directions. The busy line EXPLAINS part of the skipped number beside it
// and adds nothing to it, so the counts line is asserted unchanged in the same
// breath — a build that added the two would count one file twice.
test('a scan that met a busy index says so without touching the counts, and one that did not says nothing', async () => {
  await openWindow();

  await emit(reading({ contended: 2 }));
  expect(visible(screen.getByTestId('indexing-contended'))).toBe(
    'Індекс саме зайнятий іншим записом, тож частину файлів цей скан не записав.'
    + ' Наступне сканування спробує їх знову.',
  );
  expect(visible(screen.getByTestId('indexing-counts')))
    .toBe('Опрацьовано 3 з 8. Пропущено: 1. Відхилено: 0.');

  await emit(reading({ contended: 0 }));
  expect(screen.queryByTestId('indexing-contended')).toBeNull();
});

// Both directions on a field whose `Option<u64>` has a real `None`: nought
// seconds left is an estimate, and a truthiness check would print "not known
// yet" for it.
test('nought seconds left is an estimate, and an absent one says it is not known', async () => {
  await openWindow();

  await emit(reading({ secondsLeft: 0 }));
  expect(visible(screen.getByTestId('indexing-eta'))).toBe('Залишилось приблизно 0 с.');

  await emit(reading({ secondsLeft: null }));
  expect(visible(screen.getByTestId('indexing-eta'))).toBe('Скільки ще лишилось часу, поки не відомо.');
});

// 🔴 Stop follows `cancellable` and nothing else. It is fixed for the life of
// the job (`scan_state.rs`) — a phase change does not make an uninterruptible
// job interruptible — so a strip inferring it from the phase would offer a
// button that does nothing. Both directions, on two states that differ in that
// field alone.
test('Stop is offered exactly when the core says the job may be stopped', async () => {
  await openWindow();

  await emit(reading({}, true));
  expect(screen.getByTestId('indexing-cancel')).toBeTruthy();

  await emit(reading({}, false));
  expect(screen.queryByTestId('indexing-cancel')).toBeNull();
});

test('an idle window offers no Stop at all', async () => {
  await openWindow();

  expect(screen.queryByTestId('indexing-cancel')).toBeNull();
  expect(calls('cancel_job')).toHaveLength(0);
});

// The press asks the backend and invents nothing: the job reports its own stop
// through the observer, and a line written here would be a claim about a slot
// this window has not read.
test('Stop asks the backend, and the ending that follows is what says it stopped', async () => {
  await openWindow();
  await emit(reading());

  await fireEvent.click(screen.getByTestId('indexing-cancel'));

  expect(calls('cancel_job')).toHaveLength(1);
  expect(calls('cancel_job')[0]).toHaveLength(1); // the command name alone
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває читання теки.');

  await emit(ended({ reason: 'cancelled' }));

  expect(visible(screen.getByTestId('indexing-ended'))).toBe('Сканування зупинено на ваше прохання.');
  expect(screen.queryByTestId('indexing-cancel')).toBeNull();
});

// ---------------------------------------------------------------------------
// An ending, which is a STATE and not an event.
// ---------------------------------------------------------------------------

// The four rows after `failed` are the ones a table of three would lose. They
// are NOT malfunctions — a broken helper, rules that did not take, an
// unreadable folder, a volume that may be gone — and `job.rs` says reporting
// them as `failed` tells a person something broke when instead a folder cannot
// be read. A test covering only completed/cancelled/failed passes on a
// component that collapses all four into one sentence.
const ENDED_SENTENCES: Record<EndReason, { uk: string; en: string }> = {
  completed: {
    uk: 'Теку прочитано повністю.',
    en: 'The folder was read in full.',
  },
  cancelled: {
    uk: 'Сканування зупинено на ваше прохання.',
    en: 'The scan was stopped at your request.',
  },
  failed: {
    uk: 'Сканування обірвалося через збій.',
    en: 'The scan broke off because something went wrong.',
  },
  brokenWorker: {
    uk: 'Сканування спинилося: допоміжна програма, яка читає файли, перестала відповідати.',
    en: 'The scan stopped: the helper program that reads files stopped answering.',
  },
  rulesNotApplied: {
    uk: 'Сканування спинилося: правила виключення не вдалося застосувати, тож теку не читали зовсім.',
    en: 'The scan stopped: the exclusion rules could not be applied, so the folder was not read at all.',
  },
  rootUnavailable: {
    uk: 'Сканування спинилося: у теку не вдалося зайти. Можливо, її прибрали або диск від’єднано.',
    en: 'The scan stopped: the folder could not be entered. It may have been removed, or its drive disconnected.',
  },
  volumeMissing: {
    uk: 'Сканування спинилося: тека прочиталася порожньою, хоча в індексі є файли з неї.'
      + ' Нічого не вилучено — можливо, диск під’єднано не повністю.',
    en: 'The scan stopped: the folder read as empty although the index still holds files from it.'
      + ' Nothing was deleted — the drive may not be fully attached.',
  },
};

// Both directions on the table itself: a wire reason this file says nothing
// about, and a row here for a reason the wire cannot carry, are both defects.
test('the table names one sentence for every reason the wire can carry, and no others', () => {
  expect(Object.keys(ENDED_SENTENCES).sort()).toEqual([...END_REASONS].sort());
});

test.each(['uk', 'en'] as const)('every reason a scan can end for shows its own sentence, and no two share one (%s)', async (loc) => {
  await openWindow(loc);
  const seen: string[] = [];

  for (const reason of END_REASONS) {
    await emit(ended({ reason }));
    const shown = visible(screen.getByTestId('indexing-ended'));
    expect(shown, reason).toBe(ENDED_SENTENCES[reason][loc]);
    seen.push(shown);
  }

  // The cardinality claim, which the per-row assertions above cannot make: a
  // component that drew one sentence for the four after `failed` would satisfy
  // every one of them if the table repeated itself.
  expect(new Set(seen).size).toBe(END_REASONS.length);
});

// Both directions. `message` is `Option<String>` on the wire, so its absence is
// a shape the screen has to survive rather than a case that cannot happen — and
// when it is there it is the ONLY thing telling a panic, a broken pool and a
// missing worker binary apart.
test('a failure carries its own text, and survives its absence', async () => {
  await openWindow();

  await emit(ended({ reason: 'failed', message: 'the worker binary could not be started' }));
  expect(visible(screen.getByTestId('indexing-ended'))).toBe(
    'Сканування обірвалося через збій.'
    + ' Програма повідомила: the worker binary could not be started',
  );

  await emit(ended({ reason: 'failed', message: null }));
  expect(screen.queryByTestId('indexing-ended-failure')).toBeNull();
  expect(visible(screen.getByTestId('indexing-ended'))).toBe('Сканування обірвалося через збій.');
});

// 🔴 An ending is a STATE: a scan that finished stays finished until the next
// job claims the slot. The pair this separates is «the window heard the ending»
// from «the window was opened after it» — and the second is the one the channel
// could never answer, because there was nothing left to hear.
test('a window opened after the scan ended still reads how it went', async () => {
  reply({ job_status: { ...ended({ reason: 'volumeMissing' }) } });

  await openWindow();

  await waitFor(() => expect(screen.getByTestId('indexing-ended')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-ended'))).toBe(ENDED_SENTENCES.volumeMissing.uk);
});

// ---------------------------------------------------------------------------
// A refused command.
// ---------------------------------------------------------------------------

// A rejection crosses the IPC as text (`error.rs`) and nothing branches on it:
// one lead-in the catalogue owns, then the backend's own sentence verbatim.
test('a refused scan shows the lead-in and the backend sentence verbatim', async () => {
  reply({ start_scan_job: new Error('LEAK-TOKEN-ANOTHER-JOB') });
  await openWindow();
  await fireEvent.click(screen.getByTestId('settings-nav-folders'));
  await screen.findByTestId('folder-row-4');

  await fireEvent.click(screen.getByTestId('folder-scan-1'));

  await waitFor(() => expect(screen.getByTestId('indexing-note')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-note'))).toBe('Запит відхилено.');
  expect(visible(screen.getByTestId('indexing-rejection'))).toBe('LEAK-TOKEN-ANOTHER-JOB');
});

// 🔴 The sentence alone cannot say what the slot now holds, so the controller
// asks again — and the commonest reason this command is refused is that another
// job is running, whose Stop a person must keep. A build that only reported the
// sentence would leave the window with no way to stop the job it just collided
// with.
test('a scan refused because another job holds the slot leaves that job`s Stop in place', async () => {
  reply({
    start_scan_job: new Error('another job is already running'),
    job_status: { ...reading() },
  });
  await openWindow();
  await fireEvent.click(screen.getByTestId('settings-nav-folders'));
  await screen.findByTestId('folder-row-4');

  await fireEvent.click(screen.getByTestId('folder-scan-1'));

  await waitFor(() => expect(screen.getByTestId('indexing-rejection')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває читання теки.');
  expect(screen.getByTestId('indexing-cancel')).toBeTruthy();
});

// ---------------------------------------------------------------------------
// The strip belongs to the WINDOW, not to a section.
// ---------------------------------------------------------------------------

// 🔴 The live run's finding 3, and the reason the controller is created in
// `Settings.svelte` above every section. A controller built inside a section
// dies with the first nav click, taking the counters AND the Stop with it — and
// `cancel_job` needs no channel at all, so that Stop would be lost for nothing.
test('a scan survives switching sections, and Stop still stops it afterwards', async () => {
  await openWindow();
  await emit(reading());

  await fireEvent.click(screen.getByTestId('settings-nav-folders'));
  await screen.findByTestId('folder-row-4');
  await fireEvent.click(screen.getByTestId('settings-nav-models'));
  await tick();

  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває читання теки.');
  expect(visible(screen.getByTestId('indexing-counts')))
    .toBe('Опрацьовано 3 з 8. Пропущено: 1. Відхилено: 0.');

  await fireEvent.click(screen.getByTestId('indexing-cancel'));

  expect(calls('cancel_job')).toHaveLength(1);
});

// 🔴 The subscription belongs to the window and goes with it. `onMount` calls
// what its callback RETURNS on destroy, so a mount that starts the listener and
// returns nothing leaves it live for the life of the process — and every
// reopened window would add another. Both halves: the unlisten is called
// exactly once, and a state delivered afterwards changes nothing.
test('closing the window unsubscribes once, and a later state reaches nothing', async () => {
  const { unmount } = await openWindow();
  await emit(reading());
  expect(screen.getByTestId('indexing-pass')).toBeTruthy();

  unmount();

  expect(unlisten).toHaveBeenCalledTimes(1);
  await emit(ended({ reason: 'volumeMissing' }));
  expect(unlisten).toHaveBeenCalledTimes(1);
  expect(strip()).toBeNull();
});

// The other half of the same rule: a state that arrives while another section
// is on screen is on the strip when the person gets back, because the strip
// never went away.
test('a scan that ends on another section leaves its report on the strip', async () => {
  await openWindow();
  await emit(reading());
  await fireEvent.click(screen.getByTestId('settings-nav-application'));
  await tick();

  await emit(ended({ reason: 'brokenWorker' }));

  expect(visible(screen.getByTestId('indexing-ended'))).toBe(ENDED_SENTENCES.brokenWorker.uk);
  expect(screen.queryByTestId('indexing-cancel')).toBeNull();
});

// ---------------------------------------------------------------------------
// The sections that read the ending.
// ---------------------------------------------------------------------------

// The live run's finding 1: the folder row went on stating zero indexed
// documents while the report under it said four had been added. Both
// directions, because a list that re-read on every store emission would satisfy
// the first half alone.
test('the folder list re-reads when a scan ends, and not while one runs', async () => {
  await openWindow();
  await fireEvent.click(screen.getByTestId('settings-nav-folders'));
  await screen.findByTestId('folder-row-4');
  const before = calls('list_tree').length;

  await emit(reading());
  await tick();
  expect(calls('list_tree')).toHaveLength(before);

  await emit(ended());

  await waitFor(() => expect(calls('list_tree').length).toBe(before + 1));
});

test('the indexing section re-reads what the index holds when a scan ends', async () => {
  await openWindow();
  await fireEvent.click(screen.getByTestId('settings-nav-indexing'));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  const before = calls('model_settings').length;

  await emit(ended());

  await waitFor(() => expect(calls('model_settings').length).toBe(before + 1));
});

// ---------------------------------------------------------------------------
// The language, which every line has to follow after mount.
// ---------------------------------------------------------------------------

// D130: the switch is reactive, not a remount. A line built once at mount reads
// correctly in the language the window opened in and never changes again, which
// is exactly the defect a test that only ever seeds one locale cannot see.
test('a language switch during a scan reaches the line, the counts, the estimate and Stop', async () => {
  await openWindow('uk');
  await emit(reading({ contended: 2, secondsLeft: 12 }));

  setLocale('en');
  await tick();

  expect(visible(screen.getByTestId('indexing-pass'))).toBe('The folder is being read.');
  expect(visible(screen.getByTestId('indexing-counts')))
    .toBe('Processed 3 of 8. Skipped: 1. Given up on: 0.');
  expect(visible(screen.getByTestId('indexing-contended'))).toBe(
    'The index is busy with another write, so this scan did not write some files.'
    + ' The next scan will try them again.',
  );
  expect(visible(screen.getByTestId('indexing-eta'))).toBe('About 12 s left.');
  expect(visible(screen.getByTestId('indexing-cancel'))).toBe('Stop');
});

test('a language switch after an ending reaches its sentence and the failure text with it', async () => {
  await openWindow('uk');
  await emit(ended({ reason: 'failed', message: 'the worker binary could not be started' }));

  setLocale('en');
  await tick();

  expect(visible(screen.getByTestId('indexing-ended'))).toBe(
    'The scan broke off because something went wrong.'
    + ' The program reported: the worker binary could not be started',
  );
});

// The backend's sentence is NOT translated — it is what the backend said — and
// the lead-in is. Both halves in one assertion, because a build that translated
// the sentence would be inventing an English message the backend never sent.
test('a language switch reaches the lead-in and leaves the backend sentence verbatim', async () => {
  reply({ start_scan_job: new Error('LEAK-TOKEN-VERBATIM') });
  await openWindow('uk');
  await fireEvent.click(screen.getByTestId('settings-nav-folders'));
  await screen.findByTestId('folder-row-4');
  await fireEvent.click(screen.getByTestId('folder-scan-1'));
  await waitFor(() => expect(screen.getByTestId('indexing-note')).toBeTruthy());

  setLocale('en');
  await tick();

  expect(visible(screen.getByTestId('indexing-note'))).toBe('The request was refused.');
  expect(visible(screen.getByTestId('indexing-rejection'))).toBe('LEAK-TOKEN-VERBATIM');
});

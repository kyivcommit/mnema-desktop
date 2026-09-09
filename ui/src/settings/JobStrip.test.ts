import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import { tick } from 'svelte';
import Settings from './Settings.svelte';
import { setLocale, type Loc } from '../i18n';
import { END_REASONS } from '../lib/ipc';
import type {
  Counts, EndReason, Entry, ModelSettings, ReadingOutcome, RootOutcome, ScanReport, ScanState,
} from '../lib/ipc';

// Task 7 — the reading outcome from `scan.lastReading` (the per-root rows,
// the frozen prefixes, the folders-read count, the partly-read sentence that
// outlives the report beside it), the embedding block from `report.embedding`,
// the removal sentence, and the continue button from `continueAction`. Task 6
// left this file holding only the smallest thing drawable from the new
// `ScanState` snapshot; this is the rest of it.
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
// the §9.3 section as `undefined` and every test that stands on the Scanning
// nav item dies somewhere unrelated. Task 7: `Settings.svelte` now reads
// `model_settings` on ITS OWN mount too (for the strip's `read` prop), so
// every test in this file crosses this fixture whether or not it ever visits
// the Scanning section.
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

type IndexReadT = Extract<ModelSettings['index'], { kind: 'read' }>;
const readSettings = (over: Partial<IndexReadT> = {}): ModelSettings => ({
  ...READY_SETTINGS,
  index: { ...(READY_SETTINGS.index as IndexReadT), ...over },
});

const EMPTY_CATALOGUE = { entries: [], unreadable: 0, unreadableRecords: [] };

// ---------------------------------------------------------------------------
// The states. Every shape is the one `scan_state.rs` pins as JSON
// (`every_snapshot_has_its_wire_shape_pinned`), and every one is newer than the
// last, because the revision is the only thing the controller compares.
// ---------------------------------------------------------------------------
let revision = 0;
// Bumped by `ended` below and by nothing else, the way the core bumps
// `ScanState::jobs_done`: `JobSlot::finish` and its `Drop` move it and a
// progress tick does not, so a running fixture leaves it where it was. A
// module-level counter for the same reason `revision` is one — every state
// this file emits must carry a number no earlier one carried.
let jobsDone = 0;
const IDLE_SCAN: ScanState = {
  revision: 0, files: 0, readSeq: 0, jobsDone: 0, lastReading: null, snapshot: { kind: 'idle' },
};

const COUNTS: Counts = { done: 3, total: 8, skipped: 1, refused: 0, contended: 0, secondsLeft: null };

// `rootIndex` is ONE-BASED on the wire (`scan_job.rs`: "3 of 7" is what a
// person reads), so the default here is 1, never 0 — a fixture that used 0
// would be asserting against a folder position the backend never sends.
const reading = (
  counts: Partial<Counts> = {},
  cancellable = true,
  root: Partial<{ rootIndex: number; rootCount: number; rootPath: string }> = {},
): ScanState => ({
  ...IDLE_SCAN,
  revision: (revision += 1),
  snapshot: {
    kind: 'running',
    cancellable,
    phase: {
      kind: 'reading',
      rootIndex: root.rootIndex ?? 1,
      rootCount: root.rootCount ?? 2,
      rootPath: root.rootPath ?? '/home/a/notes',
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

const removing = (rootPath = '/home/a/papers'): ScanState => ({
  ...IDLE_SCAN,
  revision: (revision += 1),
  snapshot: {
    kind: 'running',
    cancellable: false,
    phase: { kind: 'removing', rootPath },
  },
});

const rootOutcome = (over: Partial<RootOutcome> = {}): RootOutcome => ({
  rootPath: '/home/a/notes', reason: 'completed', complete: true, message: null,
  done: 5, total: 5, indexed: 5, unchanged: 0, skipped: 0, removed: 0, contended: 0, frozen: [],
  ...over,
});

const readingOutcome = (over: Partial<ReadingOutcome> = {}): ReadingOutcome => ({
  reason: 'completed', complete: true, rootsRead: 1, rootCount: 1,
  done: 5, total: 5, indexed: 5, unchanged: 0, skipped: 0, removed: 0, contended: 0,
  roots: [rootOutcome()],
  ...over,
});

// A finished job, optionally carrying the reading it left behind. `reading`
// is a SEPARATE argument rather than folded into `over` — `ScanReport` and
// `ReadingOutcome` are different fields of `ScanState` with different
// lifetimes (`ipc.ts`), and a fixture that could only set one through the
// other would misstate that.
const ended = (over: Partial<ScanReport> = {}, reading: ReadingOutcome | null = null): ScanState => ({
  ...IDLE_SCAN,
  revision: (revision += 1),
  jobsDone: (jobsDone += 1),
  lastReading: reading,
  snapshot: {
    kind: 'ended',
    report: {
      embedding: { kind: 'notReached' }, endedIn: 'reading', reason: 'completed', message: null, resume: null,
      ...over,
    },
  },
});

// A job whose ENDING IS a reading's own ending: `report.reason` and
// `scan.lastReading.reason` are the SAME event, kept in step here the way the
// real core keeps them in step (`ipc.ts`: `lastReading` is written whenever a
// reading phase ends) — a fixture that set one without the other would be
// asserting against a shape the backend cannot produce.
const endedReading = (
  reason: EndReason,
  opts: { message?: string | null; resume?: Entry | null; complete?: boolean; roots?: RootOutcome[] } = {},
): ScanState => {
  const complete = opts.complete ?? reason === 'completed';
  return ended(
    { reason, endedIn: 'reading', message: opts.message ?? null, resume: opts.resume ?? null },
    readingOutcome({ reason, complete, roots: opts.roots ?? [rootOutcome({ reason, complete })] }),
  );
};

type Replies = Record<string, unknown>;
let replies: Replies = {};

function reply(extra: Replies = {}) {
  replies = {
    list_tree: ROOTS,
    // F10 (Task 10e, fix round 1) mounts the folders panel — the list and the
    // mask editor beside it — for the window's life, so `list_masks` is now
    // asked in every test in this file. Answered honestly for the reason the
    // rest of this map is: an unanswered command resolves `undefined` here,
    // and the editor would draw "no mask has been added yet" from a fixture
    // that never said so.
    list_masks: [],
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
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Індексація теки 1 з 2: /home/a/notes');
});

// Task 5: `other` — a probe or a model adoption nobody asked to start — used
// to have no words at all, and this pair is what that used to separate: the
// phase this build has no sentence for against one that does. A disclosure
// needs something to say in its summary while either holds the slot, so both
// now get their own name (`jobs.ts`'s own `phaseLabel`); `other` still offers
// Stop when `cancellable` says so regardless — the button never depended on
// having a sentence.
test('a probe or model-adoption phase shows its own name and offers only Stop, if any', async () => {
  await openWindow();

  await emit({
    ...IDLE_SCAN,
    revision: (revision += 1),
    snapshot: { kind: 'running', cancellable: true, phase: { kind: 'other', job: 'probe' } },
  });
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває перевірка з’єднання…');
  expect(screen.getByTestId('indexing-cancel')).toBeTruthy();

  await emit({
    ...IDLE_SCAN,
    revision: (revision += 1),
    snapshot: { kind: 'running', cancellable: false, phase: { kind: 'other', job: 'modelAdoption' } },
  });
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває заміна моделі вбудовування…');
  expect(screen.queryByTestId('indexing-cancel')).toBeNull();

  await emit(reading());
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Індексація теки 1 з 2: /home/a/notes');
});

// ---------------------------------------------------------------------------
// A running reading phase, and a running removal.
// ---------------------------------------------------------------------------

// Both directions on Stop: a reading phase names the folder it is on and, when
// cancellable, offers Stop; a removal names the folder it is emptying and, not
// being cancellable (`bridge.rs` fixes it at `false`), offers nothing at all.
// Neither offers the continue button — that only ever comes from an ENDED
// report (`continueAction`), never from a running snapshot.
test('a running reading phase names the folder and its position, a running removal names the folder alone', async () => {
  await openWindow();

  await emit(reading({}, true, { rootIndex: 2, rootCount: 5, rootPath: '/x' }));
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Індексація теки 2 з 5: /x');
  expect(screen.getByTestId('indexing-cancel')).toBeTruthy();
  expect(screen.queryByTestId('indexing-continue')).toBeNull();

  await emit(removing('/x'));
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Видаляємо теку /x…');
  expect(screen.queryByTestId('indexing-cancel')).toBeNull();
});

test('the running line reads as words, with the counts in them', async () => {
  await openWindow();

  await emit(reading({ done: 3, total: 8, skipped: 1, refused: 0 }));

  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Індексація теки 1 з 2: /home/a/notes');
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

// Both directions, and only for EMBEDDING: a reading pass already has
// `progressShape`'s own "countingUp" sentence for a total it has not measured
// yet, so this is the one shape that is embedding's alone.
test('a fresh embedding pass says it is starting rather than showing 0 of 0, and shows counts once it has some', async () => {
  await openWindow();

  await emit(embedding({ done: 0, total: 0 }));
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває вбудовування всього індексу.');
  expect(visible(screen.getByTestId('indexing-counts'))).toBe('Вбудовування починається…');

  await emit(embedding({ done: 3, total: 10 }));
  expect(visible(screen.getByTestId('indexing-counts'))).toBe('Опрацьовано 3 з 10. Пропущено: 1. Відхилено: 0.');
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
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Індексація теки 1 з 2: /home/a/notes');

  await emit(endedReading('cancelled'));

  expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe('Сканування зупинено на ваше прохання.');
  expect(screen.queryByTestId('indexing-cancel')).toBeNull();
});

// ---------------------------------------------------------------------------
// The reading block: `scan.lastReading`, in `idle` and `ended` alike (D-e).
// ---------------------------------------------------------------------------

// The four rows after `failed`, plus `partlyRead`, are the ones a table of
// three would lose. `job.rs` says reporting them as `failed` tells a person
// something broke when instead a folder cannot be read, an exclusion rule did
// not take, or a volume may have gone missing — and `partlyRead` is D-e's own
// split of `completed`, never reachable through `reason` alone.
const WALK_SENTENCES: Record<EndReason | 'partlyRead', { uk: string; en: string }> = {
  completed: {
    uk: 'Теки проіндексовано повністю.',
    en: 'The folders were indexed in full.',
  },
  partlyRead: {
    uk: 'Теки проіндексовано лише частково: до якихось підтек не вдалося зайти. Нічого в цих теках не звіряли з індексом, тож і видалені файли, і файли під вашими правилами виключення досі знаходяться пошуком — не лише всередині тих підтек.',
    en: 'The folders were only partly indexed: some subfolders could not be entered. Nothing in these folders was checked against the index, so both deleted files and files your exclusion rules now cover are still found by search — not only inside those subfolders.',
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
    uk: 'Сканування спинилося: правила виключення не вдалося застосувати, тож теку не індексували зовсім.',
    en: 'The scan stopped: the exclusion rules could not be applied, so the folder was not indexed at all.',
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

// Both directions on the table itself: a wire reason (plus `partlyRead`) this
// file says nothing about, and a row here for a kind `readingKind` cannot
// produce, are both defects.
test('the table names one sentence for every reading-outcome kind, and no others', () => {
  expect(Object.keys(WALK_SENTENCES).sort()).toEqual([...END_REASONS, 'partlyRead'].sort());
});

test.each(['uk', 'en'] as const)('every reason a reading can end for shows its own sentence, and no two share one (%s)', async (loc) => {
  await openWindow(loc);
  const seen: string[] = [];

  for (const reason of END_REASONS) {
    await emit(endedReading(reason));
    const shown = visible(screen.getByTestId('indexing-walk-outcome'));
    expect(shown, reason).toBe(WALK_SENTENCES[reason][loc]);
    seen.push(shown);
  }

  // The cardinality claim, which the per-row assertions above cannot make: a
  // component that drew one sentence for the four after `failed` would satisfy
  // every one of them if the table repeated itself.
  expect(new Set(seen).size).toBe(END_REASONS.length);
});

// D-e's own split: the SAME `reason: 'completed'` reads as two different
// sentences depending on a fact `reason` alone cannot carry.
test('a completed reading that did not see the whole tree reads as partly read, not as completed', async () => {
  await openWindow();

  await emit(endedReading('completed', { complete: true }));
  expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe(WALK_SENTENCES.completed.uk);

  await emit(endedReading('completed', { complete: false }));
  expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe(WALK_SENTENCES.partlyRead.uk);
});

// Both directions. `message` is `Option<String>` on the wire, so its absence is
// a shape the screen has to survive rather than a case that cannot happen — and
// when it is there it is the ONLY thing telling a panic, a broken pool and a
// missing worker binary apart.
test('a failure carries its own text, and survives its absence', async () => {
  await openWindow();

  await emit(endedReading('failed', { message: 'the worker binary could not be started' }));
  expect(visible(screen.getByTestId('indexing-ended-failure')))
    .toBe('Програма повідомила: the worker binary could not be started');

  await emit(endedReading('failed', { message: null }));
  expect(screen.queryByTestId('indexing-ended-failure')).toBeNull();
  expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe(WALK_SENTENCES.failed.uk);
});

// Task 9c (debt sweep). `scan_job.rs`'s own comment on the preflight
// `rulesNotApplied` refusal says the person's next step is «Теки», not a
// retry — but `read_roots` fails before a folder is ever read, so this
// ending never calls `mark_reading_done` and `scan.lastReading` is never
// touched. `readingBlock` (built from exactly that field) therefore draws
// nothing here — `ended(over, null)` is the fixture for precisely that,
// `reading: null` — and this failure message is the ONLY sentence a person
// sees. It used to be the Rust refusal's own text alone, naming the rule but
// never saying where to go fix it.
test('a preflight rules refusal shows the refusal and points at Теки', async () => {
  await openWindow();

  await emit(ended({
    reason: 'rulesNotApplied', endedIn: 'reading',
    message: 'the stored exclusion mask no longer validates', resume: null,
  }, null));

  expect(screen.queryByTestId('indexing-walk-outcome')).toBeNull();
  expect(visible(screen.getByTestId('indexing-ended-failure'))).toBe(
    'Програма повідомила: the stored exclusion mask no longer validates Виправте правило в розділі «Теки».',
  );
});

// The other direction, on the `reason` half of the guard: an ordinary
// failure carries a message too, and must not grow a pointer that has
// nothing to do with it.
test('an ordinary failure does not grow the rules pointer', async () => {
  await openWindow();

  await emit(endedReading('failed', { message: 'the worker binary could not be started' }));

  expect(visible(screen.getByTestId('indexing-ended-failure')))
    .toBe('Програма повідомила: the worker binary could not be started');
});

// 🔴 An ending is a STATE: a scan that finished stays finished until the next
// job claims the slot, and so does the reading it left behind. The pair this
// separates is «the window heard the ending» from «the window was opened after
// it» — and the second is the one a channel could never answer.
test('a window opened after the scan ended still reads how it went', async () => {
  reply({ job_status: endedReading('volumeMissing') });

  await openWindow();

  await waitFor(() => expect(screen.getByTestId('indexing-walk-outcome')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe(WALK_SENTENCES.volumeMissing.uk);
});

// D-e, read literally: `lastReading` renders in an IDLE snapshot too, not only
// an ended one — nothing about the sentence depends on `snapshot.kind` beyond
// "not running".
test('a lastReading renders in an idle snapshot too, not only an ended one', async () => {
  await openWindow();

  await emit({
    ...IDLE_SCAN,
    revision: (revision += 1),
    lastReading: readingOutcome({ reason: 'completed', complete: false }),
  });

  expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe(WALK_SENTENCES.partlyRead.uk);
});

// Important 2 (review): the doc comment above `readingBlock` claims it is
// `null` "exactly when `snapshot.kind === 'running'`", and nothing built that
// state before now — every running fixture in this file spreads `IDLE_SCAN`,
// whose `lastReading` is `null`, so "running WITH a `lastReading`" was never
// exercised. It is not a rare shape: `scan_job.rs` leaves `last_reading`
// untouched across an `embedOnly` run, so every resumed embedding (the R2-1
// sequence below included) passes through exactly this pair of states.
test('a running phase hides the reading block, its root row and its frozen row too, even with a lastReading on hand — and a following ended snapshot brings them all back', async () => {
  await openWindow();
  const partlyRead = readingOutcome({
    reason: 'completed', complete: false,
    roots: [rootOutcome({
      rootPath: '/a', complete: false,
      frozen: [{ prefix: 'sub', reason: 'emptyDirectory' }],
    })],
  });

  await emit({ ...embedding({ done: 1, total: 4 }), lastReading: partlyRead });

  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває вбудовування всього індексу.');
  expect(screen.queryByTestId('indexing-walk-outcome')).toBeNull();
  expect(screen.queryByTestId('indexing-root-row')).toBeNull();
  expect(screen.queryByTestId('indexing-frozen')).toBeNull();

  await emit(ended({}, partlyRead));

  expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe(WALK_SENTENCES.partlyRead.uk);
  expect(screen.getAllByTestId('indexing-root-row').map(visible)).toEqual(['/a: проіндексовано частково']);
  expect(visible(screen.getByTestId('indexing-frozen'))).toContain('/a/sub');
});

// The reading block's own copy of the busy sentence — sourced from
// `lastReading.contended`, not from a running phase's live counts — and it is
// what lets the fact survive past the ending. Both directions.
test('the reading block\'s own busy sentence survives past the ending, and is silent when nothing was contended', async () => {
  await openWindow();

  await emit(ended({}, readingOutcome({ contended: 3 })));
  expect(visible(screen.getByTestId('indexing-contended'))).toBe(
    'Індекс саме зайнятий іншим записом, тож частину файлів цей скан не записав.'
    + ' Наступне сканування спробує їх знову.',
  );

  await emit(ended({}, readingOutcome({ contended: 0 })));
  expect(screen.queryByTestId('indexing-contended')).toBeNull();
});

test('the folders-read count shows only when the reading actually names a root count', async () => {
  await openWindow();

  await emit(ended({}, readingOutcome({ rootsRead: 2, rootCount: 3 })));
  expect(visible(screen.getByTestId('indexing-roots-read'))).toBe('Проіндексовано тек: 2 з 3');

  await emit(ended({}, readingOutcome({ rootsRead: 0, rootCount: 0 })));
  expect(screen.queryByTestId('indexing-roots-read')).toBeNull();
});

// ---------------------------------------------------------------------------
// Per-root rows and frozen prefixes.
// ---------------------------------------------------------------------------

// An unavailable root and a volume-missing root are each their own row, named
// by path; a root that simply completed gets no row at all. Three rows in
// total once the frozen prefix under the third root is counted too.
test('an unavailable root and a volume-missing root are each their own row, and a completed root gets none', async () => {
  await openWindow();

  await emit(ended({}, readingOutcome({
    reason: 'completed', complete: false, rootCount: 3, rootsRead: 3,
    roots: [
      rootOutcome({ rootPath: '/gone', reason: 'rootUnavailable', complete: false }),
      rootOutcome({ rootPath: '/vol', reason: 'volumeMissing', complete: false }),
      rootOutcome({
        rootPath: '/a', reason: 'completed', complete: true,
        frozen: [{ prefix: 'sub', reason: 'emptyDirectory' }],
      }),
    ],
  })));

  const rows = screen.getAllByTestId('indexing-root-row').map((el) => visible(el));
  expect(rows).toEqual(['/gone: тека недоступна', '/vol: том відсутній']);
  expect(visible(screen.getByTestId('indexing-frozen'))).toContain('/a/sub — прочиталася порожньою');
});

// The one `walk_job.rs`-only kind a root row still has no sentence of its
// own for: `rootRowText` falls back to the reading-outcome table's own
// wording rather than leaving the row blank or throwing on a wire value the
// type permits but no fixture above ever names.
// Important 1 (Task 10a review, round 1): the fallback text this row draws IS
// `indexing_walk_ended_rules_not_applied`, the same key the whole-reading
// outcome line draws — so a locale that renders the outcome line correctly
// but breaks this fallback (a literal, a stale copy) would still pass every
// test that only ever checks the outcome line. Both directions, both locales.
test.each(['uk', 'en'] as const)('a root whose rules were not applied still gets a row rather than a blank one, and both name the outcome the same way (%s)', async (loc) => {
  await openWindow(loc);

  await emit(ended({}, readingOutcome({
    reason: 'rulesNotApplied', complete: false,
    roots: [
      rootOutcome({ rootPath: '/r', reason: 'rulesNotApplied', complete: false, message: null }),
    ],
  })));

  expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe(WALK_SENTENCES.rulesNotApplied[loc]);
  expect(screen.getAllByTestId('indexing-root-row').map(visible)).toEqual([
    `/r: ${WALK_SENTENCES.rulesNotApplied[loc]}`,
  ]);
});

// F6 (Task 10 live run): a cancelled root used to fall to the SAME fallback
// as `rulesNotApplied` above, whose own wording is `WALK_SENTENCES.cancelled`
// — the exact sentence `readingBlock.sentence` already draws once for the
// whole reading, so a cancelled root's own row silently repeated the top
// sentence rather than saying anything about that root. `indexing_root_cancelled`
// is its own key now; both directions: the row shows it, and does NOT equal
// the top sentence a second time.
// Minor 5 (Task 10a review, round 1): `indexing_root_cancelled` pinned in
// English too, not only Ukrainian — a literal left in the English arm alone
// would still pass a Ukrainian-only assertion.
const ROOT_CANCELLED: Record<Loc, string> = {
  uk: 'індексацію перервано',
  en: 'indexing was interrupted',
};

test.each(['uk', 'en'] as const)('a cancelled root gets its own row, not a second copy of the top sentence (%s)', async (loc) => {
  await openWindow(loc);

  await emit(ended({}, readingOutcome({
    reason: 'cancelled', complete: false,
    roots: [rootOutcome({ rootPath: '/c', reason: 'cancelled', complete: false, message: null })],
  })));

  expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe(WALK_SENTENCES.cancelled[loc]);
  const rows = screen.getAllByTestId('indexing-root-row').map(visible);
  expect(rows).toEqual([`/c: ${ROOT_CANCELLED[loc]}`]);
  expect(rows[0]).not.toBe(`/c: ${WALK_SENTENCES.cancelled[loc]}`);
});

// Both directions, and the crash this guards against: two prefixes under the
// SAME root can be equal (`walk.rs`'s own ancestor-climb argument, restated in
// the component's doc comment), and a list keyed by prefix alone would throw.
test('two frozen entries sharing a prefix under one root are both shown, not a crash — and a walk that froze nothing shows no list', async () => {
  await openWindow();

  await emit(ended({}, readingOutcome({
    roots: [rootOutcome({
      rootPath: '/a',
      frozen: [
        { prefix: 'x', reason: 'unreadableDirectory' },
        { prefix: 'x', reason: 'emptyDirectory' },
      ],
    })],
  })));
  expect(screen.getByTestId('indexing-frozen').querySelectorAll('li')).toHaveLength(2);
  expect(visible(screen.getByTestId('indexing-frozen'))).toContain('/a/x — не вдалося прочитати');
  expect(visible(screen.getByTestId('indexing-frozen'))).toContain('/a/x — прочиталася порожньою');

  await emit(ended({}, readingOutcome({ roots: [rootOutcome({ frozen: [] })] })));
  expect(screen.queryByTestId('indexing-frozen')).toBeNull();
});

// ---------------------------------------------------------------------------
// The embedding block, from `report.embedding`.
// ---------------------------------------------------------------------------

// The partly-read sentence is the READING's own fact and does not go away
// just because the embedding half that followed it succeeded — the two blocks
// are drawn from two different fields with two different lifetimes.
test('the partly-read sentence survives a successful embedding', async () => {
  await openWindow();

  await emit(ended(
    { embedding: { kind: 'ran', done: 4, total: 4, refused: 0 }, endedIn: 'embedding', reason: 'completed', resume: null },
    readingOutcome({
      reason: 'completed', complete: false,
      roots: [rootOutcome({ rootPath: '/a', complete: false })],
    }),
  ));

  expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe(WALK_SENTENCES.partlyRead.uk);
  expect(screen.getAllByTestId('indexing-root-row').map(visible)).toEqual(['/a: проіндексовано частково']);
  expect(visible(screen.getByTestId('indexing-embed-outcome'))).toBe('Вбудовування всього індексу завершено.');
  expect(visible(screen.getByTestId('indexing-embed-result'))).toBe('Вбудовано фрагментів: 4 з 4. Відхилено: 0.');
  expect(screen.queryByTestId('indexing-continue')).toBeNull();
});

test('an embedding skipped for no key, no model, or a store that did not answer gets its own sentence', async () => {
  await openWindow();

  await emit(ended({ embedding: { kind: 'skipped', why: { kind: 'noKey' } }, endedIn: 'embedding' }));
  expect(visible(screen.getByTestId('indexing-embed-outcome'))).toBe(
    'Пошук за змістом не вмикали: ключ провайдера не збережено. Пошук по словах уже працює.',
  );

  await emit(ended({ embedding: { kind: 'skipped', why: { kind: 'noModel' } }, endedIn: 'embedding' }));
  expect(visible(screen.getByTestId('indexing-embed-outcome'))).toBe(
    'Пошук за змістом не вмикали: модель вбудовування не обрана. Пошук по словах уже працює.',
  );

  await emit(ended(
    { embedding: { kind: 'skipped', why: { kind: 'storeUnavailable', message: 'locked' } }, endedIn: 'embedding' },
  ));
  expect(visible(screen.getByTestId('indexing-embed-outcome')))
    .toBe('Вбудовування не запущено: сховище ключів не відповіло: locked');

  // Minor 5 (Task 10a review, round 1): F11's English wording pinned too —
  // dropping «у цій теці»/"over this folder" was a change to BOTH arms, and
  // a Ukrainian-only assertion above would not catch a stale English one.
  setLocale('en');
  await tick();

  await emit(ended({ embedding: { kind: 'skipped', why: { kind: 'noKey' } }, endedIn: 'embedding' }));
  expect(visible(screen.getByTestId('indexing-embed-outcome'))).toBe(
    'Search by meaning was not started: no provider key is stored. Word search already works.',
  );

  await emit(ended({ embedding: { kind: 'skipped', why: { kind: 'noModel' } }, endedIn: 'embedding' }));
  expect(visible(screen.getByTestId('indexing-embed-outcome'))).toBe(
    'Search by meaning was not started: no embedding model has been chosen. Word search already works.',
  );
});

// The pair Important 1 separates: `notReached` beside `endedIn: 'reading'`
// (here) says nothing, because that ending is the reading block's own
// (`lastReading.reason` names the very same event); `notReached` beside
// `endedIn: 'embedding'` (the Stop-during-key-read fixture in "the label
// follows the reason…" above) is the one shape that DOES earn a sentence,
// because it is the only statement that ending has.
test('an embedding that never reached the phase, and never left the reading phase either, says nothing at all', async () => {
  await openWindow();

  await emit(endedReading('completed'));

  expect(screen.queryByTestId('indexing-embed-outcome')).toBeNull();
});

// ---------------------------------------------------------------------------
// The continue button — D-m's table, rendered only when it names the strip.
// ---------------------------------------------------------------------------

test('the label follows the reason, and the entry follows what the report named to resume from', async () => {
  await openWindow();

  await emit(endedReading('cancelled', { resume: 'full' }));
  expect(visible(screen.getByTestId('indexing-continue'))).toBe('Продовжити');
  await fireEvent.click(screen.getByTestId('indexing-continue'));
  expect(calls('start_scan_job').at(-1)?.[1]).toEqual({ entry: 'full' });

  await emit(ended(
    { reason: 'cancelled', endedIn: 'embedding', embedding: { kind: 'notReached' }, resume: 'embedOnly' },
    readingOutcome(),
  ));
  // Stop pressed while the credential store was still being read: the
  // embedding phase was claimed and never offered a chunk to a provider, but
  // it DID end, and the reading block above (a completed reading from an
  // earlier pass) says nothing about it — this sentence is the only place
  // that stop is stated at all (review, Important 1).
  expect(visible(screen.getByTestId('indexing-embed-outcome'))).toBe('Вбудовування зупинено на ваше прохання.');
  expect(visible(screen.getByTestId('indexing-continue'))).toBe('Продовжити');
  await fireEvent.click(screen.getByTestId('indexing-continue'));
  expect(calls('start_scan_job').at(-1)?.[1]).toEqual({ entry: 'embedOnly' });

  await emit(ended({
    reason: 'failed', endedIn: 'embedding', message: 'boom', resume: 'embedOnly',
    embedding: { kind: 'ran', done: 1, total: 3, refused: 0 },
  }));
  expect(visible(screen.getByTestId('indexing-continue'))).toBe('Повторити');
  expect(visible(screen.getByTestId('indexing-ended-failure'))).toBe('Програма повідомила: boom');
  await fireEvent.click(screen.getByTestId('indexing-continue'));
  expect(calls('start_scan_job').at(-1)?.[1]).toEqual({ entry: 'embedOnly' });

  await emit(endedReading('brokenWorker', { resume: 'full' }));
  expect(visible(screen.getByTestId('indexing-continue'))).toBe('Повторити');

  // A report naming no resumption offers nothing here, whatever it names —
  // `rulesNotApplied`'s own reading never has one (`scan_job::resume_for`).
  await emit(endedReading('rulesNotApplied', { resume: null }));
  expect(screen.queryByTestId('indexing-continue')).toBeNull();
});

// Both directions on the one thing the strip must NOT decide: whether the
// index's own markers owe a button. `continueAction`'s `where: 'section'` arm
// is the Scanning section's offer (Task 8), and the strip must stay silent for
// it whether or not the index was ever read at all. Task 8 widens this to the
// WHOLE window: the offer this pair used to leave unclaimed is now the
// Scanning section's own, read through `Settings.svelte`'s single
// `model_settings` — not a second one this section polls for itself.
test('a report naming no resumption defers to the section, with the index read and with no index read at all', async () => {
  reply({ model_settings: readSettings({ scanIncomplete: true }) });
  await openWindow();
  await emit(ended({ resume: null }));
  expect(screen.queryByTestId('indexing-continue')).toBeNull();

  await fireEvent.click(screen.getByTestId('settings-nav-indexing'));
  await waitFor(() => expect(screen.getByTestId('scanning-continue')).toBeTruthy());
  expect(visible(screen.getByTestId('scanning-continue'))).toBe('Продовжити');
  await fireEvent.click(screen.getByTestId('scanning-continue'));
  expect(calls('start_scan_job').at(-1)?.[1]).toEqual({ entry: 'full' });
});

test('a report naming no resumption defers to the section even when the index could not be read at all', async () => {
  reply({ model_settings: { key: READY_SETTINGS.key, index: { kind: 'unreadable', cause: 'notOpen', reason: 'x' }, platform: 'linux' } });
  await openWindow();
  await emit(ended({ resume: null }));
  expect(screen.queryByTestId('indexing-continue')).toBeNull();

  // Neither surface may guess: `read === null` (an `Unreadable` index) makes
  // `continueAction` answer `null` outright, so the section offers nothing
  // either — the correct degradation, not a second place a marker leaks
  // through.
  await fireEvent.click(screen.getByTestId('settings-nav-indexing'));
  await waitFor(() => expect(screen.getByTestId('indexing-index-unreadable')).toBeTruthy());
  expect(screen.queryByTestId('scanning-continue')).toBeNull();
});

// Fixture pair, both directions of D-m's ordering (`jobs.ts`): a report that
// NAMES its own resumption wins over the index's markers, wherever they point
// — the strip shows the one button and the section shows none — and a report
// that ends a cycle with nothing left offers nothing on either surface, not
// merely nothing on the one that happened to be visible.
test('a report naming its own resumption wins the strip over the section, whatever the markers say; and a completed cycle with nothing left offers neither', async () => {
  reply({ model_settings: readSettings({ scanIncomplete: true }) });
  await openWindow();

  await emit(endedReading('cancelled', { resume: 'full' }));
  await waitFor(() => expect(screen.getByTestId('indexing-continue')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-continue'))).toBe('Продовжити');
  await fireEvent.click(screen.getByTestId('settings-nav-indexing'));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(screen.queryByTestId('scanning-continue')).toBeNull();

  await fireEvent.click(screen.getByTestId('settings-nav-models'));
  reply({ model_settings: readSettings({ scanIncomplete: false }) });
  await emit(endedReading('completed', { resume: null }));
  expect(screen.queryByTestId('indexing-continue')).toBeNull();
  await fireEvent.click(screen.getByTestId('settings-nav-indexing'));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(screen.queryByTestId('scanning-continue')).toBeNull();
});

// 🔴 The R2-1 sequence, through the REAL controller: `Full(partial)` stops in
// embedding, a person presses Продовжити, the controller resumes with
// `embedOnly`, and the reading's own warning — the row and the frozen prefix
// under it — is still on screen once that resumed embedding finishes. Repeated
// across an unmount and a remount of the whole window between the stop and the
// resume, because nothing here may live in the COMPONENT: only the backend
// this new window reads is allowed to remember it.
test('the last reading\'s warning outlives a continued embedding, across an unmount and a remount', async () => {
  const partlyRead = readingOutcome({
    reason: 'completed', complete: false,
    roots: [rootOutcome({
      rootPath: '/a', complete: false,
      frozen: [{ prefix: 'sub', reason: 'emptyDirectory' }],
    })],
  });
  const cancelledInEmbedding = ended(
    { reason: 'cancelled', endedIn: 'embedding', embedding: { kind: 'notReached' }, resume: 'embedOnly' },
    partlyRead,
  );

  let rendered = await openWindow();
  await emit(cancelledInEmbedding);

  expect(visible(screen.getByTestId('indexing-continue'))).toBe('Продовжити');
  expect(screen.getAllByTestId('indexing-root-row').map(visible)).toEqual(['/a: проіндексовано частково']);
  expect(visible(screen.getByTestId('indexing-frozen'))).toContain('/a/sub');

  // Unmount and remount between the stop and the resume.
  rendered.unmount();
  deliver = null;
  reply({ job_status: cancelledInEmbedding });
  rendered = await openWindow();
  await waitFor(() => expect(screen.getByTestId('indexing-continue')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('indexing-continue'));
  expect(calls('start_scan_job')).toHaveLength(1);
  expect(calls('start_scan_job')[0][1]).toEqual({ entry: 'embedOnly' });

  await emit(ended(
    { reason: 'completed', endedIn: 'embedding', embedding: { kind: 'ran', done: 4, total: 4, refused: 0 }, resume: null },
    partlyRead, // UNCHANGED — the reading this embedding resumed from is untouched by it.
  ));

  expect(screen.getAllByTestId('indexing-root-row').map(visible)).toEqual(['/a: проіндексовано частково']);
  expect(visible(screen.getByTestId('indexing-frozen'))).toContain('/a/sub');
  expect(visible(screen.getByTestId('indexing-embed-result'))).toBe('Вбудовано фрагментів: 4 з 4. Відхилено: 0.');
  expect(screen.queryByTestId('indexing-continue')).toBeNull();

  // A LATER reading SUPERSEDES the earlier one — the warning is sticky, not
  // permanent. The top-line sentence goes back to plain `completed` (a new
  // reading, read in full); the row goes because this reading's own root is
  // `completed`; the frozen list goes because that root's `frozen` is empty.
  await emit(ended(
    { reason: 'completed', endedIn: 'embedding', embedding: { kind: 'ran', done: 4, total: 4, refused: 0 }, resume: null },
    readingOutcome({ reason: 'completed', complete: true }),
  ));
  expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe(WALK_SENTENCES.completed.uk);
  expect(screen.queryByTestId('indexing-root-row')).toBeNull();
  expect(screen.queryByTestId('indexing-frozen')).toBeNull();
});

// ---------------------------------------------------------------------------
// A refused command.
// ---------------------------------------------------------------------------

// A rejection crosses the IPC as text (`error.rs`) and nothing branches on it.
// Task 7 drops the window's own lead-in (`indexing_note_rejected`): the
// backend's sentence stands alone.
test('a refused scan shows the backend sentence verbatim, with no lead-in of this window\'s own', async () => {
  reply({ start_scan_job: new Error('LEAK-TOKEN-ANOTHER-JOB') });
  await openWindow();
  // Task 8: the one «Сканувати» control lives in the Scanning section now —
  // Folders no longer starts a scan of its own.
  await fireEvent.click(screen.getByTestId('settings-nav-indexing'));
  await waitFor(() => expect(screen.getByTestId('scanning-scan')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('scanning-scan'));

  await waitFor(() => expect(screen.getByTestId('indexing-rejection')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-rejection'))).toBe('LEAK-TOKEN-ANOTHER-JOB');
  expect(screen.queryByTestId('indexing-note')).toBeNull();
});

// 🔴 The sentence alone cannot say what the slot now holds, so the controller
// asks again — and the commonest reason this command is refused is that another
// job is running, whose Stop a person must keep. A build that only reported the
// sentence would leave the window with no way to stop the job it just collided
// with.
test('a scan refused because another job holds the slot leaves that job`s Stop in place', async () => {
  reply({ start_scan_job: new Error('another job is already running') });
  // The window has to open IDLE — a running snapshot from the first
  // `job_status` would hide the Scanning section's own button entirely (Task
  // 8: it steps aside once a run already owns the slot, correctly). The race
  // this test is about is the other job claiming the slot BETWEEN this
  // window's mount and its own press — so `job_status` answers idle for BOTH
  // calls the mount itself makes (`jobs.ts`'s own fast-paint read and the one
  // behind `listenScanProgress`'s resolution) and running only on the THIRD
  // call, the one `jobs.scan`'s own catch handler makes after `start_scan_job`
  // is refused.
  let jobStatusCalls = 0;
  // `null` until the mount itself has settled — measured below, not assumed
  // here. While it is `null`, every `job_status` call is one of the mount's
  // own (the fast-paint read and the one behind `listenScanProgress`'s
  // resolution), so all of them answer idle.
  let mountCalls: number | null = null;
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'job_status') {
      jobStatusCalls += 1;
      const stillMounting = mountCalls === null || jobStatusCalls <= mountCalls;
      return Promise.resolve(stillMounting ? IDLE_SCAN : reading());
    }
    const r = replies[cmd];
    if (r instanceof Error) return Promise.reject(r);
    return Promise.resolve(r);
  });
  await openWindow();
  await fireEvent.click(screen.getByTestId('settings-nav-indexing'));
  await waitFor(() => expect(screen.getByTestId('scanning-scan')).toBeTruthy());
  // The mount's own reads, measured from the fixture rather than assumed as a
  // fixed count: whatever `jobStatusCalls` reached by the time the window has
  // settled is the mount's, and only a call after this point is the THIRD one
  // the test is actually about — `jobs.scan`'s own catch handler, after
  // `start_scan_job` is refused below.
  mountCalls = jobStatusCalls;

  await fireEvent.click(screen.getByTestId('scanning-scan'));

  await waitFor(() => expect(screen.getByTestId('indexing-rejection')).toBeTruthy());
  // Scoped to the strip: the Scanning section is on screen too (the test
  // navigated there above) and now draws the very same running phase through
  // its own `<ScanProgress>` (Task 5) — a bare `getByTestId` would match both.
  const strip1 = within(screen.getByTestId('indexing'));
  expect(visible(strip1.getByTestId('indexing-pass'))).toBe('Індексація теки 1 з 2: /home/a/notes');
  expect(strip1.getByTestId('indexing-cancel')).toBeTruthy();
});

// ---------------------------------------------------------------------------
// The strip belongs to the WINDOW, not to a section.
// ---------------------------------------------------------------------------

// 🔴 The live run's finding 3, and the reason the controller is created in
// `Settings.svelte` above every section. A controller built inside a section
// dies when that section does, taking the counters AND the Stop with it — and
// `cancel_job` needs no channel at all, so that Stop would be lost for nothing.
// Three of the four sections are destroyed by every nav click; the fourth,
// Folders, is kept mounted and hidden by F10 (Task 10e), and that changes
// nothing about who may hold the controller: what the strip has to survive is
// the WINDOW's decisions about its sections, not one section's own luck.
test('a scan survives switching sections, and Stop still stops it afterwards', async () => {
  await openWindow();
  await emit(reading());

  await fireEvent.click(screen.getByTestId('settings-nav-folders'));
  await screen.findByTestId('folder-row-4');
  await fireEvent.click(screen.getByTestId('settings-nav-models'));
  await tick();

  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Індексація теки 1 з 2: /home/a/notes');
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
  await emit(endedReading('volumeMissing'));
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

  await emit(endedReading('brokenWorker'));

  expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe(WALK_SENTENCES.brokenWorker.uk);
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

test('the scanning section re-reads what the index holds when a scan ends', async () => {
  await openWindow();
  await fireEvent.click(screen.getByTestId('settings-nav-indexing'));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  const before = calls('model_settings').length;

  await emit(ended());

  // Two calls, not one: `Settings.svelte`'s own re-read and `Models.svelte`'s
  // (Task 4, review P2-1 — mounted-hidden for the window's life now, so its
  // `jobs.state` subscription stays live on every other section too).
  await waitFor(() => expect(calls('model_settings').length).toBe(before + 2));
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

  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Indexing folder 1 of 2: /home/a/notes');
  expect(visible(screen.getByTestId('indexing-counts')))
    .toBe('Processed 3 of 8. Skipped: 1. Given up on: 0.');
  expect(visible(screen.getByTestId('indexing-contended'))).toBe(
    'The index is busy with another write, so this scan did not write some files.'
    + ' The next scan will try them again.',
  );
  expect(visible(screen.getByTestId('indexing-eta'))).toBe('About 12 s left.');
  expect(visible(screen.getByTestId('indexing-cancel'))).toBe('Stop');
});

// One test for the whole ended shape: the reading block, its per-root row, the
// embedding block, the failure text and the continue button all follow the
// switch together.
test('a language switch while ended re-renders the reading block, the embedding block, the failure text and the continue button', async () => {
  await openWindow('uk');
  await emit(ended(
    {
      reason: 'failed', endedIn: 'embedding', message: 'boom', resume: 'embedOnly',
      embedding: { kind: 'ran', done: 1, total: 2, refused: 0 },
    },
    readingOutcome({
      reason: 'completed', complete: false,
      roots: [rootOutcome({ rootPath: '/a', complete: false })],
    }),
  ));

  setLocale('en');
  await tick();

  expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe(WALK_SENTENCES.partlyRead.en);
  expect(screen.getAllByTestId('indexing-root-row').map(visible)).toEqual(['/a: indexed partly']);
  expect(visible(screen.getByTestId('indexing-embed-outcome')))
    .toBe('The embedding pass broke off because something went wrong.');
  expect(visible(screen.getByTestId('indexing-ended-failure'))).toBe('The program reported: boom');
  expect(visible(screen.getByTestId('indexing-continue'))).toBe('Retry');
});

// The backend's sentence is NOT translated — it is what the backend said — and
// the strip has no lead-in of its own to translate any more (Task 7).
test('a language switch leaves the backend`s rejection sentence verbatim', async () => {
  reply({ start_scan_job: new Error('LEAK-TOKEN-VERBATIM') });
  await openWindow('uk');
  await fireEvent.click(screen.getByTestId('settings-nav-indexing'));
  await waitFor(() => expect(screen.getByTestId('scanning-scan')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('scanning-scan'));
  await waitFor(() => expect(screen.getByTestId('indexing-rejection')).toBeTruthy());

  setLocale('en');
  await tick();

  expect(visible(screen.getByTestId('indexing-rejection'))).toBe('LEAK-TOKEN-VERBATIM');
});

// ---------------------------------------------------------------------------
// Task 5 — the strip is a non-modal disclosure: closed by default, opened by
// a click on the summary, and closed again by Escape, an outside focus move
// or press, and a section change — never by an ordinary progress tick, and
// never hiding a failure the summary line would otherwise still say.
// ---------------------------------------------------------------------------

// A disclosure toggle must never be how a failure goes unread: the summary
// line — visible whether the panel is open or closed, that is what `<summary>`
// IS — has to say so on its own. `endedSentence` (`JobStrip.svelte`) picks the
// EMBEDDING block's own sentence here because the report ended in embedding,
// not the reading block's «проіндексовано повністю», which would say nothing
// happened to worry about.
test('disclosure_does_not_hide_failure', async () => {
  await openWindow();
  await emit(ended(
    {
      reason: 'failed', endedIn: 'embedding', message: 'boom', resume: 'embedOnly',
      embedding: { kind: 'ran', done: 1, total: 3, refused: 0 },
    },
    readingOutcome({ reason: 'completed', complete: true }),
  ));

  const disclosure = screen.getByTestId('indexing') as HTMLDetailsElement;
  expect(disclosure.open).toBe(false);
  expect(visible(screen.getByTestId('job-summary'))).toBe('Вбудовування обірвалося через збій.');
  // The diagnostic itself is still in the (closed) detail — collapsing the
  // panel does not delete it, only stops SHOWING it until reopened.
  expect(visible(screen.getByTestId('indexing-ended-failure'))).toBe('Програма повідомила: boom');
});

test('escape_restores_summary_focus', async () => {
  await openWindow();
  await emit(reading());

  const disclosure = screen.getByTestId('indexing') as HTMLDetailsElement;
  await fireEvent.click(screen.getByTestId('job-summary'));
  expect(disclosure.open).toBe(true);

  screen.getByTestId('indexing-cancel').focus();
  await fireEvent.keyDown(screen.getByTestId('indexing-cancel'), { key: 'Escape' });

  expect(disclosure.open).toBe(false);
  expect(document.activeElement).toBe(screen.getByTestId('job-summary'));
});

// Focus landing outside the panel — never a press or a focus move this
// component may cancel or redirect, only one it reacts to by closing.
test('outside_focus_closes_the_panel', async () => {
  await openWindow();
  await emit(reading());

  const disclosure = screen.getByTestId('indexing') as HTMLDetailsElement;
  await fireEvent.click(screen.getByTestId('job-summary'));
  expect(disclosure.open).toBe(true);

  const outside = screen.getByTestId('settings-nav-models');
  outside.focus();
  await tick();

  expect(disclosure.open).toBe(false);
  // The outside action itself is untouched: focus really did land there, not
  // bounced back or intercepted.
  expect(document.activeElement).toBe(outside);
});

// The last case in the doc comment above `JobStrip.svelte`'s focus-restore
// effect: the element focus was on is not merely reshaped, the WHOLE panel
// leaves the DOM (the run ends with nothing left to report at all), and
// nothing here may fall through to `<body>` — `focusFallback`, which
// `Settings.svelte` wires to the pressed nav button, is what catches it.
test('removed_panel_focus_returns_to_active_navigation', async () => {
  await openWindow();
  await emit(reading({}, true));

  await fireEvent.click(screen.getByTestId('job-summary'));
  screen.getByTestId('indexing-cancel').focus();
  expect(document.activeElement).toBe(screen.getByTestId('indexing-cancel'));

  // Idle, no note, no `lastReading` at all — `anything` goes false and the
  // disclosure itself leaves the document.
  await emit({ ...IDLE_SCAN, revision: (revision += 1) });

  expect(screen.queryByTestId('indexing')).toBeNull();
  expect(document.activeElement).toBe(screen.getByTestId('settings-nav-models'));
});

// A section change closes the panel without discarding the report — the
// report and the job survive; only the popup's own `open` does not.
test('a section change closes the open panel, and the report survives it', async () => {
  await openWindow();
  await emit(reading());

  const disclosure = screen.getByTestId('indexing') as HTMLDetailsElement;
  await fireEvent.click(screen.getByTestId('job-summary'));
  expect(disclosure.open).toBe(true);

  await fireEvent.click(screen.getByTestId('settings-nav-folders'));
  await tick();

  expect(disclosure.open).toBe(false);
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Індексація теки 1 з 2: /home/a/notes');
});

// An ordinary progress tick is not a reason to close: the popup stays open
// across a count changing underneath it.
test('an ordinary progress tick does not close an open panel', async () => {
  await openWindow();
  await emit(reading({ done: 1 }));

  const disclosure = screen.getByTestId('indexing') as HTMLDetailsElement;
  await fireEvent.click(screen.getByTestId('job-summary'));
  expect(disclosure.open).toBe(true);

  await emit(reading({ done: 2 }));

  expect(disclosure.open).toBe(true);
});

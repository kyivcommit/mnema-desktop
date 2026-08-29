import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import { tick } from 'svelte';
import Settings from './Settings.svelte';
import { setLocale, type Loc } from '../i18n';
import { OUTCOME_KINDS, type OutcomeKind } from './jobs';
import type { JobEnded, JobEvent, JobProgress } from '../lib/ipc';

// Only Tauri's own modules are faked. The whole settings window renders — the
// nav, the sections and the indexing strip — because the claim this file makes
// is about what a person READS on that window, and every previous round of this
// project that pinned a testid or a count instead shipped a defect a screenshot
// found in a minute.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...a: unknown[]) => invoke(...a),
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
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

const READY_SETTINGS = {
  key: { kind: 'present' },
  index: {
    kind: 'read', embeddingModel: 'openai/text-embedding-3-small', chatModel: null,
    embeddedChunks: 12, embeddedChunksEverywhere: 12, totalChunks: 12,
    searchTextArm: true, searchContentArm: true,
  },
  platform: 'linux',
};

const EMPTY_CATALOGUE = { entries: [], unreadable: 0, unreadableRecords: [] };

type Replies = Record<string, unknown>;
let replies: Replies = {};

function reply(extra: Replies = {}) {
  replies = {
    list_tree: ROOTS,
    model_settings: READY_SETTINGS,
    provider_models: EMPTY_CATALOGUE,
    job_status: { running: false },
    start_walk_job: undefined,
    start_embed_job: undefined,
    cancel_job: undefined,
    ...extra,
  };
}

beforeEach(() => {
  invoke.mockReset();
  reply();
  invoke.mockImplementation((cmd: string) => {
    const r = replies[cmd];
    if (r instanceof Error) return Promise.reject(r);
    return Promise.resolve(r);
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

function channelOf(cmd: string): (event: JobEvent) => void {
  const call = [...invoke.mock.calls].reverse().find((c) => c[0] === cmd);
  if (!call) throw new Error(`${cmd} was never invoked`);
  const channel = (call[1] as { onProgress: { onmessage: (e: JobEvent) => void } }).onProgress;
  return (event) => channel.onmessage(event);
}

async function openFolders(loc: Loc = 'uk') {
  setLocale(loc);
  const rendered = render(Settings);
  await fireEvent.click(screen.getByTestId('settings-nav-folders'));
  await screen.findByTestId('folder-row-4');
  return rendered;
}

const scanButton = (rootId: number) =>
  screen.getByTestId(`folder-scan-${rootId}`);

// ---------------------------------------------------------------------------
// Real endings, printed by a temporary Rust test from `walk_job.rs`'s own
// `ended_from_report` and `job::Ended::failed` — not written from a document.
// ---------------------------------------------------------------------------
const WALK: JobEnded = {
  reason: 'completed', done: 11, total: 11, skipped: 5, complete: true, frozen: [],
  indexed: 5, unchanged: 1, refused: 0, removed: 4, message: null,
};
const FROZEN = [
  { prefix: 'notes/archive', reason: 'unreadableDirectory' },
  { prefix: 'notes/link', reason: 'symlinkedSubtree' },
  { prefix: 'notes/void', reason: 'emptyDirectory' },
] as const;

const endedEvent = (over: Partial<JobEnded> = {}): JobEvent =>
  ({ event: 'ended', data: { ...WALK, ...over } });
const progressEvent = (over: Partial<JobProgress> = {}): JobEvent =>
  ({ event: 'progress', data: { done: 3, total: 8, skipped: 1, refused: 0, secondsLeft: null, ...over } });

// ---------------------------------------------------------------------------
// The exhaustive visible-state matrix. Nine rows, no wildcard, both locales.
//
// The four rows after `failed` are the ones a table of three would lose. They
// are NOT malfunctions — a broken helper, rules that did not take, an
// unreadable folder, a volume that may be gone — and `job.rs` says reporting
// them as `failed` tells a person something broke when instead a folder cannot
// be read. A test covering only completed/cancelled/failed passes on a
// component that collapses all four into one sentence.
// ---------------------------------------------------------------------------
type Row = {
  name: string;
  ending: Partial<JobEnded>;
  uk: string; en: string;
  /// The result line the SAME row puts on screen. Asserted beside the
  /// sentence because the two are read together and can contradict each
  /// other: nothing branches on the counts, so a row inheriting
  /// `completed`'s numbers prints "the folder was not read at all" above
  /// "Documents added: 5. Removed from the index: 4."
  result: { uk: string; en: string };
};

const FAILURE_TEXT = 'the worker binary could not be started';

// The counts each stop actually leaves behind, read off `walk.rs` rather than
// inherited from `completed`'s own numbers.
//
// `removed` is written by phase 3 alone, and phase 3 runs only when phase 1 saw
// the whole tree AND phase 2 ran to the end of what it handed over
// (`walk.rs:511`) — so `removed: 4` on any row but the first states a deletion
// this code cannot make. `rulesNotApplied` (`:384`) and `rootUnavailable`
// (`:298`) return before phase 2 as well: nothing was read at all, which is
// what their sentences say in words. `Ended::failed` (`job.rs:266`) zeroes
// every count of its own accord and keeps `done`/`total`.
const NOTHING_READ = { done: 0, total: 11, skipped: 0, refused: 0, indexed: 0, unchanged: 0, removed: 0 };
// A root that is not a directory never reaches `enumerate`, so not even the
// total is known (`walk.rs:289`).
const NO_ROOT = { ...NOTHING_READ, total: 0 };
// Stopped inside phase 2, part way through what phase 1 found.
const STOPPED_MIDWAY = { done: 3, total: 11, skipped: 1, refused: 0, indexed: 2, unchanged: 0, removed: 0 };
// The worker gives up only after several environmental skips in a row.
const WORKER_GAVE_UP = { done: 5, total: 11, skipped: 3, refused: 0, indexed: 2, unchanged: 0, removed: 0 };
// Phase 2 ran to the end; phase 3 was refused. Real work, no deletions.
const READ_NOT_RECONCILED = { done: 11, total: 11, skipped: 5, refused: 0, indexed: 5, unchanged: 1, removed: 0 };
const FAILED_COUNTS = { done: 7, total: 11, skipped: 0, refused: 0, indexed: 0, unchanged: 0, removed: 0 };

const NOTHING_DONE_RESULT = {
  uk: 'Додано документів: 0. Без змін: 0. Пропущено: 0. Вилучено з індексу: 0.',
  en: 'Documents added: 0. Unchanged: 0. Skipped: 0. Removed from the index: 0.',
};
const MIDWAY_RESULT = {
  uk: 'Додано документів: 2. Без змін: 0. Пропущено: 1. Вилучено з індексу: 0.',
  en: 'Documents added: 2. Unchanged: 0. Skipped: 1. Removed from the index: 0.',
};
const WORKER_RESULT = {
  uk: 'Додано документів: 2. Без змін: 0. Пропущено: 3. Вилучено з індексу: 0.',
  en: 'Documents added: 2. Unchanged: 0. Skipped: 3. Removed from the index: 0.',
};
const NOT_RECONCILED_RESULT = {
  uk: 'Додано документів: 5. Без змін: 1. Пропущено: 5. Вилучено з індексу: 0.',
  en: 'Documents added: 5. Unchanged: 1. Skipped: 5. Removed from the index: 0.',
};

const MATRIX: Row[] = [
  {
    name: 'completed',
    ending: { reason: 'completed', complete: true },
    uk: 'Теку прочитано повністю.',
    en: 'The folder was read in full.',
    result: {
      uk: 'Додано документів: 5. Без змін: 1. Пропущено: 5. Вилучено з індексу: 4.',
      en: 'Documents added: 5. Unchanged: 1. Skipped: 5. Removed from the index: 4.',
    },
  },
  {
    // Phase 1 never saw the whole tree, so phase 3 was skipped: nothing was
    // deleted, and that is precisely what the sentence warns about.
    //
    // 🔴 "nothing" is the whole root, not the unreadable subfolders (review
    // round 1, B1). `walk.rs:511` returns before `known` is read and before any
    // `delete_path`, so a rule newly covering a top-level folder nowhere near
    // the unreadable one survives too — measured `removed=0`, row kept, still
    // findable. The sentence below says so; it used to say "inside them".
    name: 'partlyRead',
    ending: { reason: 'completed', complete: false, ...READ_NOT_RECONCILED },
    uk: 'Теку прочитано лише частково: до якихось підтек не вдалося зайти. Нічого в цій теці не звіряли з індексом, тож і видалені файли, і файли під вашими правилами виключення досі знаходяться пошуком — не лише всередині тих підтек.',
    en: 'The folder was only partly read: some subfolders could not be entered. Nothing in this folder was checked against the index, so both deleted files and files your exclusion rules now cover are still found by search — not only inside those subfolders.',
    result: NOT_RECONCILED_RESULT,
  },
  {
    // `complete` stays TRUE: the cancel lands in phase 2 (`walk.rs:432`), and
    // phase 1 having finished says nothing about phase 2 being allowed to.
    name: 'cancelled',
    ending: { reason: 'cancelled', complete: true, ...STOPPED_MIDWAY },
    uk: 'Сканування зупинено на ваше прохання.',
    en: 'The scan was stopped at your request.',
    result: MIDWAY_RESULT,
  },
  {
    name: 'failed without a message',
    ending: { reason: 'failed', complete: false, message: null, ...FAILED_COUNTS },
    uk: 'Сканування обірвалося через збій.',
    en: 'The scan broke off because something went wrong.',
    result: NOTHING_DONE_RESULT,
  },
  {
    name: 'failed carrying a message',
    ending: { reason: 'failed', complete: false, message: FAILURE_TEXT, ...FAILED_COUNTS },
    uk: `Сканування обірвалося через збій. Програма повідомила: ${FAILURE_TEXT}`,
    en: `The scan broke off because something went wrong. The program reported: ${FAILURE_TEXT}`,
    result: NOTHING_DONE_RESULT,
  },
  {
    name: 'brokenWorker',
    ending: { reason: 'brokenWorker', complete: true, ...WORKER_GAVE_UP },
    uk: 'Сканування спинилося: допоміжна програма, яка читає файли, перестала відповідати.',
    en: 'The scan stopped: the helper program that reads files stopped answering.',
    result: WORKER_RESULT,
  },
  {
    // The rules gate sits before phase 2, so `complete` is phase 1's own
    // verdict and the counts are all zero — "not read at all" literally.
    name: 'rulesNotApplied',
    ending: { reason: 'rulesNotApplied', complete: true, ...NOTHING_READ },
    uk: 'Сканування спинилося: правила виключення не вдалося застосувати, тож теку не читали зовсім.',
    en: 'The scan stopped: the exclusion rules could not be applied, so the folder was not read at all.',
    result: NOTHING_DONE_RESULT,
  },
  {
    name: 'rootUnavailable',
    ending: { reason: 'rootUnavailable', complete: false, ...NO_ROOT },
    uk: 'Сканування спинилося: у теку не вдалося зайти. Можливо, її прибрали або диск від’єднано.',
    en: 'The scan stopped: the folder could not be entered. It may have been removed, or its drive disconnected.',
    result: NOTHING_DONE_RESULT,
  },
  {
    // `complete` cannot be false here: the volume check is reached only past
    // the `!walked.complete` return at `walk.rs:511`.
    name: 'volumeMissing',
    ending: { reason: 'volumeMissing', complete: true, ...READ_NOT_RECONCILED },
    uk: 'Сканування спинилося: тека прочиталася порожньою, хоча в індексі є файли з неї. Нічого не вилучено — можливо, диск під’єднано не повністю.',
    en: 'The scan stopped: the folder read as empty although the index still holds files from it. Nothing was deleted — the drive may not be fully attached.',
    result: NOT_RECONCILED_RESULT,
  },
];

// Both directions on the table itself: a wire reason the matrix says nothing
// about, and a matrix row for a state that cannot happen, are both defects.
test('the matrix names one row for every state a walk can end in, and no others', () => {
  const covered = new Set(MATRIX.map((r) => (
    r.ending.reason === 'completed' && r.ending.complete === false ? 'partlyRead' : r.ending.reason
  )));
  expect([...covered].sort()).toEqual([...OUTCOME_KINDS].sort());
  expect(MATRIX).toHaveLength(OUTCOME_KINDS.length + 1); // + the `failed` row that carries a message
});

for (const loc of ['uk', 'en'] as const) {
  test(`every way a walk can end shows its own sentence, and no two share one (${loc})`, async () => {
    const seen: string[] = [];
    for (const row of MATRIX) {
      await openFolders(loc);
      await fireEvent.click(scanButton(4));
      await waitFor(() => expect(calls('start_walk_job')).not.toHaveLength(0));

      channelOf('start_walk_job')(endedEvent(row.ending));
      const region = await screen.findByTestId('indexing-walk-outcome');
      await tick();

      expect(visible(region), row.name).toBe(row[loc]);
      // The line directly under it, in the same breath a person reads them:
      // an oracle that looks only at the outcome sentence lets the screen
      // contradict itself under its own eye.
      expect(visible(screen.getByTestId('indexing-walk-result')), row.name).toBe(row.result[loc]);
      seen.push(visible(region));
      cleanup();
      invoke.mockClear();
    }
    // A component that drew one sentence for the four variants after `failed`
    // produces nine distinct rows in the reducer and this many collisions here.
    expect(new Set(seen).size).toBe(MATRIX.length);
  });
}

// ---------------------------------------------------------------------------
// Pressing the control, and what the counts read as.
// ---------------------------------------------------------------------------

test('pressing a folder`s scan control starts the walk for THAT folder', async () => {
  await openFolders();

  await fireEvent.click(scanButton(4));

  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));
  expect((calls('start_walk_job')[0][1] as { rootId: number }).rootId).toBe(4);
  // The visible label is the plain word, not the per-row aria-label: an
  // `aria-label` overrides the accessible name, so `getByRole(..., { name })`
  // would go on passing after the visible text went stale.
  expect(scanButton(4).textContent).toBe('Сканувати');
  expect(scanButton(4).getAttribute('aria-label')).toBe('Сканувати /home/a/papers');
});

test('the running line reads as words, with the counts in them', async () => {
  const { container } = await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(progressEvent());
  await tick();

  const text = container.textContent ?? '';
  expect(text).toContain('Триває читання теки.');
  expect(text).toContain('Опрацьовано 3 з 8. Пропущено: 1. Відхилено: 0.');
  // `secondsLeft` is `Option<u64>` and arrives as `null` for the whole of an
  // ordinary run's beginning. A line that formats it unconditionally prints
  // "залишилось null" here.
  expect(text).toContain('Скільки ще лишилось часу, поки не відомо.');
  expect(text).not.toContain('null');
});

test('a run with nothing counted yet says so instead of reading as nothing to do', async () => {
  const { container } = await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(progressEvent({ done: 0, total: 0, secondsLeft: 12 }));
  await tick();

  const text = container.textContent ?? '';
  expect(text).toContain('Опрацьовано 0. Скільки їх усього, поки не відомо.');
  expect(text).not.toContain('0 з 0');
  // The other direction of the same field: an estimate that IS known is shown.
  expect(text).toContain('Залишилось приблизно 12 с.');
});

// The third state of the same `Option<u64>`, and the one a truthiness check
// eats: zero seconds left is a NUMBER — the run is about to finish — while
// "not known yet" is what the window says when it has been told nothing. A
// line written `seconds ? … : …` shows the second for the first.
test('nought seconds left is an estimate, not an absence of one', async () => {
  const { container } = await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(progressEvent({ secondsLeft: 0 }));
  await tick();

  const text = container.textContent ?? '';
  expect(text).toContain('Залишилось приблизно 0 с.');
  expect(text).not.toContain('Скільки ще лишилось часу, поки не відомо.');
});

test('an ended walk reports what it did to the index, not only that it ended', async () => {
  const { container } = await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(screen.getByTestId('indexing-walk-result')).toBeTruthy());

  expect(container.textContent).toContain(
    'Додано документів: 5. Без змін: 1. Пропущено: 5. Вилучено з індексу: 4.',
  );
});

// ---------------------------------------------------------------------------
// `frozen`: the decision is to SHOW it.
// ---------------------------------------------------------------------------

// `removed == 0` alone cannot say whether anything was silently left untouched
// (job.rs). Dropping this field would leave a person reading "removed: 4" with
// no way to learn that three subtrees were never reconciled at all.
test('subtrees reconciliation refused to touch are named, each with its own reason', async () => {
  const { container } = await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(endedEvent({ complete: false, removed: 0, frozen: [...FROZEN] }));
  await waitFor(() => expect(screen.getByTestId('indexing-frozen')).toBeTruthy());

  const text = container.textContent ?? '';
  expect(text).toContain('Ці підтеки не звіряли, тож і видалені файли, і файли під вашими правилами виключення досі знаходяться пошуком:');
  expect(text).toContain('notes/archive — не вдалося прочитати');
  expect(text).toContain('notes/link — символьне посилання, сюди не заходили');
  expect(text).toContain('notes/void — прочиталася порожньою');
});

// Two entries in one report can carry the SAME prefix: `walk.rs` decides
// whether to climb by testing `parent`, then pushes `resolve_ancestor`'s
// answer, which is a different string whenever `parent` is not itself on disk.
// A list keyed by that prefix throws and takes the whole section with it.
test('two frozen entries sharing a prefix are both shown, not a crash', async () => {
  const { container } = await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(endedEvent({
    complete: false,
    frozen: [
      { prefix: 'notes/archive', reason: 'unreadableDirectory' },
      { prefix: 'notes/archive', reason: 'emptyDirectory' },
    ],
  }));
  await waitFor(() => expect(screen.getByTestId('indexing-frozen')).toBeTruthy());

  expect(screen.getByTestId('indexing-frozen').querySelectorAll('li')).toHaveLength(2);
  expect(container.textContent).toContain('notes/archive — прочиталася порожньою');
});

test('a walk that froze nothing shows no such list', async () => {
  await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(screen.getByTestId('indexing-walk-outcome')).toBeTruthy());

  expect(screen.queryByTestId('indexing-frozen')).toBeNull();
});

// ---------------------------------------------------------------------------
// The chained embedding pass.
// ---------------------------------------------------------------------------

test('a partly read folder is embedded anyway AND stays reported as partly read', async () => {
  await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(endedEvent({ complete: false, removed: 0 }));
  await waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));

  expect(visible(screen.getByTestId('indexing-walk-outcome')))
    .toBe(MATRIX.find((r) => r.name === 'partlyRead')!.uk);
});

test('the embedding pass says it covers the whole index, never the folder that was pressed', async () => {
  const { container } = await openFolders();
  await fireEvent.click(scanButton(4));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));
  channelOf('start_embed_job')(progressEvent({ done: 2, total: 40 }));
  await tick();

  const text = container.textContent ?? '';
  expect(text).toContain('Триває вбудовування всього індексу.');
  expect(text).not.toContain('Триває читання теки.');
});

// The window checks both preconditions ITSELF and names the one that is
// absent. The walk still ran — text search needs neither.
test('with no provider key the pass is not started and the section says which is missing', async () => {
  reply({ model_settings: { ...READY_SETTINGS, key: { kind: 'absent' } } });
  const { container } = await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(screen.getByTestId('indexing-note')).toBeTruthy());

  expect(screen.getByTestId('indexing-note').textContent).toBe(
    'Пошук за змістом не вмикали: ключ провайдера не збережено. Пошук по словах у цій теці вже працює.',
  );
  expect(calls('start_embed_job')).toHaveLength(0);
  expect(container.textContent).toContain('Теку прочитано повністю.');
});

test('with no embedding model chosen the missing one is named apart from the key', async () => {
  reply({
    model_settings: { ...READY_SETTINGS, index: { ...READY_SETTINGS.index, embeddingModel: null } },
  });
  await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(screen.getByTestId('indexing-note')).toBeTruthy());

  expect(screen.getByTestId('indexing-note').textContent).toBe(
    'Пошук за змістом не вмикали: модель вбудовування не обрана. Пошук по словах у цій теці вже працює.',
  );
  expect(calls('start_embed_job')).toHaveLength(0);
});

// The second line, route one: the key went missing between the window's read
// and the call, so the command rejects before claiming the slot. A rejection
// crosses as a sentence — shown verbatim, never matched on.
test('a pass refused by the backend shows the backend`s own sentence', async () => {
  reply({ start_embed_job: new Error('no provider key is stored') });
  await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(screen.getByTestId('indexing-rejection')).toBeTruthy());

  expect(screen.getByTestId('indexing-rejection').textContent).toBe('no provider key is stored');
  expect(screen.getByTestId('indexing-note').textContent).toBe('Запит відхилено.');
});

// The second line, route two, and it is a different shape entirely: a missing
// model is not checked by `start_embed_job` at all (embed_job.rs), so the
// command is ACCEPTED and the refusal arrives as an ending carrying a sentence.
test('a model that vanished after the check arrives as an ending, and its text is shown', async () => {
  const { container } = await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));
  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));

  channelOf('start_embed_job')(endedEvent({
    reason: 'failed', complete: false, message: 'the index has no active vector space',
  }));
  await waitFor(() => expect(screen.getByTestId('indexing-embed-outcome')).toBeTruthy());

  const text = container.textContent ?? '';
  expect(visible(screen.getByTestId('indexing-embed-outcome'))).toBe(
    'Вбудовування обірвалося через збій. Програма повідомила: the index has no active vector space',
  );
  // The walk's own ending is still beside it: two passes, two results.
  expect(text).toContain('Теку прочитано повністю.');
});

test('an embedding pass stopping for a walk-only reason is not drawn as a finished one', async () => {
  await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));
  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));

  channelOf('start_embed_job')(endedEvent({ reason: 'volumeMissing' }));
  await waitFor(() => expect(screen.getByTestId('indexing-embed-outcome')).toBeTruthy());

  expect(visible(screen.getByTestId('indexing-embed-outcome'))).toBe(
    'Вбудовування спинилося з причини, якої тут не очікували (volumeMissing).',
  );
  // The pass reports its own counts too, and they are the embedding pass's —
  // chunks over the whole index, not documents in the folder that was pressed.
  expect(visible(screen.getByTestId('indexing-embed-result')))
    .toBe('Вбудовано фрагментів: 11 з 11. Відхилено: 0.');
});

// ---------------------------------------------------------------------------
// PR 25 review, P1-1 and P1-2 — state that must outlive a section used to live
// inside it.
//
// `Models` was the one section rendered without the controller, and inside the
// section conditional, so every click on another nav item destroyed it. Two
// things died with it: the flag the degraded warning was conditioned on, and
// the listener the recovery pass reported to.
//
// These are read on the WINDOW, along the path a person actually walks —
// discard, leave, come back — because both defects are invisible to anything
// that renders the section on its own and never unmounts it.
// ---------------------------------------------------------------------------

const DEGRADED_UK =
  'Пошук за змістом недоступний, доки індекс не буде вбудовано наново. Пошук за словами працює далі.';
const READY_UK = 'Підключено — OpenRouter, ключ і обрана модель embedding готові.';

/// The window's settings as `model_settings` answers them: an index on `emb-1`
/// holding `total` chunks of document text, with `active` of them embedded in
/// the space it points at.
const onModel = (active: number, everywhere = active, total = 12) => ({
  key: { kind: 'present' },
  index: {
    kind: 'read', embeddingModel: 'emb-1', chatModel: null,
    embeddedChunks: active, embeddedChunksEverywhere: everywhere, totalChunks: total,
    searchTextArm: true, searchContentArm: true,
  },
  platform: 'linux',
});

const model = (id: string, name: string) => ({
  id, name, inputLimit: { kind: 'notStated' }, price: { kind: 'notStated' }, refusal: null,
});

/// Opens the window on Models with two models to choose between, and drives the
/// change that takes semantic search away: press the other model, confirm, and
/// let `model_settings` answer with the index the change leaves behind.
async function discardOnModels() {
  replies.model_settings = onModel(7);
  replies.provider_models = {
    entries: [model('emb-1', 'Embedder One'), model('emb-2', 'Embedder Two')],
    unreadable: 0, unreadableRecords: [],
  };
  replies.set_embedding_model = {
    model: 'emb-2', dim: 1024, spaceId: 2, created: true,
    retired: [{ spaceId: 1, embeddedChunks: 4 }], index: onModel(0).index,
  };
  setLocale('uk');
  const rendered = render(Settings);
  await screen.findByTestId('model-entry-emb-2');

  // What the index says once the change has landed: a new space, and nothing
  // in it. Twelve chunks of text are still there, waiting to be embedded.
  replies.model_settings = onModel(0);
  await fireEvent.click(screen.getByTestId('model-entry-emb-2'));
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));
  await waitFor(() => expect(screen.getByTestId('model-embedding-degraded-note')).toBeTruthy());
  return rendered;
}

const toModels = async () => {
  await fireEvent.click(screen.getByTestId('settings-nav-models'));
  await screen.findByTestId('model-status-dot');
};
const toFolders = async () => {
  await fireEvent.click(screen.getByTestId('settings-nav-folders'));
  await screen.findByTestId('folder-row-4');
};

// 🔴 The whole finding, read as a person reads it. Before the fix this window
// came back showing a green dot and nothing else: the warning and the button
// that repairs the loss were both gone, while the backend went on reporting an
// empty active space.
test('the warning about a search gone dark survives leaving the section and coming back', async () => {
  const { container } = await discardOnModels();
  expect(container.textContent).toContain(DEGRADED_UK);

  await toFolders();
  await toModels();

  // Every line of it, in the order the section draws them.
  expect(screen.getByTestId('model-embedding-degraded-note').textContent).toBe(DEGRADED_UK);
  expect(screen.getByRole('button', { name: 'Вбудувати індекс наново' })).toBeTruthy();
  const text = container.textContent ?? '';
  // The dot still says what it has always said — provider, key and a chosen
  // model — and it no longer has the last word: the loss is stated after it,
  // which is the ruling Task 6's review settled and the vanishing warning had
  // silently undone.
  expect(text).toContain(READY_UK);
  expect(text.indexOf(READY_UK)).toBeLessThan(text.indexOf(DEGRADED_UK));
});

// The other direction of the same re-read, and the one a warning that never
// goes away would satisfy: the index refilled, so there is nothing left to say.
test('a section coming back to a refilled index says nothing about a search gone dark', async () => {
  const { container } = await discardOnModels();

  replies.model_settings = onModel(12);
  await toFolders();
  await toModels();

  expect(screen.queryByTestId('model-embedding-degraded-note')).toBeNull();
  expect(screen.queryByTestId('model-embedding-reembed')).toBeNull();
  expect(container.textContent).toContain(READY_UK);
});

// 🔴 P1-2. The pass reported to a listener inside the section, so switching
// away unmounted the only observer while the backend job ran on: the strip
// stayed idle, and the progress and the Cancel were gone.
test('the recovery pass keeps reporting, and stays stoppable, after a section switch', async () => {
  await discardOnModels();

  await fireEvent.click(screen.getByTestId('model-embedding-reembed'));
  await waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));
  channelOf('start_embed_job')(progressEvent({ done: 3, total: 12 }));
  await tick();

  // On the section it was started from, first.
  expect(screen.getByTestId('indexing-pass').textContent).toBe('Триває вбудовування всього індексу.');
  expect(screen.getByTestId('indexing-counts').textContent).toBe('Опрацьовано 3 з 12. Пропущено: 1. Відхилено: 0.');

  await toFolders();

  // And from a section that knows nothing about models: the strip is the
  // window's status line, and the pass is the window's.
  expect(screen.getByTestId('indexing-pass').textContent).toBe('Триває вбудовування всього індексу.');
  expect(screen.getByTestId('indexing-counts').textContent).toBe('Опрацьовано 3 з 12. Пропущено: 1. Відхилено: 0.');
  await fireEvent.click(screen.getByTestId('indexing-cancel'));
  expect(calls('cancel_job')).toHaveLength(1);

  // And the ending reaches the window wherever the person is standing.
  channelOf('start_embed_job')(endedEvent({ reason: 'cancelled' }));
  await waitFor(() => expect(screen.getByTestId('indexing-embed-outcome')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-embed-outcome')))
    .toBe('Вбудовування зупинено на ваше прохання.');
});

// The pass ending is what asks the index again, and the section is watching the
// controller for it: the warning clears itself, with the person standing on the
// section and pressing nothing. Written against the WINDOW because the listener
// this replaced was handed to `startEmbedJob` by the section — it heard a pass
// this section started, and only that one.
test('a pass that repairs the index clears the warning while the section is on screen', async () => {
  const { container } = await discardOnModels();
  await fireEvent.click(screen.getByTestId('model-embedding-reembed'));
  await waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));
  expect(screen.getByTestId('model-embedding-reembed-started').textContent).toBe('Вбудовування почалося.');

  // What the index says once the pass has filled the space.
  replies.model_settings = onModel(12);
  channelOf('start_embed_job')(endedEvent());

  await waitFor(() => expect(screen.queryByTestId('model-embedding-degraded-note')).toBeNull());
  expect(screen.queryByTestId('model-embedding-reembed')).toBeNull();
  expect(container.textContent).toContain(READY_UK);
});

// The same ending with the person standing somewhere else. Models is unmounted
// then, so its own listener is not what answers here — the mount's read is, on
// the way back. Both paths lead to the same screen, and this is the one the
// section's subscription CANNOT cover, so it is asserted rather than assumed.
test('a pass that ends while another section is on screen leaves nothing stale to come back to', async () => {
  await discardOnModels();
  await fireEvent.click(screen.getByTestId('model-embedding-reembed'));
  await waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));

  await toFolders();
  replies.model_settings = onModel(12);
  channelOf('start_embed_job')(endedEvent());
  await waitFor(() => expect(screen.getByTestId('indexing-embed-outcome')).toBeTruthy());

  await toModels();
  expect(screen.queryByTestId('model-embedding-degraded-note')).toBeNull();
});

// ---------------------------------------------------------------------------
// PR 25 review, P2-3 — an old walk's continuation writing over a newer scan.
//
// `chain()` resumes after `await modelSettings()`, and during that await
// another scan can start and take the job slot. The stale continuation then
// writes `starting`/`embed` over the live walk, is refused with "a job is
// already running", and its own catch resets the store to `idle` — so the
// running walk and its Stop are HIDDEN, and stay hidden until some other event
// happens to arrive.
//
// Driven with a deferred promise rather than a timer: the race is about which
// continuation resumes when, and a timer would assert a schedule instead of an
// ordering. Nothing here waits on a clock.
// ---------------------------------------------------------------------------
test('a superseded walk`s continuation does not hide the scan that replaced it', async () => {
  // `start_embed_job` is refused the way the backend really refuses it here:
  // the newer walk holds the slot. That refusal is what the stale catch used to
  // turn into `idle`.
  replies.start_embed_job = new Error('a job is already running');
  const { container } = await openFolders();

  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  // The first walk ends, so its continuation starts — and is held inside
  // `model_settings`, which is where the window's read of the two
  // preconditions lives.
  let release!: (value: unknown) => void;
  replies.model_settings = new Promise((resolve) => { release = resolve; });
  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(calls('model_settings').length).toBeGreaterThan(1));

  // A second scan, on the OTHER folder, while that read is still in flight. It
  // takes the slot and starts reporting.
  await fireEvent.click(scanButton(4));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(2));
  channelOf('start_walk_job')(progressEvent({ done: 2, total: 9 }));
  await tick();

  // Now let the superseded continuation resume, with everything it needs to
  // succeed: a key, a model, and nothing in the answer to tell it that the
  // world moved on.
  release(READY_SETTINGS);
  await tick();
  await tick();
  await tick();

  // Read as a person reads it: the live walk is still on screen, with its own
  // counts, and the control that stops it is still there.
  const text = container.textContent ?? '';
  expect(text).toContain('Триває читання теки.');
  expect(text).toContain('Опрацьовано 2 з 9. Пропущено: 1. Відхилено: 0.');
  expect(screen.getByTestId('indexing-cancel')).toBeTruthy();
  // And the sentence the stale continuation would have put there instead is
  // absent — both the embedding line and the refusal note it collects.
  expect(text).not.toContain('Вбудовування всього індексу починається…');
  expect(screen.queryByTestId('indexing-note')).toBeNull();
  // The pass the superseded walk was going to chain was never even asked for:
  // the guard returns before the command, so the backend is not made to refuse
  // something this window already knows is not its turn.
  expect(calls('start_embed_job')).toHaveLength(0);
});

// The same await, its other exit. `model_settings` can be refused, and the
// refusal is reported as a note — so a superseded continuation whose read fails
// would put a sentence on screen about a pass the person has already replaced.
// One guard per exit, and each named by the test that has to fail without it.
test('a superseded walk whose precondition read is refused says nothing about it', async () => {
  const { container } = await openFolders();

  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  let refuse!: (reason: unknown) => void;
  replies.model_settings = new Promise((_resolve, reject) => { refuse = reject; });
  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(calls('model_settings').length).toBeGreaterThan(1));

  await fireEvent.click(scanButton(4));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(2));
  channelOf('start_walk_job')(progressEvent({ done: 4, total: 5 }));
  await tick();

  refuse(new Error('LEAK-TOKEN-STALE-READ'));
  await tick();
  await tick();
  await tick();

  const text = container.textContent ?? '';
  expect(text).toContain('Опрацьовано 4 з 5. Пропущено: 1. Відхилено: 0.');
  expect(screen.queryByTestId('indexing-note')).toBeNull();
  expect(text).not.toContain('LEAK-TOKEN-STALE-READ');
});

// The SECOND await in the same continuation, and it needs a case of its own:
// with the first guard in place a superseded walk never reaches the command, so
// nothing above can make this one die. The window a new scan fits into here is
// `start_embed_job` itself being in flight — the refusal it collects is about
// the newer job holding the slot, and the catch would reset that job\'s
// `starting` to `idle`.
test('a continuation superseded while its embed command is in flight leaves the newer scan alone', async () => {
  let refuse!: (reason: unknown) => void;
  replies.start_embed_job = new Promise((_resolve, reject) => { refuse = reject; });
  const { container } = await openFolders();

  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));
  // Ends, chains, passes both preconditions, and stops inside the command.
  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));

  await fireEvent.click(scanButton(4));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(2));
  channelOf('start_walk_job')(progressEvent({ done: 6, total: 7 }));
  await tick();

  refuse(new Error('a job is already running'));
  await tick();
  await tick();
  await tick();

  const text = container.textContent ?? '';
  expect(text).toContain('Триває читання теки.');
  expect(text).toContain('Опрацьовано 6 з 7. Пропущено: 1. Відхилено: 0.');
  expect(screen.getByTestId('indexing-cancel')).toBeTruthy();
  // The refusal belongs to a pass nobody is watching any more, so it is not
  // reported as though it were about the scan on screen.
  expect(screen.queryByTestId('indexing-note')).toBeNull();
  expect(text).not.toContain('a job is already running');
});

// The other direction, and the one a guard that simply never chains would
// satisfy: an ordinary walk, with nothing racing it, still chains its pass.
test('a walk that nothing supersedes still chains its embedding pass', async () => {
  await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(endedEvent());

  await waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));
  expect(screen.getByTestId('indexing-pass').textContent).toBe('Вбудовування всього індексу починається…');
});

// ---------------------------------------------------------------------------
// Cancel, and where the job state lives.
// ---------------------------------------------------------------------------

test('cancelling asks the backend to stop, and the line then says it stopped', async () => {
  const { container } = await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));
  channelOf('start_walk_job')(progressEvent());
  await tick();
  expect(container.textContent).toContain('Триває читання теки.');

  await fireEvent.click(screen.getByTestId('indexing-cancel'));
  expect(calls('cancel_job')).toHaveLength(1);

  channelOf('start_walk_job')(endedEvent({ reason: 'cancelled' }));
  await waitFor(() => expect(screen.getByTestId('indexing-walk-outcome')).toBeTruthy());
  const text = container.textContent ?? '';
  expect(text).toContain('Сканування зупинено на ваше прохання.');
  expect(text).not.toContain('Триває читання теки.');
});

// 🔴 The main path, not an edge case: four nav items mean «Теки → Моделі →
// Теки» is two clicks. The channel belongs to whoever started the job, so a
// component that keeps the job inside the section takes the counters AND the
// Cancel button with it when it unmounts — and `cancel_job` needs no channel,
// so that Cancel is lost for nothing.
test('a job survives switching sections, and Cancel still stops it afterwards', async () => {
  const { container } = await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));
  channelOf('start_walk_job')(progressEvent());
  await tick();

  await fireEvent.click(screen.getByTestId('settings-nav-models'));
  await tick();
  await fireEvent.click(screen.getByTestId('settings-nav-folders'));
  await screen.findByTestId('folder-row-4');

  expect(container.textContent).toContain('Опрацьовано 3 з 8. Пропущено: 1. Відхилено: 0.');
  await fireEvent.click(screen.getByTestId('indexing-cancel'));
  expect(calls('cancel_job')).toHaveLength(1);
});

// The other direction, and the half a Cancel rendered unconditionally would
// satisfy on its own.
test('with no job running there is no Cancel to press', async () => {
  await openFolders();

  expect(screen.queryByTestId('indexing-cancel')).toBeNull();
  expect(calls('cancel_job')).toHaveLength(0);
  // And the strip itself is not there at all. An empty div left standing looks
  // near-identical in a browser, which is exactly why nothing would notice it —
  // a window somebody opened to change a model should carry no indexing strip
  // saying nothing.
  expect(screen.queryByTestId('indexing')).toBeNull();
});

// The other half of the same decision: once there IS something to say, the
// strip is there to say it.
test('the strip appears as soon as there is something to report', async () => {
  await openFolders();
  await fireEvent.click(scanButton(1));

  await waitFor(() => expect(screen.getByTestId('indexing')).toBeTruthy());
});

// ---------------------------------------------------------------------------
// A job this window has no channel for.
// ---------------------------------------------------------------------------

// `job_status` carries a boolean and nothing else, and `set_embedding_model`
// holds the same slot without ever sending an ending — so a component drawing
// "indexing" from that boolean can sit on a progress line nothing will finish.
test('a job with no channel of ours is said in words, with no counts invented for it', async () => {
  reply({ job_status: { running: true } });
  const { container } = await openFolders();

  await waitFor(() => expect(screen.getByTestId('indexing-unobserved')).toBeTruthy());
  const text = container.textContent ?? '';
  expect(screen.getByTestId('indexing-unobserved').textContent).toBe(
    'Зараз виконується інше завдання. Це вікно не бачить, як далеко воно просунулося, але зупинити його можна.',
  );
  expect(text).not.toContain('Опрацьовано');
  expect(text).not.toContain('Триває читання теки.');
  // Cancel is still offered: `cancel_job` needs no channel, and losing the
  // ability to stop a job is the one failure a person cannot work around.
  expect(screen.getByTestId('indexing-cancel')).toBeTruthy();
});

test('cancelling a job we cannot hear re-reads the status instead of waiting for an ending', async () => {
  reply({ job_status: { running: true } });
  await openFolders();
  await waitFor(() => expect(screen.getByTestId('indexing-unobserved')).toBeTruthy());

  reply({ job_status: { running: false } });
  await fireEvent.click(screen.getByTestId('indexing-cancel'));

  await waitFor(() => expect(screen.queryByTestId('indexing-unobserved')).toBeNull());
  expect(screen.queryByTestId('indexing-cancel')).toBeNull();
});

// 🔴 One press, and the Cancel is gone for the life of the window. `scan` opens
// by writing `starting` over whatever was there — destroying `runningUnobserved`
// — and the commonest reason `start_walk_job` is then refused is that the very
// job that state described still holds the slot. `syncFromStatus` runs once, at
// mount, so nothing would ever put it back: the person is left watching a job
// they can no longer stop, and reopening the window is the only way out.
test('a scan refused because a job we cannot hear holds the slot leaves its Stop in place', async () => {
  reply({ job_status: { running: true }, start_walk_job: new Error('a job is already running') });
  await openFolders();
  await waitFor(() => expect(screen.getByTestId('indexing-unobserved')).toBeTruthy());

  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(screen.getByTestId('indexing-rejection')).toBeTruthy());

  expect(screen.queryByTestId('indexing-cancel')).not.toBeNull();
  expect(screen.getByTestId('indexing-unobserved').textContent).toBe(
    'Зараз виконується інше завдання. Це вікно не бачить, як далеко воно просунулося, але зупинити його можна.',
  );
  expect(screen.getByTestId('indexing-rejection').textContent).toBe('a job is already running');
});

// The same re-read must not invent a job either: a refusal with nothing running
// leaves the window idle, not sitting on a Cancel that stops nothing.
test('a scan refused with nothing running leaves no Stop behind', async () => {
  reply({ start_walk_job: new Error('the index could not be opened') });
  await openFolders();

  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(screen.getByTestId('indexing-rejection')).toBeTruthy());

  expect(screen.queryByTestId('indexing-cancel')).toBeNull();
  expect(screen.queryByTestId('indexing-unobserved')).toBeNull();
});

// ---------------------------------------------------------------------------
// Rejections and language.
// ---------------------------------------------------------------------------

test('a refused scan shows the backend sentence and starts nothing', async () => {
  reply({ start_walk_job: new Error('a job is already running') });
  await openFolders();

  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(screen.getByTestId('indexing-rejection')).toBeTruthy());

  expect(screen.getByTestId('indexing-rejection').textContent).toBe('a job is already running');
  expect(screen.queryByTestId('indexing-cancel')).toBeNull();
});

// D130 asks every visible string to follow a live language switch, and each
// one is a `$derived.by` reading `$locale` for itself — so each has to be
// switched under, one at a time. A single test over one state satisfies the
// rule for the lines that state happens to draw and no others: the six tests
// below exist because a running strip, a job with no channel, a note, an
// embedding result and a folder row draw disjoint sets of lines.

test('a language switch after a WALK ending reaches its sentence, its counts and its frozen list', async () => {
  const { container } = await openFolders('uk');
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));
  channelOf('start_walk_job')(endedEvent({ complete: false, removed: 0, frozen: [...FROZEN] }));
  await waitFor(() => expect(screen.getByTestId('indexing-frozen')).toBeTruthy());

  setLocale('en');
  await tick();

  const text = container.textContent ?? '';
  expect(text).toContain(MATRIX.find((r) => r.name === 'partlyRead')!.en);
  expect(text).toContain('Documents added: 5.');
  expect(text).toContain('notes/link — a symbolic link, never entered');
  expect(text).not.toContain('Теку прочитано');
});

// The lines a WALK ending never draws: the pass line, the counts, the estimate
// and the button that stops it. These are the ones a person is looking at for
// the longest, and every one of them is a derived of its own.
test('a language switch DURING a pass reaches its line, its counts, its estimate and its Stop', async () => {
  const { container } = await openFolders('uk');
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));
  channelOf('start_walk_job')(progressEvent({ secondsLeft: 12 }));
  await tick();
  expect(container.textContent).toContain('Триває читання теки.');

  setLocale('en');
  await tick();

  const text = container.textContent ?? '';
  expect(screen.getByTestId('indexing-pass').textContent).toBe('The folder is being read.');
  expect(screen.getByTestId('indexing-counts').textContent)
    .toBe('Processed 3 of 8. Skipped: 1. Given up on: 0.');
  expect(screen.getByTestId('indexing-eta').textContent).toBe('About 12 s left.');
  expect(screen.getByTestId('indexing-cancel').textContent).toBe('Stop');
  expect(text).not.toContain('Триває читання теки.');
});

// A job with no channel of ours draws neither of the two above: one sentence
// and the button.
test('a language switch reaches the sentence for a job we cannot hear, and its Stop', async () => {
  reply({ job_status: { running: true } });
  await openFolders('uk');
  await waitFor(() => expect(screen.getByTestId('indexing-unobserved')).toBeTruthy());

  setLocale('en');
  await tick();

  expect(screen.getByTestId('indexing-unobserved').textContent).toBe(
    'Another job is running. This window cannot see how far it has got, but it can still be stopped.',
  );
  expect(screen.getByTestId('indexing-cancel').textContent).toBe('Stop');
});

// The note, and the sentence beside it that must NOT move: the backend's own
// text crosses the IPC as words (`error.rs`) and belongs to no catalogue.
test('a language switch reaches the note and leaves the backend`s own sentence verbatim', async () => {
  reply({ start_walk_job: new Error('a job is already running') });
  await openFolders('uk');
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(screen.getByTestId('indexing-note')).toBeTruthy());
  expect(screen.getByTestId('indexing-note').textContent).toBe('Запит відхилено.');

  setLocale('en');
  await tick();

  expect(screen.getByTestId('indexing-note').textContent).toBe('The request was refused.');
  expect(screen.getByTestId('indexing-rejection').textContent).toBe('a job is already running');
});

// The embedding pass has its own table and its own result line — neither is
// reached by switching under a walk.
test('a language switch after an EMBEDDING ending reaches its sentence and its counts', async () => {
  await openFolders('uk');
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));
  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));
  channelOf('start_embed_job')(endedEvent());
  await waitFor(() => expect(screen.getByTestId('indexing-embed-outcome')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-embed-outcome')))
    .toBe('Вбудовування всього індексу завершено.');

  setLocale('en');
  await tick();

  expect(visible(screen.getByTestId('indexing-embed-outcome')))
    .toBe('Embedding the whole index has finished.');
  expect(visible(screen.getByTestId('indexing-embed-result')))
    .toBe('Chunks embedded: 11 of 11. Given up on: 0.');
});

// The control that starts all of the above. It lives on every folder row, and
// its label is a derived of its own — the row array's `void $locale` rebuilds
// the aria-labels, not this.
test('a language switch reaches the scan control on every folder row', async () => {
  await openFolders('uk');
  expect(scanButton(1).textContent).toBe('Сканувати');

  setLocale('en');
  await tick();

  expect(scanButton(1).textContent).toBe('Scan');
  expect(scanButton(4).textContent).toBe('Scan');
});

// Every outcome kind must have a sentence in both locales, and the check is on
// the table rather than on a count: a number is a definition too, and it has
// been the wrong one here before.
test('nothing in the outcome vocabulary is left without words', () => {
  const named = new Set<OutcomeKind>(MATRIX.map((r) => (
    r.ending.reason === 'completed' && r.ending.complete === false ? 'partlyRead' : r.ending.reason as OutcomeKind
  )));
  for (const kind of OUTCOME_KINDS) expect(named.has(kind), kind).toBe(true);
});

// ---------------------------------------------------------------------------
// Live run, finding 2 — the folder row states a falsehood after its own scan.
//
// On a real screen the row read «Проіндексовано: 0 документів» while the report
// directly beneath it said four documents had been added, and the index agreed
// with the report: `SELECT COUNT(*) … WHERE watched_root_id = 4` → 4. Task 7
// re-reads the list after an add and after a remove; the one event nobody
// wired is the one that changes the number the row shows.
// ---------------------------------------------------------------------------

const FOUR_FILES = [
  { relativePath: '01-vulpine-notes.md', documentId: 'd1' },
  { relativePath: '02-survey.md', documentId: 'd2' },
  { relativePath: '03-method.md', documentId: 'd3' },
  { relativePath: '04-appendix.md', documentId: 'd4' },
];

const treeReads = () => calls('list_tree').length;

test('the folder row re-reads when the job that changed it ends, and not before', async () => {
  await openFolders();
  // Both directions start here: the row states zero, which is TRUE until the
  // walk lands. A test that only asserts the four would pass on a row that had
  // said four all along.
  expect(visible(screen.getByTestId('folder-row-4'))).toContain('Проіндексовано: 0 документів');

  await fireEvent.click(scanButton(4));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));
  // The index now holds the four documents the walk wrote. Swapped mid-run, so
  // the number on screen can only become four by the list being READ again —
  // never by anything this component could have kept from the ending itself.
  reply({
    list_tree: { roots: [ROOTS.roots[0], { ...ROOTS.roots[1], files: FOUR_FILES }], recents: [] },
  });

  // A progress report is not an ending and changes no folder's count: the list
  // must not be re-read once per tick for the length of a run.
  const beforeProgress = treeReads();
  channelOf('start_walk_job')(progressEvent());
  await tick();
  expect(treeReads()).toBe(beforeProgress);
  expect(visible(screen.getByTestId('folder-row-4'))).toContain('Проіндексовано: 0 документів');

  channelOf('start_walk_job')(endedEvent());

  // What a person reads on the row after their own scan finishes.
  await waitFor(() =>
    expect(visible(screen.getByTestId('folder-row-4'))).toContain('Проіндексовано: 4 документи'),
  );
  expect(visible(screen.getByTestId('folder-row-4'))).not.toContain('Проіндексовано: 0 документів');
  // The other row was not touched by this walk and still reads its own count —
  // a re-read, not a number written onto whichever row was pressed.
  expect(visible(screen.getByTestId('folder-row-1'))).toContain('Проіндексовано: 0 документів');
});

// The second half of "every ending, not only a walk's". The walk chains the
// embedding pass, so an ENDING arrives that carries no root at all; the list is
// read again on that one too. This is the assertion that dies if the re-read is
// narrowed to `pass === 'walk'`.
test('an embedding pass ending re-reads the list as well', async () => {
  await openFolders();
  await fireEvent.click(scanButton(4));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(calls('start_embed_job')).toHaveLength(1));
  const afterWalk = treeReads();

  channelOf('start_embed_job')(endedEvent());
  await waitFor(() => expect(treeReads()).toBe(afterWalk + 1));
});

// ---------------------------------------------------------------------------
// Live run, finding 3 — a section says it is not built, and shows the thing it
// is for.
//
// Standing on Індексація with a finished scan on screen, a person read «Ця
// секція ще не готова.» and, directly under it, the folder read in full, four
// documents added, embedding finished. Both halves were true; together they
// were a contradiction. Every test in this file was green, because none of them
// had ever rendered the strip while standing on an unbuilt section, and none
// read the window's text IN ORDER.
// ---------------------------------------------------------------------------

async function reportOnScreen() {
  const rendered = await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));
  channelOf('start_walk_job')(endedEvent());
  await waitFor(() => expect(screen.getByTestId('indexing-walk-outcome')).toBeTruthy());
  return rendered;
}

test.each(['indexing', 'application'])(
  'standing on the unbuilt %s section, the report is not read as that section`s content',
  async (id) => {
    const { container } = await reportOnScreen();
    await fireEvent.click(screen.getByTestId(`settings-nav-${id}`));
    await tick();

    const text = visible(container);
    // Not vacuous: the report is still on the window. A fix that hid the strip
    // on an unbuilt section would satisfy the ordering below and lose the
    // counters and the Cancel button the ruling exists to keep.
    expect(text).toContain('Теку прочитано повністю.');
    expect(text).toContain('Додано документів: 5. Без змін: 1. Пропущено: 5. Вилучено з індексу: 4.');
    // The sentence a person reads LAST. On the screen the live run found, the
    // whole report followed it; here nothing does, so the sentence is about the
    // panel it sits in and nothing else.
    // Compared as the tail STRING rather than `endsWith(...) === true`: a
    // boolean fails as "expected false to be true" and hides what a person is
    // actually reading after the sentence, which is the whole finding.
    const notReady = 'Ця секція ще не готова.';
    expect(text.slice(-notReady.length)).toBe(notReady);
    // Stated positively as well as by position: the report is above the
    // sentence, not under it.
    expect(text.indexOf('Теку прочитано повністю.'))
      .toBeLessThan(text.indexOf('Ця секція ще не готова.'));
  },
);

// The ruling the fix above must not break, on the path that now runs THROUGH an
// unbuilt section: the strip is outside every section, so the job it is
// reporting on survives being navigated away from — counters and all — and
// `cancel_job` needs no channel, so Cancel works from a section that has none.
test('Cancel survives a switch through an unbuilt section, in both directions', async () => {
  await openFolders();
  await fireEvent.click(scanButton(1));
  await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));
  channelOf('start_walk_job')(progressEvent());
  await tick();

  await fireEvent.click(screen.getByTestId('settings-nav-indexing'));
  await tick();
  // Still running, still counted, still stoppable — from a section that has no
  // channel of its own and no content at all.
  expect(screen.getByTestId('indexing-counts').textContent)
    .toBe('Опрацьовано 3 з 8. Пропущено: 1. Відхилено: 0.');
  await fireEvent.click(screen.getByTestId('indexing-cancel'));
  expect(calls('cancel_job')).toHaveLength(1);

  // And back again: the job is not lost by the return trip either.
  await fireEvent.click(screen.getByTestId('settings-nav-folders'));
  await screen.findByTestId('folder-row-4');
  expect(screen.getByTestId('indexing-counts').textContent)
    .toBe('Опрацьовано 3 з 8. Пропущено: 1. Відхилено: 0.');
  await fireEvent.click(screen.getByTestId('indexing-cancel'));
  expect(calls('cancel_job')).toHaveLength(2);

  channelOf('start_walk_job')(endedEvent({ reason: 'cancelled' }));
  await waitFor(() => expect(screen.getByTestId('indexing-walk-outcome')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-walk-outcome')))
    .toBe('Сканування зупинено на ваше прохання.');
});

// The other direction of the same control, on the same path: with nothing
// running, an unbuilt section offers no Cancel and calls nothing.
test('on an unbuilt section with no job running there is no Cancel and no strip', async () => {
  render(Settings);
  await fireEvent.click(screen.getByTestId('settings-nav-indexing'));
  await tick();

  expect(screen.queryByTestId('indexing-cancel')).toBeNull();
  expect(screen.queryByTestId('indexing')).toBeNull();
  expect(calls('cancel_job')).toHaveLength(0);
  expect(screen.getByText('Ця секція ще не готова.')).toBeTruthy();
});

// ---------------------------------------------------------------------------
// PR 8a, Task 6 — the two sentences that enumerate what an incomplete walk
// leaves behind, and the case both of them used to omit.
//
// 🔴 A path under a frozen prefix is never deleted (`should_delete`,
// `walk.rs:767`), and that rule does not ask WHY the path stopped being seen.
// A file the person deleted and a file a rule now excludes are the same
// absence to phase 3 — so both survive, both stay searchable, and both go to
// the provider on a later pass. Naming only deletion is an enumeration that
// leaves out the one case PR 8 is entirely about.
//
// Asserted through the rendered screen in BOTH locales, not by reading the
// catalog: a key changed in one locale and left behind in the other is exactly
// what a catalog-reading test cannot see.
// ---------------------------------------------------------------------------

const PARTLY_READ_NAMES_EXCLUSIONS = {
  uk: 'Теку прочитано лише частково: до якихось підтек не вдалося зайти. Нічого в цій теці не звіряли з індексом, тож і видалені файли, і файли під вашими правилами виключення досі знаходяться пошуком — не лише всередині тих підтек.',
  en: 'The folder was only partly read: some subfolders could not be entered. Nothing in this folder was checked against the index, so both deleted files and files your exclusion rules now cover are still found by search — not only inside those subfolders.',
} as const;

const FROZEN_NAMES_EXCLUSIONS = {
  uk: 'Ці підтеки не звіряли, тож і видалені файли, і файли під вашими правилами виключення досі знаходяться пошуком:',
  en: 'These subfolders were not reconciled, so both deleted files and files your exclusion rules now cover are still found by search inside them:',
} as const;

for (const loc of ['uk', 'en'] as const) {
  test(`a partly read folder names exclusions, not deletions alone (${loc})`, async () => {
    await openFolders(loc);
    await fireEvent.click(scanButton(1));
    await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

    channelOf('start_walk_job')(endedEvent({ reason: 'completed', complete: false, ...READ_NOT_RECONCILED }));
    await waitFor(() => expect(screen.getByTestId('indexing-walk-outcome')).toBeTruthy());

    expect(visible(screen.getByTestId('indexing-walk-outcome'))).toBe(PARTLY_READ_NAMES_EXCLUSIONS[loc]);
  });

  test(`the frozen-subtree heading names exclusions, not deletions alone (${loc})`, async () => {
    const { container } = await openFolders(loc);
    await fireEvent.click(scanButton(1));
    await waitFor(() => expect(calls('start_walk_job')).toHaveLength(1));

    // 🔴 `complete: true`, and it is not cosmetic (review round 1, M3).
    // `report.frozen` is assigned at `walk.rs:747`, past the
    // `if !walked.complete || !stopped_cleanly { return }` gate at
    // `walk.rs:511`, so `complete: false` ALWAYS carries `frozen: []` and the
    // pair this fixture used to send is a screen the backend cannot draw. The
    // heading renders off `frozen.length > 0` alone, so the assertion is the
    // same one — now made about a state that happens. Measured shape:
    // `complete=true removed=0 frozen=[Frozen{prefix:"linked",…}]`.
    channelOf('start_walk_job')(endedEvent({ complete: true, removed: 0, frozen: [...FROZEN] }));
    await waitFor(() => expect(screen.getByTestId('indexing-frozen')).toBeTruthy());

    expect(container.textContent ?? '').toContain(FROZEN_NAMES_EXCLUSIONS[loc]);
  });
}

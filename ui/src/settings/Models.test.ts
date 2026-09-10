import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import Models from './Models.svelte';
import { createJobController } from './jobs';
import { tick } from 'svelte';
import { get } from 'svelte/store';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setLocale } from '../i18n';
import { camelOf, rustEnumVariants } from '../lib/rust-enum';
import type {
  ModelSettings, Catalogue, ModelEntry, ModelRefusal, RecordId, UnreadableRecord, ModelRole,
  ScanState,
} from '../lib/ipc';

// Mocked in the shape Arms.test.ts:5-6 already uses — the typed wrappers, not
// the raw `invoke`.
const modelSettings = vi.fn();
const setKey = vi.fn();
const forgetKey = vi.fn();
const providerModels = vi.fn();
const setChatModel = vi.fn();
const setEmbeddingModel = vi.fn();
const startScanJob = vi.fn();
const cancelJob = vi.fn();
const jobStatus = vi.fn();
const listenScanProgress = vi.fn();
const unlisten = vi.fn();
// The `scan-progress` handler the mounted controller registered, so a test can
// deliver the states a real scan would.
let deliver: ((state: ScanState) => void) | null = null;
vi.mock('../lib/ipc', () => ({
  modelSettings: (...a: unknown[]) => modelSettings(...a),
  setKey: (...a: unknown[]) => setKey(...a),
  forgetKey: (...a: unknown[]) => forgetKey(...a),
  providerModels: (...a: unknown[]) => providerModels(...a),
  setChatModel: (...a: unknown[]) => setChatModel(...a),
  setEmbeddingModel: (...a: unknown[]) => setEmbeddingModel(...a),
  // The section starts its recovery pass through the shared controller
  // (`jobs.ts`), which imports these from the same module — a mock that leaves
  // them out hands the controller `undefined` and every call becomes a
  // TypeError swallowed by a catch.
  startScanJob: (...a: unknown[]) => startScanJob(...a),
  cancelJob: (...a: unknown[]) => cancelJob(...a),
  jobStatus: (...a: unknown[]) => jobStatus(...a),
  listenScanProgress: (...a: unknown[]) => listenScanProgress(...a),
}));

// Every state is newer than the one before it, because that is the only thing
// the controller compares (`apply`).
let revision = 0;
const IDLE_SCAN: ScanState = {
  revision: 0, files: 0, readSeq: 0, jobsDone: 0, lastReading: null, snapshot: { kind: 'idle' },
};
const readingScan = (): ScanState => ({
  ...IDLE_SCAN,
  revision: (revision += 1),
  snapshot: {
    kind: 'running',
    cancellable: true,
    phase: {
      kind: 'reading', rootIndex: 0, rootCount: 1, rootPath: '/home/a/notes',
      counts: { done: 1, total: 4, skipped: 0, refused: 0, contended: 0, secondsLeft: null },
    },
  },
});

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
const endedScan = (): ScanState => ({
  ...IDLE_SCAN,
  revision: (revision += 1),
  snapshot: {
    kind: 'ended',
    report: {
      embedding: { kind: 'ran', done: 4, total: 4, refused: 0 },
      endedIn: 'embedding', reason: 'completed', message: null, resume: null,
    },
  },
});

// Delivers one state the way the core's own observer does.
function emit(state: ScanState) {
  if (deliver === null) throw new Error('nothing is listening to scan-progress');
  deliver(state);
}

// An empty-but-well-formed catalogue — every test that does not care about
// the model tabs gets one for free, so Task 4's fixtures do not have to learn
// about Task 5's fetch just to keep mounting.
function emptyCatalogue(): Catalogue {
  return { entries: [], unreadable: 0, unreadableRecords: [] };
}

beforeEach(() => {
  modelSettings.mockReset();
  setKey.mockReset();
  forgetKey.mockReset();
  providerModels.mockReset();
  setChatModel.mockReset();
  setEmbeddingModel.mockReset();
  startScanJob.mockReset();
  listenScanProgress.mockReset();
  unlisten.mockReset();
  deliver = null;
  revision = 0;
  listenScanProgress.mockImplementation((cb: (state: ScanState) => void) => {
    deliver = cb;
    return Promise.resolve(unlisten);
  });
  cancelJob.mockReset();
  jobStatus.mockReset();
  providerModels.mockResolvedValue(emptyCatalogue());
  startScanJob.mockResolvedValue(undefined);
  cancelJob.mockResolvedValue(undefined);
  jobStatus.mockResolvedValue(IDLE_SCAN);
});
afterEach(() => {
  cleanup();
  setLocale('en');
});

// One base fixture, overridden per test — the three axes (key, index,
// platform) are independent, so a test that needs one changed does not have
// to restate the other two.
function settings(overrides: Partial<ModelSettings> = {}): ModelSettings {
  return {
    key: { kind: 'absent' },
    // `totalChunks: 6` and no model chosen: this index HOLDS documents. The two
    // conjuncts of the degraded rule are then distinguishable on the base
    // fixture — an index with nothing in it would satisfy the rule's absence
    // for the wrong reason, and no test could tell which half was read.
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 6, embeddingModel: null, searchTextArm: true, searchContentArm: false },
    platform: 'linux',
    ...overrides,
  };
}

// `Models` takes the window's job controller as a prop now, so every render in
// this file has to hand it one. A fresh controller per render, deliberately:
// these tests are about the section, and one shared across them would carry a
// finished pass from a previous test into the next one's first assertion.
// 🔴 Mounted, not merely created: `Settings.svelte` is what opens the window's
// subscription, and until it is opened nothing is listening for the states
// these tests deliver.
function renderModels() {
  const jobs = createJobController();
  jobs.mount();
  return render(Models, { props: { jobs } });
}

async function renderWith(s: ModelSettings) {
  setLocale('en'); // seed, do not inherit — the shape every Settings.test.ts test already uses
  modelSettings.mockResolvedValue(s);
  const result = renderModels();
  // The section fetches on mount; every test needs the settled DOM before
  // asserting on it.
  await waitFor(() => expect(modelSettings).toHaveBeenCalled());
  await Promise.resolve();
  await Promise.resolve();
  return result;
}

// ---------------------------------------------------------------------------
// Claim 0: the index Unreadable branch, and only that branch. `Unreadable`
// carries no `IndexRead` at all — the fixture question this task opens with —
// so a test that never gives the type an IndexRead field cannot pass by
// accident on a code path that quietly reads one.
// ---------------------------------------------------------------------------

test('index Unreadable/notOpen renders one sentence, never the reason', async () => {
  await renderWith(settings({
    index: { kind: 'unreadable', cause: 'notOpen', reason: 'LEAK-TOKEN-NOT-OPEN' },
  }));
  expect(screen.getByText('The index is not open yet.')).toBeTruthy();
  expect(screen.queryByText(/LEAK-TOKEN-NOT-OPEN/)).toBeNull();
});

test('index Unreadable/readFailed renders its own, different sentence, never the reason', async () => {
  await renderWith(settings({
    index: { kind: 'unreadable', cause: 'readFailed', reason: 'LEAK-TOKEN-READ-FAILED' },
  }));
  expect(screen.getByText('The index could not be read — this is a defect in this build.')).toBeTruthy();
  expect(screen.queryByText(/LEAK-TOKEN-READ-FAILED/)).toBeNull();
});

test('index Read renders no failure sentence at all', async () => {
  await renderWith(settings({
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'text-embedding-3-small', searchTextArm: true, searchContentArm: true },
  }));
  expect(screen.queryByText('The index is not open yet.')).toBeNull();
  expect(screen.queryByText('The index could not be read — this is a defect in this build.')).toBeNull();
});

// ---------------------------------------------------------------------------
// Claim 1: Absent shows an empty key field, an add-a-key affordance and a
// sentence saying what the key is for; Present states that a key is saved and
// shows the two buttons, and no key characters — there is none to show
// (models.rs:150-162).
// ---------------------------------------------------------------------------

test('key Absent: an empty field to add a key, no Change/Forget', async () => {
  await renderWith(settings({ key: { kind: 'absent' } }));
  const input = screen.getByLabelText('Key:') as HTMLInputElement;
  expect(input.value).toBe('');
  expect(screen.getByRole('button', { name: 'Save' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Change' })).toBeNull();
  expect(screen.queryByRole('button', { name: 'Forget' })).toBeNull();
});

// Review P2-4: `getByLabelText('Key:')` LOCATES the field and asserts nothing
// about it — a locator is not an assertion. `type="password"` → `type="text"`
// left the whole suite green while the one field in this product that holds a
// secret rendered its characters on screen. Asserted here in both states that
// render it, and positively: the attribute equals "password", not "is not
// text".
test('the key field is a password field when adding a key', async () => {
  await renderWith(settings({ key: { kind: 'absent' } }));
  expect(screen.getByLabelText('Key:').getAttribute('type')).toBe('password');
});

test('the key field is a password field when changing an existing key', async () => {
  await renderWith(settings({ key: { kind: 'present' } }));
  await fireEvent.click(screen.getByRole('button', { name: 'Change' }));
  expect(screen.getByLabelText('Key:').getAttribute('type')).toBe('password');
});

// Review P3-10: Absent used to render `Provider OpenRouter Key [field] Save` —
// nothing saying a key is needed, what it is for, or where it comes from.
test('key Absent says what the key is for and where it comes from', async () => {
  await renderWith(settings({ key: { kind: 'absent' } }));
  expect(screen.getByTestId('model-key-absent-hint').textContent).toBe(
    'An OpenRouter key lets this application reach the models. Create one in your OpenRouter account and paste it here.',
  );
});

test('key Present: a saved-key statement, Change and Forget, no editable field', async () => {
  await renderWith(settings({ key: { kind: 'present' } }));
  // Owner's ruling, 2026-08-28: a word, not a run of dots. The reply carries no
  // key (models.rs:150-162), so a fixed mask would state a length this window
  // cannot know — and a screen reader would announce eight bullets in a row.
  expect(screen.getByTestId('model-key-saved').textContent).toBe('A key is saved.');
  expect(screen.getByRole('button', { name: 'Change' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Forget' })).toBeTruthy();
  expect(screen.queryByLabelText('Key:')).toBeNull();
});

// Review P3-9: the provider is fixed by design (§4.4) and nothing pinned it —
// removing `disabled` left the suite green. Asserted positively, together with
// the name a person actually reads.
test('the provider control is fixed: disabled, and reading as a provider name', async () => {
  await renderWith(settings());
  const provider = screen.getByLabelText('Provider:') as HTMLSelectElement;
  expect(provider.disabled).toBe(true);
  expect(provider.textContent).toContain('OpenRouter');
});

// ---------------------------------------------------------------------------
// Claim 2: three of the four Unreadable causes name the action their own doc
// names; the fourth (Refused) says there is none. No two show the same text,
// and `reason` never reaches the screen.
// ---------------------------------------------------------------------------

type KeyCause = 'locked' | 'duplicate' | 'refused' | 'defect';
const KEY_CAUSES: readonly KeyCause[] = ['locked', 'duplicate', 'refused', 'defect'];

const KEY_FAILURE_SENTENCES: Record<KeyCause, string> = {
  // Review P1-2: the shipped sentence described a state ("it may be locked, or
  // a permission prompt was declined") on a screen that renders no buttons and
  // no inputs at all — the person was told nothing to do and had nothing to
  // press. It now names the action, and still claims neither situation:
  // models.rs:723-737 records that this build cannot tell them apart.
  locked: 'The credential store did not answer. Unlock it, or allow access when the system asks for it, then open this window again.',
  duplicate: 'More than one credential is filed under this installation. Remove the duplicate in the system credential store.',
  refused: 'The credential store refused to answer. This build cannot tell what to do next.',
  defect: 'This is a defect in this build, not a state of your system. Please report it to the developers.',
};

for (const cause of KEY_CAUSES) {
  test(`key Unreadable/${cause} shows its own sentence and never the reason`, async () => {
    await renderWith(settings({
      key: { kind: 'unreadable', cause, reason: `LEAK-TOKEN-${cause.toUpperCase()}` },
    }));
    expect(screen.getByText(KEY_FAILURE_SENTENCES[cause])).toBeTruthy();
    expect(screen.queryByText(new RegExp(`LEAK-TOKEN-${cause.toUpperCase()}`))).toBeNull();
  });
}

// Review P2-5: the old form of this test read `KEY_FAILURE_SENTENCES` — a
// literal declared twelve lines above it, in this same file — and compared it
// with itself. It never rendered anything, so it could only fail if somebody
// edited that literal, and the Ukrainian half of the catalogue collapsing to a
// single word left it green. It now collects the sentences from the RENDERED
// section, in both locales.
async function renderedKeyFailure(cause: KeyCause, loc: 'uk' | 'en'): Promise<string> {
  setLocale(loc);
  modelSettings.mockResolvedValue(settings({
    key: { kind: 'unreadable', cause, reason: `LEAK-TOKEN-${cause.toUpperCase()}` },
  }));
  renderModels();
  const sentence = await screen.findByTestId('model-key-failure');
  const text = sentence.textContent ?? '';
  cleanup();
  return text;
}

for (const loc of ['en', 'uk'] as const) {
  test(`no two of the four Unreadable causes render the same sentence (${loc})`, async () => {
    const texts: string[] = [];
    for (const cause of KEY_CAUSES) texts.push(await renderedKeyFailure(cause, loc));
    // Both directions: four sentences, all non-empty, all different. A
    // catalogue collapsed to one word satisfies neither half.
    expect(texts.length).toBe(4);
    for (const text of texts) expect(text.length).toBeGreaterThan(20);
    expect(new Set(texts).size).toBe(4);
  });
}

// ---------------------------------------------------------------------------
// Claim 3 (the mac keychain note) is gone along with the note itself (Task 4):
// `models_mac_keychain_note` is removed from the catalogue and the markup, so
// there is nothing left here for a platform to gate.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Claim 4: Forget calls forget_key and re-reads model_settings; Removed and
// NothingToRemove say different things.
//
// Task 9 (owner's ruling, live run 2026-09-10): the press no longer calls
// `forget_key` itself — it opens an inline confirmation, and only the
// confirmation's own button does. Both tests below now go through it.
// ---------------------------------------------------------------------------

test('forget_asks_before_calling_forget_key', async () => {
  setLocale('en');
  modelSettings.mockResolvedValue(settings({ key: { kind: 'present' } }));

  renderModels();
  await waitFor(() => expect(screen.getByRole('button', { name: 'Forget' })).toBeTruthy());
  const forgetButton = screen.getByRole('button', { name: 'Forget' });

  await fireEvent.click(forgetButton);

  expect(forgetKey).not.toHaveBeenCalled();
  expect(screen.getByTestId('model-key-forget-confirm')).toBeTruthy();
  expect(screen.getByText(/Forget the saved key\?/)).toBeTruthy();

  await fireEvent.click(screen.getByTestId('model-key-forget-cancel'));

  expect(forgetKey).not.toHaveBeenCalled();
  expect(screen.queryByTestId('model-key-forget-confirm')).toBeNull();
  // Cancel returns focus to the control that opened the question.
  expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Forget' }));
});

// "Ask what disappears" (CLAUDE.md): the question is about the key group,
// which stands regardless of which tab is open — Folders' own `removeQuestion`
// closes on the existing per-tab rule `pendingEmbedding` already follows, and
// this must too, or a Forget question opened under Embedding would still be
// standing, and answerable, under Chat.
test('a tab switch closes the open Forget question, without calling forget_key', async () => {
  setLocale('en');
  modelSettings.mockResolvedValue(settings({ key: { kind: 'present' } }));

  renderModels();
  await waitFor(() => expect(screen.getByRole('button', { name: 'Forget' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Forget' }));
  expect(screen.getByTestId('model-key-forget-confirm')).toBeTruthy();

  await fireEvent.click(screen.getByTestId('model-tab-chat'));

  expect(screen.queryByTestId('model-key-forget-confirm')).toBeNull();
  expect(forgetKey).not.toHaveBeenCalled();
});

test('Forget calls forget_key, re-reads model_settings, and Removed says so', async () => {
  setLocale('en');
  modelSettings
    .mockResolvedValueOnce(settings({ key: { kind: 'present' } }))
    .mockResolvedValueOnce(settings({ key: { kind: 'absent' } }));
  forgetKey.mockResolvedValue({ kind: 'removed' });

  renderModels();
  await waitFor(() => expect(screen.getByRole('button', { name: 'Forget' })).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Forget' }));
  await fireEvent.click(screen.getByTestId('model-key-forget-confirm'));

  await waitFor(() => expect(forgetKey).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(modelSettings).toHaveBeenCalledTimes(2)); // mount + the re-read Forget triggers
  await waitFor(() => expect(screen.getByText('The key was removed.')).toBeTruthy());
  // Confirm moves focus onto the key group's first control — the Absent
  // branch's own field, since the re-read above just confirmed the key gone.
  await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText('Key:')));
});

test('Forget calls forget_key, re-reads model_settings, and NothingToRemove says a different thing', async () => {
  setLocale('en');
  modelSettings
    .mockResolvedValueOnce(settings({ key: { kind: 'present' } }))
    .mockResolvedValueOnce(settings({ key: { kind: 'absent' } }));
  forgetKey.mockResolvedValue({ kind: 'nothingToRemove' });

  renderModels();
  await waitFor(() => expect(screen.getByRole('button', { name: 'Forget' })).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Forget' }));
  await fireEvent.click(screen.getByTestId('model-key-forget-confirm'));

  await waitFor(() => expect(forgetKey).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(screen.getByText('There was no key to remove.')).toBeTruthy());
  expect(screen.queryByText('The key was removed.')).toBeNull();
});

// ---------------------------------------------------------------------------
// Claim 5 / Step 5: entering a key calls set_key with it, and after the round
// completes — set_key, then the re-read model_settings — no rendered text and
// no component state contains the entered key. A distinctive fixture value
// makes a leak unmistakable.
// ---------------------------------------------------------------------------

const LEAKY_KEY = 'sk-or-DO-NOT-LEAK-9f2a71';

test('entering a key calls set_key, and no trace of it survives the round', async () => {
  setLocale('en');
  modelSettings
    .mockResolvedValueOnce(settings({ key: { kind: 'absent' } }))
    .mockResolvedValueOnce(settings({ key: { kind: 'present' } }));
  setKey.mockResolvedValue({ balance: { kind: 'notStated' } });

  const { container } = renderModels();
  await waitFor(() => expect(screen.getByLabelText('Key:')).toBeTruthy());

  await fireEvent.input(screen.getByLabelText('Key:'), { target: { value: LEAKY_KEY } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save' }));

  await waitFor(() => expect(setKey).toHaveBeenCalledWith(LEAKY_KEY));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Change' })).toBeTruthy());

  // No rendered text contains it …
  expect(container.innerHTML).not.toContain(LEAKY_KEY);

  // … and no component state does either: reopening the editor must show an
  // empty field, not the value that was just sent.
  await fireEvent.click(screen.getByRole('button', { name: 'Change' }));
  const reopened = screen.getByLabelText('Key:') as HTMLInputElement;
  expect(reopened.value).toBe('');
});

// ---------------------------------------------------------------------------
// Step 6: read the whole rendered section, as a person, in both directions —
// the everything-green state and the everything-red state.
// ---------------------------------------------------------------------------

test('reads as a person: everything configured, nothing alarming shown', async () => {
  const { container } = await renderWith(settings({
    key: { kind: 'present' },
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'text-embedding-3-small', searchTextArm: true, searchContentArm: true },
    platform: 'linux',
  }));
  const text = container.textContent ?? '';
  expect(text).toContain('OpenRouter');
  expect(text).toContain('A key is saved.');
  expect(text).toContain('Change');
  expect(text).toContain('Forget');
  expect(text).not.toContain('defect');
  expect(text).not.toContain('not open');
});

test('reads as a person: a locked keychain names the situation, not a status code', async () => {
  const { container } = await renderWith(settings({
    key: { kind: 'unreadable', cause: 'locked', reason: 'errSecInteractionNotAllowed -25308' },
    platform: 'mac',
  }));
  const text = container.textContent ?? '';
  expect(text).toContain(KEY_FAILURE_SENTENCES.locked);
  expect(text).not.toContain('errSecInteractionNotAllowed');
  expect(text).not.toContain('-25308');
});

// ---------------------------------------------------------------------------
// Review P1-1: the Ukrainian half of this section was defended by nothing.
// No test rendered `Models` in Ukrainian and none switched locale after mount,
// so two mutants ran green through the whole suite: collapsing all four
// Ukrainian `models_key_*` sentences to one word, and stripping every
// `void $locale` guard from the component. The second one matters because
// `t()` reads `get(locale)` non-reactively (i18n/index.ts:11): without the
// guard a `$derived` never re-runs, and after a language switch the section
// stays in the language it was mounted in.
//
// The switch tests below read the section under 'en' BEFORE switching, on
// purpose (the shape Settings.test.ts:137-140 uses): a `$derived` that has
// never been read has nothing stale to return, so a mutant survives a test
// that only reads after the switch.
// ---------------------------------------------------------------------------

const UK = {
  provider: 'Провайдер:',
  keyLabel: 'Ключ:',
  saved: 'Ключ збережено.',
  absentHint: 'Ключ OpenRouter потрібен, щоб застосунок міг звертатися до моделей. Створіть його в обліковому записі OpenRouter і вставте сюди.',
  change: 'Змінити',
  forget: 'Забути',
  save: 'Зберегти',
  cancel: 'Скасувати',
  removed: 'Ключ видалено.',
  indexNotOpen: 'Індекс ще не відкрито.',
  loadFailed: 'Не вдалося прочитати налаштування моделей.',
} as const;

const UK_KEY_FAILURE_SENTENCES: Record<KeyCause, string> = {
  locked: 'Сховище ключів не відповіло. Розблокуйте його або дозвольте доступ, коли система про це запитає, і відкрийте це вікно знову.',
  duplicate: 'Під іменем цієї інсталяції збережено кілька ключів. Видаліть зайвий у системному сховищі.',
  refused: 'Сховище ключів відповіло відмовою. Ця збірка не може визначити, що робити далі.',
  defect: 'Це вада цієї збірки, а не стан вашої системи. Повідомте про неї розробникам.',
};

async function renderInUk(s: ModelSettings) {
  setLocale('uk');
  modelSettings.mockResolvedValue(s);
  const result = renderModels();
  await waitFor(() => expect(modelSettings).toHaveBeenCalled());
  await Promise.resolve();
  await Promise.resolve();
  return result;
}

async function switchTo(loc: 'uk' | 'en') {
  setLocale(loc);
  await tick();
  await Promise.resolve();
}

for (const cause of KEY_CAUSES) {
  test(`key Unreadable/${cause} shows its Ukrainian sentence, and never the reason`, async () => {
    await renderInUk(settings({
      key: { kind: 'unreadable', cause, reason: `LEAK-TOKEN-${cause.toUpperCase()}` },
    }));
    expect(screen.getByTestId('model-key-failure').textContent).toBe(UK_KEY_FAILURE_SENTENCES[cause]);
    expect(screen.queryByText(new RegExp(`LEAK-TOKEN-${cause.toUpperCase()}`))).toBeNull();
  });
}

test('mounted in Ukrainian, the index sentence is Ukrainian too', async () => {
  const { container } = await renderInUk(settings({
    key: { kind: 'present' },
    index: { kind: 'unreadable', cause: 'notOpen', reason: 'LEAK-TOKEN-UK' },
  }));
  const text = container.textContent ?? '';
  expect(text).toContain(UK.indexNotOpen);
  expect(text).toContain(UK.provider);
  expect(text).toContain(UK.saved);
  expect(text).not.toContain('The index is not open yet.');
  expect(text).not.toContain('LEAK-TOKEN-UK');
});

test('a language switch after mount reaches the provider row and the saved-key line', async () => {
  const { container } = await renderWith(settings({
    key: { kind: 'present' },
    index: { kind: 'unreadable', cause: 'notOpen', reason: 'r' },
  }));
  // Read every one of them under 'en' first — see the note above.
  const before = container.textContent ?? '';
  expect(before).toContain('Provider');
  expect(before).toContain('A key is saved.');
  expect(before).toContain('Change');
  expect(before).toContain('Forget');
  expect(before).toContain('The index is not open yet.');

  await switchTo('uk');

  const after = container.textContent ?? '';
  expect(after).toContain(UK.provider);
  expect(after).toContain(UK.saved);
  expect(after).toContain(UK.change);
  expect(after).toContain(UK.forget);
  expect(after).toContain(UK.indexNotOpen);
  // The provider NAME is the one string that is deliberately the same in both
  // locales — a brand, not a translation — so it is asserted to survive the
  // switch rather than to change with it.
  expect(after).toContain('OpenRouter');
  expect(after).not.toContain('A key is saved.');
});

test('a language switch after mount reaches the add-a-key hint and the Save control', async () => {
  const { container } = await renderWith(settings({ key: { kind: 'absent' } }));
  const before = container.textContent ?? '';
  expect(before).toContain('Key');
  expect(before).toContain('Save');
  expect(before).toContain('An OpenRouter key lets this application reach the models.');

  await switchTo('uk');

  const after = container.textContent ?? '';
  // The field's own label, not just the word somewhere in the section: 'Ключ'
  // also opens the saved-key line and the hint, so a text search would be
  // satisfied by a label that stayed English.
  expect(screen.getByLabelText(UK.keyLabel)).toBeTruthy();
  expect(screen.queryByLabelText('Key:')).toBeNull();
  expect(after).toContain(UK.save);
  expect(after).toContain(UK.absentHint);
  expect(after).not.toContain('An OpenRouter key lets this application reach the models.');
});

test('a language switch after mount reaches the Cancel control of the open editor', async () => {
  const { container } = await renderWith(settings({ key: { kind: 'present' } }));
  await fireEvent.click(screen.getByRole('button', { name: 'Change' }));
  expect((container.textContent ?? '')).toContain('Cancel');

  await switchTo('uk');

  const after = container.textContent ?? '';
  expect(after).toContain(UK.cancel);
  expect(after).not.toContain('Cancel');
});

test('a language switch after mount reaches the Unreadable sentence', async () => {
  const { container } = await renderWith(settings({
    key: { kind: 'unreadable', cause: 'locked', reason: 'LEAK-TOKEN-SWITCH' },
  }));
  expect((container.textContent ?? '')).toContain(KEY_FAILURE_SENTENCES.locked);

  await switchTo('uk');

  const after = container.textContent ?? '';
  expect(after).toContain(UK_KEY_FAILURE_SENTENCES.locked);
  expect(after).not.toContain(KEY_FAILURE_SENTENCES.locked);
  expect(after).not.toContain('LEAK-TOKEN-SWITCH');
});

test('a language switch after mount reaches the removal sentence', async () => {
  setLocale('en');
  modelSettings
    .mockResolvedValueOnce(settings({ key: { kind: 'present' } }))
    .mockResolvedValueOnce(settings({ key: { kind: 'absent' } }));
  forgetKey.mockResolvedValue({ kind: 'removed' });

  const { container } = renderModels();
  await waitFor(() => expect(screen.getByRole('button', { name: 'Forget' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Forget' }));
  await fireEvent.click(screen.getByTestId('model-key-forget-confirm'));
  await waitFor(() => expect(screen.getByText('The key was removed.')).toBeTruthy());

  await switchTo('uk');

  const after = container.textContent ?? '';
  expect(after).toContain(UK.removed);
  expect(after).not.toContain('The key was removed.');
});

// ---------------------------------------------------------------------------
// Review P2-3: a rejected fetch on mount left the panel permanently blank —
// TEXT: "", HTML: "<!---->". The failure went to `console.error`, and every
// visible thing, the error paragraph included, was gated on `{#if settings}`,
// so the one paragraph that could have said something was inside the block
// that a failed mount never renders. Bounded but real: `model_settings`
// returns `ModelSettings` and not `Result` (models.rs:1131), so what fails
// here is the IPC layer, not the command.
// ---------------------------------------------------------------------------

test('a rejected mount fetch leaves a sentence on screen, not an empty panel', async () => {
  setLocale('en');
  modelSettings.mockRejectedValue(new Error('the settings window could not reach the core'));

  const { container } = renderModels();

  await waitFor(() => expect(screen.getByTestId('model-load-failure')).toBeTruthy());
  const text = container.textContent ?? '';
  expect(text).toContain('The model settings could not be read.');
  // §10: the rejection's own sentence, verbatim and never branched on.
  expect(text).toContain('the settings window could not reach the core');
  // And the panel is not the empty one this defect produced.
  expect(text.trim().length).toBeGreaterThan(0);
});

test('the mount failure sentence follows a language switch too', async () => {
  setLocale('en');
  modelSettings.mockRejectedValue(new Error('the settings window could not reach the core'));

  const { container } = renderModels();
  await waitFor(() => expect(screen.getByTestId('model-load-failure')).toBeTruthy());
  expect((container.textContent ?? '')).toContain('The model settings could not be read.');

  await switchTo('uk');

  const after = container.textContent ?? '';
  expect(after).toContain(UK.loadFailed);
  expect(after).not.toContain('The model settings could not be read.');
  // The backend's sentence is not translated — it is shown as it arrived.
  expect(after).toContain('the settings window could not reach the core');
});

// `loadError`'s own doc comment claimed it "survives no re-read: nothing on
// this screen can retry it" — true only of the mount read. The re-read the
// section fires when a scan ends (`onMount`'s `jobs.state.subscribe`) used to
// route a rejection through `.catch(() => {})`, dropping it on the floor: the
// mount's own rejection reached the screen, a later one after a scan ended
// did not. Both directions asserted: the sentence appears, and the settings a
// successful mount already rendered are not blanked out from under it — a
// stale panel says less than a fresh one but more than an empty one, and
// `settings` itself is never touched by a failed `refresh()` (only a
// successful one assigns it), so the panel below the sentence is exactly what
// the last good read produced.
test('a rejected re-read after a scan ends leaves the sentence on screen, and the last good settings stay', async () => {
  setLocale('en');
  await renderWith(settings({ key: { kind: 'present' } }));
  // The mount succeeded: something concrete is on screen before the scan ends,
  // so "stay" below is a real claim about a rendered panel, not a vacuous one.
  expect(screen.getByTestId('model-key-saved').textContent).toBe('A key is saved.');

  modelSettings.mockRejectedValue(new Error('the settings window could not reach the core'));
  emit(endedScan());

  await waitFor(() => expect(screen.getByTestId('model-load-failure')).toBeTruthy());
  expect(screen.getByTestId('model-load-reason').textContent)
    .toBe('the settings window could not reach the core');
  // The panel the mount rendered is still there, not replaced by an empty one.
  expect(screen.getByTestId('model-key-saved').textContent).toBe('A key is saved.');
});

// The other direction: a mount that fails, followed by a scan-ended re-read
// that SUCCEEDS. `refresh()` used to write `settings` on success without ever
// clearing `loadError`, so the stale "could not be read" sentence would sit
// forever beside a panel a later read had already confirmed — a claim
// outliving its own guard, the same class `Settings.svelte` already guards
// for its own copy of this state (`Settings.svelte:95-104`, mutation case
// pr9-ui.sh "a read that succeeds must take the failure sentence away with
// it"). Both directions asserted: the sentence is gone, and the panel the
// successful re-read produced is actually on screen, not merely "no crash".
test('a read that succeeds after a failed one takes the failure sentence away', async () => {
  setLocale('en');
  modelSettings.mockRejectedValue(new Error('the settings window could not reach the core'));

  renderModels();
  await waitFor(() => expect(screen.getByTestId('model-load-failure')).toBeTruthy());

  modelSettings.mockResolvedValue(settings({ key: { kind: 'present' } }));
  emit(endedScan());

  await waitFor(() => expect(screen.queryByTestId('model-load-failure')).toBeNull());
  expect(screen.queryByTestId('model-load-reason')).toBeNull();
  expect(screen.getByTestId('model-key-saved').textContent).toBe('A key is saved.');
});

// ---------------------------------------------------------------------------
// Review P3-8: `startEditing` clears `actionError` and `cancelEditing` did
// not, so a failed Save followed by Cancel left the failure sentence beside a
// state that no longer describes it.
// ---------------------------------------------------------------------------

test('Cancel clears the sentence a failed Save left behind', async () => {
  setLocale('en');
  modelSettings.mockResolvedValue(settings({ key: { kind: 'present' } }));
  setKey.mockRejectedValue(new Error('the credential store would not keep the key'));

  renderModels();
  await waitFor(() => expect(screen.getByRole('button', { name: 'Change' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Change' }));
  await fireEvent.input(screen.getByLabelText('Key:'), { target: { value: LEAKY_KEY } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save' }));

  await waitFor(() => expect(screen.getByTestId('model-action-error')).toBeTruthy());
  expect(screen.getByTestId('model-action-error').textContent)
    .toBe('the credential store would not keep the key');

  await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  expect(screen.queryByTestId('model-action-error')).toBeNull();
});

// The key-never-returns claim, on the path that was not covered: a rejection.
// The draft is cleared before the request leaves, so a failed Save must be as
// empty-handed as a successful one — including the editor it leaves open.
test('a rejected Save keeps no trace of the entered key either', async () => {
  setLocale('en');
  modelSettings.mockResolvedValue(settings({ key: { kind: 'absent' } }));
  setKey.mockRejectedValue(new Error('the credential store would not keep the key'));

  const { container } = renderModels();
  await waitFor(() => expect(screen.getByLabelText('Key:')).toBeTruthy());
  await fireEvent.input(screen.getByLabelText('Key:'), { target: { value: LEAKY_KEY } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save' }));

  await waitFor(() => expect(screen.getByTestId('model-action-error')).toBeTruthy());
  expect(container.innerHTML).not.toContain(LEAKY_KEY);
  expect((screen.getByLabelText('Key:') as HTMLInputElement).value).toBe('');
});

// ---------------------------------------------------------------------------
// Task 5 — the two model tabs, their catalogues, and the green-dot rule.
//
// Fixture question, answered first, per the three named traps: (1) a
// catalogue with `unreadable: 0` — every fixture below that never sets it
// exercises this, so a component that always shows the sentence fails
// "unreadable: 0 renders no such sentence"; (2) a catalogue whose entries are
// all selectable — every `entry(...)` below defaults `refusal` to `null`, so
// the refusal-rendering path is only reached by fixtures that deliberately
// set it (the discriminant tests further down); (3) the same model id in
// both roles' catalogues — built once, explicitly, right after the tab
// tests, because a per-role selection written into one shared variable is
// invisible to every other fixture in this file.
// ---------------------------------------------------------------------------

function entry(id: string, overrides: Partial<ModelEntry> = {}): ModelEntry {
  return {
    id, name: id,
    inputLimit: { kind: 'notStated' }, price: { kind: 'notStated' },
    refusal: null,
    ...overrides,
  };
}

// Review P3-12: `unreadable: 3` beside an empty `unreadableRecords` is a state
// the Rust side cannot produce — `catalogue.rs:251-256` documents the vector as
// "one entry per record counted in `unreadable`", and `:581-582` pushes to both
// in the same step. A fixture that builds it tests a screen no provider can
// cause. Omitting the records now synthesises the right number of them; giving
// them explicitly and disagreeing with the count is refused outright.
function catalogueOf(entries: ModelEntry[], unreadable = 0, unreadableRecords?: UnreadableRecord[]): Catalogue {
  if (unreadableRecords === undefined) {
    const synthesised: UnreadableRecord[] = Array.from(
      { length: unreadable },
      (_, index) => ({ id: { kind: 'absent' }, index }),
    );
    return { entries, unreadable, unreadableRecords: synthesised };
  }
  if (unreadableRecords.length !== unreadable) {
    throw new Error(
      `catalogueOf: unreadable=${unreadable} with ${unreadableRecords.length} unreadableRecords — ` +
      'the backend keeps them equal (catalogue.rs:251-256), so this fixture builds a state no ' +
      'provider can cause.',
    );
  }
  return { entries, unreadable, unreadableRecords };
}

function mockCatalogues(byRole: Partial<Record<ModelRole, Catalogue>>) {
  providerModels.mockImplementation((role: ModelRole) => Promise.resolve(byRole[role] ?? emptyCatalogue()));
}

// Task 4 — the catalogue list is now one native `<select>` rather than a row
// of buttons; these small helpers stand in for the `model-entry-*` testids
// the old markup gave each row of its own. `option[value=...]` rather than a
// testid because an `<option>` carries none — the same reason the old list
// gave each row a testid keyed by `entry.id` is why these helpers key on the
// select's own `value` attribute instead.
function modelSelect(): HTMLSelectElement {
  return screen.getByTestId('model-selection') as HTMLSelectElement;
}
function optionsFor(value: string): HTMLOptionElement[] {
  return [...modelSelect().querySelectorAll(`option[value="${value}"]`)] as HTMLOptionElement[];
}
function optionFor(value: string): HTMLOptionElement {
  const [opt] = optionsFor(value);
  if (!opt) throw new Error(`no <option value="${value}"> in the select`);
  return opt;
}
async function pickModel(value: string) {
  await fireEvent.change(modelSelect(), { target: { value } });
}

test('two named tabs; switching changes the select`s own options, both asserted positively', async () => {
  mockCatalogues({
    embedding: catalogueOf([entry('emb-1', { name: 'Embedding One' })]),
    chat: catalogueOf([entry('chat-1', { name: 'Chat One' })]),
  });
  await renderWith(settings());

  // Task 9: the tab button's own textContent now also carries its dot's
  // sr-only state word (`.mdot` moved inside `.mtab` as its last child) — the
  // label itself is asserted with `toContain`, and the state word is exactly
  // `tab_button_names_include_the_configured_state`'s own claim below.
  expect(screen.getByTestId('model-tab-embedding').textContent).toContain('Embedding');
  expect(screen.getByTestId('model-tab-chat').textContent).toContain('Chat');
  await waitFor(() => expect(optionsFor('emb-1').length).toBe(1));
  expect(screen.getByTestId('model-tab-embedding').getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByTestId('model-tab-chat').getAttribute('aria-pressed')).toBe('false');
  expect(optionsFor('chat-1').length).toBe(0);
  // Review P2-11: the fixture gives every entry a name that differs from its
  // id and then the old test threw that away, asserting only testids and
  // `aria-pressed`. `{entry.name}` → `{entry.id}` survived the whole suite —
  // the named class from the last PR, a card showing an identifier where a
  // person came for content. The option reads as the model's name.
  expect(optionFor('emb-1').textContent).toBe('Embedding One');

  await fireEvent.click(screen.getByTestId('model-tab-chat'));

  await waitFor(() => expect(optionsFor('chat-1').length).toBe(1));
  expect(screen.getByTestId('model-tab-chat').getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByTestId('model-tab-embedding').getAttribute('aria-pressed')).toBe('false');
  expect(optionsFor('emb-1').length).toBe(0);
  // The chat catalogue is a different tab's own read — it needs its own
  // assertion.
  expect(optionFor('chat-1').textContent).toBe('Chat One');
});

test('the same model id in both catalogues does not leak a selection across tabs', async () => {
  mockCatalogues({
    embedding: catalogueOf([entry('shared-model', { name: 'Shared' })]),
    chat: catalogueOf([entry('shared-model', { name: 'Shared' }), entry('other-model', { name: 'Other' })]),
  });
  await renderWith(settings({
    key: { kind: 'present' },
    index: {
      kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'shared-model', chatModel: 'other-model',
      searchTextArm: true, searchContentArm: true,
    },
  }));

  await waitFor(() => expect(modelSelect().value).toBe('shared-model'));

  await fireEvent.click(screen.getByTestId('model-tab-chat'));
  await waitFor(() => expect(optionsFor('other-model').length).toBe(1));
  // A per-role selection kept in one shared variable would show
  // `shared-model` chosen on the chat tab too, because it is the same string
  // the embedding tab just showed current. It must not.
  expect(modelSelect().value).toBe('other-model');
});

// 🔴 PR 25 review, P2-4. The list was keyed by `entry.id`, and the parser
// enforces no uniqueness over that field: `catalogue.rs` copies it off the raw
// record verbatim, once per record, so a provider that lists one id twice sends
// two entries with equal ids. Task 4's select renders them unkeyed for the
// same reason the old list did: a keyed `{#each}` throws on a repeat
// (`each_key_duplicate`), which does not degrade the option — it takes the
// whole section down.
//
// The fixture is the state the code branches on and nothing else: two entries,
// one id, two different names. `two_records_sharing_one_id_both_reach_the_catalogue_and_neither_is_renamed`
// (crates/mnema-provider/tests/catalogue.rs) is the other half — it pins that
// the parser really does hand this shape over, so this fixture is a measured
// state rather than one invented here.
test('two provider records sharing one id render two options and leave the section standing', async () => {
  mockCatalogues({
    embedding: catalogueOf([
      entry('vendor/twin', { name: 'First listing' }),
      entry('vendor/twin', { name: 'Second listing' }),
    ]),
  });
  await renderWith(settings({ key: { kind: 'present' } }));

  // The list itself: two options, and they are the two records rather than
  // one record drawn twice — the names differ, and both are on screen.
  await waitFor(() => expect(optionsFor('vendor/twin').length).toBe(2));
  expect(optionsFor('vendor/twin').map((o) => o.textContent)).toEqual(['First listing', 'Second listing']);

  // And the rest of the section is still there. This is the half the option
  // count cannot say: `each_key_duplicate` is thrown during render, so what
  // it costs is everything AROUND the list — read here as the visible text a
  // person came to this window for, not as a testid that could be present on
  // a broken page.
  expect(screen.getByTestId('model-key-label').textContent).toBe('Key:');
  expect(screen.getByRole('button', { name: 'Change' })).toBeTruthy();
  expect(screen.getByTestId('model-status-dot').textContent).toBe(
    'Not connected yet — add a key and choose an embedding model to enable content search.',
  );
});

// ---------------------------------------------------------------------------
// Task 8 (owner's ruling, live run, 2026-09-10): a refused entry no longer
// renders as a disabled option — it disappears from the select outright, and
// one line per DISTINCT refusal reason names how many entries it folded
// together, below the select.
// ---------------------------------------------------------------------------

test('refused_models_are_not_options', async () => {
  mockCatalogues({
    embedding: catalogueOf([
      entry('acc-1', { name: 'Accepted One' }),
      // Review round 1, Important 1: `bad-3` (the lone `noTextOutput`, count
      // 1) appears BEFORE either `inputTooSmall` entry (count 2) on purpose —
      // first-appearance order alone would put `bad-3`'s reason first, and
      // only the count-descending comparator puts it second. A fixture that
      // put the higher-count reason first too could not tell "sorted by
      // count" from "sorted by appearance, which happens to agree here" —
      // deleting the comparator left this same assertion green before this
      // reorder.
      entry('bad-3', { name: 'No Text', refusal: { kind: 'noTextOutput' } }),
      entry('bad-1', { name: 'Too Small A', refusal: { kind: 'inputTooSmall', limit: 100, floor: 2048 } }),
      entry('acc-2', { name: 'Accepted Two' }),
      entry('bad-2', { name: 'Too Small B', refusal: { kind: 'inputTooSmall', limit: 200, floor: 2048 } }),
    ]),
  });
  await renderWith(settings());
  await waitFor(() => expect(optionsFor('acc-1').length).toBe(1));

  // Exactly the two accepted entries plus the placeholder — nothing refused
  // stands in as an option, disabled or otherwise.
  const options = [...modelSelect().querySelectorAll('option')];
  expect(options.length).toBe(3);
  expect(options.filter((o) => o.disabled && o.value !== '').length).toBe(0);
  expect(optionsFor('bad-1').length).toBe(0);
  expect(optionsFor('bad-2').length).toBe(0);
  expect(optionsFor('bad-3').length).toBe(0);

  // Two distinct reasons: the two `inputTooSmall` entries share the same
  // floor and fold into one line (count 2), `noTextOutput` gets its own
  // (count 1) — ordered by count descending, NOT by which one this build
  // met first in the catalogue (the fixture above deliberately disagrees
  // with count order on appearance order alone).
  const reasons = screen.getAllByTestId('model-hidden-reason').map((el) => el.textContent);
  expect(reasons).toEqual([
    '2 hidden: input limit below 2048 tokens',
    '1 hidden: the model outputs no text',
  ]);
});

// Review round 1, Minor 5: nothing above gives two refused entries the SAME
// id, so "two records sharing an id both count" (Task 8 brief) was asserted
// nowhere — a `hiddenReasons` that counted distinct ids instead of raw
// entries would have passed every test above just the same.
test('two refused entries sharing the same id both count toward their reason', async () => {
  mockCatalogues({
    embedding: catalogueOf([
      entry('vendor/twin', { name: 'First listing', refusal: { kind: 'noStatedLimit' } }),
      entry('vendor/twin', { name: 'Second listing', refusal: { kind: 'noStatedLimit' } }),
    ]),
  });
  await renderWith(settings());
  await waitFor(() => expect(screen.getByTestId('model-hidden-reason')).toBeTruthy());

  expect(optionsFor('vendor/twin').length).toBe(0);
  expect(screen.getByTestId('model-hidden-reason').textContent).toBe(
    '2 hidden: the provider states no input limit',
  );
});

// Review round 1, Important 2: no fixture covered a catalogue that is not
// EMPTY (`cat.entries.length > 0`) but has nothing SELECTABLE left in it
// (`selectableEntries.length === 0`) — so the render guard silently
// switching from the former to the latter dropped every hidden-reason line
// (and the select itself) with no test noticing.
test('an all-refused catalogue still shows the select, placeholder only, and every hidden-reason line', async () => {
  mockCatalogues({
    embedding: catalogueOf([
      entry('bad-1', { name: 'Too Small', refusal: { kind: 'inputTooSmall', limit: 100, floor: 2048 } }),
      entry('bad-2', { name: 'No Text', refusal: { kind: 'noTextOutput' } }),
    ]),
  });
  await renderWith(settings());
  await waitFor(() => expect(screen.getByTestId('model-selection')).toBeTruthy());

  // Not the "provider lists none" sentence — the provider DID list models,
  // this build refuses all of them, and those are different claims.
  expect(screen.queryByTestId('model-catalogue-empty')).toBeNull();

  // The select still renders — nothing but the placeholder, since every
  // entry offered is refused.
  const options = [...modelSelect().querySelectorAll('option')];
  expect(options.length).toBe(1);
  expect(options[0].value).toBe('');

  const reasons = screen.getAllByTestId('model-hidden-reason').map((el) => el.textContent);
  expect(reasons).toEqual([
    '1 hidden: input limit below 2048 tokens',
    '1 hidden: the model outputs no text',
  ]);
});

// The floor above is data from the fixture, never a UI literal — proved by
// changing only the floor and watching the hidden line change with it.
test('refused_models_are_not_options: the floor in the hidden line is read from the fixture, not a literal', async () => {
  mockCatalogues({
    embedding: catalogueOf([
      entry('acc-1', { name: 'Accepted One' }),
      entry('bad-1', { name: 'Too Small A', refusal: { kind: 'inputTooSmall', limit: 100, floor: 4096 } }),
    ]),
  });
  await renderWith(settings());
  await waitFor(() => expect(optionsFor('acc-1').length).toBe(1));

  expect(screen.getByTestId('model-hidden-reason').textContent).toBe(
    '1 hidden: input limit below 4096 tokens',
  );
});

// A confirmed model the catalogue still lists, but now refuses: it is
// indistinguishable, on the select, from a model the provider retired
// outright — it shows through the existing absent-id placeholder path
// (`a confirmed model absent from the catalogue gets its own placeholder
// option` below is the retired-model half of this claim) — and the
// hidden-reason line still counts it, because it really is one of the
// entries this build refused.
test('a_refused_confirmed_model_shows_as_absent', async () => {
  mockCatalogues({
    embedding: catalogueOf([
      entry('confirmed-model', { name: 'Confirmed', refusal: { kind: 'noStatedLimit' } }),
      entry('acc-1', { name: 'Accepted' }),
    ]),
  });
  await renderWith(settings({
    key: { kind: 'present' },
    index: {
      kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null,
      embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'confirmed-model', chatModel: null,
      searchTextArm: true, searchContentArm: true,
    },
  }));
  await waitFor(() => expect(optionsFor('acc-1').length).toBe(1));

  expect(modelSelect().value).toBe('confirmed-model');
  expect(optionsFor('confirmed-model').length).toBe(1); // the placeholder, not a normal option too
  const placeholder = optionFor('confirmed-model');
  expect(placeholder.disabled).toBe(true);
  expect(placeholder.textContent).toContain('confirmed-model');

  expect(screen.getByTestId('model-hidden-reason').textContent).toBe(
    '1 hidden: the provider states no input limit',
  );
});

// ---------------------------------------------------------------------------
// The green dot: provider ∧ key ∧ a chosen embedding model, fail-safe on
// each missing in turn — three separate fixtures, because a fixture that
// drops two at once cannot tell which one the code actually reads.
// ---------------------------------------------------------------------------

test('the status dot is ready when provider, key and a chosen embedding model are all set', async () => {
  await renderWith(settings({
    key: { kind: 'present' },
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'text-embedding-3-small', chatModel: null, searchTextArm: true, searchContentArm: true },
  }));
  const dot = screen.getByTestId('model-status-dot');
  expect(dot.getAttribute('data-active')).toBe('true');
  expect(dot.textContent).toBe('Connected — OpenRouter, a key and a chosen embedding model are all set.');
});

test('the status dot is not ready when the index cannot be read', async () => {
  await renderWith(settings({
    key: { kind: 'present' },
    index: { kind: 'unreadable', cause: 'notOpen', reason: 'r' },
  }));
  const dot = screen.getByTestId('model-status-dot');
  expect(dot.getAttribute('data-active')).toBe('false');
  expect(dot.textContent).toBe('Not connected yet — add a key and choose an embedding model to enable content search.');
});

test('the status dot is not ready when there is no key', async () => {
  await renderWith(settings({
    key: { kind: 'absent' },
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'text-embedding-3-small', chatModel: null, searchTextArm: true, searchContentArm: true },
  }));
  expect(screen.getByTestId('model-status-dot').getAttribute('data-active')).toBe('false');
});

test('the status dot is not ready when no embedding model is chosen', async () => {
  await renderWith(settings({
    key: { kind: 'present' },
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: null, chatModel: null, searchTextArm: true, searchContentArm: true },
  }));
  expect(screen.getByTestId('model-status-dot').getAttribute('data-active')).toBe('false');
});

// ---------------------------------------------------------------------------
// A stated zero is never a promise the list is complete (umbrella `:529`).
// ---------------------------------------------------------------------------

test('unreadable > 0 renders a sentence naming how many entries could not be read', async () => {
  mockCatalogues({ embedding: catalogueOf([entry('e1')], 3) });
  await renderWith(settings());
  await waitFor(() => expect(screen.getByTestId('model-catalogue-unreadable')).toBeTruthy());
  expect(screen.getByTestId('model-catalogue-unreadable').textContent).toBe('3 records could not be read.');
});

test('unreadable: 0 renders no such sentence', async () => {
  mockCatalogues({ embedding: catalogueOf([entry('e1')], 0) });
  await renderWith(settings());
  await waitFor(() => expect(optionsFor('e1').length).toBe(1));
  expect(screen.queryByTestId('model-catalogue-unreadable')).toBeNull();
});

test('an empty but well-formed catalogue says the provider lists none, not that something failed', async () => {
  mockCatalogues({ embedding: catalogueOf([], 0) });
  await renderWith(settings());
  await waitFor(() => expect(screen.getByTestId('model-catalogue-empty')).toBeTruthy());
  expect(screen.getByTestId('model-catalogue-empty').textContent)
    .toBe('The provider does not currently list any models for this role.');
  expect(screen.queryByTestId('model-catalogue-failure')).toBeNull();
});

// ---------------------------------------------------------------------------
// Choosing a model, and the ordering hazard booked to this task
// (umbrella `:525`): a read redrawing while a write is in flight.
// ---------------------------------------------------------------------------

function deferredPromise<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function queuedModelSettings() {
  const queue: ReturnType<typeof deferredPromise<ModelSettings>>[] = [];
  modelSettings.mockImplementation(() => {
    const d = deferredPromise<ModelSettings>();
    queue.push(d);
    return d.promise;
  });
  return queue;
}

test('the shown selection does not change until set_chat_model AND its re-read both resolve — not on the click alone', async () => {
  mockCatalogues({ chat: catalogueOf([entry('gpt-a'), entry('gpt-b')]) });
  modelSettings.mockResolvedValueOnce(settings({
    key: { kind: 'present' },
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'text-embedding-3-small', chatModel: 'gpt-a', searchTextArm: true, searchContentArm: true },
  }));
  const setChatModelCall = deferredPromise<void>();
  setChatModel.mockImplementation(() => setChatModelCall.promise);

  renderModels();
  await fireEvent.click(await screen.findByTestId('model-tab-chat'));
  await waitFor(() => expect(modelSelect().value).toBe('gpt-a'));

  await pickModel('gpt-b');
  await waitFor(() => expect(setChatModel).toHaveBeenCalledWith('gpt-b'));
  // set_chat_model has not resolved yet — the pick alone must not repaint.
  expect(modelSelect().value).toBe('gpt-a');

  modelSettings.mockResolvedValueOnce(settings({
    key: { kind: 'present' },
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'text-embedding-3-small', chatModel: 'gpt-b', searchTextArm: true, searchContentArm: true },
  }));
  setChatModelCall.resolve();

  await waitFor(() => expect(modelSelect().value).toBe('gpt-b'));
});

test('an older in-flight model_settings does not repaint the model a set_chat_model round just chose', async () => {
  mockCatalogues({ chat: catalogueOf([entry('gpt-a'), entry('gpt-b')]) });
  setChatModel.mockResolvedValue(undefined);
  const queue = queuedModelSettings();

  renderModels(); // issues the mount's own call — call #0, deferred
  await fireEvent.click(await screen.findByTestId('model-tab-chat'));
  await waitFor(() => expect(optionsFor('gpt-b').length).toBe(1));

  await pickModel('gpt-b');
  await waitFor(() => expect(setChatModel).toHaveBeenCalledWith('gpt-b'));
  await waitFor(() => expect(queue.length).toBe(2)); // mount's call, then the choice's own refresh

  // The fresh call — issued by the choice — settles first, with the new model.
  queue[1].resolve(settings({
    key: { kind: 'present' },
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'text-embedding-3-small', chatModel: 'gpt-b', searchTextArm: true, searchContentArm: true },
  }));
  await waitFor(() => expect(modelSelect().value).toBe('gpt-b'));

  // The mount's OLDER call settles late, with the old model. It must lose.
  queue[0].resolve(settings({
    key: { kind: 'present' },
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'text-embedding-3-small', chatModel: 'gpt-a', searchTextArm: true, searchContentArm: true },
  }));
  // Not `await Promise.resolve()` twice: that gave the mutant that deletes
  // the sequence guard enough of a head start to look passing, because two
  // bare microtask turns were not enough for its (wrong) DOM update to land
  // before the assertions ran — the test was green whether the guard did its
  // job or not. `tick()` waits for Svelte's own pending update instead.
  await tick();
  await tick();
  await tick();
  expect(modelSelect().value).toBe('gpt-b');
});

// Reviewer's probe (Critical 1): `reportLoadFailure` carried no `settingsSeq`
// stamp of its own, so an OLDER read's rejection landed on `loadError`
// whatever a newer read had already done — the rejection-side twin of the
// resolution-side guard proved above. Two reads in flight, the mount's own
// (older) and the scan-ended re-read (newer): the newer settles first with
// real data, then the older rejects late. The failure sentence must not
// appear over a panel a newer read already confirmed.
test('a stale rejection from an older read does not overwrite a newer success', async () => {
  const queue = queuedModelSettings();

  renderModels(); // issues the mount's own call — call #0, deferred
  await waitFor(() => expect(queue.length).toBe(1));

  emit(endedScan()); // scan-ended re-read — call #1, deferred, still concurrent
  await waitFor(() => expect(queue.length).toBe(2));

  // The newer call settles first, with real settings.
  queue[1].resolve(settings({ key: { kind: 'present' } }));
  await waitFor(() => expect(screen.getByTestId('model-key-saved')).toBeTruthy());

  // The mount's OLDER call rejects late. It must not overwrite the newer
  // success with a failure sentence.
  queue[0].reject(new Error('a read nobody is waiting for any more'));
  await tick();
  await tick();
  await tick();
  expect(screen.queryByTestId('model-load-failure')).toBeNull();
  expect(screen.getByTestId('model-key-saved')).toBeTruthy();
});

test('a model_settings reply landing while set_chat_model is still pending does not block the new choice once it resolves', async () => {
  mockCatalogues({ chat: catalogueOf([entry('gpt-a'), entry('gpt-b')]) });
  const queue = queuedModelSettings();
  const setChatModelCall = deferredPromise<void>();
  setChatModel.mockImplementation(() => setChatModelCall.promise);

  renderModels(); // call #0, mount
  await fireEvent.click(await screen.findByTestId('model-tab-chat'));
  await waitFor(() => expect(optionsFor('gpt-b').length).toBe(1));

  await pickModel('gpt-b');
  await waitFor(() => expect(setChatModel).toHaveBeenCalledWith('gpt-b'));
  // The choice's own refresh runs AFTER set_chat_model resolves, sequentially
  // — it has not been issued yet, so only the mount's call exists so far.
  expect(queue.length).toBe(1);

  // model_settings resolves — with the OLD model, since nothing has actually
  // changed yet.
  queue[0].resolve(settings({
    key: { kind: 'present' },
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'text-embedding-3-small', chatModel: 'gpt-a', searchTextArm: true, searchContentArm: true },
  }));
  await waitFor(() => expect(modelSelect().value).toBe('gpt-a'));

  // set_chat_model resolves next; its own refresh issues call #1.
  setChatModelCall.resolve();
  await waitFor(() => expect(queue.length).toBe(2));
  queue[1].resolve(settings({
    key: { kind: 'present' },
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'text-embedding-3-small', chatModel: 'gpt-b', searchTextArm: true, searchContentArm: true },
  }));

  await waitFor(() => expect(modelSelect().value).toBe('gpt-b'));
});

// ---------------------------------------------------------------------------
// Review P2-5/P2-6/P2-7 — three branches this file built no fixture for, each
// proved undefended by a surviving mutant: the two `catalogueSeq` ordering
// guards, `provider_models`' rejection, and `set_chat_model`'s rejection.
// ---------------------------------------------------------------------------

// The `providerModels` twin of `queuedModelSettings`, kept per role because
// the guard it exists to exercise is per role.
function queuedProviderModels() {
  const queue: Record<ModelRole, ReturnType<typeof deferredPromise<Catalogue>>[]> =
    { embedding: [], rerank: [], chat: [] };
  providerModels.mockImplementation((role: ModelRole) => {
    const d = deferredPromise<Catalogue>();
    queue[role].push(d);
    return d.promise;
  });
  return queue;
}

// Two in-flight reads of the SAME role: leaving the embedding tab and coming
// back issues a second one while the first is still out. No fixture in this
// file used to defer `provider_models` at all, so deleting both
// `if (seq !== catalogueSeq[role]) return;` guards left 62 tests passing.
test('an older in-flight provider_models does not overwrite the catalogue a later one already showed', async () => {
  const queue = queuedProviderModels();
  await renderWith(settings());
  await waitFor(() => expect(queue.embedding.length).toBe(1)); // the mount's own read

  await fireEvent.click(screen.getByTestId('model-tab-chat'));
  await fireEvent.click(screen.getByTestId('model-tab-embedding'));
  await waitFor(() => expect(queue.embedding.length).toBe(2));

  // The newer read settles first.
  queue.embedding[1].resolve(catalogueOf([entry('fresh-model', { name: 'Fresh' })]));
  await waitFor(() => expect(optionsFor('fresh-model').length).toBe(1));

  // The mount's OLDER read settles late, with a different list. It must lose.
  queue.embedding[0].resolve(catalogueOf([entry('stale-model', { name: 'Stale' })]));
  await tick();
  await tick();
  await tick();
  expect(optionsFor('fresh-model').length).toBe(1);
  expect(optionsFor('stale-model').length).toBe(0);
});

// The same guard on the rejection path — a separate `if` on a separate line,
// so it needs its own fixture. An older read that fails after a newer one has
// already painted a list must not replace that list with a failure sentence.
test('an older in-flight provider_models that fails late does not replace the catalogue a later one already showed', async () => {
  const queue = queuedProviderModels();
  await renderWith(settings());
  await waitFor(() => expect(queue.embedding.length).toBe(1));

  await fireEvent.click(screen.getByTestId('model-tab-chat'));
  await fireEvent.click(screen.getByTestId('model-tab-embedding'));
  await waitFor(() => expect(queue.embedding.length).toBe(2));

  queue.embedding[1].resolve(catalogueOf([entry('fresh-model', { name: 'Fresh' })]));
  await waitFor(() => expect(optionsFor('fresh-model').length).toBe(1));

  queue.embedding[0].reject(new Error('a read nobody is waiting for any more'));
  await tick();
  await tick();
  await tick();
  expect(screen.queryByTestId('model-catalogue-failure')).toBeNull();
  expect(optionsFor('fresh-model').length).toBe(1);
});

// P2-6: the rejection branch itself. The only mention of this testid was a
// one-sided `queryByTestId(...).toBeNull()`, which zero satisfies — a mutant
// that swallowed the error survived. Asserted in the direction that says the
// paragraph is there, carrying the backend's own sentence verbatim (§10: a
// rejection arrives as a sentence, never as a kind).
test('a rejected provider_models says so in the backends own words, and claims nothing about the provider', async () => {
  providerModels.mockRejectedValue(new Error('the provider did not answer'));
  const { container } = await renderWith(settings());

  await waitFor(() => expect(screen.getByTestId('model-catalogue-failure')).toBeTruthy());
  expect(screen.getByTestId('model-catalogue-failure').textContent).toBe('the provider did not answer');
  // Both directions: no select, and — the claim that matters — NOT the
  // sentence saying the provider lists no models, which this build has no
  // grounds for.
  expect(screen.queryByTestId('model-selection')).toBeNull();
  expect(screen.queryByTestId('model-catalogue-empty')).toBeNull();
  expect(container.textContent ?? '').not.toContain('The provider does not currently list any models');
});

// P2-7: `set_chat_model`'s rejection path. No `mockRejected` on `setChatModel`
// existed anywhere, so removing the whole `try/catch` from `chooseChatModel`
// survived the suite.
test('a rejected set_chat_model shows the backends sentence and leaves the selection where it was', async () => {
  mockCatalogues({ chat: catalogueOf([entry('gpt-a', { name: 'Model A' }), entry('gpt-b', { name: 'Model B' })]) });
  setChatModel.mockRejectedValue(new Error('the provider will not serve this model to this key'));
  await renderWith(settings({
    key: { kind: 'present' },
    index: {
      kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'text-embedding-3-small', chatModel: 'gpt-a',
      searchTextArm: true, searchContentArm: true,
    },
  }));

  await fireEvent.click(screen.getByTestId('model-tab-chat'));
  await waitFor(() => expect(optionsFor('gpt-b').length).toBe(1));
  await pickModel('gpt-b');

  await waitFor(() => expect(screen.getByTestId('model-action-error')).toBeTruthy());
  expect(screen.getByTestId('model-action-error').textContent)
    .toBe('the provider will not serve this model to this key');
  // The refused choice is not shown as taken: the selection still follows the
  // backend's state, which never changed.
  expect(modelSelect().value).toBe('gpt-a');
});

// P2-9: the dot's fail-safe on a state this window does not know. On a
// rejected mount `settings` stays null forever — nothing on this screen can
// retry it — and the mutant `!settings || providerReady(settings)` makes the
// dot claim everything is connected for as long as that window stays open. A
// green dot that means less than provider ∧ key ∧ model is a promise the
// product cannot keep.
test('a mount whose model_settings rejects leaves the dot NOT connected, not connected by default', async () => {
  setLocale('en');
  modelSettings.mockRejectedValue(new Error('the settings window could not reach the core'));
  renderModels();

  await waitFor(() => expect(screen.getByTestId('model-load-failure')).toBeTruthy());
  const dot = screen.getByTestId('model-status-dot');
  expect(dot.getAttribute('data-active')).toBe('false');
  expect(dot.textContent)
    .toBe('Not connected yet — add a key and choose an embedding model to enable content search.');
});

// P2-10: zero entries beside `unreadable > 0`. The screen said "2 records
// could not be read." and then "The provider does not currently list any
// models for this role." — untrue, since the provider sent two, and it sends
// the person to look at the provider instead of at the defect.
test('a catalogue of nothing but unreadable records does not also claim the provider listed none', async () => {
  mockCatalogues({ embedding: catalogueOf([], 2) });
  const { container } = await renderWith(settings());

  await waitFor(() => expect(screen.getByTestId('model-catalogue-unreadable')).toBeTruthy());
  const text = container.textContent ?? '';
  expect(text).toContain('2 records could not be read.');
  expect(text).not.toContain('The provider does not currently list any models for this role.');
  expect(screen.queryByTestId('model-catalogue-empty')).toBeNull();
  // And the records themselves are still named, one per counted record.
  expect(screen.getByTestId('model-unreadable-record-0')).toBeTruthy();
  expect(screen.getByTestId('model-unreadable-record-1')).toBeTruthy();
});

// ---------------------------------------------------------------------------
// The discriminant mirror, homeless since `ui/render.test.js` was deleted
// (umbrella `:526`) — built here because this is the surface that renders
// `Refusal` and `RecordId`. Derived, not hand-copied: the variant list comes
// from `catalogue.rs` itself, walked at test time, so a variant added there
// and not taught to `sampleRefusal`/`sampleRecordId` fails this test with a
// message naming exactly what to do, rather than passing silently — and a
// variant taught here but never given a render arm in `Models.svelte` fails
// just as loudly, because `hiddenReasonLabel`/`unreadableRecordLabel` throw
// rather than fall through.
//
// `Balance` (`crates/mnema-provider/src/probe.rs`) is the third discriminant
// the umbrella plan named for this mirror, and does NOT get one here:
// nothing in this PR renders it (see `ipc.ts`'s own comment on `Balance`), so
// building a fixture "renderer" for it here would be exercising code this
// component does not have. That gap is real, is the same one the original
// `render.test.js` named for the same three types, and stays open rather than
// acquiring a fixture that fakes a home for it.
// ---------------------------------------------------------------------------

const HERE = dirname(fileURLToPath(import.meta.url));
const CATALOGUE_RS = readFileSync(join(HERE, '../../../crates/mnema-provider/src/catalogue.rs'), 'utf8');

// Review P1-3: `raw` is provider text under exactly the rule `reason` is under
// — it does not reach the screen. The old fixture said `'sample-raw'`, ten
// characters, and the only thing standing between a leak and a green suite was
// `expect(text.length).toBeGreaterThan(10)`: a fixture string's length, not an
// assertion. This token is long, distinctive, and asserted absent below.
const RAW_LEAK_TOKEN = 'PROVIDER-RAW-DO-NOT-RENDER-4c1f88e2';

function sampleRefusal(kind: string): ModelRefusal {
  switch (kind) {
    case 'inputTooSmall': return { kind: 'inputTooSmall', limit: 100, floor: 2048 };
    case 'noStatedLimit': return { kind: 'noStatedLimit' };
    case 'limitNotUnderstood': return { kind: 'limitNotUnderstood', raw: RAW_LEAK_TOKEN };
    case 'noStatedOutputModalities': return { kind: 'noStatedOutputModalities' };
    case 'noTextOutput': return { kind: 'noTextOutput' };
    default:
      throw new Error(
        `catalogue.rs now defines a Refusal variant ("${kind}") this test does not know how to ` +
        'build a fixture for — teach sampleRefusal about it, then verify Models.svelte renders it.',
      );
  }
}

function sampleRecordId(kind: string, id: string): RecordId {
  switch (kind) {
    case 'absent': return { kind: 'absent' };
    case 'notAString': return { kind: 'notAString', raw: RAW_LEAK_TOKEN };
    case 'known': return { kind: 'known', id };
    default:
      throw new Error(
        `catalogue.rs now defines a RecordId variant ("${kind}") this test does not know how to ` +
        'build a fixture for — teach sampleRecordId about it, then verify Models.svelte renders it.',
      );
  }
}

async function renderInLocale(loc: 'uk' | 'en', s: ModelSettings) {
  return loc === 'uk' ? renderInUk(s) : renderWith(s);
}

// Review P1-1: distinctness is not correspondence. The old form asserted only
// `new Set(texts).size === kinds.length`, so swapping two arms of
// `hiddenReasonLabel` — `noStatedOutputModalities` ↔ `noTextOutput` — left the
// suite at 62 passed: the sentences were still distinct, merely attached to
// the wrong variants. `catalogue.rs:228-236` keeps those two apart precisely
// "so a provider who renames or drops either field cannot make this code
// state, as a fact about the model, something the provider never said", and
// after the swap a model the provider said NOTHING about renders as "The
// provider states that this model does not output text" — the exact false
// claim the split exists to prevent. Exactly one variant was pinned to its
// sentence; the other four permuted freely.
//
// So: a kind → sentence table, per locale, asserted with `toBe`. The variant
// LIST still comes from Rust, so a new variant fails the lookup loudly rather
// than being quietly excused from the table.
//
// Owner's ruling (live run, 2026-09-10): a refused entry no longer renders
// inline at all — it disappears from the select, and this sentence (with
// the count of entries it folded together) is what surfaces on its own
// `model-hidden-reason` line instead. `sampleRefusal`'s `inputTooSmall`
// fixes `floor` at 2048, and every kind below appears exactly once per
// fixture, so `count` is always 1 here.
const HIDDEN_REASON_SENTENCES: Record<'en' | 'uk', Record<string, (count: number) => string>> = {
  en: {
    inputTooSmall: (count) => `${count} hidden: input limit below 2048 tokens`,
    noStatedLimit: (count) => `${count} hidden: the provider states no input limit`,
    limitNotUnderstood: (count) => `${count} hidden: input limit in a format this build cannot read`,
    noStatedOutputModalities: (count) => `${count} hidden: the provider does not say what the model outputs`,
    noTextOutput: (count) => `${count} hidden: the model outputs no text`,
  },
  uk: {
    inputTooSmall: (count) => `Приховано ${count}: ліміт входу менший за 2048 токенів`,
    noStatedLimit: (count) => `Приховано ${count}: постачальник не вказує ліміт входу`,
    limitNotUnderstood: (count) => `Приховано ${count}: ліміт входу у форматі, який ця збірка не читає`,
    noStatedOutputModalities: (count) => `Приховано ${count}: постачальник не вказує, що видає модель`,
    noTextOutput: (count) => `Приховано ${count}: модель не видає текст`,
  },
};

// Same table for `RecordId`, taking the record's own position and id — the
// only parts of these sentences the fixture, not the code, decides. The
// mapping kind → sentence is still stated here in full and never computed
// from the component.
const RECORD_ID_SENTENCES: Record<'en' | 'uk', Record<string, (i: number, id: string) => string>> = {
  en: {
    absent: (i) => `Record at position ${i}: the provider stated no model id.`,
    notAString: (i) => `Record at position ${i}: the model id was not text.`,
    known: (i, id) => `Record at position ${i}, id "${id}": this build could not read the rest of the record.`,
  },
  uk: {
    absent: (i) => `Запис на позиції ${i}: постачальник не вказав ідентифікатор моделі.`,
    notAString: (i) => `Запис на позиції ${i}: ідентифікатор моделі не був текстом.`,
    known: (i, id) => `Запис на позиції ${i}, ідентифікатор «${id}»: решту запису ця збірка прочитати не змогла.`,
  },
};

// Both directions. `expected()` below catches a variant Rust defines and the
// table does not; on its own that is one-sided, and a table entry for a variant
// Rust no longer defines would sit here forever describing a screen that cannot
// happen. The key sets must match exactly.
function expectTableCoversExactly(table: Record<string, unknown>, kinds: string[]) {
  expect(kinds.length).toBeGreaterThan(0); // the parse itself must have found something
  expect([...Object.keys(table)].sort()).toEqual([...kinds].sort());
}

// The tables are keyed by wire kind; a variant Rust defines and a table does
// not is a missing expectation, and saying so beats `toBe(undefined)`.
function expected<T>(table: Record<string, T>, kind: string, what: string): T {
  const value = table[kind];
  if (value === undefined) {
    throw new Error(
      `catalogue.rs defines a ${what} variant ("${kind}") this test states no expected sentence for — ` +
      'add the sentence Models.svelte must render for it, in both locales.',
    );
  }
  return value;
}

for (const loc of ['en', 'uk'] as const) {
  test(`every Refusal variant catalogue.rs defines renders its own, correct hidden-reason line (${loc})`, async () => {
    const kinds = rustEnumVariants(CATALOGUE_RS, 'Refusal').map(camelOf);
    expectTableCoversExactly(HIDDEN_REASON_SENTENCES[loc], kinds);
    const fixtureEntries = kinds.map((kind, i) => entry(`refusal-${i}`, { name: `Named ${kind}`, refusal: sampleRefusal(kind) }));
    mockCatalogues({ embedding: catalogueOf(fixtureEntries) });

    const { container } = await renderInLocale(loc, settings());
    await waitFor(() => expect(screen.getAllByTestId('model-hidden-reason').length).toBe(kinds.length));

    // Owner's ruling (live run, 2026-09-10): every refused entry is gone from
    // the select outright, not merely disabled — no option stands in for any
    // of them any more.
    for (let i = 0; i < kinds.length; i++) {
      expect(optionsFor(`refusal-${i}`).length).toBe(0);
    }

    // Each fixture entry is refused alone (one per kind), so every reason
    // folds exactly one entry — count 1 — and ties on count sort by first
    // appearance, which is exactly the kinds' own order.
    const texts = screen.getAllByTestId('model-hidden-reason').map((el) => el.textContent ?? '');
    const expectedTexts = kinds.map((kind) => expected(HIDDEN_REASON_SENTENCES[loc], kind, 'Refusal')(1));
    expect(texts).toEqual(expectedTexts);
    // P1-1's point still holds: each variant pinned to ITS OWN sentence, not
    // merely to a sentence no other variant happens to use.
    expect(new Set(texts).size).toBe(kinds.length);
    // P1-3: `limitNotUnderstood` carries provider text. It does not reach the
    // screen — the same rule the folded-in option label used to be under.
    expect(container.textContent ?? '').not.toContain(RAW_LEAK_TOKEN);
    // And the model's own name reaches it even less: a refused entry is
    // hidden wholesale now, not merely stripped of its reason.
    expect(container.textContent ?? '').not.toContain('Named ');
  });
}

for (const loc of ['en', 'uk'] as const) {
  test(`every RecordId variant catalogue.rs defines renders its own, correct line (${loc})`, async () => {
    const kinds = rustEnumVariants(CATALOGUE_RS, 'RecordId').map(camelOf);
    expectTableCoversExactly(RECORD_ID_SENTENCES[loc], kinds);
    const records: UnreadableRecord[] = kinds.map((kind, i) => ({ id: sampleRecordId(kind, `model-${i}`), index: i }));
    mockCatalogues({ embedding: catalogueOf([], records.length, records) });

    const { container } = await renderInLocale(loc, settings());
    const texts: string[] = [];
    for (let i = 0; i < kinds.length; i++) {
      const el = await screen.findByTestId(`model-unreadable-record-${i}`);
      texts.push(el.textContent ?? '');
      expect(texts[i]).toBe(expected(RECORD_ID_SENTENCES[loc], kinds[i], 'RecordId')(i, `model-${i}`));
    }
    expect(texts.length).toBe(kinds.length);
    expect(new Set(texts).size).toBe(kinds.length);
    // The rule is narrower than "provider text stays off the screen", and
    // saying it the wide way would be false of the line above: a `known` id IS
    // provider text and IS rendered, because `RecordId` splits that variant out
    // precisely so a person has something to go and look at. What stays off is
    // text this build could NOT read — `raw` — which is a diagnostic, not a
    // fact about a model. Both halves asserted, so neither can drift alone.
    expect(container.textContent ?? '').not.toContain(RAW_LEAK_TOKEN);
  });
}

// ---------------------------------------------------------------------------
// Step 5 — the reading order, inherited from Task 4's review. Rendered with
// a locked key store and a failed index read at once, the section used to
// produce an unlabelled index sentence sandwiched between the provider row
// and the key rows, with the one actionable line last and unmarked. Fixed by
// naming each sentence's subject, asserted on rendered TEXT ORDER rather than
// markup order alone — an assertion that only pins the DOM sequence would
// pass on a screen a person still cannot parse.
// ---------------------------------------------------------------------------

// Review P1-4: the old oracle's last condition was `keySentencePos < tabPos`
// — "the tabs follow the key sentence" — which is not the requirement, and the
// reviewer proved the oracle was pointed at the wrong proposition: a layout
// that SATISFIES the requirement (Key group lifted above Index) made it FAIL,
// and a worse one (an unrelated sentence above the provider, belonging to no
// subject at all) made it PASS. An oracle that rejects the compliant layout
// and accepts the worse one is not measuring the requirement.
//
// Owner's ruling on the layout itself, so it is not guessed: provider row,
// then the Key group (its label, the key-state sentence, and the key
// controls), then the tabs and the connected summary, then the Index group
// last. The one sentence a person can act on comes before the ones they
// cannot; every sentence sits under the subject it is about; the index
// failure is a defect report they cannot act on, so it goes last.
test('the worst screen groups every sentence under its own subject, and nothing a person cannot act on comes first', async () => {
  const { container } = await renderWith(settings({
    key: { kind: 'unreadable', cause: 'locked', reason: 'r' },
    index: { kind: 'unreadable', cause: 'notOpen', reason: 'r2' },
  }));
  const text = container.textContent ?? '';

  const providerPos = text.indexOf('OpenRouter');
  const keyLabelPos = text.indexOf('Key');
  const keySentencePos = text.indexOf(KEY_FAILURE_SENTENCES.locked);
  const indexLabelPos = text.indexOf('Index');
  const indexSentencePos = text.indexOf('The index is not open yet.');

  for (const pos of [providerPos, keyLabelPos, keySentencePos, indexLabelPos, indexSentencePos]) {
    expect(pos).toBeGreaterThanOrEqual(0);
  }

  // Grouped by subject: the key group follows the provider it belongs to, and
  // its own sentence sits under the Key label and before the next subject
  // starts.
  expect(providerPos).toBeLessThan(keyLabelPos);
  expect(keyLabelPos).toBeLessThan(keySentencePos);
  expect(keySentencePos).toBeLessThan(indexLabelPos);
  expect(indexLabelPos).toBeLessThan(indexSentencePos);

  // The requirement itself, stated directly rather than inferred from the
  // chain above: the one line a person can act on precedes the one they
  // cannot. This is the condition the old `keySentencePos < tabPos` replaced.
  expect(keySentencePos).toBeLessThan(indexSentencePos);

  // And its general form: NOTHING a person cannot act on is placed ahead of
  // the instruction that is theirs to follow. Named as a list, so a second
  // sentence added above the key group fails here rather than slipping
  // between two pairwise comparisons.
  const nothingToActOn = ['The index is not open yet.'];
  expect(nothingToActOn.filter((s) => text.indexOf(s) < keySentencePos)).toEqual([]);
});

// ---------------------------------------------------------------------------
// Every `void $locale` guard this task added, removed one at a time while
// writing this file and confirmed to redden its own test alone (report §6) —
// kept here as the tests that would catch a regression, not as the removal
// itself.
// ---------------------------------------------------------------------------

test('a language switch after mount reaches the model tab labels', async () => {
  mockCatalogues({ embedding: catalogueOf([entry('e1')]) });
  const { container } = await renderWith(settings());
  await waitFor(() => expect(optionsFor('e1').length).toBe(1));
  expect((container.textContent ?? '')).toContain('Embedding');
  expect((container.textContent ?? '')).toContain('Chat');

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('Ембединг');
  expect(after).toContain('Чат');
  expect(after).not.toContain('Embedding');
});

test('a language switch after mount reaches the index subject label', async () => {
  const { container } = await renderWith(settings({
    index: { kind: 'unreadable', cause: 'notOpen', reason: 'r' },
  }));
  expect((container.textContent ?? '')).toContain('Index');

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('Індекс');
  expect(after).not.toContain('Index');
});

test('a language switch after mount reaches the not-ready status dot sentence', async () => {
  const { container } = await renderWith(settings({ key: { kind: 'absent' } }));
  expect((container.textContent ?? '')).toContain('Not connected yet');

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('Ще не підключено');
  expect(after).not.toContain('Not connected yet');
});

// The ready and not-ready sentences are two separate `$derived.by` guards
// (`readyLabel`/`notReadyLabel`) — the test above only ever renders the
// not-ready branch, so it cannot tell `readyLabel`'s own `void $locale` apart
// from its absence. Measured while verifying each guard alone (report §6):
// removing `readyLabel`'s left this test green.
test('a language switch after mount reaches the ready status dot sentence', async () => {
  const { container } = await renderWith(settings({
    key: { kind: 'present' },
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'text-embedding-3-small', chatModel: null, searchTextArm: true, searchContentArm: true },
  }));
  expect((container.textContent ?? '')).toContain('Connected — OpenRouter');

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('Підключено — OpenRouter');
  expect(after).not.toContain('Connected — OpenRouter, a key');
});

test('a language switch after mount reaches the empty-catalogue sentence', async () => {
  mockCatalogues({ embedding: catalogueOf([]) });
  const { container } = await renderWith(settings());
  await waitFor(() => expect(screen.getByTestId('model-catalogue-empty')).toBeTruthy());
  expect((container.textContent ?? '')).toContain('The provider does not currently list any models');

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('Постачальник наразі не пропонує');
  expect(after).not.toContain('The provider does not currently list any models');
});

test('a language switch after mount reaches the unreadable-count sentence', async () => {
  mockCatalogues({ embedding: catalogueOf([entry('e1')], 2) });
  const { container } = await renderWith(settings());
  await waitFor(() => expect(screen.getByTestId('model-catalogue-unreadable')).toBeTruthy());
  expect((container.textContent ?? '')).toContain('2 records could not be read');

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('2 записи не вдалося прочитати');
  expect(after).not.toContain('records could not be read');
});

// Rewritten (owner's ruling, live run, 2026-09-10): the reason no longer
// reaches the screen through the option's own label — there is no option
// left to carry it — but the hidden-reason line below the select owes the
// same thing the option used to: it switches language after `setLocale`.
test('a language switch after mount reaches the hidden-reason line', async () => {
  mockCatalogues({ embedding: catalogueOf([entry('r1', { refusal: { kind: 'noStatedLimit' } })]) });
  const { container } = await renderWith(settings());
  await waitFor(() => expect(screen.getByTestId('model-hidden-reason')).toBeTruthy());
  expect(optionsFor('r1').length).toBe(0);
  expect((container.textContent ?? '')).toContain('1 hidden: the provider states no input limit');

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('Приховано 1: постачальник не вказує ліміт входу');
  expect(after).not.toContain('hidden: the provider states no input limit');
});

test('a language switch after mount reaches an unreadable-record label', async () => {
  mockCatalogues({ embedding: catalogueOf([], 1, [{ id: { kind: 'absent' }, index: 0 }]) });
  const { container } = await renderWith(settings());
  await waitFor(() => expect(screen.getByTestId('model-unreadable-record-0')).toBeTruthy());
  expect((container.textContent ?? '')).toContain('the provider stated no model id');

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('постачальник не вказав ідентифікатор моделі');
  expect(after).not.toContain('the provider stated no model id');
});

// ---------------------------------------------------------------------------
// Task 6 — choosing an embedding model. Every claim below is about what a
// person SEES, and each is asserted positively on visible text: "the dot is not
// green" and "no confirmation appeared" are both satisfied by a blank panel.
//
// The fixture question, first: the state these tests must build and the code
// branches on is an index that ALREADY HAS an embedding model — without it,
// "choosing a different model asks first" is indistinguishable from "every
// press asks", and the same-model direction cannot be expressed at all.
// ---------------------------------------------------------------------------

const CONFIRM_TITLE = 'Change the embedding model?';
const CONFIRM_LOSS =
  'These embeddings cannot be carried over: the change discards them. Search by meaning will be ' +
  'unavailable until the index is embedded again; search by words will still answer.';
const READY = 'Connected — OpenRouter, a key and a chosen embedding model are all set.';
const REEMBED_ENDED =
  'The embedding pass has ended, and search by meaning is still unavailable. It can be started again.';
const CHANGE_FAILED =
  'The embedding model was not adopted. Read the message below — a change that fails partway can ' +
  'still have discarded embeddings.';
const DEGRADED =
  'Search by meaning is unavailable until the index is embedded again. Search by words still answers.';
const RECOVER =
  'Choosing an embedding model again repairs this: it rewrites the pointer the index lost. ' +
  'Nothing already embedded is discarded by it.';
const JOB_RUNNING = 'An indexing job is running. It was not stopped, and it is still going.';

/// An index on `emb-1` with `everywhere` embeddings recorded across its spaces
/// and `active` in the one it points at — the two counts the section reads for
/// two different questions — over `total` chunks of document text.
///
/// `total` defaults to a NON-ZERO number because that is the ordinary index:
/// one with folders in it. It is the third number the degraded rule reads, and
/// it says whether there was anything to embed at all — an index holding no
/// text has an empty active space for a reason that is not a loss. Passing `0`
/// builds that other index deliberately.
function onModel(everywhere: number, active = everywhere, total = 12) {
  return settings({
    key: { kind: 'present' },
    index: {
      kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddingModel: 'emb-1', chatModel: null,
      embeddedChunks: active, embeddedChunksEverywhere: everywhere, totalChunks: total,
      searchTextArm: true, searchContentArm: true,
    },
  });
}

async function renderOnModel(s = onModel(7)) {
  mockCatalogues({
    embedding: catalogueOf([entry('emb-1', { name: 'Embedder One' }), entry('emb-2', { name: 'Embedder Two' })]),
  });
  const result = await renderWith(s);
  await waitFor(() => expect(optionsFor('emb-2').length).toBe(1));
  return result;
}

test('choosing a DIFFERENT embedding model asks before it calls anything', async () => {
  await renderOnModel();

  await pickModel('emb-2');

  expect(screen.getByTestId('model-embedding-confirm-title').textContent).toBe(CONFIRM_TITLE);
  expect(setEmbeddingModel).not.toHaveBeenCalled();
});

test('choosing the model the index is ALREADY on asks nothing and calls nothing', async () => {
  await renderOnModel();

  await pickModel('emb-1');

  expect(screen.queryByTestId('model-embedding-confirm-title')).toBeNull();
  expect(setEmbeddingModel).not.toHaveBeenCalled();
  // And the select still shows which model the index is on, so "nothing
  // happened" is not the same as "the pick unmarked it".
  expect(modelSelect().value).toBe('emb-1');
});

// The two counts are DIFFERENT here on purpose. `embeddedChunks` counts the
// active space and `embeddedChunksEverywhere` counts every space, and the
// change retires every space in its way — so a fixture that gives them the same
// value cannot tell the honest number from the one that understates the bill.
test('the confirmation states the number as an estimate and names what it counts', async () => {
  await renderOnModel(onModel(7, 3));

  await pickModel('emb-2');

  expect(screen.getByTestId('model-embedding-estimate').textContent).toBe(
    'The index holds 7 embeddings across all its vector spaces right now. That is an estimate ' +
    'read before the change; what the change actually discarded is reported after it.',
  );
});

test('Cancel takes the question away and still calls nothing', async () => {
  await renderOnModel();
  await pickModel('emb-2');

  await fireEvent.click(screen.getByTestId('model-embedding-cancel'));

  expect(screen.queryByTestId('model-embedding-confirm-title')).toBeNull();
  expect(setEmbeddingModel).not.toHaveBeenCalled();
  // The option is still there to pick again: cancelling a question must not
  // take the choice away with it.
  expect(optionsFor('emb-2').length).toBe(1);
});

// 🔴 The confirmation used to offer three answers and one of them was never an
// answer. `Db::refuse_unless_every_other_space_is_empty` enumerates every space
// but the requested one — `None` for a model with no space yet — so ANY
// non-empty space refuses `Keep`, and that set is exactly "the estimate above is
// above zero". The button existed only to hand a cautious person a rejection
// and a raw backend sentence. `model_commands.rs`'s
// `changing_the_model_without_confirmation_leaves_the_space_alone` is the
// committed proof against the real index.
test('the confirmation offers no Keep, because the index would refuse it in exactly this state', async () => {
  await renderOnModel(onModel(7));

  await pickModel('emb-2');

  // Positively, on visible text: these two answers, in this order, and no
  // third. A `queryByTestId(...)` alone would pass against a button whose
  // testid was merely renamed.
  const answers = within(screen.getByTestId('model-embedding-confirm'))
    .getAllByRole('button')
    .map((b) => b.textContent);
  expect(answers).toEqual(['Discard the embeddings', 'Do not change the model']);
  expect(screen.queryByText('Keep the embeddings')).toBeNull();
});

test('Discard sends its own value, and nothing supplies one by default', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true, retired: [],
    index: onModel(0).index,
  });
  await renderOnModel();

  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));

  expect(setEmbeddingModel).toHaveBeenLastCalledWith('emb-2', 'discard');
});

// The other side of Ruling A: with nothing to lose there is nothing to ask
// about, and the value that goes on the wire is the one that REFUSES rather
// than destroys — it cannot refuse here, because what it refuses on is the
// count this branch has just read as zero.
test('an index holding nothing anywhere asks nothing and sends the value that refuses', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true, retired: [],
    index: onModel(0).index,
  });
  await renderOnModel(onModel(0, 0));

  await pickModel('emb-2');

  expect(screen.queryByTestId('model-embedding-confirm-title')).toBeNull();
  await waitFor(() => expect(setEmbeddingModel).toHaveBeenCalledWith('emb-2', 'keep'));
});

// The two counts differ here on purpose, the other way round from the estimate
// test: the ACTIVE space is empty and the index still holds seven embeddings
// elsewhere. A build that decided whether to ask from `embeddedChunks` would
// destroy those seven without asking anybody.
test('the question is asked from what the whole index holds, not from the active space alone', async () => {
  await renderOnModel(onModel(7, 0));

  await pickModel('emb-2');

  expect(screen.getByTestId('model-embedding-confirm-title').textContent).toBe(CONFIRM_TITLE);
  expect(setEmbeddingModel).not.toHaveBeenCalled();
});

// 🔴 The Global Constraint, verbatim: "what can a person lose by picking a
// different embedding model, and does the window say so BEFORE it happens?"
// The sentence naming the loss existed in both locales and was rendered only
// after the irreversible act — a report, never a warning.
test('the confirmation names the loss before it happens, above the button that causes it', async () => {
  await renderOnModel(onModel(7));

  await pickModel('emb-2');

  expect(screen.getByTestId('model-embedding-confirm-loss').textContent).toBe(CONFIRM_LOSS);
  const said = screen.getByTestId('model-embedding-confirm').textContent ?? '';
  expect(said.indexOf(CONFIRM_LOSS)).toBeGreaterThanOrEqual(0);
  expect(said.indexOf(CONFIRM_LOSS)).toBeLessThan(said.indexOf('Discard the embeddings'));
  // Still nothing called: the sentence is in the window that can be cancelled.
  expect(setEmbeddingModel).not.toHaveBeenCalled();
});

test('the sentence after the act reports what the index destroyed, not the estimate before it', async () => {
  // Seven before the act, four actually retired. The two numbers must not be
  // the same number, or "the first presented as the second" is unfalsifiable.
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true,
    retired: [{ spaceId: 1, embeddedChunks: 4 }],
    index: onModel(0).index,
  });
  await renderOnModel(onModel(7));

  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));

  await waitFor(() => expect(screen.getByTestId('model-embedding-retired')).toBeTruthy());
  const said = screen.getByTestId('model-embedding-retired').textContent;
  expect(said).toBe('The change discarded 4 embeddings from 1 vector space.');
  expect(said).not.toContain('7');
});

test('a change that retired nothing says so rather than saying nothing', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true, retired: [],
    index: onModel(0).index,
  });
  await renderOnModel(onModel(0));

  // Nothing to lose, so nothing is asked — the press goes straight through.
  await pickModel('emb-2');

  await waitFor(() => expect(screen.getByTestId('model-embedding-retired')).toBeTruthy());
  expect(screen.getByTestId('model-embedding-retired').textContent).toBe(
    'The change discarded nothing: no vector space was in its way.',
  );
});

test('after a successful change the section says search by meaning is dark and offers the re-embed', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true,
    retired: [{ spaceId: 1, embeddedChunks: 4 }],
    index: onModel(0).index,
  });
  const { container } = await renderOnModel(onModel(7));
  // The other direction first: nothing on an untouched section claims this.
  expect(screen.queryByTestId('model-embedding-degraded-note')).toBeNull();

  // What `model_settings` answers AFTER the change: the index is on a new
  // space and it holds nothing. The notice is read from that re-read and not
  // from the fact that a change happened — an index whose new space is already
  // full is not degraded, and a flag alone could not tell the two apart.
  modelSettings.mockResolvedValue(onModel(0, 0));

  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));

  await waitFor(() => expect(screen.getByTestId('model-embedding-degraded-note')).toBeTruthy());
  expect(screen.getByTestId('model-embedding-degraded-note').textContent).toBe(DEGRADED);
  const reembed = screen.getByRole('button', { name: 'Embed the index again' });
  expect(reembed.getAttribute('type')).toBe('button');

  await fireEvent.click(reembed);
  await waitFor(() => expect(startScanJob).toHaveBeenCalledWith('embedOnly'));
  // The sentence follows the SNAPSHOT, not the press: the section says a pass
  // is under way when the core says one is, and never on its own account.
  emit(runningScan());
  await tick();
  expect(screen.getByTestId('model-embedding-reembed-started').textContent).toBe(
    'Embedding has started.',
  );
  // The whole section, read as a person reads it: the two numbers stand in
  // their own sentences and the loss is named in words, not by an absence.
  const text = container.textContent ?? '';
  expect(text).toContain('The change discarded 4 embeddings from 1 vector space.');
  expect(text).toContain(DEGRADED);
});

/// Drives a section into the degraded state and presses the re-embed, and hands
/// back the callback the pass will report its ending to. `after` is what
/// `model_settings` answers from that point on — the index as the pass leaves
/// it, which is the whole question the ending is worth re-reading for.
async function reembedding(after: ModelSettings) {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true,
    retired: [{ spaceId: 1, embeddedChunks: 4 }],
    index: onModel(0).index,
  });
  const rendered = await renderOnModel(onModel(7));
  modelSettings.mockResolvedValue(onModel(0, 0));

  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));
  await waitFor(() => expect(screen.getByTestId('model-embedding-degraded-note')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('model-embedding-reembed'));
  await waitFor(() => expect(startScanJob).toHaveBeenCalledWith('embedOnly'));
  // The pass is under way as far as the core is concerned, which is the only
  // account this section has of it.
  emit(runningScan());
  await tick();

  modelSettings.mockResolvedValue(after);
  // 🔴 A whole STATE, not a bare call: a build that re-read the index on every
  // change to the snapshot would pass a helper that only ever delivered one.
  return { ...rendered, ended: () => emit(endedScan()), progress: () => emit(runningScan()) };
}

// 🔴 Both directions on WHICH phase the sentence is about. A scan is one job
// with two phases now, so "a job is running" and "the embedding is running" are
// no longer the same fact: a reading pass over somebody's folders says nothing
// about the chunks this section is waiting on, and drawing "embedding has
// started" for it would promise a repair that has not begun.
//
// The same for the ended sentence: a scan stopped in the READING phase never
// reached the embedding at all (`ScanReport.endedIn`), so it is not this
// section's ending either.
test('the reading phase is not the embedding, and neither is an ending that never reached it', async () => {
  const { ended } = await reembedding(onModel(0, 0));

  emit(readingScan());
  await tick();
  expect(screen.queryByTestId('model-embedding-reembed-started')).toBeNull();
  expect(screen.queryByTestId('model-embedding-reembed-ended')).toBeNull();

  emit({
    ...IDLE_SCAN,
    revision: (revision += 1),
    snapshot: {
      kind: 'ended',
      report: {
        embedding: { kind: 'notReached' }, endedIn: 'reading',
        reason: 'cancelled', message: null, resume: 'full',
      },
    },
  });
  await tick();
  expect(screen.queryByTestId('model-embedding-reembed-ended')).toBeNull();

  // …and the states that ARE this section's own still say so.
  emit(runningScan());
  await tick();
  expect(screen.getByTestId('model-embedding-reembed-started')).toBeTruthy();

  ended();
  await waitFor(() => expect(screen.getByTestId('model-embedding-reembed-ended')).toBeTruthy());
});

// 🔴 The pass used to report to nobody: `reembed()` set a flag and stopped,
// with no listener and no poll, so a pass that SUCCEEDED left "search by
// meaning is unavailable" on screen for the rest of the session.
test('an ended pass that filled the index takes the degraded notice away with it', async () => {
  const { ended } = await reembedding(onModel(5, 5));

  ended();

  await waitFor(() => expect(screen.queryByTestId('model-embedding-degraded-note')).toBeNull());
  expect(screen.queryByTestId('model-embedding-reembed')).toBeNull();
  // Positively, and from the index rather than from the ending: the section now
  // draws a connected installation with a model chosen.
  expect(screen.getByTestId('model-status-dot').getAttribute('data-active')).toBe('true');
});

// The subscription receives every state, so the guard that keeps the re-read to
// an ENDING is a live branch: a section that re-fetched on every emission of
// the store passes the test above and fails this one, and this is the only
// thing that tells the two apart.
test('a progress report from the pass does not end it and does not re-read the index', async () => {
  const { progress } = await reembedding(onModel(5, 5));
  const readsBefore = modelSettings.mock.calls.length;

  progress();
  await tick();

  expect(screen.getByTestId('model-embedding-reembed-started')).toBeTruthy();
  expect(screen.queryByTestId('model-embedding-reembed-ended')).toBeNull();
  expect(screen.getByTestId('model-embedding-degraded-note').textContent).toBe(DEGRADED);
  expect(modelSettings.mock.calls.length).toBe(readsBefore);
});

// The other direction, and the one the flag could never express: a pass that
// ended having filled nothing. The person used to be told nothing at all.
test('an ended pass that filled nothing says the search is still dark, rather than nothing', async () => {
  const { ended } = await reembedding(onModel(0, 0));

  ended();

  await waitFor(() => expect(screen.getByTestId('model-embedding-reembed-ended')).toBeTruthy());
  expect(screen.getByTestId('model-embedding-reembed-ended').textContent).toBe(REEMBED_ENDED);
  // The claim that sentence makes about the state is still true beside it.
  expect(screen.getByTestId('model-embedding-degraded-note').textContent).toBe(DEGRADED);
  // And the sentence it replaces is gone: a pass cannot be starting and over.
  expect(screen.queryByTestId('model-embedding-reembed-started')).toBeNull();
});

// 🔴 Rendered below the degraded block, the dot had the last word — the section
// ended "Search by meaning is unavailable…" and then "Connected — …a chosen
// embedding model are all set." Both sentences are true; read in that order the
// second is a promise the product cannot keep.
test('the connected dot does not have the last word over a search that has gone dark', async () => {
  const { container } = await reembedding(onModel(0, 0));

  const text = container.textContent ?? '';
  // Both are on screen — the dot is not hidden, and the ruling on `providerReady`
  // is untouched.
  expect(text).toContain(READY);
  expect(text).toContain(DEGRADED);
  expect(text.indexOf(READY)).toBeLessThan(text.indexOf(DEGRADED));
});

// The other direction of the degraded notice, and it is not the untouched
// section: a change HAS landed, and the space the index now points at is not
// empty. Nothing is dark, so nothing may say it is — a notice conditioned on
// "a change happened" alone would fire here.
test('a change that leaves the new space full says nothing about a search going dark', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true, retired: [],
    index: onModel(5, 5).index,
  });
  await renderOnModel(onModel(5, 5));
  modelSettings.mockResolvedValue(onModel(5, 5));

  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));

  await waitFor(() => expect(screen.getByTestId('model-embedding-retired')).toBeTruthy());
  expect(screen.queryByTestId('model-embedding-degraded-note')).toBeNull();
  expect(screen.queryByTestId('model-embedding-reembed')).toBeNull();
});

// ---------------------------------------------------------------------------
// PR 25 review, P1-1 — the degraded state is a FACT about the index, not a
// memory of what this component did.
//
// It used to open with `changeLanded`, set when a change landed in this
// component — and this component is destroyed by every section switch. A person
// who discarded their vectors, went to Folders and came back met a fresh
// instance with the flag at false and an index the backend still reported as
// empty: the warning and the button that repairs it were gone, and the
// connected dot was left as the last word on a search that had gone dark.
//
// The three conjuncts get a fixture each, because a fixture that moves two axes
// at once cannot say which one the code read.
// ---------------------------------------------------------------------------

test('an index whose active space is empty says so on a section that has just been mounted', async () => {
  // No press, no change, no history — this section has done nothing at all.
  // `onModel(0, 0)` is an index on a model, holding 12 chunks of document text,
  // with nothing embedded in the space it points at.
  await renderOnModel(onModel(0, 0));

  expect(screen.getByTestId('model-embedding-degraded-note').textContent).toBe(DEGRADED);
  expect(screen.getByRole('button', { name: 'Embed the index again' })).toBeTruthy();
  // And the report of a change is NOT invented alongside it: nothing was
  // discarded here, so nothing may say anything was.
  expect(screen.queryByTestId('model-embedding-retired')).toBeNull();
});

test('an index that holds no documents at all is not reported as a search gone dark', async () => {
  // The other direction of `totalChunks`, and the one an unconditional notice
  // satisfies: a model is chosen and the active space is empty, exactly as
  // above — but there is nothing to embed, so the empty space is not a loss and
  // there is nothing for the button to repair.
  await renderOnModel(onModel(0, 0, 0));

  expect(screen.queryByTestId('model-embedding-degraded-note')).toBeNull();
  expect(screen.queryByTestId('model-embedding-reembed')).toBeNull();
});

test('an index with documents and no model chosen says it is not connected, not that a search went dark', async () => {
  // The direction of the model conjunct. The base fixture holds six chunks and
  // has adopted nothing, so the active space does not exist rather than being
  // empty — `models.rs` says on the count itself that this is "the question
  // does not arise". The screen answers it with the dot, in words a person can
  // act on, and says nothing about a loss.
  await renderWith(settings());

  expect(screen.queryByTestId('model-embedding-degraded-note')).toBeNull();
  expect(screen.getByTestId('model-status-dot').textContent).toBe(
    'Not connected yet — add a key and choose an embedding model to enable content search.',
  );
});

// PR 25 review, P1-2. The pass this section starts belongs to the window, not
// to this component: started with a listener of its own it reported to nobody
// the moment somebody clicked another section, and the strip stayed idle while
// the backend job ran on. Asserted here on the controller's own state — the
// value the strip is drawn from — rather than on the mock alone, which a
// component that called the command directly would satisfy just as well.
test('the recovery pass is started through the window`s controller, not privately', async () => {
  const jobs = createJobController();
  jobs.mount();
  mockCatalogues({
    embedding: catalogueOf([entry('emb-1', { name: 'Embedder One' })]),
  });
  setLocale('en');
  modelSettings.mockResolvedValue(onModel(0, 0));
  render(Models, { props: { jobs } });
  await waitFor(() => expect(screen.getByTestId('model-embedding-reembed')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('model-embedding-reembed'));

  await waitFor(() => expect(startScanJob).toHaveBeenCalledWith('embedOnly'));
  // The controller is watching it: this is the state `JobStrip.svelte` draws
  // the pass line and the Stop from, and it comes from the CORE — the window
  // no longer writes a phase of its own in anticipation of a job it has asked
  // for and not yet heard from.
  emit(runningScan());
  expect(get(jobs.state).scan.snapshot).toEqual({
    kind: 'running',
    cancellable: true,
    phase: {
      kind: 'embedding',
      counts: { done: 1, total: 4, skipped: 0, refused: 0, contended: 0, secondsLeft: null },
    },
  });
});

test('a rejection shows the backend sentence verbatim and branches from a re-read of the state', async () => {
  const SENTENCE = 'the index is not open';
  setEmbeddingModel.mockRejectedValue(new Error(SENTENCE));
  // The second read is what the section decides from — the message text says
  // nothing about `readFailed`, and nothing here parses it.
  modelSettings
    .mockResolvedValueOnce(onModel(7))
    .mockResolvedValue(settings({
      key: { kind: 'present' },
      index: { kind: 'unreadable', cause: 'readFailed', reason: 'LEAK-TOKEN-REJECTION' },
    }));
  mockCatalogues({
    embedding: catalogueOf([entry('emb-1', { name: 'Embedder One' }), entry('emb-2', { name: 'Embedder Two' })]),
  });
  renderModels();
  await waitFor(() => expect(optionsFor('emb-2').length).toBe(1));

  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));

  await waitFor(() => expect(screen.getByTestId('model-embedding-error')).toBeTruthy());
  expect(screen.getByTestId('model-embedding-error').textContent).toBe(SENTENCE);
  expect(screen.getByTestId('model-embedding-failed').textContent).toBe(CHANGE_FAILED);
  // The branch, taken from the re-read and not from the sentence.
  await waitFor(() =>
    expect(screen.getByTestId('model-index-failure').textContent).toBe(
      'The index could not be read — this is a defect in this build.',
    ),
  );
  // …and `reason` never reaches the screen.
  expect(screen.queryByText(/LEAK-TOKEN-REJECTION/)).toBeNull();
});

test('a rejection because a job is running leaves that job drawn as running', async () => {
  setEmbeddingModel.mockRejectedValue(new Error('a job is already running'));
  jobStatus.mockResolvedValue(runningScan());
  await renderOnModel();

  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));

  await waitFor(() => expect(screen.getByTestId('model-job-running')).toBeTruthy());
  expect(screen.getByTestId('model-job-running').textContent).toBe(JOB_RUNNING);
  expect(screen.getByTestId('model-embedding-error').textContent).toBe('a job is already running');
});

// 🔴 The two tests below exist because the pair above and below them, on their
// own, were **measured green** against a component that read `changeError` for
// the words "already running" instead of asking `job_status` — the exact
// failure `error.rs`'s own header exists to prevent, passing every assertion
// because the fixture let the sentence and the state agree.
//
// The state a fixture must build to tell the two apart is one where they
// DISAGREE, and both directions of the disagreement are real: a change refused
// for a reason that has nothing to do with a job, while a walk is running; and
// a rejection whose wording mentions a job that has since ended.
test('a job that IS running is drawn, even when the rejection never mentions one', async () => {
  setEmbeddingModel.mockRejectedValue(new Error('no key has been entered'));
  jobStatus.mockResolvedValue(runningScan());
  await renderOnModel();

  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));

  await waitFor(() => expect(screen.getByTestId('model-job-running')).toBeTruthy());
  expect(screen.getByTestId('model-job-running').textContent).toBe(JOB_RUNNING);
  expect(jobStatus).toHaveBeenCalled();
});

test('a job that is NOT running is not drawn as one, whatever the rejection said', async () => {
  setEmbeddingModel.mockRejectedValue(new Error('a job is already running'));
  jobStatus.mockResolvedValue(IDLE_SCAN);
  await renderOnModel();

  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));

  await waitFor(() => expect(screen.getByTestId('model-embedding-error')).toBeTruthy());
  expect(screen.queryByTestId('model-job-running')).toBeNull();
  // The sentence is still shown verbatim: not drawing a job is not the same as
  // hiding what the backend said.
  expect(screen.getByTestId('model-embedding-error').textContent).toBe('a job is already running');
});

test('ReadFailed offers choosing a model again as the recovering act, and presses it without asking', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true, retired: [],
    index: onModel(0).index,
  });
  mockCatalogues({
    embedding: catalogueOf([entry('emb-1', { name: 'Embedder One' }), entry('emb-2', { name: 'Embedder Two' })]),
  });
  await renderWith(settings({
    key: { kind: 'present' },
    index: { kind: 'unreadable', cause: 'readFailed', reason: 'r' },
  }));
  await waitFor(() => expect(optionsFor('emb-2').length).toBe(1));

  expect(screen.getByTestId('model-index-recover').textContent).toBe(RECOVER);

  await pickModel('emb-2');

  // No estimate can be stated about an index that will not answer, so nothing
  // is asked — and `keep` is the value that refuses rather than destroys.
  expect(screen.queryByTestId('model-embedding-confirm-title')).toBeNull();
  await waitFor(() => expect(setEmbeddingModel).toHaveBeenCalledWith('emb-2', 'keep'));
});

test('a healthy index is offered no recovering act, and its presses ask first', async () => {
  await renderOnModel();

  expect(screen.queryByTestId('model-index-recover')).toBeNull();

  await pickModel('emb-2');
  expect(screen.getByTestId('model-embedding-confirm-title').textContent).toBe(CONFIRM_TITLE);
  expect(setEmbeddingModel).not.toHaveBeenCalled();
});

test('an index that was never opened is offered no recovering act either', async () => {
  await renderWith(settings({
    key: { kind: 'present' },
    index: { kind: 'unreadable', cause: 'notOpen', reason: 'r' },
  }));
  expect(screen.getByTestId('model-index-failure').textContent).toBe('The index is not open yet.');
  expect(screen.queryByTestId('model-index-recover')).toBeNull();
});

test('switching tabs takes a pending question with it', async () => {
  await renderOnModel();
  await pickModel('emb-2');
  expect(screen.getByTestId('model-embedding-confirm-title')).toBeTruthy();

  await fireEvent.click(screen.getByTestId('model-tab-chat'));

  expect(screen.queryByTestId('model-embedding-confirm-title')).toBeNull();
  expect(setEmbeddingModel).not.toHaveBeenCalled();
});

// The same argument as the line above it, applied to the two sentences a press
// leaves BEHIND. "The change discarded 4 embeddings…" under the chat list is a
// report about an act on a list that is not on screen any more.
test('switching tabs takes the report of what was discarded with it', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true,
    retired: [{ spaceId: 1, embeddedChunks: 4 }],
    index: onModel(0).index,
  });
  await renderOnModel(onModel(7, 7));
  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));
  await waitFor(() => expect(screen.getByTestId('model-embedding-retired')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('model-tab-chat'));

  expect(screen.queryByTestId('model-embedding-retired')).toBeNull();
});

test('switching tabs takes a rejection with it', async () => {
  setEmbeddingModel.mockRejectedValue(new Error('a job is already running'));
  jobStatus.mockResolvedValue(runningScan());
  await renderOnModel(onModel(7, 7));
  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));
  await waitFor(() => expect(screen.getByTestId('model-job-running')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('model-tab-chat'));

  expect(screen.queryByTestId('model-embedding-failed')).toBeNull();
  expect(screen.queryByTestId('model-embedding-error')).toBeNull();
  expect(screen.queryByTestId('model-job-running')).toBeNull();
});

// --- the six `void $locale` guards this task added, each reddening alone -----

test('a language switch after mount reaches the confirmation', async () => {
  const { container } = await renderOnModel(onModel(7));
  await pickModel('emb-2');
  expect((container.textContent ?? '')).toContain(CONFIRM_TITLE);

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('Змінити модель ембедингу?');
  expect(after).toContain('Зараз індекс містить 7 ембедингів');
  expect(after).toContain('Ці ембединги неможливо перенести');
  expect(after).toContain('Відкинути ембединги');
  expect(after).not.toContain(CONFIRM_TITLE);
  expect(after).not.toContain(CONFIRM_LOSS);
});

test('a language switch after mount reaches the sentence about what was discarded', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true,
    retired: [{ spaceId: 1, embeddedChunks: 4 }],
    index: onModel(0).index,
  });
  const { container } = await renderOnModel(onModel(7));
  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));
  await waitFor(() => expect(screen.getByTestId('model-embedding-retired')).toBeTruthy());
  expect((container.textContent ?? '')).toContain('The change discarded 4 embeddings');

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('Зміна відкинула 4 ембединги');
  expect(after).not.toContain('The change discarded 4 embeddings');
});

test('a language switch after mount reaches the degraded notice and its button', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true, retired: [],
    index: onModel(0).index,
  });
  const { container } = await renderOnModel(onModel(0));
  await pickModel('emb-2');
  await waitFor(() => expect(screen.getByTestId('model-embedding-degraded-note')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('model-embedding-reembed'));
  // The sentence follows the snapshot, so the pass has to be under way as far
  // as the core is concerned before there is anything to translate.
  emit(runningScan());
  await waitFor(() => expect(screen.getByTestId('model-embedding-reembed-started')).toBeTruthy());
  expect((container.textContent ?? '')).toContain(DEGRADED);

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('Пошук за змістом недоступний');
  expect(after).toContain('Вбудувати індекс наново');
  expect(after).toContain('Вбудовування почалося.');
  expect(after).not.toContain(DEGRADED);
});

test('a language switch after mount reaches the ended-pass sentence', async () => {
  const { container, ended } = await reembedding(onModel(0, 0));
  ended();
  await waitFor(() => expect(screen.getByTestId('model-embedding-reembed-ended')).toBeTruthy());
  expect((container.textContent ?? '')).toContain(REEMBED_ENDED);

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('Вбудовування завершилося');
  expect(after).not.toContain(REEMBED_ENDED);
});

test('a language switch after mount reaches the lead-in above a rejection', async () => {
  setEmbeddingModel.mockRejectedValue(new Error('a job is already running'));
  const { container } = await renderOnModel();
  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));
  await waitFor(() => expect(screen.getByTestId('model-embedding-failed')).toBeTruthy());
  expect((container.textContent ?? '')).toContain(CHANGE_FAILED);

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('Модель ембедингу не прийнято.');
  expect(after).not.toContain(CHANGE_FAILED);
  // The backend's own sentence is not a catalogue string and must survive the
  // switch unchanged — a language switch may not translate what it did not write.
  expect(after).toContain('a job is already running');
});

test('a language switch after mount reaches the running-job line', async () => {
  setEmbeddingModel.mockRejectedValue(new Error('a job is already running'));
  jobStatus.mockResolvedValue(runningScan());
  const { container } = await renderOnModel();
  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));
  await waitFor(() => expect(screen.getByTestId('model-job-running')).toBeTruthy());
  expect((container.textContent ?? '')).toContain(JOB_RUNNING);

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('Триває завдання індексації.');
  expect(after).not.toContain(JOB_RUNNING);
});

test('a language switch after mount reaches the recovering-act sentence', async () => {
  const { container } = await renderWith(settings({
    key: { kind: 'present' },
    index: { kind: 'unreadable', cause: 'readFailed', reason: 'r' },
  }));
  expect((container.textContent ?? '')).toContain(RECOVER);

  await switchTo('uk');
  const after = container.textContent ?? '';
  expect(after).toContain('Повторний вибір моделі ембедингу це виправляє');
  expect(after).not.toContain(RECOVER);
});

// ---------------------------------------------------------------------------
// Live run, finding 1 — a label and its value read as one broken phrase.
//
// `Key A key is saved.` and `Provider OpenRouter` were on a real screen while
// this file was green, because every assertion above reads a testid, an
// attribute or one element's own text. Nothing had ever read the two ELEMENTS
// TOGETHER, which is the only way the seam between them is visible — and there
// is no CSS in this project to put anything in that seam.
//
// What a person reads, with the markup's own indentation collapsed the way a
// browser collapses it (JobStrip.test.ts:79 uses the same normaliser, and for
// the same reason: nobody sees the newline between two <span>s).
// ---------------------------------------------------------------------------
const visible = (el: Element | null) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();

for (const [loc, provider, keyRow] of [
  ['en', 'Provider: OpenRouter', 'Key: A key is saved.'],
  ['uk', 'Провайдер: OpenRouter', 'Ключ: Ключ збережено.'],
] as const) {
  test(`the label and its value read as two things, not one phrase (${loc})`, async () => {
    const { container } = await renderInLocale(loc, settings({ key: { kind: 'present' } }));
    const text = visible(container);

    // Positive: the separator is on the screen, in this locale, between each
    // label and the value it introduces.
    expect(text).toContain(provider);
    expect(text).toContain(keyRow);
    // And the other direction, which is the defect stated literally: the label
    // and its value with nothing but a space between them. This is the half
    // that dies when the colon is taken back out of the catalogue — the
    // positive half above would survive a separator moved anywhere else on the
    // line.
    expect(text).not.toContain(provider.replace(': ', ' '));
    expect(text).not.toContain(keyRow.replace(': ', ' '));
  });
}

// The same seam one group lower, on a branch the live run never reached: the
// index subject label and the sentence under it. `Unreadable` is the only state
// that renders either.
test('the index label and its sentence read as two things too', async () => {
  const { container } = await renderWith(settings({
    index: { kind: 'unreadable', cause: 'notOpen', reason: 'r' },
  }));
  const text = visible(container);
  expect(text).toContain('Index: The index is not open yet.');
  expect(text).not.toContain('Index The index is not open yet.');

  // Both locales, because the separator is a per-locale decision written into
  // each string: the English half alone left the Ukrainian label's colon
  // defended by nothing, and removing it was green across the whole suite.
  await switchTo('uk');
  const uk = visible(container);
  expect(uk).toContain('Індекс: Індекс ще не відкрито.');
  expect(uk).not.toContain('Індекс Індекс ще не відкрито.');
});

// And one row lower again, on the branch a provider listing a model this
// build refuses would reach. Rewritten (owner's ruling, live run,
// 2026-09-10): the name and the reason used to be folded into one option
// label (Task 4) and this test asserted they still read as two things
// rather than one run-together phrase — there is no longer an option to
// glue them onto at all, so the two things this now asserts are that the
// model has NO option (its name reaches nowhere on screen) and that its
// reason stands as its own, distinct line below the select.
test('a refused model has no option, and its reason renders as its own line below the select', async () => {
  mockCatalogues({
    embedding: catalogueOf([entry('r1', { name: 'Tiny One', refusal: { kind: 'noStatedLimit' } })]),
  });
  const { container } = await renderWith(settings());
  await waitFor(() => expect(screen.getByTestId('model-hidden-reason')).toBeTruthy());
  const text = visible(container);

  expect(optionsFor('r1').length).toBe(0);
  expect(text).not.toContain('Tiny One');
  expect(screen.getByTestId('model-hidden-reason').textContent).toBe(
    '1 hidden: the provider states no input limit',
  );
});

// ---------------------------------------------------------------------------
// Task 4 (Task 4 brief, plan review P1/P2-1/P2-2) — the select replaces the
// old row of buttons, and these are the behaviours that mechanism owes on
// top of what the row already gave: a command's own confirmed result and the
// following read are two DISTINCT sources of truth, neither is assumed from
// the other, and the DOM value the person actually sees is put back in sync
// on every change regardless of what Svelte's own dirty check thinks moved.
// ---------------------------------------------------------------------------

// The task's own P1 counter-example, verbatim: an index on a model WITH
// vectors, a rejected adoption, and a re-read that comes back unreadable.
// Neither the model the index was on nor the one just picked may be shown as
// current without a read that actually says so — the failure is reported,
// the recovery catalogue stays, and the private diagnostic never reaches the
// screen.
test('failed_adoption_does_not_restore_cached_model', async () => {
  setEmbeddingModel.mockRejectedValue(new Error('Embeddings were removed; adoption failed.'));
  modelSettings.mockResolvedValueOnce(onModel(7)).mockResolvedValue(settings({
    key: { kind: 'present' },
    index: { kind: 'unreadable', cause: 'readFailed', reason: 'PRIVATE-DIAGNOSTIC' },
  }));
  mockCatalogues({ embedding: catalogueOf([entry('emb-1'), entry('emb-2')]) });
  renderModels();
  const select = await screen.findByTestId('model-selection');
  await fireEvent.change(select, { target: { value: 'emb-2' } });
  await fireEvent.click(await screen.findByTestId('model-embedding-discard'));
  await waitFor(() => expect(screen.getByTestId('model-index-failure')).toBeTruthy());
  expect((screen.getByTestId('model-selection') as HTMLSelectElement).value).toBe('');
  expect(screen.getByTestId('model-embedding-error').textContent).toContain('Embeddings were removed');
  expect(screen.queryByText(/PRIVATE-DIAGNOSTIC/)).toBeNull();
});

// The other counter-example the test above cannot reach: the recovery
// re-read `commitEmbedding`'s catch block awaits does not merely come back
// `unreadable`, it REJECTS outright. `refresh()`'s own catch branch never
// touches `writeOutcome` (only its success branch resets it to `null`), so
// whatever the write's own catch set `writeOutcome` to is what stays on
// screen with nothing left to reconcile it — this is the one scenario
// where a mutant that has the write's catch restore the just-picked (or
// any cached) model instead of `unknown` is actually OBSERVABLE: the test
// above's own re-read SUCCEEDS (as `unreadable`), so `refresh()` still
// resets `writeOutcome` to `null` there and `currentEmbeddingModel`
// (derived from `settings.index.kind === 'read'`) decides the outcome
// instead, never crossing the write's own line at all.
test('a rejected adoption whose own re-read also fails does not restore any cached model', async () => {
  const SENTENCE = 'Embeddings were removed; adoption failed.';
  setEmbeddingModel.mockRejectedValue(new Error(SENTENCE));
  await renderOnModel(); // mount succeeds — settings holds a model WITH vectors (emb-1)
  modelSettings.mockRejectedValue(new Error('model_settings unreachable')); // the recovery re-read itself fails

  await pickModel('emb-2');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));

  await waitFor(() => expect(screen.getByTestId('model-embedding-error')).toBeTruthy());
  expect(screen.getByTestId('model-embedding-error').textContent).toBe(SENTENCE);
  // Neither model — the one the (now stale) read named, nor the one just
  // picked — is shown as current: the write's own `unknown` outcome is what
  // a failed recovery re-read leaves standing.
  expect((screen.getByTestId('model-selection') as HTMLSelectElement).value).toBe('');
});

// The other half of the same rule: a command that SUCCEEDS must not be
// reported as a rejection just because the read that follows it fails
// outright (a plain IPC rejection, not an `Unreadable` index) — command and
// refresh are two separate try/catch blocks, on purpose.
test('successful_write_with_failed_refresh_keeps_acknowledged_model', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true,
    retired: [{ spaceId: 1, embeddedChunks: 4 }],
    index: onModel(0).index,
  });
  modelSettings.mockResolvedValueOnce(onModel(7)).mockRejectedValue(new Error('model_settings unreachable'));
  mockCatalogues({ embedding: catalogueOf([entry('emb-1'), entry('emb-2')]) });
  renderModels();
  await waitFor(() => expect(optionsFor('emb-2').length).toBe(1));

  await pickModel('emb-2');
  await fireEvent.click(await screen.findByTestId('model-embedding-discard'));

  // The command succeeded — shown immediately, without waiting for (or being
  // undone by) the read that follows it.
  await waitFor(() => expect(modelSelect().value).toBe('emb-2'));
  expect(screen.queryByTestId('model-embedding-failed')).toBeNull();
  expect(screen.queryByTestId('model-embedding-error')).toBeNull();
  // The read's own failure is still reported — through the ordinary
  // `loadError` banner, not as a rejected write.
  expect(screen.getByTestId('model-load-failure')).toBeTruthy();
});

// The same scenario, the other thing it owes: the retirement report a
// successful command already handed back must not be erased by the failed
// read that follows it — `retiredReport` is set from the command's own
// reply, not from a read that never confirmed it.
test('failed_read_does_not_erase_retirement_report', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true,
    retired: [{ spaceId: 1, embeddedChunks: 4 }],
    index: onModel(0).index,
  });
  modelSettings.mockResolvedValueOnce(onModel(7)).mockRejectedValue(new Error('model_settings unreachable'));
  mockCatalogues({ embedding: catalogueOf([entry('emb-1'), entry('emb-2')]) });
  renderModels();
  await waitFor(() => expect(optionsFor('emb-2').length).toBe(1));

  await pickModel('emb-2');
  await fireEvent.click(await screen.findByTestId('model-embedding-discard'));

  await waitFor(() => expect(screen.getByTestId('model-embedding-retired')).toBeTruthy());
  expect(screen.getByTestId('model-embedding-retired').textContent).toBe(
    'The change discarded 4 embeddings from 1 vector space.',
  );
});

// Review round 1, Important 1. The same "acknowledged B, its own re-read
// failed" state as the pair above, but this time picking a DIFFERENT model —
// specifically the one `settings` (the last successful read, now stale)
// still names, `emb-1`. Comparing the pick against `currentEmbeddingModel`
// (the stale read) rather than against what the select actually shows would
// treat this as "no change" and silently swallow a pick that is a real one:
// the select is showing `emb-2`, and the person is deliberately moving it
// back.
test('picking the model a stale settings read still names, while the select shows an acknowledged different one, is not swallowed', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true,
    retired: [{ spaceId: 1, embeddedChunks: 4 }],
    index: onModel(0).index,
  });
  modelSettings.mockResolvedValueOnce(onModel(7)).mockRejectedValue(new Error('model_settings unreachable'));
  mockCatalogues({ embedding: catalogueOf([entry('emb-1'), entry('emb-2')]) });
  renderModels();
  await waitFor(() => expect(optionsFor('emb-2').length).toBe(1));

  await pickModel('emb-2');
  await fireEvent.click(await screen.findByTestId('model-embedding-discard'));
  await waitFor(() => expect(modelSelect().value).toBe('emb-2')); // acknowledged, shown at once

  // Picking `emb-1` now: it equals the stale `settings` read (still 'emb-1',
  // since the confirming re-read rejected), but NOT what the select shows.
  // A real question about a real change, not a no-op.
  await pickModel('emb-1');
  expect(screen.getByTestId('model-embedding-confirm-title')).toBeTruthy();
});

// Review round 1, Important 1, the other half: picking the SAME model the
// select already shows (a genuine no-op) must not have already cleared the
// previous round's own report on its way to deciding that — the clearing and
// the no-op check must not run in the wrong order.
test('picking the model the select already shows is a real no-op, and does not erase the previous rounds report', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true,
    retired: [{ spaceId: 1, embeddedChunks: 4 }],
    index: onModel(0).index,
  });
  modelSettings.mockResolvedValueOnce(onModel(7)).mockRejectedValue(new Error('model_settings unreachable'));
  mockCatalogues({ embedding: catalogueOf([entry('emb-1'), entry('emb-2')]) });
  renderModels();
  await waitFor(() => expect(optionsFor('emb-2').length).toBe(1));

  await pickModel('emb-2');
  await fireEvent.click(await screen.findByTestId('model-embedding-discard'));
  await waitFor(() => expect(modelSelect().value).toBe('emb-2'));
  const retiredText = screen.getByTestId('model-embedding-retired').textContent;

  await pickModel('emb-2'); // the very model already shown — a real no-op

  expect(setEmbeddingModel).toHaveBeenCalledTimes(1); // no second command
  expect(screen.getByTestId('model-embedding-retired').textContent).toBe(retiredText);
});

// An unreadable index has nothing to estimate, so a choice made from it asks
// nothing and sends `keep` — and the select owes two things across that
// round: the placeholder before the pick (never the first catalogue entry),
// and the acknowledged model shown right after, without waiting on the
// recovery re-read this index cannot currently answer either.
test('unreadable_recovery_uses_keep', async () => {
  setEmbeddingModel.mockResolvedValue({
    model: 'emb-2', dim: 1024, spaceId: 2, created: true, retired: [],
    index: onModel(0).index,
  });
  mockCatalogues({ embedding: catalogueOf([entry('emb-1'), entry('emb-2')]) });
  await renderWith(settings({
    key: { kind: 'present' },
    index: { kind: 'unreadable', cause: 'readFailed', reason: 'r' },
  }));
  await waitFor(() => expect(optionsFor('emb-2').length).toBe(1));
  expect(modelSelect().value).toBe('');

  // The recovery re-read this adoption triggers finally answers — the index
  // is open again, on the model just chosen.
  modelSettings.mockResolvedValue(settings({
    key: { kind: 'present' },
    index: { kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0, embeddingModel: 'emb-2', chatModel: null, searchTextArm: true, searchContentArm: true },
  }));
  await pickModel('emb-2');

  expect(screen.queryByTestId('model-embedding-confirm-title')).toBeNull();
  await waitFor(() => expect(setEmbeddingModel).toHaveBeenCalledWith('emb-2', 'keep'));
  await waitFor(() => expect(modelSelect().value).toBe('emb-2'));
});

// A native `<select>` with no option explicitly selected defaults to the
// FIRST one — the browser's own rule, not this build's claim. An index that
// read successfully and simply has no role model chosen must not be shown as
// though it had picked whichever entry the catalogue happens to list first.
test('unknown_model_is_not_the_first_option', async () => {
  mockCatalogues({ embedding: catalogueOf([entry('emb-1'), entry('emb-2')]) });
  await renderWith(settings()); // base fixture: index read, embeddingModel: null
  await waitFor(() => expect(optionsFor('emb-1').length).toBe(1));

  expect(modelSelect().value).toBe('');
  expect(modelSelect().value).not.toBe('emb-1');
});

// A confirmed id the active catalogue does NOT list — a provider that
// retired a model this index still points at. It gets its OWN disabled
// option, carrying that id, rather than collapsing into either the blank
// "nothing chosen" placeholder or (worse) one of the entries this build
// still happens to offer.
test('a confirmed model absent from the catalogue gets its own placeholder option, not the first entry', async () => {
  mockCatalogues({ embedding: catalogueOf([entry('emb-1'), entry('emb-2')]) });
  await renderWith(settings({
    key: { kind: 'present' },
    index: {
      kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null,
      embeddedChunks: 7, embeddedChunksEverywhere: 7, totalChunks: 12, embeddingModel: 'retired-model', chatModel: null,
      searchTextArm: true, searchContentArm: true,
    },
  }));
  await waitFor(() => expect(optionsFor('emb-1').length).toBe(1));

  expect(modelSelect().value).toBe('retired-model');
  const placeholder = optionFor('retired-model');
  expect(placeholder.disabled).toBe(true);
  expect(placeholder.textContent).toContain('retired-model');
  // Neither catalogue entry silently absorbed the selection.
  expect(optionFor('emb-1').selected).toBe(false);
});

// The per-role dot (review P2-1): its own predicate, not a shared boolean.
// Embedding is fully configured; chat is not, from the very same `settings`
// — a fixture that moved both at once could not tell which field either dot
// actually reads.
test('configuration_dots_use_their_own_role', async () => {
  mockCatalogues({
    embedding: catalogueOf([entry('emb-1')]),
    chat: catalogueOf([entry('chat-1')]),
  });
  await renderWith(settings({
    key: { kind: 'present' },
    index: {
      kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null,
      embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0,
      embeddingModel: 'emb-1', chatModel: null, searchTextArm: true, searchContentArm: true,
    },
  }));
  await waitFor(() => expect(screen.getByTestId('model-dot-embedding')).toBeTruthy());

  expect(screen.getByTestId('model-dot-embedding').getAttribute('data-configured')).toBe('true');
  expect(screen.getByTestId('model-dot-chat').getAttribute('data-configured')).toBe('false');
  expect(screen.getByTestId('model-dot-embedding').textContent).toBe('Configured');
  expect(screen.getByTestId('model-dot-chat').textContent).toBe('Not configured');
});

// Task 9 (owner's ruling, live run 2026-09-10): the dot moved inside its own
// tab button and the visible word beside it is gone — the button's ACCESSIBLE
// name is where the state word has to survive, sr-only span and all. Whether
// that span is actually invisible to a sighted reader is a computed-style
// claim, guarded in `tokens.test.ts` instead — this test is only the name.
test('tab_button_names_include_the_configured_state', async () => {
  mockCatalogues({
    embedding: catalogueOf([entry('emb-1')]),
    chat: catalogueOf([entry('chat-1')]),
  });
  await renderWith(settings({
    key: { kind: 'present' },
    index: {
      kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null,
      embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0,
      embeddingModel: 'emb-1', chatModel: null, searchTextArm: true, searchContentArm: true,
    },
  }));
  await waitFor(() => expect(screen.getByTestId('model-dot-embedding')).toBeTruthy());

  expect(screen.getByRole('button', { name: /Embedding.*Configured/ })).toBeTruthy();
  expect(screen.getByRole('button', { name: /Chat.*Not configured/ })).toBeTruthy();
});

// Review P2-2's own mechanism, on the embedding tab: the confirm question
// names the CANDIDATE in prose, but the select itself keeps showing the
// CONFIRMED model throughout — during the question and after Cancel. Without
// the handler's synchronous reset, `fireEvent.change` has already written
// 'emb-2' onto the element's own `.value`, and nothing else would ever move
// it back: `selectValue` never actually changes across this whole exchange
// (the index stays on 'emb-1' throughout), so Svelte's own compiled dirty
// check has nothing to trip on.
test('cancel_returns_the_select_to_the_current_model', async () => {
  await renderOnModel(onModel(7)); // embeddingModel: 'emb-1'

  await pickModel('emb-2');
  expect(screen.getByTestId('model-embedding-confirm-title')).toBeTruthy();
  expect(modelSelect().value).toBe('emb-1');

  await fireEvent.click(screen.getByTestId('model-embedding-cancel'));

  expect(screen.queryByTestId('model-embedding-confirm-title')).toBeNull();
  expect(modelSelect().value).toBe('emb-1');

  // Picking the SAME candidate again after Cancel must still ask — a DOM
  // left stuck on 'emb-2' would fire no `change` at all for this second
  // identical pick, and the question would never come back.
  await pickModel('emb-2');
  expect(screen.getByTestId('model-embedding-confirm-title')).toBeTruthy();
  expect(setEmbeddingModel).not.toHaveBeenCalled();
});

// The same mechanism on the rejection path: a refused command must not leave
// the DOM showing the candidate it never adopted, and picking that same
// candidate again must still be answerable — the confirmation reopens,
// which it could not if the element's own `.value` had been left equal to
// it.
test('rejected_command_returns_the_select_and_reselecting_asks_again', async () => {
  setEmbeddingModel.mockRejectedValueOnce(new Error('the provider refused this model'));
  await renderOnModel(onModel(7)); // embeddingModel: 'emb-1', settings() re-read returns the same fixture

  await pickModel('emb-2');
  await fireEvent.click(await screen.findByTestId('model-embedding-discard'));

  await waitFor(() => expect(screen.getByTestId('model-embedding-error')).toBeTruthy());
  // The rejection did not move the model — the re-read still says emb-1 —
  // and the DOM was put back the instant the pick fired, not only once the
  // rejection was known.
  expect(modelSelect().value).toBe('emb-1');
  // The round is not fully settled until `changeBusy` clears (its own
  // `job_status` check still runs after the error is already on screen);
  // picking again before then would be blocked by the busy guard rather than
  // by anything this test is about.
  await waitFor(() => expect(modelSelect().disabled).toBe(false));

  await pickModel('emb-2');
  expect(screen.getByTestId('model-embedding-confirm-title')).toBeTruthy();
});

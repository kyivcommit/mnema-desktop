import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/svelte';
import { expect, test, afterEach, beforeEach, vi } from 'vitest';
import { tick } from 'svelte';
import Settings from './Settings.svelte';
import { setLocale } from '../i18n';
import type { AppPrefs, ModelSettings, ScanState } from '../lib/ipc';

// 🔴 Annotated, so the compiler checks it. This fixture crosses a `vi.mock`
// factory, whose return type is `unknown` — Task 3's three new REQUIRED fields
// on the `read` arm went unchecked here until the §9.3 section started reading
// them, and a missing `lastIndexedAt` reached `Intl.DateTimeFormat` as
// `undefined`.
// What a process in which nothing has happened yet reports (`ScanState::default`).
const IDLE_SCAN: ScanState = {
  revision: 0, files: 0, readSeq: 0, lastReading: null, snapshot: { kind: 'idle' },
};

const SETTINGS: ModelSettings = {
  key: { kind: 'absent' },
  index: {
    kind: 'read', embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0,
    failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null,
    embeddingModel: null, searchTextArm: true, searchContentArm: false,
  },
  platform: 'linux',
};

// Task 7: `Application` mounts into the 'application' panel and reads
// `app_prefs` on mount too. Annotated for the same reason as `SETTINGS` above
// — a fixture behind a `vi.mock` factory sits where the compiler cannot check
// it, and a missing field would render `undefined` in front of a person and
// pass here silently.
const APP_PREFS: AppPrefs = {
  hotkey: { shortcut: 'Alt+Space', status: { kind: 'registered' } },
  autostart: { kind: 'disabled' },
  version: '0.0.0',
  platform: 'linux',
};

// Task 4 mounts the real `Models` into the 'models' panel, and it calls
// `model_settings` on mount — without this mock every test in this file would
// hit the real, un-mockable `invoke` (there is no global setupFiles mock; see
// `i18n/wiring.test.ts:14-15`) and fail with an unhandled rejection. A fixed
// Absent/Read/linux fixture is enough: nothing here exercises Models' own
// behaviour, that lives in Models.test.ts. Task 5 adds `providerModels` on
// the same mount, for the same reason: an empty-but-well-formed catalogue,
// so the tabs render without pulling any of Models' own catalogue behaviour
// into this file. Task 7 adds `listTree` for the same reason again: `Folders`
// now mounts into the 'folders' panel and reads it on mount too — an empty
// listing is enough, since nothing here exercises Folders' own behaviour
// (that lives in Folders.test.ts).
// `modelSettings` and `listenScanProgress` are trackable `vi.fn()`s, not plain
// arrow functions, for Task 8's own fixtures below: they count how many times
// `Settings.svelte`'s single reader calls `model_settings`, and they need the
// listener callback the controller registers so a test can deliver the states
// a real scan would (`deliver`, the shape `Scanning.test.ts`/`JobStrip.test.ts`
// already use).
const modelSettings = vi.fn();
let deliver: ((state: ScanState) => void) | null = null;
vi.mock('../lib/ipc', () => ({
  modelSettings: (...a: unknown[]) => modelSettings(...a),
  setKey: vi.fn(),
  forgetKey: vi.fn(),
  providerModels: () => Promise.resolve({ entries: [], unreadable: 0, unreadableRecords: [] }),
  setChatModel: vi.fn(),
  listTree: () => Promise.resolve({ roots: [], recents: [] }),
  // Task 11 mounts `Masks` into the same panel, and it reads the mask list on
  // mount. Left out of this mock the wrapper is `undefined`, the call throws,
  // and every test in this file would run beside an unhandled rejection —
  // `jobStatus` below is the same lesson. An empty list is enough: nothing
  // here exercises the editor's own behaviour, that lives in Masks.test.ts.
  listMasks: () => Promise.resolve([]),
  maskPreview: vi.fn(),
  addMask: vi.fn(),
  removeMask: vi.fn(),
  addWatchedFolder: vi.fn(),
  removeWatchedFolder: vi.fn(),
  // The window creates the job controller on mount and asks `job_status`
  // straight away. Left out of this mock the wrapper is `undefined`, the call
  // throws, and the controller records a REJECTION — so every test in this file
  // ran with a refused status quietly on screen outside the panel, and the
  // section Models now reads the job state from would have been mounted beside
  // one. Answering honestly costs nothing and states what these tests assume:
  // nothing is running.
  jobStatus: () => Promise.resolve(IDLE_SCAN),
  startScanJob: vi.fn(),
  cancelJob: vi.fn(),
  // The window opens the scan subscription at mount now. Returns the SAME
  // `deliver` shape `Scanning.test.ts` uses, so Task 8's own fixtures below can
  // push a `ScanState` the way the core's own observer does. Left out of this
  // mock the wrapper is `undefined`, `mount` throws, and the whole window fails
  // to render.
  listenScanProgress: (cb: (state: ScanState) => void) => {
    deliver = cb;
    return Promise.resolve(() => {});
  },
  // Task 7 mounts `Application` into the 'application' panel, for the same
  // reason as `jobStatus` above: left out of this mock the wrapper is
  // `undefined`, the call throws, and every test in this file that ever visits
  // that panel runs beside an unhandled rejection. A fixed registered/disabled
  // fixture is enough — nothing here exercises Application's own behaviour,
  // that lives in Application.test.ts.
  appPrefs: () => Promise.resolve(APP_PREFS),
  setHotkey: vi.fn(),
  setAutostart: vi.fn(),
}));

beforeEach(() => {
  modelSettings.mockReset();
  modelSettings.mockResolvedValue(SETTINGS);
  deliver = null;
});

afterEach(() => {
  cleanup();
  setLocale('en'); // the store outlives the component; leave it as found
});

// One state, the way the core's own observer sends it.
async function emit(state: ScanState) {
  if (deliver === null) throw new Error('nothing is listening to scan-progress');
  deliver(state);
  await tick();
}

test('shows all four section names, in the spec order', () => {
  setLocale('en'); // seed, do not inherit: an earlier sibling switching the language must not decide this test
  render(Settings);
  const nav = screen.getByRole('navigation');
  // spec order: Models, Folders, Scanning, Application — read as one string so
  // a swap in order fails even though all four words are still present.
  expect(nav.textContent).toBe('ModelsFoldersScanningApplication');
});

test('clicking Folders shows the Folders heading and removes the Models heading', async () => {
  setLocale('en'); // seed, do not inherit
  const { container } = render(Settings);
  const panel = () => container.querySelector<HTMLElement>('.spane');
  expect(screen.getByRole('heading', { name: 'Models' })).toBeTruthy();
  expect(screen.queryByRole('heading', { name: 'Folders' })).toBeNull();

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));

  expect(screen.getByRole('heading', { name: 'Folders' })).toBeTruthy();
  expect(screen.queryByRole('heading', { name: 'Models' })).toBeNull();
  // The heading alone only proves the <h2> in Settings.svelte rendered — it
  // says nothing about whether <Folders /> is mounted underneath it. This
  // reads text only Folders.svelte itself renders (its own empty-state
  // sentence, from its `../lib/ipc` mock's `listTree: () => ({ roots: [],
  // recents: [] })` above), so deleting `<Folders />` and keeping the <h2>
  // fails here.
  await waitFor(() =>
    expect(within(panel()!).getByText('No folder has been added yet.')).toBeTruthy(),
  );
  // And the same claim for the mask editor, which shares this panel: a mask is
  // global, so it is drawn beside the folder list rather than inside a folder
  // row. This is text only `Masks.svelte` renders, so deleting `<Masks />`
  // fails here rather than passing quietly on the <h2> above.
  await waitFor(() =>
    expect(within(panel()!).getByText('No file mask has been added yet.')).toBeTruthy(),
  );
  expect(within(panel()!).getByRole('heading', { name: 'File masks' })).toBeTruthy();
});

// Owner's ruling: `aria-disabled` came off these buttons. They are fully
// operable — a click does switch the panel — so announcing them as disabled
// was a claim the window could not back, and its cost fell on exactly the
// people who would then never press them and never hear why the section is
// empty.
//
// Task 7 built Application, so no section is left whose panel carries the
// not-ready sentence — Indexing (Task 6) was the previous-to-last, and this
// test's own history already predicted running out of them. Task 8 removed
// `NOT_READY_ID`, `notReadyLabel` and the `aria-describedby` wiring from
// `Settings.svelte` itself, so what is still worth pinning here is the
// invariant the four sections owe together now that all of them are built:
// none is disabled, and none carries `aria-describedby` at all.
//
// 🔴 (review, Minor 7) Named for exactly what is asserted — `=== null` — rather
// than "not described by an id nothing renders", which promised the WEAKER,
// different check the old placeholder-era test made (that a referenced id, if
// present, resolves to a real element). Under Task 8 the whole wiring is
// removed, so this is cosmetic, but a name that promises more than its
// assertion is the shape this project keeps getting bitten by.
//
// (review, Minor 5) Both `toBeNull()` claims below cannot fail today either,
// same as the sentence-level assertions Task 8 deleted elsewhere — but the
// two classes are not the same risk. The deleted ones stood on a catalogue key
// that no longer exists anywhere in the source; nothing can reintroduce that
// string without a `Key` union arm reappearing first, which `tsc` would flag.
// These two stand on ordinary DOM attributes that any future edit to this
// component can set again with no compiler in the way — `aria-disabled` and
// `aria-describedby` are still valid attributes on a `<button>`, just unwired
// today — so the assertion stays as a regression guard against exactly that.
test('no section claims to be disabled, and no button carries aria-describedby', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);

  for (const name of ['Models', 'Folders', 'Scanning', 'Application']) {
    await fireEvent.click(screen.getByRole('button', { name }));
    const button = screen.getByRole('button', { name });
    expect(button.getAttribute('aria-disabled')).toBeNull();
    expect(button.getAttribute('aria-describedby')).toBeNull();
  }
});

// M3 (review): aria-pressed is the only signal of which section is selected —
// there is no CSS anywhere in this project. Both directions, before and after
// a click, the shape already used at launcher/Tree.test.ts:864.
test('aria-pressed says which section is selected', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);
  expect(screen.getByRole('button', { name: 'Models' }).getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByRole('button', { name: 'Folders' }).getAttribute('aria-pressed')).toBe('false');

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));

  expect(screen.getByRole('button', { name: 'Models' }).getAttribute('aria-pressed')).toBe('false');
  expect(screen.getByRole('button', { name: 'Folders' }).getAttribute('aria-pressed')).toBe('true');
});

test('a person reading the screen sees a real window, not a bare nav', async () => {
  setLocale('en'); // seed, do not inherit
  const { container } = render(Settings);
  const panel = () => container.querySelector('.spane');
  // Equality, not containment: 'Models' already sits inside <nav>, so a
  // toContain over the whole page is satisfied by the nav alone and never
  // notices an empty or replaced panel. Equality forces the panel itself to
  // carry the heading — on the default section and on an unbuilt one.
  //
  // Task 4: the placeholder `<h2>` this assertion used to pin is now real
  // content — `Models` mounts here and fetches on mount, so this waits for
  // that fetch to settle before reading the panel, rather than pinning the
  // pre-fetch flash. Measured, not guessed (`run-it-before-you-believe-it`):
  // the exact string below is what `Models` renders for
  // Absent/Read/linux, printed by an actual render rather than assumed.
  //
  // Task 5 adds the two model tabs and the status dot to the same panel —
  // both fetch on mount too, so the string below was re-measured rather than
  // hand-edited; this file's own mock (above) answers `provider_models` with
  // an empty-but-well-formed catalogue, which is why the tab list itself has
  // nothing in it here.
  //
  // Task 5's review (P1-4) settled the section's reading order — the Key group
  // moved up under the provider row and now leads with its own subject word,
  // and the Index group moved to the end — so this string was measured again
  // from a real render rather than hand-edited. It is the whole point of this
  // assertion that a layout change has to come through here and be read.
  //
  // Task 6 adds four blocks around the status dot — the confirmation, what a
  // change discarded, the degraded notice and a rejection — and on this fixture
  // every one of them is absent: no press has been made, so there is nothing to
  // confirm, nothing was discarded and nothing failed. What they leave behind is
  // a run of whitespace, and it is measured from a real render rather than
  // hand-edited, the same way every earlier version of this string was. **A
  // person reading this screen must see no new words here**, which is the claim:
  // a confirmation that rendered itself before anybody pressed anything would
  // arrive as text in the middle of this line.
  //
  // Task 6's review moved the status sentence ABOVE those four blocks, and this
  // string is where that shows: the dot used to be the last thing on the screen,
  // under a degraded notice it contradicted. The whitespace moved with it.
  await waitFor(() => expect(screen.getByRole('button', { name: 'Save' })).toBeTruthy());
  //
  // Live run finding 1 is what the two colons below are: this line is exactly
  // where a person reads «Provider OpenRouter» and «Key An OpenRouter key…» as
  // one phrase each, and it was green on both.
  expect(panel()?.textContent).toBe(
    'Models Provider: OpenRouter Key: An OpenRouter key lets this application reach the models.'
    + ' Create one in your OpenRouter account and paste it here.   Save    '
    + ' Embedding Chat   The provider does not currently list any models for this role.'
    + ' Not connected yet — add a key and choose an embedding model to enable content search.'
    + '     ',
  );

  // Task 6: the Scanning panel (renamed from Indexing by Task 8) is the §9.3
  // section now, and this fixture is an index nothing has ever been added to
  // — so what a person reads is the count and the sentence that stands where a
  // date would be, never a blank and never an epoch, followed by the one
  // «Scan» control Task 8 gives the section (shown whenever no run already
  // owns the slot, which is true of this fixture's idle snapshot). Measured
  // from a real render rather than hand-edited, the way every earlier version
  // of this string was.
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(panel()?.textContent?.replace(/\s+/g, ' ').trim())
    .toBe('Scanning The index holds 0 files. Nothing has been indexed yet. Scan');
});

// M2 (review): the Застосунок branch was once rendered by no test — a person
// clicking it would have got an empty panel and nothing would have noticed.
// Task 7 built the section, so what this now guards is the same finding in
// its new shape: clicking Application must show ITS OWN content, mounted
// underneath the heading, not a heading standing over an empty panel.
test('clicking Application shows its own content, not an empty panel', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);
  await fireEvent.click(screen.getByRole('button', { name: 'Application' }));
  expect(screen.getByRole('heading', { name: 'Application' })).toBeTruthy();
  // Text only `Application.svelte` itself renders, once its own `app_prefs`
  // read has settled — so a heading with nothing built underneath it fails
  // here rather than passing quietly on the <h2> above.
  await waitFor(() => expect(screen.getByTestId('application-version')).toBeTruthy());
  // The wait above already proves the section's own content mounted — a
  // stronger, positive claim than "the placeholder sentence is absent" ever
  // was, and the only one this task can still make: `settings_section_not_ready`
  // is gone from the catalogue, so a `queryByText` against its old sentence
  // could never fail again.
});

test('labels stay correct across a language switch after mount', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);

  // 🔴 (review, Important 4) Application, opened under 'en' and left MOUNTED
  // across the switch below — the only shape that can guard a `$derived`
  // missing `void $locale`. An earlier version of this test read the English
  // sentence here, navigated to Models (destroying the component), and only
  // came back to Application at the very end under a FRESH mount — which reads
  // whatever locale is current whether or not `void $locale` is present, so it
  // could not have caught its own removal. This is `Scanning`'s own shape,
  // applied here first because it is the shorter case; `Scanning` gets the
  // identical treatment two blocks down for the same reason.
  await fireEvent.click(screen.getByRole('button', { name: 'Application' }));
  await waitFor(() => expect(screen.getByTestId('application-shortcut-status')).toBeTruthy());
  expect(screen.getByText('This shortcut is registered with the system.')).toBeTruthy();

  setLocale('uk');
  await waitFor(() =>
    expect(screen.getByText('Це скорочення зареєстровано в системі.')).toBeTruthy());
  // Both directions: the English sentence is gone from the same mount, not
  // merely joined by the Ukrainian one.
  expect(screen.queryByText('This shortcut is registered with the system.')).toBeNull();

  // Back to English — the rest of this test assumes it, and Application stays
  // mounted through this second switch too, for the same reason as the first.
  setLocale('en');
  await waitFor(() => expect(screen.getByText('This shortcut is registered with the system.')).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Models' }));

  // The same property, on `Scanning.svelte`'s own `$derived.by` strings. The
  // component is destroyed by every nav change, so it too is opened under
  // 'en' and left mounted ACROSS `setLocale` rather than re-opened after it —
  // a version that clicked Сканування AFTER the switch would mount it fresh
  // under `uk` and read Ukrainian whether or not the anchor is there. Measured:
  // with `void $locale` deleted from `filesLine`, this test fails on the
  // Ukrainian assertion below and the English one still resolves.
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByText('The index holds 0 files.')).toBeTruthy());
  expect(screen.getByText('Nothing has been indexed yet.')).toBeTruthy();

  setLocale('uk');
  await waitFor(() => expect(screen.getByText('В індексі 0 файлів.')).toBeTruthy());
  expect(screen.getByText('Ще нічого не проіндексовано.')).toBeTruthy();
  // Both directions: the English strings are gone from the same mount, not
  // merely joined by Ukrainian ones.
  expect(screen.queryByText('The index holds 0 files.')).toBeNull();
  expect(screen.queryByText('Nothing has been indexed yet.')).toBeNull();

  const nav = screen.getByRole('navigation');
  expect(nav.textContent).toBe(['Моделі', 'Теки', 'Сканування', 'Застосунок'].join(''));
  await fireEvent.click(screen.getByRole('button', { name: 'Моделі' }));
  expect(screen.getByRole('heading', { name: 'Моделі' })).toBeTruthy();

  // Application, re-mounted fresh under 'uk' — this is a NEW mount (every nav
  // change destroys the previous section), so it reads Ukrainian from its own
  // first `app_prefs` resolution rather than from anything cached.
  await fireEvent.click(screen.getByRole('button', { name: 'Застосунок' }));
  await waitFor(() => expect(screen.getByTestId('application-shortcut-status')).toBeTruthy());
  expect(screen.getByText('Це скорочення зареєстровано в системі.')).toBeTruthy();
});

// Task 8's controller ruling: this window's ONE `model_settings` re-read fires
// when `scan.readSeq` grows OR the snapshot reaches `ended` — not "every
// ending" alone, and not "any change at all" either. `readSeq` (`scan_state.
// rs`) counts reading passes that have ENDED, and it can grow while the
// snapshot is still `running` — a reading phase handing off to embedding
// within the same job — which is exactly the moment `scanIncomplete`/
// `indexedFiles` can have moved without an `ended` snapshot ever appearing to
// say so. The four states below are read as one sequence because they are the
// only fixture that can tell all four fates apart from one another: `readSeq`
// growing mid-run re-reads; `readSeq` UNCHANGED and still running does NOT
// (Important 2, review — an unconditional `refresh()` passes every other
// assertion here); an ending re-reads even with `readSeq` unchanged; the
// identical snapshot object again re-reads nothing, because `apply` (`jobs.
// ts`) drops it by revision before this window's subscriber is even called.
test('a growing readSeq re-reads even mid-run, but not when readSeq stays put; an ending re-reads regardless; an identical snapshot re-reads nothing', async () => {
  render(Settings);
  // `Models.svelte` reads `model_settings` on its own mount too (an
  // independent poll Task 8 does not touch), and the window opens on Models
  // by default — so the baseline after mount is TWO calls, not one. Navigating
  // to Scanning unmounts Models and tears its own `jobs.state` subscription
  // down with it, so every call from here on is `Settings.svelte`'s alone.
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  const baseline = modelSettings.mock.calls.length;

  const EMBEDDING_COUNTS = { done: 0, total: 0, skipped: 0, refused: 0, contended: 0, secondsLeft: null };
  const runningWithReadSeq = (readSeq: number): ScanState => ({
    revision: readSeq + 1, files: 0, readSeq, lastReading: null,
    snapshot: { kind: 'running', cancellable: true, phase: { kind: 'embedding', counts: EMBEDDING_COUNTS } },
  });

  // `readSeq` 0 -> 1, the snapshot stays `running`.
  await emit(runningWithReadSeq(1));
  await waitFor(() => expect(modelSettings.mock.calls.length).toBe(baseline + 1));

  // 🔴 Fix round 1, Important 2. `readSeq` STILL 1 (unchanged) and a HIGHER
  // revision — an ordinary progress tick within the same embedding phase, the
  // one state that tells "readSeq changed OR ended" apart from "any change at
  // all": an unconditional `void refresh()` at `Settings.svelte:115` survives
  // every assertion in this test EXCEPT this one, because every other state
  // here also happens to be a positive case.
  const stillRunningSameReadSeq: ScanState = {
    revision: 5, files: 0, readSeq: 1, lastReading: null,
    snapshot: {
      kind: 'running', cancellable: true,
      phase: { kind: 'embedding', counts: { ...EMBEDDING_COUNTS, done: 2 } },
    },
  };
  await emit(stillRunningSameReadSeq);
  await tick();
  expect(modelSettings.mock.calls.length).toBe(baseline + 1); // unchanged: no re-read

  const endedSameReadSeq: ScanState = {
    revision: 10, files: 0, readSeq: 1, lastReading: null,
    snapshot: {
      kind: 'ended',
      report: {
        embedding: { kind: 'ran', done: 1, total: 1, refused: 0 },
        endedIn: 'embedding', reason: 'completed', message: null, resume: null,
      },
    },
  };
  // `readSeq` unchanged (still 1) — an `embedOnly`-shaped ending — yet the
  // snapshot becoming `ended` still triggers its own re-read.
  await emit(endedSameReadSeq);
  await waitFor(() => expect(modelSettings.mock.calls.length).toBe(baseline + 2));

  // The identical object again: `apply` (`jobs.ts`) drops it as no newer than
  // what the controller already holds, so this window's subscriber is never
  // even called — the pair "an ending re-reads" above is only real evidence of
  // the trigger if a non-event like this one stays silent.
  await emit(endedSameReadSeq);
  await tick();
  expect(modelSettings.mock.calls.length).toBe(baseline + 2);
});

// ---------------------------------------------------------------------------
// Two reads in flight, and the older one answering last — moved here from
// `Scanning.test.ts` (Task 8): the `settingsSeq` stamp these two pin is now
// `Settings.svelte`'s own guard, not that section's, since this window reads
// `model_settings` exactly once for everybody rather than once per section.
// ---------------------------------------------------------------------------

type IndexReadT = Extract<ModelSettings['index'], { kind: 'read' }>;
function readFixture(over: Partial<IndexReadT> = {}): ModelSettings {
  return { key: { kind: 'absent' }, index: { ...(SETTINGS.index as IndexReadT), ...over }, platform: 'linux' };
}

const visible = (el: Element | null) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();
const pageText = () => visible(document.body);

function deferredPromise<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

const endedOnce = (revision: number, readSeq: number): ScanState => ({
  revision, files: 0, readSeq, lastReading: null,
  snapshot: {
    kind: 'ended',
    report: {
      embedding: { kind: 'notReached' }, endedIn: 'reading', reason: 'completed', message: null, resume: null,
    },
  },
});

// 🔴 `Models.svelte` reads `model_settings` on its OWN mount too (Task 4, an
// independent poll Task 8 does not touch), and the window opens on the Models
// section by default — so a fresh mount always makes TWO calls, not one, and
// navigating to Models.svelte's own next `ended` would make a THIRD. Both
// tests below navigate to Scanning FIRST and let the two mount-time reads
// settle on the default fixture before installing a deferred queue, so the
// only calls the queue ever sees are `Settings.svelte`'s own — Models is
// unmounted by then and its `jobs.state` subscription has been torn down with
// it, the same as any other section a nav change destroys.
test('an older read that settles last does not repaint over the newer one', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());

  const queue: ReturnType<typeof deferredPromise<ModelSettings>>[] = [];
  modelSettings.mockImplementation(() => {
    const d = deferredPromise<ModelSettings>();
    queue.push(d);
    return d.promise;
  });

  await emit(endedOnce(1, 1));
  await waitFor(() => expect(queue).toHaveLength(1)); // the first ending's read
  await emit(endedOnce(2, 1)); // a second ending, `readSeq` unchanged
  await waitFor(() => expect(queue).toHaveLength(2)); // the second ending's read

  // Newer first, older last — the order the network is free to choose.
  queue[1].resolve(readFixture({ indexedFiles: 99 }));
  await waitFor(() => expect(visible(screen.getByTestId('indexing-index-files'))).toBe('The index holds 99 files.'));
  queue[0].resolve(readFixture({ indexedFiles: 7 }));
  await tick();
  await tick();

  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('The index holds 99 files.');
  expect(pageText()).not.toContain('The index holds 7 files.');
});

// ---------------------------------------------------------------------------
// The `loadError` path itself (fix round 1, Important 1). The pair above
// pins the STALE-rejection half of the stamp — a superseded rejection must
// stay silent — but neither fixture in this file ever let a LIVE, non-stale
// `model_settings` call reject at all, so `loadError = e…` at
// `Settings.svelte:77` and `loadError = null` at `:74` could both be deleted
// without reddening anything (review, Important 1). These two restore that:
// a live rejection must show the banner beside the numbers it could not
// confirm, and a live success afterwards must take it away again.
// ---------------------------------------------------------------------------

test('a live rejection of a re-read shows the failure banner beside the numbers it could not confirm', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);
  // Models' own poll out of the way, as the pair above does.
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('The index holds 0 files.');
  expect(screen.queryByTestId('indexing-index-load-failed')).toBeNull();

  const SENTENCE = 'the index went away mid-session';
  modelSettings.mockRejectedValueOnce(new Error(SENTENCE));
  await emit(endedOnce(1, 1));

  await waitFor(() => expect(screen.getByTestId('indexing-index-load-failed')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-load-failed'))).toBe('The state of the index could not be read.');
  expect(visible(screen.getByTestId('indexing-index-load-error'))).toBe(SENTENCE);
  // Kept, not blanked: a count that was true a moment ago probably still is —
  // `Tree.svelte`'s ruling, restated on `refresh()` itself (Minor 1, review).
  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('The index holds 0 files.');
});

test('a live success after a rejection takes the failure banner away and shows the new numbers', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());

  modelSettings.mockRejectedValueOnce(new Error('the index went away mid-session'));
  await emit(endedOnce(1, 1));
  await waitFor(() => expect(screen.getByTestId('indexing-index-load-failed')).toBeTruthy());

  modelSettings.mockResolvedValueOnce(readFixture({ indexedFiles: 13 }));
  await emit(endedOnce(2, 1));

  await waitFor(() => expect(visible(screen.getByTestId('indexing-index-files'))).toBe('The index holds 13 files.'));
  expect(screen.queryByTestId('indexing-index-load-failed')).toBeNull();
  expect(screen.queryByTestId('indexing-index-load-error')).toBeNull();
});

// The mirror. An older read can REJECT after a newer one has already
// repainted — nothing here asserts a failure sentence, because `Settings.svelte`
// keeps no `loadError` banner of its own visible outside `Scanning.svelte`'s
// props; what this pins is that the numbers stay the newer read's.
test('an older read that is refused last does not overwrite the newer numbers', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());

  const queue: ReturnType<typeof deferredPromise<ModelSettings>>[] = [];
  modelSettings.mockImplementation(() => {
    const d = deferredPromise<ModelSettings>();
    queue.push(d);
    return d.promise;
  });

  await emit(endedOnce(1, 1));
  await waitFor(() => expect(queue).toHaveLength(1));
  await emit(endedOnce(2, 1));
  await waitFor(() => expect(queue).toHaveLength(2));

  queue[1].resolve(readFixture({ indexedFiles: 42 }));
  await waitFor(() => expect(visible(screen.getByTestId('indexing-index-files'))).toBe('The index holds 42 files.'));
  queue[0].reject(new Error('STALE-REJECTION'));
  await tick();
  await tick();

  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('The index holds 42 files.');
  expect(screen.queryByTestId('indexing-index-load-failed')).toBeNull();
  expect(pageText()).not.toContain('STALE-REJECTION');
});

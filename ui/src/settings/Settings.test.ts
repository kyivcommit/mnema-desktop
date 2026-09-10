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
  revision: 0, files: 0, readSeq: 0, jobsDone: 0, lastReading: null, snapshot: { kind: 'idle' },
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
//
// Task 10e makes `listTree` a trackable `vi.fn()` for the third reason on that
// list: `Folders` is mounted for the WINDOW's life now, so how many times it
// reads `list_tree` is a fact about this window rather than about that
// section, and one of this task's claims is a count of exactly that. The
// subfolder commands join it because a panel expanded here is a real
// expansion — `Folders.svelte` calls them, and a mock factory that leaves
// them out hands the component `undefined` to call.
//
// Fix round 1, Important 1 does the same to the mask editor's two commands,
// for the same two reasons: `list_masks` is now read once per WINDOW and this
// file counts it, and a question raised in that editor is a real
// `mask_preview` call rather than a `vi.fn()` answering `undefined`.
const modelSettings = vi.fn();
// Task 4 (review P2-1): `mutation_outcome_survives_a_section_switch` drives a
// real `Models` embedding choice through the real `Settings` window, so both
// need to be trackable/overridable here rather than the fixed stubs every
// other test in this file was content with.
const setEmbeddingModel = vi.fn();
const providerModels = vi.fn();
const listTree = vi.fn();
const listSubfolders = vi.fn();
const listExclusions = vi.fn();
const excludeSubfolder = vi.fn();
const listMasks = vi.fn();
const maskPreview = vi.fn();
const addMask = vi.fn();
let deliver: ((state: ScanState) => void) | null = null;
vi.mock('../lib/ipc', () => ({
  modelSettings: (...a: unknown[]) => modelSettings(...a),
  setKey: vi.fn(),
  forgetKey: vi.fn(),
  providerModels: (...a: unknown[]) => providerModels(...a),
  setChatModel: vi.fn(),
  setEmbeddingModel: (...a: unknown[]) => setEmbeddingModel(...a),
  listTree: (...a: unknown[]) => listTree(...a),
  listSubfolders: (...a: unknown[]) => listSubfolders(...a),
  listExclusions: (...a: unknown[]) => listExclusions(...a),
  excludeSubfolder: (...a: unknown[]) => excludeSubfolder(...a),
  includeSubfolder: vi.fn(),
  // Task 11 mounts `Masks` into the same panel, and it reads the mask list on
  // mount. Left out of this mock the wrapper is `undefined`, the call throws,
  // and every test in this file would run beside an unhandled rejection —
  // `jobStatus` below is the same lesson. An empty list is enough: nothing
  // here exercises the editor's own behaviour, that lives in Masks.test.ts.
  listMasks: (...a: unknown[]) => listMasks(...a),
  maskPreview: (...a: unknown[]) => maskPreview(...a),
  addMask: (...a: unknown[]) => addMask(...a),
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
  setEmbeddingModel.mockReset();
  providerModels.mockReset();
  providerModels.mockResolvedValue({ entries: [], unreadable: 0, unreadableRecords: [] });
  // The empty listing every test in this file assumed before Task 10e made
  // this a `vi.fn`. A test that needs a folder to expand says so itself,
  // BEFORE `render` — the window reads `list_tree` on its own mount now.
  listTree.mockReset();
  listTree.mockResolvedValue({ roots: [], recents: [] });
  listSubfolders.mockReset();
  listSubfolders.mockResolvedValue({ entries: [], unnameable: 0 });
  listExclusions.mockReset();
  listExclusions.mockResolvedValue([]);
  excludeSubfolder.mockReset();
  // The empty mask list this file has always answered with, now through a
  // countable mock. `maskPreview`'s two numbers differ on purpose, the way
  // `Masks.test.ts` states it: a fixture whose numbers are equal cannot tell
  // them apart.
  listMasks.mockReset();
  listMasks.mockResolvedValue([]);
  maskPreview.mockReset();
  maskPreview.mockResolvedValue({ paths: 4, documents: 2 });
  addMask.mockReset();
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

// Task 4 (review P2-1): Models joins Folders as mounted-hidden rather than
// torn down by a nav click, so "removes the Models heading" is no longer an
// honest name for what a click on Folders does to it — `queryByRole` returning
// `null` here is the accessibility tree excluding a `hidden` element, not
// evidence the `<h2>` or `<Models>` were ever removed from the DOM (F10's own
// lesson: a claim of "gone" needs the element's own presence checked too, not
// only a query that hidden and unmounted both satisfy the same way).
test('clicking Folders shows the Folders heading and hides the Models section, still mounted', async () => {
  setLocale('en'); // seed, do not inherit
  const { container } = render(Settings);
  const panel = () => container.querySelector<HTMLElement>('.spane');
  const modelsPanel = () => screen.getByTestId('settings-panel-models');
  expect(screen.getByRole('heading', { name: 'Models' })).toBeTruthy();
  expect(screen.queryByRole('heading', { name: 'Folders' })).toBeNull();

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));

  expect(screen.getByRole('heading', { name: 'Folders' })).toBeTruthy();
  expect(screen.queryByRole('heading', { name: 'Models' })).toBeNull();
  // Still in the DOM, and `hidden` is what took it out of the a11y tree above
  // — not an `{#if}` this file already proved wrong for Folders.
  expect(modelsPanel()).toBeTruthy();
  expect(modelsPanel().hidden).toBe(true);
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
  // 🔴 Task 10e: what is SHOWN in the panel, not what is mounted in it. The
  // Folders section stays mounted and `hidden` for the window's life now
  // (F10), and `textContent` reads a hidden subtree exactly as it reads a
  // shown one — so the assertion this test has always made, "a person reads
  // these words and no others", is only about a person once the hidden
  // sections are taken out. The clone is what keeps this a read: removing
  // `[hidden]` from the live tree would be this test editing the window it is
  // reading.
  const panel = () => {
    const pane = container.querySelector('.spane')!.cloneNode(true) as HTMLElement;
    for (const el of pane.querySelectorAll('[hidden]')) el.remove();
    return pane;
  };
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
  //
  // 🔴 Task 10e: the leading space is the hidden Folders section's own
  // indentation, left behind in the panel when the clone above drops the
  // section itself. It is not a word and nobody sees it — the claim this
  // string makes is about words — but it IS what the panel now contains, and
  // this assertion is a measurement, so it is written down rather than
  // trimmed away.
  // Task 4: the two per-role configured dots sit beside their own tabs
  // (review P2-1), each carrying its own accessible word — measured again
  // rather than hand-edited, the same rule every earlier version of this
  // string followed.
  expect(panel()?.textContent).toBe(
    ' Models Provider: OpenRouter Key: An OpenRouter key lets this application reach the models.'
    + ' Create one in your OpenRouter account and paste it here.  Save    '
    + ' Embedding Not configured Chat Not configured   The provider does not currently list any models for this role.'
    + ' Not connected yet — add a key and choose an embedding model to enable content search.'
    + '      ',
  );

  // Task 6: the Scanning panel (renamed from Indexing by Task 8) is the §9.3
  // section now, and this fixture is an index nothing has ever been added to
  // — so what a person reads is the count and the sentence that stands where a
  // date would be, never a blank and never an epoch, followed by the one
  // «Scan» control Task 8 gives the section (shown whenever no run already
  // owns the slot, which is true of this fixture's idle snapshot). The
  // statcard leads (Task 6) with the same count and the same never-sentence,
  // never a decorative number of its own. Measured from a real render rather
  // than hand-edited, the way every earlier version of this string was.
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(panel()?.textContent?.replace(/\s+/g, ' ').trim())
    .toBe('Scanning Documents0 Last updateNothing has been indexed yet. The index holds 0 files. Nothing has been indexed yet. Scan');
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
  // Task 6: the statcard reuses this same never-sentence in its own cell
  // (`Scanning.svelte`'s `lastUpdateText`), so the sentence now renders TWICE
  // — the paragraph above and the statcard beside it — both following the
  // language switch below.
  expect(screen.getAllByText('Nothing has been indexed yet.')).toHaveLength(2);

  setLocale('uk');
  await waitFor(() => expect(screen.getByText('В індексі 0 файлів.')).toBeTruthy());
  expect(screen.getAllByText('Ще нічого не проіндексовано.')).toHaveLength(2);
  // Both directions: the English strings are gone from the same mount, not
  // merely joined by Ukrainian ones.
  expect(screen.queryByText('The index holds 0 files.')).toBeNull();
  expect(screen.queryAllByText('Nothing has been indexed yet.')).toHaveLength(0);

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
  // by default — so the baseline after mount is TWO calls, not one.
  //
  // Task 4 (review P2-1): navigating to Scanning no longer unmounts Models —
  // it is mounted-hidden for the window's life now, the same as Folders —
  // so its OWN `jobs.state` subscription stays live too. It re-reads on
  // exactly one trigger, an `ended` snapshot (`Models.svelte`'s own
  // `onMount`), which is narrower than `Settings.svelte`'s three; every
  // `ended` emission below therefore adds ONE extra call beyond
  // `Settings.svelte`'s own, counted explicitly at each such assertion.
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  const baseline = modelSettings.mock.calls.length;

  const EMBEDDING_COUNTS = { done: 0, total: 0, skipped: 0, refused: 0, contended: 0, secondsLeft: null };
  // `jobsDone: 0` throughout the running half: nothing has ended yet, which is
  // what leaves `readSeq` as the only thing that can fire the trigger here.
  const runningWithReadSeq = (readSeq: number): ScanState => ({
    revision: readSeq + 1, files: 0, readSeq, jobsDone: 0, lastReading: null,
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
    revision: 5, files: 0, readSeq: 1, jobsDone: 0, lastReading: null,
    snapshot: {
      kind: 'running', cancellable: true,
      phase: { kind: 'embedding', counts: { ...EMBEDDING_COUNTS, done: 2 } },
    },
  };
  await emit(stillRunningSameReadSeq);
  await tick();
  expect(modelSettings.mock.calls.length).toBe(baseline + 1); // unchanged: no re-read

  // The job ends here, so `jobsDone` moves with it — the shape the core would
  // actually send. The re-read below is therefore over-determined, and the
  // clause it is evidence for is named in the sentence under it rather than
  // isolated by this fixture; the isolating pair for `ended` alone is the
  // identical re-emission at the end of this test.
  const endedSameReadSeq: ScanState = {
    revision: 10, files: 0, readSeq: 1, jobsDone: 1, lastReading: null,
    snapshot: {
      kind: 'ended',
      report: {
        embedding: { kind: 'ran', done: 1, total: 1, refused: 0 },
        endedIn: 'embedding', reason: 'completed', message: null, resume: null,
      },
    },
  };
  // `readSeq` unchanged (still 1) — an `embedOnly`-shaped ending — yet the
  // snapshot becoming `ended` still triggers its own re-read. Two calls, not
  // one: `Settings.svelte`'s own (`jobsDoneChanged`) and `Models.svelte`'s
  // (any `ended` snapshot) both fire from the same emission.
  await emit(endedSameReadSeq);
  await waitFor(() => expect(modelSettings.mock.calls.length).toBe(baseline + 3));

  // The identical object again: `apply` (`jobs.ts`) drops it as no newer than
  // what the controller already holds, so NEITHER subscriber is even called
  // — the pair "an ending re-reads" above is only real evidence of the
  // trigger if a non-event like this one stays silent.
  await emit(endedSameReadSeq);
  await tick();
  expect(modelSettings.mock.calls.length).toBe(baseline + 3);
});

// F1/F9 (Task 10 live run). A folder removal ends the slot with
// `finish(Terminal::Idle, Some(files))` (`bridge.rs:182`) — the snapshot goes
// straight to `idle` with a NEW `files` count, never through `ended` and
// never bumping `readSeq` (a removal is not a reading pass), so neither half
// of the trigger above fires. Two things then went stale together: the
// section kept showing the file count from before the removal, and an
// `ended{cancelled in embedding, resume: embedOnly}` report the strip had
// drawn its own «Продовжити вбудовування» from is REPLACED by this later,
// report-less idle snapshot — the strip's offer disappears with the report,
// and nothing in the section had re-read the index's own `pendingChunks`
// marker to offer it a different way. Fix: the same subscription also calls
// `refresh()` when `scan.files` differs from the value it last acted on,
// seeded from what the store already holds — the same shape as
// `seenReadSeq`/`seenSnapshot` above.
//
// RED (fix round 1, re-derived — Important 3, review round 1: the first
// attempt at this note named the wrong `waitFor` and quoted a message
// `toBe` cannot produce). With the `filesChanged` clause deleted from
// `Settings.svelte`, the FIRST `waitFor` below (`baseline + 1`) still passes
// — it is satisfied by the `ended` trigger alone, which nothing here
// touches. It is the SECOND `waitFor` (`afterEnded + 1`, the removal's own
// idle snapshot) that goes red, with:
// "AssertionError: expected 3 to be 4 // Object.is equality" — `toBe`'s own
// message, not `toHaveBeenCalledTimes`'s.
test('a files count that changed on an idle snapshot re-reads, revealing the queue the vanished report offered', async () => {
  render(Settings);
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  const baseline = modelSettings.mock.calls.length;

  // The ending re-reads on its own (the earlier test's own pair) — this is
  // the baseline the removal's own re-read is measured from, not the claim
  // this test makes.
  const endedEmbeddingCancelled: ScanState = {
    revision: 20, files: 668, readSeq: 3, jobsDone: 0, lastReading: null,
    snapshot: {
      kind: 'ended',
      report: {
        embedding: { kind: 'ran', done: 4, total: 10, refused: 0 },
        endedIn: 'embedding', reason: 'cancelled', message: null, resume: 'embedOnly',
      },
    },
  };
  await emit(endedEmbeddingCancelled);
  // Two calls: `Settings.svelte`'s own (`readSeq` 0 → 3) and `Models.svelte`'s
  // (any `ended` snapshot, mounted-hidden since Task 4 and so still
  // listening on the Scanning section).
  await waitFor(() => expect(modelSettings.mock.calls.length).toBe(baseline + 2));
  const afterEnded = modelSettings.mock.calls.length;

  // A removal: `files` drops (668 -> 0, the empty list this fixture's own
  // `listTree` mock answers), the snapshot is a bare `idle`, and `readSeq` did
  // not move. This is the ONE state in the sequence a `readSeq`-or-`ended`
  // trigger cannot see at all.
  const idleAfterRemoval: ScanState = {
    revision: 21, files: 0, readSeq: 3, jobsDone: 0, lastReading: null, snapshot: { kind: 'idle' },
  };
  // The re-read this removal earns answers with a queue still pending — the
  // fact the vanished `ended` report is no longer here to say for itself.
  modelSettings.mockResolvedValueOnce(readFixture({ pendingChunks: 3, indexedFiles: 0 }));
  await emit(idleAfterRemoval);
  await waitFor(() => expect(modelSettings.mock.calls.length).toBe(afterEnded + 1));

  await waitFor(() => expect(screen.getByTestId('indexing-index-pending-chunks')).toBeTruthy());
  expect(screen.getByTestId('scanning-continue')).toBeTruthy();
});

// The pair: `running` ticks the store on every progress event even when
// nothing a re-read would answer differently has moved — `files` here stays
// at the seeded 0 (`NO_SCAN`), so this is not the removal case above wearing
// a different snapshot kind.
test('a running tick with an unchanged files count does not re-read', async () => {
  render(Settings);
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  const baseline = modelSettings.mock.calls.length;

  const running: ScanState = {
    revision: 5, files: 0, readSeq: 0, jobsDone: 0, lastReading: null,
    snapshot: {
      kind: 'running', cancellable: true,
      phase: { kind: 'embedding', counts: { done: 1, total: 10, skipped: 0, refused: 0, contended: 0, secondsLeft: null } },
    },
  };
  await emit(running);
  await tick();
  expect(modelSettings.mock.calls.length).toBe(baseline);

  // Minor 3 (review, fix round 1): a "no call" assertion alone is satisfied
  // by an emission that never reached the subscriber at all — a `jobStatus`
  // fixture seeded at a higher revision, say, would make `apply` (`jobs.ts`)
  // drop every state this test emits, and this assertion would keep passing
  // while testing nothing. One positive assertion closes that: an `ended`
  // snapshot right after DOES reach the subscriber and DOES trigger its own
  // read, so a silently-dropped stream shows up here as a missing call
  // rather than only as fixture 1 failing elsewhere in the file. Two calls,
  // not one: `Settings.svelte`'s own (`jobsDone` 0 → 1) and `Models.svelte`'s
  // (any `ended` snapshot).
  await emit(endedOnce(10, 0));
  await waitFor(() => expect(modelSettings.mock.calls.length).toBe(baseline + 2));
});

// 🔴 Final review, Area C, Important 1. A job nobody asked for and that owes no
// report (`OtherJob`, `ipc.ts`) ends by going straight back to `idle` — no
// `ended` snapshot exists for it to reach, `readSeq` does not move because it
// read no folder, and `files` does not move because it deleted no row. All
// three of the window's named triggers are blind to it, so the fourth is not a
// fact about which job it was at all: a job having ENDED is the trigger.
//
// 🔴 Independent review, finding 3: the fourth trigger used to be «the snapshot
// left `running`», and this fixture was blind to what was wrong with that,
// because it delivers the `running` snapshot first and so always supplies the
// edge. `ScanState.jobsDone` replaces it, and the fixtures below now carry the
// count the core would: 0 while the job runs, 1 once it has ended, and 1 still
// on the idle snapshot after that — which is what keeps the «once, not once per
// emission» pair below real rather than satisfied by the snapshot's kind.
//
// The probe is the harmless half and is asserted anyway, because the fix is
// deliberately written without naming any `OtherJob`: one extra
// `model_settings` after a probe rewrites the same numbers invisibly, and the
// alternative — a list of job kinds in this file — is a list the next kind is
// not on. Its own pair is `a running tick with an unchanged files count does
// not re-read` above: a tick that stays `running` still fires nothing.
test('a probe leaving running re-reads once, even though it moved neither counter', async () => {
  render(Settings);
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  const baseline = modelSettings.mock.calls.length;

  const probeRunning: ScanState = {
    revision: 6, files: 0, readSeq: 0, jobsDone: 0, lastReading: null,
    snapshot: { kind: 'running', cancellable: false, phase: { kind: 'other', job: 'probe' } },
  };
  await emit(probeRunning);
  await tick();
  expect(modelSettings.mock.calls.length).toBe(baseline);

  const idleAfterProbe: ScanState = {
    revision: 7, files: 0, readSeq: 0, jobsDone: 1, lastReading: null, snapshot: { kind: 'idle' },
  };
  await emit(idleAfterProbe);
  await waitFor(() => expect(modelSettings.mock.calls.length).toBe(baseline + 1));

  // ONCE, not once per emission afterwards: the trigger is a job ENDING, and a
  // second idle snapshot with the same count of endings is not one. This is the
  // assertion a plain "the snapshot is not running" rule would fail.
  const stillIdle: ScanState = {
    revision: 8, files: 0, readSeq: 0, jobsDone: 1, lastReading: null, snapshot: { kind: 'idle' },
  };
  await emit(stillIdle);
  await tick();
  expect(modelSettings.mock.calls.length).toBe(baseline + 1);
});

// 🔴 The half that costs something. `set_embedding_model` claims the same slot
// as `Other { ModelAdoption }` and releases it by `Drop`, which for `Other`
// writes `Idle` — the same shape as the probe above, and the same blindness.
// What it MOVES is exactly what this section draws: adopting a different
// embedding model creates a new space, so `pendingChunks` goes from nought to
// the whole archive at once.
//
// Both halves asserted, because the call count alone would be satisfied by a
// window that re-read and then drew nothing with the answer: the section must
// end up showing the queue row AND «Продовжити вбудовування» for work that now
// covers everything. Before this, it showed neither until some unrelated scan
// happened to end.
test('a model adoption leaving running re-reads, and the queue it created is offered', async () => {
  render(Settings);
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  const baseline = modelSettings.mock.calls.length;
  // The state this test is about is the ABSENCE of the offer beforehand, so it
  // is asserted rather than assumed.
  expect(screen.queryByTestId('scanning-continue')).toBeNull();

  // `files` and `readSeq` hold the values this window was SEEDED with, so
  // neither of the two named triggers can fire anywhere in this fixture and the
  // transition is the only thing left that could.
  const adoptionRunning: ScanState = {
    revision: 9, files: 0, readSeq: 0, jobsDone: 0, lastReading: null,
    snapshot: {
      kind: 'running', cancellable: false, phase: { kind: 'other', job: 'modelAdoption' },
    },
  };
  await emit(adoptionRunning);
  await tick();
  expect(modelSettings.mock.calls.length).toBe(baseline);

  // The new space: everything queued, nothing embedded, and NEITHER `readSeq`
  // nor `files` different from the tick before it.
  modelSettings.mockResolvedValueOnce(readFixture({ pendingChunks: 902, indexedFiles: 12 }));
  const idleAfterAdoption: ScanState = {
    revision: 10, files: 0, readSeq: 0, jobsDone: 1, lastReading: null, snapshot: { kind: 'idle' },
  };
  await emit(idleAfterAdoption);
  await waitFor(() => expect(modelSettings.mock.calls.length).toBe(baseline + 1));

  await waitFor(() => expect(screen.getByTestId('indexing-index-pending-chunks')).toBeTruthy());
  expect(visible(screen.getByTestId('scanning-continue'))).toBe('Continue embedding');
});

// Task 11a (Task 8 M6/M7): the window's own `jobs.state` subscription is
// torn down on unmount independently of `jobs.mount`'s own teardown — see
// `Settings.jobs-teardown.test.ts`, which mocks `./jobs` from the top of its
// own file so the property can be observed apart from that neighbour.

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

// `jobsDone` is a parameter with a default rather than a constant, because a
// test that emits two endings in a row is emitting two jobs and the core would
// count them: the default serves the majority that deliver one, and the two
// tests that deliver a second pass `2` themselves.
const endedOnce = (revision: number, readSeq: number, jobsDone = 1): ScanState => ({
  revision, files: 0, readSeq, jobsDone, lastReading: null,
  snapshot: {
    kind: 'ended',
    report: {
      embedding: { kind: 'notReached' }, endedIn: 'reading', reason: 'completed', message: null, resume: null,
    },
  },
});

// 🔴 `Models.svelte` reads `model_settings` on its OWN mount too (an
// independent poll Task 8 does not touch), and the window opens on the Models
// section by default — so a fresh mount always makes TWO calls, not one.
// Both tests below navigate to Scanning FIRST and let the two mount-time
// reads settle on the default fixture before installing a deferred queue.
//
// Task 4 (review P2-1): Models no longer unmounts on that nav click — it is
// mounted-hidden for the window's life, the same as Folders — so its own
// `jobs.state` subscription stays live too, and EVERY `ended` emission below
// reaches BOTH subscribers, adding two entries to the queue per emission
// rather than one. Which of the two belongs to `Settings.svelte` and which
// to `Models.svelte` is not something these tests need to know: both entries
// born from the SAME emission are resolved with the SAME answer, so the
// `settingsSeq` guard this pair is pinning is exercised the same way whichever
// subscriber Settings' own read turns out to be. Folders costs nothing here
// either way — it reads `list_tree`, never `model_settings`.
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
  await waitFor(() => expect(queue).toHaveLength(2)); // the first ending's two reads
  await emit(endedOnce(2, 1, 2)); // a second ending, `readSeq` unchanged
  await waitFor(() => expect(queue).toHaveLength(4)); // the second ending's two reads

  // Newer first, older last — the order the network is free to choose. Both
  // of the second ending's reads carry the newer answer; both of the first
  // ending's carry the one that must not win.
  queue[2].resolve(readFixture({ indexedFiles: 99 }));
  queue[3].resolve(readFixture({ indexedFiles: 99 }));
  await waitFor(() => expect(visible(screen.getByTestId('indexing-index-files'))).toBe('The index holds 99 files.'));
  queue[0].resolve(readFixture({ indexedFiles: 7 }));
  queue[1].resolve(readFixture({ indexedFiles: 7 }));
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
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('The index holds 0 files.');
  expect(screen.queryByTestId('indexing-index-load-failed')).toBeNull();

  const SENTENCE = 'the index went away mid-session';
  // Task 4: this `ended` reaches BOTH `Settings.svelte`'s own subscriber and
  // `Models.svelte`'s (mounted-hidden, still listening) — two calls, in an
  // order this test does not depend on, so both are queued to reject.
  modelSettings.mockRejectedValueOnce(new Error(SENTENCE));
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

  // Task 4: each `ended` below reaches two subscribers (`Settings.svelte`'s
  // own and the mounted-hidden `Models.svelte`'s), in an order this test
  // does not depend on — both calls per emission are queued the same way.
  modelSettings.mockRejectedValueOnce(new Error('the index went away mid-session'));
  modelSettings.mockRejectedValueOnce(new Error('the index went away mid-session'));
  await emit(endedOnce(1, 1));
  await waitFor(() => expect(screen.getByTestId('indexing-index-load-failed')).toBeTruthy());

  modelSettings.mockResolvedValueOnce(readFixture({ indexedFiles: 13 }));
  modelSettings.mockResolvedValueOnce(readFixture({ indexedFiles: 13 }));
  await emit(endedOnce(2, 1, 2));

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

  // Task 4: two subscribers per `ended` (see the pair above) — two entries
  // per emission, both resolved/rejected the same way per emission.
  await emit(endedOnce(1, 1));
  await waitFor(() => expect(queue).toHaveLength(2));
  await emit(endedOnce(2, 1, 2));
  await waitFor(() => expect(queue).toHaveLength(4));

  queue[2].resolve(readFixture({ indexedFiles: 42 }));
  queue[3].resolve(readFixture({ indexedFiles: 42 }));
  await waitFor(() => expect(visible(screen.getByTestId('indexing-index-files'))).toBe('The index holds 42 files.'));
  queue[0].reject(new Error('STALE-REJECTION'));
  queue[1].reject(new Error('STALE-REJECTION'));
  await tick();
  await tick();

  expect(visible(screen.getByTestId('indexing-index-files'))).toBe('The index holds 42 files.');
  expect(screen.queryByTestId('indexing-index-load-failed')).toBeNull();
  expect(pageText()).not.toContain('STALE-REJECTION');
});

// ---------------------------------------------------------------------------
// F10 (Task 10 live run): the Folders section keeps what a person built by
// hand while another section is shown. It is the one section mounted for the
// window's life and hidden with the `hidden` attribute; the other three are
// still mounted and destroyed by every nav click.
// ---------------------------------------------------------------------------

// One watched folder holding one indexed file under `drop/`, which is what
// gives the exclude question its two numbers to freeze. The shape
// `Folders.test.ts` builds its own fixtures in.
const ONE_ROOT = {
  roots: [{
    rootId: 1,
    absolutePath: '/synthetic/root',
    name: 'root',
    files: [{ relativePath: 'drop/x.md', documentId: 'doc-1' }],
  }],
  recents: [],
};
const ONE_SUBFOLDER = {
  entries: [{ name: 'drop', relativePath: 'drop', state: { kind: 'open' } }],
  unnameable: 0,
};

// The Folders section's own element, mounted whether or not it is shown.
const foldersPanel = () => screen.getByTestId('settings-panel-folders');
// What a person can see of it, asked of the DOM rather than of this file's
// belief about `hidden`: `display` comes from jsdom's own default stylesheet,
// which carries the same `[hidden] { display: none }` rule every browser has.
const foldersShown = () =>
  !foldersPanel().hidden && getComputedStyle(foldersPanel()).display !== 'none';

// Opens «Теки», expands the one root, and raises an exclude question about
// `drop`. Returns with the question on screen.
async function raiseQuestionInFolders() {
  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));
  await screen.findByTestId('folder-confirm-1');
}

// 🔴 The finding itself. A person opens «Теки», expands a tree to find the
// folder they mean to protect, presses Exclude, goes to look at «Сканування»
// to check whether a scan is running — and comes back to a shut tree and no
// question, with nothing on screen having said either went away. The two
// states this separates are "another section is shown" and "this section was
// destroyed": under the first the panel and the press are still there when a
// person returns, under the second they are gone.
//
// RED, measured twice. Against the `{#if}` chain this task replaces, the
// section is simply not there while «Сканування» is shown, and the claim that
// it is hidden rather than gone fails first:
//   TestingLibraryElementError: Unable to find an element by:
//   [data-testid="settings-panel-folders"]
// That failure is also what a wrapper carrying only the testid produces, so it
// does not yet separate hidden from unmounted. The variant that does — the
// `hidden` wrapper kept, `<Folders>` still behind an `{#if}` inside it — was
// run too, and reddens on what a person comes back to:
//   TestingLibraryElementError: Unable to find an element by:
//   [data-testid="folder-panel-1"]
test('an expanded panel and a pending question survive a switch to another section and back', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(ONE_ROOT);
  listSubfolders.mockResolvedValue(ONE_SUBFOLDER);
  render(Settings);

  await raiseQuestionInFolders();
  expect(foldersShown()).toBe(true);
  const question = visible(screen.getByTestId('folder-confirm-1'));
  expect(question).toContain('drop');

  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  // Hidden, not unmounted — and hidden in the way that also takes it out of
  // the accessibility tree, which is what `queryByRole` is asking here.
  expect(foldersShown()).toBe(false);
  expect(screen.queryByRole('heading', { name: 'Folders' })).toBeNull();
  expect(screen.queryByRole('button', { name: 'Exclude drop' })).toBeNull();

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));

  expect(foldersShown()).toBe(true);
  // The panel is still expanded — `list_subfolders` is not asked again, and
  // the row it drew is the same one. (Fix round 1, Minor 3: the call count is
  // the half of that claim the two locators below cannot make. A section
  // rebuilt on the way back would re-read this level before drawing the same
  // row, and every assertion here would stay green.)
  expect(listSubfolders).toHaveBeenCalledTimes(1);
  expect(screen.getByTestId('folder-panel-1')).toBeTruthy();
  expect(screen.getByTestId('subfolder-1-drop')).toBeTruthy();
  // And the press is still waiting for an answer, with the same words on it.
  expect(visible(screen.getByTestId('folder-confirm-1'))).toBe(question);
  // Nothing was stored on the way: the question is pending, not answered.
  expect(excludeSubfolder).not.toHaveBeenCalled();
});

// The other half of staying mounted, and the one that could go wrong quietly:
// a section kept on screen must not be a section that stopped listening. A
// reading pass ending while «Сканування» is shown withdraws the question the
// hidden «Теки» is holding — the numbers it froze were read before that pass —
// and the person who comes back reads why their press is gone instead of
// finding it silently missing.
//
// RED, measured on the variant that isolates this claim (the `hidden` wrapper
// kept, `<Folders>` still behind an `{#if}` inside it): the section is not
// mounted while it is away, so it hears the ending not at all, and the return
// builds it fresh with neither a question nor a note —
//   TestingLibraryElementError: Unable to find an element by:
//   [data-testid="folder-question-withdrawn-1"]
// which is the `waitFor` BEFORE the return, at the moment the ending is
// delivered. Against the plain `{#if}` chain it fails earlier and for the
// weaker reason the test above quotes.
test('a reading pass ending while the section is hidden withdraws the question, and says so on return', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(ONE_ROOT);
  listSubfolders.mockResolvedValue(ONE_SUBFOLDER);
  render(Settings);

  await raiseQuestionInFolders();

  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(foldersShown()).toBe(false);
  // A reading pass has ended: `readSeq` 0 -> 1. This is the fact
  // `Folders.svelte` withdraws on, not the ending itself.
  await emit(endedOnce(1, 1));
  await waitFor(() => expect(screen.getByTestId('folder-question-withdrawn-1')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));

  expect(visible(screen.getByTestId('folder-question-withdrawn-1'))).toBe(
    'The question about “drop” has been withdrawn: indexing has finished and this panel was'
    + ' read again. Press again if you still want to.',
  );
  // Withdrawn, not answered — the pair the note stands on.
  expect(screen.queryByTestId('folder-confirm-1')).toBeNull();
  expect(excludeSubfolder).not.toHaveBeenCalled();
});

// The count the ruling changed. `Folders.svelte` reads `list_tree` on its own
// mount, and its mount is now the WINDOW's: one read when the window opens,
// and none for any number of visits to the section afterwards. Both halves
// are asserted, because "once at the end" alone is satisfied by a section
// that is never read at all.
//
// RED against the `{#if}` chain: nothing reads `list_tree` until the section
// is first opened, so the first claim fails on the window's own mount —
//   AssertionError: expected +0 to be 1 // Object.is equality
// — and on the variant that isolates the second claim (the `hidden` wrapper
// kept, `<Folders>` still behind an `{#if}` inside it) the read comes back per
// visit and the count after the return fails instead:
//   AssertionError: expected 2 to be 1 // Object.is equality
test('the folder list is read once when the window opens, and not again on a section switch', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(ONE_ROOT);
  render(Settings);

  await waitFor(() => expect(listTree.mock.calls.length).toBe(1));
  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Models' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));
  await waitFor(() => expect(foldersShown()).toBe(true));

  expect(listTree.mock.calls.length).toBe(1);
  // The pair: the section IS still listening, so the read it owes a reading
  // pass's ending still happens. A component that had stopped reading
  // altogether would pass the assertion above for the wrong reason.
  await emit(endedOnce(2, 1, 2));
  await waitFor(() => expect(listTree.mock.calls.length).toBe(2));
});

// 🔴 Fix round 1, Important 1 (by ruling). The mask editor shares the folders
// panel and was left behind by the first round: it stayed under an `{#if}`
// inside the section that had become permanent, so a mask typed but not yet
// added, and a question waiting to be confirmed, were still lost by a nav
// click — the very thing F10 ruled against, one component to the left. It is
// mounted under the same `hidden` as the folder list now.
//
// The state pair is the same as the folder tree's: "another section is shown"
// versus "this editor was destroyed". A draft is the sharper half of it,
// because nothing on screen ever said it went: the field simply comes back
// empty, and a person who typed a mask reads that as their own mistake.
//
// RED, measured with `<Masks>` still behind its inner `{#if}`:
//   AssertionError: expected '' to be '*.pdf' // Object.is equality
// and, with that claim and the two beside it neutralised so the last one can
// be reached, the count fails on the second visit's own read:
//   AssertionError: expected "spy" to be called 1 times, but got 2 times
test('a mask draft and its pending question survive a switch to another section and back', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));
  await waitFor(() => expect(screen.getByText('No file mask has been added yet.')).toBeTruthy());
  const draft = () => screen.getByLabelText('New mask:') as HTMLInputElement;
  await fireEvent.input(draft(), { target: { value: '*.pdf' } });
  // And a question standing over it, which is the other thing a nav click used
  // to discard: this press has already cost a `mask_preview` over every
  // indexed path, and answering it is the only thing that stores anything.
  await fireEvent.click(screen.getByRole('button', { name: 'Add a mask' }));
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  const cost = visible(screen.getByTestId('mask-confirm-cost'));

  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(foldersShown()).toBe(false);

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));

  expect(foldersShown()).toBe(true);
  // The draft is still what was typed, in the same field.
  expect(draft().value).toBe('*.pdf');
  // The question is still standing, with the numbers it was asked on.
  expect(visible(screen.getByTestId('mask-confirm-cost'))).toBe(cost);
  // And still unanswered: the preview was asked once, and nothing was stored.
  expect(maskPreview).toHaveBeenCalledTimes(1);
  expect(addMask).not.toHaveBeenCalled();
  // The pair, and the reason the editor is mounted rather than re-created:
  // one read of the mask list for the window, not one per visit.
  expect(listMasks).toHaveBeenCalledTimes(1);
});

// ---------------------------------------------------------------------------
// 🔴 Independent review, finding 2. The half of F10 (Task 10e) that kept more
// than it should have: `<Masks>` stays mounted for the window's life, so a
// pending question survives a nav click — and so does the `mask_preview`
// result frozen inside it, which a reading pass ending underneath makes wrong.
// The three tests below are the whole window, with the real `Masks` and the
// real controller, because the event has to travel the way it does in
// production: through `Settings.svelte`'s controller into a section that is
// `hidden` rather than unmounted.
// ---------------------------------------------------------------------------

// RED, with the `readSeq` subscription deleted from `Masks.svelte`: the
// question is still standing on the return to Folders, and
// `expect(screen.queryByTestId('mask-confirm')).toBeNull()` fails as
// "AssertionError: expected <div data-testid="mask-confirm">…(4)</div> to be
// null". The neighbour below goes red in the same run, on the note that never
// appears: "TestingLibraryElementError: Unable to find an element by:
// [data-testid="mask-question-withdrawn"]".
test('a reading pass ending while the folders section is hidden withdraws the mask question, keeps the draft, and says so on return', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));
  await waitFor(() => expect(screen.getByText('No file mask has been added yet.')).toBeTruthy());
  const draft = () => screen.getByLabelText('New mask:') as HTMLInputElement;
  await fireEvent.input(draft(), { target: { value: '*.txt' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Add a mask' }));
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  // The state the withdrawal is about is the question STANDING beforehand, so
  // it is asserted rather than assumed.
  expect(screen.queryByTestId('mask-question-withdrawn')).toBeNull();

  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(foldersShown()).toBe(false);

  // A reading pass ends while the panel is hidden: `readSeq` 0 -> 1, which is
  // the fact the withdrawal is keyed on.
  await emit(endedOnce(10, 1));
  await tick();

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));
  expect(foldersShown()).toBe(true);
  expect(screen.queryByTestId('mask-confirm')).toBeNull();
  expect(visible(screen.getByTestId('mask-question-withdrawn'))).toBe(
    'The question about mask “*.txt” has been withdrawn: indexing has finished and the estimate is stale. Press again if you still want to.',
  );
  // The draft is what the person typed and no scan makes it wrong — this is the
  // half F10 was right about, and withdrawing the question must not take it.
  expect(draft().value).toBe('*.txt');
  // Nothing was stored on the frozen estimate, and nothing re-asked behind the
  // person's back: the next preview is the one THEY press for.
  expect(addMask).not.toHaveBeenCalled();
  expect(maskPreview).toHaveBeenCalledTimes(1);

  // And pressing again asks it afresh, against the index as it now is — the
  // other direction, and what makes the withdrawal a withdrawal rather than a
  // dead end.
  maskPreview.mockResolvedValue({ paths: 100, documents: 100 });
  await fireEvent.click(screen.getByRole('button', { name: 'Add a mask' }));
  await waitFor(() => expect(maskPreview).toHaveBeenCalledTimes(2));
  expect(visible(screen.getByTestId('mask-confirm-cost'))).toContain('100');
  expect(screen.queryByTestId('mask-question-withdrawn')).toBeNull();
});

// The generation half. A `checking` question is a `mask_preview` already on the
// wire, and its answer is about the index as it stood at the press; landing
// after the pass, it would raise the very question the withdrawal has just
// taken away, carrying numbers a pass has already invalidated.
//
// RED, with `++previews` deleted from `withdrawQuestion` and the subscription
// otherwise intact — so the withdrawal itself still happens and this test is
// about the generation alone: the stale reply lands, raises the question again,
// and `expect(screen.queryByTestId('mask-confirm')).toBeNull()` fails as
// "AssertionError: expected <div data-testid="mask-confirm">…(4)</div> to be
// null".
test('a mask preview that answers after the reading pass ended raises nothing', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));
  await waitFor(() => expect(screen.getByText('No file mask has been added yet.')).toBeTruthy());
  const reply = deferredPromise<{ paths: number; documents: number }>();
  maskPreview.mockReturnValueOnce(reply.promise);
  await fireEvent.input(screen.getByLabelText('New mask:'), { target: { value: '*.md' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Add a mask' }));
  // In flight, not answered: the question on screen is the «checking» one.
  await waitFor(() => expect(screen.getByTestId('mask-confirm')).toBeTruthy());
  expect(screen.queryByTestId('mask-confirm-cost')).toBeNull();

  await emit(endedOnce(11, 1));
  await tick();
  expect(visible(screen.getByTestId('mask-question-withdrawn'))).toContain('*.md');

  // The pre-scan answer, arriving after the pass that made it wrong.
  reply.resolve({ paths: 7, documents: 3 });
  await tick();
  await tick();
  expect(screen.queryByTestId('mask-confirm')).toBeNull();
  expect(screen.queryByTestId('mask-confirm-cost')).toBeNull();
  // The note is still the one the withdrawal wrote, not replaced by a question.
  expect(visible(screen.getByTestId('mask-question-withdrawn'))).toContain('*.md');
});

// The pair, and the state that separates «a reading pass ended» from «a job
// ended»: an `embedOnly` run ends like any other and reads no folder, so
// nothing it did makes a frozen preview wrong. Withdrawing there would discard
// a press and print "indexing has finished" over a run that read nothing —
// the same distinction `Folders.svelte`'s own subscription is written on.
test('an ending that read no folder leaves the mask question standing', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));
  await waitFor(() => expect(screen.getByText('No file mask has been added yet.')).toBeTruthy());
  await fireEvent.input(screen.getByLabelText('New mask:'), { target: { value: '*.rtf' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Add a mask' }));
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  const cost = visible(screen.getByTestId('mask-confirm-cost'));

  // `readSeq` STILL 0 — the seeded value — on an ending with a higher revision,
  // and `files` still 0 as well, so neither of the two facts a `mask_preview`
  // result is made of has moved.
  await emit(endedOnce(12, 0));
  await tick();

  expect(visible(screen.getByTestId('mask-confirm-cost'))).toBe(cost);
  expect(screen.queryByTestId('mask-question-withdrawn')).toBeNull();
});

// 🔴 External review round 1, Minor 2. The other way the same estimate goes
// wrong. `mask_preview` counts indexed paths and the documents that would stop
// being findable; removing a watched folder deletes those rows and ends through
// `finish(Terminal::Idle, Some(files))` (`bridge.rs`), which moves
// `ScanState.files` and never `readSeq`. A withdrawal keyed on `readSeq` alone
// left the question quoting a count from before the removal — the harm inverts
// rather than disappearing, since the frozen number then overstates what the
// mask takes.
//
// RED, with `filesMoved` deleted from the subscription in `Masks.svelte`: the
// question stands and
// `expect(screen.queryByTestId('mask-confirm')).toBeNull()` fails as
// "AssertionError: expected <div data-testid="mask-confirm">…(4)</div> to be
// null".
test('a removal that dropped the index file count withdraws the mask question, and a running tick that moved nothing keeps it', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));
  await waitFor(() => expect(screen.getByText('No file mask has been added yet.')).toBeTruthy());
  // An index holding three files, delivered BEFORE the question is raised: the
  // editor seeds its count from a store that starts at nought, so a removal has
  // to have something to remove and the question has to be asked over the state
  // that is already on screen.
  await emit({ revision: 29, files: 3, readSeq: 0, jobsDone: 0, lastReading: null, snapshot: { kind: 'idle' } });
  await tick();

  await fireEvent.input(screen.getByLabelText('New mask:'), { target: { value: '*.doc' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Add a mask' }));
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  const cost = visible(screen.getByTestId('mask-confirm-cost'));

  // The pair, and it goes FIRST so the withdrawal below cannot be credited to
  // it: a running tick that moves neither counter must leave the question
  // exactly as it was. `files` stays at the 3 the editor has already seen.
  await emit({
    revision: 30, files: 3, readSeq: 0, jobsDone: 0, lastReading: null,
    snapshot: { kind: 'running', cancellable: false, phase: { kind: 'other', job: 'probe' } },
  });
  await tick();
  await emit({
    revision: 31, files: 3, readSeq: 0, jobsDone: 0, lastReading: null,
    snapshot: {
      kind: 'running', cancellable: false,
      phase: { kind: 'removing', rootPath: '/home/a/papers' },
    },
  });
  await tick();
  expect(visible(screen.getByTestId('mask-confirm-cost'))).toBe(cost);
  expect(screen.queryByTestId('mask-question-withdrawn')).toBeNull();

  // The removal's own ending: a bare `idle` with a LOWER `files`, `readSeq`
  // untouched — the one shape a `readSeq` trigger cannot see.
  await emit({ revision: 32, files: 0, readSeq: 0, jobsDone: 1, lastReading: null, snapshot: { kind: 'idle' } });
  await tick();

  expect(screen.queryByTestId('mask-confirm')).toBeNull();
  expect(visible(screen.getByTestId('mask-question-withdrawn'))).toBe(
    'The question about mask “*.doc” has been withdrawn: indexing has finished and the estimate is stale. Press again if you still want to.',
  );
  // The draft survives a removal the same way it survives a reading pass.
  expect((screen.getByLabelText('New mask:') as HTMLInputElement).value).toBe('*.doc');
  expect(addMask).not.toHaveBeenCalled();
});

// ---------------------------------------------------------------------------
// 🔴 Independent review, finding 3. The trigger used to be an EDGE — «the
// snapshot this window last SAW was `running` and this one is not» — and two
// reachable sequences never supply that edge. A model adoption can start and
// end inside the window between `jobs.mount` opening the subscription and its
// first `job_status` answering; and a terminal snapshot can arrive ahead of the
// older `running` one, which `apply` (`jobs.ts`) then correctly drops. In both
// the ending is delivered AND accepted, and the section still drew the state
// from before the adoption. `ScanState.jobsDone` is the same fact stated
// monotonically, and it cannot be missed.
//
// Its ordered-pair control is `a model adoption leaving running re-reads, and
// the queue it created is offered` above, which delivers the `running` snapshot
// first and is untouched by this change.
// ---------------------------------------------------------------------------

// RED, with `jobsDoneChanged` in `Settings.svelte` put back to
// `seenSnapshot.kind === 'running' && scan.snapshot.kind !== 'running'`: the
// first `waitFor` below times out, reporting
// "AssertionError: expected 3 to be 4 // Object.is equality" — the ending was
// accepted and nothing re-read the index.
test('an ending whose running snapshot this window never saw still re-reads, and offers the queue the adoption created', async () => {
  setLocale('en'); // seed, do not inherit
  modelSettings.mockResolvedValue(
    readFixture({ indexedFiles: 3, totalChunks: 3, pendingChunks: 0, embeddingModel: null }),
  );
  render(Settings);
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());

  // The state before the adoption: three files, nothing queued, no model. The
  // window has now acted on `files: 3`, so that trigger is spent.
  await emit({ revision: 1, files: 3, readSeq: 0, jobsDone: 0, lastReading: null, snapshot: { kind: 'idle' } });
  await waitFor(() => expect(visible(screen.getByTestId('indexing-index-files'))).toContain('3'));
  const baseline = modelSettings.mock.calls.length;
  // The absence of the offer beforehand is what this test is about, so it is
  // asserted rather than assumed.
  expect(screen.queryByTestId('scanning-continue')).toBeNull();

  // The adoption ran and ended while nothing was listening for its start: the
  // only snapshot delivered is the terminal one, and it moves neither `files`
  // nor `readSeq`. What it does move is the count of endings.
  modelSettings.mockResolvedValue(
    readFixture({ indexedFiles: 3, totalChunks: 3, pendingChunks: 3, embeddingModel: 'review/model' }),
  );
  await emit({ revision: 3, files: 3, readSeq: 0, jobsDone: 1, lastReading: null, snapshot: { kind: 'idle' } });
  await waitFor(() => expect(modelSettings.mock.calls.length).toBe(baseline + 1));
  await waitFor(() => expect(screen.getByTestId('indexing-index-pending-chunks')).toBeTruthy());
  expect(visible(screen.getByTestId('scanning-continue'))).toBe('Continue embedding');

  // The adoption's own `running` snapshot, arriving late and older: `apply`
  // drops it, this window's subscriber is never called, and nothing is re-read
  // a second time.
  await emit({
    revision: 2, files: 3, readSeq: 0, jobsDone: 0, lastReading: null,
    snapshot: { kind: 'running', cancellable: false, phase: { kind: 'other', job: 'modelAdoption' } },
  });
  await tick();
  expect(modelSettings.mock.calls.length).toBe(baseline + 1);

  // The pair, and the one this trigger could be written wrong by: a NEWER
  // running snapshot that carries the same count of endings is a job starting,
  // not one that ended, and it must re-read nothing. Without it, "re-read
  // whenever anything arrives" would satisfy every assertion above.
  await emit({
    revision: 4, files: 3, readSeq: 0, jobsDone: 1, lastReading: null,
    snapshot: { kind: 'running', cancellable: false, phase: { kind: 'other', job: 'modelAdoption' } },
  });
  await tick();
  expect(modelSettings.mock.calls.length).toBe(baseline + 1);
});

// ---------------------------------------------------------------------------
// Task 4 (review P2-1): Models joins Folders as mounted-hidden — an
// unconfirmed embedding choice, a command still in flight, its retirement
// report and a rejection sentence are the same class of "a question this
// build is waiting on an answer for" F10 already names for the folder tree
// and the mask editor. This drives the choice through the REAL `Settings`
// window (not `Models` standalone, which `Models.test.ts` already owns), so
// the section switch itself is real too.
// ---------------------------------------------------------------------------

test('mutation_outcome_survives_a_section_switch', async () => {
  setLocale('en'); // seed, do not inherit
  const onModel: ModelSettings = {
    key: { kind: 'present' },
    index: {
      kind: 'read', embeddedChunks: 5, embeddedChunksEverywhere: 5, totalChunks: 12,
      failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null,
      embeddingModel: 'emb-1', searchTextArm: true, searchContentArm: true,
    },
    platform: 'linux',
  };
  modelSettings.mockResolvedValue(onModel);
  providerModels.mockImplementation((role: string) => Promise.resolve(
    role === 'embedding'
      ? {
        entries: [
          { id: 'emb-1', name: 'emb-1', inputLimit: { kind: 'notStated' }, price: { kind: 'notStated' }, refusal: null },
          { id: 'emb-2', name: 'emb-2', inputLimit: { kind: 'notStated' }, price: { kind: 'notStated' }, refusal: null },
        ],
        unreadable: 0, unreadableRecords: [],
      }
      : { entries: [], unreadable: 0, unreadableRecords: [] },
  ));

  // A deferred `set_embedding_model`: this test starts the command and holds
  // it open across the section switch, the same way `deferredPromise` does
  // in `Models.test.ts`.
  let rejectAdopt!: (e: unknown) => void;
  setEmbeddingModel.mockImplementation(() => new Promise((_res, rej) => { rejectAdopt = rej; }));

  render(Settings); // opens on Models by default
  const select = () => screen.getByTestId('model-selection') as HTMLSelectElement;
  await waitFor(() => expect(select().value).toBe('emb-1'));

  await fireEvent.change(select(), { target: { value: 'emb-2' } });
  await screen.findByTestId('model-embedding-confirm-title');
  await fireEvent.click(screen.getByTestId('model-embedding-discard'));
  await waitFor(() => expect(setEmbeddingModel).toHaveBeenCalledTimes(1));

  // The section switch itself: away, then back, with the command still
  // unanswered.
  await fireEvent.click(screen.getByRole('button', { name: 'Scanning' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Models' }));

  // Still disabled — the busy guard survived the round trip along with the
  // component itself — and a second write genuinely cannot start: the busy
  // check inside the handler, not only the `disabled` attribute a real
  // person would meet, refuses a script-dispatched pick too.
  expect(select().disabled).toBe(true);
  await fireEvent.change(select(), { target: { value: 'emb-1' } });
  expect(setEmbeddingModel).toHaveBeenCalledTimes(1);

  // The command is refused, and the re-read that follows it comes back
  // unreadable — both reported, on the section a person is actually looking
  // at again.
  const SENTENCE = 'the provider refused this model';
  modelSettings.mockResolvedValue({
    key: { kind: 'present' },
    index: { kind: 'unreadable', cause: 'readFailed', reason: 'PRIVATE-DIAGNOSTIC' },
    platform: 'linux',
  });
  rejectAdopt(new Error(SENTENCE));

  await waitFor(() => expect(screen.getByTestId('model-embedding-error')).toBeTruthy());
  expect(screen.getByTestId('model-embedding-error').textContent).toContain(SENTENCE);
  expect(screen.getByTestId('model-index-failure')).toBeTruthy();
});

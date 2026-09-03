import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/svelte';
import { expect, test, afterEach, vi } from 'vitest';
import Settings from './Settings.svelte';
import { setLocale } from '../i18n';
import type { AppPrefs, ModelSettings } from '../lib/ipc';

// 🔴 Annotated, so the compiler checks it. This fixture crosses a `vi.mock`
// factory, whose return type is `unknown` — Task 3's three new REQUIRED fields
// on the `read` arm went unchecked here until the §9.3 section started reading
// them, and a missing `lastIndexedAt` reached `Intl.DateTimeFormat` as
// `undefined`.
const SETTINGS: ModelSettings = {
  key: { kind: 'absent' },
  index: {
    kind: 'read', embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0,
    failedChunks: 0, indexedFiles: 0, lastIndexedAt: null,
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
vi.mock('../lib/ipc', () => ({
  modelSettings: () => Promise.resolve(SETTINGS),
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
  jobStatus: () => Promise.resolve({ running: false }),
  startWalkJob: vi.fn(),
  startEmbedJob: vi.fn(),
  cancelJob: vi.fn(),
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

afterEach(() => {
  cleanup();
  setLocale('en'); // the store outlives the component; leave it as found
});

test('shows all four section names, in the spec order', () => {
  setLocale('en'); // seed, do not inherit: an earlier sibling switching the language must not decide this test
  render(Settings);
  const nav = screen.getByRole('navigation');
  // spec order: Models, Folders, Indexing, Application — read as one string so
  // a swap in order fails even though all four words are still present.
  expect(nav.textContent).toBe('ModelsFoldersIndexingApplication');
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
// test's own history already predicted running out of them. `NOT_READY_ID`,
// `notReadyLabel` and the `aria-describedby` wiring stay in `Settings.svelte`
// itself (that cleanup is a later task's, not this one's — see the task
// brief), so what is still worth pinning here is the invariant the four
// sections owe together now that all of them are built: none is disabled, and
// none is described by an id nothing renders.
test('no section claims to be disabled, and none is described by an id nothing renders', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);

  for (const name of ['Models', 'Folders', 'Indexing', 'Application']) {
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

  // Task 6: the Indexing panel is the §9.3 section now, and this fixture is an
  // index nothing has ever been added to — so what a person reads is the count
  // and the sentence that stands where a date would be, never a blank and never
  // an epoch. Measured from a real render rather than hand-edited, the way every
  // earlier version of this string was.
  await fireEvent.click(screen.getByRole('button', { name: 'Indexing' }));
  await waitFor(() => expect(screen.getByTestId('indexing-index-files')).toBeTruthy());
  expect(panel()?.textContent?.replace(/\s+/g, ' ').trim())
    .toBe('Indexing The index holds 0 files. Nothing has been indexed yet.');
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
  expect(screen.queryByText('This section is not ready yet.')).toBeNull();
});

test('labels stay correct across a language switch after mount', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);

  // M1 (review): read a real sentence once under 'en' BEFORE switching, so a
  // $derived missing `void $locale` still caches an English value here — the
  // mutant only dies if the read after the switch is a genuinely later one.
  // Task 7 built Application, so the placeholder this used to pin is gone; the
  // shortcut status sentence is `Application.svelte`'s own `$derived.by`, which
  // is the same shape `Indexing.svelte`'s strings below are read for.
  await fireEvent.click(screen.getByRole('button', { name: 'Application' }));
  await waitFor(() => expect(screen.getByTestId('application-shortcut-status')).toBeTruthy());
  expect(screen.getByText('This shortcut is registered with the system.')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Models' }));

  // 🔴 The same lesson for the BUILT section, and it needs a different shape
  // (review, Important 2). `Indexing.svelte`'s strings come from that
  // component's own `$derived.by`, and the component is destroyed by every nav
  // change — so a version of this that clicked Індексація AFTER the switch
  // would mount it fresh under `uk` and read Ukrainian whether or not the
  // anchor is there. The section is opened here, under 'en', and left mounted
  // ACROSS `setLocale` so the read below is genuinely a later read of the same
  // derived. Measured: with `void $locale` deleted from `filesLine`, this test
  // fails on the Ukrainian assertion below and the English one still resolves.
  await fireEvent.click(screen.getByRole('button', { name: 'Indexing' }));
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
  expect(nav.textContent).toBe('МоделіТекиІндексаціяЗастосунок');
  await fireEvent.click(screen.getByRole('button', { name: 'Моделі' }));
  expect(screen.getByRole('heading', { name: 'Моделі' })).toBeTruthy();

  // Application, re-mounted fresh under 'uk' — this is a NEW mount (every nav
  // change destroys the previous section), so it reads Ukrainian from its own
  // first `app_prefs` resolution rather than from anything cached.
  await fireEvent.click(screen.getByRole('button', { name: 'Застосунок' }));
  await waitFor(() => expect(screen.getByTestId('application-shortcut-status')).toBeTruthy());
  expect(screen.getByText('Це скорочення зареєстровано в системі.')).toBeTruthy();
});

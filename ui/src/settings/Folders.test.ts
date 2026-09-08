import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import Folders from './Folders.svelte';
import { setLocale, t } from '../i18n';
import { createJobController } from './jobs';
import { tick } from 'svelte';
import { get } from 'svelte/store';
import type {
  Counts, ScanState, StoredExclusion, Subfolder, SubfolderListing, SubfolderState, TreeFile,
  TreeListing, TreeRoot,
} from '../lib/ipc';

// Mocked in the shape Arms.test.ts:5-6 / Models.test.ts:13-30 already use —
// the typed wrappers, not the raw `invoke`.
const listTree = vi.fn();
const addWatchedFolder = vi.fn();
const removeWatchedFolder = vi.fn();
// PR 8a Task 5. Mocked as wrappers for the same reason the three above are:
// what this file is about is the screen, not the wire encoding, and
// `ipc.test.ts` owns the argument names each of these sends.
const listSubfolders = vi.fn();
const listExclusions = vi.fn();
const excludeSubfolder = vi.fn();
const includeSubfolder = vi.fn();
// The job commands are the REAL wrappers, deliberately: they are what carry
// the `'start_scan_job'` wire string this file asserts is never sent, and a
// mock of them would make that assertion about this file's own fake.
vi.mock('../lib/ipc', async (real) => ({
  ...(await real<Record<string, unknown>>()),
  listTree: (...a: unknown[]) => listTree(...a),
  addWatchedFolder: (...a: unknown[]) => addWatchedFolder(...a),
  removeWatchedFolder: (...a: unknown[]) => removeWatchedFolder(...a),
  listSubfolders: (...a: unknown[]) => listSubfolders(...a),
  listExclusions: (...a: unknown[]) => listExclusions(...a),
  excludeSubfolder: (...a: unknown[]) => excludeSubfolder(...a),
  includeSubfolder: (...a: unknown[]) => includeSubfolder(...a),
}));

// The dialog plugin needs its own mock — a separate module from `../lib/ipc`.
const open = vi.fn();
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: (...a: unknown[]) => open(...a),
}));

// this ruling’s own guard (P3-6 review), and it is a live one: this component
// holds a controller that CAN start a scan. The assertion sits at the one
// boundary every command crosses regardless of what the wrapper is called —
// the raw `invoke` and the wire string it carries — because a wrapper renamed
// in a later task must not quietly retire the guard. That renaming has now
// happened twice: `'start_walk_job'` became `'start_scan_job'` when the scan
// became one job, and the guard moved with it rather than being deleted.
// `../lib/ipc`'s own real module imports `invoke` from this path, so mocking
// it here intercepts every call the real job wrappers would make.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...a: unknown[]) => invoke(...a),
  Channel: class {},
}));

// Tauri's event module, which `listenScanProgress` imports dynamically. Faked
// at the module boundary for the same reason `invoke` is: what the controller
// registers, and on what name, is the real wrapper's doing.
const listen = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({
  listen: (...a: unknown[]) => listen(...a),
}));
let deliver: ((state: ScanState) => void) | null = null;

beforeEach(() => {
  listen.mockReset();
  deliver = null;
  revision = 0;
  listen.mockImplementation((_name: string, cb: (e: { payload: ScanState }) => void) => {
    deliver = (state: ScanState) => cb({ payload: state });
    return Promise.resolve(() => {});
  });
  listTree.mockReset();
  addWatchedFolder.mockReset();
  removeWatchedFolder.mockReset();
  invoke.mockReset();
  // 🔴 A PROMISE for every command, and a real state for `job_status`. A bare
  // `mockReset` answers `undefined`, and a mounted controller then calls
  // `.then` on it — an unhandled rejection that leaves every test green and the
  // run red. `IDLE_SCAN` is what a process in which nothing has happened yet
  // reports (`ScanState::default`).
  invoke.mockImplementation((cmd: string) =>
    Promise.resolve(cmd === 'job_status' ? IDLE_SCAN : undefined));
  open.mockReset();
  listSubfolders.mockReset();
  listExclusions.mockReset();
  excludeSubfolder.mockReset();
  includeSubfolder.mockReset();
  // Defaults for the tests that never expand a row: an empty listing and no
  // rules. A test that expands states its own.
  listSubfolders.mockResolvedValue({ entries: [], unnameable: 0 });
  listExclusions.mockResolvedValue([]);
});
afterEach(() => {
  cleanup();
  setLocale('en'); // the store outlives the component; leave it as found
});

function root(overrides: Partial<TreeRoot> = {}): TreeRoot {
  return {
    rootId: 1,
    absolutePath: '/synthetic/root',
    name: 'root',
    files: [],
    ...overrides,
  };
}
function listing(roots: TreeRoot[]): TreeListing {
  return { roots, recents: [] };
}

test('empty state: a sentence and the add control, not a bare list', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([]));
  render(Folders, { props: { jobs: createJobController() } });

  await waitFor(() => expect(screen.getByText('No folder has been added yet.')).toBeTruthy());
  expect(screen.getByRole('button', { name: 'Add a folder' })).toBeTruthy();
});

test('adding a folder saves the picked path, the list re-reads, and no job starts', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValueOnce(listing([]));
  listTree.mockResolvedValueOnce(
    listing([root({ rootId: 7, absolutePath: '/synthetic/reports', files: [] })]),
  );
  open.mockResolvedValue('/synthetic/reports');
  addWatchedFolder.mockResolvedValue(7);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('No folder has been added yet.')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Add a folder' }));

  await waitFor(() => expect(screen.getByText('/synthetic/reports')).toBeTruthy());
  expect(addWatchedFolder).toHaveBeenCalledWith('/synthetic/reports');
  // Re-read, not a locally patched list: the second listTree call is what the
  // fixture above returns, and its shape (rootId 7) is what the row must show.
  expect(listTree).toHaveBeenCalledTimes(2);
  // Adding a folder starts nothing. No assertion about the list would
  // notice a stray scan — this checks the wire protocol string directly
  // (matched by first argument alone, so it does not depend on a second
  // argument's shape), which stays armed no matter what a future wrapper
  // around it is named (P3-6 review — the previous form named an export that
  // did not exist and could never fail).
  expect(invoke.mock.calls.some(([command]) => command === 'start_scan_job')).toBe(false);
  // §9.2, Task 8: owner's ruling — nothing here starts a scan, and the note
  // says where to go do that instead.
  expect(screen.getByTestId('folders-added-note')).toBeTruthy();
});

// §9.2, Task 8. The per-row Scan button is gone (`Scanning.svelte` owns the
// one control now); this pins the three states its note replaces it with —
// both directions of "the last press was a successful add", and the direction
// that must NOT show it at all. Fix round 1, Minor 3, adds two more pairs
// (`Folders.svelte:71-73` and `:1479` each claimed one, unguarded): the note
// follows a language switch like every other sentence on this screen, and it
// survives a job ending — nothing about a scan changes what it is telling a
// person to go and do next, unlike the withdrawn-question note beside it.
test('a successful add shows the note and no row offers to scan; it follows a language switch and survives a job ending; the next remove takes it away; a rejected add shows neither', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValueOnce(listing([]));
  listTree.mockResolvedValueOnce(
    listing([root({ rootId: 7, absolutePath: '/synthetic/reports', files: [] })]),
  );
  // The job ending below re-reads the list too (`reread(true)`), one call
  // before the remove's own — nothing about the list has changed by then.
  listTree.mockResolvedValueOnce(
    listing([root({ rootId: 7, absolutePath: '/synthetic/reports', files: [] })]),
  );
  listTree.mockResolvedValueOnce(listing([])); // after the remove below
  open.mockResolvedValue('/synthetic/reports');
  addWatchedFolder.mockResolvedValue(7);
  removeWatchedFolder.mockResolvedValue(1);

  const { jobs } = renderWatching();
  await waitFor(() => expect(screen.getByText('No folder has been added yet.')).toBeTruthy());
  expect(screen.queryByTestId('folders-added-note')).toBeNull();

  await fireEvent.click(screen.getByRole('button', { name: 'Add a folder' }));
  await waitFor(() => expect(screen.getByText('/synthetic/reports')).toBeTruthy());

  expect(screen.getByTestId('folders-added-note')).toBeTruthy();
  expect(visibleText(screen.getByTestId('folders-added-note'))).toBe(
    'Folder added. Exclude subfolders and set masks, then press “Scan” in the Scanning section.',
  );
  // No row offers to scan any more: the only control this row carries besides
  // Subfolders is Remove.
  expect(screen.queryAllByRole('button', { name: /^Scan/ })).toHaveLength(0);

  // Both directions of the language switch, the note still on screen.
  setLocale('uk');
  await tick();
  expect(visibleText(screen.getByTestId('folders-added-note'))).toBe(
    'Теку додано. Виключіть підтеки й задайте маски, тоді натисніть «Сканувати» у розділі «Сканування».',
  );
  expect(screen.queryByText('Folder added. Exclude subfolders and set masks, then press “Scan” in the Scanning section.'))
    .toBeNull();
  setLocale('en');
  await tick();
  expect(visibleText(screen.getByTestId('folders-added-note'))).toBe(
    'Folder added. Exclude subfolders and set masks, then press “Scan” in the Scanning section.',
  );

  // A job ending changes nothing about what this note is telling a person to
  // go and do next — unlike the withdrawn-question note beside it, which a
  // reading pass ending really does invalidate.
  void jobs.scan('full');
  await endScan();
  expect(screen.getByTestId('folders-added-note')).toBeTruthy();

  // Task 9: the press asks first, and it is the press — not the answer — that
  // takes this note away, for the rule the note itself is written under: what
  // the last press led to stands until the next one.
  await fireEvent.click(screen.getByRole('button', { name: 'Remove /synthetic/reports' }));
  expect(screen.queryByTestId('folders-added-note')).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm removing /synthetic/reports' }));
  await waitFor(() => expect(screen.getByText('No folder has been added yet.')).toBeTruthy());
  expect(screen.queryByTestId('folders-added-note')).toBeNull();

  // A rejected add never shows the note — the add itself did not succeed.
  addWatchedFolder.mockRejectedValueOnce(new Error('This path is already watched.'));
  await fireEvent.click(screen.getByRole('button', { name: 'Add a folder' }));
  await waitFor(() => expect(screen.getByText('This path is already watched.')).toBeTruthy());
  expect(screen.queryByTestId('folders-added-note')).toBeNull();
});

test('a cancelled folder dialog calls nothing', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([]));
  open.mockResolvedValue(null);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(listTree).toHaveBeenCalledTimes(1));

  await fireEvent.click(screen.getByRole('button', { name: 'Add a folder' }));
  await Promise.resolve();
  await Promise.resolve();

  expect(addWatchedFolder).not.toHaveBeenCalled();
  expect(listTree).toHaveBeenCalledTimes(1); // no re-read: nothing changed
});

test('removing targets that row\'s rootId, not a position, with two roots in the fixture', async () => {
  setLocale('en'); // seed, do not inherit
  const alpha = root({ rootId: 3, absolutePath: '/synthetic/alpha', files: [] });
  const beta = root({ rootId: 9, absolutePath: '/synthetic/beta', files: [] });
  listTree.mockResolvedValueOnce(listing([alpha, beta]));
  listTree.mockResolvedValueOnce(listing([alpha])); // beta gone after removal
  removeWatchedFolder.mockResolvedValue(1);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/beta')).toBeTruthy());

  // The SECOND row is removed, not the first — a positional implementation
  // (always the 0th root) would call removeWatchedFolder(3) here instead.
  // Named by its own path (P2-5 review): two "Remove" buttons on screen share
  // no other accessible name, so the query has to be the qualified one.
  await fireEvent.click(within(screen.getByTestId('folder-row-9')).getByRole('button', { name: 'Remove /synthetic/beta' }));
  // Task 9: and the confirmation is the second row's own, named by that row's
  // path for the same reason the Remove button is.
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm removing /synthetic/beta' }));

  // 🔴 The path travels with the id, and it is the path of THE ROW that was
  // pressed. `bridge.rs` deletes the row only while that id still names that
  // path, so an id sent alone would be the stale-then-act shape the compare
  // exists to close — and a path taken from anywhere but the row a person read
  // would agree with the id by construction and check nothing.
  expect(removeWatchedFolder).toHaveBeenCalledWith(9, '/synthetic/beta');
  await waitFor(() => expect(screen.queryByText('/synthetic/beta')).toBeNull());
  expect(screen.getByText('/synthetic/alpha')).toBeTruthy(); // untouched
  expect(listTree).toHaveBeenCalledTimes(2); // re-read after removal
});

test('each row shows its document count, and a zero-file root says zero rather than nothing', async () => {
  setLocale('en'); // seed, do not inherit
  const many = root({ rootId: 1, absolutePath: '/synthetic/many', files: [
    { relativePath: 'a.md', documentId: 'd1' },
    { relativePath: 'b.md', documentId: 'd2' },
    { relativePath: 'c.md', documentId: 'd3' },
  ] });
  const none = root({ rootId: 2, absolutePath: '/synthetic/none', files: [] });
  listTree.mockResolvedValue(listing([many, none]));

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/many')).toBeTruthy());

  // Computed through the same catalogue message the component itself uses,
  // not a hand-duplicated literal — a duplicated form is the "two truths,
  // one message" trap this project already paid for. `settings_folders_indexed`
  // (P2-4 review), not the shared `indexed_documents`: the subject is the
  // index, not the folder — see the comment on that key in catalog.ts.
  expect(screen.getByText(t('settings_folders_indexed', { count: 3 }))).toBeTruthy();
  expect(screen.getByText(t('settings_folders_indexed', { count: 0 }))).toBeTruthy();
  // And the literal beside it, for the half `t()` cannot see. A message read
  // through the same catalogue the component reads changes with it: strip the
  // colon from the English string and both sides of the assertion above move
  // together, still equal, still green — the symmetric weakening this project
  // has already paid for. The Ukrainian row states its literal for this reason
  // (below); the English one did not, and its separator was undefended.
  expect(screen.getByText('Indexed: 3 documents')).toBeTruthy();
  expect(screen.getByText('Indexed: 0 documents')).toBeTruthy();
});

test('a rejected add shows the backend sentence verbatim, and the list keeps its prior state', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([]));
  open.mockResolvedValue('/synthetic/locked');
  addWatchedFolder.mockRejectedValue(new Error('This path is already watched.'));

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('No folder has been added yet.')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Add a folder' }));

  await waitFor(() => expect(screen.getByText('This path is already watched.')).toBeTruthy());
  // A rejection is not a re-read: only the mount call happened.
  expect(listTree).toHaveBeenCalledTimes(1);
});

test('a rejected remove shows the backend sentence verbatim, and the row stays', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 4, absolutePath: '/synthetic/stuck', files: [] })]));
  removeWatchedFolder.mockRejectedValue(new Error('The index is busy right now.'));

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/stuck')).toBeTruthy());

  // P2-5: the button's accessible name carries its row's path.
  await fireEvent.click(screen.getByRole('button', { name: 'Remove /synthetic/stuck' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm removing /synthetic/stuck' }));

  await waitFor(() => expect(screen.getByText('The index is busy right now.')).toBeTruthy());
  expect(screen.getByText('/synthetic/stuck')).toBeTruthy(); // still there
});

test('a failed initial read shows the lead-in sentence and the backend sentence beside it', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockRejectedValue(new Error('The index is not open yet.'));

  render(Folders, { props: { jobs: createJobController() } });

  await waitFor(() => expect(screen.getByText('The list of folders could not be read.')).toBeTruthy());
  expect(screen.getByText('The index is not open yet.')).toBeTruthy();
});

// P1-2 review: `refresh()` used to leave `loadError` set forever once a
// rejected `list_tree` had set it — the failed mount here is exactly the
// state the review reproduced ("the index is not open yet" being the
// rejection that caught the P0 this whole PR started from). A later
// successful add re-reads the list, and that success has to clear the
// stale banner.
test('a successful add after a failed initial read clears the load-failure banner', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockRejectedValueOnce(new Error('The index is not open yet.'));
  listTree.mockResolvedValueOnce(
    listing([root({ rootId: 5, absolutePath: '/synthetic/recovered', files: [] })]),
  );
  open.mockResolvedValue('/synthetic/recovered');
  addWatchedFolder.mockResolvedValue(5);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('The list of folders could not be read.')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Add a folder' }));

  await waitFor(() => expect(screen.getByText('/synthetic/recovered')).toBeTruthy());
  expect(screen.queryByText('The list of folders could not be read.')).toBeNull();
});

// P2-3 review: `refresh()` used to sit inside the SAME try as the action
// call, so a re-read that rejects after a successful add landed in
// `actionError` — reading as "adding failed" when the add had already
// succeeded — and the list was left stale with no warning at all. Fixed by
// catching the action's own rejection separately and re-reading outside
// that try, sending a failed re-read to `loadError` instead. Same shape for
// `confirmRemove`, checked separately below.
test('a successful add whose re-read fails reports the read failure, not an action failure', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValueOnce(listing([]));
  listTree.mockRejectedValueOnce(new Error('The index is not open yet.'));
  open.mockResolvedValue('/synthetic/reports');
  addWatchedFolder.mockResolvedValue(7);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('No folder has been added yet.')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Add a folder' }));

  await waitFor(() => expect(screen.getByText('The list of folders could not be read.')).toBeTruthy());
  expect(screen.getByText('The index is not open yet.')).toBeTruthy();
  // Not attributed to the add: no action-error banner at all.
  expect(screen.queryByTestId('folders-action-error')).toBeNull();
});

test('a successful remove whose re-read fails reports the read failure, not an action failure', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValueOnce(
    listing([root({ rootId: 4, absolutePath: '/synthetic/stuck', files: [] })]),
  );
  listTree.mockRejectedValueOnce(new Error('The index is not open yet.'));
  removeWatchedFolder.mockResolvedValue(1);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/stuck')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Remove /synthetic/stuck' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm removing /synthetic/stuck' }));

  await waitFor(() => expect(screen.getByText('The list of folders could not be read.')).toBeTruthy());
  expect(screen.getByText('The index is not open yet.')).toBeTruthy();
  expect(screen.queryByTestId('folders-action-error')).toBeNull();
});

test('rows show the absolute path, not the launcher\'s relative view', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/deep/nested/path', name: 'path', files: [] })]));
  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/deep/nested/path')).toBeTruthy());
});

test('labels and the per-row count stay correct across a language switch after mount', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/only', files: [{ relativePath: 'a.md', documentId: 'd1' }] }),
  ]));
  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/only')).toBeTruthy());
  // Read once under 'en' BEFORE switching, so a $derived missing `void
  // $locale` still caches an English value here — the mutant only dies if the
  // read after the switch is a genuinely later one (Settings.test.ts:88-89's
  // own reasoning, reused here).
  expect(screen.getByText(t('settings_folders_indexed', { count: 1 }))).toBeTruthy();

  setLocale('uk');
  await Promise.resolve();

  expect(screen.getByRole('button', { name: 'Додати теку' })).toBeTruthy();
  // P2-5: the accessible name carries the path, so it switches with the
  // locale-prefix word ("Видалити") too, not just the visible label.
  const removeButton = screen.getByRole('button', { name: 'Видалити /synthetic/only' });
  expect(removeButton).toBeTruthy();
  // `aria-label` overrides the accessible name entirely, so the query above
  // would find this button even if its own VISIBLE text (`removeLabel`) had
  // gone stale — checked separately here, since that is a distinct `$derived`
  // with its own `void $locale` guard.
  expect(removeButton.textContent).toBe('Видалити');
  expect(screen.getByText(t('settings_folders_indexed', { count: 1 }))).toBeTruthy();
  expect(screen.getByText('Проіндексовано: 1 документ')).toBeTruthy();
});

test('the load-failure sentence stays correct across a language switch after mount', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockRejectedValue(new Error('boom'));
  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('The list of folders could not be read.')).toBeTruthy());

  setLocale('uk');
  await Promise.resolve();

  expect(screen.getByText('Не вдалося прочитати список тек.')).toBeTruthy();
});

test('a language switch reaches the empty-state sentence too', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([]));
  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('No folder has been added yet.')).toBeTruthy());

  setLocale('uk');
  await Promise.resolve();

  expect(screen.getByText('Ще жодної теки не додано.')).toBeTruthy();
});

// ── PR 8a, Task 5: the folder row expands ────────────────────────────────────

function sub(name: string, state: SubfolderState = { kind: 'open' }, parent = ''): Subfolder {
  return { name, relativePath: parent ? `${parent}/${name}` : name, state };
}
function subfolders(entries: Subfolder[], unnameable = 0): SubfolderListing {
  return { entries, unnameable };
}

// Every text node under `el`, in document order, joined by a single space —
// what a person reads, in the order they read it. NOT `textContent`: that
// concatenates two neighbouring rows into one word whenever the markup happens
// to leave no whitespace between them, so an assertion written against it is an
// assertion about indentation.
function visibleText(el: HTMLElement): string {
  const walker = el.ownerDocument.createTreeWalker(el, 4 /* SHOW_TEXT */);
  const parts: string[] = [];
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    const text = (node.textContent ?? '').replace(/\s+/g, ' ').trim();
    if (text !== '') parts.push(text);
  }
  return parts.join(' ');
}

// Two roots in every fixture below, not one. The expansion is held per root,
// and a component that kept one listing for the whole list would satisfy every
// assertion a single-root fixture can make while showing the second folder's
// subfolders under the first.
async function expand(
  entries: Subfolder[],
  rules: StoredExclusion[] = [],
  unnameable = 0,
  files: TreeRoot['files'] = [],
) {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root', files }),
    root({ rootId: 2, absolutePath: '/synthetic/other' }),
  ]));
  listSubfolders.mockResolvedValue(subfolders(entries, unnameable));
  listExclusions.mockResolvedValue(rules);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByTestId('folder-rules-1')).toBeTruthy());
}

test('the row expands by value, asks for the root level, and collapsing throws the listing away', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockResolvedValue(subfolders([sub('Work'), sub('Archive')]));
  listExclusions.mockResolvedValue([]);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());

  // By VALUE, both directions: a shut row states `false`, not nothing at all.
  // The attribute is in the F3 guard's MACHINE_ATTRS (`guard.test.ts:47`), so
  // nothing else in this repository polices it.
  expect(screen.getByTestId('folder-expand-1').getAttribute('aria-expanded')).toBe('false');
  expect(screen.queryByText('Work')).toBeNull();
  expect(listSubfolders).not.toHaveBeenCalled(); // a shut row costs no read_dir

  await fireEvent.click(screen.getByTestId('folder-expand-1'));

  await waitFor(() => expect(screen.getByText('Work')).toBeTruthy());
  expect(screen.getByTestId('folder-expand-1').getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByText('Archive')).toBeTruthy();
  expect(listSubfolders).toHaveBeenCalledWith(1, '');
  expect(listExclusions).toHaveBeenCalledWith(1);

  await fireEvent.click(screen.getByTestId('folder-expand-1'));

  expect(screen.getByTestId('folder-expand-1').getAttribute('aria-expanded')).toBe('false');
  expect(screen.queryByText('Work')).toBeNull();

  // Re-expanding reads the disk again rather than redrawing what was fetched
  // before: a cached listing is a claim about a moment that has passed, and
  // the folder can have changed while the row was shut.
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(listSubfolders).toHaveBeenCalledTimes(2));
  expect(listExclusions).toHaveBeenCalledTimes(2);
});

test('two rows expand independently, each showing its own root\'s subfolders', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root' }),
    root({ rootId: 2, absolutePath: '/synthetic/other' }),
  ]));
  listSubfolders.mockImplementation((rootId: number) =>
    Promise.resolve(subfolders([sub(rootId === 1 ? 'FirstOnly' : 'SecondOnly')])));
  listExclusions.mockResolvedValue([]);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/other')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('folder-expand-2'));
  await waitFor(() => expect(screen.getByText('SecondOnly')).toBeTruthy());

  // The row that was NOT pressed stays shut and shows nothing.
  expect(screen.getByTestId('folder-expand-1').getAttribute('aria-expanded')).toBe('false');
  expect(screen.queryByText('FirstOnly')).toBeNull();
  expect(listSubfolders).toHaveBeenCalledWith(2, '');
  expect(listSubfolders).toHaveBeenCalledTimes(1);
});

// The six states, one test each. `SubfolderState` has six variants
// (`src-tauri/src/tree.rs`), and the acceptance criterion Task 4 exists to
// establish is that no folder the walk will prune may be offered to a person as
// excludable — so four of the six carry no toggle at all, for two different
// reasons that must not read alike.
test('an open subfolder is the one state that offers to exclude it', async () => {
  await expand([sub('Work')]);

  const row = within(screen.getByTestId('subfolder-1-Work'));
  expect(row.getByText('No rule excludes this folder.')).toBeTruthy();
  expect(row.getByRole('button', { name: 'Exclude Work' })).toBeTruthy();
});

test('an excluded subfolder offers to include it and says what that costs first', async () => {
  await expand([sub('Archive', { kind: 'excluded' })], [{ prefix: 'Archive', existsOnDisk: true }]);

  const row = within(screen.getByTestId('subfolder-1-Archive'));
  expect(row.getByText('Excluded by your rule.')).toBeTruthy();
  // The disclosure is on screen BEFORE the press, not after it.
  expect(row.getByText('Without this rule, anything at this path is indexed again from the next scan on.')).toBeTruthy();
  expect(row.getByRole('button', { name: 'Do not exclude Archive' })).toBeTruthy();
  expect(row.queryByRole('button', { name: 'Exclude Archive' })).toBeNull();
});

// 🔴 Review finding I1. An excluded folder opens, and the reason is not
// symmetry: every path to an `excludedByAncestor` row runs through a folder
// that is itself `Excluded` (`subfolder_state` asks about an ancestor before
// asking about the folder itself, `tree.rs:829-838`), so while `excluded` was
// shut that state was tested and unreachable — and a person who had protected
// a folder could never look inside to see what they had protected.
test('an excluded folder opens, and what is inside it names the rule and offers nothing', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockImplementation((_rootId: number, path: string) =>
    Promise.resolve(path === ''
      ? subfolders([sub('Archive', { kind: 'excluded' })])
      : subfolders([sub('tax', { kind: 'excludedByAncestor', prefix: 'Archive' }, 'Archive')])));
  listExclusions.mockResolvedValue([{ prefix: 'Archive', existsOnDisk: true }]);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  // By test id, not by text: `Archive` is on screen twice — the subfolder row
  // and the stored rule that names it — and a text query would be ambiguous.
  await waitFor(() => expect(screen.getByTestId('subfolder-1-Archive')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('subfolder-expand-1-Archive'));

  await waitFor(() => expect(screen.getByText('tax')).toBeTruthy());
  expect(screen.getByTestId('subfolder-expand-1-Archive').getAttribute('aria-expanded')).toBe('true');
  const held = within(screen.getByTestId('subfolder-1-Archive/tax'));
  expect(held.getByText('Held by your rule on Archive. Remove that rule first — another rule may still hold this folder.')).toBeTruthy();
  // Nothing under a rule is toggleable and nothing under it opens further, so
  // opening the rule's own folder adds a level to read and no control to press.
  expect(held.queryAllByRole('button')).toHaveLength(0);
  expect(screen.queryByTestId('subfolder-expand-1-Archive/tax')).toBeNull();
});

// The pin in `ipc.test.ts` is what stops this state arriving; this is what the
// screen does if it arrives anyway. Before the fix the row drew a button with
// no text and no `aria-label` whose click called `include_subfolder` — an
// unlabelled control that removed the person's exclusion rule.
test('a state this build has never heard of offers no control, and the rule stays reachable', async () => {
  // The shell is not bound by this window's union, which is the whole finding:
  // a variant added to `tree.rs` and not mirrored here arrives all the same.
  const unknown = { kind: 'quarantined' } as unknown as SubfolderState;
  await expand([sub('Vault', unknown)], [{ prefix: 'Vault', existsOnDisk: true }]);

  const row = within(screen.getByTestId('subfolder-1-Vault'));
  expect(row.getByText('Vault')).toBeTruthy(); // still listed, never silently dropped
  expect(row.queryAllByRole('button')).toHaveLength(0);
  expect(includeSubfolder).not.toHaveBeenCalled();
  expect(excludeSubfolder).not.toHaveBeenCalled();
  // And the protection is still reachable, by the control that names it.
  expect(screen.getByRole('button', { name: 'Remove the rule on Vault' })).toBeTruthy();
});

test('an ancestor-held subfolder names the rule holding it and offers no control at all', async () => {
  await expand([sub('secret', { kind: 'excludedByAncestor', prefix: 'Work' }, 'Work')]);

  const row = within(screen.getByTestId('subfolder-1-Work/secret'));
  // The prefix the state CARRIES, not the row's own path: a row that says
  // "held by a rule" without naming it leaves nothing to go and remove.
  // "first", not "to change this folder": the state names the OUTERMOST
  // ancestor rule (`tree.rs:755-759`), so with rules on both `Archive` and
  // `Archive/sub` removing `Archive` does not free `Archive/sub/x`. The
  // sentence names the first step and promises no result.
  expect(row.getByText('Held by your rule on Work. Remove that rule first — another rule may still hold this folder.')).toBeTruthy();
  expect(row.queryAllByRole('button')).toHaveLength(0);
});

// The class is what the stylesheet dims an excluded row by. Both excluded
// states carry it — a person reads "not indexed" in both, and the sentence
// beside the row says which — and the open one does not. Held through the
// component, because the stylesheet guard only sees a class it wrote itself.
test('an excluded subfolder and one held from above are marked as excluded, an open one is not', async () => {
  await expand(
    [
      sub('notes', { kind: 'open' }),
      sub('Archive', { kind: 'excluded' }),
      sub('secret', { kind: 'excludedByAncestor', prefix: 'Work' }, 'Work'),
    ],
    [{ prefix: 'Archive', existsOnDisk: true }, { prefix: 'Work', existsOnDisk: true }],
  );
  const marked = (id: string) => screen.getByTestId(id).classList.contains('excl');
  expect(marked('subfolder-1-notes')).toBe(false);
  expect(marked('subfolder-1-Archive')).toBe(true);
  expect(marked('subfolder-1-Work/secret')).toBe(true);
  expect(screen.getByTestId('subfolder-1-Archive').classList.contains('sub')).toBe(true);
});

test('a built-in subfolder says the application made the rule, and offers no control', async () => {
  await expand([sub('node_modules', { kind: 'builtIn' })]);

  const row = within(screen.getByTestId('subfolder-1-node_modules'));
  expect(row.getByText('The application never indexes this folder, so there is no rule to add or remove.')).toBeTruthy();
  expect(row.queryAllByRole('button')).toHaveLength(0);
});

// 🔴 The acceptance criterion, at the place it is easiest to break. A child of
// a symlinked directory comes back `Open` from `list_subfolders`
// (`subfolder_state` asks `is_symlink` about the entry itself), so a row that
// could be expanded here would offer "Exclude" over a subtree the walk never
// enters — a rule that excludes nothing, on a folder nothing indexes.
test('a symlinked subfolder offers neither a toggle nor a way to open it', async () => {
  await expand([sub('Link', { kind: 'symlink' })]);

  const row = within(screen.getByTestId('subfolder-1-Link'));
  expect(row.getByText('A link to another folder. The scan never follows links, so nothing inside it is indexed.')).toBeTruthy();
  expect(row.queryAllByRole('button')).toHaveLength(0);
});

test('an unusable name says the opposite fact from a built-in folder, not the same one', async () => {
  await expand([sub('node_modules', { kind: 'builtIn' }), sub('Trailing ', { kind: 'unusableName' })]);

  // The trailing space is the defect this state is ABOUT, so the query keeps
  // it: testing-library trims an attribute value before comparing it, and the
  // default matcher would have found this row under the name it does not have.
  const row = within(screen.getByTestId('subfolder-1-Trailing ', { normalizer: (v) => v }));
  expect(row.getByText('This folder is indexed, and its name cannot be written as a rule here — rename it if you need to exclude it.')).toBeTruthy();
  expect(row.queryAllByRole('button')).toHaveLength(0);
  // Both are non-toggleable, and for opposite reasons: one folder's contents
  // never reach a provider, this one's do. Two sentences that read alike would
  // pass every assertion above.
  const builtIn = screen.getByTestId('subfolder-1-node_modules').textContent;
  expect(screen.getByTestId('subfolder-1-Trailing ', { normalizer: (v) => v }).textContent)
    .not.toBe(builtIn);
});

test('expanding a subfolder asks for that subfolder\'s own path', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockImplementation((_rootId: number, path: string) =>
    Promise.resolve(path === ''
      ? subfolders([sub('Work')])
      : subfolders([sub('notes', { kind: 'open' }, 'Work')])));
  listExclusions.mockResolvedValue([]);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByText('Work')).toBeTruthy());

  const nested = screen.getByTestId('subfolder-expand-1-Work');
  expect(nested.getAttribute('aria-expanded')).toBe('false');
  await fireEvent.click(nested);

  await waitFor(() => expect(screen.getByText('notes')).toBeTruthy());
  expect(screen.getByTestId('subfolder-expand-1-Work').getAttribute('aria-expanded')).toBe('true');
  expect(listSubfolders).toHaveBeenCalledWith(1, 'Work');
  expect(screen.getByTestId('subfolder-1-Work/notes')).toBeTruthy();
});

test('a non-zero unnameable count is stated, so the folder does not read as emptier than it is', async () => {
  await expand([sub('Work')], [], 2);

  expect(screen.getByText('2 subfolders are not listed: their names could not be read as text.')).toBeTruthy();
});

test('a zero unnameable count states nothing at all', async () => {
  await expand([sub('Work')], [], 0);

  expect(screen.queryByText(/are not listed/)).toBeNull();
  expect(screen.queryByText(/is not listed/)).toBeNull();
});

test('a folder with no subfolders says so rather than showing an empty space', async () => {
  await expand([]);

  expect(screen.getByText('This folder has no subfolders.')).toBeTruthy();
});

// 🔴 D-a, amended twice. A rule is "the folder is gone" when and only when its
// own `existsOnDisk` says so. Comparing the rule list against the one-level
// listing marks `Work/private` stale — the folder is real, one level down —
// and invites a person to delete a rule that is still doing its job.
test('a nested rule whose folder is present is not labelled gone; the one that is says so', async () => {
  await expand(
    [sub('Work')],
    [
      { prefix: 'Work/private', existsOnDisk: true },
      { prefix: 'Old notes', existsOnDisk: false },
    ],
  );

  const nested = within(screen.getByTestId('folder-rule-1-Work/private'));
  expect(nested.getByText('Work/private')).toBeTruthy();
  expect(nested.queryByText('There is no folder at this path right now.')).toBeNull();

  const gone = within(screen.getByTestId('folder-rule-1-Old notes'));
  expect(gone.getByText('There is no folder at this path right now.')).toBeTruthy();
  // Both carry the cost sentence: removing either is a disclosure, not a
  // tidy-up.
  expect(gone.getByText('Without this rule, anything at this path is indexed again from the next scan on.')).toBeTruthy();
  expect(gone.getByRole('button', { name: 'Remove the rule on Old notes' })).toBeTruthy();
});

// ── Fix round 1, I2 ─────────────────────────────────────────────────────────
//
// `settings_folders_rule_cost` was unconditional, and the state that
// contradicts it is storable from this very screen: `exclude_subfolder` has no
// ancestor guard (`bridge.rs:450-483`), `add_path_exclusion` is
// `ON CONFLICT DO NOTHING` (`write.rs:604-612`), and `subfolder_state` still
// reports the ancestor as `excluded` (`tree.rs:817-848`) so the control to
// remove it is offered. Exclude `Archive/Held`, then include `Archive` back,
// and "anything at this path is indexed again" was contradicted by the rule
// list two lines below it.
//
// It over-warns rather than under-warns, so no D29 direction was crossed —
// but a sentence that is false in a reachable state is the F5 class, and the
// fix is to make it true rather than to delete it.
const RULE_COST = {
  plain: 'Without this rule, anything at this path is indexed again from the next scan on.',
  held: 'Without this rule, anything at this path is indexed again from the next scan on —'
    + ' except what your other rules further down this path still exclude.',
  plainUk: 'Без цього правила все за цим шляхом знову індексуватиметься від наступного сканування.',
  heldUk: 'Без цього правила все за цим шляхом знову індексуватиметься від наступного сканування —'
    + ' окрім того, що й далі виключають ваші інші правила глибше за цим шляхом.',
};

// ── Fix round 2, A1: the mirror of the above ────────────────────────────────
//
// The relation `heldBelow` reads is symmetric, and round 1 read one side of it.
// A rule with one of the person's own rules ABOVE it releases NOTHING when it
// goes — the walk is already pruning at the ancestor — so both cost sentences
// were false about it, unconditionally, in a state reachable by the same two
// presses in the other order.
//
// 🔴 Not a new string. This is the sentence the TREE has always drawn one row
// above (`settings_subfolder_excluded_by_ancestor`), re-used at the two sites
// that decide what removing a rule costs, so the screen says ONE thing about
// one path. Written as functions because it names the ancestor, and the
// ancestor is the OUTERMOST one — `tree.rs:755-759`'s choice, which the rule
// list must match or the same screen names two different rules to remove.
const heldBy = (prefix: string) =>
  `Held by your rule on ${prefix}. Remove that rule first — another rule may still hold this folder.`;
const heldByUk = (prefix: string) =>
  `Утримується вашим правилом на ${prefix}. Спершу приберіть те правило — теку може утримувати ще одне.`;

// 🔴 The pair is the whole point of the fixture, and the fixture asserts BOTH
// members: the ancestor has a rule under it and the descendant has none, so a
// screen that printed the new sentence everywhere satisfies neither row.
test('a rule with another of your rules under it says what removing it does not release', async () => {
  await expand(
    [sub('Archive', { kind: 'excluded' }), sub('Work')],
    [
      { prefix: 'Archive', existsOnDisk: true },
      { prefix: 'Archive/Held', existsOnDisk: true },
    ],
  );

  expect(visibleText(screen.getByTestId('folder-rule-1-Archive')))
    .toBe(['Archive', RULE_COST.held, 'Remove the rule'].join(' '));
  // 🔴 Fix round 2, A1: the mirror row. `Archive` is a rule of the person's own
  // ABOVE this one, so removing this one releases nothing at all — the walk
  // prunes at `Archive` either way. The unconditional sentence was false here.
  expect(visibleText(screen.getByTestId('folder-rule-1-Archive/Held')))
    .toBe(['Archive/Held', heldBy('Archive'), 'Remove the rule'].join(' '));

  // The same rule read from the subfolder row that names it. It draws the
  // sentence at its own site (`buildLevel`), so a fix applied only to the rule
  // list would leave these two rows disagreeing about one path.
  expect(visibleText(screen.getByTestId('subfolder-1-Archive')))
    .toBe(['Archive', 'Excluded by your rule.', RULE_COST.held, 'Do not exclude', 'Subfolders'].join(' '));
  // And the row no rule names carries no disclosure at all — the sentence
  // belongs to the control that takes protection away, not to every row.
  expect(visibleText(screen.getByTestId('subfolder-1-Work')))
    .toBe(['Work', 'No rule excludes this folder.', 'Exclude', 'Subfolders'].join(' '));

  // D130: a new string is a string in both locales, and both arms of the new
  // condition are read in the second one too.
  setLocale('uk');
  await tick();

  expect(visibleText(screen.getByTestId('folder-rule-1-Archive')))
    .toBe(['Archive', RULE_COST.heldUk, 'Прибрати правило'].join(' '));
  expect(visibleText(screen.getByTestId('folder-rule-1-Archive/Held')))
    .toBe(['Archive/Held', heldByUk('Archive'), 'Прибрати правило'].join(' '));
});

// 🔴 The whole screen for the storable pair, read line by line, because the
// brief for this fix carried an inference worth settling by rendering rather
// than by reasoning: that `list_subfolders` reporting `Held` as
// `excludedByAncestor {prefix: "Archive"}` leaves the person unable to see the
// rule on `Archive/Held` at all.
//
// Half true, and the half that is false is the half that decides the sentence's
// shape. The TREE never names the descendant's own rule — the row for `Held`
// says "Held by your rule on Archive", which is the outermost, and that is
// `subfolder_state`'s deliberate choice. But the RULE LIST in the same panel is
// `list_exclusions`' whole answer for the root, so `Archive/Held` has a row of
// its own three lines below, with its own Remove control. The conditional
// sentence therefore points at something the person can actually go and find,
// which is what makes "except what your other rules further down this path
// still exclude" an instruction rather than a riddle.
test('the pair reads as one screen: the tree names only the outermost, the rule list names both', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root' }),
    root({ rootId: 2, absolutePath: '/synthetic/other' }),
  ]));
  listSubfolders.mockImplementation((_rootId: number, path: string) =>
    Promise.resolve(path === ''
      ? subfolders([sub('Archive', { kind: 'excluded' })])
      : subfolders([sub('Held', { kind: 'excludedByAncestor', prefix: 'Archive' }, 'Archive')])));
  listExclusions.mockResolvedValue([
    { prefix: 'Archive', existsOnDisk: true },
    { prefix: 'Archive/Held', existsOnDisk: true },
  ]);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await screen.findByTestId('subfolder-1-Archive');
  await fireEvent.click(screen.getByTestId('subfolder-expand-1-Archive'));
  await screen.findByTestId('subfolder-1-Archive/Held');

  expect(visibleText(screen.getByTestId('folder-row-1'))).toBe([
    '/synthetic/root',
    'Indexed: 0 documents',
    'Subfolders', 'Remove',
    // The tree. `Archive` carries the conditional sentence, because a rule of
    // the person's own remains under it.
    'Archive', 'Excluded by your rule.', RULE_COST.held, 'Do not exclude', 'Subfolders',
    // And its child names the OUTERMOST rule, never its own — so on this line
    // alone the rule on `Archive/Held` is invisible.
    'Held', heldBy('Archive'),
    // The rule list, which is where it is not invisible.
    'Your exclusion rules for this folder:',
    'Archive', RULE_COST.held, 'Remove the rule',
    // 🔴 Fix round 2, A1. This line is why the whole row is asserted as one
    // string: the rule list's row for `Archive/Held` used to carry the
    // unconditional sentence three lines under a tree row saying the folder is
    // held by `Archive`, so one screen said two things about one path. It now
    // says the SAME thing as the tree row, word for word and about the same
    // rule — which is the point of re-using the tree's own string here.
    'Archive/Held', heldBy('Archive'), 'Remove the rule',
  ].join(' '));
});

// 🔴 A sibling is not a rule under it. `Archive2` merely starts with `Archive`;
// `anchored_pattern` produces `!/Archive`, which does not match it, so removing
// the rule on `Archive` really does release everything at that path. Without
// this row, `under`'s separator could be dropped from `heldBelow` and the test
// above would stay green.
test('a rule whose prefix merely starts with this one does not soften the sentence', async () => {
  await expand(
    [sub('Archive', { kind: 'excluded' })],
    [
      { prefix: 'Archive', existsOnDisk: true },
      { prefix: 'Archive2', existsOnDisk: true },
    ],
  );

  expect(visibleText(screen.getByTestId('folder-rule-1-Archive')))
    .toBe(['Archive', RULE_COST.plain, 'Remove the rule'].join(' '));
});

// The question a person answers has to say what the list says. It is frozen
// into `Pending` beside `existsOnDisk`, for that field's own reason: a re-read
// landing underneath must not renumber — or re-word — a sentence somebody is
// part way through reading.
test('the include question names the rules further down that removing this one leaves standing', async () => {
  await expand(
    [sub('Archive', { kind: 'excluded' })],
    [
      { prefix: 'Archive', existsOnDisk: true },
      { prefix: 'Archive/Held', existsOnDisk: true },
    ],
  );

  await fireEvent.click(screen.getByRole('button', { name: 'Do not exclude Archive' }));

  expect(visibleText(await screen.findByTestId('folder-confirm-1'))).toBe([
    'Stop excluding Archive?',
    'From the next scan on, everything inside this folder is indexed again, and its text is',
    'sent to the model provider — except what your other rules further down this path still',
    'exclude.',
    'Confirm Cancel',
  ].join(' '));
  expect(includeSubfolder).not.toHaveBeenCalled();

  setLocale('uk');
  await tick();

  expect(visibleText(screen.getByTestId('folder-confirm-1'))).toBe([
    'Більше не виключати Archive?',
    'Від наступного сканування все всередині цієї теки індексується знову, а її текст',
    'надсилається провайдеру моделі — окрім того, що й далі виключають ваші інші правила',
    'глибше за цим шляхом.',
    'Підтвердити Скасувати',
  ].join(' '));
});

// 🔴 Fix round 2, A1 — the third site, and the one a press actually lands on.
// The rule list's Remove control opens this question, so a screen corrected in
// the list alone would still ask the person to confirm a false consequence: the
// question is the last thing read before the rule goes.
test('the include question about a rule held from above says removing it releases nothing', async () => {
  await expand(
    [sub('Archive', { kind: 'excluded' })],
    [
      { prefix: 'Archive', existsOnDisk: true },
      { prefix: 'Archive/Held', existsOnDisk: true },
    ],
  );

  await fireEvent.click(screen.getByRole('button', { name: 'Remove the rule on Archive/Held' }));

  expect(visibleText(await screen.findByTestId('folder-confirm-1'))).toBe([
    'Stop excluding Archive/Held?',
    heldBy('Archive'),
    'Confirm Cancel',
  ].join(' '));
  expect(includeSubfolder).not.toHaveBeenCalled();

  // D130, and the same reason the two rows above are read in both locales: the
  // arm this fix adds is a `t()` call like any other, and a hard-coded English
  // sentence would satisfy the assertion above and nothing else.
  setLocale('uk');
  await tick();

  expect(visibleText(screen.getByTestId('folder-confirm-1'))).toBe([
    'Більше не виключати Archive/Held?',
    heldByUk('Archive'),
    'Підтвердити Скасувати',
  ].join(' '));
});

// 🔴 The state where the two conditions in the question disagree, and it is the
// reason the ancestor is read BEFORE `existsOnDisk` rather than after it. With
// the folder gone AND a rule above, the `_gone` arm's own second sentence —
// "if a folder appears there later, it is indexed and its text is sent to the
// model provider" — is false while the ancestor rule stands. No other row in
// this file builds both facts at once, so the order of those two branches is
// otherwise free to be wrong.
test('a question about a gone rule held from above names the ancestor, not the empty path', async () => {
  await expand(
    [sub('Archive', { kind: 'excluded' })],
    [
      { prefix: 'Archive', existsOnDisk: true },
      { prefix: 'Archive/Held', existsOnDisk: false },
    ],
  );

  await fireEvent.click(screen.getByRole('button', { name: 'Remove the rule on Archive/Held' }));

  expect(visibleText(await screen.findByTestId('folder-confirm-1'))).toBe([
    'Stop excluding Archive/Held?',
    heldBy('Archive'),
    'Confirm Cancel',
  ].join(' '));
});

// 🔴 Two ancestors, and the row must name the SHALLOWEST — `tree.rs:755-759`'s
// own choice, which the tree row three lines above has always made. Naming
// `Archive/Held` here instead would send the person to remove a rule that
// changes nothing while `Archive` stands, and would put two different
// instructions about one path on one screen again. `list_exclusions` arrives
// sorted (`write.rs:580`) and the ancestors of one path form a chain, so the
// first match in that order is the outermost; without this row the search
// could take the last match and the fixture above, which has one ancestor,
// would stay green.
test('a rule with two of your rules above it names the outermost, as the tree does', async () => {
  await expand(
    [sub('Archive', { kind: 'excluded' })],
    [
      { prefix: 'Archive', existsOnDisk: true },
      { prefix: 'Archive/Held', existsOnDisk: true },
      { prefix: 'Archive/Held/Deep', existsOnDisk: true },
    ],
  );

  expect(visibleText(screen.getByTestId('folder-rule-1-Archive/Held/Deep')))
    .toBe(['Archive/Held/Deep', heldBy('Archive'), 'Remove the rule'].join(' '));
  // And the middle rule, which has one rule above it and one below: the fact
  // that releases nothing is the one above, so it wins over the softening.
  expect(visibleText(screen.getByTestId('folder-rule-1-Archive/Held')))
    .toBe(['Archive/Held', heldBy('Archive'), 'Remove the rule'].join(' '));
});

// 🔴 A sibling of an ANCESTOR is not an ancestor: `Arch` is a string prefix of
// `Archive/Held` and holds nothing of it, because `anchored_pattern` produces
// `!/Arch`, which does not match. Without this row the separator could be
// dropped from the ancestor search and every test above would stay green —
// and the screen would then tell a person to go and remove a rule that is not
// holding this folder at all, leaving the real consequence unsaid.
test('a rule whose prefix merely starts the same way is not an ancestor', async () => {
  await expand(
    [sub('Archive', { kind: 'excluded' })],
    [
      { prefix: 'Arch', existsOnDisk: true },
      { prefix: 'Archive/Held', existsOnDisk: true },
    ],
  );

  expect(visibleText(screen.getByTestId('folder-rule-1-Archive/Held')))
    .toBe(['Archive/Held', RULE_COST.plain, 'Remove the rule'].join(' '));
});

// 🔴 Fix round 3, item 1. No fixture anywhere else in this file gives an
// ANCESTOR rule `existsOnDisk: false` — every case above that holds a folder
// from above uses a live one, so `heldAbove` consulting `existsOnDisk` (it
// must not, per its own doc at `Folders.svelte:353-358`) has nothing to catch
// it on. `Archive` is gone from disk but its rule row has not been removed,
// and the doc's own claim is that removing `Archive/Held` still changes
// nothing: the ancestor's pattern prunes the subtree the moment a folder
// reappears there, whatever is on disk right now.
//
// Assert both directions in one fixture: `Archive/Held` stays firm under a
// gone ancestor (this is the direction the mutant breaks — filtering the
// ancestor by `existsOnDisk` drops it, and the row falls through to the
// unconditional `RULE_COST.plain`); `Other`, which has no ancestor at all and
// is held only by a LIVE rule below it, still softens to `RULE_COST.held` —
// showing the ancestor fix left the ordinary heldBelow path alone.
test('a rule held by a gone ancestor stays firm; a rule held below a live one still softens', async () => {
  await expand(
    [sub('Archive', { kind: 'excluded' })],
    [
      { prefix: 'Archive', existsOnDisk: false },
      { prefix: 'Archive/Held', existsOnDisk: true },
      { prefix: 'Other', existsOnDisk: true },
      { prefix: 'Other/Nested', existsOnDisk: true },
    ],
  );

  expect(visibleText(screen.getByTestId('folder-rule-1-Archive/Held')))
    .toBe(['Archive/Held', heldBy('Archive'), 'Remove the rule'].join(' '));
  expect(visibleText(screen.getByTestId('folder-rule-1-Other')))
    .toBe(['Other', RULE_COST.held, 'Remove the rule'].join(' '));
});

// ── Fix round 2, A2 ─────────────────────────────────────────────────────────
//
// The softening promises an exception, and round 1 let a rule the SAME PANEL
// labels "There is no folder at this path right now" back that promise. At the
// next scan such a rule excludes nothing, so the sentence understates what
// reaches the provider — the one direction D29 does not allow. The condition
// now consults the rule's own `existsOnDisk`, which drops the exception and
// selects the unconditional sentence: an overstatement, and exactly true here,
// because everything present at this path really is indexed again.
test('a rule below whose folder is gone does not soften the sentence', async () => {
  await expand(
    [sub('Archive', { kind: 'excluded' })],
    [
      { prefix: 'Archive', existsOnDisk: true },
      { prefix: 'Archive/Held', existsOnDisk: false },
    ],
  );

  expect(visibleText(screen.getByTestId('folder-rule-1-Archive')))
    .toBe(['Archive', RULE_COST.plain, 'Remove the rule'].join(' '));

});

// The other direction, and it is not the same assertion negated: a rule whose
// folder is gone must not veto a sibling rule that is doing its job. A
// condition written as "every rule below exists" satisfies the row above and
// fails here.
test('a rule below whose folder is gone does not cancel one whose folder is there', async () => {
  await expand(
    [sub('Archive', { kind: 'excluded' })],
    [
      { prefix: 'Archive', existsOnDisk: true },
      { prefix: 'Archive/Gone', existsOnDisk: false },
      { prefix: 'Archive/Here', existsOnDisk: true },
    ],
  );

  expect(visibleText(screen.getByTestId('folder-rule-1-Archive')))
    .toBe(['Archive', RULE_COST.held, 'Remove the rule'].join(' '));
});

// The question freezes the answer, so it has to freeze the corrected one.
test('the include question does not soften on the strength of a rule whose folder is gone', async () => {
  await expand(
    [sub('Archive', { kind: 'excluded' })],
    [
      { prefix: 'Archive', existsOnDisk: true },
      { prefix: 'Archive/Held', existsOnDisk: false },
    ],
  );

  await fireEvent.click(screen.getByRole('button', { name: 'Do not exclude Archive' }));

  expect(visibleText(await screen.findByTestId('folder-confirm-1'))).toBe([
    'Stop excluding Archive?',
    'From the next scan on, everything inside this folder is indexed again, and its text is',
    'sent to the model provider.',
    'Confirm Cancel',
  ].join(' '));
});

test('a folder with no rules says so instead of showing an empty heading', async () => {
  await expand([sub('Work')], []);

  expect(screen.getByText('You have not excluded anything in this folder.')).toBeTruthy();
});

test('excluding a subfolder sends its path and re-reads the listing from disk', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockResolvedValueOnce(subfolders([sub('Work')]));
  listExclusions.mockResolvedValueOnce([]);
  listSubfolders.mockResolvedValueOnce(subfolders([sub('Work', { kind: 'excluded' })]));
  listExclusions.mockResolvedValueOnce([{ prefix: 'Work', existsOnDisk: true }]);
  excludeSubfolder.mockResolvedValue(undefined);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude Work' })).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Exclude Work' }));

  expect(excludeSubfolder).toHaveBeenCalledWith(1, 'Work');
  // The state on screen comes from the re-read, never from a local patch: the
  // row now offers the opposite control, and the rule appears in the list.
  await waitFor(() => expect(screen.getByRole('button', { name: 'Do not exclude Work' })).toBeTruthy());
  expect(screen.getByTestId('folder-rule-1-Work')).toBeTruthy();
  expect(listSubfolders).toHaveBeenCalledTimes(2);
});

test('removing a rule sends its own prefix and re-reads', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockResolvedValue(subfolders([sub('Work')]));
  listExclusions.mockResolvedValueOnce([
    { prefix: 'Work/private', existsOnDisk: true },
    { prefix: 'Old notes', existsOnDisk: false },
  ]);
  listExclusions.mockResolvedValueOnce([{ prefix: 'Work/private', existsOnDisk: true }]);
  includeSubfolder.mockResolvedValue(true);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByTestId('folder-rule-1-Old notes')).toBeTruthy());

  // The SECOND rule is removed, not the first: a positional implementation
  // would send `Work/private` here.
  await fireEvent.click(screen.getByRole('button', { name: 'Remove the rule on Old notes' }));
  // Task 6 put a question in between: the press asks, the confirmation stores.
  // The prefix this test is about is carried by the QUESTION, so a component
  // that answered with the row under the cursor would still send the wrong one.
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm not excluding Old notes' }));

  expect(includeSubfolder).toHaveBeenCalledWith(1, 'Old notes');
  await waitFor(() => expect(screen.queryByTestId('folder-rule-1-Old notes')).toBeNull());
  expect(screen.getByTestId('folder-rule-1-Work/private')).toBeTruthy(); // untouched
});

// `include_subfolder` answers whether a row went (`bridge.rs:465-471`). `false`
// is not a failure: it is the screen having been out of date, and saying so is
// the difference between a control that worked and one that did nothing.
test('a rule that was already gone says so rather than reporting success in silence', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockResolvedValue(subfolders([sub('Work')]));
  listExclusions.mockResolvedValueOnce([{ prefix: 'Old notes', existsOnDisk: false }]);
  listExclusions.mockResolvedValueOnce([]);
  includeSubfolder.mockResolvedValue(false);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByTestId('folder-rule-1-Old notes')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Remove the rule on Old notes' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm not excluding Old notes' }));

  await waitFor(() => expect(screen.getByText('There was no such rule left to remove. The list has been re-read.')).toBeTruthy());
});

test('a rule removal that answers true says nothing about a missing rule', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockResolvedValue(subfolders([sub('Work')]));
  listExclusions.mockResolvedValueOnce([{ prefix: 'Old notes', existsOnDisk: false }]);
  listExclusions.mockResolvedValueOnce([]);
  includeSubfolder.mockResolvedValue(true);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByTestId('folder-rule-1-Old notes')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Remove the rule on Old notes' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm not excluding Old notes' }));

  await waitFor(() => expect(screen.queryByTestId('folder-rule-1-Old notes')).toBeNull());
  expect(screen.queryByText('There was no such rule left to remove. The list has been re-read.')).toBeNull();
});

test('a rejected list_subfolders shows the backend sentence, apart from the load and action errors', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockRejectedValue(new Error('That folder is not there any more.'));
  listExclusions.mockResolvedValue([]);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('folder-expand-1'));

  await waitFor(() => expect(screen.getByText('The subfolders of this folder could not be read.')).toBeTruthy());
  expect(screen.getByText('That folder is not there any more.')).toBeTruthy();
  // Held apart from both banners this component already had: the list of
  // folders is still readable, and no add or remove failed.
  expect(screen.queryByTestId('folders-load-reason')).toBeNull();
  expect(screen.queryByTestId('folders-action-error')).toBeNull();
  expect(screen.getByText('/synthetic/root')).toBeTruthy(); // the row itself stays
});

// `exclude_subfolder` can refuse a path the listing showed
// (`Error::AlreadyPrunedByBuiltIn`, added by Task 4 so the command agrees with
// the listing). The sentence is rendered verbatim and nothing branches on which
// rejection it was; the decision comes from a re-read.
test('a rejected exclude shows the backend sentence and re-reads the state', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockResolvedValueOnce(subfolders([sub('.git')]));
  listExclusions.mockResolvedValue([]);
  listSubfolders.mockResolvedValueOnce(subfolders([sub('.git', { kind: 'builtIn' })]));
  excludeSubfolder.mockRejectedValue(
    new Error('The application already skips ".git" in /synthetic/root, so a rule for it would change nothing.'),
  );

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude .git' })).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Exclude .git' }));

  await waitFor(() => expect(screen.getByText(
    'The application already skips ".git" in /synthetic/root, so a rule for it would change nothing.',
  )).toBeTruthy());
  // The re-read is what decides what the row says next — not the rejection,
  // which nothing here parses.
  await waitFor(() => expect(screen.queryByRole('button', { name: 'Exclude .git' })).toBeNull());
  expect(screen.getByText('The application never indexes this folder, so there is no rule to add or remove.')).toBeTruthy();
});

test('a root that disappears from the list takes its expansion with it', async () => {
  setLocale('en'); // seed, do not inherit
  const alpha = root({ rootId: 3, absolutePath: '/synthetic/alpha' });
  const beta = root({ rootId: 9, absolutePath: '/synthetic/beta' });
  listTree.mockResolvedValueOnce(listing([alpha, beta]));
  listTree.mockResolvedValueOnce(listing([alpha]));
  listSubfolders.mockResolvedValue(subfolders([sub('OnlyUnderBeta')]));
  listExclusions.mockResolvedValue([]);
  removeWatchedFolder.mockResolvedValue(1);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/beta')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-9'));
  await waitFor(() => expect(screen.getByText('OnlyUnderBeta')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Remove /synthetic/beta' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm removing /synthetic/beta' }));

  await waitFor(() => expect(screen.queryByText('/synthetic/beta')).toBeNull());
  // A watched root's id can be handed out again by SQLite after the row is
  // deleted, and an expansion left behind under that id would draw the old
  // folder's subfolders under a new one.
  expect(screen.queryByText('OnlyUnderBeta')).toBeNull();
  expect(screen.getByTestId('folder-expand-3').getAttribute('aria-expanded')).toBe('false');
});

// ── PR 8a, fix round 4, item 1: the other half of the case above ─────────────
//
// The comment inside the test above names the hazard and the test checks ONE of
// its two cases: the id has gone. This is the case rowid reuse actually
// produces — the id is still in the listing and now names a DIFFERENT folder.
// `watched_root.id` is `INTEGER PRIMARY KEY` with no `AUTOINCREMENT`
// (`schema.sql:11-15`), so removing the most recently added root and adding
// another hands the new one the old id.
//
// The pair is written as a pair on purpose, and neither half is sufficient:
// dropping every panel on every refresh satisfies the first of these two and
// fails the second, and keeping every panel whose id is present satisfies the
// second and fails the first. The one-line mutants, named in advance and both
// measured: `live.get(rootId) === panel.rootPath` → `live.has(rootId)` fails
// the first and leaves the second green; → `false` fails the second and leaves
// the first green.
test('a root id handed to a different folder does not keep the old folder\'s expansion', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValueOnce(listing([root({ rootId: 9, absolutePath: '/synthetic/beta' })]));
  // The same id, a different path — what the person's remove-then-add did to
  // this window between its two reads.
  listTree.mockResolvedValueOnce(listing([root({ rootId: 9, absolutePath: '/synthetic/gamma' })]));
  listSubfolders.mockResolvedValue(subfolders([sub('OnlyUnderBeta')]));
  listExclusions.mockResolvedValue([]);
  open.mockResolvedValue('/synthetic/gamma');
  addWatchedFolder.mockResolvedValue(undefined);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/beta')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-9'));
  await waitFor(() => expect(screen.getByText('OnlyUnderBeta')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Add a folder' }));
  await waitFor(() => expect(screen.getByText('/synthetic/gamma')).toBeTruthy());

  // The expansion belonged to `/synthetic/beta`, and drawing it here would put
  // `OnlyUnderBeta` under `/synthetic/gamma` — with controls that then exclude
  // or include on the NEW root using the OLD root's relative path.
  expect(screen.queryByText('OnlyUnderBeta')).toBeNull();
  expect(screen.queryByTestId('folder-panel-9')).toBeNull();
  expect(screen.getByTestId('folder-expand-9').getAttribute('aria-expanded')).toBe('false');
});

test('a refresh that finds the same folder under the same id keeps its expansion open', async () => {
  setLocale('en'); // seed, do not inherit
  const beta = root({ rootId: 9, absolutePath: '/synthetic/beta' });
  listTree.mockResolvedValueOnce(listing([beta]));
  // The same root, unchanged, beside a newly added one: nothing about this
  // panel's identity has moved, so nothing about the panel may.
  listTree.mockResolvedValueOnce(
    listing([beta, root({ rootId: 10, absolutePath: '/synthetic/added' })]),
  );
  listSubfolders.mockResolvedValue(subfolders([sub('OnlyUnderBeta')]));
  listExclusions.mockResolvedValue([]);
  open.mockResolvedValue('/synthetic/added');
  addWatchedFolder.mockResolvedValue(undefined);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/beta')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-9'));
  await waitFor(() => expect(screen.getByText('OnlyUnderBeta')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Add a folder' }));
  await waitFor(() => expect(screen.getByText('/synthetic/added')).toBeTruthy());

  // Still open, and still holding the listing it was opened with: an add does
  // not re-read a panel (only a job ending does), so this text is the panel
  // the refresh kept and not one it fetched again.
  expect(screen.getByText('OnlyUnderBeta')).toBeTruthy();
  expect(screen.getByTestId('folder-panel-9')).toBeTruthy();
  expect(screen.getByTestId('folder-expand-9').getAttribute('aria-expanded')).toBe('true');
});

// ── PR 8a, fix round 4, item 2: two `list_tree` calls, answered out of order ──
//
// `refresh` is reachable from four places — mount, an add, a remove, and a job
// ending — and had no generation guard of its own, while a panel's read has had
// one since Task 5. Two of them overlap here by construction rather than by
// luck: the mount's read is left on the wire and the add's is resolved first.
//
// The mutant, named in advance: delete `if (refreshes !== generation) return;`
// (the one after the `await`, not the one in the `catch`) and the older listing
// replaces the newer one — `/synthetic/stale` appears and `/synthetic/fresh`
// goes. Both directions are asserted for the reason the brief gives: a guard
// that returns unconditionally also removes the stale row, and would pass a
// test that only looked for its absence.
test('an older list_tree that lands after a newer one replaces neither the list nor the panels', async () => {
  setLocale('en'); // seed, do not inherit
  let settleFirst: (l: TreeListing) => void = () => {};
  listTree
    .mockReturnValueOnce(new Promise<TreeListing>((r) => { settleFirst = r; }))
    .mockResolvedValueOnce(listing([root({ rootId: 2, absolutePath: '/synthetic/fresh' })]));
  open.mockResolvedValue('/synthetic/fresh');
  addWatchedFolder.mockResolvedValue(undefined);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(listTree).toHaveBeenCalledTimes(1)); // mount's read, still on the wire
  await fireEvent.click(screen.getByRole('button', { name: 'Add a folder' }));
  await waitFor(() => expect(screen.getByText('/synthetic/fresh')).toBeTruthy());
  expect(listTree).toHaveBeenCalledTimes(2); // the two really did overlap

  settleFirst(listing([root({ rootId: 1, absolutePath: '/synthetic/stale' })]));
  // A real turn of the event loop, not a microtask: the older answer has an
  // `await` and a Svelte render to get through, and a test that stopped before
  // both would report "not drawn" about a draw not yet attempted.
  await new Promise((r) => setTimeout(r, 0));

  expect(screen.queryByText('/synthetic/stale')).toBeNull();
  expect(screen.getByText('/synthetic/fresh')).toBeTruthy(); // and the newer answer stands
});

// The rejection half of the same guard, and its own line: every caller of
// `refresh` turns a rejection into `loadError`, which replaces the whole list
// with a failure banner. An older failure landing behind a newer success would
// therefore say the list cannot be read over a list that has just been read.
//
// The mutant: delete `if (refreshes !== generation) return;` from the `catch`
// so the stale rejection is rethrown, and the banner appears.
test('a list_tree rejection that lands after a newer answer prints no failure over it', async () => {
  setLocale('en'); // seed, do not inherit
  let failFirst: (e: Error) => void = () => {};
  listTree
    .mockReturnValueOnce(new Promise<TreeListing>((_resolve, reject) => { failFirst = reject; }))
    .mockResolvedValueOnce(listing([root({ rootId: 2, absolutePath: '/synthetic/fresh' })]));
  open.mockResolvedValue('/synthetic/fresh');
  addWatchedFolder.mockResolvedValue(undefined);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(listTree).toHaveBeenCalledTimes(1));
  await fireEvent.click(screen.getByRole('button', { name: 'Add a folder' }));
  await waitFor(() => expect(screen.getByText('/synthetic/fresh')).toBeTruthy());

  failFirst(new Error('The index is not open yet.'));
  await new Promise((r) => setTimeout(r, 0));

  expect(screen.queryByTestId('folders-load-reason')).toBeNull();
  expect(screen.getByText('/synthetic/fresh')).toBeTruthy();
});

// 🔴 Review finding I2. This case is `patch`'s early return
// (`Folders.svelte:126`) and nothing else, and the shape of it was measured
// three times before it held.
//
// The write it pins is the one `exclude` makes AFTER the person has shut the
// row: `await read(rootId, want)` on line `Folders.svelte:413` runs behind the
// action, raises the generation itself, and so passes `read`'s own check
// (`:167`) — the counter cannot stand in for the early return here, because
// the counter is not what says no. With the early return dropped, `patch`
// spreads a missing panel and BUILDS one out of the re-read's own fields, and
// the row the person closed comes back open with the listing from before the
// action in it.
//
// 🔴 The exclude here RESOLVES, and that is the whole reason this case can
// fail. The first two attempts used a rejected exclude, and a rejection cannot
// be the oracle: `exclude`'s `catch` patches `{ actionError }` alone, so the
// panel the mutant builds has no `tree` — `undefined`, not `null` — and
// `buildLevel` throws inside the `rows` derived before anything reaches the
// DOM. Vitest reports that as an unhandled error beside 44 passing tests, and
// Svelte draws nothing further, so every assertion about the screen still
// passes. A resolved exclude patches `{ tree, rules, loadError }`, which is a
// panel that renders, and the re-opened row is then visible to an assertion.
//
// ⚠️ Read the whole summary, not the pass count: that run prints
// `Tests 44 passed (44)` **and** `Errors 1 error`, and it **exits 1**. CI runs
// `npm --prefix ui run test` (`.github/workflows/ci.yml:189`) and would have
// failed on it, so a crashed oracle is not a false green in CI. What it defeats
// is the INSTRUMENT: `ui/` has no mutation harness, so a guard here is checked
// by deleting it and reading the output — and three readers in a row took
// `44 passed, 0 failed` for a passing run and concluded this guard had no
// mutant. **Record a hand revert by its exit code and `Errors` count, never by
// its pass count.**
//
// The `listSubfolders` count is the positive control: without it this case
// would also be green if the re-read never happened at all, which is a
// different component from the one being tested.
test('a row shut while an exclude is in flight is not re-opened by the re-read behind it', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockResolvedValue(subfolders([sub('Work')]));
  listExclusions.mockResolvedValue([]);
  let accept: () => void = () => {};
  excludeSubfolder.mockReturnValueOnce(new Promise<void>((resolve) => { accept = () => resolve(); }));

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByText('Work')).toBeTruthy());
  expect(listSubfolders).toHaveBeenCalledTimes(1);

  await fireEvent.click(screen.getByRole('button', { name: 'Exclude Work' })); // in flight
  await fireEvent.click(screen.getByTestId('folder-expand-1')); // the person shuts the row
  expect(screen.queryByTestId('folder-panel-1')).toBeNull(); // and it is shut

  accept();
  // Real turns of the event loop, not microtasks: the re-read behind the
  // action is an awaited `Promise.all` and a Svelte render, and a chain of
  // `await Promise.resolve()` stops short of both — measured, and it is what
  // made the first attempt at this case unfalsifiable.
  await new Promise((r) => setTimeout(r, 0));
  await new Promise((r) => setTimeout(r, 0));

  // The re-read DID run and DID try to write: the guard is what refused it,
  // not an absent call.
  expect(listSubfolders).toHaveBeenCalledTimes(2);
  expect(screen.getByTestId('folder-expand-1').getAttribute('aria-expanded')).toBe('false');
  expect(screen.queryByTestId('folder-panel-1')).toBeNull();
  expect(screen.queryByText('Work')).toBeNull();
});

// Renamed after review I2: this case cannot claim a late listing was DISCARDED,
// because two independent guards each satisfy it alone — the generation counter
// and `patch`'s early return — so neither has a mutant here. What it does check
// is stated in its name and both halves are real: collapsing issues no read,
// and a listing still on the wire puts nothing on screen. Each guard's own
// case is elsewhere, and each of those two now fails alone when its own guard
// is dropped and stays green when the other's is: the counter's is `an older
// listing that lands after a newer one`, and the early return's is `a row shut
// while an exclude is in flight is not re-opened by the re-read behind it`.
test('collapsing a row reads nothing, and a listing still on the wire draws nothing', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  let settle: (l: SubfolderListing) => void = () => {};
  listSubfolders.mockReturnValueOnce(new Promise<SubfolderListing>((r) => { settle = r; }));
  listExclusions.mockResolvedValue([]);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('folder-expand-1')); // opens, request in flight
  await fireEvent.click(screen.getByTestId('folder-expand-1')); // shut again before it lands

  settle(subfolders([sub('TooLate')]));
  expect(listSubfolders).toHaveBeenCalledTimes(1); // shutting the row reads nothing
  await new Promise((r) => setTimeout(r, 0)); // a real turn, so the draw is attempted

  expect(screen.queryByText('TooLate')).toBeNull();
  expect(screen.getByTestId('folder-expand-1').getAttribute('aria-expanded')).toBe('false');
});

// 🔴 The collapse case above is held by TWO neighbouring defences and not by
// the generation counter: a shut row has no panel entry left to write into, so
// dropping the counter entirely leaves that test green, and so does dropping
// `patch`'s early return (both measured). The case the counter is actually for
// is this one — a row shut and opened again while the first read is still on
// the wire, where the panel entry EXISTS when the older answer lands and
// nothing else would stop it being drawn over the newer one.
test('an older listing that lands after a newer one is discarded, not drawn over it', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  let settleFirst: (l: SubfolderListing) => void = () => {};
  listSubfolders
    .mockReturnValueOnce(new Promise<SubfolderListing>((r) => { settleFirst = r; }))
    .mockResolvedValueOnce(subfolders([sub('Fresh')]));
  listExclusions.mockResolvedValue([]);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('folder-expand-1')); // read A, in flight
  await fireEvent.click(screen.getByTestId('folder-expand-1')); // shut
  await fireEvent.click(screen.getByTestId('folder-expand-1')); // read B, a new panel
  await waitFor(() => expect(screen.getByText('Fresh')).toBeTruthy());

  settleFirst(subfolders([sub('Stale')]));
  // A real turn of the event loop, not three microtasks: the older read has an
  // awaited `Promise.all` and a Svelte render to get through, and a test that
  // stops before either would report "not drawn" about a draw that had not been
  // attempted yet.
  await new Promise((r) => setTimeout(r, 0));

  expect(screen.queryByText('Stale')).toBeNull();
  expect(screen.getByText('Fresh')).toBeTruthy(); // and the newer answer is still there
  expect(screen.getByTestId('folder-expand-1').getAttribute('aria-expanded')).toBe('true');
});

// What disappears when a rule appears. `Work` itself stays open — a rule's own
// folder is readable (I1) — but the level under `Work/notes` does not: the
// re-read gives `notes` the ancestor state, which offers no way to open it, and
// a subtree left hanging under it would be a list of folders a person can no
// longer collapse, under a row whose contents are no longer read at all.
//
// Two mock levels below the root on purpose: with one, `notes` would be the
// deepest thing on screen and its own children would never have existed, so
// nothing would be left to disappear. Measured with two: dropping
// `describe(...).expandable` from `fetchTree` fails this case and no other.
test('excluding an open folder keeps that folder open and takes the level under its new ancestor rule away', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  let excluded = false;
  listSubfolders.mockImplementation((_rootId: number, path: string) => {
    if (path === '') {
      return Promise.resolve(subfolders([sub('Work', excluded ? { kind: 'excluded' } : { kind: 'open' })]));
    }
    if (path === 'Work') {
      return Promise.resolve(subfolders([
        sub('notes', excluded ? { kind: 'excludedByAncestor', prefix: 'Work' } : { kind: 'open' }, 'Work'),
      ]));
    }
    return Promise.resolve(subfolders([sub('drafts', { kind: 'open' }, 'Work/notes')]));
  });
  listExclusions.mockImplementation(() =>
    Promise.resolve(excluded ? [{ prefix: 'Work', existsOnDisk: true }] : []));
  excludeSubfolder.mockImplementation(() => { excluded = true; return Promise.resolve(undefined); });

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByText('Work')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('subfolder-expand-1-Work'));
  await waitFor(() => expect(screen.getByText('notes')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('subfolder-expand-1-Work/notes'));
  await waitFor(() => expect(screen.getByText('drafts')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Exclude Work' }));

  await waitFor(() => expect(screen.getByRole('button', { name: 'Do not exclude Work' })).toBeTruthy());
  // The folder the person just protected is still open, and still openable.
  expect(screen.getByText('notes')).toBeTruthy();
  expect(screen.getByTestId('subfolder-expand-1-Work').getAttribute('aria-expanded')).toBe('true');
  // The level under the new ancestor rule is gone, and so is the control that
  // opened it.
  expect(screen.queryByText('drafts')).toBeNull();
  expect(screen.queryByTestId('subfolder-expand-1-Work/notes')).toBeNull();
});

test('the expanded panel switches language with everything else on screen', async () => {
  await expand(
    [
      sub('Work'),
      sub('Archive', { kind: 'excluded' }),
      sub('secret', { kind: 'excludedByAncestor', prefix: 'Archive' }, 'Archive'),
      sub('node_modules', { kind: 'builtIn' }),
    ],
    [{ prefix: 'Old notes', existsOnDisk: false }],
    1,
  );
  // Read under 'en' BEFORE the switch, so a $derived missing `void $locale`
  // still caches an English value here and the read after the switch is a
  // genuinely later one.
  expect(screen.getByText('No rule excludes this folder.')).toBeTruthy();
  // `expandLabel` and `removeRuleLabel` are each their own `$derived.by`,
  // outside `rows` — read their visible button text (not the aria-labels
  // `rows` already covers below) before the switch too.
  expect(screen.getAllByText('Subfolders').length).toBeGreaterThan(0);
  expect(screen.getByText('Remove the rule')).toBeTruthy();

  setLocale('uk');
  await Promise.resolve();

  expect(screen.getByText('Жодне правило не виключає цю теку.')).toBeTruthy();
  expect(screen.getByText('Застосунок ніколи не індексує цю теку, тож тут немає правила, яке можна додати чи прибрати.')).toBeTruthy();
  expect(screen.getByText('Наразі за цим шляхом теки немає.')).toBeTruthy();
  expect(screen.getByText('1 підтеку не показано: її назву не вдалося прочитати як текст.')).toBeTruthy();
  // The sentence that names the outermost rule, in the language it is read in:
  // "спершу", not a promise that removing that rule changes this folder.
  expect(screen.getByText('Утримується вашим правилом на Archive. Спершу приберіть те правило — теку може утримувати ще одне.')).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Виключити Work' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Не виключати Archive' })).toBeTruthy();
  // An excluded folder opens here too — the control exists in both locales, and
  // the row held by the rule below it still offers none.
  expect(screen.getByRole('button', { name: 'Підтеки теки Archive' })).toBeTruthy();
  expect(within(screen.getByTestId('subfolder-1-Archive/secret')).queryAllByRole('button')).toHaveLength(0);
  expect(screen.getByRole('button', { name: 'Прибрати правило на Old notes' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Підтеки теки /synthetic/root' })).toBeTruthy();
  // The visible button text, not just its aria-label — `expandLabel` and
  // `removeRuleLabel` each guard their own `void $locale` at
  // Folders.svelte:855-856, outside `rows`.
  expect(screen.getAllByText('Підтеки').length).toBeGreaterThan(0);
  expect(screen.getByText('Прибрати правило')).toBeTruthy();
});

// 🔴 Read the screen, not the DOM. Every assertion above is satisfied by a
// panel that renders the right elements in the wrong words, or the right words
// against the wrong row. This one reads the whole row — its count line, its
// controls, every subfolder sentence and every stored rule — as a person would,
// in order, and states the entire text.
test('the whole expanded row reads as one screen, in order, with every sentence it claims', async () => {
  await expand(
    [
      sub('Archive', { kind: 'excluded' }),
      sub('Held', { kind: 'excludedByAncestor', prefix: 'Archive' }),
      sub('Link', { kind: 'symlink' }),
      sub('Trailing ', { kind: 'unusableName' }),
      sub('Work'),
      sub('node_modules', { kind: 'builtIn' }),
    ],
    [
      { prefix: 'Archive', existsOnDisk: true },
      { prefix: 'Work/private', existsOnDisk: true },
      { prefix: 'Old notes', existsOnDisk: false },
    ],
    1,
    [{ relativePath: 'a.md', documentId: 'd1' }, { relativePath: 'b.md', documentId: 'd2' }],
  );

  const cost = 'Without this rule, anything at this path is indexed again from the next scan on.';
  const text = visibleText(screen.getByTestId('folder-row-1'));

  expect(text).toBe([
    '/synthetic/root',
    'Indexed: 2 documents',
    'Subfolders', 'Remove',
    '1 subfolder is not listed: its name could not be read as text.',
    'Archive', 'Excluded by your rule.', cost, 'Do not exclude', 'Subfolders',
    'Held', 'Held by your rule on Archive. Remove that rule first — another rule may still hold this folder.',
    'Link', 'A link to another folder. The scan never follows links, so nothing inside it is indexed.',
    'Trailing', 'This folder is indexed, and its name cannot be written as a rule here — rename it if you need to exclude it.',
    'Work', 'No rule excludes this folder.', 'Exclude', 'Subfolders',
    'node_modules', 'The application never indexes this folder, so there is no rule to add or remove.',
    'Your exclusion rules for this folder:',
    'Archive', cost, 'Remove the rule',
    'Work/private', cost, 'Remove the rule',
    'Old notes', 'There is no folder at this path right now.', cost, 'Remove the rule',
  ].join(' '));
});

// ---------------------------------------------------------------------------
// PR 8a, Task 6 — what an exclusion costs, said before it is stored.
//
// 🔴 Every fixture below states BOTH numbers in the sentence it asserts. They
// are two different facts about the same reply — indexed PATHS under the
// prefix, and DOCUMENTS for which no path outside it survives — and an
// implementation that counts paths and calls them documents satisfies any
// assertion that reads only one of them. `deleting_one_copy_keeps_the_document`
// (`crates/mnema-ingest/tests/walk.rs:1168`) is the behaviour they mirror:
// `forget_if_unnamed` drops a document when its LAST path goes, never before.
// ---------------------------------------------------------------------------

function file(relativePath: string, documentId: string): TreeFile {
  return { relativePath, documentId };
}

// `before` is what `list_tree` answered at mount; `after` is what it answers on
// the re-read the click makes. They are DIFFERENT on purpose in every count
// test — `before` carries no files at all, so a component that counted from the
// mount snapshot would store without ceremony and every count assertion here
// would fail on a missing element rather than on a wrong number.
async function askExclude(name: string, before: TreeRoot[], after: TreeRoot[] | Error) {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValueOnce(listing(before));
  if (after instanceof Error) listTree.mockRejectedValueOnce(after);
  else listTree.mockResolvedValueOnce(listing(after));
  listSubfolders.mockResolvedValue(subfolders([sub(name)]));
  listExclusions.mockResolvedValue([]);
  excludeSubfolder.mockResolvedValue(undefined);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: `Exclude ${name}` })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: `Exclude ${name}` }));
}

const EMPTY_ROOTS = [
  root({ rootId: 1, absolutePath: '/synthetic/root', files: [] }),
  root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
];

test('excluding a folder that holds two documents names two paths AND two documents', async () => {
  await askExclude('drop', EMPTY_ROOTS, [
    root({
      rootId: 1,
      absolutePath: '/synthetic/root',
      files: [file('drop/x.md', 'doc-x'), file('drop/y.md', 'doc-y')],
    }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]);

  expect(visibleText(await screen.findByTestId('folder-confirm-1'))).toBe([
    'Exclude drop?',
    'As of now: on the next scan the index loses 2 files from this folder,',
    'and 2 documents stop being findable: no other path names them.',
    'The scan can remove more than that: files that never finished indexing are not counted here.',
    'Confirm Cancel',
  ].join(' '));
  // The question is a question: nothing is stored while it is on screen.
  expect(excludeSubfolder).not.toHaveBeenCalled();
});

// 🔴 The state that tells the two numbers apart. One `documentId`, two paths,
// one of them outside the prefix: the index loses a path and loses no document.
test('a second copy inside the same root keeps the document, so the count is one path and zero documents', async () => {
  await askExclude('drop', EMPTY_ROOTS, [
    root({
      rootId: 1,
      absolutePath: '/synthetic/root',
      files: [file('drop/x.md', 'doc-1'), file('keep/copy.md', 'doc-1')],
    }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]);

  expect(visibleText(await screen.findByTestId('folder-confirm-1'))).toBe([
    'Exclude drop?',
    'As of now: on the next scan the index loses 1 file from this folder,',
    'and no document stops being findable — each is also indexed under another path.',
    'The scan can remove more than that: files that never finished indexing are not counted here.',
    'Confirm Cancel',
  ].join(' '));
});

// 🔴 The same fact across a root boundary. A count taken per root sees only
// `/synthetic/root`, finds the document's last path there, and overstates.
test('a second copy under a DIFFERENT watched folder keeps the document too', async () => {
  await askExclude('drop', EMPTY_ROOTS, [
    root({ rootId: 1, absolutePath: '/synthetic/root', files: [file('drop/x.md', 'doc-1')] }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [file('other/x.md', 'doc-1')] }),
  ]);

  expect(visibleText(await screen.findByTestId('folder-confirm-1'))).toBe([
    'Exclude drop?',
    'As of now: on the next scan the index loses 1 file from this folder,',
    'and no document stops being findable — each is also indexed under another path.',
    'The scan can remove more than that: files that never finished indexing are not counted here.',
    'Confirm Cancel',
  ].join(' '));
});

// ---------------------------------------------------------------------------
// Review round 1, M2 — what `Folders.svelte`'s `under` is a copy OF, and what
// holds the other end of it.
//
// 🔴 The review, quoting this file's own older comment, said `under` is a hand
// copy of `crates/mnema-ingest/src/walk.rs:878`. It is not, and the difference
// decides where a pin belongs. That Rust function is called from exactly two
// places — `walk.rs:696`, the ancestor climb, and `walk.rs:768`, inside
// `should_delete` — and both pass a FROZEN prefix. It never sees an exclusion
// rule. What decides whether an exclusion rule covers a path is
// `crates/mnema-walk/src/rules.rs:522`'s `anchored_pattern` — `!/{escaped}` —
// compiled by `ignore`'s gitignore line parser, whose directory patterns match
// across a separator and not into a sibling. A tripwire on `walk.rs`'s `under`
// would have pinned a neighbour: the two encode the same separator rule today
// by coincidence of correctness, not because one is derived from the other.
//
// So the tie is a PAIR, the shape `rust-enum.ts` already argues for. This side
// is `a sibling whose name merely starts with the prefix is not counted`,
// directly below. The Rust side is, in `crates/mnema-walk/tests/rules.rs`,
// `a_user_prefix_does_not_remove_a_sibling_whose_name_starts_with_it`, which
// this round had to WRITE: `a_user_prefix_removes_its_subtree` fixes `private`
// and `public`, names sharing no prefix, and no Rust fixture anywhere under
// `crates/` or `src-tauri/` paired a prefix with a sibling starting with it, so
// nothing said `private2/` survives a rule on `private`. Neither half closes the gap alone, and the real fix — one rule,
// no copy — is `list_tree` carrying the count.
// ---------------------------------------------------------------------------

// 🔴 `drop2` is a SIBLING of `drop`, not a child: `anchored_pattern` produces
// `!/drop`, which the gitignore parser matches across a separator and not into
// a sibling. A count written with `startsWith(prefix)` alone passes every other
// state in this file and fails only here — it would promise a person that
// `drop2/y.md` disappears as well.
test('a sibling whose name merely starts with the prefix is not counted', async () => {
  await askExclude('drop', EMPTY_ROOTS, [
    root({
      rootId: 1,
      absolutePath: '/synthetic/root',
      files: [file('drop/x.md', 'doc-1'), file('drop2/y.md', 'doc-2')],
    }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]);

  expect(visibleText(await screen.findByTestId('folder-confirm-1'))).toBe([
    'Exclude drop?',
    'As of now: on the next scan the index loses 1 file from this folder,',
    'and 1 document stops being findable: no other path names it.',
    'The scan can remove more than that: files that never finished indexing are not counted here.',
    'Confirm Cancel',
  ].join(' '));
});

// A confirmation over nothing trains a person to click through the one that
// matters, so there is none: the rule is stored on the press. `keep/x.md` is
// indexed and `empty/` holds nothing, so the reply is not empty — a component
// that skipped the question by failing to read the reply at all would pass an
// assertion made against a reply with no files in it.
test('excluding a folder holding no indexed path stores it with no question and no loss sentence', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValueOnce(listing(EMPTY_ROOTS));
  listTree.mockResolvedValueOnce(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root', files: [file('keep/x.md', 'doc-1')] }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]));
  listSubfolders.mockResolvedValueOnce(subfolders([sub('empty')]));
  listSubfolders.mockResolvedValue(subfolders([sub('empty', { kind: 'excluded' })]));
  listExclusions.mockResolvedValueOnce([]);
  listExclusions.mockResolvedValue([{ prefix: 'empty', existsOnDisk: true }]);
  excludeSubfolder.mockResolvedValue(undefined);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude empty' })).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Exclude empty' }));

  await waitFor(() => expect(excludeSubfolder).toHaveBeenCalledWith(1, 'empty'));
  expect(screen.queryByTestId('folder-confirm-1')).toBeNull();
  // "Says so plainly" is the row itself, after the re-read: no loss sentence,
  // and the state a person came to set.
  await waitFor(() => expect(visibleText(screen.getByTestId('subfolder-1-empty'))).toBe(
    'empty Excluded by your rule.'
    + ' Without this rule, anything at this path is indexed again from the next scan on.'
    + ' Do not exclude Subfolders',
  ));
});

test('cancelling stores nothing and leaves the row saying the folder is not excluded', async () => {
  await askExclude('drop', EMPTY_ROOTS, [
    root({
      rootId: 1,
      absolutePath: '/synthetic/root',
      files: [file('drop/x.md', 'doc-x'), file('drop/y.md', 'doc-y')],
    }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]);
  await screen.findByTestId('folder-confirm-1');

  await fireEvent.click(screen.getByRole('button', { name: 'Leave drop as it is' }));

  await waitFor(() => expect(screen.queryByTestId('folder-confirm-1')).toBeNull());
  expect(excludeSubfolder).not.toHaveBeenCalled();
  // Not only "nothing was sent": the row still SAYS the folder is open, and
  // still offers the control that would exclude it.
  const row = within(screen.getByTestId('subfolder-1-drop'));
  expect(row.getByText('No rule excludes this folder.')).toBeTruthy();
  expect(row.getByRole('button', { name: 'Exclude drop' })).toBeTruthy();
});

test('confirming stores the rule that was asked about, and the question goes', async () => {
  await askExclude('drop', EMPTY_ROOTS, [
    root({
      rootId: 1,
      absolutePath: '/synthetic/root',
      files: [file('drop/x.md', 'doc-x'), file('drop/y.md', 'doc-y')],
    }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]);
  await screen.findByTestId('folder-confirm-1');

  await fireEvent.click(screen.getByRole('button', { name: 'Confirm excluding drop' }));

  await waitFor(() => expect(excludeSubfolder).toHaveBeenCalledWith(1, 'drop'));
  expect(screen.queryByTestId('folder-confirm-1')).toBeNull();
});

// 🔴 The inverse, and deliberately NOT a count: this window does not know what
// is on disk under a folder it has been ignoring, and inventing a number there
// would be the overstatement the count above was amended to remove.
test('taking a rule away asks first, and names the provider rather than a number', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockResolvedValue(subfolders([sub('Archive', { kind: 'excluded' })]));
  listExclusions.mockResolvedValue([{ prefix: 'Archive', existsOnDisk: true }]);
  includeSubfolder.mockResolvedValue(true);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Do not exclude Archive' })).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Do not exclude Archive' }));

  expect(visibleText(await screen.findByTestId('folder-confirm-1'))).toBe([
    'Stop excluding Archive?',
    'From the next scan on, everything inside this folder is indexed again,',
    'and its text is sent to the model provider.',
    'Confirm Cancel',
  ].join(' '));
  expect(includeSubfolder).not.toHaveBeenCalled();

  await fireEvent.click(screen.getByRole('button', { name: 'Confirm not excluding Archive' }));
  await waitFor(() => expect(includeSubfolder).toHaveBeenCalledWith(1, 'Archive'));
});

// ---------------------------------------------------------------------------
// Review round 1, I1 — the panel used to disagree with itself about one folder.
//
// The question said "its text is sent to the model provider" while the rule row
// further down the same panel said "There is no folder at this path right now",
// both drawn from the same `panel.rules` in the same render. `existsOnDisk` is
// the backend's own answer (`bridge.rs:117`), already on screen; the question
// now reads it rather than promising a cost the same panel denies.
//
// Both directions, in both locales, and both against the WHOLE confirmation
// text: an assertion that only checks the new sentence is present passes on a
// box that prints both.
// ---------------------------------------------------------------------------

const GONE_COST = {
  en: [
    'Stop excluding Old notes?',
    'There is no folder at this path right now, so nothing is being indexed today.',
    'If a folder appears there later, it is indexed and its text is sent to the model provider.',
    'Confirm Cancel',
  ].join(' '),
  uk: [
    'Більше не виключати Old notes?',
    'Наразі за цим шляхом теки немає, тож зараз нічого не індексується.',
    'Якщо тека там з’явиться згодом, вона індексується, а її текст надсилається провайдеру моделі.',
    'Підтвердити Скасувати',
  ].join(' '),
} as const;

const REMOVE_RULE_LABEL = { en: 'Remove the rule on Old notes', uk: 'Прибрати правило на Old notes' } as const;

async function openWithRule(loc: 'en' | 'uk', rules: StoredExclusion[]) {
  setLocale(loc); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockResolvedValue(subfolders([sub('Work')]));
  listExclusions.mockResolvedValue(rules);
  includeSubfolder.mockResolvedValue(true);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByTestId('folder-rules-1')).toBeTruthy());
}

for (const loc of ['en', 'uk'] as const) {
  test(`removing a rule whose folder is gone is not promised to the provider (${loc})`, async () => {
    await openWithRule(loc, [{ prefix: 'Old notes', existsOnDisk: false }]);

    await fireEvent.click(screen.getByRole('button', { name: REMOVE_RULE_LABEL[loc] }));

    expect(visibleText(await screen.findByTestId('folder-confirm-1'))).toBe(GONE_COST[loc]);
    // The other half of the contradiction, still on screen, still saying the
    // same thing as the question above it.
    expect(visibleText(screen.getByTestId('folder-rule-1-Old notes')))
      .toContain(loc === 'en' ? 'There is no folder at this path right now.' : 'Наразі за цим шляхом теки немає.');
    expect(includeSubfolder).not.toHaveBeenCalled();
  });
}

// The direction the test above cannot see on its own: a rule whose folder IS
// there must keep the unconditional sentence. Without this, the gone sentence
// could be returned for every rule and every assertion above would still pass.
test('a rule whose folder is still there keeps the unconditional provider sentence', async () => {
  await openWithRule('en', [{ prefix: 'Old notes', existsOnDisk: true }]);

  await fireEvent.click(screen.getByRole('button', { name: REMOVE_RULE_LABEL.en }));

  expect(visibleText(await screen.findByTestId('folder-confirm-1'))).toBe([
    'Stop excluding Old notes?',
    'From the next scan on, everything inside this folder is indexed again,',
    'and its text is sent to the model provider.',
    'Confirm Cancel',
  ].join(' '));
});

// Task 7. `confirmView` (Folders.svelte:755-780) builds the heading, the cost
// sentence, and both aria-labels for the INCLUDE branch inside the same
// `void $locale` block the exclude branch above already proves reactive — but
// each branch is its own ternary arm, and a literal swapped in for one of
// THESE calls specifically would not be caught by a switch test that only ever
// reaches the exclude arm. Two states, `existsOnDisk` true here and false in
// the test below, cover both halves of the cost ternary (Folders.svelte:766-771).
test('the "stop excluding" question switches language with everything else', async () => {
  await openWithRule('en', [{ prefix: 'Old notes', existsOnDisk: true }]);
  await fireEvent.click(screen.getByRole('button', { name: REMOVE_RULE_LABEL.en }));
  await screen.findByTestId('folder-confirm-1');

  setLocale('uk');
  await tick();

  expect(visibleText(screen.getByTestId('folder-confirm-1'))).toBe([
    'Більше не виключати Old notes?',
    'Від наступного сканування все всередині цієї теки індексується знову,',
    'а її текст надсилається провайдеру моделі.',
    'Підтвердити Скасувати',
  ].join(' '));
  expect(screen.getByRole('button', { name: 'Підтвердити скасування правила на Old notes' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Залишити Old notes як є' })).toBeTruthy();
});

test('the "stop excluding" question for a folder that is gone switches language with everything else', async () => {
  await openWithRule('en', [{ prefix: 'Old notes', existsOnDisk: false }]);
  await fireEvent.click(screen.getByRole('button', { name: REMOVE_RULE_LABEL.en }));
  await screen.findByTestId('folder-confirm-1');

  setLocale('uk');
  await tick();

  expect(visibleText(screen.getByTestId('folder-confirm-1'))).toBe(GONE_COST.uk);
});

// 🔴 The OTHER caller, and it answers `existsOnDisk` from different evidence:
// the row is a directory entry `list_subfolders` read off the disk, so the
// folder is there whatever a stale rule list says. This pins that the row site
// still passes `true` — the rule below it says the folder is gone, and if the
// row went looking through `panel.rules` instead of standing on its own
// listing, this question would turn into the conditional sentence for a folder
// that is demonstrably on screen.
test('a subfolder row asks about the folder it is a listing of, not about the rule list', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockResolvedValue(subfolders([sub('Archive', { kind: 'excluded' })]));
  listExclusions.mockResolvedValue([{ prefix: 'Archive', existsOnDisk: false }]);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Do not exclude Archive' })).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Do not exclude Archive' }));

  expect(visibleText(await screen.findByTestId('folder-confirm-1'))).toBe([
    'Stop excluding Archive?',
    'From the next scan on, everything inside this folder is indexed again,',
    'and its text is sent to the model provider.',
    'Confirm Cancel',
  ].join(' '));
});

// The `list_tree` the count is read from can be refused like any other call.
// §10: what crosses is a sentence, so the sentence is what is shown.
test('a rejected re-read stores nothing, shows no loss sentence, and prints the backend sentence', async () => {
  await askExclude('drop', EMPTY_ROOTS, new Error('the index is not open'));

  await waitFor(() =>
    expect(screen.getByTestId('folder-subfolder-error-1').textContent).toBe('the index is not open'));
  expect(screen.queryByTestId('folder-confirm-1')).toBeNull();
  expect(excludeSubfolder).not.toHaveBeenCalled();
});

// Fix round 6, item 1. The MIRROR of the test above, for `askInclude`'s own
// re-read: fix round 5 gave this branch a deliberate D29 safety decision — a
// removal decided on an answer this window never read is the one direction
// that cannot be undone — and its twin above is pinned, but this one was not.
// Two mutants of the branch (`Folders.svelte:629-637`) were measured green
// across the whole suite: one making the refusal silent, one raising the
// removal question anyway on a rejected listing.
//
// Both directions, for the reason the pair of tests two sections up state: a
// guard that refuses every removal is satisfied by a test that only checks the
// refusal, so this one also re-presses after a successful re-read and confirms
// the removal still goes through for the same folder.
test('a rejected re-read for a rule removal removes nothing and shows the backend sentence, but a later successful re-read still removes it', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValueOnce(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listTree.mockRejectedValueOnce(new Error('the index is not open'));
  listTree.mockResolvedValueOnce(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  listSubfolders.mockResolvedValue(subfolders([sub('Work')]));
  listExclusions.mockResolvedValue([{ prefix: 'Old notes', existsOnDisk: true }]);
  includeSubfolder.mockResolvedValue(true);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByTestId('folder-rules-1')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Remove the rule on Old notes' }));
  await waitFor(() =>
    expect(screen.getByTestId('folder-subfolder-error-1').textContent).toBe('the index is not open'));
  expect(screen.queryByTestId('folder-confirm-1')).toBeNull();
  expect(includeSubfolder).not.toHaveBeenCalled();

  // The successful direction, same folder, same id: refusing this press too
  // would still satisfy every assertion above.
  await fireEvent.click(screen.getByRole('button', { name: 'Remove the rule on Old notes' }));
  await screen.findByTestId('folder-confirm-1');
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm not excluding Old notes' }));

  expect(includeSubfolder).toHaveBeenCalledWith(1, 'Old notes');
});

// D130. The question is a reactive string like everything else on this screen:
// a `t()` call frozen at the moment of the click would keep the English
// sentence in front of a person who has since switched language.
test('a question already on screen switches language with everything else', async () => {
  await askExclude('drop', EMPTY_ROOTS, [
    root({
      rootId: 1,
      absolutePath: '/synthetic/root',
      files: [file('drop/x.md', 'doc-x'), file('drop/y.md', 'doc-y')],
    }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]);
  await screen.findByTestId('folder-confirm-1');

  setLocale('uk');
  await tick();

  expect(visibleText(screen.getByTestId('folder-confirm-1'))).toBe([
    'Виключити drop?',
    'Станом на зараз: при наступному скануванні індекс втратить 2 файли із цієї теки,',
    'а 2 документи більше не знайдуться: інші шляхи на них не ведуть.',
    'Сканування може прибрати більше: файли, які так і не проіндексувалися, тут не враховані.',
    'Підтвердити Скасувати',
  ].join(' '));
  // The two aria-labels `visibleText` cannot see: `confirmAriaLabel` and
  // `cancelAriaLabel` are the same `void $locale` block, but each is its own
  // `t()` call (Folders.svelte:773-778) and neither is a text node.
  expect(screen.getByRole('button', { name: 'Підтвердити виключення drop' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Залишити drop як є' })).toBeTruthy();
});

// The gap between the press and the answer is a state a person sits in, and a
// press that draws nothing reads as a press that did nothing.
test('the wait for the fresh reply says what is being checked', async () => {
  setLocale('en'); // seed, do not inherit
  let release: (v: TreeListing) => void = () => {};
  listTree.mockResolvedValueOnce(listing(EMPTY_ROOTS));
  listTree.mockReturnValueOnce(new Promise<TreeListing>((r) => { release = r; }));
  listSubfolders.mockResolvedValue(subfolders([sub('drop')]));
  listExclusions.mockResolvedValue([]);
  excludeSubfolder.mockResolvedValue(undefined);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));

  expect(visibleText(await screen.findByTestId('folder-confirm-1')))
    .toBe('Checking what this exclusion removes…');
  expect(excludeSubfolder).not.toHaveBeenCalled();

  // Task 7. `checkingLabel` (Folders.svelte:757) is its own branch of
  // `confirmView`, reached only in the gap this test holds open — a switch
  // test landing after `release()` would never pass through here at all.
  setLocale('uk');
  await tick();
  expect(visibleText(screen.getByTestId('folder-confirm-1')))
    .toBe('Перевіряємо, що прибере це виключення…');
  setLocale('en');
  await tick();

  release(listing([root({ rootId: 1, absolutePath: '/synthetic/root', files: [file('drop/x.md', 'doc-1')] })]));
  await waitFor(() => expect(visibleText(screen.getByTestId('folder-confirm-1'))).toBe([
    'Exclude drop?',
    'As of now: on the next scan the index loses 1 file from this folder,',
    'and 1 document stops being findable: no other path names it.',
    'The scan can remove more than that: files that never finished indexing are not counted here.',
    'Confirm Cancel',
  ].join(' ')));
});

// Task 6 opened an async gap of its own — the `list_tree` between the press and
// the question — and this is that gap's own "what appears wrongly" case. The
// row is shut while the reply is on the wire; the reply must raise nothing,
// because the panel it would raise a question in is not the one that was
// pressed. `panels[rootId]` alone does NOT decide this: a row shut and reopened
// is a fresh panel under the same key, and `patch` writes to it happily.
test('a row shut while the check is in flight raises no question when the reply lands', async () => {
  setLocale('en'); // seed, do not inherit
  let release: (v: TreeListing) => void = () => {};
  listTree.mockResolvedValueOnce(listing(EMPTY_ROOTS));
  listTree.mockReturnValueOnce(new Promise<TreeListing>((r) => { release = r; }));
  listSubfolders.mockResolvedValue(subfolders([sub('drop')]));
  listExclusions.mockResolvedValue([]);

  renderWatching();
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));
  await screen.findByTestId('folder-confirm-1');

  // Shut, then open again: the panel under key 1 is a new one.
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());

  release(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root', files: [file('drop/x.md', 'doc-1')] }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]));
  await tick();
  await tick();

  expect(screen.queryByTestId('folder-confirm-1')).toBeNull();
  expect(excludeSubfolder).not.toHaveBeenCalled();
});

// ── PR 8a, Task 8: a job ending re-reads the panel, not only the row ─────────
//
// The live run's finding 1, in this file's own terms. `JobStrip.test.ts` holds
// the whole-window form — the renamed folder that went on reading "excluded by
// your rule" while its text was being sent to the provider. What is here is the
// machinery that form cannot reach: several roots open at once, and a question
// standing on screen when the ending lands.
//
// Driven through the REAL controller and the real `startScanJob`, so what is
// exercised is the wiring a press goes through rather than a store shaped like
// one. The scan is ONE job now — there is no chained second pass and no second
// ending — so every assertion below is about what one ending does.
const IDLE_SCAN: ScanState = {
  revision: 0, files: 0, readSeq: 0, jobsDone: 0, lastReading: null, snapshot: { kind: 'idle' },
};

// 🔴 MOUNTED, not merely created: `Settings.svelte` is what opens the window's
// subscription, and until it is opened nothing is listening for the state an
// ending arrives in. `Folders.svelte` only reads the store.
function renderWatching() {
  const jobs = createJobController();
  jobs.mount();
  return { jobs, ...render(Folders, { props: { jobs } }) };
}

let revision = 0;
const endedScan = (): ScanState => ({
  ...IDLE_SCAN,
  revision: (revision += 1),
  snapshot: {
    kind: 'ended',
    report: {
      embedding: { kind: 'notReached' }, endedIn: 'reading',
      reason: 'completed', message: null, resume: null,
    },
  },
});

// Delivers one state the way the core's own observer does. Waits for the
// subscription first, because `listen` resolves a microtask after `mount`.
//
// 🔴 Task 9. `endedScan()` leaves `readSeq` where `IDLE_SCAN` has it, so THIS
// helper is the ending that moved no reading counter — the negative half of
// every withdrawal pair below. The endings that move it say so, through
// `endScanAt`.
async function endScan() {
  await waitFor(() => expect(deliver).not.toBeNull());
  deliver!(endedScan());
}

// The same delivery, carrying an explicit `readSeq`: how many READING passes
// have ENDED in this process (`ipc.ts:734-745`). It is the only field that
// tells apart the two endings a person sees as one — a run that read folders
// moves it, an `embedOnly` run does not — and the numbers a pending question
// froze were read from a `list_tree` taken before whatever moved it.
async function endScanAt(readSeq: number) {
  await waitFor(() => expect(deliver).not.toBeNull());
  deliver!({ ...endedScan(), readSeq });
}

const NO_COUNTS: Counts = { done: 0, total: 0, skipped: 0, refused: 0, contended: 0, secondsLeft: null };

// A job still RUNNING, carrying a reading counter of its own. This is the
// embedding phase of a `full` run: the reading pass has ended — `readSeq` has
// moved — and the job holding the slot has not ended at all. A withdrawal keyed
// on the snapshot becoming `ended` never sees this state; one keyed on
// `readSeq` does.
async function runEmbeddingAt(readSeq: number) {
  await waitFor(() => expect(deliver).not.toBeNull());
  deliver!({
    ...IDLE_SCAN,
    revision: (revision += 1),
    readSeq,
    snapshot: { kind: 'running', cancellable: true, phase: { kind: 'embedding', counts: NO_COUNTS } },
  });
}

// An `embedOnly` run's ending: it ended IN the embedding phase, that phase ran,
// and it read no folder at all — so `readSeq` is exactly where it was. This is
// the run the withdrawal must ignore, and the state the test deleted at fix
// round 2 (I1) could not build while a scan was still a chain of two jobs.
async function endEmbedOnly() {
  await waitFor(() => expect(deliver).not.toBeNull());
  deliver!({
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
}

test('a job ending re-reads every expanded panel, not only one root', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing(EMPTY_ROOTS));
  listSubfolders.mockImplementation((rootId: number) =>
    Promise.resolve(subfolders([sub(rootId === 1 ? 'first-before' : 'second-before')])));
  listExclusions.mockResolvedValue([]);

  const { jobs } = renderWatching();
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await fireEvent.click(screen.getByTestId('folder-expand-2'));
  await screen.findByTestId('subfolder-1-first-before');
  await screen.findByTestId('subfolder-2-second-before');

  // Task 8: this component no longer starts a scan itself — the single
  // «Сканувати» control lives in the Scanning section now. Started here
  // through the SAME controller that section would use, so what is exercised
  // is still the real wiring an ending goes through, not a store shaped like
  // one.
  void jobs.scan('full');
  await waitFor(() => expect(invoke.mock.calls.some((c) => c[0] === 'start_scan_job')).toBe(true));
  // Swapped only after the scan has started, so the new names can reach the
  // screen only by both panels being READ again.
  listSubfolders.mockImplementation((rootId: number) =>
    Promise.resolve(subfolders([sub(rootId === 1 ? 'first-after' : 'second-after')])));

  await endScan();

  await screen.findByTestId('subfolder-1-first-after');
  // 🔴 The root nobody pressed Scan on. Narrowing the re-read to the pressed
  // root would leave this panel stale — and there is nothing to narrow it BY:
  // one scan covers every watched folder (`scan_state::Entry`), and its report
  // names no root at all.
  expect(screen.getByTestId('subfolder-2-second-after')).toBeTruthy();
  expect(screen.queryByTestId('subfolder-1-first-before')).toBeNull();
  expect(screen.queryByTestId('subfolder-2-second-before')).toBeNull();
});

// The question's two numbers were read from a `list_tree` taken BEFORE the
// scan, and `Pending` freezes them so the sentence cannot renumber itself under
// somebody reading it. A READING PASS ENDING is the event that makes them
// wrong, so the question goes — and it goes visibly, because a press that
// vanishes without a word is its own kind of falsehood.
//
// 🔴 Task 9. The pair this test is one half of: `readSeq` moves here, and its
// twin below delivers the same ending with `readSeq` where it was. The states
// separated are "a reading pass has ended since this question was asked" and
// "a job has ended and no reading pass has" — one sentence on screen tells them
// apart, and the counts are re-read either way.
test('a question standing when a reading pass ends is withdrawn by name, and nothing is stored', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root', files: [file('drop/x.md', 'doc-1')] }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]));
  listSubfolders.mockResolvedValue(subfolders([sub('drop')]));
  listExclusions.mockResolvedValue([]);

  const { jobs } = renderWatching();
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));
  await screen.findByTestId('folder-confirm-1');
  // Both directions: the question is on screen and unwithdrawn until the ending.
  expect(screen.queryByTestId('folder-question-withdrawn-1')).toBeNull();

  // Task 8: started through the controller directly — see the comment on the
  // first test in this section for why that is still the real wiring.
  void jobs.scan('full');
  await waitFor(() => expect(invoke.mock.calls.some((c) => c[0] === 'start_scan_job')).toBe(true));
  const readsBefore = listTree.mock.calls.length;
  await endScanAt(1);

  await waitFor(() => expect(screen.queryByTestId('folder-confirm-1')).toBeNull());
  expect(visibleText(screen.getByTestId('folder-question-withdrawn-1'))).toBe(
    'The question about “drop” has been withdrawn: indexing has finished and this panel was'
    + ' read again. Press again if you still want to.',
  );
  // Withdrawn, not answered: a question taken off the screen must not store the
  // rule it was asking about.
  expect(excludeSubfolder).not.toHaveBeenCalled();
  // The counts are re-read as well, exactly once — the half this pair shares
  // with its twin, so neither test can pass by re-reading nothing.
  await waitFor(() => expect(listTree.mock.calls.length).toBe(readsBefore + 1));

  // And asking again clears the note rather than leaving it beside a live
  // question about the same folder.
  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));
  await screen.findByTestId('folder-confirm-1');
  expect(screen.queryByTestId('folder-question-withdrawn-1')).toBeNull();
});

// 🔴 Task 9, the twin of the test above, and the fact the two separate is
// `ScanState.readSeq`: an ending is not what makes a frozen number wrong, a
// READING PASS having run is. This ending moves no reading counter — the state
// an `embedOnly` run leaves, and the state a job ending for any other reason
// leaves — so the question stands, no withdrawn note appears, and the counts
// are re-read all the same, because any phase can move what the panel draws.
//
// Withdrawing on every ending (what this component did before Task 9) fails
// here and passes its twin; withdrawing on none fails the twin and passes here.
test('a job ending that moves no reading counter leaves the question standing, and still re-reads the counts', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root', files: [file('drop/x.md', 'doc-1')] }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]));
  listSubfolders.mockResolvedValue(subfolders([sub('drop')]));
  listExclusions.mockResolvedValue([]);

  const { jobs } = renderWatching();
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));
  await screen.findByTestId('folder-confirm-1');

  void jobs.scan('full');
  await waitFor(() => expect(invoke.mock.calls.some((c) => c[0] === 'start_scan_job')).toBe(true));
  const readsBefore = listTree.mock.calls.length;
  await endScan(); // `readSeq` stays at 0

  // The re-read is what says the ending was seen at all: without it this test
  // would be green against a component that ignored the subscription entirely.
  await waitFor(() => expect(listTree.mock.calls.length).toBe(readsBefore + 1));
  expect(screen.getByTestId('folder-confirm-1')).toBeTruthy();
  expect(visibleText(screen.getByTestId('folder-confirm-1'))).toContain('Exclude drop?');
  expect(screen.queryByTestId('folder-question-withdrawn-1')).toBeNull();
  expect(excludeSubfolder).not.toHaveBeenCalled();
});

// 🔴 Task 9. The withdrawal is keyed on the SNAPSHOT'S `readSeq`, whatever kind
// carried it — and this is the state that makes "whatever kind" load-bearing: a
// `full` run whose reading pass has ended and whose embedding phase is still
// going. The job has not ended, so nothing here is `ended`; the reading counter
// has moved, so every number a pending question froze was read before it.
//
// Both directions in one fixture, in the order a run produces them: the
// embedding phase arrives first with the counter where it was (nothing happens)
// and then with the counter moved (the question goes).
test('the embedding phase of a run withdraws the question once the reading counter has moved, not before', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root', files: [file('drop/x.md', 'doc-1')] }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]));
  listSubfolders.mockResolvedValue(subfolders([sub('drop')]));
  listExclusions.mockResolvedValue([]);

  const { jobs } = renderWatching();
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));
  await screen.findByTestId('folder-confirm-1');

  void jobs.scan('full');
  await waitFor(() => expect(invoke.mock.calls.some((c) => c[0] === 'start_scan_job')).toBe(true));

  // Fix round 1, m4: "nothing" is not only "the question stands". A component
  // that re-read the list on every progress tick would satisfy the two
  // assertions below and still be wrong — the re-read is what an ENDING earns,
  // and this snapshot is neither an ending nor a moved counter.
  const readsBefore = listTree.mock.calls.length;
  await runEmbeddingAt(0);
  await tick();
  await tick();
  expect(listTree.mock.calls.length).toBe(readsBefore);
  expect(screen.getByTestId('folder-confirm-1')).toBeTruthy();
  expect(screen.queryByTestId('folder-question-withdrawn-1')).toBeNull();

  await runEmbeddingAt(1);

  await waitFor(() => expect(screen.queryByTestId('folder-confirm-1')).toBeNull());
  expect(screen.getByTestId('folder-question-withdrawn-1')).toBeTruthy();
  expect(excludeSubfolder).not.toHaveBeenCalled();
});

// 🔴 Task 9. The question was raised AFTER the reading pass ended, so its two
// numbers were read from a `list_tree` that already knew what that pass did.
// The job's own ending, arriving later with the counter where the question
// found it, invalidates nothing.
//
// This is the property fix round 2 (I1) deleted with its test when the chain of
// two jobs disappeared: back then the state was "a question raised while the
// chained embedding pass ran"; the durable fact underneath it is this counter.
test('a question raised after the reading counter last moved survives the job ending that follows', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root', files: [file('drop/x.md', 'doc-1')] }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]));
  listSubfolders.mockResolvedValue(subfolders([sub('drop')]));
  listExclusions.mockResolvedValue([]);

  const { jobs } = renderWatching();
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());

  void jobs.scan('full');
  await waitFor(() => expect(invoke.mock.calls.some((c) => c[0] === 'start_scan_job')).toBe(true));
  // The reading pass ends first, with no question on screen to withdraw.
  await runEmbeddingAt(1);
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());

  // Only now is the question asked — its numbers come from a listing read after
  // that pass landed.
  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));
  await screen.findByTestId('folder-confirm-1');

  await endScanAt(1); // the job ends; the reading counter does not move again

  await waitFor(() => expect(listTree.mock.calls.length).toBeGreaterThan(0));
  await tick();
  await tick();
  expect(screen.getByTestId('folder-confirm-1')).toBeTruthy();
  expect(screen.queryByTestId('folder-question-withdrawn-1')).toBeNull();
  expect(excludeSubfolder).not.toHaveBeenCalled();
});

// 🔴 Task 9. The run the withdrawal must ignore, in the shape the backend
// actually produces it: `endedIn: 'embedding'` with the embedding phase having
// RUN, and `readSeq` untouched because no folder was read. Its pair is the
// first test in this group, where the same `ended` snapshot carries a moved
// counter and the question goes.
test('an embedding-only run withdraws nothing, and re-reads the counts once', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root', files: [file('drop/x.md', 'doc-1')] }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]));
  listSubfolders.mockResolvedValue(subfolders([sub('drop')]));
  listExclusions.mockResolvedValue([]);

  const { jobs } = renderWatching();
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));
  await screen.findByTestId('folder-confirm-1');

  void jobs.scan('embedOnly');
  await waitFor(() => expect(invoke.mock.calls.some((c) => c[0] === 'start_scan_job')).toBe(true));
  const readsBefore = listTree.mock.calls.length;
  await endEmbedOnly();

  await waitFor(() => expect(listTree.mock.calls.length).toBe(readsBefore + 1));
  expect(screen.getByTestId('folder-confirm-1')).toBeTruthy();
  expect(screen.queryByTestId('folder-question-withdrawn-1')).toBeNull();
  expect(excludeSubfolder).not.toHaveBeenCalled();
});

// The same withdrawal one step earlier, where the generation counter is what
// does the work: the press has been made, the `list_tree` behind it is still on
// the wire, and there is no question on screen yet to remove. `ask` is bumped,
// so the reply raises nothing when it lands — and the person is told which
// press it was that came to nothing.
test('a check still in flight when a job ends raises no question when its reply lands', async () => {
  setLocale('en'); // seed, do not inherit
  let release: (v: TreeListing) => void = () => {};
  listTree.mockResolvedValueOnce(listing(EMPTY_ROOTS));
  listTree.mockReturnValueOnce(new Promise<TreeListing>((r) => { release = r; }));
  listTree.mockResolvedValue(listing(EMPTY_ROOTS));
  listSubfolders.mockResolvedValue(subfolders([sub('drop')]));
  listExclusions.mockResolvedValue([]);

  const { jobs } = renderWatching();
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));
  expect(visibleText(await screen.findByTestId('folder-confirm-1')))
    .toBe('Checking what this exclusion removes…');

  void jobs.scan('full');
  await waitFor(() => expect(invoke.mock.calls.some((c) => c[0] === 'start_scan_job')).toBe(true));
  await endScanAt(1);
  await waitFor(() => expect(screen.queryByTestId('folder-confirm-1')).toBeNull());
  expect(screen.getByTestId('folder-question-withdrawn-1')).toBeTruthy();

  release(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root', files: [file('drop/x.md', 'doc-1')] }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]));
  await tick();
  await tick();

  expect(screen.queryByTestId('folder-confirm-1')).toBeNull();
  expect(excludeSubfolder).not.toHaveBeenCalled();
});

// ── Fix round 2, I1 — RESTORED BY TASK 9, ON `readSeq` ─────────────────────
//
// The test that once stood here was «an embedding pass ending does not withdraw
// a question raised after the walk that chained it». Its whole subject was the
// CHAIN: a walk ended, `jobs.ts` started an embedding pass behind it, a person
// raised a question while that pass ran, and the pass's own ending must not
// discard it.
//
// There is no chain any more — a scan is ONE job with two phases and one ending
// (`scan_state.rs`) — so that sequence cannot be built at all. What replaces it
// is the durable fact the chain was standing in for: `ScanState.readSeq`, how
// many reading passes have ENDED. Its two states are guarded above, in
// «a question raised after the reading counter last moved survives the job
// ending that follows» and «an embedding-only run withdraws nothing, and
// re-reads the counts once».

// ── Fix round 1, I1 ─────────────────────────────────────────────────────────
//
// The withdrawal used to run inside `refresh().then(…)`, so a rejected
// `list_tree` at a scan's ending took it down with the re-read — and the ending
// is consumed once (`seen` advances before `reread` is called), so it never
// came back. The evidence was then wiped by an unrelated success: a LATER
// ending refreshes fine, clears `loadError`, and redraws the panel with the
// question still on it, stating numbers a scan has already moved and carrying
// nothing to say a scan happened.
//
// The second ending was the chained embedding pass's before this task. There is
// no chain any more, so it is a second scan's — the same shape of event, and
// the same "an unrelated success wipes the evidence" sequence.
//
// 🔴 Task 9: the first ending is the one that MOVES the reading counter, which
// is what makes it a withdrawal at all; the second leaves the counter where the
// first put it, so it re-reads and withdraws nothing — and the note the first
// wrote has to still be there when it redraws the panel.
test('a reading pass ending withdraws the question even when the re-read that follows it fails', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root', files: [file('drop/x.md', 'doc-1')] }),
  ]));
  listSubfolders.mockResolvedValue(subfolders([sub('drop')]));
  listExclusions.mockResolvedValue([]);

  const { jobs } = renderWatching();
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));
  await screen.findByTestId('folder-confirm-1');

  void jobs.scan('full');
  await waitFor(() => expect(invoke.mock.calls.some((c) => c[0] === 'start_scan_job')).toBe(true));

  // Queued here and not at the top, so it is the re-read AT THE ENDING that
  // fails and nothing earlier consumes it.
  listTree.mockRejectedValueOnce(new Error('the folder list could not be read'));
  await endScanAt(1);

  // The direction nobody asserted: the failure is visible rather than silent.
  // While it stands, the panel is off screen entirely — which is exactly how
  // the stale question used to survive unnoticed.
  await waitFor(() => expect(visibleText(screen.getByTestId('folders-load-reason')))
    .toBe('the folder list could not be read'));

  // The unrelated success that used to wipe the evidence. It was the CHAINED
  // embedding pass's ending before this task; there is no chain any more, so it
  // is a second scan's ending — the same shape of event, arriving from a job
  // this window did not have to start. The counter stays at 1: this ending
  // re-reads and withdraws nothing, and the note the first one wrote is what
  // has to survive the redraw.
  await endScanAt(1);
  await waitFor(() => expect(screen.queryByTestId('folders-load-reason')).toBeNull());
  await screen.findByTestId('folder-panel-1');

  expect(screen.queryByTestId('folder-confirm-1')).toBeNull();
  expect(visibleText(screen.getByTestId('folder-question-withdrawn-1'))).toBe(
    'The question about “drop” has been withdrawn: indexing has finished and this panel was'
    + ' read again. Press again if you still want to.',
  );
  expect(excludeSubfolder).not.toHaveBeenCalled();
});

// M1. `settings_folders_question_withdrawn` was asserted only under
// `setLocale('en')`, and `withdrawnNote` (Folders.svelte:829-832) needed its
// own switch test by this file's own stated standard (`Folders.test.ts:1450`
// above, of the exclude/include branches): a literal swapped in for THIS
// `t()` call specifically would not be caught by a switch test that only ever
// reaches a different arm.
test('the withdrawn-question note switches language with everything else', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root', files: [file('drop/x.md', 'doc-1')] }),
  ]));
  listSubfolders.mockResolvedValue(subfolders([sub('drop')]));
  listExclusions.mockResolvedValue([]);

  const { jobs } = renderWatching();
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));
  await screen.findByTestId('folder-confirm-1');

  void jobs.scan('full');
  await waitFor(() => expect(invoke.mock.calls.some((c) => c[0] === 'start_scan_job')).toBe(true));
  await endScanAt(1);
  await screen.findByTestId('folder-question-withdrawn-1');

  setLocale('uk');
  await tick();

  expect(visibleText(screen.getByTestId('folder-question-withdrawn-1'))).toBe(
    'Питання про «drop» знято: індексацію закінчено, і цю панель перечитано.'
    + ' Натисніть ще раз, якщо це досі потрібно.',
  );
});

// ---------------------------------------------------------------------------
// PR 8a, fix round 5 — the SECOND and THIRD sites of round 4's own rule.
//
// Round 4 taught `refresh` that a panel belongs to a folder and not to a
// number. `askExclude` went on matching a fresh `list_tree` on the id alone, so
// the question quoted another folder's file and document counts under this
// folder's row and stored this folder's relative path on the new root's id.
// `askInclude` read no listing at all, which is the worse half: it went
// straight to `include_subfolder` and could take a person's rule off a folder
// they never pointed at — under D29 that folder's text is at the model provider
// after the next scan.
//
// 🔴 Every fixture below moves BOTH ends of the identity, and that is what
// makes it a test of the rule rather than of half of it: the panel's id (9)
// comes back naming a different folder, AND the panel's own path comes back
// under a different id. A predicate that asked only "is this path still in the
// listing" is green on an id-only fixture; one that asked only "is this id
// still in the listing" is green on a path-only one.
//
// The pairs are pairs for the reason round 4's are: a guard that refuses
// everything satisfies every assertion about a refusal. The one-line mutants,
// named in advance:
//   `if (!namesFolder(listing, rootId, panel.rootPath))` deleted at either site
//     → that site's refusal test fails, its "still asks" twin stays green;
//   the same condition replaced by `if (true)` at either site
//     → that site's "still asks" test fails, its refusal twin stays green.
const REUSED_ID = [
  // The id the panel was opened under, now naming another folder — and holding
  // two indexed paths under `Work`, so a question asked from it would be a
  // question quoting numbers this person's folder never had.
  root({
    rootId: 9,
    absolutePath: '/synthetic/gamma',
    files: [file('Work/x.md', 'doc-x'), file('Work/y.md', 'doc-y')],
  }),
  // And the panel's own folder, still watched, under a different id.
  root({ rootId: 4, absolutePath: '/synthetic/beta', files: [] }),
];

const CHANGED_NOTE_EN =
  'This folder is no longer the one that was on screen: another folder has taken its place.'
  + ' Nothing was changed.';

async function openBetaPanel(entries: Subfolder[], rules: StoredExclusion[]) {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValueOnce(listing([
    root({ rootId: 9, absolutePath: '/synthetic/beta' }),
    root({ rootId: 4, absolutePath: '/synthetic/alpha' }),
  ]));
  listSubfolders.mockResolvedValue(subfolders(entries));
  listExclusions.mockResolvedValue(rules);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/beta')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-9'));
  await waitFor(() => expect(screen.getByTestId('folder-rules-9')).toBeTruthy());
}

test('an exclude question is not asked when the id now names another folder, and quotes none of its numbers', async () => {
  await openBetaPanel([sub('Work')], []);
  // Everything from the press on reads the changed world, the re-read included.
  listTree.mockResolvedValue(listing(REUSED_ID));
  excludeSubfolder.mockResolvedValue(undefined);

  await fireEvent.click(screen.getByRole('button', { name: 'Exclude Work' }));

  await waitFor(() => expect(screen.getByTestId('folders-root-changed')).toBeTruthy());
  expect(visibleText(screen.getByTestId('folders-root-changed'))).toBe(CHANGED_NOTE_EN);
  // Not merely "no question": none of gamma's numbers is on screen under
  // beta's row, which is the sentence the defect actually produced.
  expect(screen.queryByTestId('folder-confirm-9')).toBeNull();
  expect(screen.queryByText(/2 files/)).toBeNull();
  expect(excludeSubfolder).not.toHaveBeenCalled();
  // The panel belongs to the folder, so it goes with it, and the list now shows
  // what is at that id.
  await waitFor(() => expect(screen.queryByTestId('folder-panel-9')).toBeNull());
  expect(screen.getByText('/synthetic/gamma')).toBeTruthy();

  setLocale('uk');
  await tick();
  expect(visibleText(screen.getByTestId('folders-root-changed'))).toBe(
    'Ця тека вже не та, що була на екрані: її місце посіла інша.'
    + ' Нічого не змінено.',
  );
});

// The other direction, and the fixture still moves a root: ANOTHER folder has
// changed hands between the two reads, and this panel's has not. A guard that
// asked whether the listing as a whole had moved would refuse here.
test('an exclude question is asked as before when this folder\'s id and path still agree', async () => {
  await openBetaPanel([sub('Work')], []);
  listTree.mockResolvedValue(listing([
    root({
      rootId: 9,
      absolutePath: '/synthetic/beta',
      files: [file('Work/x.md', 'doc-x'), file('Work/y.md', 'doc-y')],
    }),
    root({ rootId: 7, absolutePath: '/synthetic/alpha', files: [] }),
  ]));
  excludeSubfolder.mockResolvedValue(undefined);

  await fireEvent.click(screen.getByRole('button', { name: 'Exclude Work' }));

  expect(visibleText(await screen.findByTestId('folder-confirm-9'))).toBe([
    'Exclude Work?',
    'As of now: on the next scan the index loses 2 files from this folder,',
    'and 2 documents stop being findable: no other path names them.',
    'The scan can remove more than that: files that never finished indexing are not counted here.',
    'Confirm Cancel',
  ].join(' '));
  expect(screen.queryByTestId('folders-root-changed')).toBeNull();
});

// 🔴 The mirror, and the one that removes protection rather than over-warning.
test('a rule is not removed when the id now names another folder, and no question offers to', async () => {
  await openBetaPanel([sub('Archive', { kind: 'excluded' })], [{ prefix: 'Archive', existsOnDisk: true }]);
  listTree.mockResolvedValue(listing(REUSED_ID));
  includeSubfolder.mockResolvedValue(true);

  await fireEvent.click(screen.getByRole('button', { name: 'Remove the rule on Archive' }));

  await waitFor(() => expect(screen.getByTestId('folders-root-changed')).toBeTruthy());
  expect(visibleText(screen.getByTestId('folders-root-changed'))).toBe(CHANGED_NOTE_EN);
  // The control that would take the rule away is not on screen at all: a person
  // cannot confirm a removal on a folder they never pointed at.
  expect(screen.queryByTestId('folder-confirm-9')).toBeNull();
  expect(screen.queryByRole('button', { name: 'Confirm not excluding Archive' })).toBeNull();
  expect(includeSubfolder).not.toHaveBeenCalled();
  await waitFor(() => expect(screen.queryByTestId('folder-panel-9')).toBeNull());
});

test('a rule removal is asked about and goes through when this folder\'s id and path still agree', async () => {
  await openBetaPanel([sub('Archive', { kind: 'excluded' })], [{ prefix: 'Archive', existsOnDisk: true }]);
  listTree.mockResolvedValue(listing([
    root({ rootId: 9, absolutePath: '/synthetic/beta', files: [] }),
    root({ rootId: 7, absolutePath: '/synthetic/alpha', files: [] }),
  ]));
  includeSubfolder.mockResolvedValue(true);

  await fireEvent.click(screen.getByRole('button', { name: 'Remove the rule on Archive' }));

  expect(visibleText(await screen.findByTestId('folder-confirm-9'))).toBe([
    'Stop excluding Archive?',
    'From the next scan on, everything inside this folder is indexed again,',
    'and its text is sent to the model provider.',
    'Confirm Cancel',
  ].join(' '));
  expect(screen.queryByTestId('folders-root-changed')).toBeNull();
  expect(includeSubfolder).not.toHaveBeenCalled();

  await fireEvent.click(screen.getByRole('button', { name: 'Confirm not excluding Archive' }));
  await waitFor(() => expect(includeSubfolder).toHaveBeenCalledWith(9, 'Archive'));
});

// The gap this press now has and did not have before, in both languages. The
// exclude wait says what it is costing; this one is not costing anything, and a
// shared sentence would tell a person their removal is being counted up.
test('the wait before a rule removal says what it is checking, and switches language', async () => {
  setLocale('en'); // seed, do not inherit
  let release: (v: TreeListing) => void = () => {};
  listTree.mockResolvedValueOnce(listing([root({ rootId: 9, absolutePath: '/synthetic/beta' })]));
  listTree.mockReturnValueOnce(new Promise<TreeListing>((r) => { release = r; }));
  listSubfolders.mockResolvedValue(subfolders([sub('Archive', { kind: 'excluded' })]));
  listExclusions.mockResolvedValue([{ prefix: 'Archive', existsOnDisk: true }]);
  includeSubfolder.mockResolvedValue(true);

  render(Folders, { props: { jobs: createJobController() } });
  await waitFor(() => expect(screen.getByText('/synthetic/beta')).toBeTruthy());
  await fireEvent.click(screen.getByTestId('folder-expand-9'));
  await waitFor(() => expect(screen.getByTestId('folder-rules-9')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Remove the rule on Archive' }));

  expect(visibleText(await screen.findByTestId('folder-confirm-9')))
    .toBe('Checking that this is still the same folder…');
  expect(includeSubfolder).not.toHaveBeenCalled();

  setLocale('uk');
  await tick();
  expect(visibleText(screen.getByTestId('folder-confirm-9')))
    .toBe('Перевіряємо, що це досі та сама тека…');
  setLocale('en');
  await tick();

  release(listing([root({ rootId: 9, absolutePath: '/synthetic/beta' })]));
  await waitFor(() => expect(visibleText(screen.getByTestId('folder-confirm-9'))).toBe([
    'Stop excluding Archive?',
    'From the next scan on, everything inside this folder is indexed again,',
    'and its text is sent to the model provider.',
    'Confirm Cancel',
  ].join(' ')));
});

// ── Task 9: «Remove» asks before it removes ─────────────────────────────────
//
// Removing a watched folder is the only press on this screen that takes
// documents out of the index without a scan and without a second thought, and
// until this task it went straight to `remove_watched_folder` from the row. The
// question in front of it is the same shape as the exclusion question one level
// down — frozen numbers, words rebuilt under `void $locale`, a named Confirm —
// with one difference that matters: this question lives at the LIST level, not
// on a panel, because the panel exists only while a row is expanded and this
// press is available whether the row is open or shut.
//
// Every fixture below carries TWO roots with DIFFERENT file counts, so a
// question that quoted the list's first row, or a confirm that re-derived its
// path from the id, is a failure rather than a coincidence.
const THREE_AND_FIVE = [
  root({
    rootId: 1,
    absolutePath: '/synthetic/root',
    files: [file('a.md', 'doc-a'), file('b.md', 'doc-b'), file('c.md', 'doc-c')],
  }),
  root({
    rootId: 2,
    absolutePath: '/synthetic/other',
    files: [
      file('p.md', 'doc-p'), file('q.md', 'doc-q'), file('r.md', 'doc-r'),
      file('s.md', 'doc-s'), file('t.md', 'doc-t'),
    ],
  }),
];

const REMOVE_QUESTION_EN =
  'Remove folder /synthetic/root from the index? 3 files from this folder will disappear from search.';

// A state the store can be handed back to after a run: `idle` is not `ended`,
// and the two are different rows in the table this section asserts.
async function goIdle() {
  await waitFor(() => expect(deliver).not.toBeNull());
  deliver!({ ...IDLE_SCAN, revision: (revision += 1) });
}

async function showTwoRoots(roots: TreeRoot[] = THREE_AND_FIVE, waitFor_ = '/synthetic/root') {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing(roots));
  const rendered = renderWatching();
  await waitFor(() => expect(screen.getByText(waitFor_)).toBeTruthy());
  return rendered;
}

// 🔴 Fix round 1, m8. A process in which a reading pass has ALREADY ended
// before this component mounted — `job_status` answers a state whose `readSeq`
// is 1, which is what a window opened mid-life reads.
//
// The store is awaited BEFORE the component renders, and that IS the fixture:
// the seeding under test happens in `onMount`, so a render that ran while the
// store still held the default would seed 0 from an empty store and the test
// would pass against the very mutant it exists to kill.
async function renderAfterAReading() {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/root', files: [file('drop/x.md', 'doc-1')] }),
    root({ rootId: 2, absolutePath: '/synthetic/other', files: [] }),
  ]));
  listSubfolders.mockResolvedValue(subfolders([sub('drop')]));
  listExclusions.mockResolvedValue([]);
  invoke.mockImplementation((cmd: string) =>
    Promise.resolve(cmd === 'job_status' ? { ...IDLE_SCAN, revision: 1, readSeq: 1 } : undefined));

  const jobs = createJobController();
  jobs.mount();
  await waitFor(() => expect(get(jobs.state).scan.readSeq).toBe(1));
  // The deliveries below have to be newer than what the store now holds.
  revision = 1;

  const rendered = render(Folders, { props: { jobs } });
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  return { jobs, ...rendered };
}

function removeButton(path: string): HTMLButtonElement {
  return screen.getByRole('button', { name: `Remove ${path}` }) as HTMLButtonElement;
}

// The states separated: "the press has been made" and "the removal has been
// asked for". Before Task 9 they were one state, and the second is the one that
// takes documents out of the index.
test('«Remove» asks a question naming the folder and counting its files, and calls nothing', async () => {
  await showTwoRoots();
  // The direction a question-shaped assertion alone cannot see: nothing is on
  // screen until the press.
  expect(screen.queryByTestId('folder-remove-confirm-1')).toBeNull();

  await fireEvent.click(removeButton('/synthetic/root'));

  expect(visibleText(await screen.findByTestId('folder-remove-confirm-1')))
    .toBe([REMOVE_QUESTION_EN, 'Confirm', 'Cancel'].join(' '));
  expect(removeWatchedFolder).not.toHaveBeenCalled();
  // Named, so two folders' Confirm buttons are told apart by a screen reader
  // exactly as the two Remove buttons already are.
  expect(screen.getByRole('button', { name: 'Confirm removing /synthetic/root' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Leave /synthetic/root as it is' })).toBeTruthy();
});

// The states separated: "a question about this folder" and "a question about
// that one". At most one is on screen, so a person answering cannot answer the
// wrong one — and the count comes from the row that was pressed, which is what
// the second press proves: 5, not the 3 the first row holds.
test('a second row\'s «Remove» replaces the question rather than putting a second one on screen', async () => {
  await showTwoRoots();

  await fireEvent.click(removeButton('/synthetic/root'));
  await screen.findByTestId('folder-remove-confirm-1');

  await fireEvent.click(removeButton('/synthetic/other'));

  expect(visibleText(await screen.findByTestId('folder-remove-confirm-2'))).toBe([
    'Remove folder /synthetic/other from the index? 5 files from this folder will disappear from search.',
    'Confirm', 'Cancel',
  ].join(' '));
  expect(screen.queryByTestId('folder-remove-confirm-1')).toBeNull();
  expect(removeWatchedFolder).not.toHaveBeenCalled();
});

// The states separated: "the question was answered no" and "the question is
// still standing". Cancelling is not a removal that failed — nothing is sent.
test('«Cancel» takes the removal question away and removes nothing', async () => {
  await showTwoRoots();

  await fireEvent.click(removeButton('/synthetic/root'));
  await screen.findByTestId('folder-remove-confirm-1');

  await fireEvent.click(screen.getByRole('button', { name: 'Leave /synthetic/root as it is' }));

  await waitFor(() => expect(screen.queryByTestId('folder-remove-confirm-1')).toBeNull());
  expect(removeWatchedFolder).not.toHaveBeenCalled();
  // The row is still there and still offers to remove it: a cancelled question
  // leaves the list exactly as it found it.
  expect(removeButton('/synthetic/root').disabled).toBe(false);
});

// 🔴 The states separated: "the question was answered" and "the list re-read
// behind it has landed". They are independent, and the answer must not wait for
// the second: what `remove_watched_folder` is sent is the pair frozen at the
// click, read from the question and from nothing on screen.
//
// Fix round 1, I1, split this test in two and this is the half that keeps its
// original subject. Its old fixture let the re-read land first and asserted the
// question still drawn under a row that had been renamed underneath it — a
// state the identity check in `refresh` now makes unreachable, and the test
// below is what pins that instead.
test('«Confirm» sends the frozen id and path while the list re-read behind it is still on the wire', async () => {
  const { jobs } = await showTwoRoots();
  removeWatchedFolder.mockResolvedValue(1);

  await fireEvent.click(removeButton('/synthetic/root'));
  await screen.findByTestId('folder-remove-confirm-1');

  // The ending's own re-read, held open: nothing has been compared against a
  // fresh listing when the answer below is given.
  let release: (v: TreeListing) => void = () => {};
  listTree.mockReturnValueOnce(new Promise<TreeListing>((r) => { release = r; }));
  void jobs.scan('full');
  await endScan(); // a job ended; no reading pass did
  await tick();
  expect(screen.getByTestId('folder-remove-confirm-1')).toBeTruthy();

  await fireEvent.click(screen.getByRole('button', { name: 'Confirm removing /synthetic/root' }));

  await waitFor(() => expect(removeWatchedFolder).toHaveBeenCalledWith(1, '/synthetic/root'));
  expect(removeWatchedFolder).toHaveBeenCalledTimes(1);
  // Answered, so the question goes: a confirmation left on screen invites a
  // second press against a list that has already changed.
  await waitFor(() => expect(screen.queryByTestId('folder-remove-confirm-1')).toBeNull());

  release(listing(THREE_AND_FIVE)); // let the held read settle
  await tick();
});

// 🔴 Fix round 1, I1. The states separated: "this id still names the folder the
// question is about" and "it does not". `refresh` runs on a mount, an add, a
// removal's answer and every ending that moves no reading counter, and the
// question is withdrawn only by `readSeq` growth — so a plain re-read leaves it
// standing by design, and `watched_root.id` is a rowid alias that a
// remove-and-add elsewhere hands to another folder.
//
// Both directions, in one fixture, in the order a person meets them: a re-read
// that keeps the identity leaves the question exactly as it was — frozen count
// included, which is the half that is still distinguishable — and a re-read
// that moves it takes the question away and names the folder it was about.
//
// Without the check in `refresh`, the second half draws "Remove folder
// /synthetic/root — 3 files" directly beneath a row reading /synthetic/renamed.
test('a re-read that keeps this row\'s identity leaves the removal question standing; one that moves it withdraws it by name', async () => {
  const { jobs } = await showTwoRoots();

  await fireEvent.click(removeButton('/synthetic/root'));
  await screen.findByTestId('folder-remove-confirm-1');

  // Same id, same path, a DIFFERENT count: the identity is intact, so the
  // question stands — and it goes on stating the number it was asked with,
  // which a question re-deriving `files` per render would not.
  listTree.mockResolvedValue(listing([
    root({
      rootId: 1,
      absolutePath: '/synthetic/root',
      files: [
        file('a.md', 'doc-a'), file('b.md', 'doc-b'), file('c.md', 'doc-c'),
        file('d.md', 'doc-d'), file('e.md', 'doc-e'), file('f.md', 'doc-f'),
        file('g.md', 'doc-g'),
      ],
    }),
    THREE_AND_FIVE[1],
  ]));
  void jobs.scan('full');
  await endScan();
  await waitFor(() => expect(screen.getByText('Indexed: 7 documents')).toBeTruthy());

  expect(visibleText(screen.getByTestId('folder-remove-confirm-1')))
    .toBe([REMOVE_QUESTION_EN, 'Confirm', 'Cancel'].join(' '));
  expect(screen.queryByTestId('folders-remove-withdrawn')).toBeNull();

  // The same id, now naming another folder — what a remove-and-add elsewhere
  // does to this window between two reads.
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/renamed', files: [file('z.md', 'doc-z')] }),
    THREE_AND_FIVE[1],
  ]));
  await endScan();

  await waitFor(() => expect(screen.getByText('/synthetic/renamed')).toBeTruthy());
  expect(screen.queryByTestId('folder-remove-confirm-1')).toBeNull();
  expect(visibleText(screen.getByTestId('folders-remove-withdrawn'))).toBe(
    'The question about folder “/synthetic/root” has been withdrawn: indexing has finished and'
    + ' the list was read again. Press again if you still want to.',
  );
  expect(removeWatchedFolder).not.toHaveBeenCalled();
});

// 🔴 Fix round 1, I1, the quieter half of the same finding. When the root
// simply LEAVES the listing, no row renders the question at all — so without
// the check the press vanishes with no word, which is the falsehood
// `withdrawQuestions` exists to prevent. One comparison answers both cases,
// which is why `namesFolder` compares against a value that is `undefined` when
// the id has gone.
test('a removal question about a root that leaves the listing is withdrawn by name, not in silence', async () => {
  const { jobs } = await showTwoRoots();

  await fireEvent.click(removeButton('/synthetic/root'));
  await screen.findByTestId('folder-remove-confirm-1');

  listTree.mockResolvedValue(listing([THREE_AND_FIVE[1]])); // root 1 is gone
  void jobs.scan('full');
  await endScan();

  await waitFor(() => expect(screen.queryByText('/synthetic/root')).toBeNull());
  expect(screen.queryByTestId('folder-remove-confirm-1')).toBeNull();
  expect(visibleText(screen.getByTestId('folders-remove-withdrawn'))).toBe(
    'The question about folder “/synthetic/root” has been withdrawn: indexing has finished and'
    + ' the list was read again. Press again if you still want to.',
  );
  expect(removeWatchedFolder).not.toHaveBeenCalled();
});

// The states separated: "the removal is in flight" and "the backend has
// answered". A row that went on offering both its buttons through the wait is
// a row a second press reaches, and `remove_watched_folder` would be sent twice
// for one folder.
test('a removal in flight says so in that row, and every other row\'s «Remove» is disabled until it answers', async () => {
  await showTwoRoots();
  let settle: (v: number) => void = () => {};
  removeWatchedFolder.mockReturnValueOnce(new Promise<number>((r) => { settle = r; }));
  const readsBefore = listTree.mock.calls.length;

  await fireEvent.click(removeButton('/synthetic/root'));
  await screen.findByTestId('folder-remove-confirm-1');
  expect(removeButton('/synthetic/other').disabled).toBe(false); // the direction before

  await fireEvent.click(screen.getByRole('button', { name: 'Confirm removing /synthetic/root' }));

  expect(visibleText(await screen.findByTestId('folder-removing-1'))).toBe('Removing…');
  // In place of that row's buttons, not beside them.
  expect(screen.queryByRole('button', { name: 'Remove /synthetic/root' })).toBeNull();
  expect(removeButton('/synthetic/other').disabled).toBe(true);
  expect(listTree.mock.calls.length).toBe(readsBefore); // nothing re-read yet

  // Its own `$derived` with its own `void $locale` guard, so it owes its own
  // switch — a literal swapped in for THIS `t()` call would not be caught by a
  // switch test that only ever reaches another one. Read under 'en' above
  // first, so a derived that never recomputes has already cached the English
  // value here (`Folders.test.ts:422`'s own reasoning).
  setLocale('uk');
  await tick();
  expect(visibleText(screen.getByTestId('folder-removing-1'))).toBe('Видаляємо…');
  setLocale('en');
  await tick();

  settle(1);

  await waitFor(() => expect(screen.queryByTestId('folder-removing-1')).toBeNull());
  expect(removeButton('/synthetic/other').disabled).toBe(false);
  // The list is re-read on success — the row's disappearance is the listing's
  // doing, never a local patch.
  expect(listTree.mock.calls.length).toBe(readsBefore + 1);
});

// 🔴 The states separated: "the removal was refused" and "the row the person
// confirmed is still the row that is there". `remove_watched_folder` answers
// `WatchedRootChanged`'s sentence when the id no longer names that path
// (`bridge.rs:97-180`), and the list is what says what is there now — so the
// refusal is shown verbatim AND the list is re-read. Nothing here branches on
// the text.
test('a refused removal shows the backend sentence verbatim and re-reads the list', async () => {
  await showTwoRoots();
  removeWatchedFolder.mockRejectedValueOnce(
    new Error('This folder is no longer the one that was on screen.'));
  // What the list holds now: the row the person confirmed is not in it.
  listTree.mockResolvedValue(listing([THREE_AND_FIVE[1]]));

  await fireEvent.click(removeButton('/synthetic/root'));
  await screen.findByTestId('folder-remove-confirm-1');
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm removing /synthetic/root' }));

  await waitFor(() => expect(visibleText(screen.getByTestId('folders-action-error')))
    .toBe('This folder is no longer the one that was on screen.'));
  // The re-read is the half a rejection alone would not do: the row the person
  // pressed is gone from the screen, because it is gone from the listing.
  await waitFor(() => expect(screen.queryByText('/synthetic/root')).toBeNull());
  expect(screen.getByText('/synthetic/other')).toBeTruthy();
  // And the row is not left saying it is being removed.
  expect(screen.queryByTestId('folder-removing-1')).toBeNull();

  // The sentence is about the LAST press, so the next press takes it away —
  // the rule `rootChanged` and the added-folder note are both written under,
  // now that the press that starts a removal is `askRemove` and not the
  // confirmation behind it.
  await fireEvent.click(removeButton('/synthetic/other'));

  expect(screen.queryByTestId('folders-action-error')).toBeNull();
});

// 🔴 The states separated: "a job holds the slot" and "it does not".
// `remove_watched_folder` is refused while a job holds the slot
// (`bridge.rs:97-180`), so the press is taken off the screen before it is made
// and ONE sentence says why. Read live from the store, not captured at the
// press: a run starting while the question is open has to reach the Confirm
// button that is already drawn.
test('while a job runs every «Remove» is disabled with one sentence, and an open question cannot be confirmed', async () => {
  await showTwoRoots();
  expect(screen.queryByTestId('folders-remove-blocked')).toBeNull(); // the direction before

  await fireEvent.click(removeButton('/synthetic/root'));
  await screen.findByTestId('folder-remove-confirm-1');
  const confirm = () =>
    screen.getByRole('button', { name: 'Confirm removing /synthetic/root' }) as HTMLButtonElement;
  expect(confirm().disabled).toBe(false);

  await runEmbeddingAt(0); // a job takes the slot while the question is open
  await tick();

  expect(visibleText(screen.getByTestId('folders-remove-blocked'))).toBe('The Remove button works again once the scan is stopped.');
  expect(removeButton('/synthetic/root').disabled).toBe(true);
  expect(removeButton('/synthetic/other').disabled).toBe(true);
  expect(confirm().disabled).toBe(true);

  // This sentence's own switch, for the reason every other one on this screen
  // has its own: it is a separate `t()` call behind a separate `void $locale`.
  setLocale('uk');
  await tick();
  expect(visibleText(screen.getByTestId('folders-remove-blocked'))).toBe('Кнопка «Видалити» запрацює після зупинки сканування.');
  setLocale('en');
  await tick();

  // Ended is not running: the offer comes back, and so does the answer to the
  // question still on screen.
  await endScan();
  await waitFor(() => expect(screen.queryByTestId('folders-remove-blocked')).toBeNull());
  expect(removeButton('/synthetic/root').disabled).toBe(false);
  expect(confirm().disabled).toBe(false);

  // And so is idle, which is a different snapshot and a different row of the
  // same table.
  await runEmbeddingAt(0);
  await waitFor(() => expect(screen.getByTestId('folders-remove-blocked')).toBeTruthy());
  await goIdle();
  await waitFor(() => expect(screen.queryByTestId('folders-remove-blocked')).toBeNull());
  expect(removeButton('/synthetic/root').disabled).toBe(false);
});

// 🔴 The states separated: "a reading pass has ended since this question was
// asked" and "it has not". The question's `files` was read from a `list_tree`
// taken before that pass, exactly as the exclusion question's numbers are — so
// it goes the same way, by name, and not in silence.
test('the removal question is withdrawn when a reading pass ends, and says which folder it was about', async () => {
  const { jobs } = await showTwoRoots();

  await fireEvent.click(removeButton('/synthetic/root'));
  await screen.findByTestId('folder-remove-confirm-1');

  void jobs.scan('full');
  await endScan(); // a job ended; the reading counter did not move
  await tick();
  await tick();
  expect(screen.getByTestId('folder-remove-confirm-1')).toBeTruthy();
  expect(screen.queryByTestId('folders-remove-withdrawn')).toBeNull();

  await endScanAt(1);

  await waitFor(() => expect(screen.queryByTestId('folder-remove-confirm-1')).toBeNull());
  // 🔴 Fix round 1, m3. Its own sentence, and the clause that makes it its own
  // is the last one: the LIST was read again, not a panel. This question's row
  // is collapsed here, as it is in the common case, so a note claiming a panel
  // had been re-read named something nobody could see.
  expect(visibleText(screen.getByTestId('folders-remove-withdrawn'))).toBe(
    'The question about folder “/synthetic/root” has been withdrawn: indexing has finished and'
    + ' the list was read again. Press again if you still want to.',
  );
  // And the panel's own note is not on screen at all: two keys, two subjects.
  expect(screen.queryByTestId('folder-question-withdrawn-1')).toBeNull();
  expect(removeWatchedFolder).not.toHaveBeenCalled();

  // A `t()` call site of its own with its own `void $locale`, so it owes its
  // own switch — exactly as the panel's note did (`M1`, above).
  setLocale('uk');
  await tick();
  expect(visibleText(screen.getByTestId('folders-remove-withdrawn'))).toBe(
    'Питання про теку «/synthetic/root» знято: індексацію закінчено, і список перечитано.'
    + ' Натисніть ще раз, якщо це досі потрібно.',
  );
  setLocale('en');
  await tick();

  // The note stands until the next press, then goes: a note left beside a live
  // question about the same folder says two things at once.
  await fireEvent.click(removeButton('/synthetic/root'));
  await screen.findByTestId('folder-remove-confirm-1');
  expect(screen.queryByTestId('folders-remove-withdrawn')).toBeNull();
});

// The states separated: "the question is this person's language" and "the
// numbers in it are the ones they were shown". The words are rebuilt on a
// switch; the count is not re-derived, so it cannot move underneath a sentence
// somebody is part way through reading.
test('the removal question switches language and keeps the number it was asked with', async () => {
  await showTwoRoots();

  await fireEvent.click(removeButton('/synthetic/root'));
  expect(visibleText(await screen.findByTestId('folder-remove-confirm-1')))
    .toBe([REMOVE_QUESTION_EN, 'Confirm', 'Cancel'].join(' '));

  setLocale('uk');
  await tick();

  expect(visibleText(screen.getByTestId('folder-remove-confirm-1'))).toBe([
    'Видалити теку /synthetic/root з індексу? 3 файли цієї теки зникнуть з пошуку.',
    'Підтвердити', 'Скасувати',
  ].join(' '));
  expect(screen.getByRole('button', { name: 'Підтвердити видалення /synthetic/root' })).toBeTruthy();
});

// The states separated: "the last press was this removal" and "the person has
// moved on to something else". A question left standing under a press about a
// different thing is a question whose subject the screen no longer shows.
test('a press elsewhere in this list abandons the removal question', async () => {
  // Fix round 1, m6: the panel carries BOTH controls, so all three of the
  // presses that call `abandonRemoveQuestion` are driven here. Deleting the
  // call at any one site now kills this test.
  listSubfolders.mockResolvedValue(subfolders([sub('drop'), sub('Archive', { kind: 'excluded' })]));
  listExclusions.mockResolvedValue([{ prefix: 'Archive', existsOnDisk: true }]);
  await showTwoRoots();

  await fireEvent.click(removeButton('/synthetic/root'));
  await screen.findByTestId('folder-remove-confirm-1');
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));

  await waitFor(() => expect(screen.queryByTestId('folder-remove-confirm-1')).toBeNull());
  expect(removeWatchedFolder).not.toHaveBeenCalled();

  // 🔴 And the include press, which is a different function with its own copy
  // of the call — `askInclude`, the one that takes a person's rule away. An
  // exclude fixture alone leaves that site's deletion silent.
  await fireEvent.click(removeButton('/synthetic/root'));
  await screen.findByTestId('folder-remove-confirm-1');

  await fireEvent.click(screen.getByRole('button', { name: 'Remove the rule on Archive' }));

  await waitFor(() => expect(screen.queryByTestId('folder-remove-confirm-1')).toBeNull());
  expect(removeWatchedFolder).not.toHaveBeenCalled();
  expect(includeSubfolder).not.toHaveBeenCalled(); // the question was abandoned, not answered

  // And an add, which is the other press this list carries.
  await fireEvent.click(removeButton('/synthetic/other'));
  await screen.findByTestId('folder-remove-confirm-2');
  open.mockResolvedValue(null); // the dialog is cancelled: nothing else moves
  await fireEvent.click(screen.getByRole('button', { name: 'Add a folder' }));

  await waitFor(() => expect(screen.queryByTestId('folder-remove-confirm-2')).toBeNull());
  expect(removeWatchedFolder).not.toHaveBeenCalled();
});

// 🔴 Fix round 1, m2. The states separated: "there is a disabled «Remove» on
// screen" and "there is not". The sentence explains why every «Remove» is
// refused, so over an empty list it names a control the person cannot see —
// and over a `loadError` it does the same, because that branch replaces the
// whole list with its own two lines while `roots` still holds what was read
// before it.
test('the blocked sentence is drawn only where there is a «Remove» for it to explain', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([]));
  renderWatching();
  await waitFor(() => expect(screen.getByText('No folder has been added yet.')).toBeTruthy());

  await runEmbeddingAt(0);
  await tick();

  expect(screen.queryByTestId('folders-remove-blocked')).toBeNull();

  // One root, the same running job: now there is a button, and the sentence
  // says why it cannot be pressed.
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/root' })]));
  await endScan(); // re-reads the list; the counter does not move
  await waitFor(() => expect(screen.getByText('/synthetic/root')).toBeTruthy());
  await runEmbeddingAt(0);
  await tick();

  expect(visibleText(screen.getByTestId('folders-remove-blocked'))).toBe('The Remove button works again once the scan is stopped.');
  expect(removeButton('/synthetic/root').disabled).toBe(true);

  // And the third state, which `roots.length` alone cannot tell from the
  // second: the list is unreadable, so it is off the screen entirely while
  // `roots` still remembers the row — `refresh` throws before it assigns.
  //
  // 🔴 The failure has to be taken while the job is NOT running and the run
  // resumed afterwards, because only an ending re-reads: a rejection delivered
  // during the run would leave the snapshot `ended` and take the sentence off
  // the screen for the wrong reason, and this arm would pass against the very
  // guard it is here to pin.
  listTree.mockRejectedValueOnce(new Error('The index is not open yet.'));
  await endScan();
  await waitFor(() => expect(screen.getByText('The list of folders could not be read.')).toBeTruthy());
  await runEmbeddingAt(0);
  await tick();

  expect(screen.getByText('The list of folders could not be read.')).toBeTruthy(); // still off screen
  expect(screen.queryByTestId('folders-remove-blocked')).toBeNull();
});

// 🔴 Fix round 1, m5. The arms the component itself renders, rather than the
// arms `t()` renders when a test calls it directly: the fixtures elsewhere in
// this file reach `other` (en 3, en 5) and `few` (uk 3) only, so the singular
// was never drawn in either language and Ukrainian `many` never at all. A
// `files` handed to `t()` as a constant, or dropped, shows up here.
test('the rendered removal question reaches the singular and the Ukrainian many arm, in both locales', async () => {
  await showTwoRoots([
    root({ rootId: 1, absolutePath: '/synthetic/one', files: [file('a.md', 'doc-a')] }),
    root({
      rootId: 2,
      absolutePath: '/synthetic/many',
      files: [
        file('p.md', 'doc-p'), file('q.md', 'doc-q'), file('r.md', 'doc-r'),
        file('s.md', 'doc-s'), file('t.md', 'doc-t'),
      ],
    }),
  ], '/synthetic/one');

  await fireEvent.click(removeButton('/synthetic/one'));

  expect(visibleText(await screen.findByTestId('folder-remove-confirm-1'))).toBe([
    'Remove folder /synthetic/one from the index? 1 file from this folder will disappear from search.',
    'Confirm', 'Cancel',
  ].join(' '));

  setLocale('uk');
  await tick();
  expect(visibleText(screen.getByTestId('folder-remove-confirm-1'))).toBe([
    'Видалити теку /synthetic/one з індексу? 1 файл цієї теки зникне з пошуку.',
    'Підтвердити', 'Скасувати',
  ].join(' '));

  // The `many` arm, which no rendered question has reached before: 5 is not 5
  // of anything the other fixtures count.
  await fireEvent.click(screen.getByRole('button', { name: 'Видалити /synthetic/many' }));
  expect(visibleText(await screen.findByTestId('folder-remove-confirm-2'))).toBe([
    'Видалити теку /synthetic/many з індексу? 5 файлів цієї теки зникнуть з пошуку.',
    'Підтвердити', 'Скасувати',
  ].join(' '));

  setLocale('en');
  await tick();
  expect(visibleText(screen.getByTestId('folder-remove-confirm-2'))).toBe([
    'Remove folder /synthetic/many from the index? 5 files from this folder will disappear from search.',
    'Confirm', 'Cancel',
  ].join(' '));
});

// 🔴 Fix round 1, m8. The states separated: this window was opened AFTER a
// reading pass had already ended, and this window saw that pass end. `readSeq`
// is a count for the whole process and never resets, so a component that seeded
// its own counter with 0 would read the first snapshot it ever receives as a
// pass that ended under a question it never saw — and withdraw a question raised
// seconds ago on numbers that are perfectly current.
//
// The pair is the seeding itself: from the store (this test passes) versus from
// zero (this test fails, and nothing else in the file does).
test('a window opened after a reading pass has already ended does not withdraw on the next ending', async () => {
  const { jobs } = await renderAfterAReading();
  await fireEvent.click(screen.getByTestId('folder-expand-1'));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Exclude drop' })).toBeTruthy());
  await fireEvent.click(screen.getByRole('button', { name: 'Exclude drop' }));
  await screen.findByTestId('folder-confirm-1');

  // The job whose reading pass ended BEFORE this component existed now ends.
  // Its counter is where the store already had it, so nothing about the
  // question's numbers has moved since they were read.
  void jobs.scan('full');
  const readsBefore = listTree.mock.calls.length;
  await endScanAt(1);

  await waitFor(() => expect(listTree.mock.calls.length).toBe(readsBefore + 1));
  expect(screen.getByTestId('folder-confirm-1')).toBeTruthy();
  expect(screen.queryByTestId('folder-question-withdrawn-1')).toBeNull();
  expect(excludeSubfolder).not.toHaveBeenCalled();
});

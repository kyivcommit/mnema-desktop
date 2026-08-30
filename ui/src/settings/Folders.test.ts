import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import Folders from './Folders.svelte';
import { setLocale, t } from '../i18n';
import { createJobController } from './jobs';
import { tick } from 'svelte';
import type {
  StoredExclusion, Subfolder, SubfolderListing, SubfolderState, TreeFile, TreeListing, TreeRoot,
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
// the `'start_walk_job'` wire string this file asserts is never sent, and a
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

// this ruling’s own guard (P3-6 review), and it is a live one now: Task 8 gave this
// component a controller that CAN start a walk, and `ipc.ts` exports
// `startWalkJob`. The assertion still sits at the one boundary every command
// crosses regardless of what the wrapper is called — the raw `invoke` and the
// wire string `'start_walk_job'` it carries — because a wrapper renamed in a
// later task must not quietly retire the guard. `../lib/ipc`'s own real module
// imports `invoke` from this path, so mocking it here intercepts every call the
// real job wrappers would make.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...a: unknown[]) => invoke(...a),
  Channel: class {},
}));

beforeEach(() => {
  listTree.mockReset();
  addWatchedFolder.mockReset();
  removeWatchedFolder.mockReset();
  invoke.mockReset();
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
  // around it is named (P3-6 review — the previous form, `startWalkJob`,
  // named an export that does not exist and could never fail).
  expect(invoke.mock.calls.some(([command]) => command === 'start_walk_job')).toBe(false);
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

  expect(removeWatchedFolder).toHaveBeenCalledWith(9);
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
// `removeFolder`, checked separately below.
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

  await waitFor(() => expect(screen.queryByText('/synthetic/beta')).toBeNull());
  // A watched root's id can be handed out again by SQLite after the row is
  // deleted, and an expansion left behind under that id would draw the old
  // folder's subfolders under a new one.
  expect(screen.queryByText('OnlyUnderBeta')).toBeNull();
  expect(screen.getByTestId('folder-expand-3').getAttribute('aria-expanded')).toBe('false');
});

// 🔴 Review finding I2. This case is `patch`'s early return
// (`Folders.svelte:86`) and nothing else, and the shape of it was measured
// three times before it held.
//
// The write it pins is the one `exclude` makes AFTER the person has shut the
// row: `await read(rootId, want)` on line `Folders.svelte:190` runs behind the
// action, raises the generation itself, and so passes `read`'s own check
// (`:127`) — the counter cannot stand in for the early return here, because
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
  // Folders.svelte:755-756, outside `rows`.
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
    'Subfolders', 'Scan', 'Remove',
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

// Task 7. `confirmView` (Folders.svelte:682-707) builds the heading, the cost
// sentence, and both aria-labels for the INCLUDE branch inside the same
// `void $locale` block the exclude branch above already proves reactive — but
// each branch is its own ternary arm, and a literal swapped in for one of
// THESE calls specifically would not be caught by a switch test that only ever
// reaches the exclude arm. Two states, `existsOnDisk` true here and false in
// the test below, cover both halves of the cost ternary (Folders.svelte:693-698).
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
    'Підтвердити Скасувати',
  ].join(' '));
  // The two aria-labels `visibleText` cannot see: `confirmAriaLabel` and
  // `cancelAriaLabel` are the same `void $locale` block, but each is its own
  // `t()` call (Folders.svelte:700-705) and neither is a text node.
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

  // Task 7. `checkingLabel` (Folders.svelte:684) is its own branch of
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

  render(Folders, { props: { jobs: createJobController() } });
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

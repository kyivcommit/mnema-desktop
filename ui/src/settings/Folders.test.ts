import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import Folders from './Folders.svelte';
import { setLocale, t } from '../i18n';
import { createJobController } from './jobs';
import type { TreeListing, TreeRoot } from '../lib/ipc';

// Mocked in the shape Arms.test.ts:5-6 / Models.test.ts:13-30 already use —
// the typed wrappers, not the raw `invoke`.
const listTree = vi.fn();
const addWatchedFolder = vi.fn();
const removeWatchedFolder = vi.fn();
// The job commands are the REAL wrappers, deliberately: they are what carry
// the `'start_walk_job'` wire string this file asserts is never sent, and a
// mock of them would make that assertion about this file's own fake.
vi.mock('../lib/ipc', async (real) => ({
  ...(await real<Record<string, unknown>>()),
  listTree: (...a: unknown[]) => listTree(...a),
  addWatchedFolder: (...a: unknown[]) => addWatchedFolder(...a),
  removeWatchedFolder: (...a: unknown[]) => removeWatchedFolder(...a),
}));

// The dialog plugin needs its own mock — a separate module from `../lib/ipc`.
const open = vi.fn();
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: (...a: unknown[]) => open(...a),
}));

// D-c's own guard (P3-6 review), and it is a live one now: Task 8 gave this
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
  // D-c: adding a folder starts nothing. No assertion about the list would
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

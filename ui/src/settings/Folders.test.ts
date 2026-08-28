import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import Folders from './Folders.svelte';
import { setLocale, t } from '../i18n';
import type { TreeListing, TreeRoot } from '../lib/ipc';

// Mocked in the shape Arms.test.ts:5-6 / Models.test.ts:13-30 already use —
// the typed wrappers, not the raw `invoke`. `startWalkJob` is not imported by
// this component at all (Task 8 adds the scan control) — it is mocked here
// anyway so a regression that wires a scan into `add` would be caught at
// THIS file's level, not only by the absence of an import.
const listTree = vi.fn();
const addWatchedFolder = vi.fn();
const removeWatchedFolder = vi.fn();
const startWalkJob = vi.fn();
vi.mock('../lib/ipc', () => ({
  listTree: (...a: unknown[]) => listTree(...a),
  addWatchedFolder: (...a: unknown[]) => addWatchedFolder(...a),
  removeWatchedFolder: (...a: unknown[]) => removeWatchedFolder(...a),
  startWalkJob: (...a: unknown[]) => startWalkJob(...a),
}));

// The dialog plugin needs its own mock — a separate module from `../lib/ipc`.
const open = vi.fn();
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: (...a: unknown[]) => open(...a),
}));

beforeEach(() => {
  listTree.mockReset();
  addWatchedFolder.mockReset();
  removeWatchedFolder.mockReset();
  startWalkJob.mockReset();
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
  render(Folders);

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

  render(Folders);
  await waitFor(() => expect(screen.getByText('No folder has been added yet.')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Add a folder' }));

  await waitFor(() => expect(screen.getByText('/synthetic/reports')).toBeTruthy());
  expect(addWatchedFolder).toHaveBeenCalledWith('/synthetic/reports');
  // Re-read, not a locally patched list: the second listTree call is what the
  // fixture above returns, and its shape (rootId 7) is what the row must show.
  expect(listTree).toHaveBeenCalledTimes(2);
  // D-c: adding a folder starts nothing. No assertion about the list would
  // notice a stray scan — this is the one that would.
  expect(startWalkJob).not.toHaveBeenCalled();
});

test('a cancelled folder dialog calls nothing', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([]));
  open.mockResolvedValue(null);

  render(Folders);
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

  render(Folders);
  await waitFor(() => expect(screen.getByText('/synthetic/beta')).toBeTruthy());

  // The SECOND row is removed, not the first — a positional implementation
  // (always the 0th root) would call removeWatchedFolder(3) here instead.
  await fireEvent.click(within(screen.getByTestId('folder-row-9')).getByRole('button', { name: 'Remove' }));

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

  render(Folders);
  await waitFor(() => expect(screen.getByText('/synthetic/many')).toBeTruthy());

  // Computed through the same catalogue message the launcher tree uses, not
  // a hand-duplicated literal — a duplicated form is the "two truths, one
  // message" trap this project already paid for.
  expect(screen.getByText(t('indexed_documents', { count: 3 }))).toBeTruthy();
  expect(screen.getByText(t('indexed_documents', { count: 0 }))).toBeTruthy();
});

test('a rejected add shows the backend sentence verbatim, and the list keeps its prior state', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([]));
  open.mockResolvedValue('/synthetic/locked');
  addWatchedFolder.mockRejectedValue(new Error('This path is already watched.'));

  render(Folders);
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

  render(Folders);
  await waitFor(() => expect(screen.getByText('/synthetic/stuck')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Remove' }));

  await waitFor(() => expect(screen.getByText('The index is busy right now.')).toBeTruthy());
  expect(screen.getByText('/synthetic/stuck')).toBeTruthy(); // still there
});

test('a failed initial read shows the lead-in sentence and the backend sentence beside it', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockRejectedValue(new Error('The index is not open yet.'));

  render(Folders);

  await waitFor(() => expect(screen.getByText('The list of folders could not be read.')).toBeTruthy());
  expect(screen.getByText('The index is not open yet.')).toBeTruthy();
});

test('rows show the absolute path, not the launcher\'s relative view', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([root({ rootId: 1, absolutePath: '/synthetic/deep/nested/path', name: 'path', files: [] })]));
  render(Folders);
  await waitFor(() => expect(screen.getByText('/synthetic/deep/nested/path')).toBeTruthy());
});

test('labels and the per-row count stay correct across a language switch after mount', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([
    root({ rootId: 1, absolutePath: '/synthetic/only', files: [{ relativePath: 'a.md', documentId: 'd1' }] }),
  ]));
  render(Folders);
  await waitFor(() => expect(screen.getByText('/synthetic/only')).toBeTruthy());
  // Read once under 'en' BEFORE switching, so a $derived missing `void
  // $locale` still caches an English value here — the mutant only dies if the
  // read after the switch is a genuinely later one (Settings.test.ts:88-89's
  // own reasoning, reused here).
  expect(screen.getByText(t('indexed_documents', { count: 1 }))).toBeTruthy();

  setLocale('uk');
  await Promise.resolve();

  expect(screen.getByRole('button', { name: 'Додати теку' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Видалити' })).toBeTruthy();
  expect(screen.getByText(t('indexed_documents', { count: 1 }))).toBeTruthy();
  expect(screen.getByText('1 документ')).toBeTruthy();
});

test('the load-failure sentence stays correct across a language switch after mount', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockRejectedValue(new Error('boom'));
  render(Folders);
  await waitFor(() => expect(screen.getByText('The list of folders could not be read.')).toBeTruthy());

  setLocale('uk');
  await Promise.resolve();

  expect(screen.getByText('Не вдалося прочитати список тек.')).toBeTruthy();
});

test('a language switch reaches the empty-state sentence too', async () => {
  setLocale('en'); // seed, do not inherit
  listTree.mockResolvedValue(listing([]));
  render(Folders);
  await waitFor(() => expect(screen.getByText('No folder has been added yet.')).toBeTruthy());

  setLocale('uk');
  await Promise.resolve();

  expect(screen.getByText('Ще жодної теки не додано.')).toBeTruthy();
});

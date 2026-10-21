import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import Tree, { buildFolderTree } from './Tree.svelte';
import {
  oneRoot,
  oneRootTwoFolders,
  oneRootMixedDepths,
  twoRootsSameRelativePath,
  oneDocumentTwoRoots,
  emptyListing,
} from '../lib/fixtures';
import { setLocale } from '../i18n';
import type { TreeListing } from '../lib/ipc';

// The house pattern for the bridge (`lib/ipc.test.ts:14-15`): one shared spy,
// the module mocked once at file scope.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

// The card's one read. A reply is answered by its own command name: one reply for
// every command would hand the listing's shape to whatever else asked, and a
// test would pass on whichever shape happened to be wrong.
let treeReply: () => Promise<TreeListing> = () => Promise.resolve(emptyListing);
let setReply: (a: unknown) => Promise<void> = () => Promise.resolve();
function route(cmd: string, args?: unknown): Promise<unknown> {
  if (cmd === 'list_tree') return treeReply();
  if (cmd === 'set_folder_searched') return setReply(args);
  throw new Error(`unexpected command ${cmd}`);
}

// Ruling L: `mockTree` is a mock, not a fixture — it stays local to this file.
function mockTree(listing: TreeListing) {
  treeReply = () => Promise.resolve(listing);
}
function mockTreeFailure() {
  treeReply = () => Promise.reject(new Error('list_tree failed'));
}
// A listing that has NOT arrived yet, so a second request can be attempted
// while the first is still on the wire (the launcher's own reads keep the same shape).
function mockTreePending() {
  let resolve!: (listing: TreeListing) => void;
  const pending = new Promise<TreeListing>((r) => (resolve = r));
  treeReply = () => pending;
  return { resolve: (l: TreeListing) => resolve(l) };
}
function treeCalls() {
  return invoke.mock.calls.filter((c) => c[0] === 'list_tree');
}

// The same root as `oneRootTwoFolders` with one more file in `archive/` — what
// an index change looks like from this card. Local, like `mockTree` (Ruling L).
function withNewFile(): TreeListing {
  const [root] = oneRootTwoFolders.roots;
  return {
    roots: [{ ...root, files: [...root.files, { relativePath: 'archive/new.md', documentId: 'doc-new' }] }],
  };
}

// The selection is the file's place on disk — a `{ rootId, relativePath }`
// pair (spec §3) — and a click hands that pair plus the document id to the
// caller. Paths are written out rather than looked up by document id: a
// selection that stops naming a row has to be visible in the test.
const SELECTED = { rootId: 1, relativePath: 'notes/a.md' };
const SELECTED_B = { rootId: 1, relativePath: 'notes/b.md' };
const pick = vi.fn();
function mount(selected: { rootId: number; relativePath: string } | null) {
  return render(Tree, { selected, onPick: pick });
}

// Everything in the card that a keyboard can reach or that claims a role, in
// document order. Used as a closed enumeration (P5) rather than a spot check.
function controls() {
  return [...screen.getByTestId('tree-body').querySelectorAll('button, input, [tabindex], [role]')]
    .map((el) => el.getAttribute('data-testid'));
}

// Every row currently marked as the selection, in document order. Since P5 the
// rows are not `treeitem`s, so `getAllByRole('treeitem', { current: true })` is
// no longer the way to ask — and this form is a full enumeration, where the
// role query only ever counted the rows it happened to match.
function currentRows() {
  return [...screen.getByTestId('tree-body').querySelectorAll('[aria-current="true"]')]
    .map((el) => el.getAttribute('data-testid'));
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation(route);
  treeReply = () => Promise.resolve(emptyListing);
  setReply = () => Promise.resolve();
  pick.mockReset();
});

// Mirrors Answer.test.ts: `locale` is a module-level store
// shared by every test in this file, and an in-test restore is skipped when an
// assertion fails first. Restore unconditionally.
afterEach(() => {
  // `locale` is a module-level store shared by every test in this file, and an
  // in-test restore is skipped when an assertion fails first. Restore
  // unconditionally.
  setLocale('en');
  vi.restoreAllMocks();
});

// --- the pure folder builder ------------------------------------------------
// Depth is what the nesting branches on, and a render can only ever show one
// depth at a time cheaply. These pin all three shapes directly.

test('buildFolderTree leaves a file with no slash at the top level, in no folder', () => {
  expect(buildFolderTree([{ relativePath: 'README.md', documentId: 'doc-r' }])).toEqual([
    { kind: 'file', name: 'README.md', path: 'README.md', documentId: 'doc-r' },
  ]);
});

test('buildFolderTree nests a folder inside a folder for a path two levels deep', () => {
  expect(buildFolderTree([{ relativePath: 'a/b/c.md', documentId: 'doc-c' }])).toEqual([
    {
      kind: 'folder',
      name: 'a',
      path: 'a',
      children: [
        {
          kind: 'folder',
          name: 'b',
          path: 'a/b',
          children: [{ kind: 'file', name: 'c.md', path: 'a/b/c.md', documentId: 'doc-c' }],
        },
      ],
    },
  ]);
});

test('buildFolderTree mixes the three depths in one root and shares one node per folder', () => {
  // Both directions: `a/` appears once holding two children, and `README.md`
  // is NOT swept into it.
  expect(buildFolderTree(oneRootMixedDepths.roots[0].files)).toEqual([
    { kind: 'file', name: 'README.md', path: 'README.md', documentId: 'doc-r' },
    {
      kind: 'folder',
      name: 'a',
      path: 'a',
      children: [
        {
          kind: 'folder',
          name: 'b',
          path: 'a/b',
          children: [{ kind: 'file', name: 'c.md', path: 'a/b/c.md', documentId: 'doc-c' }],
        },
        { kind: 'file', name: 'd.md', path: 'a/d.md', documentId: 'doc-d' },
      ],
    },
  ]);
});

// --- the rendered card ------------------------------------------------------

// File rows are actions now — clicking one opens the file in the pane (spec
// §2) — so the closed enumeration below counts them. The root's disclosure row
// comes first (document order), then the folders, then the files of the folder
// the selection opened.
test('the controls are exactly the ones with a real action', async () => {
  mockTree(oneRootTwoFolders);
  mount(SELECTED); // notes/ is open, so its files are on screen

  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  expect(controls()).toEqual([
    'tree-root-toggle-1', 'tree-check-1-',
    'tree-folder-notes', 'tree-check-1-notes', 'tree-file-doc-1', 'tree-file-doc-2',
    'tree-folder-archive', 'tree-check-1-archive',
  ]);
});

// The row's whole job: hand the pair and the document id to the caller, so the
// pane opens the file at the path that was pressed — one document can sit at
// several paths, and only one of them was clicked.
test('a file row hands the caller its (root, path) pair and document id', async () => {
  mockTree(oneRootTwoFolders);
  mount(SELECTED);

  await fireEvent.click(await screen.findByTestId('tree-file-doc-2'));
  expect(pick).toHaveBeenCalledTimes(1);
  expect(pick).toHaveBeenLastCalledWith({ rootId: 1, relativePath: 'notes/b.md', documentId: 'doc-2' });
});

test('the tree nests files under their folders instead of listing flat paths', async () => {
  mockTree(oneRootTwoFolders);
  mount(null);

  expect(await screen.findByTestId('tree-folder-notes')).toBeTruthy();
  expect(screen.getByTestId('tree-folder-archive')).toBeTruthy();
  // Both directions: the flat path is never a row of its own, before or after
  // the folder is opened, and the bare file name is what appears inside it.
  expect(screen.queryByText('notes/a.md')).toBeNull();
  expect(screen.queryByText('a.md')).toBeNull(); // still shut

  await fireEvent.click(screen.getByTestId('tree-folder-notes'));
  expect(screen.getByText('a.md')).toBeTruthy();
  expect(screen.queryByText('notes/a.md')).toBeNull();
});

// Owner, 2026-10-20: the root's branch list is a sibling of the root row, not a
// child of its <li> — nested there, level-1 folders picked up the `ul ul` indent
// meant for level 2 and deeper (`launcher.css`).
test('the root branch is not nested inside the root row', async () => {
  mockTree(oneRootMixedDepths);
  mount(null);

  const top = (await screen.findByTestId('tree-folder-a')).closest('ul')!;
  expect(top.parentElement!.tagName).toBe('SECTION');
  // Level 2 still nests inside its folder's <li>: that indent is the one that
  // tells the depths apart.
  await fireEvent.click(screen.getByTestId('tree-folder-a'));
  const nested = screen.getByTestId('tree-folder-a/b').closest('ul')!;
  expect(nested.parentElement!.tagName).toBe('LI');
});

test('a non-ASCII folder name survives the split and its own test id', async () => {
  // Ruling K: this fixture is local because `lib/fixtures.ts` is read by the
  // Cyrillic guard (`i18n/guard.test.ts:19-21`) and a `.test.ts` is not.
  mockTree({
    roots: [
      {
        rootId: 9,
        absolutePath: '/home/u/ua', searched: [],
        name: 'ua',
        files: [{ relativePath: 'Договори/dohov-01.md', documentId: 'doc-u' }],
      },
    ],
  });
  mount(null);

  expect(await screen.findByTestId('tree-folder-Договори')).toBeTruthy();
  expect(screen.queryByText('Договори/dohov-01.md')).toBeNull();
});

test('the folder holding the selected file is open; the others are not', async () => {
  mockTree(oneRootTwoFolders);
  mount(SELECTED); // notes/a.md

  // Ruling I: `toBeVisible()` is not available in this project (no
  // `@testing-library/jest-dom`), and the claim lives in the second line
  // anyway — the sibling folder's file is not rendered at all.
  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  expect(screen.getByTestId('tree-file-doc-2')).toBeTruthy(); // its sibling in the SAME folder
  expect(screen.queryByTestId('tree-file-doc-3')).toBeNull(); // inside the collapsed sibling folder
  expect(screen.getByTestId('tree-folder-archive')).toBeTruthy(); // the shut folder is still listed
});

// The open/shut state is keyed `${rootId}\0${path}`, not by path alone, so two
// roots holding a folder of the same name cannot share one flag. That claim had
// no test: the report asserted the component was correct here and only the test
// id collided, and half of that was unmeasured. The collision is real and shows
// up in the first line — `getAllByTestId`, because `getByTestId` throws on two
// matches.
test('two roots holding a folder of the same name open and shut independently', async () => {
  // Local, like `mockTree` (Ruling L): nothing else consumes this shape.
  mockTree({
    roots: [
      { rootId: 1, absolutePath: '/home/u/one', searched: [], name: 'one', files: [{ relativePath: 'notes/x.md', documentId: 'doc-one' }] },
      { rootId: 2, absolutePath: '/home/u/two', searched: [], name: 'two', files: [{ relativePath: 'notes/y.md', documentId: 'doc-two' }] },
    ],
  });
  mount(null);

  expect(await screen.findAllByTestId('tree-folder-notes')).toHaveLength(2);
  expect(screen.queryByTestId('tree-file-doc-one')).toBeNull();
  expect(screen.queryByTestId('tree-file-doc-two')).toBeNull();

  // Opening the first root's `notes/` must not open the second root's.
  await fireEvent.click(screen.getAllByTestId('tree-folder-notes')[0]);
  expect(screen.getByTestId('tree-file-doc-one')).toBeTruthy();
  expect(screen.queryByTestId('tree-file-doc-two')).toBeNull();

  await fireEvent.click(screen.getAllByTestId('tree-folder-notes')[1]);
  expect(screen.getByTestId('tree-file-doc-one')).toBeTruthy();
  expect(screen.getByTestId('tree-file-doc-two')).toBeTruthy();

  // And shutting one must not shut the other — the direction a shared flag
  // would still get right by accident if only the opening half were asserted.
  await fireEvent.click(screen.getAllByTestId('tree-folder-notes')[0]);
  expect(screen.queryByTestId('tree-file-doc-one')).toBeNull();
  expect(screen.getByTestId('tree-file-doc-two')).toBeTruthy();
});

test('two roots holding the same path mark only the root that was opened', async () => {
  mockTree(twoRootsSameRelativePath); // two roots, both holding README.md
  const { rerender } = mount(null);
  expect(await screen.findByTestId('tree-file-doc-a')).toBeTruthy();

  // Both roots hold 'README.md'; the pair says which one was opened. A key on
  // the path string alone would mark both rows, and the doc-a assertion below
  // is what catches it — a path that matches nothing would only prove that a
  // wrong lookup finds nothing.
  await rerender({ selected: { rootId: 2, relativePath: 'README.md' } });
  expect(currentRows()).toEqual(['tree-file-doc-b']);
  expect(screen.getByTestId('tree-file-doc-a').getAttribute('aria-current')).toBeNull();
});

// The decision, and its reason. `toggled` wins over `openByDefault` so that
// folders do not snap shut on every question — that is Ruling M's point and it
// stands. But "a file was opened" is a different event from "an answer
// arrived": it is the person's own click, and its whole purpose is to be shown
// in the tree. So a NEW selection clears the hand-toggle on the folders along
// its own path, and only on those.
//
// What is deliberately NOT undone: a person who shuts the folder of the file
// already open keeps it shut. They acted on a row they could see, no new event
// has happened since, and re-opening it under their hands would be the
// snapping-shut defect with the sign flipped.
test('a folder the person shut opens again when the next file lands inside it', async () => {
  mockTree(oneRootTwoFolders);
  const { rerender } = mount(SELECTED); // notes/ opens

  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  await fireEvent.click(screen.getByTestId('tree-folder-notes')); // shut by hand
  expect(screen.queryByTestId('tree-file-doc-1')).toBeNull();

  await rerender({ selected: SELECTED_B }); // a new citation, same folder
  await tick();

  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('true');
  expect(currentRows()).toEqual(['tree-file-doc-2']);
});

// Owner, 2026-10-20: the root itself is a disclosure row like the folders
// below it — a bare heading over a flat list read as a pile with no way to
// fold it. The default is EXPANDED; collapsing is the person's option.
test('the root is a disclosure row, expanded by default, folding the whole root', async () => {
  mockTree(oneRootTwoFolders);
  mount(SELECTED);

  const root = await screen.findByTestId('tree-root-toggle-1');
  expect(root.getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByTestId('tree-folder-notes')).toBeTruthy();

  await fireEvent.click(root);
  expect(root.getAttribute('aria-expanded')).toBe('false');
  expect(screen.queryByTestId('tree-folder-notes')).toBeNull();
  expect(screen.queryByTestId('tree-file-doc-1')).toBeNull();

  await fireEvent.click(root);
  expect(root.getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByTestId('tree-folder-notes')).toBeTruthy();
});

// The same rule the folders live by (P4): a NEW selection re-opens what the
// person shut on its way, and the root is the outermost folder on every path.
// The mark must be findable; a shut root would hide it.
test('a root the person shut opens again when the next file lands inside it', async () => {
  mockTree(oneRootTwoFolders);
  const { rerender } = mount(SELECTED);

  await fireEvent.click(await screen.findByTestId('tree-root-toggle-1')); // shut by hand
  expect(screen.queryByTestId('tree-file-doc-1')).toBeNull();

  await rerender({ selected: SELECTED_B }); // a new citation, same root
  await tick();

  expect(screen.getByTestId('tree-root-toggle-1').getAttribute('aria-expanded')).toBe('true');
  expect(currentRows()).toEqual(['tree-file-doc-2']);
});

// --- P3: the listing is not a mount-time snapshot ---------------------------
//
// 🔴 Owner review on PR #24, P3. `listTree()` ran on mount and never again, so
// a launcher that outlives an index change — and §7.3 keeps the window alive
// across a hide, so one launcher outlives many — kept showing rows that are no
// longer there and never showed the ones that are.
//
// The trigger is the window regaining focus, and it is chosen rather than
// invented: `Launcher.svelte:65` already treats window BLUR as "the person has
// left" and hides the launcher on it, so focus is the same signal in reverse —
// the launcher is in front of the person again, which is the moment its listing
// is about to be read and the only moment a stale row can mislead anyone. It
// costs nothing while the window is hidden, and it is not tied to the answer
// state, which is what Ruling M forbids for a reason that still holds.
//
// Ruling M is not overturned: the toggles are component state and this refresh
// does not touch them, which is what the second half of this test measures.
test('the tree refreshes when the launcher comes back, keeping the folders the person opened', async () => {
  mockTree(oneRootTwoFolders);
  mount(null);

  await fireEvent.click(await screen.findByTestId('tree-folder-archive')); // opened by hand
  expect(screen.getByTestId('tree-file-doc-3')).toBeTruthy();

  mockTree(withNewFile()); // the index changed while the launcher was away
  await fireEvent.focus(window);

  expect(await screen.findByTestId('tree-file-doc-new')).toBeTruthy();
  expect(treeCalls()).toHaveLength(2);
  // The folder is still open, and its old row is still under it: the refresh
  // replaced the listing, not the person's place in it.
  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByTestId('tree-file-doc-3')).toBeTruthy();
});

// 🔴 What disappears. A refresh is a second chance to fail, and the failure
// branch replaces the whole card with a message (Ruling N) — so a transient
// failure would take a listing that WORKS off the screen and leave the person
// with nothing, on an event they did not cause. The message belongs to a card
// that has nothing to show; a card that has something keeps showing it.
test('a refresh that fails leaves the listing that worked on screen', async () => {
  mockTree(oneRootTwoFolders);
  mount(null);
  expect(await screen.findByTestId('tree-folder-notes')).toBeTruthy();

  mockTreeFailure();
  await fireEvent.focus(window);
  await waitFor(() => expect(treeCalls()).toHaveLength(2));

  expect(screen.queryByTestId('tree-failed')).toBeNull();
  expect(screen.getByTestId('tree-folder-notes')).toBeTruthy();
  expect(screen.getByTestId('tree-folder-archive')).toBeTruthy();
});

// The mirror of the test above, and the line it defends would otherwise have
// nothing on it: a card that failed on mount and succeeds on the refresh has to
// stop saying it failed. Leaving the message beside a listing is the same
// class of defect as removing a listing for a message.
test('a refresh that succeeds after a failed mount replaces the message with the listing', async () => {
  mockTreeFailure();
  mount(null);
  expect(await screen.findByTestId('tree-failed')).toBeTruthy();

  mockTree(oneRootTwoFolders);
  await fireEvent.focus(window);

  expect(await screen.findByTestId('tree-folder-notes')).toBeTruthy();
  expect(screen.queryByTestId('tree-failed')).toBeNull();
});

// Two listings on the wire at once can land in either order, and the loser
// overwrites the winner — a card showing an older index than the one it already
// had. The second half is the control: the guard must let go, or the refresh
// above only ever happens once.
test('a launcher focused while the first listing is still on the wire does not ask twice', async () => {
  const pending = mockTreePending();
  mount(null);

  await fireEvent.focus(window);
  expect(treeCalls()).toHaveLength(1);

  mockTree(oneRootTwoFolders);
  pending.resolve(oneRootTwoFolders);
  expect(await screen.findByTestId('tree-folder-notes')).toBeTruthy();

  await fireEvent.focus(window);
  await waitFor(() => expect(treeCalls()).toHaveLength(2));
});

// Ruling M: Task 8b deliberately does not key this card, because `state`
// changes twice per question and a remounted tree would refetch and snap every
// folder the person opened shut. A change of `selected` must not re-invoke.
// P3 gives this card a refresh trigger, and it is deliberately not this one:
// `selected` changes twice per question and its own rule still holds.
test('list_tree does not run again when the selection changes', async () => {
  mockTree(twoRootsSameRelativePath);
  const { rerender } = mount(null);
  expect(await screen.findByTestId('tree-file-doc-a')).toBeTruthy();
  expect(treeCalls()).toHaveLength(1);
  expect(invoke).toHaveBeenCalledWith('list_tree');

  await rerender({ selected: { rootId: 2, relativePath: 'README.md' } });
  await tick();
  expect(treeCalls()).toHaveLength(1);
});

// Ruling J, the positive control. Without it the negative test below is
// satisfied by zero, and it is THIS one that must redden when the selection
// logic breaks.
test('a selection that names a row marks it current', async () => {
  mockTree(oneRoot);
  mount({ rootId: 3, relativePath: 'a.md' });

  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  expect(currentRows()).toEqual(['tree-file-doc-1']);
});

// §3's review focus: one document can be named from two watched roots
// (`mnema-index/src/write.rs:700-722`), so `tree-file-{documentId}` renders
// twice. The selection is the (root, path) pair, so exactly ONE of the two
// rows is marked — the one that was opened, not every row showing the id.
test('a document present under two roots marks only the row that was opened', async () => {
  mockTree(oneDocumentTwoRoots);
  const { rerender } = mount(null);
  // Both folders open, or only one of the two rows exists: the selection can
  // open only the root it names.
  await fireEvent.click((await screen.findAllByTestId('tree-folder-notes'))[0]);
  await rerender({ selected: { rootId: 2, relativePath: 'notes/shared.md' } });

  const rows = await screen.findAllByTestId('tree-file-doc-shared');
  expect(rows).toHaveLength(2); // one row per path, under alpha and under beta
  expect(rows.filter((r) => r.getAttribute('aria-current') === 'true')).toHaveLength(1);
  expect(currentRows()).toEqual(['tree-file-doc-shared']);

  // Both directions: the neighbouring document in the same open folder is not
  // swept up either.
  expect(screen.getByTestId('tree-file-doc-other').getAttribute('aria-current')).toBeNull();
});

test('a selection whose pair is not in the listing marks nothing and says nothing false', async () => {
  mockTree(oneRoot);
  mount({ rootId: 3, relativePath: 'gone.md' });

  // The row that IS in the listing is on screen either way — the difference
  // between this test and its control is `aria-current`, not an empty tree.
  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  expect(currentRows()).toEqual([]);
  expect(screen.getByTestId('tree-body').textContent).not.toMatch(/no longer|більше немає/i);
});

// --- P4: the selection must be visible in the tree --------------------------
//
// 🔴 Owner review on PR #24, P4. The invariant, in the owner's words: when the
// source card shows a passage, the tree card shows which row it came from. It
// broke in two places, both disclosed in the Task 7 report and neither fixed.
// Every test that existed asserted `aria-current` on a FILE row while the Files
// tab happened to be showing and no folder had been touched — the two states
// where the card was already right.

// The other side of the same rule: the stamp is the selection key plus the
// folders on its way, so the SAME pair selected again does not move it — the
// effect does not re-fire and the hand-shut folder stays shut. That is not a
// gap in the invariant: the mark names the file the pane is showing, and the
// file did not change.
test('the same selection again leaves the folder the person shut alone', async () => {
  mockTree(oneRootTwoFolders);
  const { rerender } = mount(SELECTED); // notes/ opens

  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  await fireEvent.click(screen.getByTestId('tree-folder-notes')); // shut by hand
  expect(screen.queryByTestId('tree-file-doc-1')).toBeNull();

  // The same file, handed over again as a fresh object.
  await rerender({ selected: { ...SELECTED } });
  await tick();

  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('false');
  expect(screen.queryByTestId('tree-file-doc-1')).toBeNull();
  // Both directions: the folder is still on screen, so this is not an empty card.
  expect(screen.getByTestId('tree-folder-archive')).toBeTruthy();
});

// The control, and the reason the clearing is scoped to the path: a folder the
// person opened somewhere else is none of the new selection's business. Without
// this, "clear all the toggles on a new selection" passes the test above and
// takes Ruling M's defect back.
test('a new selection leaves the folders the person opened elsewhere exactly as they were', async () => {
  mockTree(oneRootTwoFolders);
  const { rerender } = mount(null);

  await fireEvent.click(await screen.findByTestId('tree-folder-archive')); // opened by hand
  expect(screen.getByTestId('tree-file-doc-3')).toBeTruthy();

  await rerender({ selected: { ...SELECTED } }); // lands in notes/, not in archive/
  await tick();

  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByTestId('tree-file-doc-3')).toBeTruthy();
  expect(currentRows()).toEqual(['tree-file-doc-1']);
});

// A refresh returning the SAME listing leaves the shut folder shut: the stamp
// did not move, so the effect did not run.
test('a refresh that changes nothing leaves the folder the person shut alone', async () => {
  mockTree(oneRootTwoFolders);
  mount(SELECTED);

  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  await fireEvent.click(screen.getByTestId('tree-folder-notes')); // shut by hand

  await fireEvent.focus(window);
  await waitFor(() => expect(treeCalls()).toHaveLength(2));

  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('false');
  expect(screen.queryByTestId('tree-file-doc-1')).toBeNull();
});

// 🔴 The other direction of the same rule, and it is here because the first
// version of the fix broke it: clearing the hand-toggle on the selection's path
// made a folder the person had OPENED depend on the selection to stay open, so
// it snapped shut the moment the selection went away. The fix sets a shut
// folder open instead of forgetting what the person did, and this test says so.
test('a folder the person opened stays open after the selection goes away', async () => {
  mockTree(oneRootTwoFolders);
  const { rerender } = mount(null);

  await fireEvent.click(await screen.findByTestId('tree-folder-notes')); // opened by hand
  await rerender({ selected: { ...SELECTED } }); // a file there is opened
  await tick();
  expect(currentRows()).toEqual(['tree-file-doc-1']);

  await rerender({ selected: null }); // the pane closes or moves to a file elsewhere
  await tick();
  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByTestId('tree-file-doc-1')).toBeTruthy();
});

// The pair key changes one behaviour on purpose. A refresh that moves the
// selected file to a different path leaves the selection naming a pair the
// listing no longer holds, so nothing is marked — and because there is no
// folder chain on the new path, the hand-shut folder stays shut. Under the old
// document-id key the same refresh re-opened it and marked the moved row.
test('a refresh that moves the selected file selects nothing and re-opens nothing', async () => {
  mockTree(oneRootTwoFolders);
  mount(SELECTED); // notes/a.md — notes/ opens

  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  await fireEvent.click(screen.getByTestId('tree-folder-notes')); // shut by hand
  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('false');

  const [root] = oneRootTwoFolders.roots;
  mockTree({ // the same document id, one folder deeper
    roots: [{ ...root, files: [{ relativePath: 'notes/deep/a.md', documentId: 'doc-1' }] }],
  });
  await fireEvent.focus(window);
  await waitFor(() => expect(treeCalls()).toHaveLength(2));

  // The moved row is inside the folder still shut, and the selection names a
  // path the listing no longer holds.
  expect(screen.queryByTestId('tree-file-doc-1')).toBeNull();
  expect(currentRows()).toEqual([]);
  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('false');
});

// Ruling N: the plan's "never an empty card that looks like an empty index"
// only means something if the empty index also says something, and something
// different. Each must show its own message and not the other's.
test('an empty but successful listing says the index is empty, not that the listing failed', async () => {
  mockTree(emptyListing);
  mount(null);

  expect(await screen.findByTestId('tree-empty')).toBeTruthy();
  expect(screen.getByTestId('tree-empty').textContent).toBe('Nothing is indexed yet.');
  expect(screen.queryByTestId('tree-failed')).toBeNull();
});

test('a failed list_tree leaves a visible message, never an empty card that looks like an empty index', async () => {
  mockTreeFailure();
  mount(null);

  expect(await screen.findByTestId('tree-failed')).toBeTruthy();
  expect(screen.getByTestId('tree-failed').textContent).toBe('The tree could not be loaded.');
  expect(screen.queryByTestId('tree-empty')).toBeNull();
});

// D130: a bare `t()` in markup does not re-render on a language switch, and an
// English hardcode passes the Cyrillic guard silently. Both were review
// findings on Tasks 5 and 6.
test('the messages follow a live language switch', async () => {
  setLocale('en'); // seed, do not inherit: an earlier sibling switching the language must not decide this test
  mockTree(emptyListing);
  mount(null);

  expect(await screen.findByTestId('tree-empty')).toBeTruthy();
  expect(screen.getByTestId('tree-empty').textContent).toBe('Nothing is indexed yet.');

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('tree-empty').textContent).toBe('Ще нічого не проіндексовано.');

  setLocale('en'); // the switch back is part of the claim, not the cleanup — afterEach owns that
  await tick();
  expect(screen.getByTestId('tree-empty').textContent).toBe('Nothing is indexed yet.');
});

test('the failure message follows a live language switch too', async () => {
  setLocale('en'); // seed, do not inherit
  mockTreeFailure();
  mount(null);

  expect((await screen.findByTestId('tree-failed')).textContent).toBe('The tree could not be loaded.');

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('tree-failed').textContent).toBe('Не вдалося завантажити дерево.');
});

// Ruling N, the Files half. The Files tab is empty only when there are no roots:
// the questions do not count toward it, and the Recents tab does not borrow its
// message. Both directions are on screen in one state.
test('the tree says the index is empty only when it has no roots', async () => {
  setLocale('en'); // seed, do not inherit
  mockTree(emptyListing);
  mount(null);

  expect(await screen.findByTestId('tree-empty')).toBeTruthy();
  expect(screen.getByTestId('tree-empty').textContent).toBe('Nothing is indexed yet.');
  expect(screen.queryByTestId('tree-failed')).toBeNull();
});

// M3 (review round 1): `aria-expanded` is a second statement about the same
// state as "are the children rendered", and a second signal is free to say the
// opposite and stay green — the shape already caught once on `aria-selected`.
test('aria-expanded on a folder says what the folder is actually doing', async () => {
  mockTree(oneRootTwoFolders);
  mount(SELECTED); // notes/a.md — opens notes/

  // The auto-opened folder and its shut sibling, both directions, and each
  // cross-checked against whether the children are really there.
  expect((await screen.findByTestId('tree-folder-notes')).getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByTestId('tree-file-doc-1')).toBeTruthy();
  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('false');
  expect(screen.queryByTestId('tree-file-doc-3')).toBeNull();

  // And it tracks a click in both directions, not just the first one.
  await fireEvent.click(screen.getByTestId('tree-folder-archive'));
  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByTestId('tree-file-doc-3')).toBeTruthy();

  await fireEvent.click(screen.getByTestId('tree-folder-notes'));
  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('false');
  expect(screen.queryByTestId('tree-file-doc-1')).toBeNull();
});


// --- the search checks, persisted through the index -------------------------

const withNotesOff: TreeListing = {
  roots: [{ ...oneRootTwoFolders.roots[0], searched: [{ folderPath: 'notes', searched: false }] }],
};
const setCalls = () => invoke.mock.calls.filter((c) => c[0] === 'set_folder_searched');
const box = (path: string) => screen.getByTestId(`tree-check-1-${path}`) as HTMLInputElement;

test('the checks are read from the listing', async () => {
  mockTree(withNotesOff);
  mount(null);
  await screen.findByTestId('tree-check-1-notes');
  expect(box('notes').checked).toBe(false);
  expect(box('archive').checked).toBe(true);
  expect(box('').indeterminate).toBe(true);
});

test('a click on a mixed root ticks the whole branch and clears the entries under it', async () => {
  mockTree(withNotesOff);
  mount(null);
  await fireEvent.click(await screen.findByTestId('tree-check-1-'));
  expect(setCalls()).toEqual([['set_folder_searched', { rootId: 1, folderPath: '', searched: true }]]);
  for (const p of ['', 'notes', 'archive']) {
    expect(box(p).checked).toBe(true);
    expect(box(p).indeterminate).toBe(false);
  }
});

test('a file under an unticked folder is marked unsearched, one under a ticked folder is not', async () => {
  mockTree(withNotesOff);
  mount(null);
  await fireEvent.click(await screen.findByTestId('tree-folder-notes'));
  await fireEvent.click(screen.getByTestId('tree-folder-archive'));
  const off = screen.getByTestId('tree-file-doc-1');
  expect(off.classList.contains('unsearched')).toBe(true);
  expect(screen.getByTestId('tree-file-doc-3').classList.contains('unsearched')).toBe(false);
});

test('a click writes the new state once', async () => {
  mockTree(withNotesOff);
  mount(null);
  await fireEvent.click(await screen.findByTestId('tree-check-1-archive'));
  expect(setCalls()).toEqual([['set_folder_searched', { rootId: 1, folderPath: 'archive', searched: false }]]);
  expect(box('archive').checked).toBe(false);
});

test('writes run one after another', async () => {
  mockTree(withNotesOff);
  let release!: () => void;
  setReply = () => new Promise<void>((r) => (release = r));
  mount(null);
  await fireEvent.click(await screen.findByTestId('tree-check-1-archive'));
  await fireEvent.click(box('notes'));
  await tick();
  expect(setCalls()).toHaveLength(1); // the second waits for the first
  setReply = () => Promise.resolve();
  release();
  await waitFor(() => expect(setCalls()).toHaveLength(2));
  expect(setCalls()[1][1]).toEqual({ rootId: 1, folderPath: 'notes', searched: true });
});

// The index is the record: `stored` is what the mock backend holds, and a
// listing is read from it at the moment of the call.
function storeBackedIndex() {
  const stored: { folderPath: string; searched: boolean }[] = [{ folderPath: 'notes', searched: false }];
  treeReply = () => Promise.resolve({ roots: [{ ...oneRootTwoFolders.roots[0], searched: stored.map((e) => ({ ...e })) }] });
  return stored;
}

test('after a refused write and a later accepted one, the checks show what the index holds', async () => {
  const stored = storeBackedIndex();
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  const gates: Array<() => void> = [];
  setReply = (a) => new Promise<void>((resolve, reject) => {
    const { folderPath, searched } = a as { folderPath: string; searched: boolean };
    gates.push(gates.length === 0 ? () => reject(new Error('disk full')) : () => {
      const at = stored.findIndex((e) => e.folderPath === folderPath);
      if (at >= 0) stored.splice(at, 1);
      stored.push({ folderPath, searched });
      resolve();
    });
  });
  mount(null);
  await fireEvent.click(await screen.findByTestId('tree-check-1-archive')); // will be refused
  await fireEvent.click(box('notes')); // will be accepted
  gates[0]();
  await waitFor(() => expect(gates).toHaveLength(2));
  await waitFor(() => expect(err).toHaveBeenCalled());
  gates[1]();
  await waitFor(() => expect(box('archive').checked).toBe(true)); // the refused click is not shown
  expect(box('notes').checked).toBe(true); // the accepted one is
});

test('after a single refused write the check shows the stored value', async () => {
  storeBackedIndex();
  vi.spyOn(console, 'error').mockImplementation(() => {});
  setReply = () => Promise.reject(new Error('disk full'));
  mount(null);
  await fireEvent.click(await screen.findByTestId('tree-check-1-archive'));
  expect(box('archive').checked).toBe(false); // shown at click time
  await waitFor(() => expect(box('archive').checked).toBe(true));
  expect(console.error).toHaveBeenCalled();
});

// A listing read before a click carries the old state. Whenever it lands, the
// click stays and the index is read again.
function staleLoad(extraFile = false) {
  let land!: () => void;
  const root = oneRootTwoFolders.roots[0];
  const stale = { roots: [{ ...root, files: extraFile ? [...root.files, { relativePath: 'archive/new.md', documentId: 'doc-new' }] : root.files, searched: [{ folderPath: 'notes', searched: false }] }] };
  const fresh = treeReply;
  treeReply = () => { treeReply = fresh; return new Promise<TreeListing>((r) => (land = () => r(stale))); };
  return { land: () => land() };
}
function recordingWrite(stored: { folderPath: string; searched: boolean }[]) {
  let done!: () => void;
  setReply = (a) => new Promise<void>((resolve) => {
    const { folderPath, searched } = a as { folderPath: string; searched: boolean };
    done = () => { stored.push({ folderPath, searched }); resolve(); };
  });
  return { done: () => done() };
}

test('a stale listing landing while the write is pending does not revert it', async () => {
  const stored = storeBackedIndex();
  mount(null);
  await screen.findByTestId('tree-check-1-archive');
  const stale = staleLoad();
  await fireEvent.focus(window);
  await waitFor(() => expect(treeCalls()).toHaveLength(2));
  const write = recordingWrite(stored);
  await fireEvent.click(box('archive'));
  stale.land();
  await tick();
  expect(box('archive').checked).toBe(false);
  write.done();
  await waitFor(() => expect(treeCalls()).toHaveLength(3)); // read again after the drain
  await tick();
  expect(box('archive').checked).toBe(false); // the stored value, not the stale one
});

test('a stale listing landing after the write finished does not revert it', async () => {
  const stored = storeBackedIndex();
  mount(null);
  await screen.findByTestId('tree-check-1-archive');
  const stale = staleLoad();
  await fireEvent.focus(window);
  await waitFor(() => expect(treeCalls()).toHaveLength(2));
  const write = recordingWrite(stored);
  await fireEvent.click(box('archive'));
  write.done();
  await waitFor(() => expect(setCalls()).toHaveLength(1));
  await tick(); await tick();
  stale.land();
  await waitFor(() => expect(treeCalls()).toHaveLength(3));
  await tick();
  expect(box('archive').checked).toBe(false);
});

test('a load that starts while a write is pending and lands after the drain does not revert', async () => {
  const stored = storeBackedIndex();
  mount(null);
  await screen.findByTestId('tree-check-1-archive');
  const done: Array<() => void> = [];
  setReply = (a) => new Promise<void>((resolve) => {
    const { folderPath, searched } = a as { folderPath: string; searched: boolean };
    done.push(() => { stored.push({ folderPath, searched }); resolve(); });
  });
  await fireEvent.click(box('archive')); // write 1 in flight
  await fireEvent.click(box('notes')); // write 2 queued
  const stale = staleLoad();
  await fireEvent.focus(window);
  await waitFor(() => expect(treeCalls()).toHaveLength(2));
  done[0]();
  await waitFor(() => expect(done).toHaveLength(2));
  done[1]();
  await waitFor(() => expect(stored).toHaveLength(3));
  await tick(); await tick();
  stale.land(); // read before write 2 reached the store
  await waitFor(() => expect(treeCalls()).toHaveLength(3));
  await tick();
  expect(box('archive').checked).toBe(false);
  expect(box('notes').checked).toBe(true);
});

test('a discarded stale listing still updates the files', async () => {
  const stored = storeBackedIndex();
  mount(null);
  await fireEvent.click(await screen.findByTestId('tree-folder-archive'));
  const stale = staleLoad(true);
  await fireEvent.focus(window);
  await waitFor(() => expect(treeCalls()).toHaveLength(2));
  recordingWrite(stored);
  await fireEvent.click(box('archive')); // write stays pending
  stale.land();
  expect(await screen.findByTestId('tree-file-doc-new')).toBeTruthy();
});

import { render, screen, fireEvent, waitFor, within } from '@testing-library/svelte';
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
  generated,
  historyTwoQuestions,
} from '../lib/fixtures';
import { setLocale } from '../i18n';
import type { AskCitation, HistoryEntry, TreeListing } from '../lib/ipc';

// The house pattern for the bridge (`lib/ipc.test.ts:14-15`): one shared spy,
// the module mocked once at file scope.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

// The card makes two reads, `list_tree` and `list_history`, and each is answered
// by its own command name. One reply for every command would hand the listing's
// shape to the history reader and the other way round, and a test would pass on
// whichever shape happened to be wrong.
let treeReply: () => Promise<TreeListing> = () => Promise.resolve(emptyListing);
let history: HistoryEntry[] = [];
// Set by the failure tests only; reset before each test.
let failHistory = false;
let failDelete = false;
function route(cmd: string, args?: unknown): Promise<unknown> {
  if (cmd === 'list_tree') return treeReply();
  if (cmd === 'list_history') {
    return failHistory
      ? Promise.reject(new Error('list_history failed'))
      : Promise.resolve(history.map((h) => ({ ...h })));
  }
  if (cmd === 'delete_history_entry') {
    if (failDelete) return Promise.reject(new Error('delete_history_entry failed'));
    const { id } = args as { id: number };
    history = history.filter((h) => h.id !== id);
    return Promise.resolve();
  }
  // Answered rather than thrown: a stray `ask` must fail its own assertion, not
  // surface as an unhandled rejection that another test would be blamed for.
  if (cmd === 'ask') return Promise.resolve(generated);
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
// while the first is still on the wire (`Cards.test.ts` keeps the same shape).
function mockTreePending() {
  let resolve!: (listing: TreeListing) => void;
  const pending = new Promise<TreeListing>((r) => (resolve = r));
  treeReply = () => pending;
  return { resolve: (l: TreeListing) => resolve(l) };
}
function mockHistory(entries: HistoryEntry[]) {
  history = entries.map((e) => ({ ...e }));
}
function treeCalls() {
  return invoke.mock.calls.filter((c) => c[0] === 'list_tree');
}
function historyCalls() {
  return invoke.mock.calls.filter((c) => c[0] === 'list_history');
}
// The text of one part of a question row, by the test id that part carries.
function rowPart(id: number, part: 'history-query' | 'history-asked') {
  return screen.getByTestId(`tree-history-${id}`).querySelector(`[data-testid="${part}"]`)!.textContent;
}

// The same root as `oneRootTwoFolders` with one more file in `archive/` — what
// an index change looks like from this card. Local, like `mockTree` (Ruling L).
function withNewFile(): TreeListing {
  const [root] = oneRootTwoFolders.roots;
  return {
    roots: [{ ...root, files: [...root.files, { relativePath: 'archive/new.md', documentId: 'doc-new' }] }],
  };
}

// A citation is only ever read for its `documentId` here (Ruling P), but the
// prop is the real `AskCitation | Hit | null` union (Ruling O), so build one
// from the shipped fixture rather than from a hand-written literal that could
// drift from the wire type.
function citationFor(documentId: string, relativePath: string | null = 'notes/a.md'): AskCitation {
  if (generated.kind !== 'generated') throw new Error('fixture drifted');
  return { ...generated.citations[0], documentId, relativePath };
}

// Everything in the card that a keyboard can reach or that claims a role, in
// document order. Used as a closed enumeration (P5) rather than a spot check.
function controls() {
  return [...screen.getByTestId('tree-body').querySelectorAll('button, [tabindex], [role]')]
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
  history = [];
  failHistory = false;
  failDelete = false;
});

// Mirrors Cards.test.ts / Answer.test.ts: `locale` is a module-level store
// shared by every test in this file, and an in-test restore is skipped when an
// assertion fails first. Restore unconditionally.
afterEach(() => {
  setLocale('en');
  // The recency tests pin `Date.now`; restore unconditionally, for the same
  // reason the locale is restored here — an assertion that fails first would
  // otherwise leave the next test running against a frozen clock.
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

// 🔴 Owner review on PR #24, P5. File rows were `<button>`s with no `onclick`:
// click, Enter and Space all did nothing, and they carried `role="treeitem"` with
// no enclosing tree. Rows that promise an action they do not have are the finding.
//
// The assertion enumerates every focusable or role-bearing element in the card, in
// document order, so it is a closed statement. The Files tab's rows are not in it
// (inert); the two folders and the two tabs are (each has a real action). The
// Recents tab's rows are not in it either, but each question's own open and remove
// buttons are: those are the actions the row has.
test('the controls on each tab are exactly the ones with a real action', async () => {
  mockTree(oneRootTwoFolders);
  mockHistory(historyTwoQuestions);
  render(Tree, { selected: citationFor('doc-1') }); // notes/ is open, so its files are on screen

  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  expect(controls()).toEqual([
    'tree-tab-files',
    'tree-tab-recents',
    'tree-folder-notes',
    'tree-folder-archive',
  ]);

  await fireEvent.click(screen.getByTestId('tree-tab-recents'));
  expect(await screen.findByTestId('tree-history-1')).toBeTruthy();
  expect(controls()).toEqual([
    'tree-tab-files',
    'tree-tab-recents',
    'history-open-2',
    'history-remove-2',
    'history-open-1',
    'history-remove-1',
  ]);
});


test('the tree nests files under their folders instead of listing flat paths', async () => {
  mockTree(oneRootTwoFolders);
  render(Tree, { selected: null });

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

test('a non-ASCII folder name survives the split and its own test id', async () => {
  // Ruling K: this fixture is local because `lib/fixtures.ts` is read by the
  // Cyrillic guard (`i18n/guard.test.ts:19-21`) and a `.test.ts` is not.
  mockTree({
    roots: [
      {
        rootId: 9,
        absolutePath: '/home/u/ua',
        name: 'ua',
        files: [{ relativePath: 'Договори/dohov-01.md', documentId: 'doc-u' }],
      },
    ],
  });
  render(Tree, { selected: null });

  expect(await screen.findByTestId('tree-folder-Договори')).toBeTruthy();
  expect(screen.queryByText('Договори/dohov-01.md')).toBeNull();
});

test('the folder holding the selected file is open; the others are not', async () => {
  mockTree(oneRootTwoFolders);
  render(Tree, { selected: citationFor('doc-1') }); // notes/a.md

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
      { rootId: 1, absolutePath: '/home/u/one', name: 'one', files: [{ relativePath: 'notes/x.md', documentId: 'doc-one' }] },
      { rootId: 2, absolutePath: '/home/u/two', name: 'two', files: [{ relativePath: 'notes/y.md', documentId: 'doc-two' }] },
    ],
  });
  render(Tree, { selected: null });

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

test('the files tab lists each root with its files; the recents tab lists the questions', async () => {
  mockTree(oneRootTwoFolders);
  mockHistory(historyTwoQuestions);
  render(Tree, { selected: null });

  expect(await screen.findByTestId('tree-root-1')).toBeTruthy();
  expect(screen.getByTestId('tree-root-1').textContent).toContain('docs');
  expect(screen.queryByTestId('tree-history-1')).toBeNull(); // the other tab is not also drawn

  await fireEvent.click(screen.getByTestId('tree-tab-recents'));
  expect(await screen.findByTestId('tree-history-1')).toBeTruthy();
  expect(screen.getByTestId('tree-history-2')).toBeTruthy();
  // Both directions: switching tabs replaces the listing rather than appending.
  expect(screen.queryByTestId('tree-folder-notes')).toBeNull();

  await fireEvent.click(screen.getByTestId('tree-tab-files'));
  expect(screen.getByTestId('tree-folder-notes')).toBeTruthy();
  expect(screen.queryByTestId('tree-history-1')).toBeNull();
});

test('the selected citation selects its file by documentId, not by path string', async () => {
  mockTree(twoRootsSameRelativePath); // two roots, both holding README.md
  const { rerender } = render(Tree, { selected: null });
  expect(await screen.findByTestId('tree-file-doc-a')).toBeTruthy();

  // The citation's own path is 'README.md' — the path BOTH roots hold. An
  // implementation keyed on the path string marks both rows, and it is the
  // doc-a assertion below that catches it; a citation with a non-matching path
  // would only prove that a wrong lookup finds nothing.
  await rerender({ selected: citationFor('doc-b', 'README.md') });
  // Enumerated, not two spot checks: `aria-selected` used to be asserted here
  // as the second statement that could contradict the first, and P5 removed it
  // with the `treeitem` role it belonged to. What replaces it is the closed
  // list — exactly one row in the whole card is current, and it is doc-b.
  expect(currentRows()).toEqual(['tree-file-doc-b']);
  expect(screen.getByTestId('tree-file-doc-a').getAttribute('aria-current')).toBeNull();
});

// The decision, and its reason. `toggled` wins over `openByDefault` so that
// folders do not snap shut on every question — that is Ruling M's point and it
// stands. But "a new citation is now selected" is a different event from "an
// answer arrived": it is the person's own click on a citation, and its whole
// purpose is to be shown where the passage came from. So a NEW selection clears
// the hand-toggle on the folders along its own path, and only on those.
//
// What is deliberately NOT undone: a person who shuts the folder of the passage
// already on screen keeps it shut. They acted on a row they could see, no new
// event has happened since, and re-opening it under their hands would be the
// snapping-shut defect with the sign flipped.
test('a folder the person shut opens again when the next citation lands inside it', async () => {
  mockTree(oneRootTwoFolders);
  const { rerender } = render(Tree, { selected: citationFor('doc-1') }); // notes/ opens

  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  await fireEvent.click(screen.getByTestId('tree-folder-notes')); // shut by hand
  expect(screen.queryByTestId('tree-file-doc-1')).toBeNull();

  await rerender({ selected: citationFor('doc-2', 'notes/b.md') }); // a new citation, same folder
  await tick();

  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('true');
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
  render(Tree, { selected: null });

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
  render(Tree, { selected: null });
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
  render(Tree, { selected: null });
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
  render(Tree, { selected: null });

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
  const { rerender } = render(Tree, { selected: null });
  expect(await screen.findByTestId('tree-file-doc-a')).toBeTruthy();
  expect(treeCalls()).toHaveLength(1);
  expect(invoke).toHaveBeenCalledWith('list_tree');

  await rerender({ selected: citationFor('doc-b') });
  await tick();
  expect(treeCalls()).toHaveLength(1);
});

// Ruling J, the positive control. Without it the negative test below is
// satisfied by zero, and it is THIS one that must redden when the selection
// logic breaks.
//
// M1 (review round 1): this test used to be called "marks exactly one row
// current", which reads as a general contract and is not one — `toHaveLength(1)`
// is true here only because `oneRoot` holds ONE root with ONE file, so one is
// the only count the fixture can produce. A document named from two roots has
// two rows and both are current; that is the real contract, pinned by
// `a document present under two roots marks every row that shows it` below.
// The name now says which claim this test actually makes.
test('a citation whose document IS in the listing marks its only copy current', async () => {
  mockTree(oneRoot);
  render(Tree, { selected: citationFor('doc-1') });

  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  // The role assertion this test used to carry went with the role (P5): the
  // rows are not `treeitem`s any more, and `only the controls with a real
  // action are controls` is where that is now stated, positively and for the
  // whole card. What survives here is the claim this test is named for.
  expect(currentRows()).toEqual(['tree-file-doc-1']);
});

// M1 (review round 1): `tree-file-{documentId}` is NOT unique. One document can
// be named from two watched roots (`mnema-index/src/write.rs:700-722`), so the
// same id renders twice and `getByTestId` throws "Found multiple elements" —
// the id Task 8b selects on. Marking both is the right behaviour: it is one
// document, cited once, present twice on disk. Nothing pinned it.
test('a document present under two roots marks every row that shows it', async () => {
  mockTree(oneDocumentTwoRoots);
  render(Tree, { selected: citationFor('doc-shared', 'notes/shared.md') });

  const rows = await screen.findAllByTestId('tree-file-doc-shared');
  expect(rows).toHaveLength(2); // one row per path, under alpha and under beta
  expect(rows.map((r) => r.getAttribute('aria-current'))).toEqual(['true', 'true']);

  // Both directions: the neighbouring document in the same open folder is not
  // swept up, so "every row" means every row OF THAT DOCUMENT, not every row.
  // The enumeration says both halves at once — two current rows, both the
  // shared document's.
  expect(currentRows()).toEqual(['tree-file-doc-shared', 'tree-file-doc-shared']);
  expect(screen.getByTestId('tree-file-doc-other').getAttribute('aria-current')).toBeNull();
});

test('a citation whose document is no longer in the listing selects nothing and says nothing false', async () => {
  mockTree(oneRoot);
  render(Tree, { selected: citationFor('doc-gone') });

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

// 🔴 Review Minor 5 — the class of the owner's five findings, one tab over. A
// list called Recents whose rows carry no moment is an ordering a person has to
// take on trust, so each question says how long ago it was asked.
//
// The clock is pinned so the two fixture questions land on 4 and 5 minutes, which
// in Ukrainian are different plural arms (few and many).
const RECENTS_NOW = 1_700_000_340_000; // 240 s after the newer question, 340 s after the older

test('the recents tab lists questions newest first, each with how long ago it was asked', async () => {
  setLocale('en'); // seed, do not inherit
  vi.spyOn(Date, 'now').mockReturnValue(RECENTS_NOW);
  mockTree(oneRootTwoFolders);
  mockHistory(historyTwoQuestions);
  render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  expect(await screen.findByTestId('tree-history-1')).toBeTruthy();
  // The order is the order `list_history` sent: the card does not sort it.
  expect(screen.getAllByTestId(/^tree-history-/).map((el) => el.dataset.testid))
    .toEqual(['tree-history-2', 'tree-history-1']);
  expect([
    rowPart(2, 'history-query'),
    rowPart(2, 'history-asked'),
    rowPart(1, 'history-query'),
    rowPart(1, 'history-asked'),
  ]).toEqual([
    'when does the lease renew',
    '4 minutes ago',
    'invoice payment terms',
    '5 minutes ago',
  ]);
});

test('the recency follows a live language switch, through both plural arms', async () => {
  setLocale('en'); // seed, do not inherit
  vi.spyOn(Date, 'now').mockReturnValue(RECENTS_NOW);
  mockTree(oneRootTwoFolders);
  mockHistory(historyTwoQuestions);
  render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  expect(await screen.findByTestId('tree-history-2')).toBeTruthy();
  expect(rowPart(2, 'history-asked')).toBe('4 minutes ago');

  setLocale('uk');
  await tick();
  expect(rowPart(2, 'history-asked')).toBe('4 хвилини тому'); // few
  expect(rowPart(1, 'history-asked')).toBe('5 хвилин тому'); // many

  setLocale('en'); // the switch back is part of the claim, not the cleanup
  await tick();
  expect(rowPart(2, 'history-asked')).toBe('4 minutes ago');
});

// The hazard the row ids are a namespace against: a second id inside a row would
// be counted as a row by anything querying `/^tree-history-/`, so the question's
// parts carry ids that do not start with that prefix.
test('the question row namespace holds one id per question and nothing else', async () => {
  mockTree(oneRootTwoFolders);
  mockHistory(historyTwoQuestions);
  render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  expect(await screen.findByTestId('tree-history-1')).toBeTruthy();
  expect(screen.getAllByTestId(/^tree-history-/).map((el) => el.dataset.testid))
    .toEqual(['tree-history-2', 'tree-history-1']);
});


// `aria-current` is the attribute the style keys on, so it is what is asserted.
// The question on screen is the `historyId` the launcher reports, not the document
// the citation names: a question has no document, and the two can both be absent.
test('the question on screen is the one row marked aria-current', async () => {
  mockTree(oneRootTwoFolders);
  mockHistory(historyTwoQuestions);
  render(Tree, { selected: null, historyId: 1, historyGen: 0, onOpenHistory: () => {} });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  expect(await screen.findByTestId('tree-history-1')).toBeTruthy();
  expect(currentRows()).toEqual(['tree-history-1']);
  expect(screen.getByTestId('tree-history-2').getAttribute('aria-current')).toBeNull();
});

// Ruling N, for the history list: an empty history is not an empty index, and it
// says so in its own words, not the Files tab's.
test('an empty history says so, in the words of the catalog', async () => {
  setLocale('en'); // seed, do not inherit
  mockTree(oneRootTwoFolders); // the Files tab has something on it, so the message is the history's own
  render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  expect(await screen.findByTestId('history-empty')).toBeTruthy();
  expect(screen.getByTestId('history-empty').textContent).toBe('Your questions will appear here');
  expect(screen.queryByTestId('tree-empty')).toBeNull();

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('history-empty').textContent).toBe('Тут з’являться ваші запити');
});

// The remove button's accessible name is the only thing that tells two rows apart
// for a screen reader, so it is asserted by name, scoped to its own row, in both
// directions: row 1's button names row 1's question and not row 2's.
test('each remove button is named for its own question, so two rows can be told apart', async () => {
  setLocale('en'); // seed, do not inherit
  mockTree(oneRootTwoFolders);
  mockHistory(historyTwoQuestions);
  render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  const row1 = await screen.findByTestId('tree-history-1');
  const row2 = screen.getByTestId('tree-history-2');
  expect(within(row1).getByRole('button', { name: 'Remove from history: invoice payment terms' })).toBeTruthy();
  expect(within(row2).getByRole('button', { name: 'Remove from history: when does the lease renew' })).toBeTruthy();
  expect(within(row1).queryByRole('button', { name: 'Remove from history: when does the lease renew' })).toBeNull();
});

// A first read that fails leaves the Recents tab saying so, in the wording the
// Files tab uses for a failed listing, and not drawing an empty list that would
// read as "no questions".
test('a question list that cannot be read says so on the Recents tab', async () => {
  setLocale('en'); // seed, do not inherit
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  mockTree(oneRootTwoFolders);
  failHistory = true;
  render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  expect(await screen.findByTestId('history-failed')).toBeTruthy();
  expect(screen.getByTestId('history-failed').textContent).toBe('The tree could not be loaded.');
  expect(screen.queryByTestId('history-empty')).toBeNull();
  expect(err).toHaveBeenCalled(); // logged, so the rejection was handled, not left unhandled
});

// A delete that fails must not take the row off the screen: the list is what the
// database still holds, and the person was not told the question is gone.
test('a question whose delete fails stays on screen and is not opened', async () => {
  mockTree(oneRootTwoFolders);
  mockHistory(historyTwoQuestions);
  failDelete = true;
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  const onOpenHistory = vi.fn();
  render(Tree, { selected: null, historyId: null, historyGen: 0, onOpenHistory });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  await fireEvent.click(await screen.findByTestId('history-remove-1'));
  await waitFor(() => expect(err).toHaveBeenCalled()); // the failure was logged, so the rejection was handled

  expect(screen.getByTestId('tree-history-1')).toBeTruthy();
  expect(screen.getByTestId('tree-history-2')).toBeTruthy();
  expect(onOpenHistory).not.toHaveBeenCalled();
});


// The other side of the same rule, and the one the comment in `Tree.svelte`
// claimed before anything held it (controller, after the review): the stamp is
// `[selectedId, folders on its way]`, so a SECOND CITATION OF THE SAME DOCUMENT
// does not move it — the effect does not re-fire and the hand-shut folder stays
// shut. That is not a gap in the invariant: the mark says WHICH DOCUMENT the
// source card is showing, and the document did not change. The person shut that
// folder while this same document was selected, and nothing has happened since
// that they did not do themselves.
test('a second citation of the same document leaves the folder the person shut alone', async () => {
  mockTree(oneRootTwoFolders);
  const { rerender } = render(Tree, { selected: citationFor('doc-1') }); // notes/ opens

  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  await fireEvent.click(screen.getByTestId('tree-folder-notes')); // shut by hand
  expect(screen.queryByTestId('tree-file-doc-1')).toBeNull();

  // A different passage of the SAME document — what clicking [7] after [3] does.
  await rerender({ selected: { ...citationFor('doc-1'), chunkId: 43, ord: 1 } });
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
test('a new citation leaves the folders the person opened elsewhere exactly as they were', async () => {
  mockTree(oneRootTwoFolders);
  const { rerender } = render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-folder-archive')); // opened by hand
  expect(screen.getByTestId('tree-file-doc-3')).toBeTruthy();

  await rerender({ selected: citationFor('doc-1') }); // lands in notes/, not in archive/
  await tick();

  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByTestId('tree-file-doc-3')).toBeTruthy();
  expect(currentRows()).toEqual(['tree-file-doc-1']);
});

// 🔴 Review Minor 4: the one case where a hand-shut folder DOES re-open on an
// event the person did not cause. The stamp holds the folders on the selected
// document's way, so a refresh that reshapes that chain — the file moved — is a
// change of stamp and the effect re-fires. It is defensible: the chain really
// did change, so "which row it came from" changed with it, and the effect can
// only ever write `false → true`, so no refresh can snap a folder shut. It is
// pinned here because the rule's comment used to promise more than this, and
// because nothing else in the suite reaches the state.
//
// Passes on `9095fd7`: this is what the code already did, written down.
test('a refresh that moves the selected file re-opens the folder the person shut', async () => {
  mockTree(oneRootTwoFolders);
  render(Tree, { selected: citationFor('doc-1') }); // notes/a.md — notes/ opens

  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  await fireEvent.click(screen.getByTestId('tree-folder-notes')); // shut by hand
  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('false');

  const [root] = oneRootTwoFolders.roots;
  mockTree({ // the same document, one folder deeper
    roots: [{ ...root, files: [{ relativePath: 'notes/deep/a.md', documentId: 'doc-1' }] }],
  });
  await fireEvent.focus(window);

  expect(await screen.findByTestId('tree-folder-notes/deep')).toBeTruthy();
  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('true');
  expect(currentRows()).toEqual(['tree-file-doc-1']);
});

// The control, and the half that makes the case above narrow rather than
// general: a refresh returning the SAME listing leaves the shut folder shut,
// because the stamp did not move. Without this, "a refresh re-opens folders"
// would read as the rule instead of the exception.
test('a refresh that changes nothing leaves the folder the person shut alone', async () => {
  mockTree(oneRootTwoFolders);
  render(Tree, { selected: citationFor('doc-1') });

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
// it snapped shut the moment the next answer was a refusal — Ruling M's defect,
// reached the long way round. The fix sets a shut folder open instead of
// forgetting what the person did, and this test is what says so. It passes on
// `14ab473` too: it is the property that had to survive the fix, not a defect.
test('a folder the person opened stays open after the citation inside it goes away', async () => {
  mockTree(oneRootTwoFolders);
  const { rerender } = render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-folder-notes')); // opened by hand
  await rerender({ selected: citationFor('doc-1') }); // a citation lands inside it
  await tick();
  expect(currentRows()).toEqual(['tree-file-doc-1']);

  await rerender({ selected: null }); // the answer goes away
  await tick();
  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByTestId('tree-file-doc-1')).toBeTruthy();
});

// A question is not a document, so a selection that no question names is simply
// not shown on the Recents tab. The tab the person chose stays chosen: the rule
// that moved the card to Files when the selection had no row is gone with the
// documents it compared against. (Folders still open for a citation; see above.)
test('a selection never moves the person off the tab they chose', async () => {
  mockTree(oneRootTwoFolders);
  mockHistory(historyTwoQuestions);
  const { rerender } = render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  await rerender({ selected: citationFor('doc-2', 'notes/b.md') }); // a document no question names
  await tick();

  expect(screen.getByTestId('tree-tab-recents').getAttribute('aria-pressed')).toBe('true');
  expect(screen.queryByTestId('tree-folder-notes')).toBeNull();
});

// The row's own open button hands the window the question's id. It does not ask
// again: the answer for a question already given is read back, not recomputed.
test('clicking a question calls onOpenHistory with its id, and does not ask again', async () => {
  mockTree(oneRootTwoFolders);
  mockHistory(historyTwoQuestions);
  const onOpenHistory = vi.fn();
  render(Tree, { selected: null, historyId: null, historyGen: 0, onOpenHistory });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  await fireEvent.click(await screen.findByTestId('history-open-1'));

  expect(onOpenHistory.mock.calls).toEqual([[1]]);
  expect(invoke.mock.calls.filter((c) => c[0] === 'ask')).toEqual([]);
});

// The remove button deletes at once, without a confirmation, and it is a sibling of
// the open button, not a child of it: a click on it must not also open the entry.
test('the remove button deletes the question, does not open it, and the row goes', async () => {
  mockTree(oneRootTwoFolders);
  mockHistory(historyTwoQuestions);
  const onOpenHistory = vi.fn();
  render(Tree, { selected: null, historyId: null, historyGen: 0, onOpenHistory });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  await fireEvent.click(await screen.findByTestId('history-remove-1'));

  expect(invoke).toHaveBeenCalledWith('delete_history_entry', { id: 1 });
  await waitFor(() => expect(screen.queryByTestId('tree-history-1')).toBeNull());
  expect(screen.getByTestId('tree-history-2')).toBeTruthy();
  expect(onOpenHistory).not.toHaveBeenCalled();
  expect(invoke.mock.calls.filter((c) => c[0] === 'ask')).toEqual([]);
});

// The twin of `list_tree does not run again when the selection changes`: a change
// of selection does not re-read the questions. They are read on mount, on focus
// and when a question is answered, and never because the citation moved.
test('a change of selection does not read the questions again', async () => {
  mockTree(twoRootsSameRelativePath);
  mockHistory(historyTwoQuestions);
  const { rerender } = render(Tree, { selected: null, historyId: null, historyGen: 0, onOpenHistory: vi.fn() });
  expect(await screen.findByTestId('tree-file-doc-a')).toBeTruthy();
  expect(historyCalls()).toHaveLength(1);

  await rerender({ selected: citationFor('doc-b'), historyId: null, historyGen: 0, onOpenHistory: vi.fn() });
  await tick();
  expect(historyCalls()).toHaveLength(1);
});

// The questions are read when the card mounts, when the launcher comes back in
// front, and when the launcher says a question was answered (`historyGen`).
test('the questions are read on mount, again on focus, and again when historyGen changes', async () => {
  mockTree(oneRootTwoFolders);
  mockHistory(historyTwoQuestions);
  const onOpenHistory = vi.fn();
  const { rerender } = render(Tree, { selected: null, historyId: null, historyGen: 0, onOpenHistory });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  expect(await screen.findByTestId('tree-history-2')).toBeTruthy();
  expect(historyCalls()).toHaveLength(1);

  await fireEvent.focus(window);
  await waitFor(() => expect(historyCalls()).toHaveLength(2));

  // A question asked while the launcher is up: the list must show it at once.
  mockHistory([{ id: 3, query: 'a question just asked', askedAt: 1_700_000_300 }, ...historyTwoQuestions]);
  await rerender({ selected: null, historyId: null, historyGen: 1, onOpenHistory });
  expect(await screen.findByTestId('tree-history-3')).toBeTruthy();
  expect(historyCalls()).toHaveLength(3);
});


// Ruling N: the plan's "never an empty card that looks like an empty index"
// only means something if the empty index also says something, and something
// different. Each must show its own message and not the other's.
test('an empty but successful listing says the index is empty, not that the listing failed', async () => {
  mockTree(emptyListing);
  render(Tree, { selected: null });

  expect(await screen.findByTestId('tree-empty')).toBeTruthy();
  expect(screen.getByTestId('tree-empty').textContent).toBe('Nothing is indexed yet.');
  expect(screen.queryByTestId('tree-failed')).toBeNull();
});

test('a failed list_tree leaves a visible message, never an empty card that looks like an empty index', async () => {
  mockTreeFailure();
  render(Tree, { selected: null });

  expect(await screen.findByTestId('tree-failed')).toBeTruthy();
  expect(screen.getByTestId('tree-failed').textContent).toBe('The tree could not be loaded.');
  expect(screen.queryByTestId('tree-empty')).toBeNull();
});

// D130: a bare `t()` in markup does not re-render on a language switch, and an
// English hardcode passes the Cyrillic guard silently. Both were review
// findings on Tasks 5 and 6.
test('the tab labels and the messages follow a live language switch', async () => {
  setLocale('en'); // seed, do not inherit: an earlier sibling switching the language must not decide this test
  mockTree(emptyListing);
  render(Tree, { selected: null });

  expect(await screen.findByTestId('tree-empty')).toBeTruthy();
  expect(screen.getByTestId('tree-tab-files').textContent).toBe('Files');
  expect(screen.getByTestId('tree-tab-recents').textContent).toBe('Recents');
  expect(screen.getByTestId('tree-empty').textContent).toBe('Nothing is indexed yet.');

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('tree-tab-files').textContent).toBe('Файли');
  expect(screen.getByTestId('tree-tab-recents').textContent).toBe('Нещодавні');
  expect(screen.getByTestId('tree-empty').textContent).toBe('Ще нічого не проіндексовано.');

  setLocale('en'); // the switch back is part of the claim, not the cleanup — afterEach owns that
  await tick();
  expect(screen.getByTestId('tree-tab-files').textContent).toBe('Files');
  expect(screen.getByTestId('tree-empty').textContent).toBe('Nothing is indexed yet.');
});

test('the failure message follows a live language switch too', async () => {
  setLocale('en'); // seed, do not inherit
  mockTreeFailure();
  render(Tree, { selected: null });

  expect((await screen.findByTestId('tree-failed')).textContent).toBe('The tree could not be loaded.');

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('tree-failed').textContent).toBe('Не вдалося завантажити дерево.');
});

// Ruling N, the Files half. The Files tab is empty only when there are no roots:
// the questions do not count toward it, and the Recents tab does not borrow its
// message. Both directions are on screen in one state.
test('the Files tab says the index is empty only when it has no roots, whatever the questions', async () => {
  setLocale('en'); // seed, do not inherit
  mockTree(emptyListing);
  mockHistory(historyTwoQuestions);
  render(Tree, { selected: null });

  expect(await screen.findByTestId('tree-empty')).toBeTruthy();
  expect(screen.getByTestId('tree-empty').textContent).toBe('Nothing is indexed yet.');
  expect(screen.queryByTestId('tree-failed')).toBeNull();

  await fireEvent.click(screen.getByTestId('tree-tab-recents'));
  expect(await screen.findByTestId('tree-history-2')).toBeTruthy();
  expect(screen.queryByTestId('tree-empty')).toBeNull();
});


// M3 (review round 1): `aria-expanded` is a second statement about the same
// state as "are the children rendered", and a second signal is free to say the
// opposite and stay green — the shape already caught once on `aria-selected`.
test('aria-expanded on a folder says what the folder is actually doing', async () => {
  mockTree(oneRootTwoFolders);
  render(Tree, { selected: citationFor('doc-1') }); // notes/a.md — opens notes/

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

// M4 (review round 1): same shape one row up — `aria-pressed` is the only thing
// telling a screen reader which tab is showing, and no test read it.
test('aria-pressed says which tab is showing', async () => {
  mockTree(oneRootTwoFolders);
  mockHistory(historyTwoQuestions);
  render(Tree, { selected: null });

  expect((await screen.findByTestId('tree-tab-files')).getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByTestId('tree-tab-recents').getAttribute('aria-pressed')).toBe('false');

  await fireEvent.click(screen.getByTestId('tree-tab-recents'));
  expect(screen.getByTestId('tree-tab-files').getAttribute('aria-pressed')).toBe('false');
  expect(screen.getByTestId('tree-tab-recents').getAttribute('aria-pressed')).toBe('true');
  expect(await screen.findByTestId('tree-history-2')).toBeTruthy(); // the flag agrees with the content

  await fireEvent.click(screen.getByTestId('tree-tab-files'));
  expect(screen.getByTestId('tree-tab-files').getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByTestId('tree-tab-recents').getAttribute('aria-pressed')).toBe('false');
});

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
  generated,
} from '../lib/fixtures';
import { setLocale } from '../i18n';
import type { AskCitation, TreeListing } from '../lib/ipc';

// The house pattern for the bridge (`lib/ipc.test.ts:14-15`): one shared spy,
// the module mocked once at file scope.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

// Ruling L: `mockTree` is a mock, not a fixture — it stays local to this file.
function mockTree(listing: TreeListing) {
  invoke.mockResolvedValue(listing);
}
function mockTreeFailure() {
  invoke.mockRejectedValue(new Error('list_tree failed'));
}
// A listing that has NOT arrived yet, so a second request can be attempted
// while the first is still on the wire (`Cards.test.ts` keeps the same shape).
function mockTreePending() {
  let resolve!: (listing: TreeListing) => void;
  invoke.mockReturnValue(new Promise<TreeListing>((r) => (resolve = r)));
  return { resolve: (l: TreeListing) => resolve(l) };
}

// The same root as `oneRootTwoFolders` with one more file in `archive/` — what
// an index change looks like from this card. Local, like `mockTree` (Ruling L).
function withNewFile(): TreeListing {
  const [root] = oneRootTwoFolders.roots;
  return {
    roots: [{ ...root, files: [...root.files, { relativePath: 'archive/new.md', documentId: 'doc-new' }] }],
    recents: oneRootTwoFolders.recents,
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

// 🔴 Owner review on PR #24, P5. File and recent rows were `<button>`s with no
// `onclick`: click, Enter and Space all did nothing, and the file rows carried
// `role="treeitem"` with no enclosing tree or group and no keyboard model. They
// became buttons to satisfy the 0-warning a11y gate and nothing ever wired an
// action to them, so the card promised an action it has not got.
//
// The decision is the owner's second option — render them as non-interactive
// rows — because the first is not reachable from here: opening a document from
// the tree needs a command that takes a `documentId`, and the bridge has none
// (`lib/ipc.ts:81-107`: `ask`, `set_search_arms`, `list_tree`, `source_around`,
// `model_settings`; `source_around` needs a chunk that only a citation carries).
// An action wired to nothing is what this finding IS.
//
// The assertion enumerates every focusable or role-bearing element in the card,
// in document order, so it is a closed statement rather than a spot check: a row
// that becomes focusable again — or a `role` put back on one — appears in this
// list. The two tabs and the two folders are here because each has a real
// action; that is the other direction, and it is what stops "make everything
// inert" from passing.
test('only the controls with a real action are controls; the rows are inert', async () => {
  mockTree(oneRootTwoFolders);
  render(Tree, { selected: citationFor('doc-1') }); // notes/ is open, so its files are on screen

  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  expect(controls()).toEqual([
    'tree-tab-files',
    'tree-tab-recents',
    'tree-folder-notes',
    'tree-folder-archive',
  ]);

  await fireEvent.click(screen.getByTestId('tree-tab-recents'));
  expect(screen.getByTestId('tree-recent-doc-1')).toBeTruthy();
  expect(controls()).toEqual(['tree-tab-files', 'tree-tab-recents']);
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
    recents: [],
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
    recents: [],
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

test('the files tab lists each root with its files; the recents tab lists the recent documents', async () => {
  mockTree(oneRootTwoFolders);
  render(Tree, { selected: null });

  expect(await screen.findByTestId('tree-root-1')).toBeTruthy();
  expect(screen.getByTestId('tree-root-1').textContent).toContain('docs');
  expect(screen.queryByTestId('tree-recent-doc-1')).toBeNull(); // the other tab is not also drawn

  await fireEvent.click(screen.getByTestId('tree-tab-recents'));
  expect(screen.getByTestId('tree-recent-doc-1')).toBeTruthy();
  expect(screen.getByTestId('tree-recent-doc-3')).toBeTruthy();
  // Both directions: switching tabs replaces the listing rather than appending.
  expect(screen.queryByTestId('tree-folder-notes')).toBeNull();

  await fireEvent.click(screen.getByTestId('tree-tab-files'));
  expect(screen.getByTestId('tree-folder-notes')).toBeTruthy();
  expect(screen.queryByTestId('tree-recent-doc-1')).toBeNull();
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
  expect(invoke).toHaveBeenCalledTimes(2);
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
  await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));

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
  expect(invoke).toHaveBeenCalledTimes(1);

  mockTree(oneRootTwoFolders);
  pending.resolve(oneRootTwoFolders);
  expect(await screen.findByTestId('tree-folder-notes')).toBeTruthy();

  await fireEvent.focus(window);
  await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
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
  expect(invoke).toHaveBeenCalledTimes(1);
  expect(invoke).toHaveBeenCalledWith('list_tree');

  await rerender({ selected: citationFor('doc-b') });
  await tick();
  expect(invoke).toHaveBeenCalledTimes(1);
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

// 🔴 Review Minor 5 — the class of the owner's five findings, one notch down.
// The wire carries `indexedAt` for every recent document (`ipc.ts:65`), the card
// is called Recents, and it rendered a path and nothing else: its ordering was
// asserted by nothing a person could see, and the one fact that makes the list
// mean anything never reached the screen.
//
// The assertion is the whole of each row's visible text, in order, in both
// locales. The two fixture documents are 100 seconds apart on purpose, and the
// clock is pinned so they land on 4 and 5 minutes — which in Ukrainian are
// different plural arms (few and many), so a single-locale or single-row test
// could not see a hardcoded form.
const RECENTS_NOW = 1_700_000_340_000; // 240 s after doc-1, 340 s after doc-3

test('the recents rows say how recently each document was indexed', async () => {
  setLocale('en'); // seed, do not inherit
  vi.spyOn(Date, 'now').mockReturnValue(RECENTS_NOW);
  mockTree(oneRootTwoFolders);
  render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  const rowText = (el: HTMLElement) => (el.textContent ?? '').replace(/\s+/g, ' ').trim();
  expect([
    rowText(screen.getByTestId('tree-recent-doc-1')),
    rowText(screen.getByTestId('tree-recent-doc-3')),
  ]).toEqual([
    'notes/a.md 4 minutes ago',
    'archive/old.md 5 minutes ago',
  ]);
});

test('the recency follows a live language switch, through both plural arms', async () => {
  setLocale('en'); // seed, do not inherit
  vi.spyOn(Date, 'now').mockReturnValue(RECENTS_NOW);
  mockTree(oneRootTwoFolders);
  render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  const indexed = (id: string) =>
    screen.getByTestId(id).querySelector('[data-testid="recent-indexed"]')!.textContent;
  expect(indexed('tree-recent-doc-1')).toBe('4 minutes ago');

  setLocale('uk');
  await tick();
  expect(indexed('tree-recent-doc-1')).toBe('4 хвилини тому'); // few
  expect(indexed('tree-recent-doc-3')).toBe('5 хвилин тому'); // many

  setLocale('en'); // the switch back is part of the claim, not the cleanup
  await tick();
  expect(indexed('tree-recent-doc-1')).toBe('4 minutes ago');
});

// The hazard P1 and P2 were both written against, made live here rather than
// only asserted in a comment: the row ids are a namespace, and a second id
// inside a row would be counted as a row by anything querying `/^tree-recent-/`.
// Nothing queried it before this test, so the rule the component's comment
// states was true and unheld — which is how `preview-label` got in once.
test('the recents row namespace holds one id per document and nothing else', async () => {
  mockTree(oneRootTwoFolders);
  render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  expect(screen.getAllByTestId(/^tree-recent-/).map((el) => el.dataset.testid))
    .toEqual(['tree-recent-doc-1', 'tree-recent-doc-3']);
});

test('the recents tab marks the row the source card is showing', async () => {
  mockTree(oneRootTwoFolders); // recents: doc-1 and doc-3
  render(Tree, { selected: citationFor('doc-1') });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  expect(screen.getByTestId('tree-recent-doc-1')).toBeTruthy();
  // Enumerated: exactly one row on this tab is current, and it is the cited
  // document's. On the shipped card no recents row carried the attribute at
  // all, so the whole tab was a listing with nothing marked.
  expect(currentRows()).toEqual(['tree-recent-doc-1']);
  expect(screen.getByTestId('tree-recent-doc-3').getAttribute('aria-current')).toBeNull();
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
    recents: oneRootTwoFolders.recents,
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
  await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));

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

// The other half of the recents finding, and the half `aria-current` alone
// cannot reach: `doc-2` has no recents row to mark. Marking rows is not the
// invariant — SHOWING the person which row the passage came from is — so when
// the tab on screen has no row for the selection and the other tab does, the
// card shows the one that does. The trigger is a change of selection, which is
// the person's own click; a tab they choose while the selection stands is left
// alone (the test below).
test('a selection with no recents row shows the tab that has one', async () => {
  mockTree(oneRootTwoFolders); // recents: doc-1, doc-3 — doc-2 is not among them
  const { rerender } = render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  expect(screen.getByTestId('tree-tab-recents').getAttribute('aria-pressed')).toBe('true');

  await rerender({ selected: citationFor('doc-2', 'notes/b.md') });
  await tick();

  expect(screen.getByTestId('tree-tab-files').getAttribute('aria-pressed')).toBe('true');
  expect(currentRows()).toEqual(['tree-file-doc-2']);
});

test('a selection the recents tab CAN show leaves the person on the tab they chose', async () => {
  mockTree(oneRootTwoFolders);
  const { rerender } = render(Tree, { selected: null });

  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  await rerender({ selected: citationFor('doc-1') }); // doc-1 has a recents row
  await tick();

  expect(screen.getByTestId('tree-tab-recents').getAttribute('aria-pressed')).toBe('true');
  expect(currentRows()).toEqual(['tree-recent-doc-1']);
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

// M2 (review round 1): this is Ruling N's substance and nothing tested it. The
// `recents.length === 0` conjunct is what stops a real index with no recent
// activity from announcing itself as empty; loosened to `||`, `oneRoot` claims
// "nothing is indexed" while holding a file.
test('a listing with roots but no recents is not an empty index', async () => {
  mockTree(oneRoot); // one root, one file, recents: []
  render(Tree, { selected: null });

  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  expect(screen.queryByTestId('tree-empty')).toBeNull();
  expect(screen.queryByTestId('tree-failed')).toBeNull();

  // And the Recents tab shows nothing without claiming the index is empty —
  // an empty tab is not an empty index.
  await fireEvent.click(screen.getByTestId('tree-tab-recents'));
  expect(screen.queryByTestId('tree-empty')).toBeNull();
  expect(screen.queryByTestId('tree-failed')).toBeNull();
});

// M2, round 2 — the conjunct's OWN test, and the one mutant that survived
// round 1 (`isEmpty` with `recents.length === 0` deleted) dies here.
//
// 🔴 The listing below is one the backend CANNOT currently emit. `roots` comes
// from `list_watched_roots()` and every `RecentDoc` carries a `watchedRootId`
// (`src-tauri/src/tree.rs:80-114`), so no watched roots means no indexed
// documents and so no recents; `tree.rs:116-125` reads the whole listing inside
// one `read_snapshot` precisely so it cannot carry "a recent whose (rootId,
// relativePath) is absent from every roots[].files".
//
// It is tested anyway, because the claim here is not about the backend. It is
// that **even handed a state that should be impossible, this card does not say
// something false** — and a defensive branch is tested with the input it
// defends against, or it is not tested at all. Without the conjunct the card
// prints "nothing is indexed" while indexed documents sit on the very next tab:
// a window claiming what it cannot know, which is the failure this whole card
// exists to prevent. The unreachability is a property of today's backend, held
// by one `read_snapshot` in another repository; if this test ever starts
// describing a reachable state, that is a BACKEND regression, and the card's
// job in the meantime is to not lie about it.
test('a listing with recents but no roots — impossible today — still refuses to call itself empty', async () => {
  mockTree({ roots: [], recents: [{ documentId: 'doc-1', rootId: 1, relativePath: 'notes/a.md', indexedAt: 1_700_000_100 }] });
  render(Tree, { selected: null });

  // 🔴 Wait on something only the RESOLVED listing can produce. The tabs render
  // before `listTree()` settles, so anchoring the wait on them would assert
  // against a still-loading card where `listing` is null and `isEmpty` false for
  // a reason that has nothing to do with the conjunct — green under the mutant.
  await fireEvent.click(screen.getByTestId('tree-tab-recents'));
  expect(await screen.findByTestId('tree-recent-doc-1')).toBeTruthy();

  // Both directions: no empty-index claim, and the documents it would be lying
  // about are demonstrably on screen next to it.
  expect(screen.queryByTestId('tree-empty')).toBeNull();
  expect(screen.queryByTestId('tree-failed')).toBeNull();

  // And the Files tab, where the message would actually be drawn, is silent too.
  await fireEvent.click(screen.getByTestId('tree-tab-files'));
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
  render(Tree, { selected: null });

  expect((await screen.findByTestId('tree-tab-files')).getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByTestId('tree-tab-recents').getAttribute('aria-pressed')).toBe('false');

  await fireEvent.click(screen.getByTestId('tree-tab-recents'));
  expect(screen.getByTestId('tree-tab-files').getAttribute('aria-pressed')).toBe('false');
  expect(screen.getByTestId('tree-tab-recents').getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByTestId('tree-recent-doc-1')).toBeTruthy(); // the flag agrees with the content

  await fireEvent.click(screen.getByTestId('tree-tab-files'));
  expect(screen.getByTestId('tree-tab-files').getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByTestId('tree-tab-recents').getAttribute('aria-pressed')).toBe('false');
});

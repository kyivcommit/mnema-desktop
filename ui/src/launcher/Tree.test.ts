import { render, screen, fireEvent } from '@testing-library/svelte';
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

// A citation is only ever read for its `documentId` here (Ruling P), but the
// prop is the real `AskCitation | Hit | null` union (Ruling O), so build one
// from the shipped fixture rather than from a hand-written literal that could
// drift from the wire type.
function citationFor(documentId: string, relativePath: string | null = 'notes/a.md'): AskCitation {
  if (generated.kind !== 'generated') throw new Error('fixture drifted');
  return { ...generated.citations[0], documentId, relativePath };
}

beforeEach(() => {
  invoke.mockReset();
});

// Mirrors Cards.test.ts / Answer.test.ts: `locale` is a module-level store
// shared by every test in this file, and an in-test restore is skipped when an
// assertion fails first. Restore unconditionally.
afterEach(() => setLocale('en'));

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
  expect(screen.getByTestId('tree-file-doc-b').getAttribute('aria-current')).toBe('true');
  expect(screen.getByTestId('tree-file-doc-a').getAttribute('aria-current')).toBeNull();
  // `aria-selected` is required on a treeitem (svelte's a11y gate) and would
  // otherwise ride along undefended, free to say the opposite of aria-current.
  expect(screen.getByTestId('tree-file-doc-b').getAttribute('aria-selected')).toBe('true');
  expect(screen.getByTestId('tree-file-doc-a').getAttribute('aria-selected')).toBe('false');
});

// Ruling M: Task 8b deliberately does not key this card, because `state`
// changes twice per question and a remounted tree would refetch and snap every
// folder the person opened shut. A change of `selected` must not re-invoke.
test('list_tree runs on mount and only on mount, even when the selection changes', async () => {
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
  const current = screen.getAllByRole('treeitem', { current: true });
  expect(current).toHaveLength(1);
  expect(current[0]).toBe(screen.getByTestId('tree-file-doc-1'));
  expect(current[0].getAttribute('aria-selected')).toBe('true');
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
  expect(screen.getAllByRole('treeitem', { current: true })).toHaveLength(2);

  // Both directions: the neighbouring document in the same open folder is not
  // swept up, so "every row" means every row OF THAT DOCUMENT, not every row.
  expect(screen.getByTestId('tree-file-doc-other').getAttribute('aria-current')).toBeNull();
  expect(rows.map((r) => r.getAttribute('aria-selected'))).toEqual(['true', 'true']);
  expect(screen.getByTestId('tree-file-doc-other').getAttribute('aria-selected')).toBe('false');
});

test('a citation whose document is no longer in the listing selects nothing and says nothing false', async () => {
  mockTree(oneRoot);
  render(Tree, { selected: citationFor('doc-gone') });

  // The row that IS in the listing is on screen either way — the difference
  // between this test and its control is `aria-current`, not an empty tree.
  expect(await screen.findByTestId('tree-file-doc-1')).toBeTruthy();
  expect(screen.queryByRole('treeitem', { current: true })).toBeNull();
  expect(screen.getByTestId('tree-file-doc-1').getAttribute('aria-selected')).toBe('false');
  expect(screen.getByTestId('tree-body').textContent).not.toMatch(/no longer|більше немає/i);
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

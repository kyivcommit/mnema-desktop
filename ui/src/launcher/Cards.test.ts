import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import Cards from './Cards.svelte';
import { stateFromAnswer } from './state';
import {
  generated,
  generatedOther,
  refusedNoCandidates,
  citationsOnly,
  oneRootTwoFolders,
  emptyListing,
  excerptSpanA,
  excerptSpanB,
  SPAN_A_TEXT,
  SPAN_B_TEXT,
} from '../lib/fixtures';
import { setLocale } from '../i18n';
import type { SourceAround, TreeListing } from '../lib/ipc';

// The house pattern for the bridge (`lib/ipc.test.ts:14-15`, `Tree.test.ts:19-20`,
// `Source.test.ts:26-27`): one shared spy, the module mocked once at file scope.
//
// 🔴 Task 8b is the first suite that needs it here at all. `Cards` used to render
// two empty sections; now it mounts `Tree` (which calls `list_tree` on mount) and,
// after a click, `Source` (which calls `source_around`). Every test in this file
// — including the six that predate this task — goes through the mock below, so an
// unmocked command can never reach jsdom.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

// Mocks, not fixtures (Ruling L): they stay local to this file. Both commands go
// through the one spy, so the implementation dispatches on the command name and,
// for `source_around`, on the chunkId the caller asked for — the clicked citation
// and its sibling are two different round trips with two different answers.
let tree: Promise<TreeListing>;
let sources: Record<number, Promise<SourceAround>>;

function mockTree(listing: TreeListing) {
  tree = Promise.resolve(listing);
}
// A listing that has NOT arrived yet, so a click can be made before the left card
// has anything to mark.
function mockTreePending() {
  const d = deferred<TreeListing>();
  tree = d.promise;
  return d;
}
function mockSourceFor(byChunkId: Record<number, SourceAround | Promise<SourceAround>>) {
  sources = {};
  for (const [id, answer] of Object.entries(byChunkId)) sources[Number(id)] = Promise.resolve(answer);
}
function listTreeCalls() {
  return invoke.mock.calls.filter(([cmd]) => cmd === 'list_tree');
}
function sourceAroundCalls() {
  return invoke.mock.calls.filter(([cmd]) => cmd === 'source_around');
}

// 🔴 Ruling AB. `Source` publishes the number of round trips still in flight as
// `data-pending`, and the sibling's lands a microtask after the clicked one — so
// `findAllByTestId('hl')` returns while the sibling is still on the wire, and a
// count assertion made then is green for a reason unrelated to its claim. Wait
// for the card to say it has settled instead.
async function settled() {
  await waitFor(() => expect(screen.getByTestId('source-body').dataset.pending).toBe('0'));
}
// Drains the promise chain and then Svelte's own flush, for the one test that
// asserts something did NOT appear and therefore cannot wait on the DOM.
async function flush() {
  for (let i = 0; i < 10; i += 1) await Promise.resolve();
  await tick();
}

beforeEach(() => {
  invoke.mockReset(); // drops the implementation too — reinstall it below
  tree = Promise.resolve(emptyListing);
  sources = {};
  invoke.mockImplementation((cmd: string, args: { chunkId: number }) => {
    if (cmd === 'list_tree') return tree;
    if (cmd === 'source_around') {
      const found = sources[args.chunkId];
      // Loud, never `undefined`: a silent one would make the card read fields off
      // nothing and the failure would name the component, not the missing mock.
      if (!found) throw new Error(`no mock for chunkId ${args.chunkId}`);
      return found;
    }
    return Promise.resolve();
  });
});

// N1 (re-review 1): `locale` is a module-level store shared by every test in
// this file. The labels test below switches it, and an in-test restore is
// skipped when an assertion fails first — leaving the next test to fail for a
// reason that has nothing to do with what it claims. Restore unconditionally.
afterEach(() => setLocale('en'));

test('idle shows no cards at all (state A is the bare line)', () => {
  render(Cards, { state: { kind: 'idle' }, query: '' });
  expect(screen.queryByTestId('card-tree')).toBeNull();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

test('generated shows tree and centre; source waits for a click', () => {
  render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });
  expect(screen.getByTestId('card-tree')).toBeTruthy();
  expect(screen.getByTestId('card-centre')).toBeTruthy();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

// Controller ruling A: state F (refused) also draws no cards. The plan's
// illustrated tests only covered A and B — a `state.kind !== 'idle'` guard
// would pass both of those and still wrongly draw cards here.
test('refused shows no cards at all (state F)', () => {
  render(Cards, { state: stateFromAnswer('nothing indexed', refusedNoCandidates), query: 'nothing indexed' });
  expect(screen.queryByTestId('card-tree')).toBeNull();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

// I1 (review round 1): idle/generated/refused pinned three of `LauncherState`'s
// six variants; `Cards.svelte` branches on `state.kind`, so the other three
// — inFlight (D), citationsOnly (E), error — were free to draw cards without
// reddening anything. citationsOnly matters most: it is the line Task 9 will
// edit next, and a guard mistakenly written as
// `state.kind === 'generated' || state.kind === 'citationsOnly'` is the likely
// one. All three below must independently redden under the reviewer's mutant
// (`state.kind !== 'idle' && state.kind !== 'refused'`).
test('inFlight shows no cards at all (state D)', () => {
  render(Cards, { state: { kind: 'inFlight', query: 'q' }, query: 'q' });
  expect(screen.queryByTestId('card-tree')).toBeNull();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

test('citationsOnly shows no cards at all (state E is out of scope here)', () => {
  render(Cards, { state: stateFromAnswer('q', citationsOnly), query: 'q' });
  expect(screen.queryByTestId('card-tree')).toBeNull();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

test('error shows no cards at all', () => {
  render(Cards, { state: { kind: 'error', reason: 'blank' }, query: '' });
  expect(screen.queryByTestId('card-tree')).toBeNull();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

// I2 (review round 1): no test asserted a `Cards` aria-label, so presence, the
// right catalogue key on the right section, and $locale-reactivity (D130 /
// the Codex ④ defect on PR #20) all held by inspection only. Lifted from the
// reviewer's probe F.
test('card labels come from the catalogue, on the right section, and follow a live language switch', async () => {
  setLocale('en'); // seed, do not inherit: an earlier sibling switching the language must not decide this test
  render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });
  expect(screen.getByTestId('card-tree').getAttribute('aria-label')).toBe('Tree');
  expect(screen.getByTestId('card-centre').getAttribute('aria-label')).toBe('Answer');

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('card-tree').getAttribute('aria-label')).toBe('Дерево');
  expect(screen.getByTestId('card-centre').getAttribute('aria-label')).toBe('Відповідь');

  setLocale('en'); // the switch back is part of the claim, not the cleanup — afterEach owns that
  await tick();
  expect(screen.getByTestId('card-tree').getAttribute('aria-label')).toBe('Tree');
  expect(screen.getByTestId('card-centre').getAttribute('aria-label')).toBe('Answer');
});

// --- the composition (Task 8b) ----------------------------------------------

// Spec §13 (`…interface-design.md:335-336`): the one mapping this whole PR is
// for, and the only test in 6b that crosses all three cards. Everything else in
// this branch is green with the wire between them missing.
//
// ⚠️ Ruling AD: the fixture's two citations share `documentId: 'doc-1'`, so this
// click produces TWO round trips and TWO highlights (Decision 4). Both are
// mocked and both are asserted, and the clicked one is identified by
// `data-primary`, never by the order the calls went out in — nothing fixes that
// order, and a test written for one call would punish the correct
// implementation.
test('clicking [N] selects the cited file on the left and highlights it on the right', async () => {
  mockTree(oneRootTwoFolders);
  mockSourceFor({ 43: excerptSpanB, 42: excerptSpanA }); // both citations live in doc-1

  render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });

  // Before the click: no source card at all (§7 — it appears on the click).
  expect(screen.queryByTestId('card-source')).toBeNull();

  await fireEvent.click(await screen.findByRole('button', { name: '[7]' }));

  expect(await screen.findByTestId('card-source')).toBeTruthy();
  await settled();

  // Ruling AE: the row is found by documentId, not by the citation's path string.
  await waitFor(() =>
    expect(screen.getByTestId('tree-file-doc-1').getAttribute('aria-current')).toBe('true'));
  // Both directions: the sibling row in the same folder is on screen and is NOT marked.
  expect(screen.getByTestId('tree-file-doc-2').getAttribute('aria-current')).toBeNull();

  // Two highlights, because both citations are in this document (Decision 4) —
  // and the clicked one is the primary. DOM order is the painter's (by offset),
  // not the round trips'.
  const marks = screen.getAllByTestId('hl');
  expect(marks.map((m) => m.textContent)).toEqual([SPAN_A_TEXT, SPAN_B_TEXT]);
  expect(marks.filter((m) => m.dataset.primary === 'true').map((m) => m.textContent))
    .toEqual([SPAN_B_TEXT]); // the citation that was clicked, anchor 7 / chunk 43
  expect(sourceAroundCalls().map((c) => c[1].chunkId).sort((a, b) => a - b)).toEqual([42, 43]);
});

test('a new answer clears the previous selection instead of leaving a stale excerpt', async () => {
  mockTree(oneRootTwoFolders);
  mockSourceFor({ 43: excerptSpanB, 42: excerptSpanA });
  const { rerender } = render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });

  await fireEvent.click(await screen.findByRole('button', { name: '[7]' }));
  await screen.findByTestId('card-source');
  await settled();
  expect(screen.getAllByTestId('hl')).toHaveLength(2); // the first answer's excerpt really is on screen

  // ⚠️ The second state MUST be one that renders cards. Going to `inFlight`
  // proves nothing: no state renders the source card there, so the assertion
  // is satisfied without any reset at all and can never go red.
  await rerender({ state: stateFromAnswer('q2', generatedOther), query: 'q2' });
  await tick();

  expect(screen.queryByTestId('card-source')).toBeNull();
  expect(screen.queryAllByTestId('hl')).toHaveLength(0);
  expect(screen.getByTestId('answer-body').textContent).toContain('second answer');
  // The left card lets go too: with no selection nothing is open by default, so
  // the folder the first answer's citation had opened is shut again.
  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('false');
});

// Fixture question, state 1 of 2: the tree answers on its own schedule, and a
// person can click a citation before it has. Nothing else in this branch builds
// a selection over an empty left card.
test('a click made before the tree has answered marks the row when the listing arrives', async () => {
  const listing = mockTreePending();
  mockSourceFor({ 43: excerptSpanB, 42: excerptSpanA });

  render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });

  await fireEvent.click(await screen.findByRole('button', { name: '[7]' }));
  await settled();
  // The right card answered while the left one still had nothing to mark.
  expect(screen.getByTestId('card-source')).toBeTruthy();
  expect(screen.queryByTestId('tree-file-doc-1')).toBeNull();

  listing.resolve(oneRootTwoFolders);
  await waitFor(() =>
    expect(screen.getByTestId('tree-file-doc-1').getAttribute('aria-current')).toBe('true'));
  // The folder on the way to the cited file opened; the other one did not.
  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('false');
});

// Fixture question, state 2 of 2: the second answer lands while the source card
// is still on the wire for the first. The excerpt that arrives afterwards
// belongs to a card that is gone, and it must not paint anything — this is the
// "text the file no longer contains" shape, one component up from where
// `Source`'s own request counter guards it.
test('a second answer arriving mid-fetch leaves no excerpt from the first', async () => {
  mockTree(oneRootTwoFolders);
  const clicked = deferred<SourceAround>();
  mockSourceFor({ 43: clicked.promise, 42: excerptSpanA });
  const { rerender } = render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });

  await fireEvent.click(await screen.findByRole('button', { name: '[7]' }));
  expect(screen.getByTestId('source-loading')).toBeTruthy();
  expect(screen.getByTestId('source-body').dataset.pending).toBe('1'); // still on the wire

  await rerender({ state: stateFromAnswer('q2', generatedOther), query: 'q2' });
  expect(screen.queryByTestId('card-source')).toBeNull();

  clicked.resolve(excerptSpanB); // the answer for a card that is no longer on screen
  await flush();

  expect(screen.queryByTestId('card-source')).toBeNull();
  expect(screen.queryAllByTestId('hl')).toHaveLength(0);
  expect(screen.getByTestId('answer-body').textContent).toContain('second answer');
});

// 🔴 Ruling AC, the half that is easy to lose: only the answer-and-source pair is
// keyed. `state` changes twice per question (`Launcher.svelte:41,44`), so a keyed
// tree refetches and snaps shut every folder the person opened — in the card
// whose whole purpose is browsing folder neighbours (§7). Task 7 built `Tree` on
// that promise; this is where it is kept.
//
// Two tests, not one with two assertions: a keyed tree breaks both the refetch
// and the folder, and whichever assertion came first would be the only one the
// mutant ever reached — leaving the other green on its neighbour's defence.
test('a new answer does not refetch the tree', async () => {
  mockTree(oneRootTwoFolders);
  mockSourceFor({ 43: excerptSpanB, 42: excerptSpanA });
  const { rerender } = render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });

  await screen.findByTestId('tree-folder-archive'); // the first listing really arrived
  expect(listTreeCalls()).toHaveLength(1);

  await rerender({ state: stateFromAnswer('q2', generatedOther), query: 'q2' });
  await tick();

  expect(listTreeCalls()).toHaveLength(1); // a keyed tree would have asked twice
});

test('a new answer does not shut a hand-opened folder', async () => {
  mockTree(oneRootTwoFolders);
  mockSourceFor({ 43: excerptSpanB, 42: excerptSpanA });
  const { rerender } = render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });

  await fireEvent.click(await screen.findByTestId('tree-folder-archive')); // opened by hand
  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');

  await rerender({ state: stateFromAnswer('q2', generatedOther), query: 'q2' });
  await tick();

  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
});

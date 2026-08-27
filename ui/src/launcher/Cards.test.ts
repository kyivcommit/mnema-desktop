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
import type { AskAnswer, SourceAround, TreeListing } from '../lib/ipc';
import type { LauncherState } from './state';

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

// 🔴 C1. The ONLY transition `runSearch` can make between two answers
// (`Launcher.svelte:39-45`): it clears the echo, goes to `inFlight`, and only
// then does the next answer land. A `rerender` straight from one generated state
// to the next is a sequence the product cannot perform, so a property asserted
// across it is not a property of the product — every claim in this file about
// "a new answer" goes through here.
type Rerender = (props: { state: LauncherState; query: string }) => Promise<void>;
async function askAgain(rerender: Rerender, query: string, answer: AskAnswer) {
  await rerender({ state: { kind: 'inFlight', query }, query: '' }); // `runSearch` blanks the echo first
  await tick();
  await rerender({ state: stateFromAnswer(query, answer), query });
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

// 🔴 The whole tree gate rests on this one: `idle` is the ONLY row in §7's state
// table described as «лише рядок пошуку» — only the search line. Every other
// state keeps the tree, so this test is the single negative the rule stands on.
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

test('refused keeps the tree and draws neither answer nor source (state F)', () => {
  render(Cards, { state: stateFromAnswer('nothing indexed', refusedNoCandidates), query: 'nothing indexed' });
  expect(screen.getByTestId('card-tree')).toBeTruthy();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

// The six tests above and below cover all six `LauncherState` variants against
// TWO independent gates, and each one must be readable on its own:
//
//   card-tree               — every state except `idle` (ruling I-B)
//   card-centre, card-source — `generated` only
//
// I1 (review round 1) is why all six exist: `Cards.svelte` branches on
// `state.kind`, so any variant with no test of its own was free to draw the
// wrong cards without reddening anything. citationsOnly matters most — it is
// the line Task 9 will edit next, and a guard mistakenly written as
// `kind === 'generated' || kind === 'citationsOnly'` is the likely one.
//
// 🔴 The prose here used to say "all three below must redden under
// `state.kind !== 'idle' && state.kind !== 'refused'`", which the C1 amendment
// silently made false for state D — an edit in the test devaluing the sentence
// three lines above it. What is true now: a mutant that widens the ANSWER gate
// reddens the `card-centre` negative in D, E, F and error independently, and a
// mutant that widens or narrows the TREE gate reddens `idle` on one side and
// the other five on the other.
// 🔴 Controller ruling C1 (fix round 1) amends this one. The tree's content is
// the INDEX, not the answer, so it stays on screen while the next answer is
// fetched; §7's state D row describes the search line and never asks for the
// cards to be torn down. Ruling A's purpose here — stopping an over-broad guard
// from drawing the ANSWER in D/E/F — is served in full by the two negatives
// below, which is all this test ever meant.
test('inFlight keeps the tree and draws neither answer nor source (state D)', () => {
  render(Cards, { state: { kind: 'inFlight', query: 'q' }, query: 'q' });
  expect(screen.getByTestId('card-tree')).toBeTruthy();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

test('citationsOnly keeps the tree and draws neither answer nor source (state E is Task 9\'s)', () => {
  render(Cards, { state: stateFromAnswer('q', citationsOnly), query: 'q' });
  expect(screen.getByTestId('card-tree')).toBeTruthy();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

// `error` keeps the tree deliberately (ruling I-B): `askFailed` is an answer
// state, and it is exactly the moment a person retries — losing their folders on
// the failure they are retrying is C1's defect one gate over.
test('error keeps the tree and draws neither answer nor source', () => {
  render(Cards, { state: { kind: 'error', reason: 'blank' }, query: '' });
  expect(screen.getByTestId('card-tree')).toBeTruthy();
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

// M1 (review round 1): `card-source` is the one section this PR creates, and it
// was the one label no test pinned — it needs a click to exist, so it could not
// ride along with the two above. Same claim, same live-switch shape.
test("the source card's label comes from the catalogue and follows a live language switch", async () => {
  setLocale('en'); // seed, do not inherit
  mockTree(oneRootTwoFolders);
  mockSourceFor({ 43: excerptSpanB, 42: excerptSpanA });
  render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });

  await fireEvent.click(await screen.findByRole('button', { name: '[7]' }));
  await settled();
  expect(screen.getByTestId('card-source').getAttribute('aria-label')).toBe('Source');

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('card-source').getAttribute('aria-label')).toBe('Джерело');

  setLocale('en'); // the switch back is part of the claim, not the cleanup
  await tick();
  expect(screen.getByTestId('card-source').getAttribute('aria-label')).toBe('Source');
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
  await askAgain(rerender, 'q2', generatedOther);

  expect(screen.queryByTestId('card-source')).toBeNull();
  expect(screen.queryAllByTestId('hl')).toHaveLength(0);
  expect(screen.getByTestId('answer-body').textContent).toContain('second answer');
  // The left card lets go too: with no selection nothing is open by default, so
  // the folder the first answer's citation had opened is shut again.
  expect(screen.getByTestId('tree-folder-notes').getAttribute('aria-expanded')).toBe('false');
});

// The test above is the PRODUCT's claim and goes through state D, where the
// `{#if}` gate alone would already destroy the selection. This one is the
// `{#key}`'s own claim, and its name says the sequence out loud so it can never
// be read as a promise about the launcher: `Cards` resets on a new answer by
// itself, without depending on the state machine above it passing through
// `inFlight` first. Task 9 adds a second card-drawing state to that machine.
test('Cards clears the selection on a new answer even without passing through inFlight', async () => {
  mockTree(oneRootTwoFolders);
  mockSourceFor({ 43: excerptSpanB, 42: excerptSpanA });
  const { rerender } = render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });

  await fireEvent.click(await screen.findByRole('button', { name: '[7]' }));
  await screen.findByTestId('card-source');
  await settled();

  await rerender({ state: stateFromAnswer('q2', generatedOther), query: 'q2' });
  await tick();

  expect(screen.queryByTestId('card-source')).toBeNull();
  expect(screen.queryAllByTestId('hl')).toHaveLength(0);
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

  await askAgain(rerender, 'q2', generatedOther);
  expect(screen.queryByTestId('card-source')).toBeNull();

  clicked.resolve(excerptSpanB); // the answer for a card that is no longer on screen
  await flush();

  expect(screen.queryByTestId('card-source')).toBeNull();
  expect(screen.queryAllByTestId('hl')).toHaveLength(0);
  expect(screen.getByTestId('answer-body').textContent).toContain('second answer');
});

// The tag's SECOND witness, and it is a different transition from the one above
// through a different gate (ruling I-B put the tree on screen in state F too).
// `Selection` is destroyed by the `{#if}` without ever reporting `null` on its
// way out, so an untagged mirror keeps the previous answer's row marked under a
// refusal that found nothing — a file marked as cited by an answer that cites
// nothing.
test('the tree lets go of the mark when the next answer is a refusal', async () => {
  mockTree(oneRootTwoFolders);
  mockSourceFor({ 43: excerptSpanB, 42: excerptSpanA });
  const { rerender } = render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });

  await fireEvent.click(await screen.findByTestId('tree-folder-notes')); // opened by hand, before any selection
  await fireEvent.click(await screen.findByRole('button', { name: '[7]' }));
  await settled();
  expect(screen.getByTestId('tree-file-doc-1').getAttribute('aria-current')).toBe('true');

  await rerender({ state: stateFromAnswer('q2', refusedNoCandidates), query: 'q2' });
  await tick();

  expect(screen.getByTestId('card-tree')).toBeTruthy();
  expect(screen.getByTestId('tree-file-doc-1')).toBeTruthy(); // the row is still on screen
  expect(screen.getByTestId('tree-file-doc-1').getAttribute('aria-current')).toBeNull();
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

  await askAgain(rerender, 'q2', generatedOther);

  // A keyed tree asks twice — and so does a tree drawn only for a generated
  // answer, because state D unmounts it (C1).
  expect(listTreeCalls()).toHaveLength(1);
});

test('a new answer does not shut a hand-opened folder', async () => {
  mockTree(oneRootTwoFolders);
  mockSourceFor({ 43: excerptSpanB, 42: excerptSpanA });
  const { rerender } = render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });

  await fireEvent.click(await screen.findByTestId('tree-folder-archive')); // opened by hand
  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');

  await askAgain(rerender, 'q2', generatedOther);

  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
});

// 🔴 I1 — the other half of C1, and the reason it could not be taken alone. Once
// the tree survives state D, `Cards`'s copy of the selection outlives the answer
// that produced it: `Selection` is unmounted by the `{#if}` and never reports
// `null` on the way out, so a plain mirror keeps the previous answer's row
// marked for the whole length of the next ask. The folder is opened by hand
// first so the row is on screen either way — this is a claim about the MARK.
test('the tree keeps its rows but lets go of the mark while the next answer is in flight', async () => {
  mockTree(oneRootTwoFolders);
  mockSourceFor({ 43: excerptSpanB, 42: excerptSpanA });
  const { rerender } = render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });

  await fireEvent.click(await screen.findByTestId('tree-folder-notes')); // opened by hand, before any selection
  await fireEvent.click(await screen.findByRole('button', { name: '[7]' }));
  await settled();
  expect(screen.getByTestId('tree-file-doc-1').getAttribute('aria-current')).toBe('true');

  await rerender({ state: { kind: 'inFlight', query: 'q2' }, query: '' });
  await tick();

  expect(screen.getByTestId('card-tree')).toBeTruthy();
  expect(screen.getByTestId('tree-file-doc-1')).toBeTruthy(); // the row is still on screen
  expect(screen.getByTestId('tree-file-doc-1').getAttribute('aria-current')).toBeNull();
});

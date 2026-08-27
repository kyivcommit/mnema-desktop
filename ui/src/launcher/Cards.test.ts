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
  citationsOnlyOne,
  citationsOnlySameDocument,
  emptyCitationsOnly,
  oneRootTwoFolders,
  emptyListing,
  excerptSpanA,
  excerptSpanB,
  excerptDocTwo,
  SPAN_A_TEXT,
  SPAN_B_TEXT,
} from '../lib/fixtures';
import { setLocale } from '../i18n';
import { messages } from '../i18n/catalog';
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
//   card-centre             — `generated` AND `citationsOnly` (Task 9)
//   card-source             — those two, and only after a click
//
// I1 (review round 1) is why all six exist: `Cards.svelte` branches on
// `state.kind`, so any variant with no test of its own was free to draw the
// wrong cards without reddening anything.
//
// 🔴 The prose here used to say "all three below must redden under
// `state.kind !== 'idle' && state.kind !== 'refused'`", which the C1 amendment
// silently made false for state D — an edit in the test devaluing the sentence
// three lines above it. It then said the answer gate was `generated` only,
// which Task 9 made false in turn. What is true now: a mutant that widens the
// ANSWER gate past those two reddens the `card-centre` negative in D, F and
// error independently, one that narrows it back to `generated` reddens state E
// below, and a mutant that widens or narrows the TREE gate reddens `idle` on
// one side and the other five on the other.
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

test('citationsOnly keeps the tree and draws the centre card; source waits for a click (state E)', () => {
  render(Cards, { state: stateFromAnswer('q', citationsOnly), query: 'q' });
  expect(screen.getByTestId('card-tree')).toBeTruthy();
  expect(screen.getByTestId('card-centre')).toBeTruthy();
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

// --- state E: CitationsOnly (Task 9) ----------------------------------------

// 🔴 Ruling AF, and it is the reason this test does NOT look for the word
// "key". `bridge.rs:536-540` opens this branch whenever `chat_readiness` is not
// `Ready`, and `bridge.rs:293-302` gives that three non-ready variants —
// `NoModel`, `NoKey`, `KeyUnreadable`. The wire shape at `bridge.rs:476-480` is
// `{ citations, text, content }` and carries NONE of them, so a banner naming a
// missing key is false in two cases out of three and the card has no way to
// know which it is in. `content` does not rescue it: `ContentArmReport`
// (`ipc.ts:21-27`) reports the content SEARCH arm, filled by `retrieve` before
// readiness is consulted — a different question with a different answer.
//
// 🔴 Review I3, and it is why the assertions below are `toBe` and not the regex
// alone. A stem list is an OPEN enumeration: the first round defended Ukrainian
// with `/…|налаштув/i`, and «Генерування недоступне: чат не налаштовано» — a
// cause, false in two of the three readiness variants — walked past it one
// letter short of the stem, with the whole suite green. The round's own probe
// had happened to pick a string that matched a stem, so it measured the
// formulation and not the class (`definition-by-enumeration`, and the closed
// set here is "this exact sentence", not "words that name a cause").
//
// So each banner string is pinned exactly, in BOTH locales: any rewording is
// then a deliberate edit to this file rather than a silent pass. The regex pair
// stays as a second, weaker net — it is what catches a cause added to a string
// somebody remembered to update here too.
const CAUSE = /key|provider|model|credential|setting|ключ|провайдер|модел|обліков|налаштув/i;

test('the state E banner says generation is unavailable and names no cause (Ruling AF)', () => {
  setLocale('en'); // seed, do not inherit
  render(Cards, { state: stateFromAnswer('q', citationsOnly), query: 'q' });

  // 🔴 Re-review RI1, and it is FIRST because it is what a screen reader acts
  // on: `role="status"` is what makes this a live region, so "generation is
  // unavailable" is announced when the card appears rather than sitting there
  // silently. Round 1 defended it only by accident — the test located the
  // banner with `getByRole('status')` — and swapping to the testid locator, on
  // the strength of a note about a DIFFERENT test's ambiguity after a click,
  // dropped the guard with the suite green. A locator is not an assertion:
  // reaching an element by a property is not a claim that it has it.
  expect(screen.getByTestId('citations-banner').getAttribute('role')).toBe('status');

  expect(screen.getByTestId('citations-banner').textContent)
    .toBe('Generation is unavailable. The search found 2 passages.');

  // Both locales, both banner forms, closed: the rendered assertion above can
  // only ever reach one of the four, and it reaches a FORMATTED string while
  // these reach the ICU pattern behind it (re-review RM1).
  expect(messages.en.citations_only_banner)
    .toBe('Generation is unavailable. The search found {count, plural, one {# passage} other {# passages}}.');
  expect(messages.uk.citations_only_banner)
    .toBe('Генерування недоступне. Пошук знайшов {count, plural, one {# уривок} few {# уривки} many {# уривків} other {# уривка}}.');
  expect(messages.en.citations_only_banner_empty).toBe('Generation is unavailable.');
  expect(messages.uk.citations_only_banner_empty).toBe('Генерування недоступне.');

  for (const l of ['uk', 'en'] as const) {
    expect(messages[l].citations_only_banner).not.toMatch(CAUSE);
    expect(messages[l].citations_only_banner_empty).not.toMatch(CAUSE);
  }
});

// 🔴 Ruling AH. A `Hit` has no `anchor` (`ipc.ts:33-42`), so the rank is the
// row's own ordinal and its testid is deliberately NOT derivable from any field
// of the passage. The `toEqual` on the ids is what makes that falsifiable: a
// mutant keying the testid on `chunkId` would still produce two `rank-` rows
// and satisfy a bare length assertion, but it would produce `rank-7`/`rank-9`.
test('state E ranks the passages as neutral ordinals, with no answer prose and no anchors', () => {
  setLocale('en'); // seed, do not inherit
  render(Cards, { state: stateFromAnswer('q', citationsOnly), query: 'q' });

  const rows = screen.getAllByTestId(/^rank-/);
  expect(rows.map((r) => r.getAttribute('data-testid'))).toEqual(['rank-1', 'rank-2']);
  // Labelled by the shared Decision 1 rule (`i18n/label.ts:18`), which already
  // takes a `Hit` — the second row exercises its path-plus-locator branch.
  // Scoped to the label span since P1: the row also holds the rank and the
  // passage text now, and this assertion is about the LABEL rule. What the
  // whole row shows is `state E shows the passages themselves` above — the
  // claim this one could not make, which is how the paths-only card shipped.
  expect(rows.map((r) => r.querySelector('[data-testid="passage-label"]')!.textContent))
    .toEqual(['notes/a.md', 'notes/b.md · p. 2']);
  // No answer prose and no anchor buttons: state E has neither to show.
  expect(screen.queryByTestId('answer-body')).toBeNull();
  expect(screen.queryAllByTestId(/^preview-/)).toHaveLength(0);
  // Review I1's other side: the "nothing matched" sentence belongs to the empty
  // branch alone, and a card holding both would contradict itself here too.
  expect(screen.queryByTestId('citations-empty')).toBeNull();
});

// 🔴 Owner review on PR #24, P1 — and the reason it is written against the
// card's WHOLE VISIBLE TEXT rather than against ids. The suite this branch
// shipped with asserted the `rank-` ids, their count and their labels, and every
// one of those assertions is satisfied by a card that shows nothing but file
// paths: `notes/a.md`, `notes/b.md · p. 2`. §7 row E says «банер + самі уривки
// (нейтральні ранги)» — the passages themselves — so state E's only answer
// content was missing and no test could see it. The claim here is what a person
// reads, top to bottom, in order: the banner, then each passage's rank, its
// text, and where it came from.
const rowText = (el: HTMLElement) => (el.textContent ?? '').replace(/\s+/g, ' ').trim();

test('state E shows the passages themselves, not a list of paths', () => {
  setLocale('en'); // seed, do not inherit
  render(Cards, { state: stateFromAnswer('q', citationsOnly), query: 'q' });

  expect(screen.getByTestId('citations-banner').textContent)
    .toBe('Generation is unavailable. The search found 2 passages.');
  // The WHOLE of each row, in order — not a `toContain`, and not a count. A
  // card that dropped the passage text, reordered the parts or invented a
  // paragraph number all redden here; the shipped card reads `notes/a.md`.
  expect(screen.getAllByTestId(/^rank-/).map(rowText)).toEqual([
    '1 A bare passage. notes/a.md',
    '2 Another file entirely. notes/b.md · p. 2',
  ]);
});

// The same content, addressed per row, so a failure says WHICH of the three
// parts moved and which row it moved in — the whole-card assertion above proves
// the person sees them, this one proves each belongs to its own passage. The
// rank is read from the row's own element, not from the testid: a testid is not
// on screen, and the finding was precisely that.
test('each state E row carries its own rank, passage text and label', () => {
  setLocale('en'); // seed, do not inherit
  render(Cards, { state: stateFromAnswer('q', citationsOnly), query: 'q' });

  const part = (row: HTMLElement, id: string) =>
    row.querySelector(`[data-testid="${id}"]`)!.textContent;
  const rows = screen.getAllByTestId(/^rank-/);

  expect(rows.map((r) => part(r, 'passage-rank'))).toEqual(['1', '2']);
  expect(rows.map((r) => part(r, 'passage-text')))
    .toEqual(['A bare passage.', 'Another file entirely.']);
  expect(rows.map((r) => part(r, 'passage-label')))
    .toEqual(['notes/a.md', 'notes/b.md · p. 2']);
});

// 🔴 Ruling AK, and review I1. Zero hits is an ANSWER — the search ran and
// found nothing — so the centre card says so rather than rendering an empty
// list nobody can read.
//
// The assertion is on the WHOLE card text, not `toContain`, and that is the
// finding. Both AK tests used to be satisfied while the card read "These are
// the passages the search found. No passages matched this query." — a promise
// about what follows, denied by the next sentence, and starker in Ukrainian
// where «Нижче — уривки» points at nothing. That is Ruling AF's own theme one
// branch over: the card asserting what is not so. `toContain` cannot see it;
// this can, and it reddens the moment the passage-bearing clause can appear
// beside the empty one.
const centreText = (): string =>
  (screen.getByTestId('card-centre').textContent ?? '').replace(/\s+/g, ' ').trim();

test('zero passages is an answer, and the card does not also promise passages', () => {
  setLocale('en'); // seed, do not inherit
  render(Cards, { state: stateFromAnswer('q', emptyCitationsOnly), query: 'q' });

  expect(centreText()).toBe('Generation is unavailable. No passages matched this query.');
  expect(screen.queryAllByTestId(/^rank-/)).toHaveLength(0);
});

// The other half of Ruling AK, and the half a single-locale test cannot see:
// three different facts get three different sentences. "Nothing is indexed yet"
// (the tree), "the source could not be loaded" (the right card) and "the search
// found no passages" are not interchangeable, and Ruling N settled that once
// already for the tree.
test('the zero-passages sentence is its own, in both locales (Ruling AK)', async () => {
  setLocale('uk');
  render(Cards, { state: stateFromAnswer('q', emptyCitationsOnly), query: 'q' });
  await tick();

  expect(centreText())
    .toBe('Генерування недоступне. Жоден уривок не відповідає цьому запиту.');
  for (const l of ['uk', 'en'] as const) {
    expect(messages[l].citations_only_empty).not.toBe(messages[l].tree_empty);
    expect(messages[l].citations_only_empty).not.toBe(messages[l].source_failed);
  }
});

// 🔴 Ruling AJ: a passage click is a citation click. Same `Selection`, same
// `{#key}`, same state tag — `Source` already takes `AskCitation | Hit`
// (`Source.svelte:83-86`), so a `Hit` needs no adaptation and there is no
// second selection path to keep in step.
//
// The second row is `doc-2` while the first is `doc-1`, so Ruling U's filter
// must drop the first before any call: ONE round trip, for chunk 9. Anchored on
// `data-pending === '0'`, which only a resolved answer writes — `findByTestId`
// alone returns while the (absent) sibling would still be on the wire.
test('clicking a passage from the second document fetches THAT document, and no sibling from the first', async () => {
  setLocale('en'); // seed, do not inherit: the header is asserted below
  mockTree(oneRootTwoFolders);
  mockSourceFor({ 9: excerptDocTwo });

  render(Cards, { state: stateFromAnswer('q', citationsOnly), query: 'q' });
  await fireEvent.click(screen.getByTestId('rank-2'));

  expect(await screen.findByTestId('card-source')).toBeTruthy();
  await settled();
  expect(sourceAroundCalls()).toHaveLength(1);
  expect(sourceAroundCalls()[0][1].chunkId).toBe(9);
  // Positive, not "not null": the card really is showing the clicked passage's
  // own file, so a mismatch badge over an empty body cannot satisfy this.
  expect(screen.getByTestId('source-header').textContent).toBe('notes/b.md · p. 2');
  expect(screen.getAllByTestId('hl').map((m) => m.textContent)).toEqual(['paragraph']);
});

// 🔴 The fixture question. `citationsOnly` holds its two passages in two
// DIFFERENT documents, so the sibling branch of Ruling U never fires from state
// E and the whole "a passage has a sibling in its own file" path was a state no
// fixture built. `citationsOnlySameDocument` builds it: both in `doc-1`,
// distinct occurrences (chunk 7 ord 0, chunk 8 ord 1), so the click produces
// TWO round trips and the second one's span paints beside the clicked one's
// (Decision 4). The clicked passage is identified by `data-primary`, never by
// the order the calls went out in.
test('two passages in one document: a click asks for the sibling too and paints both', async () => {
  mockTree(oneRootTwoFolders);
  mockSourceFor({ 7: excerptSpanA, 8: excerptSpanB }); // both doc-1

  render(Cards, { state: stateFromAnswer('q', citationsOnlySameDocument), query: 'q' });
  await fireEvent.click(screen.getByTestId('rank-1'));

  expect(await screen.findByTestId('card-source')).toBeTruthy();
  await settled();
  expect(sourceAroundCalls().map((c) => c[1].chunkId).sort((a, b) => a - b)).toEqual([7, 8]);

  const marks = screen.getAllByTestId('hl');
  expect(marks.map((m) => m.textContent)).toEqual([SPAN_A_TEXT, SPAN_B_TEXT]);
  expect(marks.filter((m) => m.dataset.primary === 'true').map((m) => m.textContent))
    .toEqual([SPAN_A_TEXT]); // the passage that was clicked, rank 1 / chunk 7

  // The left card marks the cited file by documentId (Ruling AE), exactly as it
  // does for a generated answer — state E goes through the same report.
  await waitFor(() =>
    expect(screen.getByTestId('tree-file-doc-1').getAttribute('aria-current')).toBe('true'));
  expect(screen.getByTestId('tree-file-doc-2').getAttribute('aria-current')).toBeNull();
});

// --- state E: D130, one guard per test (review I2) ---------------------------
//
// 🔴 Every `void $locale` in this branch was undefended: three probes removing
// them one at a time each left the full suite at 161 passed. The guards were
// correct and load-bearing, and nothing proved they were there — which is the
// same finding the reviews of Task 5 (I2) and Task 6 (I3) made about their own
// cards, so this is the third time the shape has been paid for.
//
// 🔴 THE RULE, in the form that survives being tested (re-review, and it is
// sharper than the "one test per guard" style maxim this round first wrote):
//
//   a live-switch test is scoped to the smallest element whose text the guard
//   ALONE decides, and every branch that renders a guarded string needs its own
//   fixture.
//
// These four are therefore not four assertions on one thing that could have
// been folded together. They are TWO FIXTURE STATES — `citationsOnly` and
// `emptyCitationsOnly` — times the guards each state can reach. The single
// card-wide alternative was built and measured: it misses `emptyText` outright,
// because `citationsOnly` never renders `citations-empty` and no assertion over
// that card can see the guard in any locale; and where it does catch, its
// failure text is unreadable — under the `ranks` mutation both sides of the
// diff truncate to the same string, so the red carries no information at all.
// Coverage forces the split before identification does.
//
// The switch is what these test, not the seed: `setLocale('uk')` BEFORE a render
// passes with or without the guard, because the first read happens after it
// either way. That is exactly how the guards stayed invisible. The `en → uk →
// en` round trip is part of each claim for the same reason — a one-way switch
// passes for a `$derived` that merely happened to recompute once.

test('state E passage labels follow a live language switch', async () => {
  setLocale('en'); // seed, do not inherit
  render(Cards, { state: stateFromAnswer('q', citationsOnly), query: 'q' });
  // The locator is the one that carries a translated part: `formatLocator`
  // renders `p. 2` in English and `с. 2` in Ukrainian, so this row moves and
  // `rank-1`'s bare path would not.
  //
  // Scoped to the label span, which is the smallest element this guard ALONE
  // decides — the rule this file states above. Since P1 the row also holds the
  // rank and the passage text, neither of which the locale touches, and a
  // whole-row assertion would make the failure text read as though they had
  // moved too.
  const label = () => screen.getByTestId('rank-2').querySelector('[data-testid="passage-label"]')!.textContent;
  expect(label()).toBe('notes/b.md · p. 2');

  setLocale('uk');
  await tick();
  expect(label()).toBe('notes/b.md · с. 2');

  setLocale('en'); // the switch back is part of the claim, not the cleanup
  await tick();
  expect(label()).toBe('notes/b.md · p. 2');
});

test('the state E banner follows a live language switch', async () => {
  setLocale('en'); // seed, do not inherit
  render(Cards, { state: stateFromAnswer('q', citationsOnly), query: 'q' });
  expect(screen.getByTestId('citations-banner').textContent)
    .toBe('Generation is unavailable. The search found 2 passages.');

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('citations-banner').textContent)
    .toBe('Генерування недоступне. Пошук знайшов 2 уривки.');

  setLocale('en'); // the switch back is part of the claim, not the cleanup
  await tick();
  expect(screen.getByTestId('citations-banner').textContent)
    .toBe('Generation is unavailable. The search found 2 passages.');
});

// Scoped to `citations-empty`, deliberately NOT to the whole card: the banner
// sits in the same card and has its own guard, so an assertion on
// `card-centre.textContent` here would fall to the banner's probe as well and
// neither test could name which guard it had caught.
test('the zero-passages sentence follows a live language switch', async () => {
  setLocale('en'); // seed, do not inherit
  render(Cards, { state: stateFromAnswer('q', emptyCitationsOnly), query: 'q' });
  expect(screen.getByTestId('citations-empty').textContent)
    .toBe('No passages matched this query.');

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('citations-empty').textContent)
    .toBe('Жоден уривок не відповідає цьому запиту.');

  setLocale('en'); // the switch back is part of the claim, not the cleanup
  await tick();
  expect(screen.getByTestId('citations-empty').textContent)
    .toBe('No passages matched this query.');
});

// 🔴 The controller's aria-label ruling. The region used to announce itself as
// "Answer"/«Відповідь» while the first sentence inside it said there is no
// answer — the window stating what is not so, moved into the layer where the
// person cannot see the contradiction and correct for it. `card-centre` and its
// `<section>` stay exactly where they were; only the NAME switches on the kind,
// so this is also the negative that stops the two labels being merged back.
test("the passages card's label comes from the catalogue and follows a live language switch", async () => {
  setLocale('en'); // seed, do not inherit
  render(Cards, { state: stateFromAnswer('q', citationsOnly), query: 'q' });
  expect(screen.getByTestId('card-centre').getAttribute('aria-label')).toBe('Passages');

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('card-centre').getAttribute('aria-label')).toBe('Уривки');

  setLocale('en'); // the switch back is part of the claim, not the cleanup
  await tick();
  expect(screen.getByTestId('card-centre').getAttribute('aria-label')).toBe('Passages');
  // Both directions: state B keeps its own name, so one label cannot serve both
  // (`Cards.test.ts` pins the generated side of this pair above).
  expect(messages.en.card_passages).not.toBe(messages.en.card_answer);
  expect(messages.uk.card_passages).not.toBe(messages.uk.card_answer);
});

// 🔴 Re-review RM1. The banner used to say "these are the passages" over a list
// of one. In English that is loose; in Ukrainian it is ungrammatical, and the
// language has three arms an integer count can reach, not two. Both states are
// rendered here rather than reasoned about — the contrast IS the claim, so it
// is one test: a banner hardcoded to either arm fails on the other half.
//
// `ASK_TOP_K` is 8 (`bridge.rs:496`), so a person can reach `one`, `few` and
// `many`; the arms themselves are pinned in `i18n.test.ts` at 1, 2 and 5, where
// a count needs no fixture. What this test adds is the wiring the catalogue
// cannot show: that the card passes its OWN passage count and not a constant.
test('the banner agrees in number with the passages it introduces', async () => {
  setLocale('en'); // seed, do not inherit
  const { rerender } = render(Cards, { state: stateFromAnswer('q', citationsOnlyOne), query: 'q' });
  expect(screen.getByTestId('citations-banner').textContent)
    .toBe('Generation is unavailable. The search found 1 passage.');

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('citations-banner').textContent)
    .toBe('Генерування недоступне. Пошук знайшов 1 уривок.');

  // The same card, two passages: Ukrainian moves `one` → `few`, and the noun
  // changes with it. A card passing a constant count cannot do this.
  await rerender({ state: stateFromAnswer('q', citationsOnly), query: 'q' });
  await tick();
  expect(screen.getByTestId('citations-banner').textContent)
    .toBe('Генерування недоступне. Пошук знайшов 2 уривки.');

  setLocale('en');
  await tick();
  expect(screen.getByTestId('citations-banner').textContent)
    .toBe('Generation is unavailable. The search found 2 passages.');
});

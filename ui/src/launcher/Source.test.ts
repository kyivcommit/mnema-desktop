import { render, screen, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import Source from './Source.svelte';
import {
  citationA,
  citationB,
  hitOtherDocument,
  excerptSpanA,
  excerptSpanB,
  excerptInAnotherBlock,
  generatedArchived,
  generatedNoPath,
  SHARED_BLOCK_ID,
  SECOND_BLOCK_ID,
  SPAN_A_TEXT,
  SPAN_B_TEXT,
} from '../lib/fixtures';
import { setLocale } from '../i18n';
import type { AskAnswer, AskCitation, Freshness, SourceAround } from '../lib/ipc';

type Excerpt = Extract<SourceAround, { kind: 'excerpt' }>;

// The house pattern for the bridge (`lib/ipc.test.ts:14-15`, `Tree.test.ts:19-20`):
// one shared spy, the module mocked once at file scope.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

// Mocks, not fixtures (Ruling L): they stay local to this file.
function mockSource(answer: SourceAround) {
  invoke.mockResolvedValue(answer);
}
// `source_around` is keyed by `chunkId`, so a sibling round trip can be given
// its OWN answer — which is the whole point of the three sibling tests.
function mockSourceFor(byChunkId: Record<number, SourceAround>) {
  invoke.mockImplementation((_cmd: string, args: { chunkId: number }) => {
    const found = byChunkId[args.chunkId];
    if (!found) throw new Error(`no mock for chunkId ${args.chunkId}`);
    return Promise.resolve(found);
  });
}
function sourceAroundCalls() {
  return invoke.mock.calls.filter(([cmd]) => cmd === 'source_around');
}
// Drains the promise chain the effect builds (clicked `.then` -> sibling
// `.then` -> `.finally`) and then Svelte's own flush. The two re-selection
// tests below assert that something did NOT happen, so they cannot wait on the
// DOM; they wait on the queue instead.
async function flush() {
  for (let i = 0; i < 10; i += 1) await Promise.resolve();
  await tick();
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

// 🔴 The anchor every sibling claim needs. A sibling round trip lands a
// microtask AFTER the clicked one, so `findAllByTestId('hl')` returns the
// moment the CLICKED mark exists — and an assertion that a sibling "did not
// paint" then runs against a card that has not heard from the sibling yet,
// green for a reason unrelated to what it claims. `data-pending` counts the
// round trips still in flight; the loading test below pins that it starts at 1,
// so this wait can never be satisfied by a card that never started.
async function settled() {
  await waitFor(() => expect(screen.getByTestId('source-body').dataset.pending).toBe('0'));
}

function marks() {
  return screen.getAllByTestId('hl');
}
function primaryTexts() {
  return marks()
    .filter((m) => m.dataset.primary === 'true')
    .map((m) => m.textContent);
}

// The two extra header branches live inside shipped `AskAnswer` fixtures; take
// them from there rather than minting literals that can drift from the wire type.
function firstCitation(answer: AskAnswer): AskCitation {
  if (answer.kind !== 'generated') throw new Error('fixture drifted');
  return answer.citations[0];
}

beforeEach(() => {
  invoke.mockReset();
});

// Mirrors Answer.test.ts / Tree.test.ts: `locale` is a module-level store shared
// by every test in this file, and an in-test restore is skipped when an
// assertion fails first. Restore unconditionally.
afterEach(() => setLocale('en'));

// --- the arithmetic ---------------------------------------------------------

// 🔴 Ruling R. The block below starts with an astral character, so its code
// point offsets and its UTF-16 offsets disagree from the second character on.
// This fixture is Cyrillic and therefore local (Ruling T): `lib/fixtures.ts` is
// read by `i18n/guard.test.ts`, a `.test.ts` is not.
const ASTRAL_BLOCK = '🌍 Ціна оцифрування становить чотири гривні.';
const excerptWithAstralPrefix: Excerpt = {
  ...excerptSpanA,
  blocks: [{ blockId: SHARED_BLOCK_ID, kind: 'paragraph', text: ASTRAL_BLOCK, pageNo: 1, readingOrder: 11 }],
  // len 16, blockStart 2 → code points [2, 18) = 'Ціна оцифрування'.
  // `text.slice(2, 18)` counts UTF-16 units and yields ' Ціна оцифруванн'.
  spans: [{ blockId: SHARED_BLOCK_ID, start: 5, end: 21, blockStart: 2 }],
};

test('the highlight is placed by code points, not UTF-16 units', async () => {
  mockSource(excerptWithAstralPrefix);
  render(Source, { selected: citationA, siblings: [citationA] });

  await settled();
  expect(screen.getByTestId('hl').textContent).toBe('Ціна оцифрування');
  // Both directions: the surrounding text is not swallowed by an off-by-one run.
  expect(screen.getByTestId('source-block').textContent).toBe(ASTRAL_BLOCK);
});

test('the highlight starts at blockStart, and start/end contribute only their length', async () => {
  mockSource(excerptSpanA); // blockStart 4, start 12, end 30 — deliberately unequal
  render(Source, { selected: citationA, siblings: [citationA] });

  await settled();
  // `[...text].slice(start, end)` would paint 'tion price is fixe' instead.
  expect(screen.getByTestId('hl').textContent).toBe(SPAN_A_TEXT);
});

// --- the siblings -----------------------------------------------------------

test('two citations in one paragraph paint two highlights, from two calls merged by blockId', async () => {
  mockSourceFor({ 42: excerptSpanA, 43: excerptSpanB }); // the same blockId in both
  render(Source, { selected: citationA, siblings: [citationA, citationB] });

  await settled();
  const found = marks();
  expect(found).toHaveLength(2);
  expect(sourceAroundCalls()).toHaveLength(2); // never a substring search
  expect(found.map((m) => m.textContent)).toEqual([SPAN_A_TEXT, SPAN_B_TEXT]);

  // The clicked one is distinguishable from the sibling — PR 10 styles them
  // apart — and it is THE CLICKED ONE, not merely one of the two.
  expect(primaryTexts()).toEqual([SPAN_A_TEXT]);

  // The sibling contributes spans and NOTHING else: its own window held a
  // block 13 the clicked window does not, and that block must not appear.
  expect(screen.getByTestId('source-body').textContent).not.toContain('A later paragraph');
});

test('a sibling from another document is not fetched and does not paint', async () => {
  mockSourceFor({ 42: excerptSpanA });
  // hitOtherDocument is 'doc-2'; the clicked citation is 'doc-1'.
  render(Source, { selected: citationA, siblings: [citationA, hitOtherDocument] });

  await settled();
  expect(marks()).toHaveLength(1);
  expect(sourceAroundCalls()).toHaveLength(1);
});

// 🔴 Ruling U. The pre-call filter above compares the CITATIONS. This one
// compares the answers: a sibling round trip happens seconds later and can come
// back naming a different document (`src-tauri/src/tree.rs:155-166` puts
// `documentId` on the excerpt for exactly this). Painting its spans would put a
// highlight on text from another file. Its positive control is the two-marks
// test above, which is the same fixture with the documentId left agreeing.
test('a sibling whose excerpt names another document is fetched and still paints nothing', async () => {
  mockSourceFor({ 42: excerptSpanA, 43: { ...excerptSpanB, documentId: 'doc-9' } });
  render(Source, { selected: citationA, siblings: [citationA, citationB] });

  await settled();
  expect(marks()).toHaveLength(1);
  // Both directions: it WAS asked (so this is not the pre-call filter firing),
  // and its span did not land.
  expect(sourceAroundCalls()).toHaveLength(2);
  expect(primaryTexts()).toEqual([SPAN_A_TEXT]);
  expect(screen.getByTestId('source-body').textContent).toContain(SPAN_B_TEXT); // as plain text
  expect(marks().map((m) => m.textContent)).not.toContain(SPAN_B_TEXT);
});

test('a sibling answering Gone contributes nothing and changes no verdict', async () => {
  mockSourceFor({ 42: excerptSpanA, 43: { kind: 'gone', reason: { kind: 'noSuchChunk' } } });
  render(Source, { selected: citationA, siblings: [citationA, citationB] });

  await settled();
  expect(marks()).toHaveLength(1);
  expect(sourceAroundCalls()).toHaveLength(2);
  // The clicked excerpt's verdict stands: a gone SIBLING is not a gone card.
  expect(screen.getByTestId('freshness').textContent).toBe('Up to date');
  expect(screen.getByTestId('source-body').textContent).toContain(SPAN_A_TEXT);
});

test('a sibling span for a block outside the window neither paints nor moves the ellipsis', async () => {
  // The two excerpts DISAGREE about hasMoreAfter, or the assertion below is
  // satisfied by two equal flags.
  mockSourceFor({
    42: { ...excerptSpanA, hasMoreAfter: false },
    43: { ...excerptInAnotherBlock, hasMoreAfter: true }, // blockId 99, not in A's window
  });
  render(Source, { selected: citationA, siblings: [citationA, citationB] });

  await settled();
  expect(marks()).toHaveLength(1);
  expect(sourceAroundCalls()).toHaveLength(2); // it was fetched; only its span was dropped
  // The clicked excerpt's flags rule: an implementation that ORed the answers
  // together would render the trailing ellipsis here.
  expect(screen.queryByTestId('more-after')).toBeNull();
  // And the leading one, which the clicked excerpt DOES set, still appears —
  // so "no ellipsis" is not simply "no ellipses at all".
  expect(screen.getByTestId('more-before')).toBeTruthy();
  // The block outside the window is not appended to the card either.
  expect(screen.getByTestId('source-body').textContent).not.toContain('outside the clicked window');
});

// 🔴 Ruling V. Decision 4 unions spans per block, so spans that touch or overlap
// become ONE mark — a branch the plan's deliberately-apart fixture never
// reaches, and the moment it merges `data-primary` has a question. The rule: a
// merged run is primary when it contains the CLICKED citation's span. Block 12's
// merged run holds only sibling spans, so it must NOT be primary.
test('touching spans merge into one mark, and only the run holding the clicked span is primary', async () => {
  const merging: Excerpt = {
    ...excerptSpanB,
    spans: [
      { blockId: SHARED_BLOCK_ID, start: 5, end: 14, blockStart: 22 }, // len 9 → [22, 31), touches [4, 22)
      { blockId: SECOND_BLOCK_ID, start: 5, end: 14, blockStart: 0 }, // len 9 → [0, 9)
      { blockId: SECOND_BLOCK_ID, start: 5, end: 15, blockStart: 9 }, // len 10 → [9, 19), touches [0, 9)
    ],
  };
  mockSourceFor({ 42: excerptSpanA, 43: merging });
  render(Source, { selected: citationA, siblings: [citationA, citationB] });

  await settled();
  const found = marks();
  expect(found).toHaveLength(2); // four spans, two blocks, one run each
  expect(found.map((m) => m.textContent)).toEqual([
    'digitisation price is fixed', // [4, 22) ∪ [22, 31)
    'Following paragraph', // [0, 9) ∪ [9, 19)
  ]);
  // Both directions, and this is the pair: the run that absorbed the clicked
  // span is primary, the run built from siblings alone is not.
  expect(found[0].dataset.primary).toBe('true');
  expect(found[1].dataset.primary).toBeUndefined();
});

// 🔴 I1 and I2. Every other fixture in this file puts the CLICKED span first in
// its run, which makes two guards invisible at once: `last.primary || span.primary`
// is reached with `last.primary` already true, and the `.sort()` in `paintBlock`
// is a no-op because `painted` concatenates clicked spans ahead of sibling ones.
// Both stop being invisible the moment a sibling span starts BEFORE the clicked
// one — which is the ordinary case of two citations in one paragraph with the
// SECOND one clicked.
test('a sibling span that starts before the clicked one merges in full, and the run is still primary', async () => {
  const siblingFirst: Excerpt = {
    ...excerptSpanB,
    spans: [{ blockId: SHARED_BLOCK_ID, start: 3, end: 7, blockStart: 0 }], // len 4 → [0, 4) = 'The '
  };
  mockSourceFor({ 42: excerptSpanA, 43: siblingFirst }); // clicked span is [4, 22)
  render(Source, { selected: citationA, siblings: [citationA, citationB] });

  await settled();
  const found = marks();
  expect(found).toHaveLength(1); // [0, 4) touches [4, 22) and the two union
  // I2: without the sort the earlier sibling run is swallowed and these four
  // characters vanish from the highlight.
  expect(found[0].textContent).toBe('The digitisation price');
  // I1: the run holds the clicked citation's span, so it is primary — and the
  // clicked span is NOT the first one in it, which is the half nothing pinned.
  expect(found[0].dataset.primary).toBe('true');
});

// --- the verdicts -----------------------------------------------------------

test('Gone renders the reason and NO text', async () => {
  mockSource({ kind: 'gone', reason: { kind: 'idReused' } });
  render(Source, { selected: citationA, siblings: [citationA] });

  await settled();
  const status = screen.getByRole('status');
  expect(status.textContent).toBe('This identifier now points to another passage');
  expect(screen.queryByTestId('hl')).toBeNull();
  expect(screen.queryByTestId('source-block')).toBeNull();
  expect(screen.getByTestId('source-body').textContent).not.toContain(citationA.text);
  // The two Gone reasons are not one sentence.
  expect(status.textContent).not.toBe('This passage is no longer in the index');
});

test('the other Gone reason reads differently, and a Gone answer asks no siblings', async () => {
  mockSource({ kind: 'gone', reason: { kind: 'noSuchChunk' } });
  // A real sibling list, and M1's guard is what keeps this answer reading as a
  // `Gone`. Measured, so the claim is not larger than the evidence: with that
  // guard's `return` removed the card falls through to the wrong-document check
  // (a `Gone` payload carries no `documentId` at all) and badges this vanished
  // passage as coming from a different document — which is the first assertion
  // below, not the call count. The call count is the weaker half: for a `Gone`
  // answer the wrong-document check would stop the sibling calls too.
  render(Source, { selected: citationA, siblings: [citationA, citationB] });

  await settled();
  expect(screen.getByRole('status').textContent).toBe('This passage is no longer in the index');
  expect(screen.queryByTestId('source-block')).toBeNull();
  expect(sourceAroundCalls()).toHaveLength(1);
});

// 🔴 M2. Deliberate defence against a guarantee that lives in another
// repository: PR 6a pins `documentId` + `ord` in Rust, so a clicked citation and
// its chunk cannot disagree while that holds — but this component cannot see
// that invariant, and if it ever regresses this card is the surface that shows
// another document's text under this document's name. Treated as a `Gone` is.
test('an excerpt naming a different document than the citation shows no text and says why', async () => {
  // Every chunk answers, siblings included: with `mockSourceFor` the sibling
  // call the probe unblocks would throw instead, and this test would redden for
  // a reason unrelated to what it claims.
  mockSource({ ...excerptSpanA, documentId: 'doc-999' }); // citationA is doc-1
  render(Source, { selected: citationA, siblings: [citationA, citationB] });

  await settled();
  expect(screen.getByRole('status').textContent).toBe(
    'This excerpt came from a different document than the citation',
  );
  // No text at all, the way `Gone` renders none.
  expect(screen.queryByTestId('hl')).toBeNull();
  expect(screen.queryByTestId('source-block')).toBeNull();
  expect(screen.getByTestId('source-body').textContent).not.toContain(SPAN_A_TEXT);
  expect(screen.queryByTestId('more-before')).toBeNull(); // excerptSpanA sets this flag
  // No sibling is asked either: a window that cannot be trusted is not a window
  // to merge more spans into.
  expect(sourceAroundCalls()).toHaveLength(1);
  // Both directions: the freshness verdict does NOT ride along. It would be true
  // of doc-999 and false of the file the header names.
  expect(screen.getByTestId('freshness').textContent).not.toMatch(CURRENT_PATTERN);

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('freshness').textContent).toBe(
    'Цей уривок походить з іншого документа, ніж цитата',
  );
});

const CURRENT_PATTERN = /Up to date/;
const FRESHNESS_CASES: [Freshness['kind'], RegExp][] = [
  ['current', /Up to date/],
  ['reindexed', /another document/],
  ['fileChanged', /changed after indexing/],
  ['fileMissing', /missing from disk/],
  ['noPath', /unknown/],
];

test('every Freshness variant draws its own badge, and none of them falls through to Current', async () => {
  for (const [kind, pattern] of FRESHNESS_CASES) {
    mockSource({ ...excerptSpanA, freshness: { kind } as Freshness });
    const { unmount } = render(Source, { selected: citationA, siblings: [citationA] });

    await settled();
    expect(screen.getByTestId('freshness').textContent, kind).toMatch(pattern);
    // Both directions: a non-current verdict must not read as current.
    if (kind !== 'current') {
      expect(screen.getByTestId('freshness').textContent, kind).not.toMatch(CURRENT_PATTERN);
    }
    unmount();
  }
});

test('noPath says the location is unknown, never that the file is gone — in both locales', async () => {
  for (const [loc, yes, no] of [
    ['en', /unknown/i, /gone|deleted|missing/i],
    ['uk', /невідом/i, /зник|видален|відсутн/i],
  ] as const) {
    setLocale(loc);
    mockSource({ ...excerptSpanA, freshness: { kind: 'noPath' } });
    const { unmount } = render(Source, { selected: citationA, siblings: [citationA] });

    await settled();
    const badge = screen.getByTestId('freshness').textContent!;
    // Positive first — an empty badge would satisfy the negative alone.
    expect(badge.trim(), loc).toMatch(yes);
    // `tree.rs:226-241`: noPath has three causes and deletion is only one.
    expect(badge, loc).not.toMatch(no);
    unmount();
  }
});

test('the ellipsis appears only when the window says there is more', async () => {
  mockSource({ ...excerptSpanA, hasMoreBefore: true, hasMoreAfter: false });
  const first = render(Source, { selected: citationA, siblings: [citationA] });
  await settled();
  expect(screen.getByTestId('more-before')).toBeTruthy();
  expect(screen.queryByTestId('more-after')).toBeNull(); // the flags are not the same flag
  first.unmount();

  // The mirror image, so neither flag can be the other one in disguise.
  mockSource({ ...excerptSpanA, hasMoreBefore: false, hasMoreAfter: true });
  render(Source, { selected: citationA, siblings: [citationA] });
  await settled();
  expect(screen.getByTestId('more-after')).toBeTruthy();
  expect(screen.queryByTestId('more-before')).toBeNull();
});

// --- the request ordering ---------------------------------------------------

test('a slower answer for an older click never paints over a newer one', async () => {
  const excerptOne: Excerpt = {
    ...excerptSpanA,
    blocks: [{ blockId: 21, kind: 'paragraph', text: 'Excerpt one paragraph.', pageNo: 1, readingOrder: 21 }],
    spans: [],
  };
  const excerptTwo: Excerpt = {
    ...excerptSpanA,
    blocks: [{ blockId: 22, kind: 'paragraph', text: 'Excerpt two paragraph.', pageNo: 1, readingOrder: 22 }],
    spans: [],
  };
  const first = deferred<SourceAround>();
  const second = deferred<SourceAround>();
  invoke.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

  const { rerender } = render(Source, { selected: citationA, siblings: [] });
  await rerender({ selected: citationB, siblings: [] });
  expect(sourceAroundCalls()).toHaveLength(2);

  second.resolve(excerptTwo);
  first.resolve(excerptOne); // the stale answer lands LAST

  await settled();
  expect(screen.getByText(/Excerpt two/)).toBeTruthy();
  expect(screen.queryByText(/Excerpt one/)).toBeNull();
});

// 🔴 I3. The ordering test above passes `siblings: []`, so it exercises the
// CLICKED guard and nothing else. A sibling round trip from the previous
// selection is a separate promise that lands seconds late, and its `.then`
// appends straight into `siblingSpans`. The document check does not save this:
// the stale sibling and the current excerpt are both 'doc-1' here on purpose.
test('a sibling answer for a previous click never paints onto the current card', async () => {
  const staleSibling = deferred<SourceAround>();
  const noSpans: Excerpt = { ...excerptSpanA, spans: [] }; // the NEW selection's own window
  invoke
    .mockImplementationOnce(() => Promise.resolve(excerptSpanA)) // click 1, clicked
    .mockImplementationOnce(() => staleSibling.promise) // click 1, sibling
    .mockImplementationOnce(() => Promise.resolve(noSpans)); // click 2, clicked

  const { rerender } = render(Source, { selected: citationA, siblings: [citationA, citationB] });
  await flush();
  await rerender({ selected: citationB, siblings: [citationB] });
  await settled();
  expect(screen.queryByTestId('hl')).toBeNull();

  staleSibling.resolve(excerptSpanB); // the previous click's sibling lands late
  await flush();
  expect(screen.queryByTestId('hl')).toBeNull();
  // Both directions: the card is showing the new selection's text, so "no
  // highlight" is not "no card".
  expect(screen.getByTestId('source-body').textContent).toContain(SPAN_A_TEXT);
});

// 🔴 I4. `data-pending` is the anchor all of `settled()` stands on, so the guard
// that keeps it honest across a re-selection is the one guard whose failure would
// be invisible to every other test in this file.
test('a stale sibling does not drive data-pending to zero while the current call is in flight', async () => {
  const staleSibling = deferred<SourceAround>();
  const newClicked = deferred<SourceAround>();
  invoke
    .mockImplementationOnce(() => Promise.resolve(excerptSpanA))
    .mockImplementationOnce(() => staleSibling.promise)
    .mockImplementationOnce(() => newClicked.promise);

  const { rerender } = render(Source, { selected: citationA, siblings: [citationA, citationB] });
  await flush();
  await rerender({ selected: citationB, siblings: [citationB] });
  await flush();
  const body = () => screen.getByTestId('source-body');
  expect(body().dataset.pending).toBe('1');

  staleSibling.resolve(excerptSpanB);
  await flush();
  expect(screen.getByTestId('source-loading')).toBeTruthy(); // the new call has NOT returned
  expect(body().dataset.pending).toBe('1');

  // Both directions: the counter is not simply stuck — the current selection's
  // own answer still takes it to zero.
  newClicked.resolve({ ...excerptSpanA, spans: [] });
  await settled();
  expect(screen.queryByTestId('source-loading')).toBeNull();
});

// --- the header, the failure and the language switch ------------------------

// Ruling S: this header and `Answer`'s preview label are ONE rule (Decision 1),
// and the third branch is the only one that may say "no path on disk".
test('the card header names the file by the same three branches as the preview label', async () => {
  for (const [citation, expected, forbidden] of [
    [citationA, /^notes\/a\.md · lines 5–7$/, /no path/i],
    [firstCitation(generatedArchived), /p\. 12/, /no path/i],
    [firstCitation(generatedNoPath), /no path on disk/i, /·/],
  ] as const) {
    mockSource(excerptSpanA);
    const { unmount } = render(Source, { selected: citation, siblings: [citation] });

    // Settle the card first: an assertion read off a still-pending render is
    // green for a reason that has nothing to do with the header.
    await settled();
    const header = screen.getByTestId('source-header').textContent!.trim();
    expect(header, citation.documentId).toMatch(expected);
    expect(header, citation.documentId).not.toMatch(forbidden);
    unmount();
  }
});

test('a failed source_around says so rather than leaving an empty card', async () => {
  setLocale('en'); // seed, do not inherit
  invoke.mockRejectedValue(new Error('source_around failed'));
  render(Source, { selected: citationA, siblings: [citationA] });

  await settled();
  expect(screen.getByTestId('source-failed').textContent).toBe('The source could not be loaded.');
  expect(screen.queryByTestId('source-loading')).toBeNull();
  expect(screen.queryByTestId('freshness')).toBeNull();

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('source-failed').textContent).toBe('Не вдалося завантажити джерело.');
});

test('the card says it is loading until the answer lands', async () => {
  setLocale('en'); // seed, do not inherit
  invoke.mockReturnValue(new Promise(() => {})); // never settles
  render(Source, { selected: citationA, siblings: [citationA] });

  // 🔴 The floor under `settled()`: the card counts its clicked round trip as
  // in flight from the first synchronous render, so a wait for
  // `data-pending === '0'` can never be satisfied by a card that never asked.
  expect(screen.getByTestId('source-body').dataset.pending).toBe('1');
  expect(screen.getByTestId('source-loading').textContent).toBe('Loading the source…');
  // Both directions: nothing a resolved answer produces is on screen yet.
  expect(screen.queryByTestId('freshness')).toBeNull();
  expect(screen.queryByTestId('hl')).toBeNull();

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('source-loading').textContent).toBe('Завантаження джерела…');
});

// D130: a bare `t()` in markup does not re-render on a language switch, and an
// English hardcode passes the Cyrillic guard silently.
test('the header and the freshness badge follow a live language switch', async () => {
  setLocale('en'); // seed, do not inherit
  mockSource({ ...excerptSpanA, freshness: { kind: 'fileMissing' } });
  render(Source, { selected: citationA, siblings: [citationA] });

  await settled();
  expect(screen.getByTestId('freshness').textContent).toBe('The file is missing from disk');
  expect(screen.getByTestId('source-header').textContent).toBe('notes/a.md · lines 5–7');

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('freshness').textContent).toBe('Файла немає на диску');
  expect(screen.getByTestId('source-header').textContent).toBe('notes/a.md · рядки 5–7');

  setLocale('en'); // the switch back is part of the claim, not the cleanup — afterEach owns that
  await tick();
  expect(screen.getByTestId('freshness').textContent).toBe('The file is missing from disk');
  expect(screen.getByTestId('source-header').textContent).toBe('notes/a.md · lines 5–7');
});

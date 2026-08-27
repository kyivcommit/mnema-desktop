import { render, screen, fireEvent, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, expect, test, vi } from 'vitest';
import Answer from './Answer.svelte';
import { stateFromAnswer } from './state';
import { generated, generatedArchived, generatedNoPath } from '../lib/fixtures';
import { setLocale } from '../i18n';
import type { AskAnswer } from '../lib/ipc';

// Mirrors Cards.test.ts: `locale` is a module-level store shared by every
// test in this file. Restore unconditionally so a failed assertion in the
// locale-switch test cannot leave the next test to fail for a reason that
// has nothing to do with what it claims.
afterEach(() => setLocale('en'));

// Review I1: `Answer`'s `answer` prop is narrowed to
// `Extract<AskAnswer, { kind: 'generated' }>` (it pairs with
// `LauncherState.generated`, `state.ts:22`, the way Task 8b will mount it),
// but the fixtures in `lib/fixtures.ts` are declared as the wide `AskAnswer`
// union. Narrow through the same door `Cards.test.ts:23` already uses
// (`stateFromAnswer`) instead of widening the component's prop type.
function gen(a: AskAnswer) {
  const s = stateFromAnswer('q', a);
  if (s.kind !== 'generated') throw new Error('fixture is not a generated answer');
  return s.answer;
}

test('the query echoes as a bubble and the answer renders its anchors as buttons', () => {
  render(Answer, { query: 'how much?', answer: gen(generated), onSelect: vi.fn() });
  expect(screen.getByTestId('query-echo').textContent).toBe('how much?');
  // Scoped to the prose since P2: the preview rows carry a visible `[N]` of
  // their own now, and this claim is about the anchors INSIDE the answer —
  // unscoped it would be satisfied by four buttons, two of which are the
  // previews, and it would still say "the answer renders its anchors".
  const answerBody = screen.getByTestId('answer-body');
  expect(within(answerBody).getAllByRole('button', { name: /\[3\]|\[7\]/ })).toHaveLength(2);
  const body = answerBody.textContent!;
  // Both directions (review I2): the raw grammar never reaches the DOM, AND
  // the prose around both anchors survives IN ORDER with the anchors between
  // it. Two `toContain`s were satisfied by any arrangement — re-review N1
  // measured a reversed-segment mutant rendering
  // `.[7] and the total cannot exceed the cap[3]Costs four hryvnias` at 88/88
  // green. The whole string is the claim.
  expect(body).not.toMatch(/<c>/);
  expect(body).toBe('Costs four hryvnias[3] and the total cannot exceed the cap[7].');

  // Re-review N2: presence was defended, identity was not — swapping the two
  // headings so the prose reads "Citations" and the citation list reads
  // "Answer" was 88/88 green. Order in the document is the identity.
  expect(screen.getAllByRole('heading').map((h) => h.textContent)).toEqual([
    'Answer',
    'Citations',
  ]);
});

test('the anchor resolves by value, not by position', async () => {
  const onSelect = vi.fn();
  render(Answer, { query: 'q', answer: gen(generated), onSelect }); // anchors 3 and 7
  await fireEvent.click(screen.getByRole('button', { name: '[7]' }));
  expect(onSelect).toHaveBeenCalledTimes(1);
  const [picked] = onSelect.mock.calls[0];
  expect(picked.anchor).toBe(7);
  expect(picked.chunkId).toBe(43); // citations[6] does not exist; citations[1] is this one
});

test('the preview namespace holds one id per citation and nothing else', () => {
  // The preview rows are queried as a namespace, `queryAllByTestId(/^preview-/)`,
  // so a second testid inside it is counted as a row. The label used to be
  // `preview-label` and was exactly that; it is `citation-label` now, the rule
  // `Passages.svelte` already follows for `passage-label`.
  //
  // 🔴 This test is the ONLY thing that catches the collision — measured, by
  // putting `preview-label` back in the component AND in every query in this
  // file: 1 failed, 169 passed, and the one is this test. `Cards.test.ts` has a
  // `/^preview-/` count of its own, but it is in state E, where
  // `Selection.svelte:87-90` mounts `Passages` and not this component, so that
  // namespace is empty there whatever the label is called. It cannot witness
  // this, and an earlier version of this comment cited it as if it could.
  render(Answer, { query: 'q', answer: gen(generated), onSelect: vi.fn() });
  expect(screen.getAllByTestId(/^preview-/).map((el) => el.dataset.testid))
    .toEqual(['preview-3', 'preview-7']);
});

// 🔴 Owner review on PR #24, P2, and it is written against what the preview
// SHOWS rather than against its ids. The shipped preview rendered the locator
// label alone: `citation.text` and the anchor were absent, so the card listing
// the answer's citations was a list of file paths. The anchor appearing in the
// prose above does not make the preview itself identifiable — a person reading
// `[7]` in the sentence has nothing in the list carrying that number.
//
// The whole of the row, in order, is the claim: a preview that dropped the
// text, dropped the anchor, or put them in another order all redden here.
test('the preview shows the cited passage and its anchor, not only a path', () => {
  setLocale('en'); // seed, do not inherit
  render(Answer, { query: 'q', answer: gen(generated), onSelect: vi.fn() });

  const row = (n: number) =>
    (screen.getByTestId(`preview-${n}`).textContent ?? '').replace(/\s+/g, ' ').trim();
  expect(row(3)).toBe('[3] A cited passage. notes/a.md · lines 5–7');
  // The second citation has no coordinate, so its label is the bare path — the
  // dangling-separator branch, with the passage and anchor beside it.
  expect(row(7)).toBe('[7] A second cited passage. notes/a.md');
});

test('the preview label is path · locator, and never invents a paragraph number', () => {
  render(Answer, { query: 'q', answer: gen(generated), onSelect: vi.fn() });
  const preview = screen.getByTestId('preview-3');
  const label = preview.querySelector('[data-testid="citation-label"]')!.textContent!;
  expect(label).toContain('notes/a.md');
  expect(label).toMatch(/рядки|lines/);
  expect(label).not.toMatch(/абзац|paragraph/);
  // Review M1 asked that the button's accessible name be exactly its label, so
  // that a bracketed ordinal fused on elsewhere could not grow the name
  // invisibly. P2 makes the ordinal and the passage text DELIBERATE parts of
  // the row, so the claim moves rather than disappears: the accessible name is
  // the whole row and nothing besides — a fourth part added anywhere in the
  // button still reddens this line, and `label` is still read from the
  // `citation-label` span alone.
  expect(screen.getByRole('button', { name: `[3] A cited passage. ${label}` })).toBe(preview);
});

test('a citation with no coordinate shows the path alone, with no dangling separator', () => {
  render(Answer, { query: 'q', answer: gen(generated), onSelect: vi.fn() }); // citation 7 has coordinate none
  expect(screen.getByTestId('preview-7').querySelector('[data-testid="citation-label"]')!
    .textContent!.trim()).toBe('notes/a.md');
});

test('no path on disk but a real location keeps the location', () => {
  render(Answer, { query: 'q', answer: gen(generatedArchived), onSelect: vi.fn() });
  const label = screen.getByTestId('preview-1').querySelector('[data-testid="citation-label"]')!.textContent!;
  expect(label).toMatch(/с\. 12|p\. 12/);
  // Both directions: the location is not replaced by the no-path string.
  expect(label).not.toMatch(/no path|нема на диску/i);
});

test('neither path nor location says so rather than rendering an empty label', () => {
  render(Answer, { query: 'q', answer: gen(generatedNoPath), onSelect: vi.fn() });
  expect(screen.getByTestId('preview-1').querySelector('[data-testid="citation-label"]')!
    .textContent).toMatch(/no path|нема на диску/i);
});

test('clicking an anchor and clicking its preview both select the same citation', async () => {
  const onSelect = vi.fn();
  render(Answer, { query: 'q', answer: gen(generated), onSelect });
  await fireEvent.click(screen.getByRole('button', { name: '[7]' }));
  await fireEvent.click(screen.getByTestId('preview-7'));
  expect(onSelect).toHaveBeenCalledTimes(2);
  expect(onSelect.mock.calls.every(([c]) => c.anchor === 7)).toBe(true);
});

// Ruling D — 🔴 `formatLocator` calls a bare `t()` and reads no `$locale`, so
// a label computed outside a reactive wrapper freezes at the locale it was
// first rendered in (D130, the Codex ④ defect on PR #20, Task 5's I2). This
// is the test the task's seven illustrated ones would not have caught.
// Review I3: the two headings share the same gap — folded into the same
// switch here rather than left undefended (Task 5's review found the
// identical class as its Important I2, one component earlier).
test('the preview label and both headings follow a live language switch', async () => {
  setLocale('en'); // seed, do not inherit: an earlier sibling switching the language must not decide this test
  render(Answer, { query: 'q', answer: gen(generated), onSelect: vi.fn() });
  expect(screen.getByTestId('preview-3').querySelector('[data-testid="citation-label"]')!.textContent)
    .toMatch(/lines 5–7/);
  expect(screen.getByRole('heading', { name: 'Answer' })).toBeTruthy();
  expect(screen.getByRole('heading', { name: 'Citations' })).toBeTruthy();

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('preview-3').querySelector('[data-testid="citation-label"]')!.textContent)
    .toMatch(/рядки 5–7/);
  expect(screen.getByRole('heading', { name: 'Відповідь' })).toBeTruthy();
  expect(screen.getByRole('heading', { name: 'Цитати' })).toBeTruthy();

  setLocale('en'); // the switch back is part of the claim, not the cleanup — afterEach owns that
  await tick();
  expect(screen.getByTestId('preview-3').querySelector('[data-testid="citation-label"]')!.textContent)
    .toMatch(/lines 5–7/);
  expect(screen.getByRole('heading', { name: 'Answer' })).toBeTruthy();
  expect(screen.getByRole('heading', { name: 'Citations' })).toBeTruthy();
});

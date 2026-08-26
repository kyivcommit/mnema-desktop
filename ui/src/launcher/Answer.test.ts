import { render, screen, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, expect, test, vi } from 'vitest';
import Answer from './Answer.svelte';
import { generated, generatedArchived, generatedNoPath } from '../lib/fixtures';
import { setLocale } from '../i18n';

// Mirrors Cards.test.ts: `locale` is a module-level store shared by every
// test in this file. Restore unconditionally so a failed assertion in the
// locale-switch test cannot leave the next test to fail for a reason that
// has nothing to do with what it claims.
afterEach(() => setLocale('en'));

test('the query echoes as a bubble and the answer renders its anchors as buttons', () => {
  render(Answer, { query: 'how much?', answer: generated, onSelect: vi.fn() });
  expect(screen.getByTestId('query-echo').textContent).toBe('how much?');
  expect(screen.getAllByRole('button', { name: /\[3\]|\[7\]/ })).toHaveLength(2);
  // Both directions: the raw grammar never reaches the DOM.
  expect(screen.getByTestId('answer-body').textContent).not.toMatch(/<c>/);
});

test('the anchor resolves by value, not by position', async () => {
  const onSelect = vi.fn();
  render(Answer, { query: 'q', answer: generated, onSelect }); // anchors 3 and 7
  await fireEvent.click(screen.getByRole('button', { name: '[7]' }));
  expect(onSelect).toHaveBeenCalledTimes(1);
  const [picked] = onSelect.mock.calls[0];
  expect(picked.anchor).toBe(7);
  expect(picked.chunkId).toBe(43); // citations[6] does not exist; citations[1] is this one
});

test('the preview label is path · locator, and never invents a paragraph number', () => {
  render(Answer, { query: 'q', answer: generated, onSelect: vi.fn() });
  const label = screen.getByTestId('preview-3').textContent!;
  expect(label).toContain('notes/a.md');
  expect(label).toMatch(/рядки|lines/);
  expect(label).not.toMatch(/абзац|paragraph/);
});

test('a citation with no coordinate shows the path alone, with no dangling separator', () => {
  render(Answer, { query: 'q', answer: generated, onSelect: vi.fn() }); // citation 7 has coordinate none
  expect(screen.getByTestId('preview-7').querySelector('[data-testid="preview-label"]')!
    .textContent!.trim()).toBe('notes/a.md');
});

test('no path on disk but a real location keeps the location', () => {
  render(Answer, { query: 'q', answer: generatedArchived, onSelect: vi.fn() });
  const label = screen.getByTestId('preview-1').querySelector('[data-testid="preview-label"]')!.textContent!;
  expect(label).toMatch(/с\. 12|p\. 12/);
  // Both directions: the location is not replaced by the no-path string.
  expect(label).not.toMatch(/no path|нема на диску/i);
});

test('neither path nor location says so rather than rendering an empty label', () => {
  render(Answer, { query: 'q', answer: generatedNoPath, onSelect: vi.fn() });
  expect(screen.getByTestId('preview-1').querySelector('[data-testid="preview-label"]')!
    .textContent).toMatch(/no path|нема на диску/i);
});

test('clicking an anchor and clicking its preview both select the same citation', async () => {
  const onSelect = vi.fn();
  render(Answer, { query: 'q', answer: generated, onSelect });
  await fireEvent.click(screen.getByRole('button', { name: '[7]' }));
  await fireEvent.click(screen.getByTestId('preview-7'));
  expect(onSelect).toHaveBeenCalledTimes(2);
  expect(onSelect.mock.calls.every(([c]) => c.anchor === 7)).toBe(true);
});

// Ruling D — 🔴 `formatLocator` calls a bare `t()` and reads no `$locale`, so
// a label computed outside a reactive wrapper freezes at the locale it was
// first rendered in (D130, the Codex ④ defect on PR #20, Task 5's I2). This
// is the test the task's seven illustrated ones would not have caught.
test('the preview label follows a live language switch', async () => {
  setLocale('en'); // seed, do not inherit: an earlier sibling switching the language must not decide this test
  render(Answer, { query: 'q', answer: generated, onSelect: vi.fn() });
  expect(screen.getByTestId('preview-3').querySelector('[data-testid="preview-label"]')!.textContent)
    .toMatch(/lines 5–7/);

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('preview-3').querySelector('[data-testid="preview-label"]')!.textContent)
    .toMatch(/рядки 5–7/);

  setLocale('en'); // the switch back is part of the claim, not the cleanup — afterEach owns that
  await tick();
  expect(screen.getByTestId('preview-3').querySelector('[data-testid="preview-label"]')!.textContent)
    .toMatch(/lines 5–7/);
});

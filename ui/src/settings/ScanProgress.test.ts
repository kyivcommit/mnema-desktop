import { render, screen, cleanup } from '@testing-library/svelte';
import { expect, test, afterEach } from 'vitest';
import ScanProgress from './ScanProgress.svelte';
import { setLocale } from '../i18n';
import type { Counts, Phase } from '../lib/ipc';

// Task 5 — `ScanProgress` is a pure projection of one `Phase`: no store, no
// IPC, no controller. `JobStrip.svelte` (the bottom disclosure) and
// `Scanning.svelte` (§9.3) each hold their own subscription and hand down the
// SAME running phase from the SAME snapshot; this file only proves what this
// component draws from it, never how either caller gets it.

afterEach(() => {
  cleanup();
  setLocale('en');
});

// What a person reads, with the markup's own indentation collapsed the way a
// browser collapses it.
const visible = (el: Element | null) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();

const COUNTS: Counts = { done: 0, total: 0, skipped: 0, refused: 0, contended: 0, secondsLeft: null };

// `total: 0` still draws a `<progress>` — reading and embedding both do,
// unconditionally — but an INDETERMINATE one: no `max`, no `value`, because
// `progressShape`'s own "countingUp" shape has no total to measure against
// yet. "0 of 0" would read as "nothing to do" while a run is genuinely under
// way; a bare, moving bar does not claim a fraction it cannot back.
test('a reading pass with nothing counted yet (total=0) draws an indeterminate progressbar and reads as counting up', () => {
  setLocale('uk');
  const phase: Phase = {
    kind: 'reading', rootIndex: 1, rootCount: 2, rootPath: '/home/a/notes',
    counts: { ...COUNTS, done: 4 },
  };
  render(ScanProgress, { props: { phase } });

  const bar = screen.getByRole('progressbar') as HTMLProgressElement;
  expect(bar.hasAttribute('max')).toBe(false);
  expect(bar.hasAttribute('value')).toBe(false);
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Індексація теки 1 з 2: /home/a/notes');
  expect(visible(screen.getByTestId('indexing-counts')))
    .toBe('Опрацьовано 4. Скільки їх усього, поки не відомо. Пропущено: 0. Відхилено: 0.');
});

test('a reading pass with a known total draws a ratio progressbar, labelled with the running phase', () => {
  setLocale('uk');
  const phase: Phase = {
    kind: 'reading', rootIndex: 1, rootCount: 2, rootPath: '/home/a/notes',
    counts: { ...COUNTS, done: 3, total: 8 },
  };
  render(ScanProgress, { props: { phase } });

  const bar = screen.getByRole('progressbar') as HTMLProgressElement;
  expect(bar.max).toBe(8);
  expect(bar.value).toBe(3);
  expect(bar.getAttribute('aria-label')).toBe('Індексація теки 1 з 2: /home/a/notes');
  expect(visible(screen.getByTestId('indexing-counts')))
    .toBe('Опрацьовано 3 з 8. Пропущено: 0. Відхилено: 0.');
});

// Two roots, each its own line: root 1 of 2 finishing at 8 of 8, then root 2
// of 2 starting fresh at 0 of 30 — the position, the path and the ratio all
// change together because they come from the same `phase`, never a stale
// mix of one root's count beside another's position.
test('two roots draw two different lines, each its own position and its own ratio', async () => {
  setLocale('uk');
  const first: Phase = {
    kind: 'reading', rootIndex: 1, rootCount: 2, rootPath: '/a',
    counts: { ...COUNTS, done: 8, total: 8 },
  };
  const { rerender } = render(ScanProgress, { props: { phase: first } });

  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Індексація теки 1 з 2: /a');
  expect(visible(screen.getByTestId('indexing-counts'))).toBe('Опрацьовано 8 з 8. Пропущено: 0. Відхилено: 0.');
  let bar = screen.getByRole('progressbar') as HTMLProgressElement;
  expect(bar.max).toBe(8);
  expect(bar.value).toBe(8);

  const second: Phase = {
    kind: 'reading', rootIndex: 2, rootCount: 2, rootPath: '/b',
    counts: { ...COUNTS, done: 0, total: 30 },
  };
  await rerender({ phase: second });

  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Індексація теки 2 з 2: /b');
  expect(visible(screen.getByTestId('indexing-counts'))).toBe('Опрацьовано 0 з 30. Пропущено: 0. Відхилено: 0.');
  bar = screen.getByRole('progressbar') as HTMLProgressElement;
  expect(bar.max).toBe(30);
  expect(bar.value).toBe(0);
});

test('an embedding pass counts chunks, not files, and draws its own ratio bar', () => {
  setLocale('uk');
  const phase: Phase = { kind: 'embedding', counts: { ...COUNTS, done: 5, total: 20 } };
  render(ScanProgress, { props: { phase } });

  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває вбудовування всього індексу.');
  expect(visible(screen.getByTestId('indexing-counts')))
    .toBe('Опрацьовано 5 з 20. Пропущено: 0. Відхилено: 0.');
  const bar = screen.getByRole('progressbar') as HTMLProgressElement;
  expect(bar.max).toBe(20);
  expect(bar.value).toBe(5);
});

test('a fresh embedding pass at 0 of 0 says it is starting, not "0 of 0"', () => {
  setLocale('uk');
  const phase: Phase = { kind: 'embedding', counts: { ...COUNTS } };
  render(ScanProgress, { props: { phase } });

  expect(visible(screen.getByTestId('indexing-counts'))).toBe('Вбудовування починається…');
});

// Neither a removal nor a job this build only just started naming (`other`)
// carries counts of its own — both still get a name (Task 5), but neither
// ever draws a progressbar over a number that does not exist.
test('a removal and a probe or model-adoption phase draw no progressbar, only their own name', () => {
  setLocale('uk');
  const removing: Phase = { kind: 'removing', rootPath: '/home/a/papers' };
  const { rerender } = render(ScanProgress, { props: { phase: removing } });

  expect(screen.queryByRole('progressbar')).toBeNull();
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Видаляємо теку /home/a/papers…');
  expect(screen.queryByTestId('indexing-counts')).toBeNull();

  rerender({ phase: { kind: 'other', job: 'probe' } as Phase });
  expect(screen.queryByRole('progressbar')).toBeNull();
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває перевірка з’єднання…');

  rerender({ phase: { kind: 'other', job: 'modelAdoption' } as Phase });
  expect(screen.queryByRole('progressbar')).toBeNull();
  expect(visible(screen.getByTestId('indexing-pass'))).toBe('Триває заміна моделі вбудовування…');
});

// Both directions on a field whose `Option<u64>` has a real `None`: nought
// seconds left is an estimate, and a truthiness check would print "not known
// yet" for it instead.
test('nought seconds left is an estimate, and an absent one says it is not known', () => {
  setLocale('uk');
  const zero: Phase = {
    kind: 'reading', rootIndex: 1, rootCount: 1, rootPath: '/a',
    counts: { ...COUNTS, done: 1, total: 2, secondsLeft: 0 },
  };
  const { rerender } = render(ScanProgress, { props: { phase: zero } });
  expect(visible(screen.getByTestId('indexing-eta'))).toBe('Залишилось приблизно 0 с.');

  rerender({ phase: { ...zero, counts: { ...zero.counts, secondsLeft: null } } });
  expect(visible(screen.getByTestId('indexing-eta'))).toBe('Скільки ще лишилось часу, поки не відомо.');
});

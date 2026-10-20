import { render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import FilePane from './FilePane.svelte';
import { setLocale } from '../i18n';
import type { DocumentFocus, DocumentLocation, DocumentView } from '../lib/ipc';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const location: DocumentLocation = { documentId: 'doc-1', rootId: 1, relativePath: 'notes/a.md' };

function mockView(view: DocumentView) {
  invoke.mockResolvedValue(view);
}
function openCalls() {
  return invoke.mock.calls.filter(([cmd]) => cmd === 'open_document');
}

const scroll = vi.fn();
Element.prototype.scrollIntoView = scroll; // jsdom has no layout and no scroller

beforeEach(() => {
  invoke.mockReset();
  scroll.mockReset();
  mockView({
    kind: 'file',
    blocks: [
      { blockId: 1, kind: 'headline', text: '## Розділ', pageNo: 1, sectionTitle: null },
      { blockId: 2, kind: 'paragraph', text: 'Перший абзац.', pageNo: 1, sectionTitle: null },
      { blockId: 3, kind: 'code', text: 'let x = 1;', pageNo: 1, sectionTitle: null },
    ],
    focusBlockIds: [],
    spans: [],
    freshness: { kind: 'current' },
  });
});

// `locale` is a module-level store shared by every test in this file; restore
// unconditionally, as every other suite does.
afterEach(() => setLocale('en'));

const blocks = () => screen.getAllByTestId('file-block').map((el) => el.textContent);

test('blocks render in document order, each in the element its kind names', async () => {
  mockView({
    kind: 'file',
    blocks: [
      { blockId: 1, kind: 'headline', text: '## Розділ', pageNo: 1, sectionTitle: null },
      { blockId: 2, kind: 'paragraph', text: 'Перший абзац.', pageNo: 1, sectionTitle: null },
      { blockId: 3, kind: 'code', text: 'let x = 1;', pageNo: 1, sectionTitle: null },
    ],
    focusBlockIds: [],
    spans: [],
    freshness: { kind: 'current' },
  });
  render(FilePane, { location, focus: null, onFocusHandled: () => {} });

  await screen.findAllByTestId('file-block');
  expect(blocks()).toEqual(['Розділ', 'Перший абзац.', 'let x = 1;']);
  // The headline loses only its display markers; the stored text is untouched.
  expect(screen.getAllByTestId('file-block')[0].tagName).toBe('H4');
  expect(screen.getAllByTestId('file-block')[1].tagName).toBe('P');
  expect(screen.getAllByTestId('file-block')[2].tagName).toBe('PRE');
});

// 🔴 Ruling R, ported from the deleted source card. The block starts with an
// astral character, so its code point offsets and its UTF-16 offsets disagree
// from the second character on; the fixture is Cyrillic and therefore local
// (`lib/fixtures.ts` is read by the Cyrillic guard, a `.test.ts` is not).
test('the highlight is placed by code points, not UTF-16 units', async () => {
  const text = '🌍 Ціна оцифрування становить чотири гривні.';
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'paragraph', text, pageNo: 1, sectionTitle: null }],
    focusBlockIds: [1],
    // Code points [2, 18) = 'Ціна оцифрування'; `text.slice(2, 18)` would start
    // one character late and end one early.
    spans: [{ blockId: 1, blockStart: 2, length: 16, primary: true }],
    freshness: { kind: 'current' },
  });
  render(FilePane, { location, focus: null, onFocusHandled: () => {} });

  expect((await screen.findByTestId('hl')).textContent).toBe('Ціна оцифрування');
  expect(screen.getByTestId('file-block').textContent).toBe(text);
});

test('a cited span is highlighted inside its block, and the rest is plain', async () => {
  mockView({
    kind: 'file',
    blocks: [
      { blockId: 1, kind: 'paragraph', text: 'Ціна становить дві гривні.', pageNo: 1, sectionTitle: null },
      { blockId: 2, kind: 'paragraph', text: 'Другий абзац.', pageNo: 1, sectionTitle: null },
    ],
    focusBlockIds: [1],
    spans: [{ blockId: 1, blockStart: 0, length: 4, primary: true }],
    freshness: { kind: 'current' },
  });
  render(FilePane, {
    location,
    focus: { chunkId: 42, ord: 0, text: 'Ціна' },
    onFocusHandled: () => {},
  });

  const mark = await screen.findByTestId('hl');
  expect(mark.textContent).toBe('Ціна');
  expect(mark.getAttribute('data-primary')).toBe('true');
  // Exactly one mark: the snippet is painted, not the whole block.
  expect(screen.getAllByTestId('hl')).toHaveLength(1);
  expect(screen.getAllByTestId('file-block')[1].querySelector('mark')).toBeNull();
});

test('the first focus block is scrolled into view and the focus is acknowledged', async () => {
  const acknowledged = vi.fn();
  mockView({
    kind: 'file',
    blocks: [
      { blockId: 1, kind: 'paragraph', text: 'Перший.', pageNo: 1, sectionTitle: null },
      { blockId: 2, kind: 'paragraph', text: 'Другий.', pageNo: 1, sectionTitle: null },
    ],
    focusBlockIds: [2],
    spans: [{ blockId: 2, blockStart: 0, length: 6, primary: true }],
    freshness: { kind: 'current' },
  });
  render(FilePane, {
    location,
    focus: { chunkId: 42, ord: 0, text: 'Другий' },
    onFocusHandled: acknowledged,
  });

  await screen.findByTestId('hl');
  await waitFor(() => expect(scroll).toHaveBeenCalledTimes(1));
  expect(scroll.mock.contexts.at(-1)).toBe(screen.getAllByTestId('file-block')[1]);
  expect(scroll).toHaveBeenLastCalledWith({ block: 'center' });
  expect(acknowledged).toHaveBeenCalledTimes(1);
});

test('a gone file shows the badge and keeps its text', async () => {
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'paragraph', text: 'Текст, який усе ще в індексі.', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [],
    spans: [],
    freshness: { kind: 'fileMissing' },
  });
  render(FilePane, { location, focus: null, onFocusHandled: () => {} });

  expect((await screen.findByTestId('file-badge')).textContent).toBe('The file is missing from disk');
  // The marker is not a refusal: the indexed text is still shown.
  expect(blocks()).toEqual(['Текст, який усе ще в індексі.']);
});

test('a file without a path opens without a badge', async () => {
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'paragraph', text: 'Без шляху.', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [],
    spans: [],
    freshness: { kind: 'noPath' },
  });
  render(FilePane, {
    location: { documentId: 'doc-1', rootId: null, relativePath: null },
    focus: null,
    onFocusHandled: () => {},
  });

  await screen.findByTestId('file-block');
  expect(screen.queryByTestId('file-badge')).toBeNull();
  // The file really is open: this is not an empty card.
  expect(blocks()).toEqual(['Без шляху.']);
  expect(screen.queryByTestId('file-header')).toBeNull();
});

test('an unavailable document says why instead of drawing an empty file', async () => {
  mockView({ kind: 'unavailable', reason: 'rebuilding' });
  render(FilePane, { location, focus: null, onFocusHandled: () => {} });

  expect((await screen.findByTestId('file-unavailable')).textContent).toBe(
    'This document is being reindexed — try again after the scan.',
  );
  expect(screen.queryByTestId('file-block')).toBeNull();
});

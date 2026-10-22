import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
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
    relativePath: 'notes/a.md',
    rootName: null,
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
    relativePath: 'notes/a.md',
    rootName: null,
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
    relativePath: 'notes/a.md',
    rootName: null,
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
    relativePath: 'notes/a.md',
    rootName: null,
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
    relativePath: 'notes/a.md',
    rootName: null,
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

// The parent acknowledges the handled focus by clearing it. That is not a new
// open: without the guard the effect re-reads the file WITHOUT the citation,
// the answer comes back with no `spans`, and the highlight disappears under the
// person who just clicked the citation.
test('clearing the handled focus neither refetches nor drops the highlight', async () => {
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'paragraph', text: 'Ціна становить дві гривні.', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [1],
    spans: [{ blockId: 1, blockStart: 0, length: 4, primary: true }],
    relativePath: 'notes/a.md',
    rootName: null,
    freshness: { kind: 'current' },
  });
  const { rerender } = render(FilePane, {
    location,
    focus: { chunkId: 42, ord: 0, text: 'Ціна' },
    onFocusHandled: () => {},
  });
  expect((await screen.findByTestId('hl')).textContent).toBe('Ціна');
  expect(openCalls()).toHaveLength(1);

  await rerender({ focus: null });

  expect(openCalls()).toHaveLength(1);
  expect(screen.getByTestId('hl').textContent).toBe('Ціна');
});

// `headline` is `block.type` verbatim, and the HTML and docx readers emit it
// for a real heading. Their text is not markdown: `#1 Priority` is the line.
test('a headline from a reader that is not markdown keeps its leading hash', async () => {
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'headline', text: '#1 Priority', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [],
    spans: [],
    relativePath: 'notes/a.md',
    rootName: null,
    freshness: { kind: 'current' },
  });
  render(FilePane, { location, focus: null, onFocusHandled: () => {} });

  expect((await screen.findByTestId('file-block')).textContent).toBe('#1 Priority');
});

// A span may begin inside the marker (a chunk can cover a heading). The start
// clamps to 0, and the end must move by the same amount or the highlight grows
// by the stripped prefix: `##   Heading text` with [2, 12) paints "Heading".
test('a span that starts inside the heading marker keeps its end', async () => {
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'headline', text: '##   Heading text', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [1],
    spans: [{ blockId: 1, blockStart: 2, length: 10, primary: true }],
    relativePath: 'notes/a.md',
    rootName: null,
    freshness: { kind: 'current' },
  });
  render(FilePane, { location, focus: null, onFocusHandled: () => {} });

  expect((await screen.findByTestId('file-block')).textContent).toBe('Heading text');
  expect(screen.getByTestId('hl').textContent).toBe('Heading');
});

test('a gone file shows the badge and keeps its text', async () => {
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'paragraph', text: 'Текст, який усе ще в індексі.', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [],
    spans: [],
    relativePath: 'notes/a.md',
    rootName: null,
    freshness: { kind: 'fileMissing' },
  });
  render(FilePane, { location, focus: null, onFocusHandled: () => {} });

  expect((await screen.findByTestId('file-badge')).textContent).toBe('The file is missing from disk');
  // The marker is not a refusal: the indexed text is still shown.
  expect(blocks()).toEqual(['Текст, який усе ще в індексі.']);
});

// Owner, 2026-10-20: «Up to date» is not news. The badge speaks only when it
// carries some — changed, reindexed, missing — and an unchanged file draws
// none.
test('an unchanged file draws no badge', async () => {
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'paragraph', text: 'Текст без змін.', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [],
    spans: [],
    relativePath: 'notes/a.md',
    rootName: null,
    freshness: { kind: 'current' },
  });
  render(FilePane, { location, focus: null, onFocusHandled: () => {} });

  await screen.findByTestId('file-block');
  expect(screen.queryByTestId('file-badge')).toBeNull();
  // The file really is open: this is not an empty card.
  expect(blocks()).toEqual(['Текст без змін.']);
});

test('a file without a path opens without a badge', async () => {
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'paragraph', text: 'Без шляху.', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [],
    spans: [],
    relativePath: null,
    rootName: null,
    freshness: { kind: 'noPath' },
  });
  const onTitle = vi.fn();
  render(FilePane, {
    location: { documentId: 'doc-1', rootId: null, relativePath: null },
    focus: null,
    onFocusHandled: () => {},
    onTitle,
  });

  await screen.findByTestId('file-block');
  expect(screen.queryByTestId('file-badge')).toBeNull();
  // The file really is open: this is not an empty card.
  expect(blocks()).toEqual(['Без шляху.']);
  expect(onTitle).toHaveBeenLastCalledWith(null);
});

test('an unavailable document says why instead of drawing an empty file', async () => {
  mockView({ kind: 'unavailable', reason: 'rebuilding' });
  render(FilePane, { location, focus: null, onFocusHandled: () => {} });

  expect((await screen.findByTestId('file-unavailable')).textContent).toBe(
    'This document is being reindexed — try again after the scan.',
  );
  expect(screen.queryByTestId('file-block')).toBeNull();
});

// Markdown's own noise, and nothing else's: a Setext underline is the file's
// own line, and a closing `##` is a marker only when the leading one proved the
// text is ATX. An HTML/docx heading's `Chapter ##` is the document's own text.
test('a setext heading loses its underline and an ATX closing run its hashes', async () => {
  mockView({
    kind: 'file',
    blocks: [
      { blockId: 1, kind: 'headline', text: 'Заголовок\n=========', pageNo: 1, sectionTitle: null },
      { blockId: 2, kind: 'headline', text: '## Розділ ##', pageNo: 1, sectionTitle: null },
      { blockId: 3, kind: 'headline', text: 'Chapter ##', pageNo: 1, sectionTitle: null },
    ],
    focusBlockIds: [],
    spans: [],
    relativePath: 'notes/a.md',
    rootName: null,
    freshness: { kind: 'current' },
  });
  render(FilePane, { location, focus: null, onFocusHandled: () => {} });

  await screen.findAllByTestId('file-block');
  expect(blocks()).toEqual(['Заголовок', 'Розділ', 'Chapter ##']);
});

// A citation with no place on disk still names a document, and the backend
// resolves that document's sole `path` row for freshness — the same row is the
// title, so the header is not blank over a file the pane can place exactly.
test('a citation with no path is titled from the path the backend resolved', async () => {
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'paragraph', text: 'Без шляху в цитаті.', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [],
    spans: [],
    relativePath: 'notes/a.md',
    rootName: null,
    freshness: { kind: 'current' },
  });
  const onTitle = vi.fn();
  render(FilePane, {
    location: { documentId: 'doc-1', rootId: null, relativePath: null },
    focus: null,
    onFocusHandled: () => {},
    onTitle,
  });

  await waitFor(() => expect(onTitle).toHaveBeenLastCalledWith('notes/a.md'));
});

// The path is shown from the root the view answered for. The title is reported
// upward (the right panel's head draws it), and the card keeps none of its own.
test('the title is the root name and the path, reported through onTitle and not drawn in the card', async () => {
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'paragraph', text: 'Текст.', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [],
    spans: [],
    relativePath: 'notes/b.md',
    rootName: 'docs',
    freshness: { kind: 'current' },
  });
  const onTitle = vi.fn();
  render(FilePane, {
    location: { documentId: 'doc-1', rootId: 1, relativePath: 'notes/b.md' },
    focus: null,
    onFocusHandled: () => {},
    onTitle,
  });

  await screen.findByTestId('file-block');
  expect(onTitle).toHaveBeenLastCalledWith('docs/notes/b.md');
  expect(screen.queryByTestId('file-header')).toBeNull();
});

test('a view that names no root titles the file by its path alone', async () => {
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'paragraph', text: 'Текст.', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [],
    spans: [],
    relativePath: 'notes/b.md',
    rootName: null,
    freshness: { kind: 'current' },
  });
  const onTitle = vi.fn();
  render(FilePane, {
    location: { documentId: 'doc-1', rootId: 1, relativePath: 'notes/b.md' },
    focus: null,
    onFocusHandled: () => {},
    onTitle,
  });

  await screen.findByTestId('file-block');
  expect(onTitle).toHaveBeenLastCalledWith('notes/b.md');
});

// A stored citation outlives the chunks it names: a reindex or a reused chunk
// id leaves the focus pointing at nothing, and an empty `focusBlockIds` is the
// backend's answer. The whole file is still shown — and said to be the whole
// file, not the citation.
test('a focus the backend rejected says the passage is gone', async () => {
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'paragraph', text: 'Текст без того уривка.', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [],
    spans: [],
    relativePath: 'notes/a.md',
    rootName: null,
    freshness: { kind: 'current' },
  });
  render(FilePane, {
    location,
    focus: { chunkId: 9, ord: 0, text: 'зниклий уривок' },
    onFocusHandled: () => {},
  });

  expect((await screen.findByTestId('file-focus-gone')).textContent).toBe(
    'The passage this citation pointed at is no longer in this file.',
  );
  expect(screen.queryByTestId('hl')).toBeNull();
  expect(blocks()).toEqual(['Текст без того уривка.']);
});

// The other branch: a focus that was accepted says nothing at all.
test('an accepted focus draws no gone-passage line', async () => {
  mockView({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'paragraph', text: 'Ціна становить дві гривні.', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [1],
    spans: [{ blockId: 1, blockStart: 0, length: 4, primary: true }],
    relativePath: 'notes/a.md',
    rootName: null,
    freshness: { kind: 'current' },
  });
  render(FilePane, {
    location,
    focus: { chunkId: 42, ord: 0, text: 'Ціна' },
    onFocusHandled: () => {},
  });

  await screen.findByTestId('hl');
  expect(screen.queryByTestId('file-focus-gone')).toBeNull();
});

// §7.3: the window is hidden, not closed, so the file may have been edited in
// another application meanwhile. A focus refresh re-reads it — and it is not a
// new open: the citation is re-sent (so the highlight stays where the person
// left it) and the scroll offset is theirs, not reset by the repaint.
test('a window focus refreshes the file and keeps the highlight and the scroll', async () => {
  const focused = {
    kind: 'file' as const,
    blocks: [{ blockId: 1, kind: 'paragraph', text: 'Ціна становить дві гривні.', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [1],
    spans: [{ blockId: 1, blockStart: 0, length: 4, primary: true }],
    relativePath: 'notes/a.md',
    rootName: null,
    freshness: { kind: 'current' as const },
  };
  mockView(focused);
  render(FilePane, {
    location,
    focus: { chunkId: 42, ord: 0, text: 'Ціна' },
    onFocusHandled: () => {},
  });
  await screen.findByTestId('hl');

  // jsdom has no layout, so the scroller's offset is a plain property: this
  // reads and writes the same one the component does, and the mock read moves
  // it to the top the way a repaint over changed text does. Without the
  // restore, the assertion below reads 0.
  const scroller = screen.getByTestId('file-body').parentElement!;
  let offset = 0;
  Object.defineProperty(scroller, 'scrollTop', {
    get: () => offset,
    set: (v: number) => { offset = v; },
    configurable: true,
  });
  scroller.scrollTop = 120;

  const refreshed = { ...focused, freshness: { kind: 'fileChanged' as const } };
  invoke.mockImplementation((cmd: string) => {
    if (cmd !== 'open_document') return Promise.resolve();
    scroller.scrollTop = 0;
    return Promise.resolve(refreshed);
  });
  await fireEvent.focus(window);

  await waitFor(() => expect(screen.getByTestId('file-badge').textContent).toBe('The file changed after indexing'));
  expect(openCalls()).toHaveLength(2);
  expect(openCalls()[1][1]).toMatchObject({ chunkId: 42, citedOrd: 0, citedText: 'Ціна' });
  expect(screen.getByTestId('hl').textContent).toBe('Ціна');
  expect(scroller.scrollTop).toBe(120);
});

// A focus that lands while the first read is still on the wire is not a second
// read: the answer already coming is the newer one, and dropping it would drop
// the focus acknowledgement the caller is owed.
test('a window focus during the first read does not start a second one', async () => {
  let answer!: (v: DocumentView) => void;
  invoke.mockReturnValue(new Promise((r) => { answer = r; }));
  render(FilePane, { location, focus: { chunkId: 42, ord: 0, text: 'Ціна' }, onFocusHandled: () => {} });
  await screen.findByTestId('file-loading');

  await fireEvent.focus(window);
  expect(openCalls()).toHaveLength(1);

  answer({
    kind: 'file',
    blocks: [{ blockId: 1, kind: 'paragraph', text: 'Ціна становить дві гривні.', pageNo: 1, sectionTitle: null }],
    focusBlockIds: [1],
    spans: [{ blockId: 1, blockStart: 0, length: 4, primary: true }],
    relativePath: 'notes/a.md',
    rootName: null,
    freshness: { kind: 'current' },
  });
  expect((await screen.findByTestId('hl')).textContent).toBe('Ціна');
});

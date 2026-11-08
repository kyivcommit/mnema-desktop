import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import FilePane from './FilePane.svelte';
import { setLocale } from '../i18n';
import type { DocPageView, DocumentLocation, DocumentView } from '../lib/ipc';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
const scrollWithin = vi.fn();
vi.mock('../lib/scroll', () => ({ scrollWithin: (...a: unknown[]) => scrollWithin(...a) }));

const location: DocumentLocation = { documentId: 'md-1', rootId: 1, relativePath: 'notes/a.md' };
const spans = [{ blockId: 2, blockStart: 0, length: 5, primary: true }];

function md(over: Partial<Extract<DocumentView, { kind: 'file' }>> = {}): DocumentView {
  return {
    kind: 'file',
    mime: 'text/markdown',
    blocks: [
      { blockId: 1, kind: 'headline', text: '# Нотатки', pageNo: 1, sectionTitle: 'Нотатки' },
      { blockId: 2, kind: 'paragraph', text: 'Абзац про важливе.', pageNo: 1, sectionTitle: 'Нотатки' },
    ],
    focusBlockIds: [2],
    spans,
    relativePath: 'notes/a.md',
    rootName: 'docs',
    freshness: { kind: 'current' },
    ...over,
  };
}
const HTML = '<div data-block="1:0"><h1>Нотатки</h1></div>'
  + '<div data-block="1:1"><p><mark>Абзац</mark> про <mark data-primary="true">важливе</mark>.</p></div>';
const page = (over: Partial<Extract<DocPageView, { kind: 'page' }>> = {}): DocPageView =>
  ({ kind: 'page', html: HTML, pages: 1, highlight: 'shown', pageNo: null, ...over });

let view: DocumentView;
let docReply: (args: Record<string, unknown>) => Promise<unknown>;
const docCalls = () => invoke.mock.calls.filter(([c]) => c === 'render_doc').map(([, a]) => a as Record<string, unknown>);

beforeEach(() => {
  invoke.mockReset();
  scrollWithin.mockReset();
  view = md();
  docReply = () => Promise.resolve(page());
  invoke.mockImplementation((cmd: string, args: Record<string, unknown>) => {
    if (cmd === 'open_document') return Promise.resolve(view);
    if (cmd === 'render_doc') return docReply(args);
    return Promise.resolve();
  });
});
afterEach(() => setLocale('en'));

function mount() {
  // A scroller around the pane, as `.pane-body` is in the window.
  const scroller = document.createElement('div');
  document.body.appendChild(scroller);
  return render(FilePane, {
    target: scroller,
    props: { location, focus: null, onFocusHandled: () => {}, mode: 'page' },
  });
}

test('the HTML the backend answers is the page, asked with the pane\'s spans', async () => {
  mount();
  const body = await screen.findByTestId('doc-body');
  expect(body.querySelector('h1')!.textContent).toBe('Нотатки');
  expect(docCalls()).toEqual([
    { documentId: 'md-1', rootId: 1, relativePath: 'notes/a.md', spans, pageNo: null },
  ]);
  expect(screen.queryByTestId('file-block')).toBeNull();
});

test('the clicked citation is scrolled in, not the first mark', async () => {
  mount();
  await screen.findByTestId('doc-body');
  await waitFor(() => expect(scrollWithin).toHaveBeenCalled());
  const [, target] = scrollWithin.mock.calls[0] as [HTMLElement, HTMLElement];
  expect(target.textContent).toBe('важливе');
  expect(target.dataset.primary).toBe('true');
});

test('a changed file says so where a citation was asked for, and not otherwise', async () => {
  docReply = () => Promise.resolve(page({ highlight: 'changed' }));
  const first = mount();
  expect((await screen.findByTestId('page-note')).textContent).toMatch(/changed/i);
  first.unmount();
  view = md({ spans: [], focusBlockIds: [] });
  mount();
  await screen.findByTestId('doc-body');
  expect(screen.queryByTestId('page-note')).toBeNull();
});

test('a file too large for the page says so and offers the text', async () => {
  docReply = () => Promise.resolve({ kind: 'unavailable', reason: 'tooLarge' });
  mount();
  expect((await screen.findByTestId('page-unavailable')).textContent).toMatch(/too large to show as a page/i);
  expect(screen.getByTestId('page-show-text')).toBeTruthy();
});

test('a slow answer for an older draw does not paint over the newer one', async () => {
  mount();
  await screen.findByTestId('doc-body');
  let release!: (v: unknown) => void;
  const replies = [
    new Promise((r) => { release = r; }),
    Promise.resolve(page({ html: '<p>нове</p>' })),
  ];
  let n = 0;
  docReply = () => (replies[n++] ?? Promise.resolve(page())) as Promise<unknown>;
  // Two refreshes in a row (the window came back twice): the first is slow.
  window.dispatchEvent(new Event('focus'));
  await waitFor(() => expect(docCalls()).toHaveLength(2));
  window.dispatchEvent(new Event('focus'));
  await waitFor(() => expect(docCalls()).toHaveLength(3));
  await waitFor(() => expect(screen.getByTestId('doc-body').textContent).toBe('нове'));
  release(page({ html: '<p>старе</p>' }));
  await new Promise((r) => setTimeout(r, 0));
  expect(screen.getByTestId('doc-body').textContent).toBe('нове');
});

test('the window coming back draws the page again without blanking it', async () => {
  mount();
  await screen.findByTestId('doc-body');
  let release!: (v: unknown) => void;
  docReply = () => new Promise((r) => { release = r; });
  window.dispatchEvent(new Event('focus'));
  await waitFor(() => expect(docCalls()).toHaveLength(2));
  // Still up while the refresh is in flight.
  expect(screen.getByTestId('doc-body').querySelector('h1')).toBeTruthy();
  release(page({ html: '<p>оновлено</p>' }));
  await waitFor(() => expect(screen.getByTestId('doc-body').textContent).toBe('оновлено'));
  expect(scrollWithin).toHaveBeenCalledTimes(1);
});

test('a failed draw is not final: a later answer replaces the failure', async () => {
  docReply = () => Promise.reject(new Error('index busy'));
  mount();
  await screen.findByTestId('page-unavailable');
  docReply = () => Promise.resolve(page());
  window.dispatchEvent(new Event('focus'));
  expect((await screen.findByTestId('doc-body')).querySelector('h1')).toBeTruthy();
  expect(screen.queryByTestId('page-unavailable')).toBeNull();
});

// ── An EPUB, one chapter at a time ──────────────────────────────────────────

// Three chapters (pages 1–3), the citation in the second.
function epub(): DocumentView {
  return md({
    mime: 'application/epub+zip',
    blocks: [
      { blockId: 1, kind: 'paragraph', text: 'Перший.', pageNo: 1, sectionTitle: null },
      { blockId: 2, kind: 'paragraph', text: 'Абзац про важливе.', pageNo: 2, sectionTitle: null },
      { blockId: 4, kind: 'paragraph', text: 'Ще другий.', pageNo: 2, sectionTitle: null },
      { blockId: 3, kind: 'paragraph', text: 'Третій.', pageNo: 3, sectionTitle: null },
    ],
  });
}
// The backend's choice: the cited chapter when none is asked for.
const chapterReply = (args: Record<string, unknown>) => {
  const no = (args.pageNo as number | null) ?? 2;
  return Promise.resolve(page({ pageNo: no, html: `<p>розділ ${no}</p>` }));
};
const button = (which: 'prev' | 'next', n = 0) =>
  screen.getAllByTestId(`doc-chapter-${which}`)[n] as HTMLButtonElement;
const label = () => screen.getAllByTestId('doc-chapter-label')[0].textContent;
async function openEpub() {
  view = epub();
  docReply = chapterReply;
  mount();
  await waitFor(() => expect(screen.getByTestId('doc-body').textContent).toBe('розділ 2'));
}

test('an EPUB opens on the cited chapter with both neighbours offered, above and below', async () => {
  await openEpub();
  expect(docCalls()[0].pageNo).toBeNull();
  expect(label()).toBe('Chapter 2 of 3');
  expect(screen.getAllByTestId('doc-chapter-prev')).toHaveLength(2);
  expect(screen.getAllByTestId('doc-chapter-next')).toHaveLength(2);
  expect(button('prev').disabled).toBe(false);
  expect(button('next').disabled).toBe(false);
});

test('next and previous ask for the neighbour chapter; there is no next after the last', async () => {
  await openEpub();
  await fireEvent.click(button('next', 1));
  await waitFor(() => expect(screen.getByTestId('doc-body').textContent).toBe('розділ 3'));
  expect(docCalls().at(-1)!.pageNo).toBe(3);
  expect(docCalls().at(-1)!.spans).toEqual(spans);
  expect(label()).toBe('Chapter 3 of 3');
  expect(button('next').disabled).toBe(true);
  expect(button('next', 1).disabled).toBe(true);
  await fireEvent.click(button('prev'));
  await waitFor(() => expect(screen.getByTestId('doc-body').textContent).toBe('розділ 2'));
  expect(docCalls().at(-1)!.pageNo).toBe(2);
});

test('there is no previous chapter on the first', async () => {
  view = epub();
  docReply = (args) => Promise.resolve(page({ pageNo: (args.pageNo as number | null) ?? 1, html: '<p>перший</p>' }));
  mount();
  await screen.findByTestId('doc-body');
  expect(label()).toBe('Chapter 1 of 3');
  expect(button('prev').disabled).toBe(true);
  expect(button('prev', 1).disabled).toBe(true);
  expect(button('next').disabled).toBe(false);
});

test('a chapter without the citation is shown from its top', async () => {
  await openEpub();
  const scroller = screen.getByTestId('doc-page').parentElement!.parentElement!;
  scroller.scrollTop = 120;
  await fireEvent.click(button('next'));
  await waitFor(() => expect(screen.getByTestId('doc-body').textContent).toBe('розділ 3'));
  await waitFor(() => expect(scroller.scrollTop).toBe(0));
});

test('a Markdown or HTML file has no chapters', async () => {
  const first = mount();
  await screen.findByTestId('doc-body');
  expect(screen.queryByTestId('doc-chapter-next')).toBeNull();
  first.unmount();
  view = md({ mime: 'text/html' });
  // Even an answer that names a page draws no chapter bar for a whole file.
  docReply = () => Promise.resolve(page({ pageNo: 1 }));
  mount();
  await screen.findByTestId('doc-body');
  expect(screen.queryByTestId('doc-chapter-next')).toBeNull();
  expect(screen.queryByTestId('doc-chapter-label')).toBeNull();
});

test('a slow chapter is not painted once the person has moved on', async () => {
  await openEpub();
  let release!: (v: unknown) => void;
  docReply = (args) => args.pageNo === 3
    ? new Promise((r) => { release = r; })
    : chapterReply(args);
  await fireEvent.click(button('next'));
  await waitFor(() => expect(docCalls().at(-1)!.pageNo).toBe(3));
  await fireEvent.click(button('prev'));
  await waitFor(() => expect(screen.getByTestId('doc-body').textContent).toBe('розділ 2'));
  release(page({ pageNo: 3, html: '<p>розділ 3</p>' }));
  await new Promise((r) => setTimeout(r, 0));
  expect(screen.getByTestId('doc-body').textContent).toBe('розділ 2');
  expect(label()).toBe('Chapter 2 of 3');
});

test('the window coming back draws the chapter on the page, not the cited one', async () => {
  await openEpub();
  await fireEvent.click(button('next'));
  await waitFor(() => expect(screen.getByTestId('doc-body').textContent).toBe('розділ 3'));
  const before = docCalls().length;
  window.dispatchEvent(new Event('focus'));
  await waitFor(() => expect(docCalls().length).toBe(before + 1));
  expect(docCalls().at(-1)!.pageNo).toBe(3);
  await new Promise((r) => setTimeout(r, 0));
  expect(screen.getByTestId('doc-body').textContent).toBe('розділ 3');
});

test('a chapter with no indexed text says so and offers the text', async () => {
  view = epub();
  docReply = () => Promise.resolve({ kind: 'unavailable', reason: 'noChapter' });
  mount();
  expect((await screen.findByTestId('page-unavailable')).textContent).toBe('This chapter has no indexed text.');
  expect(screen.getByTestId('page-show-text')).toBeTruthy();
});

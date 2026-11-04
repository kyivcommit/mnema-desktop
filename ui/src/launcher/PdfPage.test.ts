import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import FilePane from './FilePane.svelte';
import { setLocale } from '../i18n';
import type { DocumentLocation, DocumentView, PdfPageView } from '../lib/ipc';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
vi.mock('../lib/scroll', () => ({ scrollWithin: vi.fn() }));

const location: DocumentLocation = { documentId: 'doc-1', rootId: 1, relativePath: 'papers/a.pdf' };

// One block per page, as a PDF is read.
function pdf(over: Partial<Extract<DocumentView, { kind: 'file' }>> = {}): DocumentView {
  return {
    kind: 'file',
    mime: 'application/pdf',
    blocks: [
      { blockId: 10, kind: 'paragraph', text: 'Перша сторінка.', pageNo: 1, sectionTitle: null },
      { blockId: 20, kind: 'paragraph', text: 'Друга сторінка.', pageNo: 2, sectionTitle: null },
      { blockId: 30, kind: 'paragraph', text: 'Третя сторінка.', pageNo: 3, sectionTitle: null },
    ],
    focusBlockIds: [],
    spans: [],
    relativePath: 'papers/a.pdf',
    rootName: 'docs',
    freshness: { kind: 'current' },
    ...over,
  };
}
const page = (over: Partial<Extract<PdfPageView, { kind: 'page' }>> = {}): PdfPageView => ({
  kind: 'page', pages: 3, widthPx: 600, heightPx: 800, image: 'data:image/png;base64,AAAA',
  highlight: 'shown', rects: [], ...over,
});

let view: DocumentView;
// What `render_pdf_page` answers; a test swaps it to hold or reject a reply.
let renderReply: (args: Record<string, unknown>) => Promise<unknown>;
const renderCalls = () => invoke.mock.calls.filter(([cmd]) => cmd === 'render_pdf_page').map(([, a]) => a as Record<string, unknown>);
const openCalls = () => invoke.mock.calls.filter(([cmd]) => cmd === 'open_document');

// jsdom has no layout: the pane's height (FilePane alone has no `.pane-body`,
// so the window's) and the observer that watches it are stood in for here.
function paneHeight(px: number) {
  Object.defineProperty(window, 'innerHeight', { configurable: true, value: px });
}
let resize: (() => void) | null = null;
class FakeObserver {
  constructor(cb: () => void) { resize = cb; }
  observe() {}
  disconnect() { resize = null; }
}

beforeEach(() => {
  invoke.mockReset();
  view = pdf();
  renderReply = () => Promise.resolve(page());
  invoke.mockImplementation((cmd: string, args: Record<string, unknown>) => {
    if (cmd === 'open_document') return Promise.resolve(view);
    if (cmd === 'render_pdf_page') return renderReply(args);
    return Promise.resolve();
  });
  paneHeight(856); // 800 px of page at 0.75 = 600 px wide
  Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: 1 });
  vi.stubGlobal('ResizeObserver', FakeObserver);
});
afterEach(() => { setLocale('en'); vi.unstubAllGlobals(); });

function mount(props: Record<string, unknown> = {}) {
  return render(FilePane, { location, focus: null, onFocusHandled: () => {}, mode: 'page', ...props });
}
const FOCUS = { chunkId: 7, ord: 0, text: 'x' };
const image = () => screen.getByTestId('page-image').getAttribute('src');

test('the page shown is the cited block\'s page, else the first', async () => {
  view = pdf({ focusBlockIds: [20] });
  const { unmount } = mount({ focus: FOCUS });
  await screen.findByTestId('page-image');
  expect(renderCalls()[0].pageNo).toBe(2);
  unmount();

  invoke.mockClear();
  view = pdf();
  mount();
  await screen.findByTestId('page-image');
  expect(renderCalls()[0].pageNo).toBe(1);
});

test('only the spans of the shown page\'s block are sent as ranges', async () => {
  view = pdf({
    focusBlockIds: [20],
    spans: [
      { blockId: 10, blockStart: 1, length: 2, primary: false },
      { blockId: 20, blockStart: 3, length: 4, primary: true },
      { blockId: 30, blockStart: 5, length: 6, primary: false },
    ],
  });
  mount({ focus: FOCUS });
  await screen.findByTestId('page-image');
  expect(renderCalls()[0]).toEqual({
    documentId: 'doc-1', rootId: 1, relativePath: 'papers/a.pdf', pageNo: 2, widthPx: 600,
    ranges: [{ blockId: 20, blockStart: 3, length: 4, primary: true }],
  });
});

test('boxes sit over the picture in percent of it, the primary one marked', async () => {
  renderReply = () => Promise.resolve(page({
    rects: [{ x: 0.1, y: 0.2, w: 0.3, h: 0.4, primary: true }, { x: 0.5, y: 0.6, w: 0.25, h: 0.05, primary: false }],
  }));
  mount();
  await screen.findByTestId('page-image');
  const boxes = screen.getAllByTestId('page-box');
  const at = (el: HTMLElement) => ['left', 'top', 'width', 'height'].map((p) => parseFloat(el.style.getPropertyValue(p)));
  expect(boxes).toHaveLength(2);
  expect(at(boxes[0])).toEqual([10, 20, 30, 40].map((n) => expect.closeTo(n, 6)));
  expect(boxes[0].style.left.endsWith('%')).toBe(true);
  expect(at(boxes[1])).toEqual([50, 60, 25, 5].map((n) => expect.closeTo(n, 6)));
  expect(boxes[0].getAttribute('data-primary')).toBe('true');
  expect(boxes[1].getAttribute('data-primary')).toBeNull();
  expect(screen.getByTestId('page-image').parentElement!.contains(boxes[0])).toBe(true);
});

test('the first load says it is loading; a later page keeps the old picture meanwhile', async () => {
  let release!: (v: unknown) => void;
  renderReply = (a) => a.pageNo === 1
    ? Promise.resolve(page({ image: 'data:image/png;base64,ONE' }))
    : new Promise((r) => { release = r; });
  mount();
  // Before the first answer: the loading line, no picture.
  expect(screen.getByTestId('file-loading')).toBeTruthy();
  await screen.findByTestId('page-image');
  expect(screen.queryByTestId('file-loading')).toBeNull();

  await fireEvent.click(screen.getByTestId('page-next'));
  await waitFor(() => expect(renderCalls()).toHaveLength(2));
  expect(image()).toBe('data:image/png;base64,ONE');
  expect(screen.queryByTestId('file-loading')).toBeNull();
  release(page({ image: 'data:image/png;base64,TWO' }));
  await waitFor(() => expect(image()).toBe('data:image/png;base64,TWO'));
});

test('a late answer for an older request is not painted', async () => {
  const held: Array<(v: unknown) => void> = [];
  renderReply = (a) => a.pageNo === 1
    ? Promise.resolve(page())
    : new Promise((r) => { held.push(r); });
  mount();
  await screen.findByTestId('page-image');

  await fireEvent.click(screen.getByTestId('page-next')); // page 2, held
  await fireEvent.click(screen.getByTestId('page-next')); // page 3, held
  await waitFor(() => expect(held).toHaveLength(2));
  held[1](page({ image: 'data:image/png;base64,THREE' }));
  await waitFor(() => expect(image()).toBe('data:image/png;base64,THREE'));
  held[0](page({ image: 'data:image/png;base64,TWO' }));
  await new Promise((r) => setTimeout(r, 20));
  expect(image()).toBe('data:image/png;base64,THREE');
});

test('a late failure for an older request does not replace a newer page', async () => {
  const error = vi.spyOn(console, 'error').mockImplementation(() => {});
  let failTwo: (e: unknown) => void = () => {};
  renderReply = (a) => a.pageNo === 2
    ? new Promise((_, reject) => { failTwo = reject; })
    : Promise.resolve(page({ image: `data:image/png;base64,P${a.pageNo}` }));
  mount();
  await screen.findByTestId('page-image');

  await fireEvent.click(screen.getByTestId('page-next')); // page 2, held
  await fireEvent.click(screen.getByTestId('page-next')); // page 3, answered
  await waitFor(() => expect(image()).toBe('data:image/png;base64,P3'));
  failTwo('boom');
  await new Promise((r) => setTimeout(r, 20));
  expect(image()).toBe('data:image/png;base64,P3');
  expect(screen.queryByTestId('page-unavailable')).toBeNull();
  error.mockRestore();
});

test('prev and next stop at the first and the last page', async () => {
  mount();
  await screen.findByTestId('page-image');
  const prev = screen.getByTestId('page-prev') as HTMLButtonElement;
  const next = screen.getByTestId('page-next') as HTMLButtonElement;
  expect(screen.getByTestId('page-of').textContent).toBe('page 1 of 3');
  expect(prev.disabled).toBe(true);
  expect(next.disabled).toBe(false);

  await fireEvent.click(next);
  await waitFor(() => expect(screen.getByTestId('page-of').textContent).toBe('page 2 of 3'));
  expect(prev.disabled).toBe(false);
  await fireEvent.click(next);
  await waitFor(() => expect(screen.getByTestId('page-of').textContent).toBe('page 3 of 3'));
  expect(next.disabled).toBe(true);
  expect(renderCalls().map((c) => c.pageNo)).toEqual([1, 2, 3]);
});

test('the arrow keys turn the page inside the pane, and stop at both ends', async () => {
  mount();
  await screen.findByTestId('page-image');
  const pane = screen.getByTestId('pdf-page');

  await fireEvent.keyDown(pane, { key: 'ArrowLeft' }); // already on page 1
  await new Promise((r) => setTimeout(r, 10));
  expect(renderCalls().map((c) => c.pageNo)).toEqual([1]);

  await fireEvent.keyDown(pane, { key: 'ArrowRight' });
  await waitFor(() => expect(screen.getByTestId('page-of').textContent).toBe('page 2 of 3'));
  await fireEvent.keyDown(pane, { key: 'ArrowRight' });
  await waitFor(() => expect(screen.getByTestId('page-of').textContent).toBe('page 3 of 3'));
  await fireEvent.keyDown(pane, { key: 'ArrowRight' }); // past the last
  await new Promise((r) => setTimeout(r, 10));
  expect(renderCalls().map((c) => c.pageNo)).toEqual([1, 2, 3]);

  await fireEvent.keyDown(pane, { key: 'ArrowLeft' });
  await waitFor(() => expect(screen.getByTestId('page-of').textContent).toBe('page 2 of 3'));
});

test('the arrow keys do not turn before the first page has come, when the count is unknown', async () => {
  let answer: (v: unknown) => void = () => {};
  renderReply = () => new Promise((resolve) => { answer = resolve; });
  mount();
  const pane = await screen.findByTestId('pdf-page');
  await screen.findByTestId('file-loading');
  await fireEvent.keyDown(pane, { key: 'ArrowRight' });
  await fireEvent.keyDown(pane, { key: 'ArrowRight' });
  await new Promise((r) => setTimeout(r, 10));
  expect(renderCalls().map((c) => c.pageNo)).toEqual([1]);

  answer(page());
  await waitFor(() => expect(screen.getByTestId('page-of').textContent).toBe('page 1 of 3'));
  await fireEvent.keyDown(pane, { key: 'ArrowRight' });
  await waitFor(() => expect(screen.getByTestId('page-of').textContent).toBe('page 2 of 3'));
});

test('each note state says its line, and shown and noText say nothing', async () => {
  const note = async (highlight: 'shown' | 'changed' | 'scan' | 'noText' | 'unplaced') => {
    invoke.mockClear();
    renderReply = () => Promise.resolve(page({ highlight }));
    const { unmount } = mount();
    await screen.findByTestId('page-image');
    const el = screen.queryByTestId('page-note');
    const text = el?.textContent ?? null;
    unmount();
    return text;
  };
  expect(await note('changed')).toBe('The file changed after indexing — there is no highlight. The citation text is in “Text” mode.');
  expect(await note('scan')).toBe('This page is a scan — the highlight is only available in “Text” mode.');
  expect(await note('unplaced')).toBe('The passage could not be marked on this page — it is shown in “Text” mode.');
  expect(await note('shown')).toBeNull();
  expect(await note('noText')).toBeNull();

  setLocale('uk');
  expect(await note('changed')).toBe('Файл змінено після індексації — підсвітки немає. Текст цитати — у режимі «Текст».');
  expect(await note('scan')).toBe('Сторінка зі скану — підсвітка лише в режимі «Текст».');
  expect(await note('unplaced')).toBe('Уривок не вдалося позначити на сторінці — він є в режимі «Текст».');
});

test('an unavailable page says so and offers the text, which switches the mode', async () => {
  const onShowText = vi.fn();
  renderReply = () => Promise.resolve({ kind: 'unavailable', reason: 'failed' });
  mount({ onShowText });
  expect((await screen.findByTestId('page-unavailable')).textContent).toBe('The page could not be shown.');
  expect(screen.queryByTestId('page-image')).toBeNull();
  await fireEvent.click(screen.getByTestId('page-show-text'));
  expect(onShowText).toHaveBeenCalledTimes(1);
});

test('a missing file has its own line, and so does a rejected command', async () => {
  renderReply = () => Promise.resolve({ kind: 'unavailable', reason: 'fileMissing' });
  const { unmount } = mount();
  expect((await screen.findByTestId('page-unavailable')).textContent).toBe('The file is missing from disk.');
  unmount();

  const error = vi.spyOn(console, 'error').mockImplementation(() => {});
  renderReply = () => Promise.reject('boom');
  mount();
  expect((await screen.findByTestId('page-unavailable')).textContent).toBe('The page could not be shown.');
  expect(screen.getByTestId('page-show-text')).toBeTruthy();
  error.mockRestore();
});

test('the window coming back reads the document again and draws the same page again', async () => {
  mount();
  await screen.findByTestId('page-image');
  await fireEvent.click(screen.getByTestId('page-next'));
  await waitFor(() => expect(screen.getByTestId('page-of').textContent).toBe('page 2 of 3'));
  const opened = openCalls().length;
  const drawn = renderCalls().length;

  await fireEvent.focus(window);
  await waitFor(() => expect(renderCalls().length).toBe(drawn + 1));
  expect(openCalls().length).toBe(opened + 1);
  expect(renderCalls().at(-1)!.pageNo).toBe(2);
});

test('the page is as tall as the pane: the width asked follows, rounded up to 200 and capped at 3000', async () => {
  paneHeight(753); // 697 tall x 0.75 = 522.75 wide, x2 = 1045.5 -> 1200
  Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: 2 });
  const { unmount } = mount();
  await screen.findByTestId('page-image');
  expect(renderCalls().map((c) => c.widthPx)).toEqual([1200]);
  unmount();

  invoke.mockClear();
  paneHeight(3000);
  mount();
  await waitFor(() => expect(renderCalls()).toHaveLength(1));
  expect(renderCalls()[0].widthPx).toBe(3000);
});

test('a resize asks again only when the rounded width changes', async () => {
  paneHeight(753);
  Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: 2 });
  mount();
  await screen.findByTestId('page-image');
  expect(renderCalls()).toHaveLength(1);

  paneHeight(840); // 784 x 0.75 x 2 = 1176 -> still 1200
  resize!();
  await new Promise((r) => setTimeout(r, 20));
  expect(renderCalls()).toHaveLength(1);

  paneHeight(990); // 934 x 0.75 x 2 = 1401 -> 1600
  resize!();
  await waitFor(() => expect(renderCalls().map((c) => c.widthPx)).toEqual([1200, 1600]));
});

test('the text mode draws no page, and neither does a document that is not a PDF', async () => {
  const { unmount } = mount({ mode: 'text' });
  await screen.findAllByTestId('file-block');
  expect(renderCalls()).toHaveLength(0);
  expect(screen.queryByTestId('pdf-page')).toBeNull();
  unmount();

  view = pdf({ mime: 'text/plain' });
  mount();
  await screen.findAllByTestId('file-block');
  expect(renderCalls()).toHaveLength(0);
});

test('the pane reports the document\'s type upward', async () => {
  const onMime = vi.fn();
  mount({ onMime });
  await screen.findByTestId('page-image');
  expect(onMime).toHaveBeenLastCalledWith('application/pdf');
});

test('the image is drawn as tall as the pane, less the page line', async () => {
  paneHeight(856);
  mount();
  const img = await screen.findByTestId('page-image');
  expect(img.style.height).toBe('800px');
});

test('a page of another shape asks again for the width that fills the height', async () => {
  // 856 - 56 = 800 tall; portrait guess 0.75 -> 600; the answer is landscape 4:3.
  renderReply = () => Promise.resolve(page({ widthPx: 800, heightPx: 600 }));
  mount();
  await waitFor(() => expect(renderCalls().map((c) => c.widthPx)).toEqual([600, 1200]));
});

test('a new page opens scrolled sideways to its citation, and a redraw keeps the person\'s scroll', async () => {
  Object.defineProperty(HTMLElement.prototype, 'scrollWidth', { configurable: true, get: () => 2000 });
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', { configurable: true, get: () => 500 });
  renderReply = () => Promise.resolve(page({
    rects: [{ x: 0.1, y: 0.1, w: 0.1, h: 0.01, primary: false }, { x: 0.5, y: 0.2, w: 0.1, h: 0.01, primary: true }],
  }));
  try {
    mount();
    const strip = await screen.findByTestId('page-strip');
    // The primary box: 0.5 x 2000 = 1000, 200 wide, centred in 500 -> 1000 - 150.
    await waitFor(() => expect(strip.scrollLeft).toBe(850));

    strip.scrollLeft = 0;
    const drawn = renderCalls().length;
    await fireEvent.focus(window);
    await waitFor(() => expect(renderCalls().length).toBe(drawn + 1));
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.getByTestId('page-strip').scrollLeft).toBe(0);
  } finally {
    delete (HTMLElement.prototype as unknown as Record<string, unknown>).scrollWidth;
    delete (HTMLElement.prototype as unknown as Record<string, unknown>).clientWidth;
  }
});

test('a note above the page takes its height from the page, not from below the pane', async () => {
  const real = HTMLElement.prototype.getBoundingClientRect;
  HTMLElement.prototype.getBoundingClientRect = function () {
    const top = this.dataset.testid === 'page-strip' ? 150 : 0;
    return { top, bottom: top, left: 0, right: 0, width: 0, height: 0, x: 0, y: top, toJSON() {} } as DOMRect;
  };
  renderReply = () => Promise.resolve(page({ highlight: 'scan' }));
  try {
    paneHeight(856);
    mount();
    await screen.findByTestId('page-note');
    // 856 - 150 (the strip's top, under the note and the page line) - 12 = 694.
    await waitFor(() => expect(screen.getByTestId('page-image').style.height).toBe('694px'));
  } finally {
    HTMLElement.prototype.getBoundingClientRect = real;
  }
});

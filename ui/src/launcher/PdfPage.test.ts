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
// The strip's geometry in jsdom: 856 px of pane, less the guessed 44 px above
// and 12 px below, is 800 px of page; slots are 800 + 12 apart.
const TALL = 800;
const STEP = TALL + 12;

// A document of `n` pages, one block per page (block id = page x 10).
function book(n: number, over: Partial<Extract<DocumentView, { kind: 'file' }>> = {}): DocumentView {
  return pdf({
    blocks: Array.from({ length: n }, (_, i) => ({
      blockId: (i + 1) * 10, kind: 'paragraph' as const, text: `Сторінка ${i + 1}.`, pageNo: i + 1, sectionTitle: null,
    })),
    ...over,
  });
}
const slot = (n: number) => screen.getAllByTestId('page-slot').find((s) => s.dataset.page === String(n))!;
const imageOf = (n: number) => slot(n).querySelector('[data-testid="page-image"]')?.getAttribute('src') ?? null;
const asked = () => renderCalls().map((c) => c.pageNo);
const settle = () => new Promise((r) => setTimeout(r, 20));
async function scrollTo(n: number) {
  document.documentElement.scrollTop = (n - 1) * STEP;
  await fireEvent.scroll(window);
}
// Held answers by page, released by the test.
function holding() {
  const held = new Map<number, (v: unknown) => void>();
  renderReply = (a) => new Promise((r) => { held.set(a.pageNo as number, r); });
  return held;
}
afterEach(() => { document.documentElement.scrollTop = 0; });

test('the cited page is asked for first and alone; then the pages either side', async () => {
  view = book(3, { focusBlockIds: [20] });
  const held = holding();
  mount({ focus: FOCUS });
  await waitFor(() => expect(asked()).toEqual([2]));
  held.get(2)!(page({ pages: 3 }));
  await waitFor(() => expect(asked()).toEqual([2, 1, 3]));
});

test('with nothing cited the strip starts at the first page', async () => {
  view = book(3);
  mount();
  await waitFor(() => expect(asked()).toEqual([1, 2]));
});

test('each page is sent its own spans as ranges', async () => {
  view = book(3, {
    focusBlockIds: [20],
    spans: [
      { blockId: 10, blockStart: 1, length: 2, primary: false },
      { blockId: 20, blockStart: 3, length: 4, primary: true },
      { blockId: 30, blockStart: 5, length: 6, primary: false },
    ],
  });
  mount({ focus: FOCUS });
  await waitFor(() => expect(renderCalls()).toHaveLength(3));
  expect(renderCalls()[0]).toEqual({
    documentId: 'doc-1', rootId: 1, relativePath: 'papers/a.pdf', pageNo: 2, widthPx: 600,
    ranges: [{ blockId: 20, blockStart: 3, length: 4, primary: true }],
  });
  expect(renderCalls()[1].ranges).toEqual([{ blockId: 10, blockStart: 1, length: 2, primary: false }]);
  expect(renderCalls()[2].ranges).toEqual([{ blockId: 30, blockStart: 5, length: 6, primary: false }]);
});

test('boxes sit over their own page\'s picture in percent of it, the primary one marked', async () => {
  renderReply = (a) => Promise.resolve(page(a.pageNo === 1 ? {
    rects: [{ x: 0.1, y: 0.2, w: 0.3, h: 0.4, primary: true }, { x: 0.5, y: 0.6, w: 0.25, h: 0.05, primary: false }],
  } : {}));
  mount();
  await waitFor(() => expect(imageOf(2)).not.toBeNull());
  const boxes = screen.getAllByTestId('page-box');
  const at = (el: HTMLElement) => ['left', 'top', 'width', 'height'].map((p) => parseFloat(el.style.getPropertyValue(p)));
  expect(boxes).toHaveLength(2);
  expect(at(boxes[0])).toEqual([10, 20, 30, 40].map((n) => expect.closeTo(n, 6)));
  expect(boxes[0].style.left.endsWith('%')).toBe(true);
  expect(at(boxes[1])).toEqual([50, 60, 25, 5].map((n) => expect.closeTo(n, 6)));
  expect(boxes[0].getAttribute('data-primary')).toBe('true');
  expect(boxes[1].getAttribute('data-primary')).toBeNull();
  const img = slot(1).querySelector('[data-testid="page-image"]')!;
  expect(img.parentElement!.contains(boxes[0])).toBe(true);
});

test('the first load says it is loading; a page not drawn yet holds its place', async () => {
  const held = holding();
  mount();
  expect(screen.getByTestId('file-loading')).toBeTruthy();
  await waitFor(() => expect(held.has(1)).toBe(true));
  held.get(1)!(page({ image: 'data:image/png;base64,ONE' }));
  await waitFor(() => expect(imageOf(1)).toBe('data:image/png;base64,ONE'));
  expect(screen.queryByTestId('file-loading')).toBeNull();
  expect(imageOf(2)).toBeNull();
  expect(slot(2).querySelector('[data-testid="page-wait"]')!.getAttribute('aria-busy')).toBe('true');
  expect((slot(2).querySelector('[data-testid="page-wait"]') as HTMLElement).style.width).toBe('600px');
});

test('every slot is as tall as the page, and the strip opens with the cited page at the top', async () => {
  view = book(5, { focusBlockIds: [40] });
  renderReply = () => Promise.resolve(page({ pages: 5 }));
  mount({ focus: FOCUS });
  await waitFor(() => expect(imageOf(4)).not.toBeNull());
  for (const n of [1, 2, 3, 4, 5]) {
    expect(slot(n).style.height).toBe(`${TALL}px`);
    expect(slot(n).style.marginBottom).toBe('12px');
  }
  await waitFor(() => expect(document.documentElement.scrollTop).toBe(3 * STEP));
});

test('no more than two pages are on their way at once', async () => {
  view = book(10, { focusBlockIds: [30] });
  const held = holding();
  mount({ focus: FOCUS });
  await waitFor(() => expect(asked()).toEqual([3]));
  held.get(3)!(page({ pages: 10 }));
  await waitFor(() => expect(asked()).toEqual([3, 2, 4]));

  await scrollTo(7); // 7, 6, 8 wanted; two are still on their way
  await settle();
  expect(asked()).toEqual([3, 2, 4]);
  held.get(2)!(page({ pages: 10 }));
  await waitFor(() => expect(asked()).toEqual([3, 2, 4, 7]));
});

test('scrolling draws the page in view and its neighbours, and the page line follows', async () => {
  view = book(10);
  renderReply = () => Promise.resolve(page({ pages: 10 }));
  mount();
  await waitFor(() => expect(asked()).toEqual([1, 2]));
  expect(screen.getByTestId('page-of').textContent).toBe('page 1 of 10');

  await scrollTo(6);
  await waitFor(() => expect(asked()).toEqual([1, 2, 6, 5, 7]));
  expect(screen.getByTestId('page-of').textContent).toBe('page 6 of 10');
  expect(screen.getByTestId('page-of').getAttribute('aria-live')).toBeNull();
});

test('past twelve pictures the farthest from the page in view are let go', async () => {
  view = book(30);
  renderReply = (a) => Promise.resolve(page({ pages: 30, image: `data:image/png;base64,P${a.pageNo}` }));
  mount();
  await waitFor(() => expect(imageOf(2)).not.toBeNull());
  for (let n = 2; n <= 20; n++) {
    await scrollTo(n);
    await waitFor(() => expect(imageOf(Math.min(n + 1, 30))).not.toBeNull());
  }
  const drawn = screen.getAllByTestId('page-slot').filter((s) => s.querySelector('[data-testid="page-image"]'));
  expect(drawn.map((s) => Number(s.dataset.page))).toEqual([10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21]);

  // Back two pages: still there, nothing asked again.
  const before = renderCalls().length;
  await scrollTo(18);
  await settle();
  expect(renderCalls().length).toBe(before);
});

test('the window coming back draws the pages in view again, the citation boxed again', async () => {
  view = book(3, { focusBlockIds: [20], spans: [{ blockId: 20, blockStart: 3, length: 4, primary: true }] });
  renderReply = (a) => Promise.resolve(page({
    image: `data:image/png;base64,A${a.pageNo}`,
    rects: (a.ranges as unknown[]).length > 0 ? [{ x: 0.1, y: 0.1, w: 0.1, h: 0.1, primary: true }] : [],
  }));
  mount({ focus: FOCUS });
  await waitFor(() => expect(renderCalls()).toHaveLength(3));
  const opened = openCalls().length;

  const held = holding();
  await fireEvent.focus(window);
  await waitFor(() => expect(held.has(2)).toBe(true));
  expect(openCalls().length).toBe(opened + 1);
  expect(renderCalls().slice(3).find((c) => c.pageNo === 2)!.ranges).toEqual([{ blockId: 20, blockStart: 3, length: 4, primary: true }]);
  // The old picture stays up until the new one comes.
  expect(imageOf(2)).toBe('data:image/png;base64,A2');
  held.get(2)!(page({ image: 'data:image/png;base64,B2', rects: [{ x: 0.2, y: 0.2, w: 0.1, h: 0.1, primary: true }] }));
  await waitFor(() => expect(imageOf(2)).toBe('data:image/png;base64,B2'));
  expect(slot(2).querySelectorAll('[data-testid="page-box"]')).toHaveLength(1);
});

test('a late answer asked before the window came back is not painted', async () => {
  view = book(3);
  const first = holding();
  mount();
  await waitFor(() => expect(first.has(1)).toBe(true));
  first.get(1)!(page());
  await waitFor(() => expect(first.has(2)).toBe(true)); // page 2, held

  const second = holding();
  await fireEvent.focus(window);
  await waitFor(() => expect(second.has(2)).toBe(true));
  second.get(2)!(page({ image: 'data:image/png;base64,NEW' }));
  await waitFor(() => expect(imageOf(2)).toBe('data:image/png;base64,NEW'));
  first.get(2)!(page({ image: 'data:image/png;base64,OLD' }));
  await settle();
  expect(imageOf(2)).toBe('data:image/png;base64,NEW');
});

test('there is no turning: no buttons, and the arrow keys ask for nothing', async () => {
  mount();
  await waitFor(() => expect(asked()).toEqual([1, 2]));
  expect(screen.queryByTestId('page-prev')).toBeNull();
  expect(screen.queryByTestId('page-next')).toBeNull();
  await fireEvent.keyDown(screen.getByTestId('pdf-page'), { key: 'ArrowRight' });
  await fireEvent.keyDown(screen.getByTestId('pdf-page'), { key: 'ArrowLeft' });
  await settle();
  expect(asked()).toEqual([1, 2]);
});

test('each page has its number in the picture\'s text', async () => {
  mount();
  await waitFor(() => expect(imageOf(2)).not.toBeNull());
  expect(slot(2).querySelector('img')!.getAttribute('alt')).toBe('Page 2');
});

test('each note says its line over the cited page, and a page with nothing cited has none', async () => {
  const note = async (highlight: 'shown' | 'changed' | 'scan' | 'noText' | 'unplaced') => {
    invoke.mockClear();
    view = book(3, { focusBlockIds: [20], spans: [{ blockId: 20, blockStart: 1, length: 2, primary: true }] });
    renderReply = () => Promise.resolve(page({ highlight }));
    const { unmount } = mount({ focus: FOCUS });
    await waitFor(() => expect(renderCalls()).toHaveLength(3));
    await waitFor(() => expect(imageOf(3)).not.toBeNull());
    expect(slot(1).querySelector('[data-testid="page-note"]')).toBeNull();
    expect(slot(3).querySelector('[data-testid="page-note"]')).toBeNull();
    const text = slot(2).querySelector('[data-testid="page-note"]')?.textContent ?? null;
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

test('a page that could not be drawn says so in its own place; the rest of the strip is drawn', async () => {
  const error = vi.spyOn(console, 'error').mockImplementation(() => {});
  view = book(3, { focusBlockIds: [20] });
  renderReply = (a) => a.pageNo === 1 ? Promise.reject('boom')
    : a.pageNo === 3 ? Promise.resolve({ kind: 'unavailable', reason: 'failed' })
    : Promise.resolve(page());
  mount({ focus: FOCUS });
  await waitFor(() => expect(slot(1).querySelector('[data-testid="page-failed"]')).not.toBeNull());
  await waitFor(() => expect(slot(3).querySelector('[data-testid="page-failed"]')!.textContent).toBe('The page could not be shown.'));
  expect(imageOf(2)).not.toBeNull();
  expect(screen.queryByTestId('page-unavailable')).toBeNull();
  error.mockRestore();
});

test('a file that cannot be drawn says so for the whole document and offers the text', async () => {
  const onShowText = vi.fn();
  renderReply = () => Promise.resolve({ kind: 'unavailable', reason: 'failed' });
  const { unmount } = mount({ onShowText });
  expect((await screen.findByTestId('page-unavailable')).textContent).toBe('The page could not be shown.');
  expect(screen.queryByTestId('page-image')).toBeNull();
  await fireEvent.click(screen.getByTestId('page-show-text'));
  expect(onShowText).toHaveBeenCalledTimes(1);
  unmount();

  renderReply = () => Promise.resolve({ kind: 'unavailable', reason: 'fileMissing' });
  const second = mount();
  expect((await screen.findByTestId('page-unavailable')).textContent).toBe('The file is missing from disk.');
  second.unmount();

  // A neighbour that finds the file gone takes the whole strip down too.
  view = book(3);
  renderReply = (a) => Promise.resolve(a.pageNo === 1 ? page() : { kind: 'unavailable', reason: 'fileMissing' });
  const third = mount();
  expect((await screen.findByTestId('page-unavailable')).textContent).toBe('The file is missing from disk.');
  third.unmount();

  const error = vi.spyOn(console, 'error').mockImplementation(() => {});
  renderReply = () => Promise.reject('boom');
  mount();
  expect((await screen.findByTestId('page-unavailable')).textContent).toBe('The page could not be shown.');
  expect(screen.getByTestId('page-show-text')).toBeTruthy();
  error.mockRestore();
});

test('the page is as tall as the pane: the width asked follows, rounded up to 200 and capped at 3000', async () => {
  paneHeight(753); // 697 tall x 0.75 = 522.75 wide, x2 = 1045.5 -> 1200
  Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: 2 });
  const { unmount } = mount();
  await waitFor(() => expect(renderCalls()).toHaveLength(2));
  expect(renderCalls().map((c) => c.widthPx)).toEqual([1200, 1200]);
  unmount();

  invoke.mockClear();
  paneHeight(3000);
  mount();
  await waitFor(() => expect(renderCalls().length).toBeGreaterThan(0));
  expect(renderCalls()[0].widthPx).toBe(3000);
});

test('a resize asks again only when the rounded width changes', async () => {
  paneHeight(753);
  Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: 2 });
  mount();
  await waitFor(() => expect(renderCalls()).toHaveLength(2));

  paneHeight(840); // 784 x 0.75 x 2 = 1176 -> still 1200
  resize!();
  await settle();
  expect(renderCalls()).toHaveLength(2);

  paneHeight(990); // 934 x 0.75 x 2 = 1401 -> 1600
  resize!();
  await waitFor(() => expect(renderCalls().map((c) => c.widthPx)).toEqual([1200, 1200, 1600, 1600]));
});

test('the cited page\'s shape sets the width asked; a neighbour of another shape does not', async () => {
  // 800 tall; portrait guess 0.75 -> 600; the cited page is landscape 4:3.
  // A strip that took every page's shape would ask again for ever: past a
  // handful of calls the answers stop, so that shows as a count, not a hang.
  renderReply = (a) => renderCalls().length > 10 ? new Promise(() => {})
    : Promise.resolve(a.pageNo === 1 ? page({ widthPx: 800, heightPx: 600 }) : page({ widthPx: 300, heightPx: 900 }));
  mount();
  await waitFor(() => expect(renderCalls().map((c) => [c.pageNo, c.widthPx])).toEqual([[1, 600], [1, 1200], [2, 1200]]));
  await settle();
  expect(renderCalls()).toHaveLength(3);
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
  await waitFor(() => expect(asked().length).toBeGreaterThan(0));
  expect(onMime).toHaveBeenLastCalledWith('application/pdf');
});

test('the strip opens scrolled sideways to the citation, and a redraw keeps the person\'s scroll', async () => {
  Object.defineProperty(HTMLElement.prototype, 'scrollWidth', { configurable: true, get: () => 2000 });
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', { configurable: true, get: () => 500 });
  view = book(3, { focusBlockIds: [20] });
  renderReply = (a) => Promise.resolve(page(a.pageNo === 2 ? {
    rects: [{ x: 0.1, y: 0.1, w: 0.1, h: 0.01, primary: false }, { x: 0.5, y: 0.2, w: 0.1, h: 0.01, primary: true }],
  } : {}));
  try {
    mount({ focus: FOCUS });
    const strip = await screen.findByTestId('page-strip');
    // The primary box: 0.5 x 2000 = 1000, 200 wide, centred in 500 -> 1000 - 150.
    await waitFor(() => expect(strip.scrollLeft).toBe(850));

    strip.scrollLeft = 0;
    document.documentElement.scrollTop = 5;
    const drawn = renderCalls().length;
    await fireEvent.focus(window);
    await waitFor(() => expect(renderCalls().length).toBeGreaterThan(drawn));
    await settle();
    expect(screen.getByTestId('page-strip').scrollLeft).toBe(0);
    expect(document.documentElement.scrollTop).toBe(5);
  } finally {
    delete (HTMLElement.prototype as unknown as Record<string, unknown>).scrollWidth;
    delete (HTMLElement.prototype as unknown as Record<string, unknown>).clientWidth;
  }
});

test('the page takes its height from where the strip starts, under the page line', async () => {
  const real = HTMLElement.prototype.getBoundingClientRect;
  HTMLElement.prototype.getBoundingClientRect = function () {
    const top = this.dataset.testid === 'page-strip' ? 150 : 0;
    return { top, bottom: top, left: 0, right: 0, width: 0, height: 0, x: 0, y: top, toJSON() {} } as DOMRect;
  };
  try {
    paneHeight(856);
    mount();
    await waitFor(() => expect(imageOf(1)).not.toBeNull());
    // 856 - 150 (the strip's top, under the page line) - 12 = 694.
    await waitFor(() => expect(slot(1).style.height).toBe('694px'));
  } finally {
    HTMLElement.prototype.getBoundingClientRect = real;
  }
});

test('a new page height keeps the strip on the page it was on', async () => {
  view = book(10, { focusBlockIds: [60] });
  renderReply = () => Promise.resolve(page({ pages: 10 }));
  mount({ focus: FOCUS });
  await waitFor(() => expect(document.documentElement.scrollTop).toBe(5 * STEP));

  paneHeight(756); // 700 tall: every slot above page 6 is 100 px shorter
  resize!();
  await waitFor(() => expect(document.documentElement.scrollTop).toBe(5 * (700 + 12)));
  await fireEvent.scroll(window);
  expect(screen.getByTestId('page-of').textContent).toBe('page 6 of 10');
});

test('a drawn page\'s sheet has the page\'s own shape, so the boxes land on its picture', async () => {
  renderReply = (a) => Promise.resolve(page(a.pageNo === 1 ? { widthPx: 600, heightPx: 800 } : { widthPx: 900, heightPx: 600 }));
  mount();
  await waitFor(() => expect(imageOf(2)).not.toBeNull());
  const sheet = (n: number) => slot(n).querySelector<HTMLElement>('[data-testid="page-sheet"]')!;
  expect(sheet(1).style.aspectRatio).toBe('600 / 800');
  expect(sheet(2).style.aspectRatio).toBe('900 / 600');
});

test('a whole-document failure clears once the window coming back finds the file again', async () => {
  view = book(3);
  renderReply = () => Promise.resolve({ kind: 'unavailable', reason: 'fileMissing' });
  mount();
  expect((await screen.findByTestId('page-unavailable')).textContent).toBe('The file is missing from disk.');

  renderReply = () => Promise.resolve(page());
  await fireEvent.focus(window);
  await waitFor(() => expect(imageOf(1)).not.toBeNull());
  expect(screen.queryByTestId('page-unavailable')).toBeNull();
});

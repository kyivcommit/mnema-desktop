import { render, screen, waitFor } from '@testing-library/svelte';
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
  ({ kind: 'page', html: HTML, pages: 1, highlight: 'shown', ...over });

let view: DocumentView;
let docReply: () => Promise<unknown>;
const docCalls = () => invoke.mock.calls.filter(([c]) => c === 'render_doc').map(([, a]) => a as Record<string, unknown>);

beforeEach(() => {
  invoke.mockReset();
  scrollWithin.mockReset();
  view = md();
  docReply = () => Promise.resolve(page());
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'open_document') return Promise.resolve(view);
    if (cmd === 'render_doc') return docReply();
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
    { documentId: 'md-1', rootId: 1, relativePath: 'notes/a.md', spans },
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

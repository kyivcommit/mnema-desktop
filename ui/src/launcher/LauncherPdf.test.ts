import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import Launcher from './Launcher.svelte';
import { oneRootTwoFolders } from '../lib/fixtures';
import { setLocale } from '../i18n';

// The right head's Text / Page choice for a PDF (the page itself is
// `PdfPage.test.ts`). The launcher around it is answered minimally.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...a: unknown[]) => a[0] === 'job_status'
    ? Promise.resolve({ revision: 0, files: 0, readSeq: 0, jobsDone: 0, lastReading: null, snapshot: { kind: 'idle' } })
    : invoke(...a),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

const READY = {
  key: { kind: 'present' },
  index: { kind: 'read', embeddedChunks: 0, embeddedChunksEverywhere: 0, embeddingModel: 'emb', chatModel: 'chat', searchTextArm: true, searchContentArm: false },
  roles: { embed: 'openRouter', chat: 'openRouter' },
  configured: { openRouter: true, mnemaEmbed: false, mnemaChat: false },
};
let mime = 'application/pdf';
let pageReply: unknown;
const document_ = () => ({
  kind: 'file', mime,
  blocks: [{ blockId: 1, kind: 'paragraph', text: 'Перший абзац.', pageNo: 1, sectionTitle: null }],
  focusBlockIds: [], spans: [], relativePath: 'notes/a.md', rootName: 'docs', freshness: { kind: 'current' },
});
const PAGE = { kind: 'page', pages: 1, widthPx: 600, heightPx: 800, image: 'data:image/png;base64,AAAA', highlight: 'shown', rects: [] };

// A fresh storage per test: local Node and CI differ in what `localStorage` is.
let store: Map<string, string>;
function stubStorage() {
  store = new Map();
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => { store.set(k, v); },
      removeItem: (k: string) => { store.delete(k); },
    },
  });
}

beforeEach(() => {
  invoke.mockReset();
  mime = 'application/pdf';
  pageReply = PAGE;
  stubStorage();
  invoke.mockImplementation((cmd: string) => {
    switch (cmd) {
      case 'model_settings': return Promise.resolve(READY);
      case 'provider_status': return Promise.resolve({ embed: { kind: 'ok' }, chat: { kind: 'ok' } });
      case 'list_tree': return Promise.resolve(oneRootTwoFolders);
      case 'list_history': return Promise.resolve([]);
      case 'open_document': return Promise.resolve(document_());
      case 'render_pdf_page': return Promise.resolve(pageReply);
      case 'render_doc': return Promise.resolve({ kind: 'page', html: '<p>drawn</p>', pages: 1, highlight: 'shown' });
      default: return Promise.resolve();
    }
  });
});
afterEach(() => { cleanup(); setLocale('en'); });

async function openFile() {
  const folder = await screen.findByTestId('tree-folder-notes');
  // The tree remembers which folders were open in the same storage.
  if (screen.queryByTestId('tree-file-doc-1') === null) await fireEvent.click(folder);
  await fireEvent.click(await screen.findByTestId('tree-file-doc-1'));
  await screen.findByTestId('file-body');
}
const pressed = (v: 'text' | 'page') => screen.getByTestId(`pdf-view-${v}`).getAttribute('aria-pressed');

test('a PDF is drawn as its page by default, with the choice in the right head', async () => {
  render(Launcher);
  await openFile();
  await screen.findByTestId('page-image');
  expect(pressed('page')).toBe('true');
  expect(pressed('text')).toBe('false');
  expect(document.querySelector('.right-head')!.contains(screen.getByTestId('pdf-view'))).toBe(true);
  expect(screen.queryByTestId('file-block')).toBeNull();
});

test('a Markdown file is offered the choice too, and its page is the HTML view', async () => {
  mime = 'text/markdown';
  render(Launcher);
  await openFile();
  await screen.findByTestId('doc-body');
  expect(pressed('page')).toBe('true');
  expect(screen.queryByTestId('file-block')).toBeNull();
  expect(invoke.mock.calls.some(([c]) => c === 'render_pdf_page')).toBe(false);
  await fireEvent.click(screen.getByTestId('pdf-view-text'));
  expect(await screen.findByTestId('file-block')).toBeTruthy();
  expect(screen.queryByTestId('doc-body')).toBeNull();
});

test('the choice is not offered for anything but a PDF or Markdown', async () => {
  mime = 'text/plain';
  render(Launcher);
  await openFile();
  await screen.findByTestId('file-block');
  expect(screen.queryByTestId('pdf-view')).toBeNull();
  expect(invoke.mock.calls.some(([c]) => c === 'render_pdf_page')).toBe(false);
  expect(invoke.mock.calls.some(([c]) => c === 'render_doc')).toBe(false);
});

test('choosing the text shows the text, and the choice survives a remount', async () => {
  const first = render(Launcher);
  await openFile();
  await screen.findByTestId('page-image');
  await fireEvent.click(screen.getByTestId('pdf-view-text'));
  expect(await screen.findByTestId('file-block')).toBeTruthy();
  expect(screen.queryByTestId('page-image')).toBeNull();
  expect(pressed('text')).toBe('true');
  expect(store.get('mnema.pdfView')).toBe('text');
  first.unmount();

  render(Launcher);
  await openFile();
  await screen.findByTestId('file-block');
  expect(pressed('text')).toBe('true');
  expect(screen.queryByTestId('page-image')).toBeNull();
});

test('a stored value that is neither is the default, and a refusing storage does not break it', async () => {
  store.set('mnema.pdfView', 'sideways');
  const first = render(Launcher);
  await openFile();
  await screen.findByTestId('page-image');
  expect(pressed('page')).toBe('true');
  first.unmount();

  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    get: () => { throw new Error('denied'); },
  });
  render(Launcher);
  await openFile();
  await screen.findByTestId('page-image');
  await fireEvent.click(screen.getByTestId('pdf-view-text'));
  expect(await screen.findByTestId('file-block')).toBeTruthy();
});

test('"Show text" on an unavailable page switches the whole pane to the text', async () => {
  pageReply = { kind: 'unavailable', reason: 'failed' };
  render(Launcher);
  await openFile();
  await fireEvent.click(await screen.findByTestId('page-show-text'));
  expect(await screen.findByTestId('file-block')).toBeTruthy();
  expect(pressed('text')).toBe('true');
  expect(store.get('mnema.pdfView')).toBe('text');
});

test('opening another kind of file takes the choice away', async () => {
  render(Launcher);
  await openFile();
  await screen.findByTestId('pdf-view');
  mime = 'text/plain';
  await fireEvent.click(screen.getByTestId('tree-file-doc-1'));
  await waitFor(() => expect(screen.queryByTestId('pdf-view')).toBeNull());
});

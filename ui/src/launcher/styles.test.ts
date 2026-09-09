import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { beforeEach, afterEach, expect, test, vi } from 'vitest';
import Launcher from './Launcher.svelte';
import Tree from './Tree.svelte';
import Source from './Source.svelte';
import { citationA, citationB, generated, oneRootTwoFolders, excerptSpanA, excerptSpanB } from '../lib/fixtures';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/webviewWindow', () => ({ getCurrentWebviewWindow: () => ({ hide: vi.fn() }) }));
let sheets: HTMLStyleElement[] = [];
const HERE = dirname(fileURLToPath(import.meta.url));

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation((cmd: string, args?: { chunkId: number }) => {
    if (cmd === 'model_settings') return Promise.resolve({
      key: { kind: 'absent' }, index: { kind: 'read', embeddedChunks: 0,
        embeddedChunksEverywhere: 0, embeddingModel: null, searchTextArm: true, searchContentArm: false },
    });
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
    if (cmd === 'ask') return Promise.resolve(generated);
    if (cmd === 'source_around' && args?.chunkId === 42) return Promise.resolve(excerptSpanA);
    if (cmd === 'source_around' && args?.chunkId === 43) return Promise.resolve(excerptSpanB);
    throw new Error(`unexpected command ${cmd}`);
  });
  for (const path of ['base.css', 'launcher.css']) {
    const style = document.createElement('style');
    style.textContent = readFileSync(join(HERE, '../styles', path), 'utf8');
    document.head.append(style);
    sheets.push(style);
  }
});
afterEach(() => { cleanup(); sheets.forEach((s) => s.remove()); sheets = []; });

test('transparent launcher disables the native window shadow', () => {
  const conf = JSON.parse(readFileSync(join(HERE, '../../../src-tauri/tauri.conf.json'), 'utf8')) as {
    app: { windows: Array<{ label: string; transparent?: boolean; shadow?: boolean }> };
  };
  const launcher = conf.app.windows.find((window) => window.label === 'launcher');
  expect(launcher).toMatchObject({ transparent: true, shadow: false });
});

test('real launcher places all cards and keeps the document transparent', async () => {
  const { container } = render(Launcher);
  const main = container.querySelector('main')!;
  expect(getComputedStyle(main).display).toBe('grid');
  expect(getComputedStyle(document.body).backgroundColor).toBe('rgba(0, 0, 0, 0)');
  const bar = container.querySelector('.searchbar')!;
  expect(bar).not.toBeNull();
  expect(bar.contains(screen.getByRole('textbox'))).toBe(true);
  expect(bar.contains(screen.getByTestId('pin'))).toBe(true);
  expect(bar.querySelectorAll('input[type="checkbox"]')).toHaveLength(2);
  expect(screen.queryByTestId('card-tree')).toBeNull();
  const box = screen.getByRole('textbox');
  await fireEvent.input(box, { target: { value: 'question' } });
  await fireEvent.keyDown(box, { key: 'Enter' });
  await screen.findByTestId('card-centre');
  expect(screen.queryByTestId('card-source')).toBeNull();
  await fireEvent.click(screen.getByTestId('preview-3'));
  await waitFor(() => expect(screen.getByTestId('source-body').getAttribute('data-pending')).toBe('0'));
  expect(screen.getAllByTestId('source-block').length).toBeGreaterThan(0);
  for (const [id, column, row] of [
    ['card-tree', '1', '1 / 3'], ['card-centre', '2', '2'], ['card-source', '3', '1 / 3'],
  ]) {
    const el = screen.getByTestId(id);
    expect(el.parentElement).toBe(main);
    const style = getComputedStyle(el);
    expect(style.gridColumn).toBe(column);
    expect(style.gridRow).toBe(row);
    expect(style.overflow).toBe('auto');
  }
  expect(getComputedStyle(bar).gridColumn).toBe('2');
  expect(getComputedStyle(bar).gridRow).toBe('1');
});

test('real tree marks current file and recent while leaving neighbours plain', async () => {
  render(Tree, { selected: citationA });
  const selected = await screen.findByTestId('tree-file-doc-1');
  const other = screen.getByTestId('tree-file-doc-2');
  function check(on: Element, off: Element) {
    expect(on.getAttribute('aria-current')).toBe('true');
    expect(off.hasAttribute('aria-current')).toBe(false);
    expect(getComputedStyle(on).background).toBe('var(--cite-wash)');
    expect(getComputedStyle(on).borderColor).toBe('var(--cite-line)');
    expect(getComputedStyle(off).background).not.toBe('var(--cite-wash)');
    expect(getComputedStyle(off).borderColor).not.toBe('var(--cite-line)');
  }
  check(selected, other);
  await fireEvent.click(screen.getByTestId('tree-tab-recents'));
  check(screen.getByTestId('tree-recent-doc-1'), screen.getByTestId('tree-recent-doc-3'));
  const on = getComputedStyle(screen.getByTestId('tree-tab-recents'));
  const off = getComputedStyle(screen.getByTestId('tree-tab-files'));
  expect(on.background).toBe('var(--surface)');
  expect(off.background).not.toBe(on.background);
});

test('real source distinguishes primary highlight from its sibling', async () => {
  render(Source, { selected: citationA, siblings: [citationA, citationB] });
  await waitFor(() => expect(screen.getByTestId('source-body').getAttribute('data-pending')).toBe('0'));
  const marks = screen.getAllByTestId('hl');
  const primary = marks.filter((m) => m.getAttribute('data-primary') === 'true');
  const secondary = marks.filter((m) => !m.hasAttribute('data-primary'));
  expect(primary).toHaveLength(1);
  expect(secondary).toHaveLength(1);
  for (const mark of marks) expect(getComputedStyle(mark).background).toBe('var(--cite-wash)');
  expect(getComputedStyle(primary[0]).outline).toBe('1px solid var(--cite-line)');
  expect(getComputedStyle(secondary[0]).outline).toBe('none');
  expect(screen.getByTestId('freshness').textContent?.length).toBeGreaterThan(0);
});

test('real pin exposes pressed state through its style', async () => {
  render(Launcher);
  const pin = screen.getByTestId('pin');
  const before = getComputedStyle(pin).background;
  await fireEvent.click(pin);
  expect(pin.getAttribute('aria-pressed')).toBe('true');
  expect(getComputedStyle(pin).background).toBe('var(--accent-soft)');
  expect(getComputedStyle(pin).background).not.toBe(before);
});

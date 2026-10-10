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
vi.mock('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }));
let sheets: HTMLStyleElement[] = [];
const HERE = dirname(fileURLToPath(import.meta.url));

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation((cmd: string, args?: { chunkId: number }) => {
    if (cmd === 'model_settings') return Promise.resolve({
      key: { kind: 'absent' }, index: { kind: 'read', embeddedChunks: 0,
        embeddedChunksEverywhere: 0, embeddingModel: null, searchTextArm: true, searchContentArm: false },
    });
    if (cmd === 'provider_status') return Promise.resolve({ kind: 'ok' });
    if (cmd === 'provider_choice') return Promise.resolve('openRouter');
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
    if (cmd === 'ask') return Promise.resolve(generated);
    if (cmd === 'set_launcher_layout' || cmd === 'launcher_answered') return Promise.resolve();
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
  // The first card is preselected, so the source card is up before any click.
  expect(screen.getByTestId('card-source')).toBeTruthy();
  await fireEvent.click(screen.getByTestId('preview-3'));
  await waitFor(() => expect(screen.getByTestId('source-body').getAttribute('data-pending')).toBe('0'));
  expect(screen.getAllByTestId('source-block').length).toBeGreaterThan(0);
  const centre = bar.parentElement!;
  expect(centre.className).toBe('col-centre');
  expect(centre.parentElement).toBe(main);
  expect(screen.getByTestId('card-centre').parentElement).toBe(centre);
  for (const [id, column] of [['card-tree', '1'], ['card-source', '3']]) {
    const el = screen.getByTestId(id);
    expect(el.parentElement).toBe(main);
    const style = getComputedStyle(el);
    expect(style.gridColumn).toBe(column);
    expect(style.overflow).toBe('auto');
  }
  expect(getComputedStyle(centre).gridColumn).toBe('2');
  expect(getComputedStyle(centre).display).toBe('flex');
  expect(getComputedStyle(screen.getByTestId('card-centre')).overflow).toBe('auto');
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

test('the left switch hides the tree by the UA [hidden] rule, and the stylesheet does not override it', async () => {
  render(Launcher);
  const box = screen.getByRole('textbox');
  await fireEvent.input(box, { target: { value: 'question' } });
  await fireEvent.keyDown(box, { key: 'Enter' });
  const tree = await screen.findByTestId('card-tree');
  expect(getComputedStyle(tree).display).not.toBe('none');
  tree.setAttribute('hidden', '');
  expect(getComputedStyle(tree).display).toBe('none');
});

// Placement in every layout, not only the widest: a wrong rule would put a
// panel into an implicit track with the other three layouts untested.
test('each data-cols layout places the tree, search, results and source in its own tracks', async () => {
  const { container } = render(Launcher);
  const main = container.querySelector('main')!;
  const box = screen.getByRole('textbox');
  await fireEvent.input(box, { target: { value: 'question' } });
  await fireEvent.keyDown(box, { key: 'Enter' });
  await screen.findByTestId('card-source');
  const col = (el: Element) => getComputedStyle(el).gridColumn;
  const bar = container.querySelector('.col-centre')!;
  const expected: Record<string, { bar: string; results: string; tree?: string; source?: string }> = {
    s: { bar: '1', results: '1' },
    ls: { bar: '2', results: '2', tree: '1' },
    sr: { bar: '1', results: '1', source: '2' },
    lsr: { bar: '2', results: '2', tree: '1', source: '3' },
  };
  for (const [cols, want] of Object.entries(expected)) {
    main.setAttribute('data-cols', cols);
    expect(col(bar), `${cols} centre column`).toBe(want.bar);
    expect(screen.getByTestId('card-centre').parentElement, `${cols} results`).toBe(bar);
    if (want.tree) expect(col(screen.getByTestId('card-tree')), `${cols} tree`).toBe(want.tree);
    if (want.source) expect(col(screen.getByTestId('card-source')), `${cols} source`).toBe(want.source);
  }
});

test('nothing spans two grid rows, so no card can size the search panel (WebKit spanning-item distribution)', () => {
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(/main\.panels\s*\{[^}]*grid-template-rows:\s*minmax\(0, 1fr\);/);
  expect(css).not.toMatch(/(^|\n)main[^{]*\{[^}]*grid-row/); // the card-internal grids below may use rows
  expect(css).toMatch(/\.col-centre\s*\{[^}]*display:\s*flex;[^}]*flex-direction:\s*column;[^}]*gap:\s*5px/);
  expect(css).toMatch(/\.col-centre > \.results\s*\{[^}]*flex:\s*1 1 auto/);
  expect(css).toMatch(/\.searchbar\s*\{[^}]*flex:\s*none/);
});

test('the cloud is coloured by what it reports, and an inactive one has no hover background', () => {
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(/\.cloud\[data-status="unreachable"\]\s*\{[^}]*color:\s*var\(--err\)/);
  expect(css).toMatch(/\.cloud\[data-status="notConfigured"\][^{]*\{[^}]*stroke-dasharray/);
  expect(css).toMatch(/button:hover[^{]*:not\(\[aria-disabled="true"\]\)[^{]*\{/);
});

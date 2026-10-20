import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { beforeEach, afterEach, expect, test, vi } from 'vitest';
import Launcher from './Launcher.svelte';
import Tree from './Tree.svelte';
import Source from './Source.svelte';
import {
  citationA, citationB, generated, oneRootTwoFolders, excerptSpanA, excerptSpanB, historyTwoQuestions,
} from '../lib/fixtures';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
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
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
    if (cmd === 'list_history') return Promise.resolve(historyTwoQuestions);
    if (cmd === 'ask') return Promise.resolve(generated);
    if (cmd === 'source_around' && args?.chunkId === 42) return Promise.resolve(excerptSpanA);
    if (cmd === 'source_around' && args?.chunkId === 43) return Promise.resolve(excerptSpanB);
    if (cmd === 'chat_tools_enabled') return Promise.resolve(false);
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

test('the launcher is a normal window: decorated, resizable, and not a popup', () => {
  const conf = JSON.parse(readFileSync(join(HERE, '../../../src-tauri/tauri.conf.json'), 'utf8')) as {
    app: { windows: Array<Record<string, unknown> & { label: string }> };
  };
  const launcher = conf.app.windows.find((window) => window.label === 'launcher')!;
  expect(launcher).toMatchObject({
    decorations: true, resizable: true,
    width: 1440, height: 900, minWidth: 900, minHeight: 600,
    // macOS draws the traffic lights over the content; elsewhere the keys are
    // ignored and the normal title bar stays.
    titleBarStyle: 'Overlay', hiddenTitle: true,
  });
  for (const popup of ['transparent', 'shadow', 'alwaysOnTop', 'skipTaskbar']) {
    expect(launcher, popup).not.toHaveProperty(popup);
  }
});

test('real launcher places the three areas with the tree inside the right one', async () => {
  const { container } = render(Launcher);
  const main = container.querySelector('main')!;
  expect(getComputedStyle(main).display).toBe('grid');

  // The composer and the toolbar share the transcript's column; the tree is a
  // pane inside the right area, not a column of its own (§2).
  const centre = screen.getByTestId('col-transcript');
  expect(centre.contains(screen.getByRole('textbox'))).toBe(true);
  expect(centre.contains(screen.getByTestId('settings'))).toBe(true);
  const tree = screen.getByTestId('col-tree');
  expect(tree.contains(screen.getByTestId('tree-body'))).toBe(true);
  expect(screen.getByTestId('col-right').contains(tree)).toBe(true);
  // The pane itself does not scroll — its splitter lives on its edge and would
  // scroll with the content — the body inside it does.
  expect(getComputedStyle(tree).overflow).toBe('');
  expect(getComputedStyle(tree.querySelector('.pane-body')!).overflow).toBe('auto');

  const box = screen.getByRole('textbox');
  await fireEvent.input(box, { target: { value: 'question' } });
  await fireEvent.keyDown(box, { key: 'Enter' });
  await screen.findByTestId('turn');
  expect(screen.getAllByTestId('turn')).toHaveLength(1);
});

test('real tree marks current file and recent while leaving neighbours plain', async () => {
  render(Tree, { selected: citationA, historyId: 1 });
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
  check(await screen.findByTestId('tree-history-1'), screen.getByTestId('tree-history-2'));
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

test('the cloud is coloured by what it reports, and an inactive one has no hover background', () => {
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(/\.cloud\[data-status="unreachable"\]\s*\{[^}]*color:\s*var\(--err\)/);
  expect(css).toMatch(/\.cloud\[data-status="notConfigured"\][^{]*\{[^}]*stroke-dasharray/);
  expect(css).toMatch(/button:hover[^{]*:not\(\[aria-disabled="true"\]\)[^{]*\{/);
});

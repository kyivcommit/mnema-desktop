import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { render, screen, fireEvent, cleanup, within } from '@testing-library/svelte';
import { beforeEach, afterEach, expect, onTestFinished, test, vi } from 'vitest';
import Launcher from './Launcher.svelte';
import Shell from './Shell.svelte';
import Tree from './Tree.svelte';
import Recents from './Recents.svelte';
import FilePane from './FilePane.svelte';
import {
  generated, oneRootTwoFolders, historyTwoQuestions,
} from '../lib/fixtures';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }));
let sheets: HTMLStyleElement[] = [];
const HERE = dirname(fileURLToPath(import.meta.url));

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'model_settings') return Promise.resolve({
      key: { kind: 'absent' }, index: { kind: 'read', embeddedChunks: 0,
        embeddedChunksEverywhere: 0, embeddingModel: null, searchTextArm: true, searchContentArm: false },
    });
    if (cmd === 'provider_status') return Promise.resolve({ kind: 'ok' });
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
    if (cmd === 'list_history') return Promise.resolve(historyTwoQuestions);
    if (cmd === 'ask') return Promise.resolve({ answer: generated, historyId: null });
    if (cmd === 'open_document') return Promise.resolve({
      kind: 'file',
      blocks: [{ blockId: 1, kind: 'paragraph', text: 'A cited passage. And more.', pageNo: 1, sectionTitle: null }],
      focusBlockIds: [1],
      spans: [{ blockId: 1, blockStart: 2, length: 6, primary: true }],
      relativePath: 'notes/a.md',
      rootName: 'docs',
      // `fileChanged`, not `current`: the badge speaks only when it carries
      // news (owner, 2026-10-20), and the pin below needs one on screen.
      freshness: { kind: 'fileChanged' },
    });
    if (cmd === 'chat_tools_enabled') return Promise.resolve(false);
    if (cmd === 'job_status') return Promise.resolve({ revision: 0, files: 0, readSeq: 0, jobsDone: 0, lastReading: null, snapshot: { kind: 'idle' } });
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
  expect(centre.contains(screen.getByTestId('new-chat'))).toBe(true);
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

// Both panels' title rows are one rule at one height, so the tree's search field
// lands level with the conversations' (measured: 38-68 title, 68-102 field).
test('real CSS: both panel titles sit in one 30px .pane-head', async () => {
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  render(Tree, { selected: null, onPick: () => {} });
  const heads = [await screen.findByTestId('recents-title'), await screen.findByTestId('tree-title')]
    .map((title) => title.parentElement!);
  for (const head of heads) {
    expect(head.classList.contains('pane-head')).toBe(true);
    expect(getComputedStyle(head).height).toBe('30px');
  }
  expect(heads[0]).not.toBe(heads[1]);
  // Read from the sheet: base.css already sets border-box on everything, so a
  // computed value cannot tell whether this rule keeps its own.
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(/\.pane-head\s*\{[^}]*height:\s*30px;\s*box-sizing:\s*border-box/);
});

// The find-in-files field never scrolls, in either direction: the nearest
// scrolling ancestor of the tree must not contain it (`.tree` is as wide as its
// deepest row, so a field inside that scroller would slide out sideways).
test('real tree: the search field sits outside the element that scrolls the tree', async () => {
  render(Launcher);
  const body = await screen.findByTestId('tree-body');
  const scrolls = (el: Element) => /auto|scroll/.test(getComputedStyle(el).overflow);
  let scroller: Element | null = body.parentElement;
  while (scroller && !scrolls(scroller)) scroller = scroller.parentElement;
  expect(scroller).not.toBeNull();
  expect(scroller!.contains(body)).toBe(true);
  expect(scroller!.contains(screen.getByRole('searchbox', { name: 'Find words in files' }))).toBe(false);
});

// The open file's path lives in the right panel's head, which never scrolls:
// the nearest scrolling ancestor of the path must not be the one that holds the
// file's text, or the path would slide away with it.
test('real file path sits outside the element that scrolls the file', async () => {
  render(Launcher);
  await fireEvent.click(await screen.findByTestId('tree-folder-notes'));
  await fireEvent.click(screen.getByTestId('tree-file-doc-1'));
  const block = await screen.findByTestId('file-block');
  const header = await screen.findByTestId('file-header');
  expect(header.textContent).toBe('docs/notes/a.md');
  const scrolls = (el: Element) => /auto|scroll/.test(getComputedStyle(el).overflow);
  let scroller: Element | null = block.parentElement;
  while (scroller && !scrolls(scroller)) scroller = scroller.parentElement;
  expect(scroller).not.toBeNull();
  expect(scroller!.contains(block)).toBe(true);
  expect(scroller!.contains(header)).toBe(false);
});

test('real tree marks the current file while leaving neighbours plain', async () => {
  render(Tree, { selected: { rootId: 1, relativePath: 'notes/a.md' }, onPick: () => {} });
  const selected = await screen.findByTestId('tree-file-doc-1');
  const other = screen.getByTestId('tree-file-doc-2');
  expect(selected.getAttribute('aria-current')).toBe('true');
  expect(other.hasAttribute('aria-current')).toBe(false);
  // The same selection as the conversation on screen (owner, 2026-10-22):
  // `--select` with its own ink, and no outline the history row lacks.
  expect(getComputedStyle(selected).background).toBe('var(--select)');
  expect(getComputedStyle(selected).borderColor).toBe('transparent');
  expect(getComputedStyle(other).background).not.toBe('var(--select)');
  // A file reads as secondary to a folder: same weight, `--ink-soft` — while
  // the open row above is lifted back to `--ink`.
  expect(getComputedStyle(other).color).toBe('var(--ink-soft)');
  expect(getComputedStyle(other).fontWeight).toBe('400');
  expect(getComputedStyle(screen.getByTestId('tree-folder-archive')).color).toBe('var(--ink)');
  expect(getComputedStyle(screen.getByTestId('tree-folder-archive')).fontWeight).toBe('500');
  expect(getComputedStyle(selected).color).toBe('var(--on-select)');
  // jsdom ranks specificity by source order, so the computed colours above
  // cannot prove the open row outranks the file row. The selector is read from
  // the sheet instead: the file-row rule must exclude the current row, or a
  // later simplification silently paints the open file as secondary. The lift
  // itself is a stand-only check.
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(/\.tree li > button\.file-row:not\(\[aria-current="true"\]\)\s*\{/);
});

// §4: the conversation on screen is `--select` — the same fill as the open
// file in the tree (owner, 2026-10-22).
test('real recents marks the conversation on screen with --select', async () => {
  render(Recents, { activeId: 1, gen: 0, onOpen: () => {} });
  const on = await screen.findByTestId('recents-row-1');
  const off = screen.getByTestId('recents-row-2');
  expect(getComputedStyle(on).background).toBe('var(--select)');
  expect(getComputedStyle(off).background).not.toBe('var(--select)');
});

// The card is a column with one gap between its parts, and `Answer` relies on
// it: the paragraph and the citations carry `margin: 0`. Without this rule
// the paragraph and the citation list sit on each other. Pinned by reading the sheet, as the cloud
// colours below are: no jsdom render computes the gap (review, Important 1).
test('the answer card keeps the column gap its parts depend on', () => {
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(
    /\.answer-card,\s*\.passages-card\s*\{[^}]*display:\s*flex;[^}]*flex-direction:\s*column;[^}]*gap:\s*13px/,
  );
});

// §4 (owner's mockup, 2026-10-21): two lines — the question, then a clipped
// preview of the answer — with the × in its own column to the right. The
// relative time each row used to carry is gone: the group heading says when.
test('a conversation row is two lines: question, preview, then ×', async () => {
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  const row = await screen.findByTestId('recents-row-1');
  const open = within(row).getByTestId('recents-open-1');
  const query = within(row).getByTestId('recents-query');
  const preview = within(row).getByTestId('recents-preview');
  expect(query.parentElement).toBe(open); // both lines inside the row's one button
  expect(preview.parentElement).toBe(open);
  expect(getComputedStyle(open).display).toBe('flex');
  expect(getComputedStyle(open).flexDirection).toBe('column');
  expect(getComputedStyle(query).textOverflow).toBe('ellipsis'); // each line clips, never wraps
  expect(getComputedStyle(preview).textOverflow).toBe('ellipsis');
  expect(getComputedStyle(preview).color).toBe('var(--ink-soft)'); // an echo of the question
  const remove = within(row).getByTestId('recents-remove-1');
  expect(getComputedStyle(remove).flexGrow).toBe('0'); // the × keeps its own column
  // The selected row lifts both lines to the selection's own ink — read from the
  // sheet, as jsdom hands `inherit` back unresolved.
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(/\.history-row\[aria-current="true"\] :is\(\.history-query, \.history-preview\)\s*\{[^}]*color:\s*inherit/);
  expect(css).toMatch(/\.recents-group h3\s*\{[^}]*text-transform:\s*uppercase/);
});

// The header is pinned: it sits outside the body that scrolls, so the panel's
// name and its compose button stay put above fifty rows.
test('the conversations header is pinned above the scrolling body', () => {
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(/\.pane-head\s*\{[^}]*flex:\s*none/);
  expect(css).toMatch(/\.recents-body\s*\{[^}]*overflow:\s*auto/);
});

// The open-failure alert sits beside the list and carried only `.composer`'s
// rule before the fix round, so it drew as bare text (review, minor). The frame
// is read from the sheet as well as the render: jsdom drops a `background` and
// a `border` written as a `var(…)` shorthand, so the computed colour cannot
// answer for it.
test('the conversation alert is the same framed message as the composer guard', async () => {
  render(Recents, { activeId: null, gen: 0, onOpen: () => {}, error: 'historyFailed' });
  const alert = await screen.findByTestId('recents-error');
  expect(alert.getAttribute('role')).toBe('alert');
  const computed = getComputedStyle(alert);
  expect(computed.fontSize).toBe('12px'); // the rule reaches this element…
  expect(computed.margin).toBe('0px 0px 8px 0px');
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(
    /\.recents-body \.guard\s*\{[^}]*background:\s*var\(--surface-2\);[^}]*border:\s*1px solid var\(--line\)/,
  );
});

test('real file pane paints the cited span and keeps its badge on screen', async () => {
  render(FilePane, {
    location: { documentId: 'doc-1', rootId: 1, relativePath: 'notes/a.md' },
    focus: null,
    onFocusHandled: () => {},
  });
  const mark = await screen.findByTestId('hl');
  expect(mark.getAttribute('data-primary')).toBe('true');
  expect(getComputedStyle(mark).background).toBe('var(--cite-wash)');
  expect(getComputedStyle(mark).outline).toBe('1px solid var(--cite-line)');
  expect(screen.getByTestId('file-badge').textContent?.length).toBeGreaterThan(0);
});

// The carried finding from the shell: with the tree hidden its track is 0 px,
// and a file pane placed by auto-flow lands in that track and is invisible.
// Both panes name their column, so the file stays in the second one in both
// modes — asserted after the tree is actually hidden, not just with the class
// rule in the sheet.
test('the file pane keeps the second track when the tree is hidden', async () => {
  render(Shell, { fileOpen: true });
  expect(getComputedStyle(screen.getByTestId('col-tree')).gridColumn).toBe('1');
  expect(getComputedStyle(screen.getByTestId('col-file')).gridColumn).toBe('2');

  await fireEvent.click(screen.getByTestId('toggle-tree'));
  expect(screen.getByTestId('col-tree').hasAttribute('hidden')).toBe(true);
  expect(getComputedStyle(screen.getByTestId('col-file')).gridColumn).toBe('2');
});

// Owner, 2026-10-21: the composer carries the same recessed fill as the
// conversations search field. On `--surface` it read as part of the page.
test('the composer sits on the search field fill, not on the page surface', () => {
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(/\.composer\s*\{[^}]*background:\s*var\(--surface-2\)/);
});

// Owner, 2026-10-20: one focus ring on the composer — the box's
// `:focus-within` outline — and never a second, UA-drawn one on the field
// inside it. Read from the sheet: jsdom's UA stylesheet draws no ring, so a
// computed style cannot see the doubling.
test('the composer draws one focus ring: the box, never the field inside it', () => {
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(/\.composer:focus-within\s*\{\s*outline:\s*2px solid var\(--accent\)/);
  expect(css).toMatch(/\.composer textarea\s*\{[^}]*outline:\s*none/);
});

// §2 (owner, 2026-10-20): the right area is a column like the others — the
// overlay painted over the transcript and the composer, and typing into
// invisible text is not a usable transcript. Nothing in the sheet positions it
// over the centre; a hidden pane leaves the grid instead.
test('the right area is a column, never an overlay', () => {
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(/\.shell > \.right\s*\{[^}]*position:\s*relative/);
  expect(css).not.toMatch(/\.overlay/);
  expect(css).not.toMatch(/\.scrim/);
  expect(css).not.toMatch(/--glow/);
});

test('the cloud is coloured by what it reports, and an inactive one has no hover background', () => {
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(/\.cloud\[data-status="unreachable"\]\s*\{[^}]*color:\s*var\(--err\)/);
  expect(css).toMatch(/\.cloud\[data-status="notConfigured"\][^{]*\{[^}]*stroke-dasharray/);
  expect(css).toMatch(/button:hover[^{]*:not\(\[aria-disabled="true"\]\)[^{]*\{/);
});

// Owner, 2026-10-21: the «×» must cost no width until the row is hovered or
// focused — an always-present 22px clipped every question and preview. Read
// from the sheet: jsdom computes no hover, and the failure this guards is a
// future edit moving the width back into the default rule.
test('the history remove button takes no width until its row is hovered or focused', () => {
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  expect(css).toMatch(/\.history-remove\s*\{\s*[^}]*width:\s*0\b/);
  expect(css).toMatch(/\.history-row:hover \.history-remove[^{]*\{[^}]*width:\s*22px/);
  expect(css).toMatch(/\.history-row:focus-within \.history-remove[^{]*\{[^}]*width:\s*22px/);
});

// Owner, 2026-10-22: the sidebar and the header carry the same pencil; only one
// is on screen. Both directions: the header's shows once the sidebar is shut.
test('real CSS: the header pencil hides while the sidebar is open', async () => {
  // jsdom's 1024 is too narrow for the sidebar beside the transcript; a wide
  // window shows all three areas (`Launcher.test.ts` lays out in 1440 too).
  const width = Object.getOwnPropertyDescriptor(window, 'innerWidth');
  Object.defineProperty(window, 'innerWidth', { value: 1440, configurable: true });
  onTestFinished(() => { if (width) Object.defineProperty(window, 'innerWidth', width); });
  render(Launcher);
  const pencil = screen.getByTestId('new-chat');
  expect(screen.getByTestId('recents-new')).toBeTruthy();
  expect(getComputedStyle(pencil).display).toBe('none');
  await fireEvent.click(screen.getByTestId('toggle-left'));
  expect(screen.getByTestId('shell').dataset.left).toBe('false');
  expect(getComputedStyle(pencil).display).not.toBe('none');
});

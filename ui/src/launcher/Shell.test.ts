import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { render, screen, fireEvent, cleanup } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { afterEach, beforeEach, expect, test } from 'vitest';
import Shell from './Shell.svelte';
import { DEFAULTS, MIN } from './layout';

const HERE = dirname(fileURLToPath(import.meta.url));

// jsdom under vitest hands out a `localStorage` with no working methods (an
// opaque origin), so this stubs one in memory — the layout has to survive a
// remount, and that is the whole of one test below.
let storage: { getItem: (k: string) => string | null; setItem: (k: string, v: string) => void };
function memoryStorage() {
  const map = new Map<string, string>();
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => { map.set(k, v); },
    removeItem: (k: string) => { map.delete(k); },
  };
}

// `defineProperty`, not `stubGlobal`: Node 26's own `localStorage` getter
// warns when touched, and this replaces it without reading it.
function stubStorage() {
  storage = memoryStorage();
  Object.defineProperty(globalThis, 'localStorage', { value: storage, configurable: true });
}

// jsdom has no layout; the shell reads the window width once, at mount, as a
// real window does when it is raised. The default window is 1440 wide (§1).
const WINDOW = 1440;
function setWindowWidth(width: number) {
  Object.defineProperty(window, 'innerWidth', { value: width, configurable: true });
}

beforeEach(() => {
  stubStorage();
  setWindowWidth(WINDOW);
});
afterEach(cleanup);

// The tracks are inline styles on the shell, so this reads what the browser
// would lay out rather than a class name.
const tracks = () => (screen.getByTestId('shell') as HTMLElement).getAttribute('style') ?? '';
const rightTracks = () => document.querySelector('.right-body')!.getAttribute('style') ?? '';
const toggle = (side: 'left' | 'right' | 'tree') => screen.getByTestId(`toggle-${side}`) as HTMLButtonElement;
const stored = () => JSON.parse(storage.getItem('mnema.window')!) as typeof DEFAULTS;

test('a toggle hands its width to the neighbour', async () => {
  render(Shell);
  // The recents pane is its own track; the transcript is the 1fr between the
  // two and never gets a number of its own.
  expect(tracks()).toContain(`grid-template-columns: ${DEFAULTS.L}px minmax(0, 1fr) ${DEFAULTS.W}px`);
  expect(toggle('left').getAttribute('aria-pressed')).toBe('true');

  await fireEvent.click(toggle('left'));
  expect(toggle('left').getAttribute('aria-pressed')).toBe('false');
  expect(screen.queryByTestId('col-recents')).toBeNull();
  // The width left the grid, so the 1fr takes it; the right area is untouched
  // and the window did not change size.
  expect(tracks()).toContain(`grid-template-columns: 0px minmax(0, 1fr) ${DEFAULTS.W}px`);
  // With the sidebar shut this header is under the traffic lights, so it keeps
  // clear of them; with the sidebar open it carries no inset.
  expect(screen.getByTestId('shell').getAttribute('data-left')).toBe('false');
  await fireEvent.click(toggle('left'));
  expect(screen.getByTestId('shell').getAttribute('data-left')).toBe('true');

  // A splitter has a name, a vertical orientation, a declared range and arrow
  // keys (±8px), and Enter returns its pane to the default width.
  const [recents] = screen.getAllByRole('separator');
  expect(recents.getAttribute('aria-orientation')).toBe('vertical');
  expect(recents.getAttribute('aria-label')).toBeTruthy();
  expect(recents.getAttribute('aria-valuemin')).toBe(String(MIN.L));
  expect(recents.getAttribute('aria-valuemax')).toBe(String(WINDOW - MIN.TRANSCRIPT - DEFAULTS.W));
  await fireEvent.keyDown(recents, { key: 'ArrowRight' });
  expect(tracks()).toContain(`grid-template-columns: ${DEFAULTS.L + 8}px `);
  await fireEvent.keyDown(recents, { key: 'Enter' });
  expect(tracks()).toContain(`grid-template-columns: ${DEFAULTS.L}px `);
});

test('the tree toggle does not change the transcript width', async () => {
  render(Shell, { fileOpen: true });
  // A file is open, so the tree is its own T and the file gets the rest of W.
  expect(rightTracks()).toContain(`grid-template-columns: ${DEFAULTS.T}px`);
  const pane = screen.getByTestId('col-tree');
  await fireEvent.click(toggle('tree'));
  expect(toggle('tree').getAttribute('aria-pressed')).toBe('false');
  // Hidden rather than unmounted: the tree keeps its fetch, its folders and its
  // tab, and the file pane takes the freed width.
  expect(screen.getByTestId('col-tree')).toBe(pane);
  expect(pane.hasAttribute('hidden')).toBe(true);
  expect(rightTracks()).toContain('grid-template-columns: 0px');
  expect(tracks()).toContain(`${DEFAULTS.L}px minmax(0, 1fr) ${DEFAULTS.W}px`);
});

// Owner, 2026-10-20: with no file open the tree is constrained as if the file
// were there — its own T, the file stripe reserved empty — so the list never
// sprawls across the whole W.
test('with no file open the tree keeps its own width and the file stripe is reserved', () => {
  render(Shell);
  expect(rightTracks()).toContain(`grid-template-columns: ${DEFAULTS.T}px minmax(0, 1fr)`);
  expect(rightTracks()).not.toContain(`${DEFAULTS.W}px`);
  // recents | transcript, transcript | right, tree | reserved file stripe
  expect(screen.getAllByRole('separator')).toHaveLength(3);
});

test('the files toggle hides the right area without unmounting the tree', async () => {
  render(Shell);
  const area = screen.getByTestId('col-right');
  await fireEvent.click(toggle('right'));
  expect(screen.getByTestId('col-right')).toBe(area);
  expect(area.hasAttribute('hidden')).toBe(true);
  expect(screen.getByTestId('col-tree')).toBeTruthy();
  await fireEvent.click(toggle('right'));
  expect(area.hasAttribute('hidden')).toBe(false);
});

test('the layout survives a remount', async () => {
  const first = render(Shell, { fileOpen: true });
  await fireEvent.click(toggle('left'));
  await fireEvent.click(toggle('tree'));
  expect(tracks()).toContain('grid-template-columns: 0px');
  first.unmount();

  render(Shell, { fileOpen: true });
  expect(tracks()).toContain('grid-template-columns: 0px');
  expect(toggle('tree').getAttribute('aria-pressed')).toBe('false');
});

// Owner, 2026-10-20: the «tree is hidden» placeholder is gone. The way back is
// the tree toggle in the pane's own header, which is always rendered — file or
// no file, tree open or shut.
test('hiding the tree with no file open leaves the pane header toggle as the way back', async () => {
  render(Shell);
  await fireEvent.click(toggle('tree'));

  expect(toggle('tree').getAttribute('aria-pressed')).toBe('false');
  expect(screen.getByTestId('col-tree').hasAttribute('hidden')).toBe(true);
  expect(screen.queryByTestId('tree-hidden')).toBeNull();
  expect(screen.queryByTestId('col-file')).toBeNull();

  await fireEvent.click(toggle('tree'));
  expect(screen.getByTestId('col-tree').hasAttribute('hidden')).toBe(false);
  expect(toggle('tree').getAttribute('aria-pressed')).toBe('true');
});

// The transition that made the old force-on rule wrong: hide the tree, open a
// file, and the tree must not reappear under the person's click (§3 — a tree
// that was open stays open, and one that was shut stays shut).
test('a file opened while the tree is hidden does not bring the tree back', async () => {
  const { rerender } = render(Shell);
  await fireEvent.click(toggle('tree'));
  expect(toggle('tree').getAttribute('aria-pressed')).toBe('false');

  // `panels` is passed through the rerender because this harness has no parent
  // binding one: without it a prop update re-applies the component's captured
  // default and would report the toggle as never having happened. The state the
  // test carries over is the one the person chose — the tree shut.
  await rerender({ fileOpen: true, panels: { ...DEFAULTS, tree: false } });

  expect(screen.getByTestId('col-tree').hasAttribute('hidden')).toBe(true);
  expect(screen.getByTestId('col-file')).toBeTruthy();
  expect(screen.queryByTestId('tree-hidden')).toBeNull();
});

// Owner, 2026-10-20: the overlay is not modal — no scrim blocks the centre, so
// the transcript keeps typing and scrolling beside the panel, and the person
// keeps their cursor. The ways out: Esc, and the «Файли» toggle the panel no
// longer covers (it starts below the header — `launcher.css`'s `top: 38px`).
test('at 900px the right area floats over a transcript that stays usable; Esc and «Файли» close it', async () => {
  setWindowWidth(900);
  const probe = createRawSnippet(() => ({ render: () => '<textarea data-testid="probe"></textarea>' }));
  render(Shell, { transcript: probe });
  const shell = screen.getByTestId('shell');
  expect(shell.getAttribute('data-overlay')).toBe('true');
  // Same width W, painted over the transcript instead of squeezing it — and
  // the recents pane keeps the width it was given, not its minimum.
  expect(screen.getByTestId('col-right').className).toContain('overlay');
  expect(screen.getByTestId('col-right').getAttribute('style')).toContain(`width: ${DEFAULTS.W}px`);
  expect(tracks()).toContain(`grid-template-columns: ${DEFAULTS.L}px minmax(0, 1fr) 0px`);
  // No scrim: nothing sits between the person and the centre.
  expect(screen.queryByTestId('overlay-scrim')).toBeNull();

  // Typing in the composer beside the panel works, and Esc takes the cursor
  // from nowhere: it stays in the field the person was typing in.
  const box = screen.getByTestId('probe') as HTMLTextAreaElement;
  box.focus();
  await fireEvent.input(box, { target: { value: 'draft' } });
  expect(box.value).toBe('draft');
  const area = screen.getByTestId('col-right');
  await fireEvent.keyDown(window, { key: 'Escape' });
  // Closed means hidden: the tree stays mounted for the next open.
  expect(screen.getByTestId('col-right')).toBe(area);
  expect(area.hasAttribute('hidden')).toBe(true);
  expect(toggle('right').getAttribute('aria-pressed')).toBe('false');
  expect(document.activeElement).toBe(box);

  // The pointer way out is the header's own «Файли» toggle.
  await fireEvent.click(toggle('right'));
  expect(screen.getByTestId('col-right').hasAttribute('hidden')).toBe(false);
  await fireEvent.click(toggle('right'));
  expect(screen.getByTestId('col-right').hasAttribute('hidden')).toBe(true);
});

test('a drag writes back only the pane it moved, never the clamped neighbour', async () => {
  // 1100 is narrower than the person's 240 + 520 + 480: the right area is an
  // overlay and L is drawn as it was stored. A drag must not persist the
  // squeezed copy — that is how the person's widths disappeared for good.
  setWindowWidth(1100);
  render(Shell);
  const [recents] = screen.getAllByRole('separator');
  await fireEvent.keyDown(recents, { key: 'ArrowRight' });
  expect(stored().L).toBe(DEFAULTS.L + 8);
  expect(stored().W).toBe(DEFAULTS.W);

  // The window grows back and the person's widths are still there.
  cleanup();
  setWindowWidth(WINDOW);
  render(Shell);
  expect(tracks()).toContain(`grid-template-columns: ${DEFAULTS.L + 8}px minmax(0, 1fr) ${DEFAULTS.W}px`);
});

test('an overlay wider than the room never covers the conversations pane', () => {
  // 380 + 600 + 520 > 900: the overlay paints min(600, 900 − 380) and stops at
  // the recents pane's edge, so its splitter stays reachable.
  storage.setItem('mnema.window', JSON.stringify({ ...DEFAULTS, L: 380, W: 600 }));
  setWindowWidth(900);
  render(Shell);
  expect(screen.getByTestId('shell').getAttribute('data-overlay')).toBe('true');
  expect(tracks()).toContain(`grid-template-columns: 380px minmax(0, 1fr) 0px`);
  expect(screen.getByTestId('col-right').getAttribute('style')).toContain(`width: ${900 - 380}px`);
  // The right area's own splitter stays out while the overlay is open; the
  // recents splitter — the way back from an overlay grown too wide — is on
  // screen, not under the overlay. The tree's splitter rides inside the panel.
  expect(screen.getAllByRole('separator')).toHaveLength(2);
});

test('the header is the drag handle, and it is the only one', () => {
  const { container } = render(Shell);
  const handles = container.querySelectorAll('[data-tauri-drag-region]');
  expect(handles).toHaveLength(1);
  expect(handles[0].className).toBe('head');
  // The attribute is inert without the permission — guard both in one place.
  const capability = JSON.parse(
    readFileSync(join(HERE, '../../../src-tauri/capabilities/launcher.json'), 'utf8'),
  ) as { windows: string[]; permissions: string[] };
  expect(capability.windows).toContain('launcher');
  expect(capability.permissions).toContain('core:window:allow-start-dragging');
  expect(capability.permissions).not.toContain('core:window:allow-internal-toggle-maximize');
});

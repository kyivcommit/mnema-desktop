import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { render, screen, fireEvent, cleanup } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, test } from 'vitest';
import Shell from './Shell.svelte';
import { DEFAULTS, MIN, layoutFor } from './layout';

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
// The two columns the shell hands the browser, parsed back out of the style.
function drawn() {
  const m = tracks().match(/grid-template-columns: (\d+)px minmax\(0, 1fr\) (\d+)px/);
  expect(m).not.toBeNull();
  return { left: Number(m![1]), right: Number(m![2]) };
}
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

// The open file's path stands in the right panel's head, after the tree toggle;
// with no file the head carries the button alone.
test('the open file path is drawn in the right head after the tree toggle', () => {
  const { unmount } = render(Shell, { fileOpen: true, fileTitle: 'docs/notes/b.md' });
  const head = document.querySelector('.right-head')!;
  const path = screen.getByTestId('file-header');
  expect(head.contains(path)).toBe(true);
  expect(path.textContent).toBe('docs/notes/b.md');
  expect(path.getAttribute('title')).toBe('docs/notes/b.md');
  expect(toggle('tree').compareDocumentPosition(path) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  unmount();

  render(Shell, { fileOpen: false, fileTitle: null });
  expect(screen.queryByTestId('file-header')).toBeNull();
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

// Owner, 2026-10-20: the composer is never covered. These are the tracks the
// browser is handed, at the widths the spec names — a probe textarea in the
// transcript proves nothing, and the earlier test passed while the box was 74%
// painted over. The composer is `.centre-body`'s child, inset by its 20px
// padding, so its edges are the middle track's edges inset.
test.each([900, 1000, 1240])(
  'at %ipx the composer keeps its 520 and nothing paints over it',
  (width) => {
    setWindowWidth(width);
    render(Shell, { fileOpen: true });
    const view = layoutFor(DEFAULTS, width);
    expect(drawn()).toEqual({ left: view.left, right: view.right });
    // Visible panes plus the transcript's floor never exceed the window.
    expect(view.left + view.right + MIN.TRANSCRIPT).toBeLessThanOrEqual(width);
    const composerLeft = view.left + 20;
    const composerRight = width - view.right - 20;
    expect(composerLeft).toBeGreaterThanOrEqual(view.left);
    expect(composerRight).toBeLessThanOrEqual(width - view.right);
    // 520 minus the two insets: a line of text at every one of these widths.
    expect(composerRight - composerLeft).toBeGreaterThanOrEqual(MIN.TRANSCRIPT - 40);
    // And the right area is a column like any other: no overlay, no painted
    // width, no scrim.
    expect(screen.getByTestId('shell').hasAttribute('data-overlay')).toBe(false);
    expect(screen.getByTestId('col-right').className).not.toContain('overlay');
    expect(screen.getByTestId('col-right').getAttribute('style')).toBeNull();
  },
);

// §2's priority, high to low: transcript > file panel > conversations. The
// toggle keeps showing the person's choice, so an auto-hidden pane is not a
// switched-off one and a press can bring it back.
test('at 900px the conversations pane waits for room, and its toggle still brings it back', async () => {
  setWindowWidth(900);
  render(Shell, { fileOpen: true });
  // The window took the track; the person did not untoggle it.
  expect(screen.queryByTestId('col-recents')).toBeNull();
  expect(toggle('left').getAttribute('aria-pressed')).toBe('true');
  expect(drawn()).toEqual({ left: 0, right: 380 });

  // The press asks for it at a width where it cannot sit beside the panel, so
  // the FILE panel yields: the open file stays in state, the transcript keeps
  // its 520, and the 240 goes to the conversations.
  await fireEvent.click(toggle('left'));
  expect(screen.getByTestId('col-recents')).toBeTruthy();
  expect(screen.getByTestId('col-right').hasAttribute('hidden')).toBe(true);
  expect(screen.getByTestId('col-file')).toBeTruthy();
  expect(drawn()).toEqual({ left: DEFAULTS.L, right: 0 });
  expect(toggle('right').getAttribute('aria-pressed')).toBe('true');

  // «Файли» takes the room back — the file panel has priority — and the
  // conversations yield again, still wanted.
  await fireEvent.click(toggle('right'));
  expect(screen.getByTestId('col-right').hasAttribute('hidden')).toBe(false);
  expect(screen.queryByTestId('col-recents')).toBeNull();
  expect(toggle('left').getAttribute('aria-pressed')).toBe('true');

  // The window grows and everything fits: the conversations return untouched.
  setWindowWidth(1240);
  await fireEvent(window, new Event('resize'));
  expect(screen.getByTestId('col-recents')).toBeTruthy();
  expect(drawn()).toEqual({ left: DEFAULTS.L, right: DEFAULTS.W });
  expect(toggle('left').getAttribute('aria-pressed')).toBe('true');
});

// The press used to be read from the state before it: turning the pane back on
// in a narrow window wrote the preference while leaving `pinned` false, so the
// button read pressed and `layoutFor` still drew no track — the pane appeared
// only on a second press. One press each way is what the toggle promises.
test('at 900px a conversations pane turned off comes back drawn on one press', async () => {
  setWindowWidth(900);
  storage.setItem('mnema.window', JSON.stringify({ ...DEFAULTS, left: false }));
  render(Shell, { fileOpen: true });
  expect(screen.queryByTestId('col-recents')).toBeNull();

  await fireEvent.click(toggle('left'));
  expect(toggle('left').getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByTestId('col-recents')).toBeTruthy();

  await fireEvent.click(toggle('left'));
  expect(toggle('left').getAttribute('aria-pressed')).toBe('false');
  expect(screen.queryByTestId('col-recents')).toBeNull();

  await fireEvent.click(toggle('left'));
  expect(toggle('left').getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByTestId('col-recents')).toBeTruthy();
  expect(drawn().left).toBe(DEFAULTS.L);
});

test('a drag writes only the pane it moved, and stops at the transcript wall', async () => {
  // 1240 is exactly L + 520 + W: the recents pane has no room to grow, so a
  // drag leaves BOTH widths as the person set them — no squeezed copy lands in
  // storage, which is how those widths used to disappear for good.
  setWindowWidth(1240);
  render(Shell);
  const [recents] = screen.getAllByRole('separator');
  await fireEvent.keyDown(recents, { key: 'ArrowRight' });
  expect(stored().L).toBe(DEFAULTS.L);
  expect(stored().W).toBe(DEFAULTS.W);

  // At 900 the panel is drawn at the room beside the 520 (380), not at the
  // stored 480. Its splitter narrows the panel and never the transcript; the
  // conversations pane keeps its width while it waits.
  cleanup();
  setWindowWidth(900);
  render(Shell);
  const [right] = screen.getAllByRole('separator');
  expect(right.getAttribute('aria-valuemax')).toBe(String(900 - MIN.TRANSCRIPT));
  await fireEvent.keyDown(right, { key: 'ArrowRight' });
  expect(stored().W).toBe(900 - MIN.TRANSCRIPT - 8);
  expect(stored().L).toBe(DEFAULTS.L);
  expect(drawn()).toEqual({ left: 0, right: 900 - MIN.TRANSCRIPT - 8 });
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

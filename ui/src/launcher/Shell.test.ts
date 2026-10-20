import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { render, screen, fireEvent, cleanup } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, test } from 'vitest';
import Shell from './Shell.svelte';
import { DEFAULTS, MIN } from './layout';

const HERE = dirname(fileURLToPath(import.meta.url));

// jsdom under vitest hands out a `localStorage` with no working methods (an
// opaque origin), so this stubs one in memory — the layout has to survive a
// remount, and that is the whole of the test below.
function memoryStorage() {
  const map = new Map<string, string>();
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => { map.set(k, v); },
    removeItem: (k: string) => { map.delete(k); },
  };
}

// jsdom has no layout; the shell reads the window width once, at mount, as a
// real window does when it is raised. The default window is 1440 wide (§1).
const WINDOW = 1440;
function setWindowWidth(width: number) {
  Object.defineProperty(window, 'innerWidth', { value: width, configurable: true });
}

// `defineProperty`, not `stubGlobal`: Node 26's own `localStorage` getter
// warns when touched, and this replaces it without reading it.
function stubStorage() {
  Object.defineProperty(globalThis, 'localStorage', { value: memoryStorage(), configurable: true });
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

  // A splitter has a name, a vertical orientation and arrow keys (±8px).
  await fireEvent.click(toggle('left'));
  const [recents] = screen.getAllByRole('separator');
  expect(recents.getAttribute('aria-orientation')).toBe('vertical');
  expect(recents.getAttribute('aria-label')).toBeTruthy();
  await fireEvent.keyDown(recents, { key: 'ArrowRight' });
  expect(tracks()).toContain(`grid-template-columns: ${DEFAULTS.L + 8}px `);
});

test('the tree toggle does not change the transcript width', async () => {
  render(Shell, { fileOpen: true });
  // A file is open, so the tree is its own T and the file gets the rest of W.
  expect(rightTracks()).toContain(`grid-template-columns: ${DEFAULTS.T}px`);
  await fireEvent.click(toggle('tree'));
  expect(toggle('tree').getAttribute('aria-pressed')).toBe('false');
  expect(screen.queryByTestId('col-tree')).toBeNull();
  // The file pane takes the tree's width; the transcript's two side tracks did
  // not move.
  expect(rightTracks()).toContain('grid-template-columns: 0px');
  expect(tracks()).toContain(`${DEFAULTS.L}px minmax(0, 1fr) ${DEFAULTS.W}px`);
});

test('with no file open the tree fills the whole right area', () => {
  render(Shell);
  // §2: no file open means no file stripe at all — the tree is W wide and its
  // splitter is not rendered.
  expect(rightTracks()).toContain(`grid-template-columns: ${DEFAULTS.W}px`);
  expect(screen.getAllByRole('separator')).toHaveLength(2); // recents | transcript, transcript | right
});

test('the layout survives a remount', async () => {
  const first = render(Shell);
  await fireEvent.click(toggle('left'));
  await fireEvent.click(toggle('tree'));
  expect(tracks()).toContain('grid-template-columns: 0px');
  first.unmount();

  render(Shell);
  expect(tracks()).toContain('grid-template-columns: 0px');
  expect(toggle('tree').getAttribute('aria-pressed')).toBe('false');
});

test('at 900px the right area is an overlay; Esc and a click beside it close it', async () => {
  setWindowWidth(900);
  render(Shell);
  const shell = screen.getByTestId('shell');
  expect(shell.getAttribute('data-overlay')).toBe('true');
  // At 900 the two panes clamp to their minimums, and the right area is
  // painted over the transcript at that W instead of squeezing it.
  expect(screen.getByTestId('col-right').className).toContain('overlay');
  expect(tracks()).toContain(`minmax(0, 1fr) 0px`);
  expect(screen.getByTestId('col-right').getAttribute('style')).toContain(`width: ${MIN.W}px`);

  await fireEvent.keyDown(window, { key: 'Escape' });
  expect(screen.queryByTestId('col-right')).toBeNull();
  expect(toggle('right').getAttribute('aria-pressed')).toBe('false');

  // And the other way out: a click anywhere beside the overlay.
  cleanup();
  stubStorage();
  render(Shell);
  await fireEvent.click(screen.getByTestId('overlay-scrim'));
  expect(screen.queryByTestId('col-right')).toBeNull();
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

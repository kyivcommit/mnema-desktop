import { beforeEach, expect, test } from 'vitest';
import {
  DEFAULTS, MIN, clampLayout, isOverlay, loadLayout, resetPane, saveLayout, type PanelState,
} from './layout';

// The window the specs are written against: 1440 wide (§1).
const WINDOW = 1440;

// jsdom under vitest hands out a `localStorage` with no working methods (an
// opaque origin), so the tests stub one in memory, as `Models.test.ts` does.
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
  Object.defineProperty(globalThis, 'localStorage', { value: memoryStorage(), configurable: true });
}
beforeEach(stubStorage);

const saved = (over: Partial<PanelState>) => {
  localStorage.setItem('mnema.window', JSON.stringify({ ...DEFAULTS, ...over }));
};

test('a saved layout narrower than the minimums is clamped on read', () => {
  saved({ L: 50, W: 100, T: 20 });
  const s = loadLayout();
  expect(s.L).toBe(MIN.L);
  expect(s.W).toBe(MIN.W);
  // The tree also leaves the file stripe its 200: W now is 360, so T caps at 160.
  expect(s.T).toBe(MIN.T);
  expect(s).toMatchObject({ left: true, right: true, tree: true });
});

test('toggle left hands its width to the transcript, not to the window', () => {
  // At 1000 the right area and the transcript cannot both fit: the columns are
  // the reason. Hiding the recents panel — not resizing the window — is what
  // makes it fit, which is exactly the width going to the 1fr track.
  const narrow = 1000;
  expect(isOverlay(DEFAULTS, narrow)).toBe(true);
  expect(isOverlay({ ...DEFAULTS, left: false }, narrow)).toBe(false);
  // And the hidden width is the same one the pane had while it was on screen.
  expect(clampLayout(DEFAULTS, WINDOW).L).toBe(DEFAULTS.L);
});

test('the transcript never goes below 520 while dragging the left splitter', () => {
  // 1440 − 520 − 480: the wall, not the window's minimum.
  expect(clampLayout({ ...DEFAULTS, L: 900 }, WINDOW, 'L').L).toBe(WINDOW - MIN.TRANSCRIPT - DEFAULTS.W);
  // The same drag with the right area hidden may go further: W is not taking room.
  expect(clampLayout({ ...DEFAULTS, right: false, L: 900 }, WINDOW, 'L').L).toBe(900);
  // The right splitter stops at the same wall: a real drag (steps of 8 from the
  // current widths) leaves the transcript at exactly 520 and does not touch L.
  let w = DEFAULTS;
  for (let i = 0; i < 200; i += 1) w = clampLayout({ ...w, W: w.W + 8 }, WINDOW, 'W');
  expect(w.W).toBe(WINDOW - MIN.TRANSCRIPT - DEFAULTS.L);
  expect(w.L).toBe(DEFAULTS.L);
  expect(WINDOW - w.L - w.W).toBe(MIN.TRANSCRIPT);
  // A splitter's keyboard step is clamped like a drag: eight presses past the
  // wall leave the transcript at exactly 520.
  let s = DEFAULTS;
  for (let i = 0; i < 200; i += 1) s = clampLayout({ ...s, L: s.L + 8 }, WINDOW, 'L');
  expect(WINDOW - s.L - s.W).toBeGreaterThanOrEqual(MIN.TRANSCRIPT);
  expect(WINDOW - s.L - s.W).toBeLessThan(MIN.TRANSCRIPT + 8);
});

test('below the sum the right area becomes an overlay', () => {
  const overlayAt = DEFAULTS.L + MIN.TRANSCRIPT + DEFAULTS.W; // 1240
  expect(isOverlay(DEFAULTS, 900)).toBe(true);
  expect(isOverlay(DEFAULTS, overlayAt - 1)).toBe(true);
  expect(isOverlay(DEFAULTS, overlayAt)).toBe(false);
  // The threshold follows the widths the person dragged, not the constants.
  const wide = clampLayout({ ...DEFAULTS, W: 600 }, WINDOW);
  expect(isOverlay(wide, overlayAt)).toBe(true);
  // A hidden right area is never an overlay, however narrow the window.
  expect(isOverlay({ ...DEFAULTS, right: false }, 900)).toBe(false);
});

test('double-click restores the default width for that pane', () => {
  const dragged = clampLayout({ ...DEFAULTS, L: 300, W: 520, T: 240 }, WINDOW);
  expect(resetPane(dragged, 'L', WINDOW).L).toBe(DEFAULTS.L);
  expect(resetPane(dragged, 'W', WINDOW).W).toBe(DEFAULTS.W);
  expect(resetPane(dragged, 'T', WINDOW).T).toBe(DEFAULTS.T);
  // A round trip through storage keeps the widths and the flags.
  saveLayout(dragged);
  expect(loadLayout()).toEqual(dragged);
});

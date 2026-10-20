import { beforeEach, expect, test } from 'vitest';
import {
  DEFAULTS, MIN, clampLayout, defaultWidth, isOverlay, loadLayout, maxWidth, saveLayout,
  type PanelState,
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
  // The drag wall, read from the same function `Shell` writes the pane with.
  expect(maxWidth(DEFAULTS, 'L', WINDOW)).toBe(WINDOW - MIN.TRANSCRIPT - DEFAULTS.W);
  // The same drag with the right area hidden may go further: W takes no room.
  expect(maxWidth({ ...DEFAULTS, right: false }, 'L', WINDOW)).toBe(WINDOW - MIN.TRANSCRIPT);
  // A real drag is steps of 8 from the current widths, each one clamped: the
  // transcript ends at exactly 520 and the W pane does not move.
  let s = DEFAULTS;
  for (let i = 0; i < 200; i += 1) s = { ...s, L: Math.min(s.L + 8, maxWidth(s, 'L', WINDOW)) };
  expect(s.L).toBe(WINDOW - MIN.TRANSCRIPT - DEFAULTS.W);
  expect(s.W).toBe(DEFAULTS.W);
  expect(WINDOW - s.L - s.W).toBe(MIN.TRANSCRIPT);
  // The right splitter stops at the same wall from its own side.
  let w = DEFAULTS;
  for (let i = 0; i < 200; i += 1) w = { ...w, W: Math.min(w.W + 8, maxWidth(w, 'W', WINDOW)) };
  expect(w.W).toBe(WINDOW - MIN.TRANSCRIPT - DEFAULTS.L);
  expect(w.L).toBe(DEFAULTS.L);
  expect(WINDOW - w.L - w.W).toBe(MIN.TRANSCRIPT);
  // The tree wall leaves the file stripe its 200.
  expect(maxWidth(DEFAULTS, 'T', WINDOW)).toBe(DEFAULTS.W - MIN.FILE);
});

test('below the sum the right area becomes an overlay', () => {
  const overlayAt = DEFAULTS.L + MIN.TRANSCRIPT + DEFAULTS.W; // 1240
  // The widths the person chose, at the widths a real window can be: the
  // decision and the overlay's W both survive the window being too narrow,
  // instead of collapsing to the minimums (180 + 520 + 360 = 1060).
  for (const narrow of [900, 1100, overlayAt - 1]) {
    expect(isOverlay(DEFAULTS, narrow), String(narrow)).toBe(true);
    const drawn = clampLayout(DEFAULTS, narrow);
    expect(drawn.L, String(narrow)).toBe(DEFAULTS.L);
    expect(drawn.W, String(narrow)).toBe(DEFAULTS.W);
    // The overlay is the person's W, not the squeezed one.
    expect(maxWidth(drawn, 'W', narrow), String(narrow)).toBe(DEFAULTS.W);
  }
  expect(isOverlay(DEFAULTS, overlayAt)).toBe(false);
  // The threshold follows the widths the person dragged, not the constants.
  expect(isOverlay({ ...DEFAULTS, W: 600 }, overlayAt)).toBe(true);
  // A hidden right area is never an overlay, however narrow the window.
  expect(isOverlay({ ...DEFAULTS, right: false }, 900)).toBe(false);
});

test('an overlay wider than the room keeps the recents pane uncovered', () => {
  // The recents pane's splitter is the only way back from an overlay that grew
  // too wide, so the overlay paints `min(W, window − L)`, never over the pane.
  const wide = { ...DEFAULTS, L: 380, W: 600 };
  expect(maxWidth(wide, 'W', 900)).toBe(900 - 380);
  expect(clampLayout(wide, 900).W).toBe(900 - 380);
  // Under the cap the person's W is what is painted: min(600, 900 − 240) is 600.
  expect(maxWidth({ ...DEFAULTS, W: 600 }, 'W', 900)).toBe(600);
  // The floor is the pane minimum, not zero.
  expect(maxWidth({ ...DEFAULTS, W: 600 }, 'W', 900)).toBeGreaterThanOrEqual(MIN.W);
  // Once the window has room, the stored width is back — the cap is display
  // only.
  expect(clampLayout(wide, 1600).W).toBe(600);
});

test('double-click restores the default width for that pane', () => {
  expect(defaultWidth('L')).toBe(DEFAULTS.L);
  expect(defaultWidth('W')).toBe(DEFAULTS.W);
  expect(defaultWidth('T')).toBe(DEFAULTS.T);
  // The write that follows a reset is clamped like a drag: a window where the
  // panes are already at their minimums has only 400 for the right area, so
  // the default 480 cannot come back at that width.
  const narrow = { ...DEFAULTS, L: MIN.L, W: MIN.W }; // 1060, fits a 1100 window
  const wall = maxWidth(narrow, 'W', 1100);
  expect(wall).toBe(1100 - MIN.TRANSCRIPT - MIN.L);
  expect(Math.min(defaultWidth('W'), wall)).toBe(wall);
  // A round trip through storage keeps the widths and the flags.
  saveLayout({ ...DEFAULTS, L: 300, W: 520, T: 240 });
  expect(loadLayout()).toEqual({ ...DEFAULTS, L: 300, W: 520, T: 240 });
});

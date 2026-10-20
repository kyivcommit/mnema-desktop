import { beforeEach, expect, test } from 'vitest';
import {
  DEFAULTS, MIN, clampLayout, defaultWidth, layoutFor, loadLayout, maxWidth, saveLayout,
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

test('a hidden pane hands its width to the transcript, not to the window', () => {
  // The three areas at 1440: the recents track, the 1fr, the panel's track.
  expect(layoutFor(DEFAULTS, WINDOW)).toEqual({ left: 240, right: 480, tree: 212, transcript: 720 });
  // Hiding a pane removes its track; the 1fr takes it and the window keeps its
  // size. The panel never changes width because a neighbour left.
  expect(layoutFor({ ...DEFAULTS, left: false }, WINDOW)).toEqual({
    left: 0, right: 480, tree: 212, transcript: 960,
  });
  expect(layoutFor({ ...DEFAULTS, right: false }, WINDOW)).toEqual({
    left: 240, right: 0, tree: 0, transcript: 1200,
  });
});

// §2, the ruling of 2026-10-20: the composer is never covered. These are the
// tracks the browser is handed, so the claim is arithmetic, not prose: the sum
// of every visible pane plus the transcript's floor can never exceed the window.
test('the transcript keeps its 520 at every legal window width', () => {
  for (const width of [900, 1000, 1240, WINDOW]) {
    const t = layoutFor(DEFAULTS, width);
    expect(t.left + t.right + MIN.TRANSCRIPT, String(width)).toBeLessThanOrEqual(width);
    expect(t.transcript, String(width)).toBeGreaterThanOrEqual(MIN.TRANSCRIPT);
  }
});

test('the conversations pane yields first and comes back with the room', () => {
  // 240 + 520 + 480 = 1240: at that width all three fit, one pixel less and
  // the conversations cannot be a column beside the panel.
  expect(layoutFor(DEFAULTS, 1240)).toEqual({ left: 240, right: 480, tree: 212, transcript: 520 });
  expect(layoutFor(DEFAULTS, 1239)).toEqual({ left: 0, right: 480, tree: 212, transcript: 759 });
  // The 900 minimum: the panel shrinks to the room beside the 520 — never under
  // its 360 — and the conversations wait.
  const tight = layoutFor(DEFAULTS, 900);
  expect(tight).toEqual({ left: 0, right: 380, tree: 180, transcript: 520 });
  // None of this rewrites the person's widths.
  expect(clampLayout(DEFAULTS)).toEqual(DEFAULTS);
  expect(clampLayout({ ...DEFAULTS, W: 600 })).toEqual({ ...DEFAULTS, W: 600 });
});

// §2: file panel above conversations. A pane the window took is not a pane the
// person switched off: pressing its toggle at that width makes the file panel
// yield instead, and the open file comes back when both fit.
test('the file panel yields for a conversations pane the person insists on', () => {
  expect(layoutFor(DEFAULTS, 900, true)).toEqual({ left: 240, right: 0, tree: 0, transcript: 660 });
  // The room returns and the pin costs nothing: both panes are drawn again.
  expect(layoutFor(DEFAULTS, 1240, true)).toEqual({ left: 240, right: 480, tree: 212, transcript: 520 });
  // Pinned, the recents splitter may use the room the file panel left.
  expect(maxWidth(DEFAULTS, 'L', 900, true)).toBe(380);
});

test('the transcript never goes below 520 while dragging a splitter', () => {
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

test('a saved width wider than the room is drawn narrower, and kept as saved', () => {
  const wide = { ...DEFAULTS, L: 380, W: 600 };
  expect(layoutFor(wide, 900)).toEqual({ left: 0, right: 380, tree: 180, transcript: 520 });
  // The squeeze is display only: the person's 600 is still in the state, so a
  // wider window draws it again.
  expect(clampLayout(wide)).toEqual(wide);
  expect(layoutFor(clampLayout(wide), 1440).right).toBe(600);
});

test('double-click restores the default width for that pane', () => {
  expect(defaultWidth('L')).toBe(DEFAULTS.L);
  expect(defaultWidth('W')).toBe(DEFAULTS.W);
  expect(defaultWidth('T')).toBe(DEFAULTS.T);
  // The write that follows a reset is clamped like a drag: at 1100 the room
  // beside the 520 leaves the panel 400 once the recents sit at their 180.
  const narrow = { ...DEFAULTS, L: MIN.L, W: MIN.W }; // 1060, fits a 1100 window
  const wall = maxWidth(narrow, 'W', 1100);
  expect(wall).toBe(1100 - MIN.TRANSCRIPT - MIN.L);
  expect(Math.min(defaultWidth('W'), wall)).toBe(wall);
  // A round trip through storage keeps the widths and the flags.
  saveLayout({ ...DEFAULTS, L: 300, W: 520, T: 240 });
  expect(loadLayout()).toEqual({ ...DEFAULTS, L: 300, W: 520, T: 240 });
});

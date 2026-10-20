// §2: the three toggles and the three widths are the window's own state — the
// Rust layout module is gone, and no `data-cols` decides where a panel goes.
// The transcript is the window's `1fr`; these numbers are the columns that take
// space from it, so a hidden panel hands its width to the transcript by being
// removed from the grid, never by the window changing size.
export type PanelState = {
  left: boolean;
  right: boolean;
  tree: boolean;
  L: number;
  W: number;
  T: number;
};

export const DEFAULTS: PanelState = { left: true, right: true, tree: true, L: 240, W: 480, T: 212 };

// §2's table. `FILE` is not a stored width: the file stripe is `W - T`, so the
// tree splitter is capped to leave it that much room.
export const MIN = { L: 180, W: 360, T: 160, FILE: 200, TRANSCRIPT: 520 };

// The tracks a window hands the browser, in px. `transcript` is the `1fr`
// carried as a number so callers can check the sum instead of inferring it.
export type Tracks = { left: number; right: number; tree: number; transcript: number };

const KEY = 'mnema.window';

// §2: what is actually drawn at this width. The composer's 520 is the one width
// that is never negotiated — no pane ever paints over it, at any legal window:
//
// - the right area is always a COLUMN, at `min(W, window − 520)`, never under
//   360; at the 900 minimum that is 380, so it always fits;
// - the conversations pane is a column of `L` only while `L + 520 + W` fits,
//   and yields first when it does not;
// - `pinned` is the person pressing its toggle anyway: the FILE panel yields
//   instead, and the open file stays in state until there is room for both.
export function layoutFor(s: PanelState, windowWidth: number, pinned = false): Tracks {
  const roomy = s.right ? Math.max(MIN.W, Math.min(s.W, windowWidth - MIN.TRANSCRIPT)) : 0;
  const L = Math.max(MIN.L, s.L);
  const share = roomy > 0 && s.left && L + roomy + MIN.TRANSCRIPT <= windowWidth;
  const showLeft = s.left && (share || pinned || roomy === 0);
  const showRight = s.right && !(pinned && !share);
  const right = showRight ? roomy : 0;
  // The pane that stays without `share` — pinned, or beside a shut right area —
  // is still bounded by the transcript's 520: a width dragged at a wider window
  // must not become the composer's loss when the window narrows, and the same
  // bound is what `aria-valuemax` announces.
  const left = showLeft ? Math.min(L, windowWidth - MIN.TRANSCRIPT - right) : 0;
  // The tree sits inside the right area: it takes its T while leaving the file
  // stripe its 200, and the file takes the rest.
  const tree = showRight ? Math.max(MIN.T, Math.min(s.T, roomy - MIN.FILE)) : 0;
  return { left, right, tree, transcript: windowWidth - left - right };
}

// The widest a pane may get: the wall a drag stops at, and the one `aria-valuemax`
// announces. Read from the tracks on screen, so a splitter never drags its own
// pane off the layout.
export function maxWidth(
  s: PanelState, pane: 'L' | 'W' | 'T', windowWidth: number, pinned = false,
): number {
  const drawn = layoutFor(s, windowWidth, pinned);
  switch (pane) {
    case 'L':
      return Math.max(MIN.L, windowWidth - MIN.TRANSCRIPT - drawn.right);
    case 'W':
      return Math.max(MIN.W, windowWidth - MIN.TRANSCRIPT - drawn.left);
    case 'T':
      return Math.max(MIN.T, drawn.right - MIN.FILE);
  }
}

// Saved widths are lifted to the minimums and the tree is capped to leave the
// file stripe its 200. The window is not consulted: a saved layout is not
// squeezed to the window it happens to be read in — `layoutFor` decides what
// fits on every render and its answer is never written back.
export function clampLayout(s: PanelState): PanelState {
  const W = Math.max(MIN.W, s.W);
  return { ...s, L: Math.max(MIN.L, s.L), W, T: Math.min(Math.max(MIN.T, s.T), W - MIN.FILE) };
}

// A saved layout belongs to the window that wrote it; the window may have
// become narrower since (§2), so what is read is clamped to the minimums. The
// window-width cap is the Shell's, on every render.
export function loadLayout(): PanelState {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? 'null') as Partial<PanelState> | null;
    if (raw === null || typeof raw !== 'object') return { ...DEFAULTS };
    const flag = (v: unknown, fallback: boolean) => (typeof v === 'boolean' ? v : fallback);
    const width = (v: unknown, fallback: number) => (typeof v === 'number' && Number.isFinite(v) ? v : fallback);
    return clampLayout({
      left: flag(raw.left, DEFAULTS.left),
      right: flag(raw.right, DEFAULTS.right),
      tree: flag(raw.tree, DEFAULTS.tree),
      L: width(raw.L, DEFAULTS.L),
      W: width(raw.W, DEFAULTS.W),
      T: width(raw.T, DEFAULTS.T),
    });
  } catch {
    return { ...DEFAULTS };
  }
}

// Best effort: a window that cannot remember its layout still lays out (§2
// asks for memory across restarts, not for the run to depend on it).
export function saveLayout(s: PanelState): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    // quota or a locked-down webview: the layout is not worth a crash
  }
}

// A splitter's double-click — or Enter, so the reset is not mouse-only — returns
// its pane to the default width (§2). The write that follows is clamped like a
// drag, by `maxWidth`.
export function defaultWidth(pane: 'L' | 'W' | 'T'): number {
  return DEFAULTS[pane];
}

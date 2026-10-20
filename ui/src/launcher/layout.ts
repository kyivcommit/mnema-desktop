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

const KEY = 'mnema.window';

// §2: below the sum of the widths the person chose, the right area stops being
// a column and becomes an overlay of the same width W over the transcript.
// Read from the STORED widths, never from a clamped layout: the clamped one has
// already been squeezed to fit, so a decision taken from it would always be
// 180 + 520 + 360 = 1060 whatever the person set, and the overlay would be as
// wide as the squeeze rather than as wide as the pane.
export function isOverlay(s: PanelState, windowWidth: number): boolean {
  return s.right && windowWidth < (s.left ? s.L : 0) + MIN.TRANSCRIPT + s.W;
}

// The widest a pane may get. The transcript keeps its 520 and the file stripe
// its 200; the overlay is not a column, so it is not bounded by the transcript.
// `clampLayout`, `Shell`'s splitter writes and `aria-valuemax` all read this
// one rule, so the wall a drag stops at and the wall a screen reader announces
// cannot differ.
export function maxWidth(s: PanelState, pane: 'L' | 'W' | 'T', windowWidth: number): number {
  const overlay = isOverlay(s, windowWidth);
  switch (pane) {
    case 'L':
      return Math.max(MIN.L, windowWidth - MIN.TRANSCRIPT - (s.right && !overlay ? s.W : 0));
    case 'W':
      return Math.max(MIN.W, overlay ? s.W : windowWidth - MIN.TRANSCRIPT - (s.left ? s.L : 0));
    case 'T':
      return Math.max(MIN.T, s.W - MIN.FILE);
  }
}

// What is drawn. In a column window the stored widths already fit (otherwise
// `isOverlay` would be true), so this only lifts a width past the minimums and
// caps the tree against the file's 200 — it never makes a decision about the
// overlay, and its result is never written back to storage.
export function clampLayout(s: PanelState, windowWidth: number): PanelState {
  const base = { ...s, L: Math.max(MIN.L, s.L), W: Math.max(MIN.W, s.W) };
  const L = base.left ? Math.min(base.L, maxWidth(base, 'L', windowWidth)) : base.L;
  const W = base.right ? Math.min(base.W, maxWidth({ ...base, L }, 'W', windowWidth)) : base.W;
  const T = Math.min(Math.max(MIN.T, s.T), maxWidth({ ...base, L, W }, 'T', windowWidth));
  return { ...base, L, W, T };
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
    }, Number.POSITIVE_INFINITY);
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

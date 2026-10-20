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

// The transcript never shrinks below 520 (§2): a splitter drag stops at that
// wall instead of pushing through it, on either side. The cap only applies
// while the pane is on screen — a hidden pane takes nothing from the transcript.
//
// `keep` is the pane the person is moving: it stops at its own wall and the
// other pane keeps the width it had. Without it (a layout read back from
// storage) both are capped, W first, so a too-wide W cannot squeeze L below
// its minimum on the way in.
export function clampLayout(s: PanelState, windowWidth: number, keep?: 'L' | 'W'): PanelState {
  const L0 = Math.max(MIN.L, s.L);
  const W0 = Math.max(MIN.W, s.W);
  const fitW = windowWidth - MIN.TRANSCRIPT - (s.left ? L0 : 0);
  const W = s.right
    ? Math.max(MIN.W, Math.min(W0, keep === 'L' ? W0 : fitW))
    : W0;
  const fitL = windowWidth - MIN.TRANSCRIPT - (s.right ? W : 0);
  const L = s.left
    ? Math.max(MIN.L, Math.min(L0, keep === 'W' ? L0 : fitL))
    : L0;
  const T = Math.min(Math.max(MIN.T, s.T), Math.max(MIN.T, W - MIN.FILE));
  return { ...s, L, W, T };
}

// §2: below the sum of the current widths the right area stops being a column
// and becomes an overlay of the same width W over the transcript. Current
// widths, not constants — a dragged W moves the threshold with it.
export function isOverlay(s: PanelState, windowWidth: number): boolean {
  return s.right && windowWidth < (s.left ? s.L : 0) + MIN.TRANSCRIPT + s.W;
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

// A splitter's double-click returns its pane to the default width (§2), still
// inside the minimums and the transcript's wall.
export function resetPane(s: PanelState, pane: 'L' | 'W' | 'T', windowWidth: number): PanelState {
  return clampLayout({ ...s, [pane]: DEFAULTS[pane] }, windowWidth, pane === 'T' ? undefined : pane);
}

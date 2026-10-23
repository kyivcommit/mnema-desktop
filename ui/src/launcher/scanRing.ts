import type { ScanSnapshot } from '../lib/ipc';

/** How full the ring around the add-folder button is: reading fills the first
 *  half, embedding the second. `'spin'` while a phase's total is still unknown,
 *  `null` when no scan is reading or embedding. `rootIndex` is one-based on the wire. */
export function ringFraction(s: ScanSnapshot): number | 'spin' | null {
  if (s.kind !== 'running') return null;
  const p = s.phase;
  if (p.kind !== 'reading' && p.kind !== 'embedding') return null;
  const { done, total } = p.counts;
  if (total === 0) return 'spin';
  const part = Math.min(done / total, 1);
  return p.kind === 'reading' ? 0.5 * (p.rootIndex - 1 + part) / p.rootCount : 0.5 + 0.5 * part;
}

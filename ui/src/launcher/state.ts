import type { AskAnswer, Refusal } from '../lib/ipc';

// Mirrors MAX_ASK_QUERY (bridge.rs:486). Backend is the source of truth; this
// is the convenience mirror so a blank/over-long query never reaches `ask`.
export const MAX_ASK_QUERY = 2048;

export type QueryCheck =
  | { ok: true; query: string }
  | { ok: false; reason: 'blank' | 'tooLong' };

export function checkQuery(raw: string): QueryCheck {
  if (raw.trim() === '') return { ok: false, reason: 'blank' };
  // Code points, like Rust's query.chars().count() — spread iterates code
  // points, raw.length would count UTF-16 units and diverge past the BMP.
  if ([...raw].length > MAX_ASK_QUERY) return { ok: false, reason: 'tooLong' };
  return { ok: true, query: raw };
}

export type LauncherState =
  | { kind: 'idle' } // A
  | { kind: 'inFlight'; query: string } // D
  | { kind: 'generated'; query: string; answer: Extract<AskAnswer, { kind: 'generated' }> } // B (PR 6)
  | { kind: 'citationsOnly'; query: string; answer: Extract<AskAnswer, { kind: 'citationsOnly' }> } // E (PR 6)
  | { kind: 'refused'; reason: Refusal } // F
  | { kind: 'error'; reason: 'blank' | 'tooLong' | 'askFailed' }; // the query guard AND a rejected ask: every non-idle state goes through the machine, so `error` is live

export function stateFromAnswer(query: string, a: AskAnswer): LauncherState {
  switch (a.kind) {
    case 'generated': return { kind: 'generated', query, answer: a };
    case 'citationsOnly': return { kind: 'citationsOnly', query, answer: a };
    case 'refused': return { kind: 'refused', reason: a.reason };
  }
}

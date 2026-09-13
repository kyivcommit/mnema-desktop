import type { AskAnswer, Refusal, ModelSettings } from '../lib/ipc';

// Mirrors MAX_ASK_QUERY (bridge.rs:486). Backend is the source of truth; this
// is the convenience mirror so a blank/over-long query never reaches `ask`.
export const MAX_ASK_QUERY = 2048;

// D155: on some window managers (mutter on X11) the drag of a frameless
// window is a pointer+keyboard grab that takes focus for the whole move and
// hands it back at the end. The webview sees that as `blur` a few
// milliseconds after the press on the handle. A blur that close to a press
// on the drag handle is the drag, not a dismissal. The window is short on
// purpose: where the drag keeps focus (macOS) no blur arrives, and after
// 300 ms a blur is a dismissal again — nothing is left armed. Lives here
// (not as an instance-script `export const` in Launcher.svelte) so the test
// can import it as a plain value.
export const DRAG_GRAB_WINDOW_MS = 300;

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

// §9.1 / owner ruling 2026-08-24: content (network/dense) search is offered only when a provider
// key is present AND the index has a chosen embedding model. A stored key with no chosen model
// still cannot embed a query, so key-present alone is not enough. Fail SAFE: `typeof … === 'string'`
// is true only for a real model name, so a null model — or a wire field renamed away to `undefined`
// — reads as no-model and content stays OFF, never wrongly ON.
export function providerReady(s: ModelSettings): boolean {
  return (
    s.key.kind === 'present' &&
    s.index.kind === 'read' &&
    typeof s.index.embeddingModel === 'string'
  );
}

export function stateFromAnswer(query: string, a: AskAnswer): LauncherState {
  switch (a.kind) {
    case 'generated': return { kind: 'generated', query, answer: a };
    case 'citationsOnly': return { kind: 'citationsOnly', query, answer: a };
    case 'refused': return { kind: 'refused', reason: a.reason };
  }
}

import type { AskAnswer, AskCitation, Hit, Refusal, ModelSettings } from '../lib/ipc';

// Mirrors MAX_ASK_QUERY (bridge.rs:486). Backend is the source of truth; this
// is the convenience mirror so a blank/over-long query never reaches `ask`.
export const MAX_ASK_QUERY = 2048;

// D155: on some window managers (mutter on X11) the drag of a frameless
// window is a pointer+keyboard grab that takes focus for the whole move and
// hands it back at the end. The webview sees that as `blur` a few
// milliseconds after the press on the handle. A blur that close to a press
// on the drag handle is the drag, not a dismissal. Measured on the Ubuntu
// stand (X11, mutter, software rendering): press-to-blur landed at
// 392 / 516 / 504 ms. A `pointerup` disarms the window as soon as it is a
// click, not a drag, so 1000 ms is only the fallback for a platform where
// neither the release nor a focus change reaches the webview after a drag —
// a 2x margin over the slowest measured trial. Lives here (not as an
// instance-script `export const` in Launcher.svelte) so the test can import
// it as a plain value.
export const DRAG_GRAB_WINDOW_MS = 1000;

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
  | { kind: 'error'; reason: 'tooLong' | 'askFailed' }; // the query guard AND a rejected ask: every non-idle state goes through the machine, so `error` is live

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

// The two answers that draw cards. `refused` is not one of them and the type
// says so, so a refusal cannot reach a card component by accident.
export type CardAnswer = Extract<AskAnswer, { kind: 'generated' | 'citationsOnly' }>;

// The launcher is cold until an answer with something to show arrives, and
// the first such answer is the only way in; going back is the `launcher-cold`
// event, not a state. A refusal, an error and a citations-only answer with no
// passages leave a cold launcher as narrow as it was, and a hot one stays hot
// through everything.
export type Heat = 'cold' | 'hot';

export function heatAfter(prev: Heat, s: LauncherState): Heat {
  if ((s.kind === 'generated' || s.kind === 'citationsOnly') && s.answer.citations.length > 0) {
    return 'hot';
  }
  return prev;
}

// The card the source panel opens on. The centre lists previews (generated) or
// ranked passages (citations only) in `citations` order, so the first of that
// list is the first card it draws; `Cards.test.ts` holds this against the DOM.
export function firstCard(a: CardAnswer): AskCitation | Hit | null {
  return a.citations[0] ?? null;
}

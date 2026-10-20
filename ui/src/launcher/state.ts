import type { AskAnswer, AskCitation, Hit, Refusal, ModelSettings } from '../lib/ipc';

// Mirrors MAX_ASK_QUERY (bridge.rs:486). Backend is the source of truth; this
// is the convenience mirror so a blank/over-long query never reaches `ask`.
export const MAX_ASK_QUERY = 2048;


export type QueryCheck =
  | { ok: true; query: string }
  | { ok: false; reason: 'tooLong' };

// A blank line is not judged here: Enter on one does nothing, and the caller
// returns before asking (owner, 2026-10-08).
export function checkQuery(raw: string): QueryCheck {
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
  | { kind: 'error'; reason: 'tooLong' | 'askFailed' | 'historyUnreadable' | 'historyFailed' }; // the query guard, a rejected ask, and a Recents open that failed: every non-idle state goes through the machine, so `error` is live

// §9.1 / owner ruling 2026-08-24: content (network/dense) search is offered only when the embed
// role has a provider that can serve it AND the index has a chosen embedding model. A stored key
// with no chosen model still cannot embed a query, so a provider alone is not enough. Fail SAFE:
// `typeof … === 'string'` is true only for a real model name, so a null model — or a wire field
// renamed away to `undefined` — reads as no-model and the role stays NOT ready, never wrongly ready.
// One predicate for both roles: the settings tab dots and status line call it, and so does the
// launcher's cloud (Launcher.svelte), which is `ok` only when the backend probe is ok AND this holds.
// Whether the role's provider can serve it right now, apart from the index.
export function roleServed(s: ModelSettings, role: 'embed' | 'chat'): boolean {
  const provider = s.roles[role];
  if (provider === null) return false;
  if (provider === 'openRouter') return s.key.kind === 'present';
  return role === 'embed' ? s.configured.mnemaEmbed : s.configured.mnemaChat;
}

export function roleReady(s: ModelSettings, role: 'embed' | 'chat'): boolean {
  if (!roleServed(s, role) || s.index.kind !== 'read') return false;
  return typeof (role === 'embed' ? s.index.embeddingModel : s.index.chatModel) === 'string';
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
// the first such answer is the only way in; nothing goes back — the window is
// no longer forgotten while it is hidden. A refusal, an error and a
// citations-only answer with no passages leave a cold launcher as narrow as it
// was, and a hot one stays hot through everything.
export type Heat = 'cold' | 'hot';

export function heatAfter(prev: Heat, s: LauncherState): Heat {
  if ((s.kind === 'generated' || s.kind === 'citationsOnly') && s.answer.citations.length > 0) {
    return 'hot';
  }
  return prev;
}

// One question with the answer it got. A conversation is a list of these.
export type Turn = { query: string; answer: CardAnswer };

// A turn joins the conversation when the model answered it, or when it was
// never asked (`notAsked`: the cause-free citations-only answer). A refusal or a
// provider failure is not a turn: it is drawn as the current answer and the next
// good one replaces it, so `turns` comes back unchanged.
export function turnsAfter(turns: Turn[], query: string, a: AskAnswer): Turn[] {
  if (a.kind === 'generated' || (a.kind === 'citationsOnly' && a.why.kind === 'notAsked')) {
    return [...turns, { query, answer: a }];
  }
  return turns;
}

// The card the source panel opens on. The centre lists previews (generated) or
// ranked passages (citations only) in `citations` order, so the first of that
// list is the first card it draws; `Cards.test.ts` holds this against the DOM.
export function firstCard(a: CardAnswer): AskCitation | Hit | null {
  return a.citations[0] ?? null;
}

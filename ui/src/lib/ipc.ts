import { invoke } from '@tauri-apps/api/core';

// Wire types — a hand mirror of the Rust serialization pinned in
// `bridge.rs`/`locator.rs` (see the PR 3/6 wire-pin tests). camelCase + a
// `kind` tag, EXCEPT Coordinate which is snake_case (§10 exception).

export type Coordinate =
  | { kind: 'page'; number: number }
  | { kind: 'line'; start: number; end: number }
  | { kind: 'sheet_rows'; sheet: string; start: number; end: number }
  | { kind: 'section'; title: string }
  | { kind: 'none' };

// The wire shape of `Refused`'s `reason` field (an object, tagged `kind`).
export type Refusal = { kind: 'noCandidates' } | { kind: 'emptyCompletion' };
// The bare discriminant value, for `refusalText(...)` in i18n.
export type RefusalKind = Refusal['kind'];

export type TextArmReport = { kind: 'off' } | { kind: 'answered'; matched: number };

export type ContentArmReport =
  | { kind: 'off' }
  | { kind: 'noKey' }
  | { kind: 'noModel' }
  | { kind: 'failed'; reason: string }
  | { kind: 'answered'; matched: number; embedded: number; total: number; reachable: number; inspected: number };

// documentId/ord/rootId are the citation's occurrence identity (PR 6a,
// owner-Codex P1 on PR #22): documentId + ord close the reused-chunk-id
// hazard (chunk.id has no AUTOINCREMENT), and rootId feeds Freshness only —
// `Some` when exactly one distinct watched root holds the document, `null`
// when zero or several do (`mnema_index::Citation`'s own doc comment).
export type Hit = {
  chunkId: number;
  text: string;
  relativePath: string | null;
  sectionTitle: string | null;
  coordinate: Coordinate;
  documentId: string;
  ord: number;
  rootId: number | null;
};

export type AskCitation = {
  anchor: number;
  chunkId: number;
  text: string;
  relativePath: string | null;
  sectionTitle: string | null;
  coordinate: Coordinate;
  documentId: string;
  ord: number;
  rootId: number | null;
};

export type AskAnswer =
  | { kind: 'generated'; answer: string; citations: AskCitation[]; text: TextArmReport; content: ContentArmReport }
  | { kind: 'citationsOnly'; citations: Hit[]; text: TextArmReport; content: ContentArmReport }
  | { kind: 'refused'; reason: Refusal; text: TextArmReport; content: ContentArmReport };

export type SearchAnswer = { hits: Hit[]; text: TextArmReport; content: ContentArmReport };

// Typed invoke wrappers. A rejected command rejects the promise with the
// backend `Error`'s Display string (error.rs:252-256) — callers branch on the
// command, not on parsed error shape.
export const ask = (query: string) => invoke<AskAnswer>('ask', { query });
export const setSearchArms = (text: boolean, content: boolean) =>
  invoke<void>('set_search_arms', { text, content });

// A NARROW read of `model_settings` (models.rs:590) — only what the arms row
// needs: is a provider key present, and which arms are on. PR 7 replaces this
// with the full ModelSettings / IndexRead. Structural typing lets `invoke`
// return the wider object; we read only the fields declared here.
export type KeyState =
  | { kind: 'present' }
  | { kind: 'absent' }
  | { kind: 'unreadable'; cause: string; reason: string };

export type IndexSettings =
  | { kind: 'read'; embeddingModel: string | null; searchTextArm: boolean; searchContentArm: boolean }
  | { kind: 'unreadable'; cause: string; reason: string };

export type ModelSettings = { key: KeyState; index: IndexSettings };

export const modelSettings = () => invoke<ModelSettings>('model_settings');

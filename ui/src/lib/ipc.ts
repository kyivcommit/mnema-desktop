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

export type TreeFile = { relativePath: string; documentId: string };
export type TreeRoot = { rootId: number; absolutePath: string; name: string; files: TreeFile[] };
export type RecentDoc = { documentId: string; rootId: number; relativePath: string; indexedAt: number };
export type TreeListing = { roots: TreeRoot[]; recents: RecentDoc[] };

export type Freshness =
  | { kind: 'current' } | { kind: 'reindexed' } | { kind: 'fileChanged' }
  | { kind: 'fileMissing' } | { kind: 'noPath' };
export type SourceBlock = { blockId: number; kind: string; text: string; pageNo: number; readingOrder: number };
export type WireSegment = { blockId: number; start: number; end: number; blockStart: number };
export type SourceAround =
  | { kind: 'excerpt'; blocks: SourceBlock[]; spans: WireSegment[]; documentId: string;
      sectionTitle: string | null; hasMoreBefore: boolean; hasMoreAfter: boolean; freshness: Freshness }
  | { kind: 'gone'; reason: { kind: 'noSuchChunk' } | { kind: 'idReused' } };

// Typed invoke wrappers. A rejected command rejects the promise with the
// backend `Error`'s Display string (error.rs:252-256) — callers branch on the
// command, not on parsed error shape.
export const ask = (query: string) => invoke<AskAnswer>('ask', { query });
export const setSearchArms = (text: boolean, content: boolean) =>
  invoke<void>('set_search_arms', { text, content });
export const listTree = () => invoke<TreeListing>('list_tree');
export const sourceAround = (c: AskCitation | Hit, radius = 3) =>
  invoke<SourceAround>('source_around', {
    chunkId: c.chunkId, passageText: c.text,
    citedDocumentId: c.documentId, citedOrd: c.ord, citedRootId: c.rootId,
    citedRelativePath: c.relativePath, radius,
  });

// A NARROW read of `model_settings` (models.rs:590) — the arms row (PR 6) only
// ever needed presence and the two arm flags, so `index`'s `Read` arm carried
// only those. PR 7's Models section (Task 4) widens the two `cause` fields
// from bare `string` to the real unions below — `KeyStoreFailure` and
// `UnreadableCause` each name a closed, small set of values (models.rs:707-719,
// models.rs:809-826) and a caller matching on a bare string cannot be told
// apart from one matching on message text, the failure mode `error.rs`'s own
// header exists to avoid. `ModelSettings.platform` is added for the same
// reason `IndexRead`'s numeric fields are still left out: Task 4 renders the
// `Unreadable` branch of `index` and nothing from `Read` beyond what the arms
// row already used, so this stays a structural subset rather than a mirror of
// every field `models.rs` sends — the full `IndexRead` card is §9.3, PR 9.
export type KeyStoreFailureCause = 'locked' | 'duplicate' | 'refused' | 'defect';

export type KeyState =
  | { kind: 'present' }
  | { kind: 'absent' }
  | { kind: 'unreadable'; cause: KeyStoreFailureCause; reason: string };

export type UnreadableCause = 'notOpen' | 'readFailed';

// `chatModel` widens the same structural subset one field: Task 5 needs the
// index's own answer for "which chat model is chosen" to show a selection
// that follows the backend rather than the click (§10 / the ordering hazard).
// Optional, not required: every fixture in this module that predates Task 5
// exercises key/index-read/arms behaviour and never renders a chat selection,
// so making the field required would force an unrelated edit onto every one
// of them for a value they do not use — `?? null` at the one call site that
// reads it treats "absent" and "stated null" alike, which is the same
// collapse this structural subset already makes for the fields it omits
// entirely.
export type IndexSettings =
  | { kind: 'read'; embeddingModel: string | null; chatModel?: string | null; searchTextArm: boolean; searchContentArm: boolean }
  | { kind: 'unreadable'; cause: UnreadableCause; reason: string };

// `Mac` | `Windows` | `Linux` (models.rs:625-629), camelCase per the wire
// convention every union in this module already follows.
export type Platform = 'mac' | 'windows' | 'linux';

export type ModelSettings = { key: KeyState; index: IndexSettings; platform: Platform };

export const modelSettings = () => invoke<ModelSettings>('model_settings');

// `Balance` (crates/mnema-provider/src/probe.rs:53-77): four states over the
// provider's account balance. Nothing in this PR renders it — a stated zero
// is the collapse the type was split in four to prevent, and rendering the
// other three arms correctly is not this task's — so `raw`/`Unreadable`'s
// payload is left as `unknown` rather than mirrored field-by-field for a
// value nothing here reads.
export type Balance =
  | { kind: 'known'; amount: number }
  | { kind: 'notStated' }
  | { kind: 'unreadable'; raw: unknown }
  | { kind: 'envelopeNotUnderstood' };

export type KeyStatus = { balance: Balance };

// `KeyRemoval` (models.rs:101-108): what `forget_key` answers, tagged `kind`
// like every other union in this module.
export type KeyRemoval = { kind: 'removed' } | { kind: 'nothingToRemove' };

export const setKey = (key: string) => invoke<KeyStatus>('set_key', { key });
export const forgetKey = () => invoke<KeyRemoval>('forget_key');

// The provider's model catalogue (`crates/mnema-provider/src/catalogue.rs`),
// mirrored field-by-field — unlike `IndexSettings.Read`, this type has no
// narrower subset to fall back on: `refusal` and `unreadableRecords` are the
// whole reason Task 5 exists, not fields it could leave out.
//
// Named `ModelRole`/`ModelRefusal` rather than `Role`/`Refusal`: this module
// already exports a `Refusal` for `AskCitation`'s refused-answer reason
// (`{kind:'noCandidates'}|{kind:'emptyCompletion'}`, `mnema_index`'s type) —
// a different enum in a different crate that happens to share a name in
// Rust, where the two never collide because they live in separate modules.
// Reusing the bare name here would either shadow that export or silently
// widen its union, and either failure mode is exactly the "same word, two
// meanings" class this project has paid for before.
export type ModelRole = 'embedding' | 'rerank' | 'chat';

export type InputLimit =
  | { kind: 'notStated' }
  | { kind: 'known'; tokens: number }
  | { kind: 'notUnderstood'; raw: string };

export type Price =
  | { kind: 'notStated' }
  | { kind: 'known'; amount: number }
  | { kind: 'notAPrice'; raw: string }
  | { kind: 'unreadable'; raw: string };

// `catalogue.rs`'s `Refusal`: five variants; one of them (`limitNotUnderstood`)
// carries the provider's own `raw` text, and `Models.svelte`'s
// `refusalReason` does not render it — see that function for why the
// sentence is fixed catalogue text rather than provider text.
export type ModelRefusal =
  | { kind: 'inputTooSmall'; limit: number; floor: number }
  | { kind: 'noStatedLimit' }
  | { kind: 'limitNotUnderstood'; raw: string }
  | { kind: 'noStatedOutputModalities' }
  | { kind: 'noTextOutput' };

export type ModelEntry = {
  id: string;
  name: string;
  inputLimit: InputLimit;
  price: Price;
  refusal: ModelRefusal | null;
};

// `catalogue.rs:293-304`: three states on purpose, not folded together — see
// `Models.svelte`'s `unreadableRecordLabel` for why each gets its own words.
export type RecordId =
  | { kind: 'absent' }
  | { kind: 'notAString'; raw: string }
  | { kind: 'known'; id: string };

export type UnreadableRecord = { id: RecordId; index: number };

export type Catalogue = {
  entries: ModelEntry[];
  unreadable: number;
  unreadableRecords: UnreadableRecord[];
};

// Public — no key (models.rs:178-179) — which is what lets the choice be
// shown before an account exists. `role` is validated on the Rust side
// (`Error::UnknownRole`); this module only ever passes the two roles the
// window offers (D123/D124 keep rerank and verify off screen).
export const providerModels = (role: ModelRole) => invoke<Catalogue>('provider_models', { role });

// `set_embedding_model` is deliberately absent from this module's exports —
// it belongs to Task 6, retires vector spaces and takes the job slot; nothing
// here may call it.
export const setChatModel = (model: string) => invoke<void>('set_chat_model', { model });

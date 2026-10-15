import { ROLE_FIELDS } from '../lib/fixtures';
import { expect, test } from 'vitest';
import { checkQuery, MAX_ASK_QUERY, stateFromAnswer, providerReady, roleReady, heatAfter, firstCard, turnsAfter } from './state';
import { generated, citationsOnly, emptyCitationsOnly, refusedNoCandidates, refusedEmptyCompletion } from '../lib/fixtures';
import type { AskAnswer, ModelSettings, IndexSettings } from '../lib/ipc';

test('a non-blank query within the limit is accepted', () => {
  expect(checkQuery('hello')).toEqual({ ok: true, query: 'hello' });
});

test('the length limit mirrors the backend at code-point granularity', () => {
  const atLimit = 'a'.repeat(MAX_ASK_QUERY);
  expect(checkQuery(atLimit)).toEqual({ ok: true, query: atLimit });
  const overLimit = 'a'.repeat(MAX_ASK_QUERY + 1);
  expect(checkQuery(overLimit)).toEqual({ ok: false, reason: 'tooLong' });
});

// The ASCII case above can't tell `[...raw].length` apart from `raw.length` —
// 'a' is one UTF-16 unit and one code point either way, so a regression to
// `raw.length` would still pass it. An astral character (surrogate pair: two
// UTF-16 units, one code point) is the case where the two metrics diverge,
// which is the actual thing the code-point requirement guards against.
test('the length limit counts an astral character as one code point, not two UTF-16 units', () => {
  const atLimit = '😀'.repeat(MAX_ASK_QUERY);
  expect(atLimit.length).toBe(MAX_ASK_QUERY * 2); // sanity: confirms this string exercises the divergence
  expect(checkQuery(atLimit)).toEqual({ ok: true, query: atLimit });
  const overLimit = '😀'.repeat(MAX_ASK_QUERY + 1);
  expect(checkQuery(overLimit)).toEqual({ ok: false, reason: 'tooLong' });
});

test('AskAnswer maps to the right launcher state', () => {
  expect(stateFromAnswer('q', generated)).toMatchObject({ kind: 'generated' });
  expect(stateFromAnswer('q', citationsOnly)).toMatchObject({ kind: 'citationsOnly' });
  expect(stateFromAnswer('q', refusedNoCandidates)).toMatchObject({ kind: 'refused', reason: { kind: 'noCandidates' } });
  expect(stateFromAnswer('q', refusedEmptyCompletion)).toMatchObject({ kind: 'refused', reason: { kind: 'emptyCompletion' } });
});

// §9.1 / owner ruling 2026-08-24: content search needs a provider key AND a
// chosen embedding model — key-presence alone (the old rule) is not enough.
// Four cases, both directions, and the second is the exact configuration the
// owner's live run hit: a key with no chosen model.
const presentKey: ModelSettings['key'] = { kind: 'present' };
const absentKey: ModelSettings['key'] = { kind: 'absent' };
const readWithModel: ModelSettings['index'] = {
  kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0,
  embeddingModel: 'text-embedding-3-small',
  searchTextArm: true,
  searchContentArm: false,
};
const readNoModel: ModelSettings['index'] = {
  kind: 'read', failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0,
  embeddingModel: null,
  searchTextArm: true,
  searchContentArm: false,
};
const unreadableIndex: ModelSettings['index'] = { kind: 'unreadable', cause: 'notOpen', reason: '' };
// `providerReady` reads only `key` and `index` — `platform` is irrelevant to
// it, so one fixed value stands for all four cases here (PR 7 Task 4 widened
// `ModelSettings` with this field).
const platform: ModelSettings['platform'] = 'linux';

test('providerReady: a present key and a chosen model → true', () => {
  expect(providerReady({ key: presentKey, index: readWithModel, platform, ...ROLE_FIELDS })).toBe(true);
});

test('providerReady: a present key with no chosen model → false (the live-smoke config)', () => {
  expect(providerReady({ key: presentKey, index: readNoModel, platform, ...ROLE_FIELDS })).toBe(false);
});

test('providerReady: a present key with an unreadable index → false', () => {
  expect(providerReady({ key: presentKey, index: unreadableIndex, platform, ...ROLE_FIELDS })).toBe(false);
});

test('providerReady: an absent key, even with a chosen model → false', () => {
  expect(providerReady({ key: absentKey, index: readWithModel, platform, ...ROLE_FIELDS })).toBe(false);
});

// Review P2-8: `index.kind === 'read'` was defended by nothing. Replacing it
// with `true &&` left all 284 tests passing, because the fixture above that was
// meant to cover it — `unreadableIndex` — drops TWO conditions at once: an
// `Unreadable` index carries no `embeddingModel`, so the third conjunct is
// already false and the second is never the reason the answer is `false`. The
// fixture below discriminates this conjunct alone: an index that is not `read`
// yet does carry a model name, which only a cast can build because the wire
// type does not admit it — and that is the point. The rule is about the index
// being READ, not merely about a model string being present somewhere.
const unreadableIndexCarryingAModel = {
  kind: 'unreadable', cause: 'notOpen', reason: 'r', embeddingModel: 'text-embedding-3-small',
} as unknown as IndexSettings;

test('providerReady: a present key and a model name on an index that is not read → false', () => {
  expect(providerReady({ key: presentKey, index: unreadableIndexCarryingAModel, platform, ...ROLE_FIELDS })).toBe(false);
});

// Cold -> hot happens on an answer that has something to show, and only then.
test('heatAfter: a generated or non-empty citations-only answer warms the launcher', () => {
  expect(heatAfter('cold', stateFromAnswer('q', generated))).toBe('hot');
  expect(heatAfter('cold', stateFromAnswer('q', citationsOnly))).toBe('hot');
});

test('heatAfter: a refusal, an empty citations-only answer, idle and in-flight leave a cold launcher cold', () => {
  expect(heatAfter('cold', stateFromAnswer('q', refusedNoCandidates))).toBe('cold');
  expect(heatAfter('cold', stateFromAnswer('q', emptyCitationsOnly))).toBe('cold');
  expect(heatAfter('cold', { kind: 'idle' })).toBe('cold');
  expect(heatAfter('cold', { kind: 'inFlight', query: 'q' })).toBe('cold');
  expect(heatAfter('cold', { kind: 'error', reason: 'askFailed' })).toBe('cold');
});

test('heatAfter: a hot launcher stays hot through every state', () => {
  expect(heatAfter('hot', { kind: 'error', reason: 'askFailed' })).toBe('hot');
  expect(heatAfter('hot', stateFromAnswer('q', refusedNoCandidates))).toBe('hot');
  expect(heatAfter('hot', { kind: 'idle' })).toBe('hot');
});

test('firstCard: the first citation of either answer, null when there is none', () => {
  const g = stateFromAnswer('q', generated);
  const c = stateFromAnswer('q', citationsOnly);
  const e = stateFromAnswer('q', emptyCitationsOnly);
  if (g.kind !== 'generated' || c.kind !== 'citationsOnly' || e.kind !== 'citationsOnly') throw new Error('fixture');
  expect(firstCard(g.answer)).toBe(g.answer.citations[0]);
  expect(firstCard(c.answer)).toBe(c.answer.citations[0]);
  expect(firstCard(e.answer)).toBeNull();
});

test('turnsAfter adds a generated answer and a not-asked citations-only one', () => {
  const t1 = turnsAfter([], 'q1', generated);
  expect(t1).toEqual([{ query: 'q1', answer: generated }]);
  const t2 = turnsAfter(t1, 'q2', citationsOnly);
  expect(t2.map((t) => t.query)).toEqual(['q1', 'q2']);
});

test('turnsAfter leaves the turns alone for a refusal and for a failed citations-only answer', () => {
  const t1 = turnsAfter([], 'q1', generated);
  expect(turnsAfter(t1, 'q2', refusedNoCandidates)).toBe(t1);
  expect(turnsAfter(t1, 'q2', { ...citationsOnly, why: { kind: 'offline' } } as AskAnswer)).toBe(t1);
  expect(turnsAfter(t1, 'q2', { ...citationsOnly, why: { kind: 'noToolUse' } } as AskAnswer)).toBe(t1);
});

// Per-role providers: readiness follows the provider the ROLE has, not the key.
const noKeyNoRoles = { key: absentKey, index: readWithModel, platform, roles: { embed: 'mnema' as const, chat: 'mnema' as const },
  configured: { openRouter: false, mnemaEmbed: true, mnemaChat: false } };

test('providerReady: a Mnema embed role needs no key, only its local model', () => {
  expect(providerReady(noKeyNoRoles)).toBe(true);
  expect(providerReady({ ...noKeyNoRoles, configured: { ...noKeyNoRoles.configured, mnemaEmbed: false } })).toBe(false);
});

test('providerReady: an empty embed role is not ready, whatever the key says', () => {
  expect(providerReady({ key: presentKey, index: readWithModel, platform, ...ROLE_FIELDS, roles: { embed: null, chat: 'openRouter' } })).toBe(false);
});

test('roleReady: the chat role reads its own provider and its own chosen model', () => {
  const chatIndex = { ...readWithModel, chatModel: 'some/chat' } as ModelSettings['index'];
  expect(roleReady({ ...noKeyNoRoles, index: chatIndex, configured: { ...noKeyNoRoles.configured, mnemaChat: true } }, 'chat')).toBe(true);
  expect(roleReady({ ...noKeyNoRoles, index: chatIndex }, 'chat')).toBe(false); // mnemaChat not downloaded
  expect(roleReady({ ...noKeyNoRoles, index: readWithModel, configured: { ...noKeyNoRoles.configured, mnemaChat: true } }, 'chat')).toBe(false); // no chat model chosen
  expect(roleReady({ key: presentKey, index: chatIndex, platform, ...ROLE_FIELDS, roles: { embed: 'openRouter', chat: null } }, 'chat')).toBe(false);
});

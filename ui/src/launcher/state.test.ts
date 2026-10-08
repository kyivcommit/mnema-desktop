import { expect, test } from 'vitest';
import { checkQuery, MAX_ASK_QUERY, stateFromAnswer, providerReady, heatAfter, firstCard } from './state';
import { generated, citationsOnly, emptyCitationsOnly, refusedNoCandidates, refusedEmptyCompletion } from '../lib/fixtures';
import type { ModelSettings, IndexSettings } from '../lib/ipc';

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
  expect(providerReady({ key: presentKey, index: readWithModel, platform })).toBe(true);
});

test('providerReady: a present key with no chosen model → false (the live-smoke config)', () => {
  expect(providerReady({ key: presentKey, index: readNoModel, platform })).toBe(false);
});

test('providerReady: a present key with an unreadable index → false', () => {
  expect(providerReady({ key: presentKey, index: unreadableIndex, platform })).toBe(false);
});

test('providerReady: an absent key, even with a chosen model → false', () => {
  expect(providerReady({ key: absentKey, index: readWithModel, platform })).toBe(false);
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
  expect(providerReady({ key: presentKey, index: unreadableIndexCarryingAModel, platform })).toBe(false);
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

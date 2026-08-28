import { expect, test } from 'vitest';
import { checkQuery, MAX_ASK_QUERY, stateFromAnswer, providerReady } from './state';
import { generated, citationsOnly, refusedNoCandidates, refusedEmptyCompletion } from '../lib/fixtures';
import type { ModelSettings } from '../lib/ipc';

test('a blank query is rejected', () => {
  expect(checkQuery('')).toEqual({ ok: false, reason: 'blank' });
  expect(checkQuery('   ')).toEqual({ ok: false, reason: 'blank' });
});

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
  kind: 'read',
  embeddingModel: 'text-embedding-3-small',
  searchTextArm: true,
  searchContentArm: false,
};
const readNoModel: ModelSettings['index'] = {
  kind: 'read',
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

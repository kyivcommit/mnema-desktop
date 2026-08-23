import { expect, test } from 'vitest';
import { checkQuery, MAX_ASK_QUERY, stateFromAnswer } from './state';
import { generated, citationsOnly, refusedNoCandidates, refusedEmptyCompletion } from '../lib/fixtures';

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

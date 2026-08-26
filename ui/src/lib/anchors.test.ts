import { expect, test } from 'vitest';

import { splitAnchors } from './anchors';

test('an anchor becomes an anchor segment and the prose around it survives', () => {
  expect(splitAnchors('Costs four hryvnias<c>1</c> per sheet.', new Set([1]))).toEqual([
    { kind: 'text', text: 'Costs four hryvnias' },
    { kind: 'anchor', n: 1 },
    { kind: 'text', text: ' per sheet.' },
  ]);
});

test('an anchor with no citation behind it stays literal text — never a dead link', () => {
  expect(splitAnchors('A claim<c>9</c>.', new Set([1]))).toEqual([
    { kind: 'text', text: 'A claim<c>9</c>.' },
  ]);
});

test('prose that merely looks like an anchor is not one', () => {
  const plain = 'The tag <c> is written like this, and 1</c> is not an anchor.';
  expect(splitAnchors(plain, new Set([1]))).toEqual([{ kind: 'text', text: plain }]);
});

test('two anchors in one sentence both resolve', () => {
  expect(splitAnchors('A<c>3</c> and B<c>7</c>.', new Set([3, 7])).filter(s => s.kind === 'anchor'))
    .toEqual([{ kind: 'anchor', n: 3 }, { kind: 'anchor', n: 7 }]);
});

test('an unknown tag stays literal without hiding a later known anchor', () => {
  expect(splitAnchors('A<c>9</c> then B<c>3</c>.', new Set([3]))).toEqual([
    { kind: 'text', text: 'A<c>9</c> then B' },
    { kind: 'anchor', n: 3 },
    { kind: 'text', text: '.' },
  ]);
});

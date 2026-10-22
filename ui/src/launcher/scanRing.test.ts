import { expect, test } from 'vitest';
import { ringFraction } from './scanRing';
import type { ScanSnapshot } from '../lib/ipc';

const counts = (done: number, total: number) =>
  ({ done, total, skipped: 0, refused: 0, contended: 0, secondsLeft: null });
const reading = (rootIndex: number, rootCount: number, done: number, total: number): ScanSnapshot =>
  ({ kind: 'running', cancellable: true, phase: { kind: 'reading', rootIndex, rootCount, rootPath: '/r', counts: counts(done, total) } });
const embedding = (done: number, total: number): ScanSnapshot =>
  ({ kind: 'running', cancellable: true, phase: { kind: 'embedding', counts: counts(done, total) } });

test('no ring outside a reading or embedding scan', () => {
  expect(ringFraction({ kind: 'idle' })).toBeNull();
  expect(ringFraction({ kind: 'ended', report: { embedding: { kind: 'notReached' }, endedIn: 'reading',
    reason: 'completed', message: null, resume: null } } as ScanSnapshot)).toBeNull();
  expect(ringFraction({ kind: 'running', cancellable: false, phase: { kind: 'removing', rootPath: '/r' } })).toBeNull();
  expect(ringFraction({ kind: 'running', cancellable: false, phase: { kind: 'other', job: 'probe' } })).toBeNull();
});

test('an unknown total spins', () => {
  expect(ringFraction(reading(1, 1, 0, 0))).toBe('spin');
  expect(ringFraction(embedding(0, 0))).toBe('spin');
});

test('reading fills the first half, one-based root index', () => {
  expect(ringFraction(reading(1, 1, 5, 10))).toBe(0.25);
  expect(ringFraction(reading(1, 1, 10, 10))).toBe(0.5);
  expect(ringFraction(reading(2, 2, 0, 4))).toBe(0.25);
  expect(ringFraction(reading(2, 2, 4, 4))).toBe(0.5);
});

test('embedding fills the second half', () => {
  expect(ringFraction(embedding(5, 10))).toBe(0.75);
  expect(ringFraction(embedding(10, 10))).toBe(1);
});

test('a count past its total stays inside its phase', () => {
  expect(ringFraction(reading(1, 1, 12, 10))).toBe(0.5);
  expect(ringFraction(embedding(12, 10))).toBe(1);
});

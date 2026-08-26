import { render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { expect, test } from 'vitest';
import Cards from './Cards.svelte';
import { stateFromAnswer } from './state';
import { generated, refusedNoCandidates, citationsOnly } from '../lib/fixtures';
import { setLocale } from '../i18n';

test('idle shows no cards at all (state A is the bare line)', () => {
  render(Cards, { state: { kind: 'idle' }, query: '' });
  expect(screen.queryByTestId('card-tree')).toBeNull();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

test('generated shows tree and centre; source waits for a click', () => {
  render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });
  expect(screen.getByTestId('card-tree')).toBeTruthy();
  expect(screen.getByTestId('card-centre')).toBeTruthy();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

// Controller ruling A: state F (refused) also draws no cards. The plan's
// illustrated tests only covered A and B — a `state.kind !== 'idle'` guard
// would pass both of those and still wrongly draw cards here.
test('refused shows no cards at all (state F)', () => {
  render(Cards, { state: stateFromAnswer('nothing indexed', refusedNoCandidates), query: 'nothing indexed' });
  expect(screen.queryByTestId('card-tree')).toBeNull();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

// I1 (review round 1): idle/generated/refused pinned three of `LauncherState`'s
// six variants; `Cards.svelte:14` branches on `state.kind`, so the other three
// — inFlight (D), citationsOnly (E), error — were free to draw cards without
// reddening anything. citationsOnly matters most: it is the line Task 9 will
// edit next, and a guard mistakenly written as
// `state.kind === 'generated' || state.kind === 'citationsOnly'` is the likely
// one. All three below must independently redden under the reviewer's mutant
// (`state.kind !== 'idle' && state.kind !== 'refused'`).
test('inFlight shows no cards at all (state D)', () => {
  render(Cards, { state: { kind: 'inFlight', query: 'q' }, query: 'q' });
  expect(screen.queryByTestId('card-tree')).toBeNull();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

test('citationsOnly shows no cards at all (state E is out of scope here)', () => {
  render(Cards, { state: stateFromAnswer('q', citationsOnly), query: 'q' });
  expect(screen.queryByTestId('card-tree')).toBeNull();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

test('error shows no cards at all', () => {
  render(Cards, { state: { kind: 'error', reason: 'blank' }, query: '' });
  expect(screen.queryByTestId('card-tree')).toBeNull();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

// I2 (review round 1): no test asserted a `Cards` aria-label, so presence, the
// right catalogue key on the right section, and $locale-reactivity (D130 /
// the Codex ④ defect on PR #20) all held by inspection only. Lifted from the
// reviewer's probe F.
test('card labels come from the catalogue, on the right section, and follow a live language switch', async () => {
  render(Cards, { state: stateFromAnswer('q', generated), query: 'q' });
  expect(screen.getByTestId('card-tree').getAttribute('aria-label')).toBe('Tree');
  expect(screen.getByTestId('card-centre').getAttribute('aria-label')).toBe('Answer');

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('card-tree').getAttribute('aria-label')).toBe('Дерево');
  expect(screen.getByTestId('card-centre').getAttribute('aria-label')).toBe('Відповідь');

  setLocale('en'); // restore — `locale` is a module-level store shared across this file's tests
  await tick();
  expect(screen.getByTestId('card-tree').getAttribute('aria-label')).toBe('Tree');
  expect(screen.getByTestId('card-centre').getAttribute('aria-label')).toBe('Answer');
});

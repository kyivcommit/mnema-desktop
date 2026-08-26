import { render, screen } from '@testing-library/svelte';
import { expect, test } from 'vitest';
import Cards from './Cards.svelte';
import { stateFromAnswer } from './state';
import { generated, refusedNoCandidates } from '../lib/fixtures';

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

import { render, screen, fireEvent } from '@testing-library/svelte';
import { expect, test, vi } from 'vitest';
import SearchLine from './SearchLine.svelte';
import { MAX_ASK_QUERY, type LauncherState } from './state';

test('state A shows only the search input, no message', () => {
  render(SearchLine, { state: { kind: 'idle' } as LauncherState, onSubmit: vi.fn() });
  expect(screen.getByRole('textbox')).toBeTruthy();
  expect(screen.queryByRole('status')).toBeNull();
  expect(screen.queryByRole('alert')).toBeNull();
});

test('Enter emits onSubmit with the raw query (the owner validates, not the line)', async () => {
  const onSubmit = vi.fn();
  render(SearchLine, { state: { kind: 'idle' } as LauncherState, onSubmit, query: 'hello' });
  await fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });
  expect(onSubmit).toHaveBeenCalledWith('hello');
});

test('state error(blank) shows the blank message, not a refusal', () => {
  render(SearchLine, { state: { kind: 'error', reason: 'blank' } as LauncherState, onSubmit: vi.fn() });
  // Tightened from /query|запит/i (Task 4 Finding 3): that also matches
  // query_failed's text, so it could not tell blank from askFailed apart.
  expect(screen.getByRole('alert').textContent).toMatch(/enter|введіть/i);
  expect(screen.queryByRole('status')).toBeNull();
});

test('state error(tooLong) shows the too-long message with the limit interpolated', () => {
  // Task 4 Finding 2: the only real-logic error branch (an interpolated
  // placeholder) was untested; if the intl param name ever drifts from
  // MAX_ASK_QUERY, IntlMessageFormat throws at render time and this fails.
  render(SearchLine, { state: { kind: 'error', reason: 'tooLong' } as LauncherState, onSubmit: vi.fn() });
  expect(screen.getByRole('alert').textContent).toContain(String(MAX_ASK_QUERY));
});

test('state error(askFailed) shows the failure message (a rejected ask is visible — Finding 1)', () => {
  render(SearchLine, { state: { kind: 'error', reason: 'askFailed' } as LauncherState, onSubmit: vi.fn() });
  expect(screen.getByRole('alert').textContent).toMatch(/could not|не вдалося/i);
});

test('state F shows the refusal message, not an alert', () => {
  render(SearchLine, { state: { kind: 'refused', reason: { kind: 'noCandidates' } } as LauncherState, onSubmit: vi.fn() });
  expect(screen.getByRole('status').textContent).toMatch(/found|знайдено/i);
  expect(screen.queryByRole('alert')).toBeNull();
});

test('state D shows a spinner and the phase line, query stays in the input', () => {
  render(SearchLine, { state: { kind: 'inFlight', query: 'my question' }, onSubmit: vi.fn(), query: 'my question' });
  expect(screen.getByRole('progressbar')).toBeTruthy();
  const phases = screen.getByTestId('phases').textContent ?? '';
  expect(phases).toMatch(/чат|chat/i);
  expect((screen.getByRole('textbox') as HTMLInputElement).value).toBe('my question');
  expect(screen.queryByRole('alert')).toBeNull(); // in flight is not also an error (assert both directions)
});

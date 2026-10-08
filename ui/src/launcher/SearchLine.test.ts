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

test('state error(tooLong) shows the too-long message with the limit interpolated', () => {
  // The only real-logic error branch (an interpolated placeholder) was
  // untested; if the intl param name ever drifts from MAX_ASK_QUERY,
  // IntlMessageFormat throws at render time and this fails.
  render(SearchLine, { state: { kind: 'error', reason: 'tooLong' } as LauncherState, onSubmit: vi.fn(), query: 'x' });
  expect(screen.getByRole('alert').textContent).toContain(String(MAX_ASK_QUERY));
});

test('state error(askFailed) shows the failure message (a rejected ask is visible)', () => {
  render(SearchLine, { state: { kind: 'error', reason: 'askFailed' } as LauncherState, onSubmit: vi.fn() });
  expect(screen.getByRole('alert').textContent).toMatch(/could not|не вдалося/i);
});

test('state F shows the refusal message, not an alert', () => {
  const { container } = render(SearchLine, { state: { kind: 'refused', reason: { kind: 'noCandidates' } } as LauncherState, onSubmit: vi.fn() });
  // Reached by class, asserted as a role — see the blank test above.
  expect(container.querySelector('.refusal')!.getAttribute('role')).toBe('status');
  expect(screen.getByRole('status').textContent).toMatch(/found|знайдено/i);
  expect(screen.queryByRole('alert')).toBeNull();
});

test('state D draws no extra row and no spinner; the input is the only busy signal', () => {
  const { container } = render(SearchLine, { state: { kind: 'inFlight', query: 'my question' }, onSubmit: vi.fn(), query: 'my question' });
  // The search panel must not change height while an ask runs: the line holds
  // the input and nothing else.
  expect(screen.queryByTestId('phases')).toBeNull();
  expect(container.querySelector('.spinner')).toBeNull();
  expect(screen.queryByRole('progressbar')).toBeNull();
  expect(container.querySelector('.search-line')!.children).toHaveLength(1);
  const box = screen.getByRole('textbox') as HTMLInputElement;
  expect(box.value).toBe('my question');
  expect(box.getAttribute('aria-busy')).toBe('true');
  expect(screen.queryByRole('alert')).toBeNull(); // in flight is not also an error (assert both directions)
});

test('the input is not busy outside state D', () => {
  render(SearchLine, { state: { kind: 'idle' }, onSubmit: vi.fn(), query: '' });
  expect(screen.getByRole('textbox').getAttribute('aria-busy')).toBeNull();
});

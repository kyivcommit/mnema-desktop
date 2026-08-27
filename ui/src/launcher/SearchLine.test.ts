import { render, screen, fireEvent } from '@testing-library/svelte';
import { expect, test, vi } from 'vitest';
import { tick } from 'svelte';
import SearchLine from './SearchLine.svelte';
import { setLocale } from '../i18n';
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
  const { container } = render(SearchLine, { state: { kind: 'error', reason: 'blank' } as LauncherState, onSubmit: vi.fn() });
  // The role is a claim here, not the way in: the element is reached by its
  // class, so the assertion cannot be satisfied by the locator that found it.
  // Reached by role alone, `role="alert"` survives only while nobody rewrites
  // the query — a locator is not an assertion.
  expect(container.querySelector('.guard')!.getAttribute('role')).toBe('alert');
  // Tightened from /query|запит/i: that also matches query_failed's text, so
  // it could not tell blank from askFailed apart.
  expect(screen.getByRole('alert').textContent).toMatch(/enter|введіть/i);
  expect(screen.queryByRole('status')).toBeNull();
});

test('state error(tooLong) shows the too-long message with the limit interpolated', () => {
  // The only real-logic error branch (an interpolated placeholder) was
  // untested; if the intl param name ever drifts from MAX_ASK_QUERY,
  // IntlMessageFormat throws at render time and this fails.
  render(SearchLine, { state: { kind: 'error', reason: 'tooLong' } as LauncherState, onSubmit: vi.fn() });
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

test('state D shows a spinner and the phase line, query stays in the input', () => {
  const { container } = render(SearchLine, { state: { kind: 'inFlight', query: 'my question' }, onSubmit: vi.fn(), query: 'my question' });
  // Reached by class, asserted as a role — see the blank test above.
  expect(container.querySelector('.spinner')!.getAttribute('role')).toBe('progressbar');
  expect(screen.getByRole('progressbar')).toBeTruthy();
  const phases = screen.getByTestId('phases').textContent ?? '';
  expect(phases).toMatch(/чат|chat/i);
  expect((screen.getByRole('textbox') as HTMLInputElement).value).toBe('my question');
  expect(screen.queryByRole('alert')).toBeNull(); // in flight is not also an error (assert both directions)
});

test('state D announces the phase line, it is not silent decoration', () => {
  // `role="status"` on the phase line is the whole of what a screen reader gets
  // told while a search runs — every other test in this file reaches that
  // element by its testid, so deleting the role left the suite green.
  render(SearchLine, { state: { kind: 'inFlight', query: 'q' }, onSubmit: vi.fn(), query: 'q' });
  expect(screen.getByRole('status')).toBe(screen.getByTestId('phases'));
});

test('state D phase line and spinner label follow a live language switch (Codex #4)', async () => {
  // The phase line and the spinner aria-label were bare `t()` calls with no
  // `$locale` dependency, so a switch during an in-flight search left them in
  // the old language while the placeholder/error strings updated. Both
  // directions: English first, then the switch must reach BOTH the line and
  // the aria-label.
  setLocale('en');
  render(SearchLine, { state: { kind: 'inFlight', query: 'q' }, onSubmit: vi.fn(), query: 'q' });
  const phases = () => screen.getByTestId('phases').textContent ?? '';
  const spinnerLabel = () => screen.getByRole('progressbar').getAttribute('aria-label') ?? '';
  expect(phases()).toContain('text'); // en
  expect(spinnerLabel()).toBe('chat'); // en
  setLocale('uk');
  await tick();
  expect(phases()).toContain('текст'); // uk — the live switch reached the phase line
  expect(phases()).toContain('зміст');
  expect(spinnerLabel()).toBe('чат'); // uk — and the spinner aria-label
  setLocale('en'); // leave the shared store as it was found
});

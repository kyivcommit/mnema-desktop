import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { render, screen, fireEvent, cleanup } from '@testing-library/svelte';
import { tick } from 'svelte';
import { expect, test, vi } from 'vitest';
import Composer from './Composer.svelte';
import { t } from '../i18n';
import { MAX_ASK_QUERY, type LauncherState } from './state';

const HERE = dirname(fileURLToPath(import.meta.url));

const idle: LauncherState = { kind: 'idle' };
const box = () => screen.getByRole('textbox') as HTMLTextAreaElement;
const send = () => screen.getByRole('button', { name: t('composer_send') }) as HTMLButtonElement;

// A keydown built by hand, so the test can see whether the browser's own default
// (inserting a newline) was cancelled — `fireEvent` does not report that.
function key(k: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init });
  box().dispatchEvent(event);
  return event;
}

test('enter_sends_and_shift_enter_inserts_a_newline', async () => {
  const onSubmit = vi.fn();
  render(Composer, { value: 'hello', state: idle, onSubmit });

  const enter = key('Enter');
  expect(onSubmit).toHaveBeenCalledWith('hello');
  // No newline in the value: Enter is the send key.
  expect(enter.defaultPrevented).toBe(true);

  // Shift+Enter is left to the browser: the newline it inserts is the point.
  const shift = key('Enter', { shiftKey: true });
  expect(shift.defaultPrevented).toBe(false);
  expect(onSubmit).toHaveBeenCalledTimes(1);

  // A held Enter repeats keydown; only the press submits.
  const repeat = key('Enter', { repeat: true });
  expect(repeat.defaultPrevented).toBe(false);
  expect(onSubmit).toHaveBeenCalledTimes(1);
});

test('the_send_button_is_disabled_for_an_empty_query_and_while_in_flight', async () => {
  const onSubmit = vi.fn();

  render(Composer, { value: '   ', state: idle, onSubmit });
  expect(send().disabled).toBe(true);
  key('Enter');
  expect(onSubmit).not.toHaveBeenCalled();

  cleanup();
  render(Composer, { value: 'question', state: idle, onSubmit });
  expect(send().disabled).toBe(false);
  await fireEvent.click(send());
  expect(onSubmit).toHaveBeenCalledWith('question');

  cleanup();
  onSubmit.mockClear();
  render(Composer, { value: 'question', state: { kind: 'inFlight', query: 'question' }, onSubmit });
  expect(send().disabled).toBe(true);
  expect(key('Enter').defaultPrevented).toBe(true); // still no newline, but nothing leaves
  expect(onSubmit).not.toHaveBeenCalled();
});

test('the_box_grows_to_five_rows_and_then_scrolls', async () => {
  render(Composer, { value: '', state: idle, onSubmit: vi.fn() });
  expect(box().getAttribute('rows')).toBe('1');

  // jsdom has no layout, so the height the browser would measure is stubbed:
  // three rows of text, taller than the one-row box it started as.
  Object.defineProperty(box(), 'scrollHeight', { value: 68, configurable: true });
  await fireEvent.input(box(), { target: { value: 'one\ntwo\nthree' } });
  await tick();
  expect(box().style.height).toBe('68px');

  // The five-row ceiling is the stylesheet's: past it the box stops growing and
  // scrolls instead.
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  const rule = css.match(/\.composer textarea\s*\{[^}]*\}/);
  expect(rule).not.toBeNull();
  expect(rule![0]).toMatch(/max-height:\s*calc\(1\.5em\s*\*\s*5\)/);
  expect(rule![0]).toMatch(/overflow-y:\s*auto/);
});

// Carried from `SearchLine.test.ts` before that line was replaced (§5): the
// composer inherits the guard and refusal rows, so the four checks that pinned
// their real logic move with them rather than dying with the input they sat in.

test('state error(tooLong) shows the too-long message with the limit interpolated', () => {
  // The only real-logic error branch (an interpolated placeholder): if the intl
  // param name drifts from MAX_ASK_QUERY, IntlMessageFormat throws at render
  // time and this fails.
  render(Composer, { value: 'x', state: { kind: 'error', reason: 'tooLong' } as LauncherState, onSubmit: vi.fn() });
  expect(screen.getByRole('alert').textContent).toContain(String(MAX_ASK_QUERY));
});

test('state error(askFailed) shows the failure message (a rejected ask is visible)', () => {
  render(Composer, { state: { kind: 'error', reason: 'askFailed' } as LauncherState, onSubmit: vi.fn() });
  expect(screen.getByRole('alert').textContent).toMatch(/could not|не вдалося/i);
});

test('state F shows the refusal message, not an alert', () => {
  const { container } = render(Composer, { state: { kind: 'refused', reason: { kind: 'noCandidates' } } as LauncherState, onSubmit: vi.fn() });
  // Reached by class, asserted as a role: the refusal is a status, not an error.
  expect(container.querySelector('.refusal')!.getAttribute('role')).toBe('status');
  expect(screen.getByRole('status').textContent).toMatch(/found|знайдено/i);
  expect(screen.queryByRole('alert')).toBeNull();
});

test('state D draws no alert and no progress bar; the field is the only busy signal', () => {
  const { container } = render(Composer, { value: 'my question', state: { kind: 'inFlight', query: 'my question' }, onSubmit: vi.fn() });
  expect(container.querySelector('.spinner')).toBeNull();
  expect(screen.queryByRole('progressbar')).toBeNull();
  expect((screen.getByRole('textbox') as HTMLTextAreaElement).value).toBe('my question');
  expect(screen.getByRole('textbox').getAttribute('aria-busy')).toBe('true');
  expect(screen.queryByRole('alert')).toBeNull(); // in flight is not also an error
});

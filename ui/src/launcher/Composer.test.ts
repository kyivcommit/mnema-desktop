import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { render, screen, fireEvent, cleanup } from '@testing-library/svelte';
import { tick } from 'svelte';
import { expect, test, vi } from 'vitest';
import Composer from './Composer.svelte';
import { t } from '../i18n';
import type { LauncherState } from './state';

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

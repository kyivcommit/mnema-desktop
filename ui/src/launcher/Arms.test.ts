import { render, screen, fireEvent } from '@testing-library/svelte';
import { vi, expect, test, beforeEach } from 'vitest';
import Arms from './Arms.svelte';

const setSearchArms = vi.fn();
vi.mock('../lib/ipc', () => ({ setSearchArms: (...a: unknown[]) => setSearchArms(...a) }));
beforeEach(() => setSearchArms.mockReset());

const boxes = () => screen.getAllByRole('checkbox') as HTMLInputElement[];

test('no provider: content is disabled, text is the locked floor', () => {
  render(Arms, { textOn: true, contentOn: false, provider: false });
  const [text, content] = boxes();
  expect(text.checked).toBe(true);
  expect(text.disabled).toBe(true);    // the only active arm locks (§7.2)
  expect(content.disabled).toBe(true); // content needs a provider
});

test('provider, both on: either can be turned off, neither locks', async () => {
  render(Arms, { textOn: true, contentOn: true, provider: true });
  const [text, content] = boxes();
  expect(text.disabled).toBe(false);
  expect(content.disabled).toBe(false);
  await fireEvent.click(text);
  expect(setSearchArms).toHaveBeenCalledWith(false, true); // text off; content stays
});

test('provider, only content on: content is the locked floor, text can come on', async () => {
  render(Arms, { textOn: false, contentOn: true, provider: true });
  const [text, content] = boxes();
  expect(content.disabled).toBe(true); // the only active arm locks
  expect(text.disabled).toBe(false);
  await fireEvent.click(text);
  expect(setSearchArms).toHaveBeenCalledWith(true, true);
});

test('content is off on the wire without a provider (the box and the arm that runs agree)', () => {
  render(Arms, { textOn: true, contentOn: true, provider: false });
  expect(boxes()[1].checked).toBe(false); // contentOn but no provider → shown and sent off
});

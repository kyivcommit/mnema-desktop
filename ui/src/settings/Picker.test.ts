import { render, screen, fireEvent, cleanup } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { expect, test, vi, afterEach } from 'vitest';
import Picker from './Picker.svelte';
import { matches } from './picker';

afterEach(cleanup);

const options = [
  { id: 'a', label: 'Alpha', aside: 'openrouter', group: 'G1' },
  { id: 'b', label: 'Beta', disabled: true, group: 'G1' },
  { id: 'c', label: 'Gamma', aside: 'mnema (local)', group: 'G2' },
];

function setup(extra: Record<string, unknown> = {}) {
  const onchange = vi.fn();
  render(Picker, { options, value: null, onchange, placeholder: 'Pick', labelledby: 'lbl', ...extra });
  return { onchange, button: screen.getByRole('combobox') };
}

test('matches_every_word_case_insensitively', () => {
  expect(matches('LUNA Open', 'OpenAI: GPT-6 Luna openrouter')).toBe(true);
  expect(matches('luna mnema', 'OpenAI: GPT-6 Luna openrouter')).toBe(false);
});

test('arrow_keys_move_the_active_option', async () => {
  const { button, onchange } = setup();
  await fireEvent.keyDown(button, { key: 'ArrowDown' }); // opens
  const active = () => button.getAttribute('aria-activedescendant');
  const first = active();
  expect(screen.getByRole('listbox')).toBeTruthy();
  expect(document.getElementById(first!)!.textContent).toContain('Alpha');
  await fireEvent.keyDown(button, { key: 'End' });
  expect(document.getElementById(active()!)!.textContent).toContain('Gamma');
  await fireEvent.keyDown(button, { key: 'Home' });
  expect(active()).toBe(first);
  await fireEvent.keyDown(button, { key: 'End' });
  await fireEvent.keyDown(button, { key: 'Enter' });
  expect(onchange).toHaveBeenCalledWith('c');
});

test('escape_closes_and_returns_focus', async () => {
  const { button } = setup({ searchable: true });
  await fireEvent.click(button);
  const input = screen.getByRole('searchbox');
  input.focus();
  await fireEvent.keyDown(input, { key: 'Escape' });
  expect(screen.queryByRole('listbox')).toBeNull();
  expect(document.activeElement).toBe(button);
});

test('disabled_options_are_skipped', async () => {
  const { button } = setup();
  await fireEvent.keyDown(button, { key: 'ArrowDown' });
  await fireEvent.keyDown(button, { key: 'ArrowDown' });
  expect(document.getElementById(button.getAttribute('aria-activedescendant')!)!.textContent).toContain('Gamma');
  await fireEvent.click(screen.getByText('Beta'));
  expect(screen.getByRole('listbox')).toBeTruthy();
});

test('the_marker_snippet_renders_per_option', async () => {
  const marker = createRawSnippet((o: () => { id: string }) => ({
    render: () => `<i data-testid="dot">${o().id}</i>`,
  }));
  const { button } = setup({ marker, value: 'a' });
  expect(screen.getAllByTestId('dot')).toHaveLength(1); // closed button
  await fireEvent.click(button);
  expect(screen.getAllByTestId('dot')).toHaveLength(4);
});

test('search_filters_and_groups_render', async () => {
  const { button } = setup({ searchable: true });
  await fireEvent.click(button);
  expect(screen.getAllByRole('group')).toHaveLength(2);
  await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'mnema gam' } });
  expect(screen.getAllByRole('option')).toHaveLength(1);
  expect(screen.getAllByRole('group')).toHaveLength(1);
});

test('click_outside_closes_without_change', async () => {
  const { button, onchange } = setup();
  await fireEvent.click(button);
  await fireEvent.mouseDown(document.body);
  expect(screen.queryByRole('listbox')).toBeNull();
  expect(onchange).not.toHaveBeenCalled();
});

test('closed_button_shows_label_aside_or_placeholder', () => {
  setup({ value: 'a' });
  const b = screen.getByRole('combobox');
  expect(b.textContent).toContain('Alpha');
  expect(b.textContent).toContain('openrouter');
  cleanup();
  setup({ value: 'zzz' });
  expect(screen.getByRole('combobox').textContent).toContain('Pick');
});

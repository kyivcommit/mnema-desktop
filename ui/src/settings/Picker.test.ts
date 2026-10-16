import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { render, screen, fireEvent, cleanup } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { expect, test, vi, afterEach } from 'vitest';
import Picker from './Picker.svelte';
import { matches } from './picker';

const scroll = vi.fn();
Element.prototype.scrollIntoView = scroll;
afterEach(() => { cleanup(); scroll.mockClear(); });

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

test('tab_to_an_outside_element_closes_the_list', async () => {
  const { button } = setup();
  const other = document.createElement('button');
  document.body.appendChild(other);
  await fireEvent.click(button);
  await fireEvent.focusOut(button, { relatedTarget: other });
  expect(screen.queryByRole('listbox')).toBeNull();
  expect(button.getAttribute('aria-expanded')).toBe('false');
  other.remove();
});

test('the_active_option_is_scrolled_into_view', async () => {
  const { button } = setup();
  await fireEvent.click(button);
  scroll.mockClear();
  await fireEvent.keyDown(button, { key: 'End' });
  const el = document.getElementById(button.getAttribute('aria-activedescendant')!)!;
  expect(scroll.mock.contexts.at(-1)).toBe(el);
  expect(scroll).toHaveBeenLastCalledWith({ block: 'nearest' });
});

test('home_and_end_are_left_to_the_search_input', async () => {
  const { button } = setup({ searchable: true });
  await fireEvent.click(button);
  const input = screen.getByRole('searchbox');
  const before = input.getAttribute('aria-activedescendant');
  const ev = await fireEvent.keyDown(input, { key: 'End' });
  expect(ev).toBe(true); // not preventDefault-ed
  expect(input.getAttribute('aria-activedescendant')).toBe(before);
});

test('option_ids_with_whitespace_give_valid_dom_ids', async () => {
  const { button } = setup({ options: [{ id: 'x y/z', label: 'Spacey' }] });
  await fireEvent.click(button);
  expect(screen.getByRole('option').id).not.toMatch(/\s/);
});

test('enter_with_no_match_does_nothing_and_activedescendant_stays_visible', async () => {
  const { button, onchange } = setup({ searchable: true });
  await fireEvent.click(button);
  const input = screen.getByRole('searchbox');
  await fireEvent.input(input, { target: { value: 'gam' } });
  const id = input.getAttribute('aria-activedescendant')!;
  expect(document.getElementById(id)!.textContent).toContain('Gamma');
  await fireEvent.input(input, { target: { value: 'nomatch' } });
  await fireEvent.keyDown(input, { key: 'Enter' });
  expect(onchange).not.toHaveBeenCalled();
});

test('markerAfter_follows_the_name_in_the_option_text', async () => {
  const markerAfter = createRawSnippet((o: () => { id: string }) => ({
    render: () => `<span>, state-${o().id}</span>`,
  }));
  const { button } = setup({ markerAfter, value: 'a' });
  expect(button.textContent).toMatch(/Alpha.*, state-a/);
  await fireEvent.click(button);
  const opt = screen.getAllByRole('option').find((x) => x.textContent?.includes('Gamma'))!;
  expect(opt.textContent).toMatch(/Gamma.*, state-c/);
});

test('options_carry_their_id', async () => {
  const { button } = setup();
  await fireEvent.click(button);
  expect(screen.getAllByRole('option').map((o) => o.getAttribute('data-id'))).toEqual(['a', 'b', 'c']);
});

test('the_placeholder_truncates_like_the_label_and_the_button_clips', () => {
  const src = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'Picker.svelte'), 'utf8');
  const rule = (sel: string) => src.match(new RegExp(`${sel.replace('.', '\\.')}\\s*\\{([^}]*)\\}`))![1];
  for (const decl of ['min-width: 0', 'overflow: hidden', 'text-overflow: ellipsis', 'white-space: nowrap']) {
    expect(rule('.picker-placeholder'), decl).toContain(decl);
  }
  expect(rule('.picker-button')).toContain('overflow: hidden');
});

import { render, screen, cleanup } from '@testing-library/svelte';
import { expect, test, afterEach } from 'vitest';
import Settings from './Settings.svelte';
import { setLocale } from '../i18n';

afterEach(() => {
  cleanup();
  setLocale('en'); // the store outlives the component; leave it as found
});

test('shows all four section names, in the spec order', () => {
  render(Settings);
  const nav = screen.getByRole('navigation');
  // spec order: Models, Folders, Indexing, Application — read as one string so
  // a swap in order fails even though all four words are still present.
  expect(nav.textContent).toBe('ModelsFoldersIndexingApplication');
});

test('clicking Folders shows the Folders heading and removes the Models heading', async () => {
  render(Settings);
  expect(screen.getByRole('heading', { name: 'Models' })).toBeTruthy();
  expect(screen.queryByRole('heading', { name: 'Folders' })).toBeNull();

  await screen.getByRole('button', { name: 'Folders' }).click();

  expect(screen.getByRole('heading', { name: 'Folders' })).toBeTruthy();
  expect(screen.queryByRole('heading', { name: 'Models' })).toBeNull();
});

test('the two unbuilt sections carry aria-disabled=true, the built two carry aria-disabled=false', () => {
  render(Settings);
  expect(screen.getByRole('button', { name: 'Models' }).getAttribute('aria-disabled')).toBe('false');
  expect(screen.getByRole('button', { name: 'Folders' }).getAttribute('aria-disabled')).toBe('false');
  expect(screen.getByRole('button', { name: 'Indexing' }).getAttribute('aria-disabled')).toBe('true');
  expect(screen.getByRole('button', { name: 'Application' }).getAttribute('aria-disabled')).toBe('true');
});

test('a person reading the screen sees a real window, not a bare nav', () => {
  const { container } = render(Settings);
  const text = container.textContent ?? '';
  // Read as a person: the four names are on screen, AND the panel carries
  // more than the nav alone would — the current section's own heading. A
  // nav-only render (no panel content at all) would make this equal the
  // nav's own text and this assertion would catch it.
  expect(text.length).toBeGreaterThan('ModelsFoldersIndexingApplication'.length);
  expect(text).toContain('Models'); // heading for the default section, distinct node from the nav button
});

test('clicking an unbuilt section shows its one placeholder sentence', async () => {
  render(Settings);
  await screen.getByRole('button', { name: 'Indexing' }).click();
  expect(screen.getByText('This section is not ready yet.')).toBeTruthy();
});

test('labels stay correct across a language switch after mount', async () => {
  render(Settings);
  setLocale('uk');
  await Promise.resolve(); // let the $derived reactions flush
  const nav = screen.getByRole('navigation');
  expect(nav.textContent).toBe('МоделіТекиІндексаціяЗастосунок');
  expect(screen.getByRole('heading', { name: 'Моделі' })).toBeTruthy();
});

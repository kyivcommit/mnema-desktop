import { render, screen, fireEvent, cleanup } from '@testing-library/svelte';
import { expect, test, afterEach } from 'vitest';
import Settings from './Settings.svelte';
import { setLocale } from '../i18n';

afterEach(() => {
  cleanup();
  setLocale('en'); // the store outlives the component; leave it as found
});

test('shows all four section names, in the spec order', () => {
  setLocale('en'); // seed, do not inherit: an earlier sibling switching the language must not decide this test
  render(Settings);
  const nav = screen.getByRole('navigation');
  // spec order: Models, Folders, Indexing, Application — read as one string so
  // a swap in order fails even though all four words are still present.
  expect(nav.textContent).toBe('ModelsFoldersIndexingApplication');
});

test('clicking Folders shows the Folders heading and removes the Models heading', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);
  expect(screen.getByRole('heading', { name: 'Models' })).toBeTruthy();
  expect(screen.queryByRole('heading', { name: 'Folders' })).toBeNull();

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));

  expect(screen.getByRole('heading', { name: 'Folders' })).toBeTruthy();
  expect(screen.queryByRole('heading', { name: 'Models' })).toBeNull();
});

// Owner's ruling: `aria-disabled` came off these buttons. They are fully
// operable — a click does switch the panel — so announcing them as disabled
// was a claim the window could not back, and its cost fell on exactly the
// people who would then never press them and never hear why the section is
// empty. What replaces it is a description that RESOLVES: asserting the
// attribute's presence would pass on a reference pointing at nothing, so the
// test reads the referenced node's own text.
test.each(['Indexing', 'Application'])(
  '%s describes itself with the not-ready sentence, and no section claims to be disabled',
  async (name) => {
    setLocale('en'); // seed, do not inherit
    const { container } = render(Settings);

    // A built section must carry no description WHILE IT IS SELECTED — that is
    // the state the condition branches on, and asserting it on a deselected
    // button instead passes even when the `disabled` half of the condition is
    // gone and every selected section points at the sentence.
    expect(screen.getByRole('button', { name: 'Models' }).getAttribute('aria-describedby')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));
    expect(screen.getByRole('button', { name: 'Folders' }).getAttribute('aria-describedby')).toBeNull();

    // Before it is selected the sentence is not on the page, so nothing may
    // point at it — a reference to a missing id is worse than none.
    expect(screen.getByRole('button', { name }).getAttribute('aria-describedby')).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name }));

    const id = screen.getByRole('button', { name }).getAttribute('aria-describedby');
    expect(id).toBe('section-not-ready');
    expect(container.querySelector(`#${id}`)?.textContent).toBe('This section is not ready yet.');

    // And no section claims to be disabled any more — all four, positively.
    for (const other of ['Models', 'Folders', 'Indexing', 'Application']) {
      expect(screen.getByRole('button', { name: other }).getAttribute('aria-disabled')).toBeNull();
    }
  },
);

// M3 (review): aria-pressed is the only signal of which section is selected —
// there is no CSS anywhere in this project. Both directions, before and after
// a click, the shape already used at launcher/Tree.test.ts:864.
test('aria-pressed says which section is selected', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);
  expect(screen.getByRole('button', { name: 'Models' }).getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByRole('button', { name: 'Folders' }).getAttribute('aria-pressed')).toBe('false');

  await fireEvent.click(screen.getByRole('button', { name: 'Folders' }));

  expect(screen.getByRole('button', { name: 'Models' }).getAttribute('aria-pressed')).toBe('false');
  expect(screen.getByRole('button', { name: 'Folders' }).getAttribute('aria-pressed')).toBe('true');
});

test('a person reading the screen sees a real window, not a bare nav', async () => {
  setLocale('en'); // seed, do not inherit
  const { container } = render(Settings);
  const panel = () => container.querySelector('.spane');
  // Equality, not containment: 'Models' already sits inside <nav>, so a
  // toContain over the whole page is satisfied by the nav alone and never
  // notices an empty or replaced panel. Equality forces the panel itself to
  // carry the heading — on the default section and on an unbuilt one.
  expect(panel()?.textContent).toBe('Models');

  await fireEvent.click(screen.getByRole('button', { name: 'Indexing' }));
  expect(panel()?.textContent).toBe('Indexing This section is not ready yet.');
});

// M2 (review): the Застосунок branch was rendered by no test — a person
// clicking it would get an empty panel and nothing would notice. Both
// unbuilt sections carry the sentence, so both are exercised here.
test.each(['Indexing', 'Application'])('clicking %s shows its one placeholder sentence', async (name) => {
  setLocale('en'); // seed, do not inherit
  render(Settings);
  await fireEvent.click(screen.getByRole('button', { name }));
  expect(screen.getByText('This section is not ready yet.')).toBeTruthy();
});

test('labels stay correct across a language switch after mount', async () => {
  setLocale('en'); // seed, do not inherit
  render(Settings);

  // M1 (review): read the placeholder once under 'en' BEFORE switching, so a
  // $derived missing `void $locale` still caches an English value here — the
  // mutant only dies if the read after the switch is a genuinely later one.
  await fireEvent.click(screen.getByRole('button', { name: 'Indexing' }));
  expect(screen.getByText('This section is not ready yet.')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Models' }));

  setLocale('uk');
  await Promise.resolve(); // let the $derived reactions flush
  const nav = screen.getByRole('navigation');
  expect(nav.textContent).toBe('МоделіТекиІндексаціяЗастосунок');
  expect(screen.getByRole('heading', { name: 'Моделі' })).toBeTruthy();

  await fireEvent.click(screen.getByRole('button', { name: 'Індексація' }));
  expect(screen.getByText('Ця секція ще не готова.')).toBeTruthy();
});

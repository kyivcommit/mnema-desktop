import { describe, it, expect } from 'vitest';
import { t, setLocale } from './index';
import { messages, type Key } from './catalog';

describe('i18n', () => {
  it('returns the string for the active locale', () => {
    setLocale('en'); expect(t('pin')).toBe('Pin');
    setLocale('uk'); expect(t('pin')).toBe('Пін');
  });

  it('falls back to EN and never returns a raw key', () => {
    setLocale('uk');
    // @ts-expect-error probing an unknown key
    expect(t('does_not_exist')).not.toBe('does_not_exist');
  });

  it('applies Ukrainian CLDR plural incl. the teen exception', () => {
    setLocale('uk');
    const f = (n: number) => t('indexed_documents', { count: n });
    expect(f(1)).toBe('1 документ');
    expect(f(2)).toBe('2 документи');
    expect(f(5)).toBe('5 документів');
    expect(f(11)).toBe('11 документів'); // teen → many
    expect(f(21)).toBe('21 документ');
    expect(f(22)).toBe('22 документи');
    expect(f(111)).toBe('111 документів');
  });

  // Re-review RM1. The state E banner introduces a list whose length the card
  // knows, and it used to be plural over all of them. Pinned HERE, beside the
  // catalogue's other plural, because an arm needs a COUNT and not a fixture:
  // `ASK_TOP_K` is 8 (`bridge.rs:496`), so 1 reaches `one`, 2 reaches `few` and
  // 5 reaches `many` — three states a person can actually get to. `Cards.test.ts`
  // owns the other half of the claim, that the card passes its own count.
  it('the state E banner counts passages through CLDR plural, in both locales', () => {
    setLocale('uk');
    const uk = (n: number) => t('citations_only_banner', { count: n });
    expect(uk(1)).toBe('Генерування недоступне. Пошук знайшов 1 уривок.');
    expect(uk(2)).toBe('Генерування недоступне. Пошук знайшов 2 уривки.');
    expect(uk(5)).toBe('Генерування недоступне. Пошук знайшов 5 уривків.');

    setLocale('en');
    const en = (n: number) => t('citations_only_banner', { count: n });
    expect(en(1)).toBe('Generation is unavailable. The search found 1 passage.');
    expect(en(2)).toBe('Generation is unavailable. The search found 2 passages.');
  });

  // Task 7. PR 8a Task 6 added this key with two independent plural arguments
  // (`paths`, `documents`) and no test at any count that tells the four CLDR
  // arms apart — the component tests that exercise it (Folders.test.ts) only
  // ever reach 1 and 2. Pinned here, beside the catalogue's other plurals, at
  // the same five counts `indexed_documents` above already uses: 1/21 reach
  // `one`, 2/22 reach `few`, 5 reaches `many` — and 21/22 confirm the arm reads
  // `count % 10` and `% 100`, not merely "is this 1 or 2". `documents` also
  // carries an explicit `=0` arm (Folders.svelte review I1: a stated zero is
  // its own sentence, never the plural's `other`), pinned separately.
  it('the exclusion-cost sentence counts both paths and documents through CLDR plural, in both locales', () => {
    setLocale('uk');
    const uk = (paths: number, documents: number) => t('settings_folders_exclude_cost', { paths, documents });
    expect(uk(1, 1)).toBe(
      'Станом на зараз: при наступному скануванні індекс втратить 1 файл із цієї теки, '
      + 'а 1 документ більше не знайдеться: інші шляхи на нього не ведуть.');
    expect(uk(2, 2)).toBe(
      'Станом на зараз: при наступному скануванні індекс втратить 2 файли із цієї теки, '
      + 'а 2 документи більше не знайдуться: інші шляхи на них не ведуть.');
    expect(uk(5, 5)).toBe(
      'Станом на зараз: при наступному скануванні індекс втратить 5 файлів із цієї теки, '
      + 'а 5 документів більше не знайдуться: інші шляхи на них не ведуть.');
    expect(uk(21, 21)).toBe(
      'Станом на зараз: при наступному скануванні індекс втратить 21 файл із цієї теки, '
      + 'а 21 документ більше не знайдеться: інші шляхи на нього не ведуть.');
    expect(uk(22, 22)).toBe(
      'Станом на зараз: при наступному скануванні індекс втратить 22 файли із цієї теки, '
      + 'а 22 документи більше не знайдуться: інші шляхи на них не ведуть.');
    // The `=0` arm for `documents`, against a `paths` count that is itself
    // non-zero — the state Folders.svelte review comment "🔴 ДВА числа..."
    // exists for: a path is lost and no document is.
    expect(uk(2, 0)).toBe(
      'Станом на зараз: при наступному скануванні індекс втратить 2 файли із цієї теки, '
      + 'а жоден документ не перестане знаходитися — кожен із них проіндексовано ще й за іншим шляхом.');

    setLocale('en');
    const en = (paths: number, documents: number) => t('settings_folders_exclude_cost', { paths, documents });
    expect(en(1, 1)).toBe(
      'As of now: on the next scan the index loses 1 file from this folder, '
      + 'and 1 document stops being findable: no other path names it.');
    expect(en(2, 2)).toBe(
      'As of now: on the next scan the index loses 2 files from this folder, '
      + 'and 2 documents stop being findable: no other path names them.');
    expect(en(5, 0)).toBe(
      'As of now: on the next scan the index loses 5 files from this folder, '
      + 'and no document stops being findable — each is also indexed under another path.');
  });

  it('every catalog value is non-empty in both locales', () => {
    for (const loc of ['uk', 'en'] as const) {
      for (const key of Object.keys(messages[loc]) as Key[]) {
        expect(messages[loc][key].length, `${loc}.${key} is empty`).toBeGreaterThan(0);
      }
    }
  });
});

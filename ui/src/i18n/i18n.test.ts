import { describe, it, expect } from 'vitest';
import { t, setLocale } from './index';
import { messages, type Key } from './catalog';

describe('i18n', () => {
  it('returns the string for the active locale', () => {
    setLocale('en'); expect(t('toolbar_settings')).toBe('Settings');
    setLocale('uk'); expect(t('toolbar_settings')).toBe('Налаштування');
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
  // 5 reaches `many` — three states a person can actually get to. `Passages.svelte`
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

  // 1/21 reach `one`, 2/22 `few`, 5 `many`: the arm reads `% 10` and `% 100`.
  it('the exclusion-cost sentence counts files through CLDR plural, in both locales', () => {
    setLocale('uk');
    const uk = (paths: number) => t('settings_folders_exclude_cost', { paths });
    expect(uk(1)).toBe('1 файл із цієї теки зникне з пошуку після наступного сканування.');
    expect(uk(2)).toBe('2 файли із цієї теки зникнуть з пошуку після наступного сканування.');
    expect(uk(5)).toBe('5 файлів із цієї теки зникнуть з пошуку після наступного сканування.');
    expect(uk(21)).toBe('21 файл із цієї теки зникне з пошуку після наступного сканування.');
    expect(uk(22)).toBe('22 файли із цієї теки зникнуть з пошуку після наступного сканування.');

    setLocale('en');
    const en = (paths: number) => t('settings_folders_exclude_cost', { paths });
    expect(en(1)).toBe('1 file from this folder will disappear from search after the next scan.');
    expect(en(2)).toBe('2 files from this folder will disappear from search after the next scan.');
  });

  // The verb sits inside the plural arm, so `1 file will` / `2 files will` and
  // the Ukrainian `зникне` / `зникнуть` agree with the count.
  it('the mask add-cost sentence counts files through CLDR plural, in both locales', () => {
    setLocale('uk');
    const uk = (paths: number) => t('settings_masks_add_cost', { paths });
    expect(uk(1)).toBe('1 файл зникне з пошуку після наступного сканування.');
    expect(uk(2)).toBe('2 файли зникнуть з пошуку після наступного сканування.');
    expect(uk(5)).toBe('5 файлів зникнуть з пошуку після наступного сканування.');
    expect(uk(21)).toBe('21 файл зникне з пошуку після наступного сканування.');

    setLocale('en');
    const en = (paths: number) => t('settings_masks_add_cost', { paths });
    expect(en(1)).toBe('1 file will disappear from search after the next scan.');
    expect(en(2)).toBe('2 files will disappear from search after the next scan.');
  });

  // 🔴 Fix round 4, F2's disclosed half. `?` is NOT refused by `validate_mask`,
  // and the reason it is not is that its breakage is a property of the NAME
  // rather than of the mask: `?.txt` fails to match `й.txt` because `й` is two
  // bytes, in a mask that is all ASCII. Refusing every `?` would cut through
  // the healthy case, so the explainer says it instead — one clause, both
  // locales, and both examples measured through `MaskLayer::matches` rather
  // than reasoned from the regex.
  it('the mask explainer discloses that `?` counts bytes, in both locales', () => {
    setLocale('uk');
    expect(t('settings_masks_explainer')).toContain(
      'А «?» замінює один байт, а не одну літеру, тож для літер поза латиницею його треба '
      + 'ставити кілька: «?.txt» не збігається з «й.txt», а «??.txt» збігається.');
    setLocale('en');
    expect(t('settings_masks_explainer')).toContain(
      'And ? stands for a single byte rather than a single letter, so a letter outside the basic'
      + ' Latin alphabet needs more than one of them: ?.txt does not match й.txt, and ??.txt'
      + ' does.');
    // The other direction: the clause is an ADDITION, not a replacement — the
    // case and normalisation halves the live run confirmed must still be there.
    setLocale('uk');
    expect(t('settings_masks_explainer')).toContain('«*.PDF» і «*.pdf» — це одне й те саме правило');
    setLocale('en');
    expect(t('settings_masks_explainer')).toContain('*.PDF and *.pdf are one and the same rule');
  });

  // Fix round 1. PR 8a Task 5 added a four-arm Ukrainian plural and the only
  // counts anything reached were 1 and 2 (`Folders.test.ts:629`, `:1174`) —
  // which `one` and `few` alone answer, so `many` and `other` crossed
  // untested. `unnameable` is a count of directory entries whose names are not
  // valid UTF-8 (`tree.rs`), and nothing bounds it, so 5 and 21 are states a
  // person can reach. Pinned at the same counts the plurals above use: 5
  // reaches `many` and 21 reaches `one`, which is what shows the arm reads
  // `count % 10` and `% 100` rather than "is this 1 or 2".
  it('the unnameable-subfolders count reaches every Ukrainian plural arm', () => {
    setLocale('uk');
    const uk = (n: number) => t('settings_subfolders_unnameable', { count: n });
    expect(uk(1)).toBe('1 запис не показано: його назву не вдалося прочитати як текст.');
    expect(uk(2)).toBe('2 записи не показано: їхні назви не вдалося прочитати як текст.');
    expect(uk(5)).toBe('5 записів не показано: їхні назви не вдалося прочитати як текст.');
    expect(uk(11)).toBe('11 записів не показано: їхні назви не вдалося прочитати як текст.'); // teen → many
    expect(uk(21)).toBe('21 запис не показано: його назву не вдалося прочитати як текст.');

    setLocale('en');
    const en = (n: number) => t('settings_subfolders_unnameable', { count: n });
    expect(en(1)).toBe('1 item is not listed: its name could not be read as text.');
    expect(en(5)).toBe('5 items are not listed: their names could not be read as text.');
  });

  // Task 9. Two substitutions in one sentence, and the plural is over only one
  // of them: `#` counts the files the folder holds, and `{path}` has to come
  // out inside EVERY arm — a placeholder left out of the `few` branch is a
  // sentence that names no folder at exactly the counts the component reaches
  // most often, and no component fixture can see the arms it does not hit.
  // Pinned at 1 / 3 / 5 in Ukrainian (`one`, `few`, `many`) and 1 / 3 in
  // English, the counts the removal question's own tests use.
  it('the folder-removal question counts the files it drops in every plural arm, in both locales', () => {
    setLocale('uk');
    const uk = (files: number) => t('settings_folders_confirm_remove', { path: '/synthetic/root', files });
    expect(uk(1)).toBe('Видалити теку /synthetic/root з індексу? 1 файл цієї теки зникне з пошуку.');
    expect(uk(3)).toBe('Видалити теку /synthetic/root з індексу? 3 файли цієї теки зникнуть з пошуку.');
    expect(uk(5)).toBe('Видалити теку /synthetic/root з індексу? 5 файлів цієї теки зникнуть з пошуку.');

    setLocale('en');
    const en = (files: number) => t('settings_folders_confirm_remove', { path: '/synthetic/root', files });
    expect(en(1)).toBe(
      'Remove folder /synthetic/root from the index? 1 file from this folder will disappear from search.');
    expect(en(3)).toBe(
      'Remove folder /synthetic/root from the index? 3 files from this folder will disappear from search.');
  });

  it('every catalog value is non-empty in both locales', () => {
    for (const loc of ['uk', 'en'] as const) {
      for (const key of Object.keys(messages[loc]) as Key[]) {
        expect(messages[loc][key].length, `${loc}.${key} is empty`).toBeGreaterThan(0);
      }
    }
  });
});

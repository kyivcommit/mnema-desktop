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
      + 'а 1 документ більше не знайдеться: інші шляхи на нього не ведуть.'
      + ' Сканування може прибрати більше: файли, які так і не проіндексувалися, тут не враховані.');
    expect(uk(2, 2)).toBe(
      'Станом на зараз: при наступному скануванні індекс втратить 2 файли із цієї теки, '
      + 'а 2 документи більше не знайдуться: інші шляхи на них не ведуть.'
      + ' Сканування може прибрати більше: файли, які так і не проіндексувалися, тут не враховані.');
    expect(uk(5, 5)).toBe(
      'Станом на зараз: при наступному скануванні індекс втратить 5 файлів із цієї теки, '
      + 'а 5 документів більше не знайдуться: інші шляхи на них не ведуть.'
      + ' Сканування може прибрати більше: файли, які так і не проіндексувалися, тут не враховані.');
    expect(uk(21, 21)).toBe(
      'Станом на зараз: при наступному скануванні індекс втратить 21 файл із цієї теки, '
      + 'а 21 документ більше не знайдеться: інші шляхи на нього не ведуть.'
      + ' Сканування може прибрати більше: файли, які так і не проіндексувалися, тут не враховані.');
    expect(uk(22, 22)).toBe(
      'Станом на зараз: при наступному скануванні індекс втратить 22 файли із цієї теки, '
      + 'а 22 документи більше не знайдуться: інші шляхи на них не ведуть.'
      + ' Сканування може прибрати більше: файли, які так і не проіндексувалися, тут не враховані.');
    // The `=0` arm for `documents`, against a `paths` count that is itself
    // non-zero — the state Folders.svelte review comment "🔴 ДВА числа..."
    // exists for: a path is lost and no document is.
    expect(uk(2, 0)).toBe(
      'Станом на зараз: при наступному скануванні індекс втратить 2 файли із цієї теки, '
      + 'а жоден документ не перестане знаходитися — кожен із них проіндексовано ще й за іншим шляхом.'
      + ' Сканування може прибрати більше: файли, які так і не проіндексувалися, тут не враховані.');

    setLocale('en');
    const en = (paths: number, documents: number) => t('settings_folders_exclude_cost', { paths, documents });
    expect(en(1, 1)).toBe(
      'As of now: on the next scan the index loses 1 file from this folder, '
      + 'and 1 document stops being findable: no other path names it.'
      + ' The scan can remove more than that: files that never finished indexing are not counted here.');
    expect(en(2, 2)).toBe(
      'As of now: on the next scan the index loses 2 files from this folder, '
      + 'and 2 documents stop being findable: no other path names them.'
      + ' The scan can remove more than that: files that never finished indexing are not counted here.');
    expect(en(5, 0)).toBe(
      'As of now: on the next scan the index loses 5 files from this folder, '
      + 'and no document stops being findable — each is also indexed under another path.'
      + ' The scan can remove more than that: files that never finished indexing are not counted here.');
  });

  // Task 11 fix round 1, F2. `settings_masks_add_cost`'s English string put the
  // verb OUTSIDE the `paths` plural ("... already indexed match this mask"),
  // fixed at the plural form while `# file` was not — so at `paths: 1` the
  // sentence read "1 file already indexed match this mask". It survived
  // because no component fixture ever rendered `paths: 1`
  // (`Masks.test.ts` reaches 4 and 0 only). Pinned here at the same five counts
  // `settings_folders_exclude_cost` above uses, for the same reason the
  // sibling's comment gives: the component only ever reaches 1 and 2.
  // `uk` was already correct — its plural already carries the verb form — and
  // is pinned here too so a future edit cannot reintroduce the English bug's
  // shape on this side.
  it('the mask add-cost sentence counts both paths and documents through CLDR plural, in both locales', () => {
    setLocale('uk');
    const uk = (paths: number, documents: number) => t('settings_masks_add_cost', { paths, documents });
    expect(uk(1, 1)).toBe(
      'Станом на зараз ця маска забирає 1 файл понад те, що вже забирають ваші '
      + 'правила, і 1 документ більше не знайдеться: жодного іншого шляху на нього не '
      + 'залишиться. Наступне сканування кожної теки може прибрати і більше, і менше: файли, '
      + 'які так і не проіндексувалися, тут не враховані, а файл, який уже виключає '
      + '«.gitignore» у самій теці, тут може бути порахований.');
    expect(uk(2, 2)).toBe(
      'Станом на зараз ця маска забирає 2 файли понад те, що вже забирають ваші '
      + 'правила, і 2 документи більше не знайдуться: жодного іншого шляху на них не '
      + 'залишиться. Наступне сканування кожної теки може прибрати і більше, і менше: файли, '
      + 'які так і не проіндексувалися, тут не враховані, а файл, який уже виключає '
      + '«.gitignore» у самій теці, тут може бути порахований.');
    expect(uk(5, 5)).toBe(
      'Станом на зараз ця маска забирає 5 файлів понад те, що вже забирають ваші '
      + 'правила, і 5 документів більше не знайдуться: жодного іншого шляху на них не '
      + 'залишиться. Наступне сканування кожної теки може прибрати і більше, і менше: файли, '
      + 'які так і не проіндексувалися, тут не враховані, а файл, який уже виключає '
      + '«.gitignore» у самій теці, тут може бути порахований.');
    expect(uk(21, 21)).toBe(
      'Станом на зараз ця маска забирає 21 файл понад те, що вже забирають ваші '
      + 'правила, і 21 документ більше не знайдеться: жодного іншого шляху на нього не '
      + 'залишиться. Наступне сканування кожної теки може прибрати і більше, і менше: файли, '
      + 'які так і не проіндексувалися, тут не враховані, а файл, який уже виключає '
      + '«.gitignore» у самій теці, тут може бути порахований.');
    expect(uk(22, 22)).toBe(
      'Станом на зараз ця маска забирає 22 файли понад те, що вже забирають ваші '
      + 'правила, і 22 документи більше не знайдуться: жодного іншого шляху на них не '
      + 'залишиться. Наступне сканування кожної теки може прибрати і більше, і менше: файли, '
      + 'які так і не проіндексувалися, тут не враховані, а файл, який уже виключає '
      + '«.gitignore» у самій теці, тут може бути порахований.');
    // 🔴 Fix round 7, F1, owner's ruling. The `=0` arm for `documents`, against a
    // non-zero `paths`, is now EMPTY: at zero the sentence is the file count and
    // the two-way hedge and says nothing about documents at all. `mask_preview`
    // counts a difference between two rule sets, and a zero from it cannot carry
    // a claim about the world — an in-tree `.gitignore` (outside both sets) or a
    // document that never reached `status = 'indexed'` takes the last path this
    // count says is kept. The `, і` moved INSIDE the non-zero arms so the empty
    // arm leaves a whole sentence rather than a dangling conjunction.
    //
    // The `not.toContain` runs before the `toBe` deliberately: it is the weaker
    // assertion, but a re-added clause in ANY wording fails it first and names
    // the property, where the `toBe` would only print a paragraph diff.
    expect(uk(2, 0)).not.toContain('документ');
    expect(uk(2, 0)).toBe(
      'Станом на зараз ця маска забирає 2 файли понад те, що вже забирають ваші '
      + 'правила. Наступне сканування кожної теки може '
      + 'прибрати і більше, і менше: файли, які так і не проіндексувалися, тут не враховані, '
      + 'а файл, який уже виключає «.gitignore» у самій теці, тут може бути порахований.');

    setLocale('en');
    const en = (paths: number, documents: number) => t('settings_masks_add_cost', { paths, documents });
    // The count this bug actually shipped at: `paths: 1` reaches the `one` arm,
    // and the fix moves the verb ("matches") inside it.
    expect(en(1, 1)).toBe(
      'As of now this mask takes 1 file beyond what your rules already take, and 1'
      + ' document stops being findable: no other path will be left naming it. The next scan of'
      + ' each folder can remove more than that or fewer: files that never finished indexing'
      + ' are not counted here, and a file a .gitignore in the folder itself already excludes'
      + ' may be counted here.');
    expect(en(2, 2)).toBe(
      'As of now this mask takes 2 files beyond what your rules already take, and 2'
      + ' documents stop being findable: no other path will be left naming them. The next scan of'
      + ' each folder can remove more than that or fewer: files that never finished indexing'
      + ' are not counted here, and a file a .gitignore in the folder itself already excludes'
      + ' may be counted here.');
    // The same deletion, the English half — pinned in both locales because the
    // clause was written in both and removed from both.
    expect(en(5, 0)).not.toContain('document');
    expect(en(5, 0)).toBe(
      'As of now this mask takes 5 files beyond what your rules already take.'
      + ' The next scan of each folder can remove more than that or fewer:'
      + ' files that never finished indexing are not counted here, and a file a .gitignore in'
      + ' the folder itself already excludes may be counted here.');

    // `settings_masks_add_cost_none` carries no plural argument — a fixed
    // sentence for the `paths === 0` arm the component picks itself
    // (`Masks.svelte`, F2's sibling finding) — pinned here beside its neighbour
    // rather than left unread by any test in the catalogue.
    setLocale('uk');
    expect(t('settings_masks_add_cost_none')).toBe(
      'Станом на зараз ця маска не забирає нічого понад те, що вже забирають ваші правила. '
      + 'Наступне сканування кожної теки все одно може щось прибрати: файли, які так і не '
      + 'проіндексувалися, тут не враховані.');
    setLocale('en');
    expect(t('settings_masks_add_cost_none')).toBe(
      'As of now this mask takes nothing beyond what your rules already take. The next scan of'
      + ' each folder can still remove files: those that never finished indexing are not counted'
      + ' here.');
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
    expect(uk(1)).toBe('1 підтеку не показано: її назву не вдалося прочитати як текст.');
    expect(uk(2)).toBe('2 підтеки не показано: їхні назви не вдалося прочитати як текст.');
    expect(uk(5)).toBe('5 підтек не показано: їхні назви не вдалося прочитати як текст.');
    expect(uk(11)).toBe('11 підтек не показано: їхні назви не вдалося прочитати як текст.'); // teen → many
    expect(uk(21)).toBe('21 підтеку не показано: її назву не вдалося прочитати як текст.');

    setLocale('en');
    const en = (n: number) => t('settings_subfolders_unnameable', { count: n });
    expect(en(1)).toBe('1 subfolder is not listed: its name could not be read as text.');
    expect(en(5)).toBe('5 subfolders are not listed: their names could not be read as text.');
  });

  it('every catalog value is non-empty in both locales', () => {
    for (const loc of ['uk', 'en'] as const) {
      for (const key of Object.keys(messages[loc]) as Key[]) {
        expect(messages[loc][key].length, `${loc}.${key} is empty`).toBeGreaterThan(0);
      }
    }
  });
});

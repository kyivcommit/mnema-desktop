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

  it('every catalog value is non-empty in both locales', () => {
    for (const loc of ['uk', 'en'] as const) {
      for (const key of Object.keys(messages[loc]) as Key[]) {
        expect(messages[loc][key].length, `${loc}.${key} is empty`).toBeGreaterThan(0);
      }
    }
  });
});

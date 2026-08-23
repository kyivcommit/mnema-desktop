import { describe, it, expect } from 'vitest';
import { t, setLocale } from './index';

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
});

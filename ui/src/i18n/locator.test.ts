import { describe, it, expect } from 'vitest';
import { setLocale } from './index';
import { formatLocator } from './locator';

describe('locator', () => {
  it('renders all forms in UK and EN', () => {
    setLocale('uk');
    expect(formatLocator({ kind: 'page', number: 3 })).toBe('с. 3');
    expect(formatLocator({ kind: 'line', start: 7, end: 7 })).toBe('рядок 7');
    expect(formatLocator({ kind: 'line', start: 412, end: 427 })).toBe('рядки 412–427');
    expect(formatLocator({ kind: 'sheet_rows', sheet: 'Кошторис', start: 14, end: 14 })).toBe('аркуш Кошторис, рядок 14');
    expect(formatLocator({ kind: 'section', title: 'Розділ 3' })).toBe('Розділ 3');
    expect(formatLocator({ kind: 'none' })).toBe('');
    setLocale('en');
    expect(formatLocator({ kind: 'page', number: 3 })).toBe('p. 3');
    expect(formatLocator({ kind: 'line', start: 7, end: 7 })).toBe('line 7');
    expect(formatLocator({ kind: 'sheet_rows', sheet: 'Budget', start: 2, end: 5 })).toBe('sheet Budget, rows 2–5');
  });
});

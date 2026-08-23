import { describe, it, expect } from 'vitest';
import { setLocale } from './index';
import { refusalText } from './refusal';

describe('refusal', () => {
  it('maps every refusal kind in both languages', () => {
    setLocale('uk');
    expect(refusalText('noCandidates')).toBe('Нічого не знайдено за цим запитом.');
    expect(refusalText('emptyCompletion')).toBe('Модель не повернула відповіді.');
    setLocale('en');
    expect(refusalText('noCandidates')).toBe('Nothing was found for this query.');
    expect(refusalText('emptyCompletion')).toBe('The model returned no answer.');
  });
});

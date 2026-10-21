import { describe, it, expect } from 'vitest';
import { setLocale } from './index';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { refusalText, type RefusalKind } from './refusal';
import { camelOf, rustEnumVariants } from '../lib/rust-enum';

const BRIDGE_RS = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), '../../../src-tauri/src/bridge.rs'),
  'utf8',
);
// Typed as a total record, so a TS-side kind added to the union without a row
// here does not compile.
const KINDS: Record<RefusalKind, true> = { noCandidates: true, emptyCompletion: true, noFoldersSearched: true };


describe('refusal', () => {
  it('maps every refusal kind in both languages', () => {
    setLocale('uk');
    expect(refusalText('noCandidates')).toBe('Нічого не знайдено за цим запитом.');
    expect(refusalText('emptyCompletion')).toBe('Модель не повернула відповіді.');
    expect(refusalText('noFoldersSearched')).toBe('Пошук вимкнено для всіх тек. Увімкніть хоча б одну в дереві файлів.');
    setLocale('en');
    expect(refusalText('noCandidates')).toBe('Nothing was found for this query.');
    expect(refusalText('emptyCompletion')).toBe('The model returned no answer.');
    expect(refusalText('noFoldersSearched')).toBe('Search is off for every folder. Turn at least one on in the file tree.');
  });

  it('names exactly the variants Rust sends, and every one has text', () => {
    expect(Object.keys(KINDS).sort()).toEqual(rustEnumVariants(BRIDGE_RS, 'RefusalKind').map(camelOf).sort());
    for (const k of Object.keys(KINDS) as RefusalKind[]) expect(refusalText(k)).not.toBe('');
  });
});

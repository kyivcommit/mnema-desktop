import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}

describe('Svelte hardcode guard', () => {
  it('no Cyrillic literals outside src/i18n', () => {
    const root = join(dirname(fileURLToPath(import.meta.url)), '..'); // ui/src (ESM-safe, no __dirname)
    const offenders = walk(root)
      .filter((p) => /\.(ts|svelte)$/.test(p) && !p.includes(join('src', 'i18n')) && !p.endsWith('.test.ts'))
      .filter((p) => /[Ѐ-ӿ]/.test(readFileSync(p, 'utf8')));
    expect(offenders).toEqual([]);
  });
});

import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const CYRILLIC = /[Ѐ-ӿ]/;

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}

describe('Svelte hardcode guard', () => {
  const srcRoot = join(dirname(fileURLToPath(import.meta.url)), '..'); // ui/src (ESM-safe, no __dirname)
  const uiRoot = join(srcRoot, '..'); // ui/ — the Vite entry HTML shells live here, one level above src/

  it('no Cyrillic literals outside src/i18n', () => {
    const offenders = walk(srcRoot)
      .filter((p) => /\.(ts|svelte)$/.test(p) && !p.includes(join('src', 'i18n')) && !p.endsWith('.test.ts'))
      .filter((p) => CYRILLIC.test(readFileSync(p, 'utf8')));
    expect(offenders).toEqual([]);
  });

  it('no Cyrillic literals in the top-level HTML shells', () => {
    // Non-recursive on purpose: readdirSync(uiRoot) lists top-level entries only, so filtering by
    // extension never descends into ui/node_modules or ui/dist.
    const offenders = readdirSync(uiRoot)
      .filter((f) => f.endsWith('.html'))
      .map((f) => join(uiRoot, f))
      .filter((p) => CYRILLIC.test(readFileSync(p, 'utf8')));
    expect(offenders).toEqual([]);
  });
});

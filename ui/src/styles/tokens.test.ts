// The design tokens are the ONE place both themes are written down, so the
// thing that can silently go wrong is the two themes drifting apart: a token
// added to the light block and forgotten in a dark one renders as `initial`
// in dark mode — invisible text on a dark ground — and no component test can
// see it, because vitest never computes styles. This file reads the
// stylesheet as text and holds the two themes to each other.
//
// The parser below is deliberately tiny: it understands `selector { decls }`
// with one level of nesting (the media query) and nothing else, which is all
// tokens.css is allowed to contain. If tokens.css grows past that, this file
// fails to find its blocks and says so, rather than reading past them.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const HERE = dirname(fileURLToPath(import.meta.url)); // ui/src/styles
const SRC = join(HERE, '..'); // ui/src
const TOKENS_PATH = join(HERE, 'tokens.css');

// Tokens that are the SAME in both themes on purpose. Anything else that is
// identical in light and dark is a colour somebody forgot to theme.
const THEME_INVARIANT = new Set(['--sans', '--serif', '--mono']);

type Rule = { selector: string; body: string; children: Rule[] };

function stripComments(css: string): string {
  return css.replace(/\/\*[\s\S]*?\*\//g, '');
}

// Splits `css` into top-level rules by brace matching. A rule whose body
// contains `{` (the media query) gets its inner rules parsed as children.
function parseRules(css: string): Rule[] {
  const rules: Rule[] = [];
  let i = 0;
  for (;;) {
    const open = css.indexOf('{', i);
    if (open < 0) break;
    const selector = css.slice(i, open).trim().replace(/\s+/g, ' ');
    let depth = 1;
    let j = open + 1;
    while (j < css.length && depth > 0) {
      if (css[j] === '{') depth += 1;
      else if (css[j] === '}') depth -= 1;
      j += 1;
    }
    if (depth !== 0) throw new Error(`tokens.css: unbalanced braces after "${selector}"`);
    const body = css.slice(open + 1, j - 1);
    rules.push({ selector, body, children: body.includes('{') ? parseRules(body) : [] });
    i = j;
  }
  return rules;
}

// `--name: value` pairs of ONE block body (not its children). Declarations
// that are not custom properties (`color-scheme`) are skipped on purpose.
function customProperties(body: string): Map<string, string> {
  const out = new Map<string, string>();
  for (const decl of body.split(';')) {
    const colon = decl.indexOf(':');
    if (colon < 0) continue;
    const name = decl.slice(0, colon).trim();
    if (!name.startsWith('--')) continue;
    out.set(name, decl.slice(colon + 1).trim().replace(/\s+/g, ' '));
  }
  return out;
}

function findRule(rules: Rule[], selector: string): Rule {
  const hit = rules.find((r) => r.selector === selector);
  if (!hit) {
    throw new Error(
      `tokens.css: no block with selector "${selector}"; found: ${rules.map((r) => r.selector).join(' | ')}`,
    );
  }
  return hit;
}

function walk(dir: string, exts: string[]): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return walk(path, exts);
    return exts.some((e) => name.endsWith(e)) ? [path] : [];
  });
}

function loadThemes() {
  const css = stripComments(readFileSync(TOKENS_PATH, 'utf8'));
  const rules = parseRules(css);
  const light = customProperties(findRule(rules, ':root').body);
  const media = rules.find(
    (r) => r.selector.startsWith('@media') && /prefers-color-scheme:\s*dark/.test(r.selector),
  );
  if (!media) throw new Error('tokens.css: no @media (prefers-color-scheme: dark) block');
  const mediaDark = customProperties(findRule(media.children, ':root:not([data-theme="light"])').body);
  const attrDark = customProperties(findRule(rules, ':root[data-theme="dark"]').body);
  return { light, mediaDark, attrDark };
}

const sortedNames = (m: Map<string, string>) => [...m.keys()].sort();

describe('tokens.css holds its two themes to each other', () => {
  it('declares the same token names in the light block and in both dark blocks', () => {
    const { light, mediaDark, attrDark } = loadThemes();
    // Set equality in both directions, so a token missing from EITHER side
    // fails — an `expect(dark).toContain` per light token would pass with a
    // dark-only stray.
    expect(sortedNames(mediaDark)).toEqual(sortedNames(light));
    expect(sortedNames(attrDark)).toEqual(sortedNames(light));
    expect(light.size).toBeGreaterThan(0);
  });

  it('gives the media dark block and the attribute dark block identical values', () => {
    const { mediaDark, attrDark } = loadThemes();
    // Compared as objects so a mismatch names the token, not an index.
    expect(Object.fromEntries(attrDark)).toEqual(Object.fromEntries(mediaDark));
  });

  it('themes every colour token — a value identical in light and dark is a forgotten one', () => {
    const { light, mediaDark } = loadThemes();
    const unthemed = [...light.keys()].filter(
      (name) => !THEME_INVARIANT.has(name) && light.get(name) === mediaDark.get(name),
    );
    expect(unthemed).toEqual([]);
  });
});

describe('the stylesheets stay inside the app', () => {
  it('reach no network resource from any CSS file under ui/src', () => {
    const offenders: string[] = [];
    for (const file of walk(SRC, ['.css'])) {
      const css = stripComments(readFileSync(file, 'utf8'));
      css.split('\n').forEach((line, idx) => {
        if (/url\(\s*['"]?(https?:)?\/\//.test(line) || /@import\s+(url\()?\s*['"]?https?:/.test(line) || /@import\s+url\(/.test(line)) {
          offenders.push(`${file}:${idx + 1}: ${line.trim()}`);
        }
      });
    }
    expect(offenders).toEqual([]);
  });

  it('use only tokens that tokens.css declares', () => {
    const { light } = loadThemes();
    const undeclared: string[] = [];
    for (const file of walk(SRC, ['.css', '.svelte'])) {
      const text = stripComments(readFileSync(file, 'utf8'));
      for (const m of text.matchAll(/var\(\s*(--[\w-]+)/g)) {
        if (!light.has(m[1])) undeclared.push(`${file}: ${m[1]}`);
      }
    }
    expect(undeclared).toEqual([]);
  });
});

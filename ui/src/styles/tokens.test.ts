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

// Same as stripComments, but a comment's characters (its newlines included)
// are replaced by blanks instead of removed, so a line number counted after
// this call still points at the real file. Used only where a failure
// message reports a line number — parsing never reports one, and keeps
// plain stripComments.
function blankComments(css: string): string {
  return css.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '));
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

// Splits ONE block body into `name: value` declarations, respecting quotes
// and parens so a `;` inside a quoted string or a data: URI does not end a
// declaration early. tokens.css holds no such value today, but a later
// token (an inline icon, say) could easily introduce one, and a naive
// `.split(';')` would silently start comparing the wrong halves of two
// declarations against each other.
function splitDeclarations(body: string): string[] {
  const decls: string[] = [];
  let current = '';
  let quote: '"' | "'" | null = null;
  let depth = 0;
  for (const ch of body) {
    if (quote) {
      current += ch;
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      current += ch;
      continue;
    }
    if (ch === '(') depth += 1;
    if (ch === ')') depth -= 1;
    if (ch === ';' && depth === 0) {
      decls.push(current);
      current = '';
      continue;
    }
    current += ch;
  }
  if (current.trim()) decls.push(current);
  return decls;
}

// `--name: value` pairs of ONE block body (not its children). Declarations
// that are not custom properties (`color-scheme`) are skipped on purpose —
// see `declaration` below for those.
function customProperties(body: string): Map<string, string> {
  const out = new Map<string, string>();
  for (const decl of splitDeclarations(body)) {
    const colon = decl.indexOf(':');
    if (colon < 0) continue;
    const name = decl.slice(0, colon).trim();
    if (!name.startsWith('--')) continue;
    out.set(name, decl.slice(colon + 1).trim().replace(/\s+/g, ' '));
  }
  return out;
}

// The value of one plain (non-custom-property) declaration in a block body,
// e.g. `color-scheme`. undefined if the block does not declare it.
function declaration(body: string, name: string): string | undefined {
  for (const decl of splitDeclarations(body)) {
    const colon = decl.indexOf(':');
    if (colon < 0) continue;
    if (decl.slice(0, colon).trim() !== name) continue;
    return decl.slice(colon + 1).trim().replace(/\s+/g, ' ');
  }
  return undefined;
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

function loadRules(): Rule[] {
  return parseRules(stripComments(readFileSync(TOKENS_PATH, 'utf8')));
}

function loadThemes() {
  const rules = loadRules();
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

// No `url(...)` may resolve outside the app: an explicit `http(s)` target,
// or a scheme-relative (`//`) one, which fetches under this app's own
// origin exactly like an explicit `https:` target would; `data:` URIs and
// relative paths never match, on purpose. No `@import` at all, of any
// shape, local or remote: this project composes stylesheets only in the two
// `main.ts` entry points (tokens.css, then base.css, then a window-specific
// file), so a CSS `@import` would be a second, unaudited composition path,
// and a rule with no scheme to parse cannot narrow itself again.
function reachesNetwork(line: string): boolean {
  return /url\(\s*['"]?(?:https?:)?\/\//i.test(line) || /@import\b/i.test(line);
}

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

  it('keeps color-scheme in step with the theme blocks', () => {
    // customProperties deliberately ignores `color-scheme` (it is not a
    // custom property), which left the whole non-token half of the theme
    // mechanism unguarded: the three declarations, or the entire explicit
    // `:root[data-theme="light"]` block that carries one of them, could be
    // deleted without a single colour value changing — the exact "silently
    // vanish" shape this project asks about.
    const rules = loadRules();
    const light = findRule(rules, ':root');
    const attrDark = findRule(rules, ':root[data-theme="dark"]');
    const attrLight = findRule(rules, ':root[data-theme="light"]');
    expect(declaration(light.body, 'color-scheme')).toBe('light dark');
    expect(declaration(attrDark.body, 'color-scheme')).toBe('dark');
    expect(declaration(attrLight.body, 'color-scheme')).toBe('light');
  });
});

describe('the stylesheets stay inside the app', () => {
  it('reach no network resource from any CSS file under ui/src', () => {
    // The predicate is checked against its own small table first, so a
    // change that breaks a known shape is caught right here rather than
    // only if some future file happens to contain that exact shape.
    const table: [string, boolean][] = [
      ['url(https://x)', true],
      ['url( "//x" )', true],
      ['@import url("https://x")', true],
      ['@import "//x"', true],
      ["@import 'http://x'", true],
      ['@import url("./launcher.css")', true],
      ["@import './a.css'", true],
      ['url(data:font/woff2;base64,AA)', false],
      ['url(./fonts/a.woff2)', false],
    ];
    for (const [line, expected] of table) {
      expect(reachesNetwork(line), line).toBe(expected);
    }

    // A guard that walks zero files is satisfied by nothing to complain
    // about — assert the walk actually found stylesheets before trusting
    // the empty result below.
    const files = walk(SRC, ['.css']);
    expect(files.length, `no .css files found under ${SRC}`).toBeGreaterThan(0);

    const offenders: string[] = [];
    for (const file of files) {
      const css = blankComments(readFileSync(file, 'utf8'));
      css.split('\n').forEach((line, idx) => {
        if (reachesNetwork(line)) {
          offenders.push(`${file}:${idx + 1}: ${line.trim()}`);
        }
      });
    }
    expect(offenders).toEqual([]);
  });

  it('use only tokens that tokens.css declares', () => {
    const { light } = loadThemes();

    // Same reasoning as above: an empty walk would satisfy this vacuously.
    const files = walk(SRC, ['.css', '.svelte']);
    expect(files.length, `no .css/.svelte files found under ${SRC}`).toBeGreaterThan(0);

    const undeclared: string[] = [];
    for (const file of files) {
      const text = stripComments(readFileSync(file, 'utf8'));
      for (const m of text.matchAll(/var\(\s*(--[\w-]+)/g)) {
        if (!light.has(m[1])) undeclared.push(`${file}: ${m[1]}`);
      }
    }
    expect(undeclared).toEqual([]);
  });
});

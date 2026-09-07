// The design tokens are the ONE place both themes are written down, so the
// thing that can silently go wrong is the two themes drifting apart: a token
// added to the light block and forgotten in a dark one renders as `initial`
// in dark mode — invisible text on a dark ground — and no component test can
// see it, because vitest never computes styles. This file reads the
// stylesheet as text and holds the two themes to each other.
//
// The parser below is deliberately tiny: it understands `selector { decls }`
// with one level of nesting (the media query) and nothing else. `findRule`
// throws if a selector it looks up is missing, or if more than one
// top-level rule shares it (a duplicate `:root`, say, which would otherwise
// win the cascade silently). It does NOT understand any other at-rule
// (`@supports`, `@layer`, …) — those pass through unparsed, folded into
// whichever rule's body text they happen to fall inside.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const HERE = dirname(fileURLToPath(import.meta.url)); // ui/src/styles
const SRC = join(HERE, '..'); // ui/src
const UI_ROOT = join(SRC, '..'); // ui — the two HTML entry points live here, not under src
const TOKENS_PATH = join(HERE, 'tokens.css');

// Tokens that are the SAME in both themes on purpose — enforced by the
// "keeps the theme-invariant font stacks identical" test below. Anything
// else identical in light and dark is a colour somebody forgot to theme.
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

// Keeps only the text inside <style>...</style> tags of a file — a .svelte
// component or an HTML entry point, both hold markup around their styles —
// blanking everything else (markup, script) to spaces while preserving
// newlines — so a line number reported below still points at the real
// file, and script or markup text cannot trigger a CSS-shaped false
// positive (a component that renders the word "@import", say).
function styleBlocksOnly(text: string): string {
  const blank = (s: string) => s.replace(/[^\n]/g, ' ');
  let out = '';
  let i = 0;
  const openTag = /<style\b[^>]*>/gi;
  for (;;) {
    openTag.lastIndex = i;
    const m = openTag.exec(text);
    if (!m) {
      out += blank(text.slice(i));
      break;
    }
    out += blank(text.slice(i, m.index + m[0].length));
    const bodyStart = m.index + m[0].length;
    const closeIdx = text.indexOf('</style>', bodyStart);
    const bodyEnd = closeIdx < 0 ? text.length : closeIdx;
    out += text.slice(bodyStart, bodyEnd);
    i = bodyEnd;
  }
  return out;
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

// Exactly one top-level (or, for a nested lookup, one child) rule may carry
// a given selector. Zero is a parse failure worth naming; more than one is
// a duplicate that would win the cascade silently while every assertion
// here keeps reading the first, so it is rejected rather than resolved.
function findRule(rules: Rule[], selector: string): Rule {
  const hits = rules.filter((r) => r.selector === selector);
  if (hits.length === 0) {
    throw new Error(
      `tokens.css: no block with selector "${selector}"; found: ${rules.map((r) => r.selector).join(' | ')}`,
    );
  }
  if (hits.length > 1) {
    throw new Error(
      `tokens.css: ${hits.length} blocks with selector "${selector}", expected exactly one — a duplicate wins the cascade silently`,
    );
  }
  return hits[0];
}

function walk(dir: string, exts: string[]): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return walk(path, exts);
    return exts.some((e) => name.endsWith(e)) ? [path] : [];
  });
}

// Non-recursive: only files directly inside `dir`. Used for `ui/`'s two HTML
// entry points, so `node_modules/` and `dist/` are never descended into.
function topLevelFiles(dir: string, exts: string[]): string[] {
  return readdirSync(dir)
    .filter((name) => exts.some((e) => name.endsWith(e)))
    .map((name) => join(dir, name))
    .filter((path) => !statSync(path).isDirectory());
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

// 1-indexed line number of the character at `offset` into `text`, and that
// line's own text (trimmed) — both computed from the character offset, not
// from a line already split out by the caller. A construct that spans
// multiple lines is reported at its FIRST line this way, which is also the
// line a regex match's `.index` always points at.
function locate(text: string, offset: number): { line: number; text: string } {
  const before = text.slice(0, offset);
  const line = before.split('\n').length;
  const lineStart = before.lastIndexOf('\n') + 1;
  const lineEnd = text.indexOf('\n', offset);
  return { line, text: text.slice(lineStart, lineEnd < 0 ? text.length : lineEnd).trim() };
}

// Finds every place `text` breaks the two rules a stylesheet's own text
// must obey, because this project composes stylesheets only in the two
// main.ts entry points (tokens.css, then base.css, then a window-specific
// file): no url(...) that resolves outside the app — an explicit http(s)
// target, or a scheme-relative (`//`) one, which fetches under this app's
// own origin exactly like an explicit https: target would (data: URIs and
// relative paths are fine) — and no @import at all, of any shape, local or
// remote, since a CSS @import would be a second, unaudited composition
// path. Scans the WHOLE text at once, never line by line: ordinary
// multi-line formatting can put `url(` on one line and its quoted address
// on the next, and a line-by-line regex would miss the construct entirely.
// Returns each hit's character offset into `text` — `locate` above turns
// that into a line number and the line's own text.
function findForbiddenInStylesheet(text: string): { offset: number; reason: string }[] {
  const hits: { offset: number; reason: string }[] = [];
  for (const m of text.matchAll(/url\(\s*['"]?\s*(?:https?:|\/\/)/gi)) {
    hits.push({ offset: m.index, reason: 'a url() reaching outside the app' });
  }
  for (const m of text.matchAll(/@import\b/gi)) {
    hits.push({ offset: m.index, reason: 'a CSS @import' });
  }
  return hits;
}

// Finds every <link> tag in `text` that is a rel="stylesheet" or
// rel="preconnect" whose href reaches outside the app — the same "reaches
// outside the app" rule above, expressed as a <link> tag instead of CSS.
// Deliberately narrow: only rel="stylesheet" and rel="preconnect" are
// checked, because those are the two forms the mockups' heads actually
// contain and the ones a transcriber would paste; other rels that also
// fetch (icon, preload, prefetch, dns-prefetch, …) are not scanned. The tag
// itself may span multiple lines (one attribute per line is ordinary
// formatting) and `rel`/`href` may be quoted, unquoted, or in either
// order — both are read from the whole matched tag, not from a single line
// of it. Returns each hit's character offset into `text`, same as above.
function findForbiddenInHtml(text: string): { offset: number; reason: string }[] {
  const hits: { offset: number; reason: string }[] = [];
  const relIsStylesheetOrPreconnect =
    /\brel\s*=\s*(?:"(?:stylesheet|preconnect)"|'(?:stylesheet|preconnect)'|(?:stylesheet|preconnect)\b)/i;
  const hrefIsExternal = /\bhref\s*=\s*(?:"\s*(?:https?:|\/\/)|'\s*(?:https?:|\/\/)|(?:https?:|\/\/))/i;
  for (const m of text.matchAll(/<link\b[\s\S]*?>/gi)) {
    const tag = m[0];
    if (relIsStylesheetOrPreconnect.test(tag) && hrefIsExternal.test(tag)) {
      hits.push({ offset: m.index, reason: 'a <link> reaching outside the app' });
    }
  }
  return hits;
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

  it('keeps the theme-invariant font stacks identical in every block', () => {
    // THEME_INVARIANT excuses --sans/--serif/--mono from the "themes every
    // colour token" check above, but exempting a token from "must differ"
    // is not the same as requiring it to "stay the same" — an edit to one
    // block only (the shape PR 10c's font-stack change takes) would pass
    // both of the checks above unnoticed.
    const { light, mediaDark, attrDark } = loadThemes();
    for (const name of THEME_INVARIANT) {
      expect(mediaDark.get(name), name).toBe(light.get(name));
      expect(attrDark.get(name), name).toBe(light.get(name));
    }
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
  it('forbids a url() outside the app and any @import, in every stylesheet, svelte style block, and HTML entry point', () => {
    // Both scanners are checked against their own small table first, so a
    // change that breaks a known shape is caught right here rather than
    // only if some future file happens to contain that exact shape. Two
    // rows in each table are deliberately multi-line — the shape ordinary
    // formatting produces — so the fix cannot regress to "any multi-line
    // text is an offender": one multi-line row is a real offender, the
    // other is a multi-line LOCAL construct that must still be allowed.
    const table: [string, boolean][] = [
      ['url(https://x)', true],
      ['url( "//x" )', true],
      ['@import url("https://x")', true],
      ['@import "//x"', true],
      ["@import 'http://x'", true],
      ['@import url("./launcher.css")', true],
      ["@import './a.css'", true],
      ['.review-probe {\n  background-image: url(\n    "https://example.invalid/review.png"\n  );\n}', true],
      ['url(data:font/woff2;base64,AA)', false],
      ['url(./fonts/a.woff2)', false],
      ['url(\n  "./fonts/a.woff2"\n)', false],
    ];
    for (const [text, expected] of table) {
      expect(findForbiddenInStylesheet(text).length > 0, text).toBe(expected);
    }

    const htmlTable: [string, boolean][] = [
      ['<link rel="stylesheet" href="https://fonts.googleapis.com/css2">', true],
      ['<link rel="preconnect" href="https://fonts.gstatic.com">', true],
      ['<link\n  rel="stylesheet"\n  href="https://example.invalid/review.css"\n>', true],
      ['<link rel="stylesheet" href="/src/x.css">', false],
      ['<link rel="stylesheet"\n href="/src/x.css">', false],
    ];
    for (const [text, expected] of htmlTable) {
      expect(findForbiddenInHtml(text).length > 0, text).toBe(expected);
    }

    // A guard that walks zero files of a kind is satisfied by nothing to
    // complain about — assert each walk actually found something before
    // trusting an empty offenders list.
    const cssFiles = walk(SRC, ['.css']);
    expect(cssFiles.length, `no .css files found under ${SRC}`).toBeGreaterThan(0);
    const svelteFiles = walk(SRC, ['.svelte']);
    expect(svelteFiles.length, `no .svelte files found under ${SRC}`).toBeGreaterThan(0);
    const htmlFiles = topLevelFiles(UI_ROOT, ['.html']);
    expect(htmlFiles.length, `no .html files found directly under ${UI_ROOT}`).toBeGreaterThan(0);

    const offenders: string[] = [];

    for (const file of cssFiles) {
      const text = blankComments(readFileSync(file, 'utf8'));
      for (const { offset, reason } of findForbiddenInStylesheet(text)) {
        const { line, text: lineText } = locate(text, offset);
        offenders.push(`${file}:${line}: ${reason}: ${lineText}`);
      }
    }

    for (const file of svelteFiles) {
      const text = blankComments(styleBlocksOnly(readFileSync(file, 'utf8')));
      for (const { offset, reason } of findForbiddenInStylesheet(text)) {
        const { line, text: lineText } = locate(text, offset);
        offenders.push(`${file}:${line}: ${reason}: ${lineText}`);
      }
    }

    for (const file of htmlFiles) {
      const raw = readFileSync(file, 'utf8');
      for (const { offset, reason } of findForbiddenInHtml(raw)) {
        const { line, text: lineText } = locate(raw, offset);
        offenders.push(`${file}:${line}: ${reason}: ${lineText}`);
      }
      // The mockups this project transcribes from are HTML with an inline
      // <style> block as well as a <link>, so the HTML entries get the same
      // CSS-content scan as a .svelte file's <style> block, not only the
      // <link>-tag check above.
      const styleOnly = blankComments(styleBlocksOnly(raw));
      for (const { offset, reason } of findForbiddenInStylesheet(styleOnly)) {
        const { line, text: lineText } = locate(styleOnly, offset);
        offenders.push(`${file}:${line}: ${reason}: ${lineText}`);
      }
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

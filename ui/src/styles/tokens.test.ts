// The design tokens are the ONE place both themes are written down, so the
// thing that can silently go wrong is the two themes drifting apart: a token
// added to the light block and forgotten in a dark one renders as `initial`
// in dark mode — invisible text on a dark ground — and no component test can
// see it, because jsdom hands a var() back as unresolved text, so even a
// computed style cannot see a token resolve to `initial`. This file reads
// the stylesheet as text and holds the two themes to each other.
//
// The parser below is deliberately tiny: it understands `selector { decls }`
// with one level of nesting (the media query) and nothing else. `findRule`
// throws if a selector it looks up is missing, or if more than one
// top-level rule shares it (a duplicate `:root`, say, which would otherwise
// win the cascade silently). It does NOT understand any other at-rule
// (`@supports`, `@layer`, …) — those pass through unparsed, folded into
// whichever rule's body text they happen to fall inside.
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import * as ts from 'typescript';

const HERE = dirname(fileURLToPath(import.meta.url)); // ui/src/styles
const SRC = join(HERE, '..'); // ui/src
const UI_ROOT = join(SRC, '..'); // ui — the two HTML entry points live here, not under src
const TOKENS_PATH = join(HERE, 'tokens.css');
const FONTS_PATH = join(HERE, 'fonts.css');
const FONTS_DIR = join(HERE, 'fonts');
const VITE_CONFIG_PATH = join(UI_ROOT, 'vite.config.ts');
const TAURI_CONF_PATH = join(UI_ROOT, '..', 'src-tauri', 'tauri.conf.json');
const SETTINGS_PATH = join(HERE, 'settings.css');
const SETTINGS_MAIN_PATH = join(SRC, 'settings', 'main.ts');
const LAUNCHER_MAIN_PATH = join(SRC, 'launcher', 'main.ts');

// The stylesheets each window imports, in the order the cascade needs them:
// tokens before anything that reads them, fonts before the stacks are used,
// base before a window's own sheet overrides it. No test mounts through
// main.ts, so a dropped or reordered import is invisible to every other suite.
const WINDOW_STYLESHEETS: [path: string, imports: string[]][] = [
  [SETTINGS_MAIN_PATH, ['../styles/tokens.css', '../styles/fonts.css', '../styles/base.css', '../styles/settings.css']],
  [LAUNCHER_MAIN_PATH, ['../styles/tokens.css', '../styles/fonts.css', '../styles/base.css', '../styles/launcher.css']],
];

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
// e.g. `color-scheme`. undefined if the block does not declare it. The
// first occurrence, same as `declarations` below returns them in order —
// see that function for why a caller checking for a duplicate wants every
// occurrence instead.
function declaration(body: string, name: string): string | undefined {
  return declarations(body, name)[0];
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

function contrast(foreground: string, background: string): number {
  const luminance = (hex: string) => {
    const match = /^#([\da-f]{6})$/i.exec(hex);
    if (!match) throw new Error(`expected #RRGGBB, got ${hex}`);
    return [0, 2, 4].map((offset) => parseInt(match[1].slice(offset, offset + 2), 16) / 255)
      .map((value) => value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4)
      .reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0);
  };
  const [a, b] = [luminance(foreground), luminance(background)];
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

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

// One @font-face block of fonts.css, read as text the same way tokens.css
// is. `ranges` is the parsed unicode-range: inclusive [from, to] code-point
// pairs. Google's sheets use only `U+XXXX` and `U+XXXX-YYYY`; the wildcard
// form (`U+4??`) is rejected rather than guessed at, since fonts.css is
// generated from that source and a wildcard there would be a new shape.
type Face = {
  family: string;
  weight: string;
  style: string;
  display: string | undefined;
  format: string | undefined;
  src: string;
  ranges: [number, number][];
};

function parseUnicodeRange(value: string): [number, number][] {
  return value.split(',').map((part) => {
    const m = /^\s*U\+([0-9A-F]+)(?:-([0-9A-F]+))?\s*$/i.exec(part);
    if (!m) throw new Error(`fonts.css: unicode-range token not understood: "${part.trim()}"`);
    const lo = parseInt(m[1], 16);
    return [lo, m[2] ? parseInt(m[2], 16) : lo];
  });
}

function unquote(s: string): string {
  return s.trim().replace(/^['"]|['"]$/g, '');
}

function loadFaces(): Face[] {
  const rules = parseRules(stripComments(readFileSync(FONTS_PATH, 'utf8')));
  const strays = rules.filter((r) => r.selector !== '@font-face');
  if (strays.length > 0) {
    throw new Error(
      `fonts.css: only @font-face blocks belong here; found: ${strays.map((r) => r.selector).join(' | ')}`,
    );
  }
  return rules.map((r) => {
    const need = (name: string): string => {
      const v = declaration(r.body, name);
      if (v === undefined) throw new Error(`fonts.css: an @font-face block without ${name}: ${r.body.trim()}`);
      return v;
    };
    const src = need('src');
    const url = /url\(\s*['"]?([^'")]+?)['"]?\s*\)/.exec(src);
    if (!url) throw new Error(`fonts.css: src without url(): ${src}`);
    return {
      family: unquote(need('font-family')),
      weight: need('font-weight'),
      style: need('font-style'),
      display: declaration(r.body, 'font-display'),
      format: /format\(\s*['"]?([^'")]+?)['"]?\s*\)/.exec(src)?.[1],
      src: url[1],
      ranges: parseUnicodeRange(need('unicode-range')),
    };
  });
}

const faceKey = (f: Face) => `${f.family} ${f.weight} ${f.style}`;

// The first family of a `font-family` stack, unquoted. `'IBM Plex Sans',
// system-ui, …` → `IBM Plex Sans`.
function stackLeader(stack: string): string {
  return unquote(stack.split(',')[0]);
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

describe('fonts.css bundles the faces the stacks lead with', () => {
  // Every face this sheet promises must be a file the bundle can ship, and
  // every file under fonts/ must be a face the sheet promises: an unnamed
  // file is dead weight nobody will notice, a named file that is missing is
  // a face the browser silently replaces with the next family in the stack.
  it('names a file that exists in every src url, and names every font file', () => {
    const faces = loadFaces();
    expect(faces.length, 'fonts.css declares no @font-face at all').toBeGreaterThan(0);
    const files = walk(FONTS_DIR, ['.woff2']);
    expect(files.length, `no .woff2 files under ${FONTS_DIR}`).toBeGreaterThan(0);

    const referenced = faces.map((f) => join(HERE, f.src));
    const missing = referenced.filter((p) => !existsSync(p));
    expect(missing).toEqual([]);

    const unnamed = files.filter((p) => !referenced.includes(p));
    expect(unnamed).toEqual([]);
  });

  // The stacks in tokens.css are what the interface actually asks for; the
  // sheet is what it can deliver. A family named in one and not the other —
  // a typo, a rename, a face fetched and never wired — falls through to the
  // system stack on every machine that does not happen to have it installed,
  // which is the reason 10a kept the mockup families out of the stacks.
  it('leads every stack in tokens.css with a bundled family, and bundles no family that leads none', () => {
    const { light } = loadThemes();
    const bundled = [...new Set(loadFaces().map((f) => f.family))].sort();
    const leaders = [...THEME_INVARIANT].map((name) => {
      const stack = light.get(name);
      if (stack === undefined) throw new Error(`tokens.css: ${name} missing from :root`);
      return stackLeader(stack);
    });
    for (const [i, name] of [...THEME_INVARIANT].entries()) {
      expect(bundled, `${name} leads with "${leaders[i]}"`).toContain(leaders[i]);
    }
    expect([...new Set(leaders)].sort()).toEqual(bundled);
  });

  // "Latin and Cyrillic" as a property of each face, not as a list of
  // subset names: a face whose ranges skip any of these code points would
  // render that character in the fallback family, mid-word, with no error
  // anywhere. `₴` is the one that lives in cyrillic-ext, not cyrillic; `ł`
  // is latin-ext, the reason that subset is bundled at all.
  it('covers the Ukrainian alphabet, the hryvnia sign and the typographic punctuation in every face', () => {
    const probes = [...'AaЄєІіЇїҐґЯя₴№—…«»ł'];
    const byFace = new Map<string, [number, number][]>();
    for (const f of loadFaces()) {
      byFace.set(faceKey(f), [...(byFace.get(faceKey(f)) ?? []), ...f.ranges]);
    }
    expect(byFace.size, 'no faces parsed').toBeGreaterThan(0);

    const uncovered: string[] = [];
    for (const [key, ranges] of byFace) {
      for (const ch of probes) {
        const cp = ch.codePointAt(0)!;
        if (!ranges.some(([lo, hi]) => lo <= cp && cp <= hi)) {
          uncovered.push(`${key}: '${ch}' U+${cp.toString(16).toUpperCase().padStart(4, '0')}`);
        }
      }
    }
    expect(uncovered).toEqual([]);
  });

  // The files are local: `block` means the text waits the few milliseconds
  // the file takes rather than painting once in the fallback family and
  // once again in the right one. And the format is the one Vite was told
  // to ship, so a stray TTF cannot slip in under a woff2 name.
  it('declares font-display: block and format woff2 on every face', () => {
    const faces = loadFaces();
    expect(faces.length).toBeGreaterThan(0);
    const wrong = faces
      .filter((f) => f.display !== 'block' || f.format !== 'woff2')
      .map((f) => `${faceKey(f)} (${f.src}): display=${f.display} format=${f.format}`);
    expect(wrong).toEqual([]);
  });

  // The woff2 files reach the user inside the executable, and the OFL's
  // condition 2 wants the licence text beside every distributed copy. Vite
  // copies nothing it is not asked to import, so each family's OFL.txt has
  // to be named in bundle.resources of tauri.conf.json — and a family added
  // to fonts/ without that line ships unlicensed with every check green,
  // which is the state PR 10c's first bundle was in. Both directions: every
  // family on disk is named, and every fonts/ resource names a family on
  // disk. scripts/verify-bundle.sh checks the built image; this checks the
  // configuration that produces it, on every `npm test`.
  it('names every font family licence in tauri.conf.json bundle.resources', () => {
    const families = readdirSync(FONTS_DIR).filter((name) => statSync(join(FONTS_DIR, name)).isDirectory());
    expect(families.length, `no family directory under ${FONTS_DIR}`).toBeGreaterThan(0);
    for (const family of families) {
      expect(existsSync(join(FONTS_DIR, family, 'OFL.txt')), `${family}/OFL.txt`).toBe(true);
    }
    const conf = JSON.parse(readFileSync(TAURI_CONF_PATH, 'utf8')) as {
      bundle?: { resources?: Record<string, string> };
    };
    const resources = conf.bundle?.resources ?? {};
    const expected = Object.fromEntries(
      families.map((f) => [`../ui/src/styles/fonts/${f}/OFL.txt`, `fonts/${f}/OFL.txt`]),
    );
    const actual = Object.fromEntries(
      Object.entries(resources).filter(([from]) => from.includes('/styles/fonts/')),
    );
    expect(actual).toEqual(expected);
  });

  // Vite inlines any asset under `build.assetsInlineLimit` (4096 bytes by
  // default) as a data: URI — and the app's CSP, `default-src 'self'`,
  // refuses a data: font. The only trace is a devtools console line, which
  // a shipped app never shows; the text falls back to the next family. A
  // subset can weigh less than that default. This reads the config with
  // TypeScript's own parser rather than importing it (the config uses
  // `__dirname`, which is not reliably present when a test module imports
  // it under ESM) and rather than a regex over the text: a regex was fooled
  // by `/* assetsInlineLimit: 0, */` — the value commented out, the guard
  // green — and `stripComments` above is fooled the other way by the
  // config's own `/**/` glob strings. The build itself is the proof that
  // the value means what it says — `grep -c 'url(data:font'
  // dist/assets/*.css` must print 0.
  it('keeps vite from inlining any asset as a data: URI', () => {
    const source = ts.createSourceFile(
      'vite.config.ts',
      readFileSync(VITE_CONFIG_PATH, 'utf8'),
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TS,
    );
    const propertyName = (p: ts.ObjectLiteralElementLike): string | undefined =>
      p.name && (ts.isIdentifier(p.name) || ts.isStringLiteral(p.name)) ? p.name.text : undefined;
    // `build: { assetsInlineLimit: <initializer> }` — the initializer's
    // source text, or undefined when no such property is declared under
    // `build` (a top-level one, or one inside `server`, would not count).
    let limit: string | undefined;
    const visit = (node: ts.Node) => {
      if (
        ts.isPropertyAssignment(node) &&
        propertyName(node) === 'build' &&
        ts.isObjectLiteralExpression(node.initializer)
      ) {
        for (const p of node.initializer.properties) {
          if (ts.isPropertyAssignment(p) && propertyName(p) === 'assetsInlineLimit') {
            limit = p.initializer.getText(source);
          }
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
    expect(limit, 'ui/vite.config.ts: build.assetsInlineLimit must be declared').toBeDefined();
    expect(limit, 'ui/vite.config.ts: build.assetsInlineLimit must be 0').toBe('0');
  });
});

describe('the stylesheets use what tokens.css declares', () => {
  it('uses AA text colours on launcher surfaces', () => {
    const { light, mediaDark } = loadThemes();
    const launcher = readFileSync(join(HERE, 'launcher.css'), 'utf8');
    expect(launcher).not.toMatch(/var\(\s*--ink-faint\s*\)/);
    for (const [theme, tokens] of [['light', light], ['dark', mediaDark]] as const) {
      for (const [text, background] of [
        ['--ink', '--surface'], ['--ink-soft', '--surface'], ['--cite', '--cite-wash'],
      ]) {
        expect(contrast(tokens.get(text)!, tokens.get(background)!), `${theme}: ${text} on ${background}`)
          .toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  // The other direction of `use only tokens that tokens.css declares`: a
  // token nobody reads is a value that can drift in one theme and never be
  // seen.
  it('uses every token it declares', () => {
    const { light } = loadThemes();
    const files = walk(SRC, ['.css', '.svelte']).filter((f) => f !== TOKENS_PATH && f !== FONTS_PATH);
    expect(files.length, `no .css/.svelte files under ${SRC}`).toBeGreaterThan(0);

    const used = new Set<string>();
    for (const file of files) {
      const text = stripComments(readFileSync(file, 'utf8'));
      for (const m of text.matchAll(/var\(\s*(--[\w-]+)/g)) used.add(m[1]);
    }

    const unused = [...light.keys()].filter((n) => !used.has(n)).sort();
    expect(unused, 'declared in tokens.css, read by no stylesheet').toEqual([]);
  });
});

// Every rule in `css`, media-query children included — but a rule whose
// selector starts with `@` is skipped, for two different reasons. A
// grouping at-rule (`@media`, `@supports`, `@layer { … }`, `@container`)
// holds rules, not declarations: its body is nested rule text, so handing
// it to `fontTriple` would read ".x { font-family" as a property name — the
// container is skipped and its children are checked on their own selectors
// instead. An `@font-face` block is skipped too, but on purpose and for an
// unrelated reason: it declares a face, not a style rule, and fonts.css is
// the only place faces belong (`loadFaces` reads them straight off
// fonts.css, and rejects any other top-level selector there) — one stray in
// a non-fonts sheet is meant to fall through here unchecked, not rejected.
// This is NOT "skip every rule that has children" — CSS nesting can put
// declarations directly on a parent that also has nested children, and that
// parent must still be checked itself.
function fontProblems(
  css: string,
  where: string,
  light: Map<string, string>,
  faces: Face[],
): { problems: string[]; checked: number } {
  const flatten = (rules: Rule[]): Rule[] => rules.flatMap((r) => [r, ...flatten(r.children)]);
  const problems: string[] = [];
  let checked = 0;
  for (const rule of flatten(parseRules(stripComments(css)))) {
    if (rule.selector.startsWith('@')) continue;
    const loc = `${where}: ${rule.selector}`;
    const triple = fontTriple(rule.body);
    if (triple === null) continue;
    if (typeof triple === 'string') {
      problems.push(`${loc}: ${triple}`);
      continue;
    }
    checked += 1;
    const token = /^var\(\s*(--[\w-]+)\s*\)$/.exec(triple.family);
    if (!token || !THEME_INVARIANT.has(token[1])) {
      problems.push(`${loc}: font-family "${triple.family}" is not one of var(--sans|--serif|--mono)`);
      continue;
    }
    const stack = light.get(token[1]);
    if (stack === undefined) throw new Error(`tokens.css: ${token[1]} missing from :root`);
    const leader = stackLeader(stack);
    let weight: number;
    try {
      weight = weightOf(triple.weight);
    } catch (e) {
      problems.push(`${loc}: ${(e as Error).message}`);
      continue;
    }
    if (!faces.some((f) => f.family === leader && faceCovers(f, weight, triple.style))) {
      problems.push(`${loc}: ${leader} ${weight} ${triple.style} is not a bundled face`);
    }
  }
  return { problems, checked };
}

// All values of one property in a block body — `declaration` returns the
// first and the browser takes the last, which is exactly the gap a duplicate
// would hide in, so a caller sees every occurrence and rejects two.
function declarations(body: string, name: string): string[] {
  const out: string[] = [];
  for (const decl of splitDeclarations(body)) {
    const colon = decl.indexOf(':');
    if (colon < 0) continue;
    if (decl.slice(0, colon).trim() !== name) continue;
    out.push(decl.slice(colon + 1).trim().replace(/\s+/g, ' '));
  }
  return out;
}

function weightOf(value: string): number {
  const words: Record<string, number> = { normal: 400, bold: 700 };
  if (/^[1-9]00$/.test(value)) return Number(value);
  if (value in words) return words[value];
  throw new Error(`font-weight "${value}" is not a number or normal/bold`);
}

// `400` or `400 600` (a variable font's range) — inclusive on both ends.
function faceCovers(face: Face, weight: number, style: string): boolean {
  if (face.style !== style) return false;
  const [lo, hi = lo] = face.weight.trim().split(/\s+/).map(Number);
  return weight >= lo && weight <= hi;
}

// The grammar a rule's font declarations must fit, and the triple it yields.
// `null` means the rule sets no font of its own and inherits; a string is a
// problem. Closed on purpose: under "all three or none, no duplicates, no
// !important, `font:` only as `inherit` or `<size> var(--stack)`" the winning
// family, weight and style of ANY element come from ONE rule (the cascade
// ranks rules, not properties) or are inherited from an element for which
// the same holds — so there is nothing to resolve.
function fontTriple(body: string): { family: string; weight: string; style: string } | null | string {
  const names = ['font', 'font-family', 'font-weight', 'font-style'] as const;
  const found = Object.fromEntries(names.map((n) => [n, declarations(body, n)])) as Record<(typeof names)[number], string[]>;
  for (const n of names) {
    if (found[n].length > 1) return `declares ${n} ${found[n].length} times — the browser takes the last, the guard would read the first`;
    if (found[n].some((v) => /!important/.test(v))) return `${n} carries !important, which would let it win over a rule that sets all three`;
  }
  const [shorthand] = found.font;
  const longhands = [found['font-family'][0], found['font-weight'][0], found['font-style'][0]];
  const set = longhands.filter((v) => v !== undefined).length;

  if (shorthand !== undefined) {
    if (shorthand === 'inherit') {
      if (set === 0) return null;
      if (set !== 3) return 'font: inherit with some of font-family/font-weight/font-style — set all three or none';
    } else {
      const sized = /^\d+(?:\.\d+)?px(?:\/\d+(?:\.\d+)?)? (var\(\s*--[\w-]+\s*\))$/.exec(shorthand);
      if (!sized) return `font shorthand "${shorthand}" — only \`inherit\` or \`<size>px[/<line-height>] var(--stack)\` is allowed; write the longhands`;
      if (set !== 0) return 'a sized font shorthand beside a font longhand — the shorthand already sets all three';
      return { family: sized[1], weight: '400', style: 'normal' };
    }
  } else if (set === 0) {
    return null;
  } else if (set !== 3) {
    return `sets ${set} of font-family/font-weight/font-style — set all three, so the rule's triple is the element's`;
  }
  return { family: longhands[0]!, weight: longhands[1]!, style: longhands[2]! };
}

describe('the stylesheets ask only for faces the bundle has', () => {
  // A weight the bundle has no face for is not an error anywhere: the
  // browser synthesises bold from 400 (or slants upright glyphs) and moves
  // on. Held as a closed grammar (see fontTriple) rather than by resolving
  // the cascade, which would be a second CSS engine in a test file.
  it('sets the font family, weight and style together, and only as faces the bundle has', () => {
    const { light } = loadThemes();
    const faces = loadFaces();
    const problems: string[] = [];
    let checked = 0;

    // Same scope as the url()/@import guard above: every .css/.svelte under
    // SRC, plus the two HTML entry points' own <style> blocks, so a font rule
    // written into a mockup transcription is held to the same grammar.
    // tokens.css declares no fonts and fonts.css IS the declaration, so both
    // stay out.
    const htmlFiles = topLevelFiles(UI_ROOT, ['.html']);
    expect(htmlFiles.length, `no .html files found directly under ${UI_ROOT}`).toBeGreaterThan(0);
    const files: string[] = [...walk(SRC, ['.css', '.svelte']), ...htmlFiles];
    for (const file of files) {
      if (file === TOKENS_PATH || file === FONTS_PATH) continue;
      const raw = readFileSync(file, 'utf8');
      const css = file.endsWith('.css') ? raw : styleBlocksOnly(raw);
      const result = fontProblems(css, file, light, faces);
      problems.push(...result.problems);
      checked += result.checked;
    }

    expect(checked, 'no rule sets a font at all — nothing to hold').toBeGreaterThan(0);
    expect(problems).toEqual([]);
  });

  it('accepts a valid triple inside a media query', () => {
    const { light } = loadThemes();
    const faces = loadFaces();
    const css = `@media (min-width: 0px) { .probe { font-family: var(--sans); font-weight: 400; font-style: normal; } }`;
    const { problems, checked } = fontProblems(css, 'inline', light, faces);
    expect(problems).toEqual([]);
    expect(checked).toBe(1);
  });

  it('rejects a face the bundle lacks inside a media query', () => {
    const { light } = loadThemes();
    const faces = loadFaces();
    const css = `@media (min-width: 0px) { .probe { font-family: var(--mono); font-weight: 600; font-style: normal; } }`;
    const { problems } = fontProblems(css, 'inline', light, faces);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain('IBM Plex Mono 600 normal is not a bundled face');
    // The location is "inline: .probe", never "inline: @media (...)" — the
    // whole point of the fix is that the at-rule container is skipped and
    // its child rule is blamed instead.
    expect(problems[0].startsWith('inline: .probe:')).toBe(true);
  });
});

describe('settings.css gives the DOM-only states a visual form', () => {
  // jsdom applies stylesheet rules to getComputedStyle, attribute selectors
  // included, and hands a var() back as text — enough to tell an element in
  // a state from its neighbour without one, which is all these hold. They
  // do not say what the state LOOKS like (the owner's screenshot against the
  // mockup does), and they cannot see whether a COMPONENT puts the class on:
  // Folders.test.ts holds that, and the import guard below holds that the
  // sheet reaches the window at all.
  function mount(html: string): void {
    document.head.querySelectorAll('style[data-guard]').forEach((s) => s.remove());
    const style = document.createElement('style');
    style.dataset.guard = '';
    style.textContent = readFileSync(SETTINGS_PATH, 'utf8');
    document.head.append(style);
    document.body.innerHTML = html;
  }
  function differ(inState: Element, without: Element, property: string): void {
    const a = getComputedStyle(inState).getPropertyValue(property);
    const b = getComputedStyle(without).getPropertyValue(property);
    expect(a, `${property}: "${a}" in the state, "${b}" without — no visual difference`).not.toBe(b);
  }

  it('marks the active section in the navigation', () => {
    mount(`<main><div class="scols"><nav class="snav">
      <button class="item" aria-pressed="false">a</button>
      <button class="item" aria-pressed="true">b</button>
    </nav></div></main>`);
    const [off, on] = document.querySelectorAll('.snav .item');
    differ(on, off, 'background');
    differ(on, off, 'font-weight');
  });

  // Task 4: the model list's "current" button is gone (a native `<select>`
  // replaces it); what marks a role configured or not now is the small dot
  // beside each tab, and it is the MARK that carries the colour — the label
  // beside it stays the ordinary readable ink, held to the same AA pair as
  // `--ink`/`--ink-soft` elsewhere in this file. Which model is actually
  // current is a fact `Models.test.ts` checks against the real component;
  // this only holds that the two dot states are told apart visually.
  //
  // Task 9 (owner's ruling, live run 2026-09-10): the dot moved INSIDE its own
  // tab button, as its last child — the fixture is re-pointed to that shape
  // rather than the two bare siblings it used to mount, so this guard keeps
  // proving something true of the actual markup rather than of a layout the
  // component no longer draws.
  it('marks a configuration dot by role', () => {
    mount(`<main><div class="spane"><div class="mtabs">
      <button type="button" class="mtab">a<span class="mdot" data-configured="true"><span class="mdot-mark"></span><span class="sr-only">configured</span></span></button>
      <button type="button" class="mtab">b<span class="mdot" data-configured="false"><span class="mdot-mark"></span><span class="sr-only">not configured</span></span></button>
    </div></div></main>`);
    const [ok, err] = document.querySelectorAll('.mdot-mark');
    // jsdom's `getComputedStyle` does not resolve `var(...)` — cssstyle hands
    // back the declared text verbatim, so this only proves the two states
    // are wired to DIFFERENT custom properties (`var(--ok)` vs `var(--err)`
    // are different strings regardless of what either token resolves to). A
    // mutant setting `--ok` and `--err` to the same colour would still pass
    // it, which is why the actual values are pinned separately below, parsed
    // straight from the declarations rather than through jsdom's style
    // engine, in every theme block this file holds to each other elsewhere.
    differ(ok, err, 'background');

    const { light, mediaDark, attrDark } = loadThemes();
    for (const [name, tokens] of [
      ['light', light], ['media dark', mediaDark], ['attribute dark', attrDark],
    ] as const) {
      expect(tokens.get('--ok'), `${name}: --ok declared`).toBeTruthy();
      expect(tokens.get('--err'), `${name}: --err declared`).toBeTruthy();
      expect(tokens.get('--ok'), `${name}: --ok must resolve to a different colour than --err`)
        .not.toBe(tokens.get('--err'));
    }
  });

  // Task 9: the tab dot's state word is `sr-only` now, not the ordinary
  // readable ink it used to be. `differ()` against the button's own visible
  // text proves the EXISTING `.sr-only` rule (`settings.css:106`) actually
  // reaches an element placed inside `.mtab .mdot`, not merely that the class
  // exists somewhere in the file.
  it('hides the tab dot\'s state word from sighted view, inside the button', () => {
    mount(`<main><div class="mtabs">
      <button type="button" class="mtab">Embedding<span class="mdot" data-configured="true"><span class="mdot-mark"></span><span class="sr-only">Configured</span></span></button>
    </div></main>`);
    const label = document.querySelector('.mtab')!;
    const hidden = document.querySelector('.mtab .sr-only')!;
    differ(hidden, label, 'position');
  });

  it('dims an excluded folder', () => {
    mount(`<main><div class="spane"><div class="folders"><ul><li><div class="fsubs"><ul>
      <li class="sub">a</li>
      <li class="sub excl">b</li>
    </ul></div></li></ul></div></div></main>`);
    const [open, excluded] = document.querySelectorAll('.sub');
    differ(excluded, open, 'color');
  });

  it('marks the chosen theme', () => {
    // `.seg`, not a bare `role="group"` (Task 6): Application's four section
    // groups carry that role too now, for their own accessible name, and this
    // fixture must mirror the class the real segmented control carries or it
    // tests a selector nothing in the app uses any more.
    mount(`<main><div class="spane"><div class="seg" role="group">
      <button type="button" aria-pressed="false">a</button>
      <button type="button" aria-pressed="true">b</button>
    </div></div></main>`);
    const [off, on] = document.querySelectorAll('.seg button');
    differ(on, off, 'background');
    differ(on, off, 'font-weight');
  });

  // Task 6, review round 1: nothing here asked for the `.statcard` rule
  // itself, only for its markup — so it went missing for a whole review
  // round with every other test still green. A bare `<dl>` against one
  // carrying the class is what a missing rule turns back into one element.
  it('gives the statcard a bordered card, not a bare list', () => {
    mount(`<main><div class="spane">
      <dl><div><dt>a</dt><dd>b</dd></div></dl>
      <dl class="statcard"><div><dt>a</dt><dd>b</dd></div></dl>
    </div></main>`);
    const [plain, card] = document.querySelectorAll('dl');
    differ(card, plain, 'border');
    differ(card, plain, 'padding-top');
  });

  // The mockup's `.frow .rm`: a 24×24 icon, never a bordered `main button`.
  it('draws the folder remove control as a small icon, not a bordered button', () => {
    mount(`<main><div class="folders"><ul><li><div class="row">
      <button type="button">plain</button>
      <button type="button" class="rm">✕</button>
    </div></li></ul></div></main>`);
    const [plain, rm] = document.querySelectorAll('.row button');
    differ(rm, plain, 'border-top-width');
    differ(rm, plain, 'width');
  });

  // Whole-branch review, Important 2. `list-style: none` plus the
  // `::-webkit-details-marker` rule removes the native triangle and drew
  // nothing in its place. `differ()` above cannot hold this rule to its
  // `[open]` counterpart: jsdom's `getComputedStyle` cannot see a
  // pseudo-element at all (`window.getComputedStyle(elt, pseudoElt)` calls
  // `notImplemented` in jsdom 25's own `Window.js` and then falls through to
  // the SAME real-element declarations a bare call would have matched,
  // which never include a pseudo-element-only rule) — proven directly: an
  // element carrying the `::after` rule and one without it report the same
  // (empty) `transform`. The stylesheet's own parsed rules are not subject
  // to that limitation (a `CSSStyleRule`'s `selectorText`/`style` are read
  // straight off the parse, never matched against an element), so this
  // reads the two `::after` rules themselves and holds their `transform`
  // declarations to each other the same way `differ()` holds two elements'.
  it('rotates the disclosure chevron between closed and open', () => {
    mount('<main></main>');
    const sheet = document.head.querySelector<HTMLStyleElement>('style[data-guard]')!.sheet!;
    const rules = Array.from(sheet.cssRules) as CSSStyleRule[];
    const closed = rules.find((r) => r.selectorText === '.job-disclosure > summary::after');
    const open = rules.find((r) => r.selectorText === '.job-disclosure[open] > summary::after');
    expect(closed, 'no ::after rule styling the closed chevron').toBeTruthy();
    expect(open, 'no [open] ::after rule styling the rotated chevron').toBeTruthy();
    const a = open!.style.getPropertyValue('transform');
    const b = closed!.style.getPropertyValue('transform');
    expect(a, `transform: "${a}" open, "${b}" closed — no visual difference`).not.toBe(b);
  });
});

describe('each window imports its stylesheets', () => {
  // Read as a syntax tree, not as text: a commented-out import is not an
  // import, and the order is the cascade's order. The window-specific sheet
  // is last so it overrides base.css; a window that forgets its own sheet
  // renders unstyled HTML with no error anywhere.
  it('imports the stylesheets each window needs, in order', () => {
    expect(WINDOW_STYLESHEETS.length, 'no window listed').toBeGreaterThan(0);
    for (const [path, expected] of WINDOW_STYLESHEETS) {
      const source = ts.createSourceFile(path, readFileSync(path, 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
      const imports: string[] = [];
      for (const s of source.statements) {
        if (ts.isImportDeclaration(s) && ts.isStringLiteral(s.moduleSpecifier) && s.moduleSpecifier.text.endsWith('.css')) {
          imports.push(s.moduleSpecifier.text);
        }
      }
      expect(imports, path).toEqual(expected);
    }
  });
});

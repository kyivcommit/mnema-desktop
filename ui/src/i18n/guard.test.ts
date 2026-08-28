import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const CYRILLIC = /[Ѐ-ӿ]/;
const LATIN_RUN = /[A-Za-z]{2,}/;

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}

// English literals that survive `textNodesOnly` for a reason other than "this
// is a user-facing string someone forgot to route through the catalogue" —
// each entry names the one file and the one literal it excuses, so it cannot
// silently cover a second, unrelated occurrence of the same word.
const LATIN_ALLOWLIST: { file: string; text: string; reason: string }[] = [];

function isAllowlisted(file: string, text: string): boolean {
  return LATIN_ALLOWLIST.some((e) => file.endsWith(e.file) && e.text === text);
}

// Replaces every character except newlines with a space, so the blanked span
// keeps the original line numbers of whatever text remains around it.
function blank(s: string): string {
  return s.replace(/[^\n]/g, ' ');
}

// Blanks every `<tag>...</tag>` block for a given tag name (script, style).
// Loops rather than matching once because Svelte allows more than one
// `<script>` block (e.g. `<script module>` alongside the instance script).
function blankBlocks(src: string, tag: string): string {
  const open = new RegExp(`<${tag}\\b`, 'i');
  const close = `</${tag}>`;
  let out = src;
  for (;;) {
    const start = out.search(open);
    if (start === -1) return out;
    const closeIdx = out.toLowerCase().indexOf(close, start);
    const end = closeIdx === -1 ? out.length : closeIdx + close.length;
    out = out.slice(0, start) + blank(out.slice(start, end)) + out.slice(end);
  }
}

function blankComments(src: string): string {
  let out = src;
  for (;;) {
    const start = out.indexOf('<!--');
    if (start === -1) return out;
    const closeIdx = out.indexOf('-->', start + 4);
    const end = closeIdx === -1 ? out.length : closeIdx + 3;
    out = out.slice(0, start) + blank(out.slice(start, end)) + out.slice(end);
  }
}

// Index just past the `>` that closes the tag opening at `src[start]` ('<').
// Cannot be a `<[^>]*>` regex: an attribute expression may itself contain a
// bare `>` (an arrow function, `onclick={() => ...}`, is exactly that), so the
// real end of the tag can only be found by tracking brace depth and quote
// state char-by-char and only accepting `>` once both are back at rest.
function tagEnd(src: string, start: number): number {
  let depth = 0;
  let quote: string | null = null;
  for (let i = start + 1; i < src.length; i++) {
    const c = src[i];
    if (quote) {
      if (c === '\\') { i++; continue; }
      if (c === quote) quote = null;
      continue;
    }
    if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
    if (c === '{') { depth++; continue; }
    if (c === '}') { if (depth > 0) depth--; continue; }
    if (c === '>' && depth === 0) return i + 1;
  }
  return src.length;
}

// Index just past the `}` matching the `{` at `src[start]`, same depth/quote
// tracking as `tagEnd` so a template literal's `${...}` inside the expression
// (e.g. `` {`tree-folder-${node.path}`} ``) does not close it early.
function exprEnd(src: string, start: number): number {
  let depth = 1;
  let quote: string | null = null;
  for (let i = start + 1; i < src.length; i++) {
    const c = src[i];
    if (quote) {
      if (c === '\\') { i++; continue; }
      if (c === quote) quote = null;
      continue;
    }
    if (c === '"' || c === "'" || c === '`') { quote = c; continue; }
    if (c === '{') { depth++; continue; }
    if (c === '}') { depth--; if (depth === 0) return i + 1; continue; }
  }
  return src.length;
}

// Reduces a .svelte file to only the characters a person can actually read on
// screen: `<script>`/`<style>` blocks, HTML comments, every tag (attributes
// included, whether a quoted literal or a `{expression}`) and every top-level
// `{expression}` in the markup are all blanked, never deleted — so whatever
// survives keeps the source file's own line numbers.
function textNodesOnly(src: string): string {
  const noBlocks = blankBlocks(blankBlocks(src, 'script'), 'style');
  const noComments = blankComments(noBlocks);
  let out = '';
  let i = 0;
  while (i < noComments.length) {
    const c = noComments[i];
    if (c === '<') {
      const end = tagEnd(noComments, i);
      out += blank(noComments.slice(i, end));
      i = end;
    } else if (c === '{') {
      const end = exprEnd(noComments, i);
      out += blank(noComments.slice(i, end));
      i = end;
    } else {
      out += c;
      i++;
    }
  }
  return out;
}

// Runs `textNodesOnly` over `src` and returns `"<line>: <match>"` for every
// remaining run of two-or-more Latin letters not on `LATIN_ALLOWLIST`.
function latinOffenses(file: string, src: string): string[] {
  return textNodesOnly(src)
    .split('\n')
    .flatMap((line, idx) => {
      const m = LATIN_RUN.exec(line);
      if (!m || isAllowlisted(file, m[0])) return [];
      return [`${idx + 1}: ${m[0]}`];
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

  // D130's Svelte half (F3): the Cyrillic guard above passes on an English
  // literal, and this PR is where English UI strings land in bulk. Text nodes
  // outside an expression must come from the catalogue in both locales; this
  // guard cannot see whether a key resolves in both, only that a `{...}`
  // expression stands where a hardcoded literal would otherwise sit.
  it('the text-node stripper keeps a catalogue call and drops everything else', () => {
    const fixture = `<script lang="ts">
  import { t } from '../i18n';
  const heading = t('models_heading');
</script>

<!-- Provider list, unbuilt: Models.svelte mounts here later -->
<main>
  <h2 id="heading" data-testid={\`section-\${heading}\`} onclick={() => (heading > 0)}>{heading}</h2>
  Models
</main>
`;
    const stripped = textNodesOnly(fixture);

    // Script content, the comment, every attribute (literal or expression)
    // and the `{heading}` expression must all be gone.
    expect(stripped).not.toContain('models_heading');
    expect(stripped).not.toContain('Provider list');
    expect(stripped).not.toContain('id="heading"');
    expect(stripped).not.toContain('data-testid');
    expect(stripped).not.toContain('heading');

    // The bare literal survives — it is exactly what the guard must catch.
    expect(stripped).toContain('Models');
  });

  it('rejects a bare Latin literal the stripper would otherwise miss', () => {
    // Same three traps named in the plan for this task: a literal between two
    // expressions, one that follows an attribute containing `>` (an arrow
    // function), and a comment immediately next to real text.
    const fixture = `<main>
  {before}Loose text{after}
  <button onclick={() => (x > 0)}>OK</button>
  <!-- not user-facing --> Sibling
</main>
`;
    const offenses = latinOffenses('fixture.svelte', fixture);
    expect(offenses.some((o) => o.includes('Loose'))).toBe(true);
    expect(offenses.some((o) => o.includes('OK'))).toBe(true);
    expect(offenses.some((o) => o.includes('Sibling'))).toBe(true);
  });

  it('no Latin literals in Svelte text nodes outside src/i18n', () => {
    const offenders = walk(srcRoot)
      .filter((p) => p.endsWith('.svelte') && !p.includes(join('src', 'i18n')))
      .flatMap((p) => latinOffenses(p, readFileSync(p, 'utf8')).map((o) => `${p}:${o}`));
    expect(offenders).toEqual([]);
  });
});

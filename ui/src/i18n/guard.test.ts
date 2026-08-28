import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const CYRILLIC = /[Ѐ-ӿ]/;
// Global on purpose. `latinOffenses` reports *every* run on a line: with a
// single `exec` the second offender on a line was invisible, so an allowlist
// entry written for the first one silently forgave its unrelated neighbour.
const LATIN_RUN = /[A-Za-z]{2,}/g;

// Attributes whose quoted value a person reads on screen. Everything else
// (`class`, `data-testid`, `role`, `type`, `id`, `src`, …) is machine-facing and
// stays hidden — as does every attribute whose value is a `{expression}`,
// because a catalogue call is exactly what this guard wants written there.
const VISIBLE_ATTRS = new Set([
  'aria-label',
  'aria-placeholder',
  'aria-roledescription',
  'aria-valuetext',
  'aria-description',
  'aria-keyshortcuts',
  'title',
  'placeholder',
  'alt',
  'label',
]);

// Tags whose body is raw text rather than markup: nothing inside them is read.
const RAW_BLOCKS = new Set(['script', 'style']);

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}

// English literals that survive `visibleTextOnly` for a reason other than "this
// is a user-facing string someone forgot to route through the catalogue" —
// each entry names the one file and the one literal it excuses, so it cannot
// silently cover a second, unrelated occurrence of the same word. Matching is
// by base name, not by suffix: an entry for `s.svelte` must not stand in for
// `Settings.svelte`.
type Allowlisted = { file: string; text: string; reason: string };
const LATIN_ALLOWLIST: Allowlisted[] = [];

function isAllowlisted(file: string, text: string, list: Allowlisted[]): boolean {
  return list.some((e) => basename(file) === e.file && e.text === text);
}

// Replaces every character except newlines with a space, so the blanked span
// keeps the original line numbers of whatever text remains around it.
function blank(s: string): string {
  return s.replace(/[^\n]/g, ' ');
}

// `&nbsp;` is not someone's hardcoded English: it is punctuation spelled with
// Latin letters. Blanked (never deleted) like everything else, so the first
// non-breaking space in the markup cannot push someone into writing an
// allowlist entry with an untrue reason.
function blankEntities(s: string): string {
  return s.replace(/&#?\w+;/g, blank);
}

// Index just past the `}` matching the `{` at `src[start]`, or -1 if it never
// closes. The stack tracks JS string/template nesting so a `}` inside a string
// (`{t('a}b')}`), inside a template literal (`` {`a } b`} ``) or inside a
// `${...}` within one does not close the expression early, and so a `//` or
// `/* */` comment (which may hold an unbalanced apostrophe) is skipped whole.
function exprEnd(src: string, start: number): number {
  const stack: string[] = ['{'];
  for (let i = start + 1; i < src.length; i++) {
    const top = stack[stack.length - 1];
    const c = src[i];
    if (top === '"' || top === "'") {
      if (c === '\\') { i++; continue; }
      if (c === top) stack.pop();
      continue;
    }
    if (top === '`') {
      if (c === '\\') { i++; continue; }
      if (c === '`') { stack.pop(); continue; }
      if (c === '$' && src[i + 1] === '{') { stack.push('{'); i++; }
      continue;
    }
    if (c === '/' && src[i + 1] === '/') {
      const nl = src.indexOf('\n', i);
      if (nl === -1) return -1;
      i = nl;
      continue;
    }
    if (c === '/' && src[i + 1] === '*') {
      const close = src.indexOf('*/', i + 2);
      if (close === -1) return -1;
      i = close + 1;
      continue;
    }
    if (c === '"' || c === "'" || c === '`') { stack.push(c); continue; }
    if (c === '{') { stack.push('{'); continue; }
    if (c === '}') { stack.pop(); if (stack.length === 0) return i + 1; }
  }
  return -1;
}

type Tag = {
  end: number; // index just past the `>` that closes the opening tag
  name: string;
  closing: boolean;
  selfClosing: boolean;
  visible: [number, number][]; // spans of user-facing attribute *values*
};

// Walks one tag from the `<` at `src[start]`, in attribute position the whole
// way, and returns null when what follows is not a tag at all.
//
// Cannot be a `<[^>]*>` regex: an attribute expression may hold a bare `>` (an
// arrow function, `onclick={() => ...}`, is exactly that), so the real end of
// the tag is only findable by tracking quote and brace state char-by-char.
// Attribute position is also the only place `title="…"` can be told apart from
// the `'files'` in `aria-pressed={tab === 'files'}`, which a regex over the
// whole tag would read as a hardcoded value and report as an offender.
function scanTag(src: string, start: number): Tag | null {
  let i = start + 1;
  const closing = src[i] === '/';
  if (closing) i++;
  const nameStart = i;
  while (i < src.length && !/[\s/><]/.test(src[i])) i++;
  const name = src.slice(nameStart, i).toLowerCase();
  const visible: [number, number][] = [];
  for (;;) {
    while (i < src.length && /\s/.test(src[i])) i++;
    if (i >= src.length) return null;
    const c = src[i];
    if (c === '>') return { end: i + 1, name, closing, selfClosing: false, visible };
    if (c === '/' && src[i + 1] === '>') return { end: i + 2, name, closing, selfClosing: true, visible };
    if (c === '{') {
      const end = exprEnd(src, i);
      if (end === -1) return null;
      i = end;
      continue;
    }
    if (c === '/' || c === '=') { i++; continue; }
    const attrStart = i;
    while (i < src.length && !/[\s=/><{]/.test(src[i])) i++;
    // Nothing here that an attribute name can even start with — in practice a
    // bare `<`, which means this tag never closed and the `<` that opened it
    // was text: `Cost < 5`, or a stray `<script` word in a sentence. Bailing
    // out on zero progress is also what keeps this loop guaranteed to advance.
    if (i === attrStart) return null;
    const attr = src.slice(attrStart, i).toLowerCase();
    let j = i;
    while (j < src.length && /\s/.test(src[j])) j++;
    if (src[j] !== '=') continue; // valueless attribute (`disabled`, `hidden`)
    j++;
    while (j < src.length && /\s/.test(src[j])) j++;
    const q = src[j];
    if (q === '"' || q === "'") {
      // HTML has no backslash escape inside an attribute value: it ends at the
      // first matching quote. Reading `\"` as an escape made `title="a\">` run
      // to end of file and took the rest of the markup with it, and it makes a
      // Windows path in a `title` — `C:\path` — behave differently by accident.
      const close = src.indexOf(q, j + 1);
      if (close === -1) return null;
      if (VISIBLE_ATTRS.has(attr)) visible.push([j + 1, close]);
      i = close + 1;
    } else if (q === '{') {
      const end = exprEnd(src, j);
      if (end === -1) return null;
      i = end;
    } else {
      i = j;
      while (i < src.length && !/[\s>]/.test(src[i])) i++;
    }
  }
}

// Reduces a .svelte file to only the characters a person can actually read on
// screen: text nodes, plus the string values of user-facing attributes.
//
// One left-to-right pass holding a state — text / tag / attribute string /
// expression / comment / raw block — rather than independent passes over the
// raw text. Independent passes each answer "is this a delimiter?" without
// knowing what the others established, so a `<script` inside a comment, an
// attribute or a text node opened a block that never closed and blanked the
// file to its end; the guard then returned nothing and looked green.
//
// Nothing is deleted, only blanked, so whatever survives keeps the source
// file's own line numbers.
function visibleTextOnly(src: string): string {
  const lower = src.toLowerCase();
  const out: string[] = [];
  let i = 0;
  while (i < src.length) {
    const c = src[i];
    if (c === '<' && /[A-Za-z!/?]/.test(src[i + 1] ?? '')) {
      if (src.startsWith('<!--', i)) {
        const close = src.indexOf('-->', i + 4);
        // Malformed markup must fail loudly. Blanking to end of file instead
        // leaves a guard that cannot reject anything and says so as `[]`.
        if (close === -1) throw new Error(`unterminated HTML comment at index ${i}`);
        out.push(blank(src.slice(i, close + 3)));
        i = close + 3;
        continue;
      }
      const tag = scanTag(src, i);
      if (tag === null) { out.push('<'); i++; continue; }
      let cursor = i;
      for (const [from, to] of tag.visible) {
        out.push(blank(src.slice(cursor, from)));
        out.push(src.slice(from, to));
        cursor = to;
      }
      out.push(blank(src.slice(cursor, tag.end)));
      i = tag.end;
      if (RAW_BLOCKS.has(tag.name) && !tag.closing && !tag.selfClosing) {
        // Svelte allows more than one `<script>` (`<script module>` beside the
        // instance script), so this is per-tag, not once per file.
        const closeIdx = lower.indexOf(`</${tag.name}`, i);
        const gt = closeIdx === -1 ? -1 : src.indexOf('>', closeIdx);
        if (gt === -1) throw new Error(`unterminated <${tag.name}> block at index ${i}`);
        out.push(blank(src.slice(i, gt + 1)));
        i = gt + 1;
      }
      continue;
    }
    if (c === '{') {
      const end = exprEnd(src, i);
      // An unbalanced `{` is a Svelte compile error. Treating it as one
      // character of text keeps the guard reading the markup that follows,
      // instead of going blind from here to end of file.
      if (end === -1) { out.push('{'); i++; continue; }
      out.push(blank(src.slice(i, end)));
      i = end;
      continue;
    }
    out.push(c);
    i++;
  }
  return blankEntities(out.join(''));
}

// Runs `visibleTextOnly` over `src` and returns `"<line>: <match>"` for every
// remaining run of two-or-more Latin letters not on the allowlist.
function latinOffenses(file: string, src: string, list: Allowlisted[] = LATIN_ALLOWLIST): string[] {
  return visibleTextOnly(src)
    .split('\n')
    .flatMap((line, idx) =>
      [...line.matchAll(LATIN_RUN)]
        .map((m) => m[0])
        .filter((text) => !isAllowlisted(file, text, list))
        .map((text) => `${idx + 1}: ${text}`),
    );
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
  it('the stripper keeps a catalogue call and drops everything else', () => {
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
    const stripped = visibleTextOnly(fixture);

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

  // Kills: `LATIN_RUN` loosened to `{1,}`, and tightened to `{3,}`. The rule
  // was asserted in the strengthening direction only, so half of it was free.
  it('a run of two Latin letters offends and a single letter does not', () => {
    expect(latinOffenses('f.svelte', '<main>\n  OK\n</main>\n')).toEqual(['2: OK']);
    expect(latinOffenses('f.svelte', '<main>\n  O K\n</main>\n')).toEqual([]);
  });

  // Kills: `blank` deleting instead of blanking, or flattening newlines to
  // spaces. The whole purpose of `blank` is the reported line number, and
  // asserting on the offending *word* alone never touched it.
  it('reports the line the offender is on, past a multi-line block and comment', () => {
    const fixture = `<script lang="ts">
  const a = 1;
  const b = 2;
</script>

<!-- a comment
     spanning two lines -->
<main>
  Models
</main>
`;
    expect(latinOffenses('f.svelte', fixture)).toEqual(['9: Models']);
  });

  // Kills: `isAllowlisted` returning false always; the allowlist ignored
  // outright; `endsWith` file matching; and one `exec` per line, which let an
  // entry for the first offender forgive a second, unrelated one beside it.
  it('an allowlist entry suppresses exactly its own offender', () => {
    const settings = '/ui/src/settings/Settings.svelte';
    const twoOnOneLine = '<main>\n  OK Cancel\n</main>\n';
    const entry = [{ file: 'Settings.svelte', text: 'OK', reason: 'test fixture' }];

    expect(latinOffenses(settings, twoOnOneLine, [])).toEqual(['2: OK', '2: Cancel']);
    expect(latinOffenses(settings, twoOnOneLine, entry)).toEqual(['2: Cancel']);
    // The entry belongs to one file …
    expect(latinOffenses('/ui/src/launcher/Tree.svelte', twoOnOneLine, entry)).toEqual(['2: OK', '2: Cancel']);
    // … named exactly, not by suffix.
    const suffix = [{ file: 's.svelte', text: 'OK', reason: 'test fixture' }];
    expect(latinOffenses(settings, twoOnOneLine, suffix)).toEqual(['2: OK', '2: Cancel']);
  });

  // Kills: comment handling removed. Without it `<!--` is swallowed as a tag
  // that ends at the first `>` *inside* the comment, and the prose after that
  // `>` leaks out as text — which is why a sibling-literal fixture proved
  // nothing here: the sibling survived stripping either way.
  it('a comment is read as a comment, not as a tag ending at its first `>`', () => {
    const fixture = "<main>\n  <!-- D130 -> follow-up: Provider list -->\n  {t('x')}\n</main>\n";
    expect(latinOffenses('f.svelte', fixture)).toEqual([]);
  });

  // Kills: backticks dropped from `exprEnd`'s quote set. The expression must
  // be a *markup* one — a template literal inside an attribute never reaches
  // this path, so a fixture that puts it there tests nothing.
  it('a template literal in a markup expression is not closed by a brace inside it', () => {
    expect(latinOffenses('f.svelte', '<main>\n  {`aa } bb`} Provider\n</main>\n')).toEqual(['2: Provider']);
  });

  // Kills: entity blanking removed. No `&nbsp;` exists in the markup yet, so
  // this is a mine rather than a bug — the first one written would be reported
  // as the offender `nbsp`, and the fix for that is an untrue allowlist entry.
  it('an HTML entity is punctuation, not a hardcoded English word', () => {
    expect(latinOffenses('f.svelte', '<main>\n  {a}&nbsp;{b}&mdash;{c}&#8212;{d}\n</main>\n')).toEqual([]);
  });

  // Kills: any return to searching the raw text for `<script`/`<style`. Each
  // of these three put a block opener where no block opens, and each one blanked
  // the file from there to its end — the guard then returned `[]`, green.
  it('a block opener inside a comment, an attribute or a text node opens no block', () => {
    expect(latinOffenses('f.svelte', '<main>\n  <!-- see <script above -->\n  Provider\n</main>\n'))
      .toEqual(['3: Provider']);
    expect(latinOffenses('f.svelte', '<main>\n  <div title="a <script tag">{x}</div>\n  Provider\n</main>\n'))
      .toEqual(['2: script', '2: tag', '3: Provider']);
    expect(latinOffenses('f.svelte', '<main>\n  <style\n  Provider\n</main>\n'))
      .toEqual(['2: style', '3: Provider']);
    expect(latinOffenses('f.svelte', '<main>\n  <div title="a <!-- b">Provider</div>\n</main>\n'))
      .toEqual(['2: Provider']);
  });

  // Kills: blanking to end of file when a block or comment never closes. A
  // malformed file must fail loudly; today it passed silently, which is worse
  // than either — the guard reported nothing and nothing said why.
  it('an unterminated block or comment throws instead of blanking to end of file', () => {
    expect(() => visibleTextOnly('<script lang="ts">\n  const a = 1;\n<main>\n  Provider\n</main>\n'))
      .toThrow(/unterminated <script>/);
    expect(() => visibleTextOnly('<main>\n  <style>\n  .a { color: red }\n</main>\n'))
      .toThrow(/unterminated <style>/);
    expect(() => visibleTextOnly('<main>\n  <!-- open\n  Provider\n</main>\n'))
      .toThrow(/unterminated HTML comment/);
    // …and a well-formed pair of blocks does not throw.
    expect(visibleTextOnly('<script>\n  const a = 1;\n</script>\n<style>\n  .a { color: red }\n</style>\n').trim())
      .toEqual('');
  });

  // Kills: user-facing attribute values stripped along with the machine ones.
  // Measured on this repository the day it was written: zero false positives,
  // because every user-facing attribute already goes through the catalogue.
  it('scans the string values of user-facing attributes and hides the rest', () => {
    const facing = `<main>
  <nav aria-label="Sections"><button title="Not ready" placeholder="Filter" /></nav>
  <img src="a.png" alt="Model diagram" />
</main>
`;
    expect(latinOffenses('f.svelte', facing))
      .toEqual(['2: Sections', '2: Not', '2: ready', '2: Filter', '3: Model', '3: diagram']);

    const machine = `<main>
  <div class="snav wide" data-testid="section-models" role="navigation">{x}</div>
</main>
`;
    expect(latinOffenses('f.svelte', machine)).toEqual([]);

    // An expression-valued attribute is a catalogue call, not a literal. Read
    // by a regex over the whole tag, `aria-pressed={tab === 'files'}` becomes
    // `aria-pressed="files"` and invents an offender that does not exist.
    const expressions = "<main>\n  <button title={t('x')} aria-pressed={tab === 'files'}>{y}</button>\n</main>\n";
    expect(latinOffenses('f.svelte', expressions)).toEqual([]);
  });

  // Kills: one `exec` per line; and `<` in a text node swallowing up to the
  // next `>`, which ate `Provider` out of `Cost < 5 Provider limit`.
  it('reports every offender on a line, and a bare `<` in text eats nothing', () => {
    expect(latinOffenses('f.svelte', '<main>\n  Cost < 5 Provider limit\n</main>\n'))
      .toEqual(['2: Cost', '2: Provider', '2: limit']);
    // A `>` later on the line is what makes this shape bite: without it the
    // malformed-tag bail-out rescues the words anyway, so a fixture that omits
    // the `>` proves only that the *other* defence works.
    expect(latinOffenses('f.svelte', '<main>\n  Cost < 5 and Provider > limit\n</main>\n'))
      .toEqual(['2: Cost', '2: and', '2: Provider', '2: limit']);
    expect(latinOffenses('f.svelte', '<main>\n  {a}Provider{b}Models\n</main>\n'))
      .toEqual(['2: Provider', '2: Models']);
    expect(latinOffenses('f.svelte', "<main>\n  it's <b>Provider</b>\n</main>\n"))
      .toEqual(['2: it', '2: Provider']);
    // An unbalanced `{` is one character of text, not a blind spot to EOF.
    expect(latinOffenses('f.svelte', '<main>\n  { <b>Provider</b>\n</main>\n'))
      .toEqual(['2: Provider']);
  });

  // Kills: a backslash treated as an escape inside an attribute value (HTML has
  // no such escape, so `title="a\">` used to run to end of file) — while the
  // same backslash inside a JS string in an expression must keep working.
  it('a backslash escapes inside an expression string but not inside an attribute', () => {
    expect(latinOffenses('f.svelte', '<main>\n  <div title="a\\">Provider</div>\n</main>\n'))
      .toEqual(['2: Provider']);
    expect(latinOffenses('f.svelte', "<main>\n  <div title='a\\'>Provider</div>\n</main>\n"))
      .toEqual(['2: Provider']);
    expect(latinOffenses('f.svelte', "<main>\n  {t('a\\'} bb')} Provider\n</main>\n"))
      .toEqual(['2: Provider']);
  });

  it('no Latin literals in Svelte text nodes outside src/i18n', () => {
    const offenders = walk(srcRoot)
      .filter((p) => p.endsWith('.svelte') && !p.includes(join('src', 'i18n')))
      .flatMap((p) => latinOffenses(p, readFileSync(p, 'utf8')).map((o) => `${p}:${o}`));
    expect(offenders).toEqual([]);
  });
});

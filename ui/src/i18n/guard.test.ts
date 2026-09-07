import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as ts from 'typescript';

const CYRILLIC = /[Ѐ-ӿ]/;
// Global on purpose. `latinOffenses` reports *every* run on a line: with a
// single `exec` the second offender on a line was invisible, so an allowlist
// entry written for the first one silently forgave its unrelated neighbour.
const LATIN_RUN = /[A-Za-z]{2,}/g;

// The Cyrillic sweep's own predicate, extracted so a fixture test can drive
// the EXACT function the production sweep below calls, over synthetic files,
// rather than asserting `CYRILLIC` matches a bare string in isolation
// (external review, Important 1). The difference matters: a regression that
// re-wraps `read` in a comment/`//`-line stripper — restoring either of the
// two holes `stripJsComments` had — changes what THIS function returns, so a
// fixture test built on it catches that regression; a test that only calls
// `CYRILLIC.test(...)` directly never goes near `read` and cannot.
function cyrillicOffenders(files: string[], read: (file: string) => string): string[] {
  return files.filter((p) => CYRILLIC.test(read(p)));
}

// Attribute names whose value is never prose. Every attribute NOT named here
// has its string value scanned, which is the point: naming the readable ones
// instead would be a closed list over an open set, and every component prop a
// later screen invents (`<Models heading="Providers" />`) would join that set
// unguarded. A wrong entry below costs one false positive — loud, and one line
// to fix. A name missing from an inclusion list ships English to a person in
// silence. The guard has to fail in the loud direction.
//
// Values that are `{expressions}` are never scanned whatever the name, because
// a catalogue call is exactly what this guard wants written there.
const MACHINE_ATTRS = new Set([
  // Selector hooks and slots: names the stylesheet and the tests reach for.
  'class', 'id', 'style', 'slot', 'part', 'exportparts', 'key',
  // URLs, paths and MIME types: addresses, not sentences.
  'href', 'src', 'srcset', 'imagesrcset', 'sizes', 'action', 'formaction',
  'poster', 'cite', 'ping', 'manifest', 'integrity', 'xmlns', 'accept', 'media',
  // Wiring: one element naming another, or naming itself to a form.
  'name', 'for', 'form', 'list', 'headers', 'itemprop', 'itemtype', 'itemid',
  // Values drawn from a grammar the HTML spec fixes, not from the catalogue.
  'role', 'type', 'method', 'enctype', 'rel', 'target', 'kind', 'scope',
  'shape', 'preload', 'sandbox', 'as', 'blocking', 'capture', 'decoding',
  'loading', 'fetchpriority', 'referrerpolicy', 'crossorigin', 'wrap',
  'autocomplete', 'inputmode', 'enterkeyhint', 'autocapitalize', 'spellcheck',
  'translate', 'contenteditable', 'draggable', 'popover', 'popovertargetaction',
  'formmethod', 'formtarget', 'formenctype', 'dir', 'lang', 'hreflang',
  // Numbers and machine-readable dates: no prose can hide in them.
  'width', 'height', 'size', 'rows', 'cols', 'span', 'colspan', 'rowspan',
  'start', 'min', 'max', 'step', 'maxlength', 'minlength', 'tabindex', 'datetime',
  // ARIA attributes that hold an id reference — the id, never its text.
  'aria-controls', 'aria-labelledby', 'aria-describedby', 'aria-details',
  'aria-owns', 'aria-flowto', 'aria-activedescendant', 'aria-errormessage',
  // ARIA attributes whose value is a token, a boolean or a number fixed by the
  // ARIA spec. `aria-hidden="true"` reads as English but nobody reads it.
  'aria-live', 'aria-haspopup', 'aria-current', 'aria-sort', 'aria-orientation',
  'aria-autocomplete', 'aria-relevant', 'aria-dropeffect', 'aria-hidden',
  'aria-expanded', 'aria-selected', 'aria-checked', 'aria-pressed',
  'aria-disabled', 'aria-readonly', 'aria-required', 'aria-invalid',
  'aria-modal', 'aria-atomic', 'aria-busy', 'aria-multiline',
  'aria-multiselectable', 'aria-level', 'aria-posinset', 'aria-setsize',
  'aria-colcount', 'aria-colindex', 'aria-colspan', 'aria-rowcount',
  'aria-rowindex', 'aria-rowspan', 'aria-valuemax', 'aria-valuemin',
  'aria-valuenow',
  // SVG geometry and paint: coordinates and colour keywords.
  'd', 'points', 'viewbox', 'transform', 'fill', 'stroke', 'stroke-width',
  'stroke-linecap', 'stroke-linejoin', 'stroke-dasharray', 'fill-rule',
  'clip-rule', 'vector-effect', 'preserveaspectratio', 'gradientunits',
  'patternunits', 'pathlength', 'text-anchor', 'dominant-baseline',
  'font-family', 'font-size', 'font-weight', 'opacity', 'stop-color',
  'stop-opacity', 'offset', 'cx', 'cy', 'rx', 'ry', 'x1', 'y1', 'x2', 'y2',
]);

function isMachineAttr(attr: string): boolean {
  // `data-*` is an author-defined hook for scripts and tests by definition.
  if (attr.startsWith('data-')) return true;
  // A colon means a Svelte directive (`bind:`, `use:`, `class:`, `transition:`)
  // or an XML namespace (`xlink:href`): the value is code or a machine name.
  if (attr.includes(':')) return true;
  return MACHINE_ATTRS.has(attr);
}

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
// each entry names the one file and the one literal it excuses. That excuses
// EVERY occurrence of that literal in that file, not only the one that
// motivated the entry: `isAllowlisted` matches on (file, text) alone, with no
// notion of location, so a second `Ctrl` added anywhere else in `shortcut.ts`
// is just as invisible to this sweep as the first one. That is the accepted
// cost of an allowlist keyed this coarsely, not a guarantee — an earlier
// version of this comment claimed the opposite ("cannot silently cover a
// second, unrelated occurrence"), and a reviewer's second `'Ctrl'` proved it
// false by staying green. Matching is by base name, not by suffix: an entry
// for `s.svelte` must not stand in for `Settings.svelte`.
type Allowlisted = { file: string; text: string; reason: string };
const LATIN_ALLOWLIST: Allowlisted[] = [
  // shortcut.ts emits the DISPLAY vocabulary a global shortcut is drawn
  // with — modifier names, key names, their Windows/Linux spellings — never
  // a sentence (every sentence this module shows comes from `catalog.ts`).
  // Each token is a name printed on a physical key or a DOM `KeyboardEvent`
  // constant, not prose read as a phrase.
  { file: 'shortcut.ts', text: 'Ctrl', reason: 'modifier name, printed on the key' },
  { file: 'shortcut.ts', text: 'Alt', reason: 'modifier name, printed on the key' },
  { file: 'shortcut.ts', text: 'Shift', reason: 'modifier name, printed on the key' },
  { file: 'shortcut.ts', text: 'Super', reason: 'modifier name, Linux spelling' },
  { file: 'shortcut.ts', text: 'Win', reason: 'modifier name, Windows spelling' },
  { file: 'shortcut.ts', text: 'Cmd', reason: 'modifier name for prose, mac Command key' },
  { file: 'shortcut.ts', text: 'Control', reason: 'KeyboardEvent.key value for a bare modifier press' },
  { file: 'shortcut.ts', text: 'AltGraph', reason: 'KeyboardEvent.key value for a bare modifier press' },
  { file: 'shortcut.ts', text: 'Meta', reason: 'KeyboardEvent.key value for a bare modifier press' },
  { file: 'shortcut.ts', text: 'OS', reason: 'KeyboardEvent.key value some browsers report for Meta' },
  { file: 'shortcut.ts', text: 'CapsLock', reason: 'KeyboardEvent.key/code value for a bare modifier press' },
  { file: 'shortcut.ts', text: 'ControlLeft', reason: 'KeyboardEvent.code value for a bare modifier press' },
  { file: 'shortcut.ts', text: 'ControlRight', reason: 'KeyboardEvent.code value for a bare modifier press' },
  { file: 'shortcut.ts', text: 'AltLeft', reason: 'KeyboardEvent.code value for a bare modifier press' },
  { file: 'shortcut.ts', text: 'AltRight', reason: 'KeyboardEvent.code value for a bare modifier press' },
  { file: 'shortcut.ts', text: 'ShiftLeft', reason: 'KeyboardEvent.code value for a bare modifier press' },
  { file: 'shortcut.ts', text: 'ShiftRight', reason: 'KeyboardEvent.code value for a bare modifier press' },
  { file: 'shortcut.ts', text: 'MetaLeft', reason: 'KeyboardEvent.code value for a bare modifier press' },
  { file: 'shortcut.ts', text: 'MetaRight', reason: 'KeyboardEvent.code value for a bare modifier press' },
  { file: 'shortcut.ts', text: 'Space', reason: "key name: a stored-shortcut token, a KeyboardEvent.code, and the literal keyName (shortcut.ts:147) returns and formatShortcut then displays verbatim (e.g. '⌃Space') — not a sentence" },
  { file: 'shortcut.ts', text: 'Escape', reason: "the recorder's own cancel key, a KeyboardEvent.key/code value" },
  { file: 'shortcut.ts', text: 'mac', reason: "the Platform union's own tag, matched in a type argument and a comparison, not prose" },

  // index.ts: a BCP-47 locale code and the two halves of Tauri command/event
  // names, none of it prose. `'uk'` needs no entry here: every occurrence in
  // this file (the `Loc` alias, and two generic type arguments) is a string
  // literal TYPE, not a value — `isTypePosition` in the sweep above excludes
  // all three before the allowlist is ever consulted, so an entry for `'uk'`
  // would excuse nothing (review, Minor 1). `'en'` still needs one:
  // `writable<Loc>('en')` and `const FALLBACK: Loc = 'en'` are real runtime
  // values, the only two, so this stays a live entry rather than a second
  // dead one.
  { file: 'index.ts', text: 'en', reason: 'BCP-47 locale code, not prose' },
  { file: 'index.ts', text: 'locale', reason: "half of the Tauri event name 'locale-changed'" },
  { file: 'index.ts', text: 'changed', reason: "half of the Tauri event name 'locale-changed'" },
  { file: 'index.ts', text: 'get', reason: "half of the Tauri command name 'get_locale'" },

  // recency.ts: an Intl formatting option, not prose.
  { file: 'recency.ts', text: 'long', reason: 'Intl.DateTimeFormat dateStyle option value' },
];

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
    // Inverted rule: an attribute nobody thought to classify is scanned.
    const prose = !isMachineAttr(attr);
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
      if (prose) visible.push([j + 1, close]);
      i = close + 1;
    } else if (q === '{') {
      const end = exprEnd(src, j);
      if (end === -1) return null;
      i = end;
    } else {
      // An unquoted value (`title=Hello`) is still a value a person reads, and
      // leaving it out would be a hole in the very rule above.
      i = j;
      while (i < src.length && !/[\s>]/.test(src[i])) i++;
      if (prose) visible.push([j, i]);
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

// Shared by both sweeps below: given text already reduced to "what a person
// could read", split into lines and report every Latin run not on the
// allowlist. This is the one place the allowlist rule itself lives, so the
// `.ts` sweep further down is a different REDUCTION over the same rule, not a
// second copy of it.
function offensesFrom(file: string, reduced: string, list: Allowlisted[]): string[] {
  return reduced
    .split('\n')
    .flatMap((line, idx) =>
      [...line.matchAll(LATIN_RUN)]
        .map((m) => m[0])
        .filter((text) => !isAllowlisted(file, text, list))
        .map((text) => `${idx + 1}: ${text}`),
    );
}

// Runs `visibleTextOnly` over `src` and returns `"<line>: <match>"` for every
// remaining run of two-or-more Latin letters not on the allowlist.
function latinOffenses(file: string, src: string, list: Allowlisted[] = LATIN_ALLOWLIST): string[] {
  return offensesFrom(file, visibleTextOnly(src), list);
}

// ---------------------------------------------------------------------------
// The sweep above only ever looks at `.svelte` markup. A `.ts` module under
// `src/i18n` can emit its own display words without ever touching a `<...>`
// tag — `shortcut.ts` is the concrete case: `Ctrl`, `Alt`, `Win`, the DOM key
// names, none of it routed through `catalog.ts` — and no sweep looked at it
// at all. `visibleTextOnly` cannot be reused as-is; it walks HTML/Svelte
// markup and a `.ts` file has none. What follows reduces a `.ts` file to only
// the CONTENTS of its string and template literals, blanking four things
// that are lexically string-shaped but are never prose:
//
//   - an import/export module specifier, including a dynamic `import(...)`
//     — an address, the same reason `href` is excluded from the attribute
//     sweep above;
//   - the FIRST argument of a call to `t(...)`, the catalogue accessor — the
//     key, whatever shape it is written in. A key picked by a ternary
//     (`t(cond ? 'a' : 'b')`) is caught at ANY depth below that argument, not
//     only when it is an immediate literal, mirroring `{t('key')}` being
//     blanked whole in the markup sweep above. Every OTHER argument is
//     scanned on its own terms: `t('k', { x: 'Zebra' })`'s `Zebra` was
//     invisible on `main` because the whole call was blanked as one span —
//     a booked blind spot, closed here, because only the first argument is
//     ever the catalogue's own vocabulary;
//   - a `case` label's own expression — a discriminant tag matched against a
//     union type, the same "fixed vocabulary, not a sentence" class as
//     `role`/`type` in the Svelte attribute sweep;
//   - a string used as a PROPERTY NAME rather than a value — an object
//     literal key, or a `type`/`interface` member name — and a string used
//     as a TYPE rather than a value (`'mac'` in `Exclude<Platform, 'mac'>`,
//     a type argument, never a runtime string).
//
// Unlike `visibleTextOnly`, this reduction is built on `typescript`'s own
// parser (`ts.createSourceFile`) rather than a hand-rolled character walk.
// Two independent hand-rolled scanners over this file — this one, in an
// earlier revision, and the Cyrillic sweep's now-deleted `stripJsComments` —
// separately reinvented "where does a comment end" and "where does a string
// end", and got the same question wrong the same way (external review, P2):
// neither recognised a REGEX literal, so a `//` sitting inside one
// (`/\/\//`, never inside quotes) was read as a `//` comment, and everything
// after it on the line — Latin or Cyrillic — silently vanished; and a nested
// template literal's own inner backtick pair was read as closing the OUTER
// one. A real parser has neither hole: a `RegularExpressionLiteral` is its
// own node kind, never visited by the walk below, so a quote or a `//`
// inside one is never even examined as string/comment syntax; and nested
// template literals nest in the AST exactly as they nest in the source, so
// the false branch's own template is visited on its own terms with no
// special-casing needed. That is the reason to parse rather than
// pattern-match, not a stylistic preference.
// ---------------------------------------------------------------------------

// Node kinds whose text is something a person could read on screen: a plain
// string, and each piece of a template literal around its `${...}`
// interpolations (which are visited separately, as their own expressions,
// and so are never read as this literal's own text).
const STRING_LIKE_KINDS = new Set<ts.SyntaxKind>([
  ts.SyntaxKind.StringLiteral,
  ts.SyntaxKind.NoSubstitutionTemplateLiteral,
  ts.SyntaxKind.TemplateHead,
  ts.SyntaxKind.TemplateMiddle,
  ts.SyntaxKind.TemplateTail,
]);

// The half-open span of `node`'s OWN text, its delimiters dropped: one quote
// or backtick at the start for every kind, and — depending on which side(s)
// still carry a backtick versus a `${`/`}` interpolation boundary — one or
// two characters at the end.
function contentSpan(sourceFile: ts.SourceFile, node: ts.Node): [number, number] {
  const start = node.getStart(sourceFile) + 1;
  switch (node.kind) {
    case ts.SyntaxKind.TemplateHead: // `` `text${ `` — drop the backtick and the `${`
    case ts.SyntaxKind.TemplateMiddle: // `` }text${ `` — drop the leading `}` and the `${`
      return [start, node.end - 2];
    default: // StringLiteral, NoSubstitutionTemplateLiteral, TemplateTail: one delimiter on each side
      return [start, node.end - 1];
  }
}

// True when `node` sits inside the FIRST argument of a call to the bare
// identifier `t`, at any depth — so a key picked by a ternary is caught the
// same as a plain literal. Climbs one parent at a time rather than checking
// only the immediate parent, because the catalogue key is not always an
// immediate child of the call (`t(cond ? 'a' : 'b')`'s `'a'` is two levels
// down, inside the `ConditionalExpression` that IS the first argument).
//
// This is by NAME alone, the same as the deleted hand-rolled version's
// lookbehind was: a local variable or parameter that shadows `t` with
// something that is NOT the catalogue accessor (review, Minor 3) would still
// have its first argument skipped here. That is the QUIET failure direction
// — a real hardcoded string going unreported rather than a false alarm — and
// unlike every other blind spot named in this file it is not asserted shut
// by a test, only accepted: no call site anywhere in this codebase shadows
// `t` today, and the day one does, this comment is where to look.
function isCatalogueKey(node: ts.Node): boolean {
  let current: ts.Node = node;
  for (let parent = current.parent; parent; current = parent, parent = current.parent) {
    if (
      ts.isCallExpression(parent) &&
      ts.isIdentifier(parent.expression) &&
      parent.expression.text === 't' &&
      parent.arguments[0] === current
    ) return true;
  }
  return false;
}

// True when `node` is the module specifier of a static import/export
// declaration, or the sole argument of a dynamic `import(...)`.
function isModuleSpecifier(node: ts.Node): boolean {
  const p = node.parent;
  if (!p) return false;
  if ((ts.isImportDeclaration(p) || ts.isExportDeclaration(p)) && p.moduleSpecifier === node) return true;
  // `ts.isImportCall` exists at runtime but is not part of the package's
  // public `.d.ts`, so a dynamic `import(...)` is recognised the same way
  // that helper does internally: a call whose callee token is `import`.
  if (ts.isCallExpression(p) && p.expression.kind === ts.SyntaxKind.ImportKeyword && p.arguments[0] === node) return true;
  return false;
}

// True when `node` is a `case` clause's own discriminant expression.
function isCaseLabel(node: ts.Node): boolean {
  const p = node.parent;
  return !!p && ts.isCaseClause(p) && p.expression === node;
}

// True when `node` stands in for a PROPERTY NAME rather than a value — an
// object-literal key, or a type/interface/class/enum member name.
function isKeyPosition(node: ts.Node): boolean {
  const p = node.parent;
  if (!p) return false;
  return (
    (ts.isPropertyAssignment(p) ||
      ts.isPropertySignature(p) ||
      ts.isPropertyDeclaration(p) ||
      ts.isMethodDeclaration(p) ||
      ts.isMethodSignature(p) ||
      ts.isGetAccessorDeclaration(p) ||
      ts.isSetAccessorDeclaration(p) ||
      ts.isEnumMember(p)) &&
    p.name === node
  );
}

// True when `node` is a string literal TYPE (`'mac'` in
// `Exclude<Platform, 'mac'>`) rather than a runtime value.
function isTypePosition(node: ts.Node): boolean {
  return !!node.parent && ts.isLiteralTypeNode(node.parent);
}

// The first syntax error `src` produces, or `null` if it parses cleanly.
//
// An earlier revision read `ts.createSourceFile(...).parseDiagnostics`
// directly — a field the parser genuinely populates, but one that is not
// part of the package's public `.d.ts` (@internal), reached only through a
// cast. `ts.transpileModule` with `reportDiagnostics: true` asks the same
// question through the public surface instead: it runs the same parser and
// returns the same syntactic diagnostics, without a full type-checking pass
// (nothing here needs one, and a free identifier like `cond` in
// `t(cond ? 'a' : 'b')` must not itself count as invalid). Diagnostics from
// `transpileModule` are less specific than `getSyntacticDiagnostics` off a
// real `ts.Program` would be — no source-mapped location — but this
// function only ever needs to know THAT parsing failed and WHY, both of
// which `messageText` carries.
function invalidTypeScriptDiagnostic(src: string): string | null {
  const { diagnostics } = ts.transpileModule(src, {
    reportDiagnostics: true,
    compilerOptions: { target: ts.ScriptTarget.Latest, module: ts.ModuleKind.ESNext },
  });
  if (!diagnostics || diagnostics.length === 0) return null;
  return ts.flattenDiagnosticMessageText(diagnostics[0].messageText, '\n');
}

// The `.ts` analogue of `visibleTextOnly`: reduces a plain TypeScript source
// file to only the contents of its string/template literals, minus the four
// machine categories the block comment above names — using `typescript`'s
// own parser rather than a character walk. Malformed source fails loudly,
// the same rule `visibleTextOnly` follows for markup: a syntax error must
// not read as "nothing to report".
function visibleStringLiteralsOnly(src: string): string {
  const invalid = invalidTypeScriptDiagnostic(src);
  if (invalid !== null) throw new Error(`invalid TypeScript source, cannot be scanned: ${invalid}`);
  const sourceFile = ts.createSourceFile('guarded.ts', src, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);

  // Split by UTF-16 code unit, not code point (`[...src]` would), because
  // `ts.Node` positions are UTF-16 offsets — this file's own emoji markers
  // (🔴, ⚠️) are surrogate pairs, and splitting by code point would shift
  // every index after the first one out of alignment with the AST.
  const chars = src.split('');
  const out: string[] = chars.map((c) => (c === '\n' ? '\n' : ' ')); // blanked by default, newlines preserved

  const visit = (node: ts.Node) => {
    if (
      STRING_LIKE_KINDS.has(node.kind) &&
      !isCatalogueKey(node) &&
      !isModuleSpecifier(node) &&
      !isCaseLabel(node) &&
      !isKeyPosition(node) &&
      !isTypePosition(node)
    ) {
      const [from, to] = contentSpan(sourceFile, node);
      for (let i = from; i < to; i++) out[i] = chars[i];
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);

  return out.join('');
}

// The `.ts` counterpart of `latinOffenses`: same allowlist rule
// (`offensesFrom`), a different reduction (`visibleStringLiteralsOnly`).
function tsStringLiteralOffenses(file: string, src: string, list: Allowlisted[] = LATIN_ALLOWLIST): string[] {
  return offensesFrom(file, visibleStringLiteralsOnly(src), list);
}

describe('Svelte hardcode guard', () => {
  const srcRoot = join(dirname(fileURLToPath(import.meta.url)), '..'); // ui/src (ESM-safe, no __dirname)
  const uiRoot = join(srcRoot, '..'); // ui/ — the Vite entry HTML shells live here, one level above src/

  // Was "outside src/i18n": excluding the whole directory let a Cyrillic
  // literal in `shortcut.ts` or `recency.ts` pass unseen. The real boundary
  // is narrower — `catalog.ts` is the one file allowed to hold Cyrillic
  // because it is bilingual by design — so only it (and test fixtures) are
  // excluded now; every other `.ts`/`.svelte` file in the tree, i18n
  // directory included, is scanned. Excluded by its full path within
  // `srcRoot`, not by base name (review, Minor 2): `walk` covers all of
  // `ui/src`, not only `src/i18n`, and a base-name match would exempt any
  // OTHER file anywhere in the tree that happened to share the name
  // `catalog.ts` — which the "strictly stronger" claim below must hold
  // unconditionally, not only in the layout this tree happens to have today.
  //
  // Reads the RAW file text, unconditionally — no comment-stripping step of
  // any kind. An earlier revision ran `.ts` files through a hand-written
  // `stripJsComments` first, so a Ukrainian citation inside a doc comment
  // (`recency.ts:46`, §9.3 D-e) would not have to exclude the whole file. That
  // stripper read a `//` sitting inside a REGEX literal, or inside a nested
  // template literal, as a comment start — external review, P2 — and
  // silently dropped the Cyrillic (or Latin) text after it on the line, on
  // every `.ts` file this sweep reads, not only the one the stripper was
  // written for. `recency.ts` is now reworded to carry no Cyrillic at all
  // (D144: the guard reads comments too), which removes the reason the
  // stripper existed rather than papering over its two holes — this sweep is
  // strictly stronger than the one on `main` that shipped before this PR, a
  // property pinned directly below rather than only argued here in prose.
  it('no Cyrillic literals outside the catalogue', () => {
    const catalogue = join(srcRoot, 'i18n', 'catalog.ts');
    const files = walk(srcRoot)
      .filter((p) => /\.(ts|svelte)$/.test(p) && p !== catalogue && !p.endsWith('.test.ts'));
    expect(cyrillicOffenders(files, (p) => readFileSync(p, 'utf8'))).toEqual([]);
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
  it('the markup reduction keeps a catalogue call and drops everything else', () => {
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

  it('rejects a bare Latin literal the markup reduction would otherwise miss', () => {
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
  <div class="snav wide" data-testid="section-models" role="navigation" type="button">{x}</div>
  <p aria-labelledby="not-ready-note" aria-hidden="true" lang="en">{y}</p>
</main>
`;
    expect(latinOffenses('f.svelte', machine)).toEqual([]);

    // An expression-valued attribute is a catalogue call, not a literal. Read
    // by a regex over the whole tag, `aria-pressed={tab === 'files'}` becomes
    // `aria-pressed="files"` and invents an offender that does not exist.
    const expressions = "<main>\n  <button title={t('x')} aria-pressed={tab === 'files'}>{y}</button>\n</main>\n";
    expect(latinOffenses('f.svelte', expressions)).toEqual([]);
  });

  // Kills: the rule inverted back to an inclusion list of readable attribute
  // names. `heading` is on nobody's list — that is the whole point. Every screen
  // this PR series still has to write mounts components with props like it, and
  // an inclusion list would pass each one in silence until someone remembered
  // to add the name. The exclusion list gets a loud false positive instead.
  it('an attribute nobody classified is scanned, not ignored', () => {
    expect(latinOffenses('f.svelte', '<main>\n  <Models heading="Recent files" />\n</main>\n'))
      .toEqual(['2: Recent', '2: files']);
    expect(latinOffenses('f.svelte', '<main>\n  <Folders caption="Watched folders" summary="None yet" />\n</main>\n'))
      .toEqual(['2: Watched', '2: folders', '2: None', '2: yet']);
    // …while the machine vocabulary stays excluded, by name and by prefix.
    // Each excluded value here carries a Latin run of its own, or the branch
    // that excludes it is never reached and the assertion proves nothing.
    expect(latinOffenses('f.svelte', '<main>\n  <Models class="side pane" data-testid="models-pane" />\n</main>\n'))
      .toEqual([]);
    expect(latinOffenses('f.svelte', '<main>\n  <svg><use xlink:href="#tree-icon" /></svg>\n  <p xml:lang="en" on:click="reset()">{x}</p>\n</main>\n'))
      .toEqual([]);
  });

  // Kills: unquoted attribute values left unscanned. HTML allows `title=Hello`
  // and it renders exactly like the quoted form, so skipping it would be a hole
  // in the rule directly above.
  it('an unquoted attribute value is read too', () => {
    expect(latinOffenses('f.svelte', '<main>\n  <button title=Hello>{x}</button>\n</main>\n'))
      .toEqual(['2: Hello']);
    expect(latinOffenses('f.svelte', '<main>\n  <div class=snav data-testid=pane>{x}</div>\n</main>\n'))
      .toEqual([]);
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

  // External review, P2: this hand-written tokenizer has no notion of a regex
  // literal EITHER, so `//` sitting inside one (`/\/\//`, not inside quotes) is
  // read the same way a real `//` comment would be — everything after it on the
  // line is silently blanked, Latin words included. `stripJsComments` (deleted
  // below) had the identical hole for the Cyrillic sweep; this is its twin for
  // the Latin one, and it is closed the same way: by no longer hand-rolling
  // comment/string detection over raw characters at all.
  it('a `//` inside a regex literal does not swallow the rest of the line', () => {
    const fixture = "export function label(path: string) { return /\\/\\//.test(path) ? 'Yes' : 'No'; }\n";
    expect(tsStringLiteralOffenses('f.ts', fixture)).toEqual(['1: Yes', '1: No']);
  });

  // Same review note: a nested template literal hides text behind an *inner*
  // backtick pair the same way a nested string hides behind an inner quote pair
  // — the outer template's own scan must not stop at the first backtick it
  // meets. `Zebra` sits inside the FALSE branch's own template, one level down
  // from the interpolation the outer template opens.
  it('a nested template literal does not hide the text inside it', () => {
    const fixture = 'export function label(ok: boolean) { return `${ok ? `//` : `Zebra`}`; }\n';
    expect(tsStringLiteralOffenses('f.ts', fixture)).toEqual(['1: Zebra']);
  });

  // Booked blind spot (see the block comment above `visibleStringLiteralsOnly`
  // on `main`): blanking a `t(...)` call's whole argument list hid a hardcoded
  // word sitting in the SECOND argument, the interpolation-values object. Only
  // the first argument — the catalogue key — is the part this sweep must never
  // read as prose.
  it('scans every argument of a `t(...)` call except the first', () => {
    expect(tsStringLiteralOffenses('f.ts', "t('k');\n")).toEqual([]);
    // A key picked by a ternary is still the first argument, at one level
    // down — both branches must stay unreported, the same as a bare literal.
    expect(tsStringLiteralOffenses('f.ts', "t(cond ? 'a' : 'b');\n")).toEqual([]);
    expect(tsStringLiteralOffenses('f.ts', "t('k', { x: 'Zebra' });\n")).toEqual(['1: Zebra']);
  });

  // A plain string that happens to contain `://` must not be confused with a
  // regex, and must not swallow a real offender sitting in a later string on
  // the same line. `http` is suppressed by a fixture-only allowlist entry so
  // the assertion says only what this test is actually about.
  it('a URL-shaped string literal does not hide the next literal on its line', () => {
    const list = [{ file: 'f.ts', text: 'http', reason: 'test fixture, not a real allowlist entry' }];
    expect(tsStringLiteralOffenses('f.ts', "const u = 'http://x'; const z = 'Zebra';\n", list))
      .toEqual(['1: Zebra']);
  });

  // A module specifier is an address, not prose — same rule as `href` in the
  // markup sweep above.
  it('an import module specifier is not scanned', () => {
    expect(tsStringLiteralOffenses('f.ts', "import x from 'zebra-lib';\n")).toEqual([]);
  });

  // A `case` label is a discriminant tag matched against a union, not a
  // sentence — same rule as `role`/`type` in the markup sweep above.
  it('a `case` label is not scanned', () => {
    const fixture = "switch (x) {\n  case 'Zebra': break;\n}\n";
    expect(tsStringLiteralOffenses('f.ts', fixture)).toEqual([]);
  });

  // The AST sees a `/['"]/` regex literal for exactly what it is — a
  // `RegularExpressionLiteral` node this sweep never collects — so the quote
  // inside it is never read as an opening string quote, and nothing after it
  // is lost. An earlier revision asserted the opposite of this (a thrown
  // "unterminated string literal", naming the regex as the likely cause): the
  // failure mode that test pinned cannot occur once string/comment detection
  // is no longer hand-rolled over raw characters, so the fact worth pinning
  // now is the positive one.
  it('a regex literal holding a quote is not an offense and hides nothing after it', () => {
    expect(() => tsStringLiteralOffenses('f.ts', "const RE = /['\"]/; const z = 'Zebra';\n"))
      .not.toThrow();
    expect(tsStringLiteralOffenses('f.ts', "const RE = /['\"]/; const z = 'Zebra';\n")).toEqual(['1: Zebra']);
  });

  // External review, Important 2: the loud throw above is only as trustworthy
  // as the check that feeds it. Pinned at two levels — the outward behaviour
  // (`tsStringLiteralOffenses` throws) AND the detector it is built on
  // (`invalidTypeScriptDiagnostic` itself fires non-null on broken input and
  // stays null on good input) — so a change that makes the detector always
  // agree with one side cannot pass both.
  it('an invalid TypeScript source is refused before any scanning happens', () => {
    expect(() => tsStringLiteralOffenses('f.ts', 'const a = ;\n')).toThrow(/invalid TypeScript source/);
  });

  it('the diagnostic check the refusal is built on actually distinguishes broken from good source', () => {
    expect(invalidTypeScriptDiagnostic('const a = ;\n')).not.toBeNull();
    expect(invalidTypeScriptDiagnostic("const a = 1;\n")).toBeNull();
  });

  // The Cyrillic sweep used to run every `.ts` file through `stripJsComments`
  // first so a citation inside a doc comment would not have to be excluded
  // file-wide. That stripper read a `//` inside a regex literal, or inside a
  // nested template, the same wrong way `visibleStringLiteralsOnly` did above.
  // Driven through `cyrillicOffenders` itself — the exact function the
  // production sweep above calls — over two synthetic files, not through a
  // bare `CYRILLIC.test(...)` on an inline string: a bare regex test cannot
  // notice a stripping step reappearing inside `cyrillicOffenders`, and one
  // reviewer mutant that did exactly that (wrapping `read` in a naive
  // `//`-to-end-of-line stripper) left every other test in this file green.
  it('the sweep itself, not just the regex, catches a `//` inside a regex and inside a nested template', () => {
    const content: Record<string, string> = {
      'a.ts': "export function label(path: string) { return /\\/\\//.test(path) ? 'Так' : 'Ні'; }\n",
      'b.ts': 'export function label(ok: boolean) { return `${ok ? `//` : `Ні`}`; }\n',
    };
    expect(cyrillicOffenders(Object.keys(content), (p) => content[p])).toEqual(['a.ts', 'b.ts']);
  });

  // The `.svelte` sweep above never looks at `.ts` files, so a `.ts` module
  // under `src/i18n` that builds display words of its own — `shortcut.ts` —
  // was invisible to every sweep in this file. `tsStringLiteralOffenses`
  // closes that: same allowlist, a reduction built for plain TypeScript. The
  // catalogue and every `.test.ts` are excluded — the catalogue is
  // intentionally bilingual, and a test's fixtures are not product text.
  it('no unlisted Latin string literals in i18n .ts modules other than the catalogue', () => {
    const offenders = walk(srcRoot)
      .filter((p) => p.includes(join('src', 'i18n')) && p.endsWith('.ts'))
      .filter((p) => basename(p) !== 'catalog.ts' && !p.endsWith('.test.ts'))
      .flatMap((p) => tsStringLiteralOffenses(p, readFileSync(p, 'utf8')).map((o) => `${p}:${o}`));
    expect(offenders).toEqual([]);
  });
});

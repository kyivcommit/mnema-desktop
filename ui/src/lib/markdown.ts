// The answer is model prose, not a document. This parses the four block forms
// it actually emits — ATX headings, both list markers, a fence — and takes
// everything else as a paragraph; links, tables, quotes, rules and HTML stay
// literal text. A single `*` or `_` is never emphasis either: this model writes
// about shell commands, and `ls *.md` has to keep its asterisk.
//
// Anchors are not this layer's business. `<c>N</c>` stays inside the text of a
// span, untouched, and `Answer.svelte` splits it out per span — so a citation
// survives inside a paragraph, a list item and a heading alike, while this file
// stays a pure function with no DOM and no knowledge of citations.

export type Span = { kind: 'text' | 'strong' | 'code'; text: string };

export type Block =
  | { kind: 'h'; spans: Span[] }
  | { kind: 'p'; spans: Span[] }
  | { kind: 'ul' | 'ol'; items: Span[][] }
  | { kind: 'code'; text: string };

const FENCE = /^\s*```/;
const HEADING = /^(#{1,6})\s+(.*)$/;
const BULLET = /^[-*]\s+(.*)$/;
const NUMBERED = /^\d+\.\s+(.*)$/;

function startsBlock(line: string): boolean {
  return FENCE.test(line) || HEADING.test(line) || BULLET.test(line) || NUMBERED.test(line);
}

// `**bold**` and `` `code` ``, and they do not nest — a bold run keeps any
// backticks inside it as text, which is the lesser evil next to a parser that
// eats a shell command.
// shortcut: no nesting of inline marks; upgrade when an answer writes **see `x`**.
const INLINE = /(\*\*[^*]+\*\*|`[^`]+`)/g;

function toSpans(text: string): Span[] {
  const spans: Span[] = [];
  let last = 0;
  for (const match of text.matchAll(INLINE)) {
    const [all] = match;
    const strong = all.startsWith('**');
    if (match.index > last) spans.push({ kind: 'text', text: text.slice(last, match.index) });
    spans.push({ kind: strong ? 'strong' : 'code', text: all.slice(strong ? 2 : 1, strong ? -2 : -1) });
    last = match.index + all.length;
  }
  if (last < text.length) spans.push({ kind: 'text', text: text.slice(last) });
  return spans;
}

export function renderMarkdown(text: string): Block[] {
  const lines = text.replace(/\r\n?/g, '\n').split('\n');
  const blocks: Block[] = [];
  let i = 0;

  while (i < lines.length) {
    if (lines[i].trim() === '') {
      i++;
      continue;
    }

    if (FENCE.test(lines[i])) {
      const body: string[] = [];
      for (i++; i < lines.length && !FENCE.test(lines[i]); i++) body.push(lines[i]);
      i++; // the closing fence, or past the end when the model forgot one
      blocks.push({ kind: 'code', text: body.join('\n') });
      continue;
    }

    const heading = HEADING.exec(lines[i]);
    if (heading) {
      blocks.push({ kind: 'h', spans: toSpans(heading[2].trim()) });
      i++;
      continue;
    }

    const bullet = BULLET.exec(lines[i]);
    const numbered = bullet ? null : NUMBERED.exec(lines[i]);
    if (bullet || numbered) {
      const ordered = numbered !== null;
      const item = ordered ? NUMBERED : BULLET;
      const items: Span[][] = [];
      for (; i < lines.length; i++) {
        const match = item.exec(lines[i]);
        if (!match) break;
        items.push(toSpans(match[1].trim()));
      }
      blocks.push({ kind: ordered ? 'ol' : 'ul', items });
      continue;
    }

    // A paragraph runs to the next blank line or block, and its lines join with
    // a space: a soft line break is a space in markdown, and the block parser
    // now owns the newlines that `white-space: pre-wrap` used to.
    const paragraph: string[] = [];
    for (; i < lines.length && lines[i].trim() !== '' && !startsBlock(lines[i]); i++) {
      paragraph.push(lines[i].trim());
    }
    blocks.push({ kind: 'p', spans: toSpans(paragraph.join(' ')) });
  }

  return blocks;
}

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
  | { kind: 'ul'; items: Span[][] }
  | { kind: 'ol'; start: number; items: Span[][] }
  | { kind: 'code'; text: string };

const FENCE = /^\s*```/;
const HEADING = /^(#{1,6})\s+(.*)$/;
// Up to three spaces of indentation, as markdown allows: a model's list is
// often indented under the sentence before it, and gluing `- item` into that
// sentence shows the raw marker.
const BULLET = /^ {0,3}[-*]\s+(.*)$/;
const NUMBERED = /^ {0,3}(\d+)\.\s+(.*)$/;
// A table row or a quote line: neither is parsed here, both stay literal text,
// so the line keeps its own break instead of running into its neighbour. The
// marker must lead the line — `</c>` would otherwise match every anchored line.
const LITERAL = /^\s*[|>]/;

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
      const open = i;
      const body: string[] = [];
      for (i++; i < lines.length && !FENCE.test(lines[i]); i++) body.push(lines[i]);
      if (i === lines.length) {
        // The model forgot the closing fence. It must not swallow the rest of
        // the answer: the fence ends at the first line carrying an anchor, so
        // every citation after an open fence still renders as a button.
        const anchor = body.findIndex((line) => line.includes('<c>'));
        if (anchor !== -1) body.length = anchor;
        i = open + 1 + body.length;
      } else {
        i++; // past the closing fence
      }
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
      const items: Span[][] = [];
      // A blank line between items does not end the list — models write loose
      // lists — and the first number is the list's `start`, so an interrupted
      // list resumes at the number it was written with.
      const start = numbered ? Number(numbered[1]) : 1;
      for (; i < lines.length; i++) {
        const match = ordered ? NUMBERED.exec(lines[i]) : BULLET.exec(lines[i]);
        if (match) items.push(toSpans((ordered ? match[2] : match[1]).trim()));
        else if (lines[i].trim() !== '') break;
      }
      blocks.push(ordered ? { kind: 'ol', start, items } : { kind: 'ul', items });
      continue;
    }

    // A paragraph runs to the next blank line or block, and its lines join with
    // a space: a soft line break is a space in markdown, and the block parser
    // now owns the newlines that `white-space: pre-wrap` used to. A line left
    // literal — a table row, a quote — keeps its own newline so it still reads
    // as a row, and `pre-wrap` on the paragraph is what draws that break.
    const paragraph: string[] = [];
    for (; i < lines.length && lines[i].trim() !== '' && !startsBlock(lines[i]); i++) {
      paragraph.push(lines[i].trim());
    }
    let text = '';
    let literal = false;
    for (const line of paragraph) {
      if (text !== '') text += literal ? '\n' : ' ';
      text += line;
      literal = LITERAL.test(line);
    }
    blocks.push({ kind: 'p', spans: toSpans(text) });
  }

  return blocks;
}

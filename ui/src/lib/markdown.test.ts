import { expect, test } from 'vitest';

import { renderMarkdown } from './markdown';

// One answer from the live app, kept whole: the shape it has to render is the
// sequence of blocks, not any one line of them.
const TAIL = [
  '## Refinements of follow mode',
  '',
  'Follow mode keeps the file open and reads what arrives at the end<c>1</c>. It is',
  '**not** a replacement for a collector that ships the log.',
  '',
  '- `tail -f app.log` prints appended lines as they arrive<c>3</c>.',
  '- `tail -F app.log` reopens the file after a rotation.',
  '- `tail -n 20 app.log` starts from the last twenty lines.',
  '',
  '1. Start the tail.',
  '2. Filter for the lines that matter.',
  '',
  '### Two caveats',
  '',
  'A file truncated in place is not noticed by `tail -f`<c>7</c>; use `-F` when the',
  'writer rotates.',
  '',
  '```',
  'tail -f app.log | grep WARN',
  '```',
].join('\n');

test('a live tail answer becomes the block sequence it is written as', () => {
  const blocks = renderMarkdown(TAIL);
  expect(blocks.map((b) => b.kind)).toEqual(['h', 'p', 'ul', 'ol', 'h', 'p', 'code']);

  expect(blocks[0]).toEqual({ kind: 'h', spans: [{ kind: 'text', text: 'Refinements of follow mode' }] });

  // Soft newlines join with a space, `**not**` is the only strong span, and the
  // anchor reaches this layer verbatim — it is `Answer.svelte`'s to resolve.
  expect(blocks[1]).toEqual({
    kind: 'p',
    spans: [
      { kind: 'text', text: 'Follow mode keeps the file open and reads what arrives at the end<c>1</c>. It is ' },
      { kind: 'strong', text: 'not' },
      { kind: 'text', text: ' a replacement for a collector that ships the log.' },
    ],
  });

  const list = blocks[2];
  if (list.kind !== 'ul') throw new Error('expected a ul');
  expect(list.items).toHaveLength(3);
  expect(list.items[0]).toEqual([
    { kind: 'code', text: 'tail -f app.log' },
    { kind: 'text', text: ' prints appended lines as they arrive<c>3</c>.' },
  ]);

  const ordered = blocks[3];
  if (ordered.kind !== 'ol') throw new Error('expected an ol');
  expect(ordered.start).toBe(1);
  expect(ordered.items).toEqual([
    [{ kind: 'text', text: 'Start the tail.' }],
    [{ kind: 'text', text: 'Filter for the lines that matter.' }],
  ]);

  expect(blocks[4]).toEqual({ kind: 'h', spans: [{ kind: 'text', text: 'Two caveats' }] });
  expect(blocks[6]).toEqual({ kind: 'code', text: 'tail -f app.log | grep WARN' });
});

test('a single * or _ is text: this model writes about shell commands', () => {
  expect(renderMarkdown('ls *.md — not *emphasis*, and not _this_ either.'))
    .toEqual([{
      kind: 'p',
      spans: [{ kind: 'text', text: 'ls *.md — not *emphasis*, and not _this_ either.' }],
    }]);
});

test('an unterminated ** stays literal rather than swallowing the rest', () => {
  expect(renderMarkdown('A **stray marker and the prose after it.'))
    .toEqual([{ kind: 'p', spans: [{ kind: 'text', text: 'A **stray marker and the prose after it.' }] }]);
});

test('every ATX level is a heading, and a heading ends the paragraph above it', () => {
  expect(renderMarkdown('# one\n## two\n### three\n#### four\n##### five\n###### six').map((b) => b.kind))
    .toEqual(['h', 'h', 'h', 'h', 'h', 'h']);
  expect(renderMarkdown('prose\n## head').map((b) => b.kind)).toEqual(['p', 'h']);
});

test('a fence keeps its contents literal, headings and bullets inside it included', () => {
  expect(renderMarkdown('```\n# not a heading\n- not a bullet\n**not bold**\n```'))
    .toEqual([{ kind: 'code', text: '# not a heading\n- not a bullet\n**not bold**' }]);
});

test('a list ends at a blank line or a non-item, so the prose after it is a paragraph', () => {
  expect(renderMarkdown('- one\n- two\n\nAfter.\n\n1. first\n2. second')).toEqual([
    { kind: 'ul', items: [[{ kind: 'text', text: 'one' }], [{ kind: 'text', text: 'two' }]] },
    { kind: 'p', spans: [{ kind: 'text', text: 'After.' }] },
    { kind: 'ol', start: 1, items: [[{ kind: 'text', text: 'first' }], [{ kind: 'text', text: 'second' }]] },
  ]);
});

// Models write loose lists, and they number the second half of an interrupted
// one: a blank line between items kept the list apart, and every `ol` drew «1.»
// because its first number had nowhere to go.
test('a blank line between items keeps one list, and its first number is its start', () => {
  expect(renderMarkdown('1. a\n\n2. b')).toEqual([{
    kind: 'ol', start: 1,
    items: [[{ kind: 'text', text: 'a' }], [{ kind: 'text', text: 'b' }]],
  }]);
  expect(renderMarkdown('3. c\n4. d')).toEqual([
    { kind: 'ol', start: 3, items: [[{ kind: 'text', text: 'c' }], [{ kind: 'text', text: 'd' }]] },
  ]);
  // Interrupted by prose, the second list resumes at the number it was written
  // with instead of restarting at 1.
  const interrupted = renderMarkdown('1. a\n\nProse.\n\n2. b');
  expect(interrupted.map((b) => b.kind)).toEqual(['ol', 'p', 'ol']);
  expect(interrupted[2]).toEqual({ kind: 'ol', start: 2, items: [[{ kind: 'text', text: 'b' }]] });
});

// Neither form is parsed here, so both stay literal — but a table squashed onto
// one line was a regression: `pre-wrap` used to show its rows.
test('a table row and a quote keep their own line breaks', () => {
  expect(renderMarkdown('| a | b |\n| --- | --- |\n| 1 | 2 |')).toEqual([{
    kind: 'p',
    spans: [{ kind: 'text', text: '| a | b |\n| --- | --- |\n| 1 | 2 |' }],
  }]);
  expect(renderMarkdown('> one\n> two'))
    .toEqual([{ kind: 'p', spans: [{ kind: 'text', text: '> one\n> two' }] }]);
  // Ordinary prose still joins with a space.
  expect(renderMarkdown('one\ntwo'))
    .toEqual([{ kind: 'p', spans: [{ kind: 'text', text: 'one two' }] }]);
});

test('an indented marker starts a list instead of gluing into the prose', () => {
  expect(renderMarkdown('Prose:\n  - item')).toEqual([
    { kind: 'p', spans: [{ kind: 'text', text: 'Prose:' }] },
    { kind: 'ul', items: [[{ kind: 'text', text: 'item' }]] },
  ]);
});

test('an unterminated fence ends at the first anchor, so the rest still renders', () => {
  expect(renderMarkdown('```\ntail -f app.log\n<c>3</c> follows the end.')).toEqual([
    { kind: 'code', text: 'tail -f app.log' },
    { kind: 'p', spans: [{ kind: 'text', text: '<c>3</c> follows the end.' }] },
  ]);
  // A closed fence still wins: its contents are literal, anchor included.
  expect(renderMarkdown('```\necho <c>3</c>\n```'))
    .toEqual([{ kind: 'code', text: 'echo <c>3</c>' }]);
  // With nothing to recover, an open fence runs to the end, as it must.
  expect(renderMarkdown('```\none\ntwo')).toEqual([{ kind: 'code', text: 'one\ntwo' }]);
});

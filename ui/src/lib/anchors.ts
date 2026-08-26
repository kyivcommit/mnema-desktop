export type Segment =
  | { kind: 'text'; text: string }
  | { kind: 'anchor'; n: number };

export function splitAnchors(answer: string, known: ReadonlySet<number>): Segment[] {
  const segments: Segment[] = [];
  const anchors = /<c>(\d+)<\/c>/g;
  let last = 0;

  for (const match of answer.matchAll(anchors)) {
    const n = Number(match[1]);
    if (!known.has(n)) continue;

    const text = answer.slice(last, match.index);
    if (text) segments.push({ kind: 'text', text });
    segments.push({ kind: 'anchor', n });
    last = match.index + match[0].length;
  }

  const text = answer.slice(last);
  if (text || segments.length === 0) segments.push({ kind: 'text', text });

  return segments;
}

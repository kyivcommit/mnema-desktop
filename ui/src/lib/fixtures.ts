import type { AskAnswer, AskCitation, Hit } from './ipc';

const citation: AskCitation = {
  anchor: 1,
  chunkId: 42,
  text: 'A cited passage.',
  relativePath: 'notes/a.md',
  sectionTitle: 'Intro',
  coordinate: { kind: 'line', start: 5, end: 7 },
};

const hit: Hit = {
  chunkId: 7,
  text: 'A bare passage.',
  relativePath: 'notes/b.md',
  sectionTitle: null,
  coordinate: { kind: 'none' },
};

export const generated: AskAnswer = {
  kind: 'generated',
  answer: 'The answer, with a source <c>1</c>.',
  citations: [citation],
  text: { kind: 'answered', matched: 3 },
  content: { kind: 'off' },
};

export const citationsOnly: AskAnswer = {
  kind: 'citationsOnly',
  citations: [hit],
  text: { kind: 'answered', matched: 1 },
  content: { kind: 'noKey' },
};

export const refusedNoCandidates: AskAnswer = {
  kind: 'refused',
  reason: { kind: 'noCandidates' },
  text: { kind: 'answered', matched: 0 },
  content: { kind: 'off' },
};

export const refusedEmptyCompletion: AskAnswer = {
  kind: 'refused',
  reason: { kind: 'emptyCompletion' },
  text: { kind: 'answered', matched: 2 },
  content: { kind: 'off' },
};

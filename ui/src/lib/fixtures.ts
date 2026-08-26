import type { AskAnswer, AskCitation, Hit } from './ipc';

// Anchors 3 and 7, NOT 1 and 2: `anchor` is the model's own ordinal into
// `hits`, not a position in `citations` (PR 6 plan, Decision 5) — a fixture
// with contiguous anchors starting at 1 cannot tell a correct
// `citations.find(c => c.anchor === n)` lookup from the wrong
// `citations[n - 1]`.
//
// `relativePath: 'notes/a.md'` is kept as-is on purpose — do not change it.
const citationA: AskCitation = {
  anchor: 3,
  chunkId: 42,
  ord: 0,
  documentId: 'doc-1',
  rootId: 7,
  text: 'A cited passage.',
  relativePath: 'notes/a.md',
  sectionTitle: 'Intro',
  coordinate: { kind: 'line', start: 5, end: 7 },
};

// Same document as citationA — both citations land in the same paragraph
// (mockup:282-283), so a sibling filter keyed on documentId has something to
// keep. No coordinate, so the preview label's second branch (a path with no
// verifiable location) is exercised too.
const citationB: AskCitation = {
  anchor: 7,
  chunkId: 43,
  ord: 1,
  documentId: 'doc-1',
  rootId: 7,
  text: 'A second cited passage.',
  relativePath: 'notes/a.md',
  sectionTitle: null,
  coordinate: { kind: 'none' },
};

export const generated: AskAnswer = {
  kind: 'generated',
  // Both anchors appear in the prose, non-contiguous — a two-anchor test that
  // used <c>1</c><c>2</c> could not fail against citations[n-1].
  answer: 'Costs four hryvnias<c>3</c> and the total cannot exceed the cap<c>7</c>.',
  citations: [citationA, citationB],
  text: { kind: 'answered', matched: 3 },
  content: { kind: 'off' },
};

// Preview-label branch 2 (Decision 1): no path on disk, but a real location.
// The rule keeps the location — a citation with no `path` row of its own
// still knows it is on page 12 — where the vanilla `hitLocation` would have
// thrown it away. "Archived" here is illustrative, not a proven cause:
// `schema.sql:66`'s `parent_document_id` models archive membership, but
// nothing in Rust writes that column yet, so this stands for any document
// with no path row, not specifically an archive member.
export const generatedArchived: AskAnswer = {
  kind: 'generated',
  answer: 'An archived source says so<c>1</c>.',
  citations: [
    {
      anchor: 1,
      chunkId: 50,
      ord: 0,
      documentId: 'doc-3',
      rootId: null,
      text: 'A passage from an archive member.',
      relativePath: null,
      sectionTitle: null,
      coordinate: { kind: 'page', number: 12 },
    },
  ],
  text: { kind: 'answered', matched: 1 },
  content: { kind: 'off' },
};

// Preview-label branch 3 (Decision 1): neither a path nor a coordinate — the
// only branch that shows the "no path on disk" string.
export const generatedNoPath: AskAnswer = {
  kind: 'generated',
  answer: 'Nothing locates this passage<c>1</c>.',
  citations: [
    {
      anchor: 1,
      chunkId: 51,
      ord: 0,
      documentId: 'doc-4',
      rootId: null,
      text: 'A passage with no recorded location at all.',
      relativePath: null,
      sectionTitle: null,
      coordinate: { kind: 'none' },
    },
  ],
  text: { kind: 'answered', matched: 1 },
  content: { kind: 'off' },
};

const hit: Hit = {
  chunkId: 7,
  ord: 0,
  documentId: 'doc-1',
  rootId: 7,
  text: 'A bare passage.',
  relativePath: 'notes/a.md',
  sectionTitle: null,
  coordinate: { kind: 'none' },
};

// A different document entirely (mockup:368-369) — a sibling filter that
// forgets to compare documentId is visibly wrong against this fixture.
const hitOtherDocument: Hit = {
  chunkId: 9,
  ord: 0,
  documentId: 'doc-2',
  rootId: 7,
  text: 'Another file entirely.',
  relativePath: 'notes/b.md',
  sectionTitle: null,
  coordinate: { kind: 'page', number: 2 },
};

export const citationsOnly: AskAnswer = {
  kind: 'citationsOnly',
  citations: [hit, hitOtherDocument],
  text: { kind: 'answered', matched: 2 },
  content: { kind: 'noKey' },
};

// Zero hits is an answer, not the absence of one (the vanilla `searchResultItems`
// rule this plan re-homes to state E).
export const emptyCitationsOnly: AskAnswer = {
  kind: 'citationsOnly',
  citations: [],
  text: { kind: 'answered', matched: 0 },
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

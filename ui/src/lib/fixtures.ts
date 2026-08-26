import type { AskAnswer, AskCitation, Hit, TreeListing } from './ipc';

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

// ---------------------------------------------------------------------------
// Tree fixtures (Task 7). Folder names are Latin on purpose (Ruling K): this
// file is a `.ts` under `ui/src` outside `src/i18n`, so `i18n/guard.test.ts`
// reads it and a Cyrillic folder name here would turn `npm test` red with a
// message about hardcoded strings that says nothing about trees. A non-ASCII
// folder under test lives inside `Tree.test.ts`, where the guard does not look.

// One root, two folders. `notes/a.md` is `doc-1` so the citation fixtures
// above (`citationA`/`citationB`, both `doc-1` at `notes/a.md`) point into
// this listing. Task 8b consumes this fixture and requires these two names.
export const oneRootTwoFolders: TreeListing = {
  roots: [
    {
      rootId: 1,
      absolutePath: '/home/u/docs',
      name: 'docs',
      files: [
        { relativePath: 'notes/a.md', documentId: 'doc-1' },
        { relativePath: 'notes/b.md', documentId: 'doc-2' },
        { relativePath: 'archive/old.md', documentId: 'doc-3' },
      ],
    },
  ],
  recents: [
    { documentId: 'doc-1', rootId: 1, relativePath: 'notes/a.md', indexedAt: 1_700_000_100 },
    { documentId: 'doc-3', rootId: 1, relativePath: 'archive/old.md', indexedAt: 1_700_000_000 },
  ],
};

// Two roots holding the SAME relative path under different documentIds. A card
// that keys its selection on the path string cannot tell these two apart;
// keying on documentId (Ruling P) can.
export const twoRootsSameRelativePath: TreeListing = {
  roots: [
    { rootId: 1, absolutePath: '/home/u/alpha', name: 'alpha', files: [{ relativePath: 'README.md', documentId: 'doc-a' }] },
    { rootId: 2, absolutePath: '/home/u/beta', name: 'beta', files: [{ relativePath: 'README.md', documentId: 'doc-b' }] },
  ],
  recents: [],
};

// A file at a root's top level (no `/` at all — `split('/')` returns one
// element and no folder is created) beside a folder two levels deep, where the
// nesting has to recurse into itself. Neither shape exists in the fixtures
// above, and the folder builder branches on both.
export const oneRootMixedDepths: TreeListing = {
  roots: [
    {
      rootId: 4,
      absolutePath: '/home/u/mixed',
      name: 'mixed',
      files: [
        { relativePath: 'README.md', documentId: 'doc-r' },
        { relativePath: 'a/b/c.md', documentId: 'doc-c' },
        { relativePath: 'a/d.md', documentId: 'doc-d' },
      ],
    },
  ],
  recents: [],
};

// A single root holding a single top-level file. The negative selection test
// (a citation whose document is gone) and its positive control (Ruling J) both
// render this: the row is on screen either way, so only `aria-current` differs
// between them and neither test can be satisfied by an empty tree.
export const oneRoot: TreeListing = {
  roots: [{ rootId: 3, absolutePath: '/home/u/solo', name: 'solo', files: [{ relativePath: 'a.md', documentId: 'doc-1' }] }],
  recents: [],
};

// ONE document named from TWO roots — the same `documentId` under both, which
// `mnema-index`'s `delete_watched_root` (write.rs:700-722) exists to handle: a
// document survives the deletion of one root when another still names it, so
// `path` rows are per (document, root) and two are legitimate. The tree renders
// one row per path, so `tree-file-{documentId}` is not unique in this state and
// more than one row can be current at once. No other fixture builds it, and the
// selection code branches per row.
export const oneDocumentTwoRoots: TreeListing = {
  roots: [
    {
      rootId: 1,
      absolutePath: '/home/u/alpha',
      name: 'alpha',
      files: [
        { relativePath: 'notes/shared.md', documentId: 'doc-shared' },
        { relativePath: 'notes/other.md', documentId: 'doc-other' },
      ],
    },
    {
      rootId: 2,
      absolutePath: '/home/u/beta',
      name: 'beta',
      files: [{ relativePath: 'notes/shared.md', documentId: 'doc-shared' }],
    },
  ],
  recents: [],
};

// Empty but SUCCESSFUL (Ruling N): nothing indexed is not the same event as a
// listing that could not be read, and the card must not say the same thing.
export const emptyListing: TreeListing = { roots: [], recents: [] };

import type { AskAnswer, AskCitation, Hit, SourceAround, SourceBlock, TreeListing } from './ipc';

// Anchors 3 and 7, NOT 1 and 2: `anchor` is the model's own ordinal into
// `hits`, not a position in `citations` (PR 6 plan, Decision 5) — a fixture
// with contiguous anchors starting at 1 cannot tell a correct
// `citations.find(c => c.anchor === n)` lookup from the wrong
// `citations[n - 1]`.
//
// `relativePath: 'notes/a.md'` is kept as-is on purpose — do not change it.
export const citationA: AskCitation = {
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
export const citationB: AskCitation = {
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

// The SECOND answer (Task 8b), with a citation of its OWN — deliberately not a
// copy of `citationA`/`citationB`. A second answer that shared the first's
// citations could not show a stale selection surviving: the previous citation
// would still be a member of the new answer's list, so a card still painting it
// would look correct. `doc-2` also has a row in `oneRootTwoFolders`, so the tree
// can mark it. Latin only, like every other string in this file (Ruling K).
export const generatedOther: AskAnswer = {
  kind: 'generated',
  answer: 'A second answer entirely<c>2</c>.',
  citations: [
    {
      anchor: 2,
      chunkId: 61,
      ord: 0,
      documentId: 'doc-2',
      rootId: 7,
      text: 'A passage the second answer cites.',
      relativePath: 'notes/b.md',
      sectionTitle: null,
      coordinate: { kind: 'line', start: 1, end: 2 },
    },
  ],
  text: { kind: 'answered', matched: 1 },
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
export const hitOtherDocument: Hit = {
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

// 🔴 Task 9's fixture question. `citationsOnly` above holds its two passages in
// TWO documents, so Ruling U's sibling filter drops the other one before any
// call and a state E click can only ever produce ONE round trip — the branch
// where a passage HAS a sibling in its own document is a state nothing built.
// `chunkId` 8 and `ord` 1 keep it a distinct occurrence from `hit`
// (`Source.svelte`'s `occurrence` key is documentId + ord + chunkId), and both
// rows label from `notes/a.md` so the ranks cannot be told apart by their text.
const hitSameDocument: Hit = {
  chunkId: 8,
  ord: 1,
  documentId: 'doc-1',
  rootId: 7,
  text: 'A second bare passage from the same file.',
  relativePath: 'notes/a.md',
  sectionTitle: null,
  coordinate: { kind: 'none' },
};

// Re-review RM1: ONE passage. `citationsOnly` has two and `emptyCitationsOnly`
// has none, so the singular was a state nothing built and the banner's plural
// over a one-item list had to be reasoned about rather than rendered. Ukrainian
// makes it a grammar question, not a style one, and `ASK_TOP_K` is 8
// (`bridge.rs:496`) so `one` (1), `few` (2-4) and `many` (5-8) are all states a
// person can reach; the arms themselves are pinned in `i18n.test.ts`, where a
// count needs no fixture at all.
export const citationsOnlyOne: AskAnswer = {
  kind: 'citationsOnly',
  citations: [hitOtherDocument],
  text: { kind: 'answered', matched: 1 },
  content: { kind: 'noKey' },
};

export const citationsOnlySameDocument: AskAnswer = {
  kind: 'citationsOnly',
  citations: [hit, hitSameDocument],
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

// ---------------------------------------------------------------------------
// Source fixtures (Task 8). Block text is Latin for the same reason the folder
// names above are (Ruling K/T): `i18n/guard.test.ts:19-21` reads every `.ts`
// under `src/` outside `src/i18n` that is not a `.test.ts`, and this file is
// one of them. The astral-prefix block, whose expected highlight is Cyrillic,
// therefore lives inside `Source.test.ts`, where the guard does not look.
//
// 🔴 The span numbers below are the whole point of this fixture set
// (`src-tauri/src/tree.rs:265-278`): `blockStart` is the offset into the BLOCK
// and is where the highlight begins; `start`/`end` index the CHUNK's own text,
// which never reaches the wire, and only their difference is ever used. Every
// span here keeps `start !== blockStart` on purpose — with the two equal, a
// suite cannot tell `slice(blockStart, blockStart + (end - start))` from the
// wrong `slice(start, end)` and both arithmetics look correct forever.

type Excerpt = Extract<SourceAround, { kind: 'excerpt' }>;

/** The block both cited passages land in — the same paragraph (mockup:282-283). */
export const SHARED_BLOCK_ID = 11;
/** A second block inside the clicked window, for spans that merge with no clicked span. */
export const SECOND_BLOCK_ID = 12;
/** A block the clicked excerpt's window does NOT contain. */
export const OUTSIDE_BLOCK_ID = 99;

const SHARED_BLOCK_TEXT = 'The digitisation price is fixed, and the archive fee is separate.';
const SECOND_BLOCK_TEXT = 'Following paragraph about the archive fee schedule.';

/** Code points [4, 22) of the shared block — what `excerptSpanA` paints. */
export const SPAN_A_TEXT = 'digitisation price';
/** Code points [41, 52) of the same block, far from span A so the two do not merge. */
export const SPAN_B_TEXT = 'archive fee';

const block = (blockId: number, text: string): SourceBlock => ({
  blockId,
  kind: 'paragraph',
  text,
  pageNo: 1,
  readingOrder: blockId,
});

// The clicked citation's own window: three blocks in reading order, one span.
// `hasMoreBefore` and `hasMoreAfter` DISAGREE — they are not the same flag, and
// a fixture that set them equal would satisfy an ellipsis assertion either way.
export const excerptSpanA: Excerpt = {
  kind: 'excerpt',
  blocks: [
    block(10, 'Preceding paragraph with no highlight.'),
    block(SHARED_BLOCK_ID, SHARED_BLOCK_TEXT),
    block(SECOND_BLOCK_ID, SECOND_BLOCK_TEXT),
  ],
  spans: [{ blockId: SHARED_BLOCK_ID, start: 12, end: 30, blockStart: 4 }], // len 18 → [4, 22)
  documentId: 'doc-1',
  sectionTitle: 'Intro',
  hasMoreBefore: true,
  hasMoreAfter: false,
  freshness: { kind: 'current' },
};

// The sibling's own round trip: a DIFFERENT window (block 13 is not in the
// clicked one) and the opposite `hasMore*` flags, so an implementation that
// took the sibling's window or ORed its flags is visibly wrong.
export const excerptSpanB: Excerpt = {
  kind: 'excerpt',
  blocks: [
    block(SHARED_BLOCK_ID, SHARED_BLOCK_TEXT),
    block(SECOND_BLOCK_ID, SECOND_BLOCK_TEXT),
    block(13, 'A later paragraph the clicked window does not reach.'),
  ],
  spans: [{ blockId: SHARED_BLOCK_ID, start: 7, end: 18, blockStart: 41 }], // len 11 → [41, 52)
  documentId: 'doc-1',
  sectionTitle: 'Intro',
  hasMoreBefore: false,
  hasMoreAfter: true,
  freshness: { kind: 'current' },
};

// The window for a passage in `doc-2` (`hitOtherDocument`, chunk 9). Every
// other excerpt fixture here is `doc-1`, and `Source`'s M2 check compares the
// excerpt's `documentId` with the CLICKED passage's — so a `doc-1` excerpt
// answered for a `doc-2` click renders the mismatch badge and no text at all,
// and a test written on one could not tell that apart from a working card.
export const excerptDocTwo: Excerpt = {
  kind: 'excerpt',
  blocks: [block(20, 'A paragraph from the second file entirely.')],
  spans: [{ blockId: 20, start: 6, end: 15, blockStart: 2 }], // len 9 → [2, 11)
  documentId: 'doc-2',
  sectionTitle: null,
  hasMoreBefore: false,
  hasMoreAfter: true,
  freshness: { kind: 'current' },
};

// A sibling whose only span names a block the clicked window does not hold.
// Same `documentId` on purpose: if it disagreed, Ruling U's filter would drop
// it first and the window test would pass for the wrong reason.
export const excerptInAnotherBlock: Excerpt = {
  kind: 'excerpt',
  blocks: [block(OUTSIDE_BLOCK_ID, 'A paragraph outside the clicked window.')],
  spans: [{ blockId: OUTSIDE_BLOCK_ID, start: 3, end: 12, blockStart: 2 }],
  documentId: 'doc-1',
  sectionTitle: null,
  hasMoreBefore: false,
  hasMoreAfter: true,
  freshness: { kind: 'current' },
};

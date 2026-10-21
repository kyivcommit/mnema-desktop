import type {
  AskAnswer, AskCitation, Hit, HistoryEntry, ModelSettings, TreeListing,
} from './ipc';

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
  via: 'search',
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
  via: 'search',
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
  via: 'search',
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
  via: 'search',
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
  why: { kind: 'notAsked' },
  citations: [hit, hitOtherDocument],
  text: { kind: 'answered', matched: 2 },
  content: { kind: 'noKey' },
};

// 🔴 Task 9's fixture question. `citationsOnly` above holds its two passages in
// TWO documents, so Ruling U's sibling filter drops the other one before any
// call and a state E click can only ever produce ONE round trip — the branch
// where a passage HAS a sibling in its own document is a state nothing built.
// `chunkId` 8 and `ord` 1 keep it a distinct occurrence from `hit`
// (documentId + ord + chunkId is what names an occurrence on the wire), and
// both rows label from `notes/a.md` so the ranks cannot be told apart by their
// text.
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
  why: { kind: 'notAsked' },
  citations: [hitOtherDocument],
  text: { kind: 'answered', matched: 1 },
  content: { kind: 'noKey' },
};

export const citationsOnlySameDocument: AskAnswer = {
  kind: 'citationsOnly',
  why: { kind: 'notAsked' },
  citations: [hit, hitSameDocument],
  text: { kind: 'answered', matched: 2 },
  content: { kind: 'noKey' },
};

// Zero hits is an answer, not the absence of one (the vanilla `searchResultItems`
// rule this plan re-homes to state E).
export const emptyCitationsOnly: AskAnswer = {
  kind: 'citationsOnly',
  why: { kind: 'notAsked' },
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
      absolutePath: '/home/u/docs', searched: [],
      name: 'docs',
      files: [
        { relativePath: 'notes/a.md', documentId: 'doc-1' },
        { relativePath: 'notes/b.md', documentId: 'doc-2' },
        { relativePath: 'archive/old.md', documentId: 'doc-3' },
      ],
    },
  ],
};

// Two roots holding the SAME relative path under different documentIds. A card
// that keys its selection on the path string cannot tell these two apart;
// keying on documentId (Ruling P) can.
export const twoRootsSameRelativePath: TreeListing = {
  roots: [
    { rootId: 1, absolutePath: '/home/u/alpha', searched: [], name: 'alpha', files: [{ relativePath: 'README.md', documentId: 'doc-a' }] },
    { rootId: 2, absolutePath: '/home/u/beta', searched: [], name: 'beta', files: [{ relativePath: 'README.md', documentId: 'doc-b' }] },
  ],
};

// A file at a root's top level (no `/` at all — `split('/')` returns one
// element and no folder is created) beside a folder two levels deep, where the
// nesting has to recurse into itself. Neither shape exists in the fixtures
// above, and the folder builder branches on both.
export const oneRootMixedDepths: TreeListing = {
  roots: [
    {
      rootId: 4,
      absolutePath: '/home/u/mixed', searched: [],
      name: 'mixed',
      files: [
        { relativePath: 'README.md', documentId: 'doc-r' },
        { relativePath: 'a/b/c.md', documentId: 'doc-c' },
        { relativePath: 'a/d.md', documentId: 'doc-d' },
      ],
    },
  ],
};

// A single root holding a single top-level file. The negative selection test
// (a citation whose document is gone) and its positive control (Ruling J) both
// render this: the row is on screen either way, so only `aria-current` differs
// between them and neither test can be satisfied by an empty tree.
export const oneRoot: TreeListing = {
  roots: [{ rootId: 3, absolutePath: '/home/u/solo', searched: [], name: 'solo', files: [{ relativePath: 'a.md', documentId: 'doc-1' }] }],
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
      absolutePath: '/home/u/alpha', searched: [],
      name: 'alpha',
      files: [
        { relativePath: 'notes/shared.md', documentId: 'doc-shared' },
        { relativePath: 'notes/other.md', documentId: 'doc-other' },
      ],
    },
    {
      rootId: 2,
      absolutePath: '/home/u/beta', searched: [],
      name: 'beta',
      files: [{ relativePath: 'notes/shared.md', documentId: 'doc-shared' }],
    },
  ],
};

// Empty but SUCCESSFUL (Ruling N): nothing indexed is not the same event as a
// listing that could not be read, and the card must not say the same thing.
export const emptyListing: TreeListing = { roots: [] };

// The Recents tab's list, as `list_history` answers it: newest first, `askedAt`
// in seconds and 100 s apart, so a pinned clock puts the rows in two different
// minutes. Latin text on purpose, the same reason as the folder names above.
export const historyTwoQuestions: HistoryEntry[] = [
  { id: 2, query: 'when does the lease renew', askedAt: 1_700_000_100 },
  { id: 1, query: 'invoice payment terms', askedAt: 1_700_000_000 },
];

// The two `ModelSettings` fields added with provider-per-role, for test
// fixtures that do not care about them: both roles on OpenRouter, key only.
export const ROLE_FIELDS: Pick<ModelSettings, 'roles' | 'configured'> = {
  roles: { embed: 'openRouter', chat: 'openRouter' },
  configured: { openRouter: true, mnemaEmbed: false, mnemaChat: false },
};

// Settings on which the backend's own provider probe may say "ok" while the role is not set up.
// Read by the settings tab dots and by the launcher's cloud, which must agree on both (`roleReady`).
const AGREE_INDEX = { kind: 'read' as const, failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null, embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 6, searchTextArm: true, searchContentArm: false };
export const READINESS_FIXTURES: { name: string; settings: ModelSettings; embedReady: boolean; chatReady: boolean }[] = [
  { name: 'ready', embedReady: true, chatReady: true, settings: {
    key: { kind: 'present' }, platform: 'linux', ...ROLE_FIELDS,
    index: { ...AGREE_INDEX, embeddingModel: 'emb', chatModel: 'chat' } } },
  { name: 'Mnema embed downloaded, no active space', embedReady: false, chatReady: true, settings: {
    key: { kind: 'present' }, platform: 'linux',
    roles: { embed: 'mnema', chat: 'openRouter' }, configured: { openRouter: true, mnemaEmbed: true, mnemaChat: false },
    index: { ...AGREE_INDEX, embeddingModel: null, chatModel: 'chat' } } },
  { name: 'OpenRouter chat with a key, no chat model', embedReady: true, chatReady: false, settings: {
    key: { kind: 'present' }, platform: 'linux', ...ROLE_FIELDS,
    index: { ...AGREE_INDEX, embeddingModel: 'emb', chatModel: null } } },
];

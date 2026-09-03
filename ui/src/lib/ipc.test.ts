import { expect, test, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as ipc from './ipc';
import { camelOf, rustEnumVariants } from './rust-enum';
import type {
  AppPrefs,
  AutostartState,
  HotkeyState,
  HotkeyStatus,
  IndexSettings,
  SourceAround,
  StoredExclusion,
  SubfolderListing,
  SubfolderState,
} from './ipc';
import {
  generated,
  generatedArchived,
  generatedNoPath,
  citationsOnly,
  emptyCitationsOnly,
  refusedNoCandidates,
  refusedEmptyCompletion,
} from './fixtures';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...a: unknown[]) => invoke(...a),
  // Tauri's `Channel` as far as this module uses it: something with an
  // `onmessage` the runtime calls. Declared inside the factory because
  // `vi.mock` is hoisted above every binding in this file.
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

test('listTree invokes list_tree', async () => {
  invoke.mockResolvedValue({ roots: [], recents: [] });

  await ipc.listTree();

  expect(invoke).toHaveBeenCalledWith('list_tree');
});

// PR 7 Task 4: the two model-settings commands `Models.svelte` calls.
test('setKey invokes set_key with the typed key', async () => {
  invoke.mockResolvedValue({ balance: { kind: 'notStated' } });

  await ipc.setKey('a-key-value');

  expect(invoke).toHaveBeenCalledWith('set_key', { key: 'a-key-value' });
});

// PR 7 Task 8: the whole event crosses, not one field of it. The mutation this
// kills is the shape the module used to have — an `onmessage` that read `event`
// and threw the ending's contents away, which left every reason, count and
// frozen prefix unavailable to whatever drew the screen.
const ENDED_PAYLOAD = {
  reason: 'volumeMissing', done: 11, total: 11, skipped: 5, complete: true, frozen: [],
  indexed: 5, unchanged: 1, refused: 0, removed: 4, message: null,
} as const;
const PROGRESS_PAYLOAD = { done: 3, total: 8, skipped: 1, refused: 0, contended: 0, secondsLeft: null } as const;

test('startEmbedJob forwards every job event, whole, and takes no root', async () => {
  invoke.mockResolvedValue(undefined);
  const seen: unknown[] = [];

  await ipc.startEmbedJob((e) => seen.push(e));

  const call = invoke.mock.calls.at(-1) as [string, { onProgress: { onmessage: (m: unknown) => void } }];
  expect(call[0]).toBe('start_embed_job');
  // The pass covers the whole index: a root id here would be a promise it
  // cannot keep (embed_job.rs).
  expect(Object.keys(call[1])).toEqual(['onProgress']);
  const channel = call[1].onProgress;
  expect(typeof channel.onmessage).toBe('function');

  channel.onmessage({ event: 'progress', data: PROGRESS_PAYLOAD });
  channel.onmessage({ event: 'ended', data: ENDED_PAYLOAD });

  expect(seen).toEqual([
    { event: 'progress', data: PROGRESS_PAYLOAD },
    { event: 'ended', data: ENDED_PAYLOAD },
  ]);
});

test('startWalkJob sends the root id it was given and forwards the whole event', async () => {
  invoke.mockResolvedValue(undefined);
  const seen: unknown[] = [];

  await ipc.startWalkJob(42, (e) => seen.push(e));

  const call = invoke.mock.calls.at(-1) as [string, { rootId: number; onProgress: { onmessage: (m: unknown) => void } }];
  expect(call[0]).toBe('start_walk_job');
  expect(call[1].rootId).toBe(42);
  call[1].onProgress.onmessage({ event: 'ended', data: ENDED_PAYLOAD });
  expect(seen).toEqual([{ event: 'ended', data: ENDED_PAYLOAD }]);
});

// Both directions on the one thing this command must not need: a channel.
test('cancelJob invokes cancel_job with no arguments and no channel', async () => {
  invoke.mockResolvedValue(undefined);

  await ipc.cancelJob();

  expect(invoke).toHaveBeenCalledWith('cancel_job');
  expect(invoke.mock.calls.at(-1)).toHaveLength(1);
});

test('forgetKey invokes forget_key with no arguments', async () => {
  invoke.mockResolvedValue({ kind: 'removed' });

  await ipc.forgetKey();

  expect(invoke).toHaveBeenCalledWith('forget_key');
});

test('sourceAround echoes the whole identity, not just the id', async () => {
  if (generated.kind !== 'generated') throw new Error('fixture drifted');
  invoke.mockResolvedValue({ kind: 'gone', reason: { kind: 'noSuchChunk' } });

  await ipc.sourceAround(generated.citations[0]);

  expect(invoke).toHaveBeenCalledWith('source_around', {
    chunkId: 42, passageText: 'A cited passage.',
    citedDocumentId: 'doc-1', citedOrd: 0, citedRootId: 7,
    citedRelativePath: 'notes/a.md', radius: 3,
  });
});

test('SourceAround rejects Rust snake_case wire fields', () => {
  const source: SourceAround = {
    kind: 'excerpt', blocks: [], spans: [], documentId: 'doc-1', sectionTitle: null,
    hasMoreBefore: false, hasMoreAfter: false, freshness: { kind: 'current' },
    // @ts-expect-error TypeScript must reject Rust's pre-serialization spelling.
    has_more_before: false,
  };

  expect(source.kind).toBe('excerpt');
});

test('the fixtures carry the pinned AskAnswer tags', () => {
  expect(generated.kind).toBe('generated');
  expect(citationsOnly.kind).toBe('citationsOnly');
  expect(refusedNoCandidates.kind).toBe('refused');
  expect(citationsOnly.kind).not.toBe('generated'); // a wrong tag would fail the mapping in state.ts
});

test('a refused fixture nests its reason under `reason`', () => {
  if (refusedNoCandidates.kind !== 'refused') throw new Error('fixture drifted');
  expect(refusedNoCandidates.reason.kind).toBe('noCandidates');
  expect(refusedEmptyCompletion.kind === 'refused' && refusedEmptyCompletion.reason.kind).toBe('emptyCompletion');
});

test('a citationsOnly fixture carries Hit-shaped citations with a snake_case coordinate', () => {
  if (citationsOnly.kind !== 'citationsOnly') throw new Error('fixture drifted');
  expect(citationsOnly.citations[0].chunkId).toBe(7);
  expect(citationsOnly.citations[0].coordinate.kind).toBe('none');
});

// PR 6a: the citation's occurrence identity (documentId/ord/rootId), and the
// fixture shapes that make a wrong reader of them fail rather than pass by
// accident.
test('generated carries two anchors that are NOT the index of their citation', () => {
  if (generated.kind !== 'generated') throw new Error('fixture drifted');
  // Non-contiguous on purpose: a lookup written as `citations[n - 1]` instead
  // of `citations.find(c => c.anchor === n)` must not be able to pass this.
  expect(generated.citations.map((c) => c.anchor)).toEqual([3, 7]);
});

test('generated citations carry documentId/ord/rootId, not just a chunk id', () => {
  if (generated.kind !== 'generated') throw new Error('fixture drifted');
  expect(generated.citations[0]).toMatchObject({ documentId: 'doc-1', ord: 0, rootId: 7 });
  // Same document, different ord: the intra-document duplicate the ord field
  // exists to distinguish.
  expect(generated.citations[1]).toMatchObject({ documentId: 'doc-1', ord: 1, rootId: 7 });
});

test('a citationsOnly answer can cite two different documents', () => {
  if (citationsOnly.kind !== 'citationsOnly') throw new Error('fixture drifted');
  expect(citationsOnly.citations[0].documentId).toBe('doc-1');
  expect(citationsOnly.citations[1].documentId).toBe('doc-2');
  expect(citationsOnly.citations[0].documentId).not.toBe(citationsOnly.citations[1].documentId);
});

test('zero hits is an answer, not the absence of one', () => {
  if (emptyCitationsOnly.kind !== 'citationsOnly') throw new Error('fixture drifted');
  expect(emptyCitationsOnly.citations).toEqual([]);
});

// The preview label's three branches (Decision 1): a coordinate with no path,
// a path with no coordinate — the middle branch a fixture set forgets — and
// neither.
test('generatedArchived has a real location but no path on disk', () => {
  if (generatedArchived.kind !== 'generated') throw new Error('fixture drifted');
  const c = generatedArchived.citations[0];
  expect(c.relativePath).toBeNull();
  expect(c.coordinate).toEqual({ kind: 'page', number: 12 });
  expect(c.rootId).toBeNull(); // no path on disk: unresolvable to a root
});

test('generatedNoPath has neither a path nor a coordinate', () => {
  if (generatedNoPath.kind !== 'generated') throw new Error('fixture drifted');
  const c = generatedNoPath.citations[0];
  expect(c.relativePath).toBeNull();
  expect(c.coordinate).toEqual({ kind: 'none' });
});

// PR 8a Task 5: the four commands the folder row's expansion calls.
//
// Argument NAMES, not just values. Tauri matches a command's parameters by
// name, so `relative_path` here would reach the shell as a missing argument and
// be rejected — a failure no assertion about the returned shape would see.
test('listSubfolders names its arguments the way the command declares them', async () => {
  invoke.mockResolvedValue({ entries: [], unnameable: 0 });

  await ipc.listSubfolders(7, 'Work/private');

  expect(invoke).toHaveBeenCalledWith('list_subfolders', { rootId: 7, relativePath: 'Work/private' });
});

// The root's own level is `''`, not a missing argument: `list_subfolders`
// validates the path it is given, and `WalkRules::check_prefix("")` is the one
// empty string it accepts (`crates/mnema-walk/src/rules.rs:554-556`).
test('listSubfolders asks for the root level with an empty path, not without one', async () => {
  invoke.mockResolvedValue({ entries: [], unnameable: 0 });

  await ipc.listSubfolders(1, '');

  expect(invoke).toHaveBeenCalledWith('list_subfolders', { rootId: 1, relativePath: '' });
});

test('listSubfolders returns the listing whole — the state tag and its payload included', async () => {
  const listing: SubfolderListing = {
    entries: [
      { name: 'private', relativePath: 'Work/private', state: { kind: 'excludedByAncestor', prefix: 'Work' } },
      { name: 'notes', relativePath: 'Work/notes', state: { kind: 'open' } },
    ],
    unnameable: 2,
  };
  invoke.mockResolvedValue(listing);

  await expect(ipc.listSubfolders(7, 'Work')).resolves.toEqual(listing);
});

test('listExclusions takes the root id and nothing else', async () => {
  invoke.mockResolvedValue([]);

  await ipc.listExclusions(4);

  expect(invoke).toHaveBeenCalledWith('list_exclusions', { rootId: 4 });
  // Both directions: the assertion above is satisfied by a call carrying a
  // second argument nobody looked at, and a stray `relativePath` here would be
  // a rule prefix sent to a command that does not take one.
  expect(Object.keys((invoke.mock.calls.at(-1) as [string, object])[1])).toEqual(['rootId']);
});

test('listExclusions returns each stored rule with its own existsOnDisk', async () => {
  const rules: StoredExclusion[] = [
    { prefix: 'Old notes', existsOnDisk: false },
    { prefix: 'Work/private', existsOnDisk: true },
  ];
  invoke.mockResolvedValue(rules);

  await expect(ipc.listExclusions(4)).resolves.toEqual(rules);
});

test('excludeSubfolder sends the root id and the path the listing gave it', async () => {
  invoke.mockResolvedValue(undefined);

  await ipc.excludeSubfolder(3, 'Work/private');

  expect(invoke).toHaveBeenCalledWith('exclude_subfolder', { rootId: 3, relativePath: 'Work/private' });
});

// `include_subfolder` answers whether a row went (`bridge.rs:465-471`), and the
// wrapper has to carry that answer through: "the rule is gone now" and "there
// was no rule left to remove" are two different things to say to a person.
test('includeSubfolder sends the pair and returns whether a rule was removed', async () => {
  invoke.mockResolvedValue(true);

  await expect(ipc.includeSubfolder(3, 'Archive')).resolves.toBe(true);
  expect(invoke).toHaveBeenCalledWith('include_subfolder', { rootId: 3, relativePath: 'Archive' });

  invoke.mockResolvedValue(false);
  await expect(ipc.includeSubfolder(3, 'Archive')).resolves.toBe(false);
});

// The four mask wrappers. `Masks.test.ts` mocks these wrappers rather than
// `invoke`, deliberately — what that file is about is the screen — so the wire
// names live here and nowhere else. A wrapper that sent `{ mask: … }` where the
// command declares `pattern` would leave every screen test green and every real
// call refused.
test('listMasks takes no argument at all — a mask is global, not per root', async () => {
  invoke.mockResolvedValue([]);

  await ipc.listMasks();

  // Both directions: the wire string AND the absence of a payload. `listMasks`
  // is the one list command in this file with no `rootId`, and a `rootId`
  // smuggled in here would be a per-root reading of a global rule.
  expect(invoke).toHaveBeenCalledWith('list_masks');
  expect((invoke.mock.calls.at(-1) as unknown[]).length).toBe(1);
});

test('listMasks returns the stored patterns whole, in the order the shell sent them', async () => {
  // Two, not one: a wrapper that returned `[list[0]]` satisfies a one-element
  // fixture, and the order is the shell's (`Db::list_masks` sorts) rather than
  // anything this side re-derives.
  invoke.mockResolvedValue(['*.pdf', '*.tmp']);

  await expect(ipc.listMasks()).resolves.toEqual(['*.pdf', '*.tmp']);
});

test('maskPreview sends the pattern under the name the command declares', async () => {
  invoke.mockResolvedValue({ paths: 4, documents: 1 });

  await expect(ipc.maskPreview('*.pdf')).resolves.toEqual({ paths: 4, documents: 1 });

  expect(invoke).toHaveBeenCalledWith('mask_preview', { pattern: '*.pdf' });
  expect(Object.keys((invoke.mock.calls.at(-1) as [string, object])[1])).toEqual(['pattern']);
});

test('addMask sends the pattern exactly as typed, untrimmed', async () => {
  invoke.mockResolvedValue({ kind: 'stored' });

  // The surrounding space is the point: `validate_mask` refuses it with a
  // sentence of its own, and a wrapper that trimmed would hand the shell a
  // different rule from the one the person typed and looked at.
  await expect(ipc.addMask(' *.pdf ')).resolves.toEqual({ kind: 'stored' });

  expect(invoke).toHaveBeenCalledWith('add_mask', { pattern: ' *.pdf ' });

  // Task 11 fix round 2, F2. The other outcome, and the spelling it carries:
  // `add_mask` no longer answers "nothing", so a wrapper that swallowed the
  // reply would leave the editor unable to tell a stored mask from one that
  // was already there under another spelling.
  invoke.mockResolvedValue({ kind: 'alreadyStored', stored: '*.pdf' });
  await expect(ipc.addMask('*.PDF')).resolves.toEqual({ kind: 'alreadyStored', stored: '*.pdf' });
});

test('removeMask carries back whether a row actually went', async () => {
  invoke.mockResolvedValue(true);

  await expect(ipc.removeMask('*.pdf')).resolves.toBe(true);
  expect(invoke).toHaveBeenCalledWith('remove_mask', { pattern: '*.pdf' });

  // The other direction, because `false` is the answer the editor turns into a
  // different sentence: there was no such mask left to remove.
  invoke.mockResolvedValue(false);
  await expect(ipc.removeMask('*.pdf')).resolves.toBe(false);
});

// The whole union, both halves of it: the six spellings pinned as values, and a
// mapping TypeScript can only accept when every variant of `SubfolderState` has
// an entry. A seventh variant added to `tree.rs` and mirrored here fails
// `npm run check` on the missing key; one mirrored under a wrong tag fails the
// list below. The tags are `tree.rs`'s own
// (`the_subfolder_wire_shape_is_camel_case`), not this file's invention.
//
// ⚠️ It says nothing at all about a variant added to `tree.rs` and NOT
// mirrored here — the direction that actually happens. That is the next test.
// One sample per variant, and the annotation is the point: TypeScript accepts
// this object only when it has an entry for every member of the union and no
// entry for anything else, so `Object.keys` below is the union enumerated at
// run time rather than a list written beside it.
const byTag: Record<SubfolderState['kind'], SubfolderState> = {
  open: { kind: 'open' },
  excluded: { kind: 'excluded' },
  excludedByAncestor: { kind: 'excludedByAncestor', prefix: 'Work' },
  builtIn: { kind: 'builtIn' },
  symlink: { kind: 'symlink' },
  unusableName: { kind: 'unusableName' },
};

test('SubfolderState carries every state the shell can send, and exactly those six', () => {
  expect(Object.keys(byTag).sort()).toEqual(
    ['builtIn', 'excluded', 'excludedByAncestor', 'open', 'symlink', 'unusableName'],
  );
  // The payload the ancestor state carries is the whole reason that row can
  // name the rule holding it.
  const held = byTag.excludedByAncestor;
  expect(held.kind === 'excludedByAncestor' && held.prefix).toBe('Work');
});

// 🔴 The direction the test above cannot see, and the one that actually
// happens. `Record<SubfolderState['kind'], …>` checks the union against
// ITSELF, so it is satisfied by any union — including one that is a variant
// behind `tree.rs`. Rust and TypeScript share no compiler: a variant added to
// the enum that OWNS this wire shape, and not mirrored here, fails nothing.
// It reaches `Folders.svelte`'s classifier as a state that file has never
// heard of, and the review that found this rendered the result — a folder
// name, an empty sentence, and an unlabelled button that removed the person's
// exclusion rule.
//
// Both directions in one comparison: a variant Rust gains and this file has
// never heard of fails here, and so does one this file still lists after Rust
// has dropped it. The spelling half is Rust's own
// `the_subfolder_wire_shape_is_camel_case` (`tree.rs:1254`), which serializes
// each state and pins the tag string — this half derives the wire name with
// serde's camelCase rule and would not notice `rename_all` changing. Neither
// closes the gap alone.
const HERE = dirname(fileURLToPath(import.meta.url));
const TREE_RS = readFileSync(join(HERE, '../../../src-tauri/src/tree.rs'), 'utf8');

test('SubfolderState is exactly what tree.rs defines, in the spelling serde sends', () => {
  expect(Object.keys(byTag).sort()).toEqual(
    rustEnumVariants(TREE_RS, 'SubfolderState').map(camelOf).sort(),
  );
});

test('the subfolder wire types reject Rust snake_case spellings', () => {
  const listing: SubfolderListing = {
    entries: [{
      name: 'private', relativePath: 'Work/private', state: { kind: 'open' },
      // @ts-expect-error TypeScript must reject Rust's pre-serialization spelling.
      relative_path: 'Work/private',
    }],
    unnameable: 0,
  };
  const rule: StoredExclusion = {
    prefix: 'Work/private', existsOnDisk: true,
    // @ts-expect-error same, for the rule list's own field.
    exists_on_disk: true,
  };

  expect(listing.entries[0].relativePath).toBe('Work/private');
  expect(rule.existsOnDisk).toBe(true);
});

// ---------------------------------------------------------------------------
// The Indexing section's own fields on `model_settings`' index read arm
// (`src-tauri/src/models.rs`, §9.3). The Rust side pins that the wire carries
// them, against a really walked index, in
// `the_settings_carry_the_whole_index_file_count_and_its_last_indexed_moment`
// (`src-tauri/tests/commands.rs`); this side pins that this module asks for
// them and asks for them in the spelling serde sends. Neither half closes the
// gap alone — a hand-written type and a hand-written fixture can carry the
// same mistake and pass together.

type IndexRead = Extract<IndexSettings, { kind: 'read' }>;

test('the index read arm carries the file count, the moment, and the refusal count', () => {
  const read: IndexRead = {
    kind: 'read', embeddingModel: 'emb-1', chatModel: null,
    embeddedChunks: 3, embeddedChunksEverywhere: 3, totalChunks: 4,
    failedChunks: 1, indexedFiles: 2, lastIndexedAt: 1_700_000_000,
    searchTextArm: true, searchContentArm: true,
  };

  expect(read.indexedFiles).toBe(2);
  expect(read.lastIndexedAt).toBe(1_700_000_000);
  expect(read.failedChunks).toBe(1);
});

test('an index that has never finished indexing states that as null, not as an absence', () => {
  const read: IndexRead = {
    kind: 'read', embeddingModel: null, chatModel: null,
    embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0,
    failedChunks: 0, indexedFiles: 0, lastIndexedAt: null,
    searchTextArm: true, searchContentArm: true,
  };

  // `null` is the backend's own statement, and it is a value this arm holds —
  // not the same thing as the field being missing, which the test below pins.
  expect(read.lastIndexedAt).toBeNull();
  expect('lastIndexedAt' in read).toBe(true);
});

// 🔴 Each of the three omitted on its own, because "the object is missing
// something" is satisfied by any one of them and would not notice the other
// two turning optional. Required and not optional for the reason `ipc.ts`
// gives over the type: the only substitute for a missing count is `0`, and `0`
// in front of a person reads as a measured claim this build has not made —
// a fail-quiet field is a number that is silently always wrong.
test('the three counts are required, so no fixture can leave one to a default', () => {
  const rest = {
    kind: 'read' as const, embeddingModel: null, chatModel: null,
    embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0,
    searchTextArm: true, searchContentArm: true,
  };

  // @ts-expect-error `indexedFiles` is required.
  const noFiles: IndexRead = { ...rest, failedChunks: 0, lastIndexedAt: null };
  // @ts-expect-error `lastIndexedAt` is required.
  const noMoment: IndexRead = { ...rest, failedChunks: 0, indexedFiles: 0 };
  // @ts-expect-error `failedChunks` is required.
  const noRefusals: IndexRead = { ...rest, indexedFiles: 0, lastIndexedAt: null };

  expect([noFiles.kind, noMoment.kind, noRefusals.kind]).toEqual(['read', 'read', 'read']);
});

test('the index read arm rejects Rust snake_case spellings', () => {
  const read: IndexRead = {
    kind: 'read', embeddingModel: null, chatModel: null,
    embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0,
    failedChunks: 0, indexedFiles: 2, lastIndexedAt: 1_700_000_000,
    searchTextArm: true, searchContentArm: true,
    // @ts-expect-error TypeScript must reject Rust's pre-serialization spelling.
    indexed_files: 2,
  };

  expect(read.indexedFiles).toBe(2);
});

// ---------------------------------------------------------------------------
// PR 9 Task 7: the three commands the Application section calls
// (`src-tauri/src/prefs.rs`). The reply shapes are mirrored WHOLE — the tagged
// `status`, the tagged autostart state, the version and the platform — rather
// than in the subset the section happens to draw today; `JobEnded`'s eleven
// fields are the precedent and the reason is the same one.
// ---------------------------------------------------------------------------

const REGISTERED: AppPrefs = {
  hotkey: { shortcut: 'Alt+Space', status: { kind: 'registered' } },
  autostart: { kind: 'disabled' },
  version: '0.0.0',
  platform: 'mac',
};

test('appPrefs invokes app_prefs and takes no arguments at all', async () => {
  invoke.mockResolvedValue(REGISTERED);

  const prefs = await ipc.appPrefs();

  expect(invoke).toHaveBeenCalledWith('app_prefs');
  // The whole reply crosses, not the shortcut alone: an earlier form of the
  // job wrapper read one field and threw the rest away, which was invisible
  // until something needed the rest.
  expect(prefs).toEqual(REGISTERED);
});

test('setHotkey invokes set_hotkey with the shortcut under its camelCase name', async () => {
  const reply: HotkeyState = { shortcut: 'Ctrl+Alt+Space', status: { kind: 'registered' } };
  invoke.mockResolvedValue(reply);

  const state = await ipc.setHotkey('Ctrl+Alt+Space');

  expect(invoke).toHaveBeenCalledWith('set_hotkey', { shortcut: 'Ctrl+Alt+Space' });
  expect(state).toEqual(reply);
});

test('setAutostart invokes set_autostart with the boolean, and answers the OS state rather than the request', async () => {
  // The reply and the request disagree on purpose: `set_autostart` re-reads the
  // operating system after the change (D-c), so a wrapper that echoed its own
  // argument would be indistinguishable from one that returned the reply — on
  // every fixture except this one.
  const reply: AutostartState = { kind: 'unknown', reason: 'the login item list could not be read' };
  invoke.mockResolvedValue(reply);

  const state = await ipc.setAutostart(true);

  expect(invoke).toHaveBeenCalledWith('set_autostart', { enabled: true });
  expect(state).toEqual(reply);
});

test('the hotkey status is a tagged union of exactly two arms', () => {
  // A `Record` over the discriminant rather than a list: an arm added on the
  // Rust side and left unmapped here is a compile error, which is what keeps
  // the window exhaustive over `HotkeyStatus` instead of falling through.
  const sentence: Record<HotkeyStatus['kind'], string> = {
    registered: 'the system holds it',
    unavailable: 'the system does not hold it',
  };
  const unavailable: HotkeyStatus = { kind: 'unavailable', reason: 'already taken' };

  expect(sentence[unavailable.kind]).toBe('the system does not hold it');
  // `unavailable` carries its reason; `registered` carries nothing to show.
  expect(unavailable.kind === 'unavailable' ? unavailable.reason : null).toBe('already taken');
});

test('the autostart state is a tagged union of exactly three arms, and unknown is not disabled', () => {
  const sentence: Record<AutostartState['kind'], string> = {
    enabled: 'it starts at sign-in',
    disabled: 'it does not start at sign-in',
    unknown: 'nobody could tell',
  };
  const unknown: AutostartState = { kind: 'unknown', reason: 'the read failed' };

  expect(sentence[unknown.kind]).toBe('nobody could tell');
  // 🔴 Three sentences, no two alike: a failed READ rendered as "off" would
  // show a person a switch in the position opposite to the machine's.
  expect(new Set(Object.values(sentence)).size).toBe(3);
});

test('app prefs reject Rust snake_case spellings', () => {
  const prefs: AppPrefs = {
    ...REGISTERED,
    // @ts-expect-error TypeScript must reject a spelling the wire never sends.
    hot_key: { shortcut: 'Alt+Space', status: { kind: 'registered' } },
  };

  expect(prefs.hotkey.shortcut).toBe('Alt+Space');
});

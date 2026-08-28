import { expect, test, vi } from 'vitest';
import * as ipc from './ipc';
import type { SourceAround } from './ipc';
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
const PROGRESS_PAYLOAD = { done: 3, total: 8, skipped: 1, refused: 0, secondsLeft: null } as const;

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

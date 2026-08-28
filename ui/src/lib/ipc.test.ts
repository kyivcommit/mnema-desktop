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

// PR 7 Task 6, review: the pass used to report to nobody. Both directions, in
// one test — a callback that fires on every message would pass a test that only
// sent an ending, and that is the mutation that matters here: the section
// re-reads the index when this fires, and re-reading it on every progress
// report is a call per 250 ms for the length of a run.
test('startEmbedJob calls back when the pass ENDS, and not when it reports progress', async () => {
  invoke.mockResolvedValue(undefined);
  const ended = vi.fn();

  await ipc.startEmbedJob(ended);

  const call = invoke.mock.calls.at(-1) as [string, { onProgress: { onmessage: (m: unknown) => void } }];
  expect(call[0]).toBe('start_embed_job');
  const channel = call[1].onProgress;
  expect(typeof channel.onmessage).toBe('function');

  channel.onmessage({ event: 'progress', data: { done: 1, total: 9 } });
  expect(ended).not.toHaveBeenCalled();

  // The tag `JobEvent` carries, spelled as the Rust side serializes it
  // (`job.rs:309-313`, pinned against the real serialization by
  // `src-tauri/tests/commands.rs:826-828`).
  channel.onmessage({ event: 'ended', data: { reason: 'finished' } });
  expect(ended).toHaveBeenCalledTimes(1);
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

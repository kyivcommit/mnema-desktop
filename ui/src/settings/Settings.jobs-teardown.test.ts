import { render, cleanup } from '@testing-library/svelte';
import { expect, test, afterEach, vi } from 'vitest';
import { tick } from 'svelte';
import { writable } from 'svelte/store';
import Settings from './Settings.svelte';
import type { ScanState } from '../lib/ipc';

// Task 11a (Task 8 M6/M7). `Settings.test.ts` proves "a state delivered
// after this window is unmounted changes nothing" over and over, always
// through `jobs.mount()`'s own `listenScanProgress` callback (`jobs.ts`) —
// and that callback carries its OWN `destroyed` guard, checked before it
// ever touches `jobs.state`. Once this window is gone, that guard alone
// stops any delivered state from reaching the store, so every one of those
// tests would pass identically whether or not `Settings.svelte`'s own
// `onMount(() => { const stop = jobs.state.subscribe(...); ...; return
// stop; })` ever returns `stop` at all — removing that `return` changes
// nothing they can see. That is a test standing on a neighbouring defence
// (`jobs.mount`'s guard) rather than on the property this window itself
// owns: that its OWN subscription to `jobs.state` is torn down when it is.
//
// Telling the two apart needs a `jobs.state` this test can push into
// directly, wired through a controller that does none of `jobs.ts`'s own
// guarding — so `./jobs` is mocked here, in a file of its own, from the
// top: mocking it mid-file (`vi.doMock` after `Settings.svelte` and
// `@testing-library/svelte` are already loaded) reloads a second copy of
// Svelte's client runtime that does not share component context with the
// first, and every render throws `effect_orphan` before this ever gets to
// the assertion.
const jobState = writable<{ scan: ScanState; note: string | null }>({
  scan: { revision: 0, files: 0, readSeq: 0, lastReading: null, snapshot: { kind: 'idle' } },
  note: null,
});
const controllerMount = vi.fn(() => () => {});
vi.mock('./jobs', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./jobs')>();
  return {
    ...actual,
    createJobController: () => ({
      state: jobState,
      scan: vi.fn(),
      cancel: vi.fn(),
      mount: controllerMount,
    }),
  };
});

// `Settings.svelte` still mounts the other three sections' real components on
// its default 'models' section, and `Models.svelte` still calls
// `model_settings` on its own mount — a mock is needed for the same reason
// every other file in this directory needs one, or the real, un-mockable
// `invoke` runs and every render fails on an unhandled rejection.
const modelSettings = vi.fn();
vi.mock('../lib/ipc', () => ({
  modelSettings: (...a: unknown[]) => modelSettings(...a),
  setKey: vi.fn(),
  forgetKey: vi.fn(),
  providerModels: () => Promise.resolve({ entries: [], unreadable: 0, unreadableRecords: [] }),
  setChatModel: vi.fn(),
  listTree: () => Promise.resolve({ roots: [], recents: [] }),
  listSubfolders: () => Promise.resolve({ entries: [], unnameable: 0 }),
  listExclusions: () => Promise.resolve([]),
  excludeSubfolder: vi.fn(),
  includeSubfolder: vi.fn(),
  listMasks: () => Promise.resolve([]),
  maskPreview: () => Promise.resolve({ paths: 0, documents: 0 }),
  addMask: vi.fn(),
  removeMask: vi.fn(),
  addWatchedFolder: vi.fn(),
  removeWatchedFolder: vi.fn(),
  appPrefs: () => Promise.resolve({
    hotkey: { shortcut: 'Alt+Space', status: { kind: 'registered' } },
    autostart: { kind: 'disabled' },
    version: '0.0.0',
    platform: 'linux',
  }),
  setHotkey: vi.fn(),
  setAutostart: vi.fn(),
}));

afterEach(() => {
  cleanup();
  modelSettings.mockReset();
  jobState.set({
    scan: { revision: 0, files: 0, readSeq: 0, lastReading: null, snapshot: { kind: 'idle' } },
    note: null,
  });
});

test('the window\'s own subscription to jobs.state is torn down on unmount, not just the channel jobs.mount guards separately', async () => {
  modelSettings.mockResolvedValue({
    key: { kind: 'absent' },
    index: {
      kind: 'read', embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0,
      failedChunks: 0, pendingChunks: 0, scanIncomplete: false, indexedFiles: 0, lastIndexedAt: null,
      embeddingModel: null, searchTextArm: true, searchContentArm: false,
    },
    platform: 'linux',
  });

  const { unmount } = render(Settings);
  await tick();
  const baseline = modelSettings.mock.calls.length;
  expect(baseline).toBeGreaterThan(0); // the mount read actually happened

  unmount();
  // One more snapshot, pushed straight into the store this window's own
  // subscriber reads — no `jobs.mount()` guard sits between this call and
  // that subscriber, so only `Settings.svelte`'s own teardown can stop it.
  jobState.set({
    scan: { revision: 1, files: 0, readSeq: 1, lastReading: null, snapshot: { kind: 'idle' } },
    note: null,
  });
  await tick();

  expect(modelSettings.mock.calls.length).toBe(baseline);
});

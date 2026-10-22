import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import AddFolder from './AddFolder.svelte';
import { setLocale } from '../i18n';
import type { ScanSnapshot, ScanState } from '../lib/ipc';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
const open = vi.fn();
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: (...a: unknown[]) => open(...a) }));
// Faked at the module boundary, as `settings/jobs.test.ts` does, so the event
// name and the payload unwrapping in `listenScanProgress` are exercised too.
const listen = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({ listen: (...a: unknown[]) => listen(...a) }));

const state = (over: Partial<ScanState> = {}): ScanState => ({
  revision: 0, files: 0, readSeq: 0, jobsDone: 0, lastReading: null, snapshot: { kind: 'idle' }, ...over,
});
const RUNNING: ScanSnapshot = { kind: 'running', cancellable: true, phase: { kind: 'embedding',
  counts: { done: 0, total: 0, skipped: 0, refused: 0, contended: 0, secondsLeft: null } } };

// One reply per command name; an unexpected command fails the test.
let addReply: () => Promise<number>;
let startReply: () => Promise<void>;
let statusReply: () => Promise<ScanState>;
function route(cmd: string): Promise<unknown> {
  if (cmd === 'add_watched_folder') return addReply();
  if (cmd === 'start_scan_job') return startReply();
  if (cmd === 'job_status') return statusReply();
  throw new Error(`unexpected command ${cmd}`);
}
const calls = (cmd: string) => invoke.mock.calls.filter(([c]) => c === cmd);

let onChanged: ReturnType<typeof vi.fn<() => void>>;
beforeEach(() => {
  setLocale('en');
  invoke.mockReset().mockImplementation(route);
  open.mockReset().mockResolvedValue('/Users/me/Books');
  listen.mockReset().mockResolvedValue(() => {});
  addReply = () => Promise.resolve(7);
  startReply = () => Promise.resolve();
  statusReply = () => Promise.resolve(state());
  onChanged = vi.fn<() => void>();
});
afterEach(cleanup);

const button = () => screen.getByTestId('tree-add-folder');
const press = () => fireEvent.click(button());

test('cancelled dialog calls nothing', async () => {
  open.mockResolvedValue(null);
  render(AddFolder, { onChanged });
  await press();
  await waitFor(() => expect(open).toHaveBeenCalledTimes(1));
  await new Promise((r) => setTimeout(r, 0));
  expect(calls('add_watched_folder')).toHaveLength(0);
  expect(calls('start_scan_job')).toHaveLength(0);
  expect(onChanged).not.toHaveBeenCalled();
});

test('idle press adds the folder, reloads the tree, then starts a full scan', async () => {
  const order: string[] = [];
  invoke.mockImplementation((cmd: string, args: unknown) => { order.push(cmd); return route(cmd); });
  onChanged.mockImplementation(() => order.push('reload'));
  render(AddFolder, { onChanged });
  await press();
  await waitFor(() => expect(calls('start_scan_job')).toHaveLength(1));
  expect(calls('add_watched_folder')[0][1]).toEqual({ path: '/Users/me/Books' });
  expect(calls('start_scan_job')[0][1]).toEqual({ entry: 'full' });
  expect(order.filter((c) => c !== 'job_status')).toEqual(['add_watched_folder', 'reload', 'start_scan_job']);
});

test('a refused add shows the sentence and starts nothing', async () => {
  addReply = () => Promise.reject(new Error('folder already watched'));
  render(AddFolder, { onChanged });
  await press();
  await waitFor(() => expect(screen.getByTestId('tree-add-folder-error').textContent).toBe('folder already watched'));
  expect(calls('start_scan_job')).toHaveLength(0);
});

test('a scan refused while another runs shows nothing', async () => {
  startReply = () => Promise.reject('a job is already running');
  statusReply = () => Promise.resolve(state({ revision: 3, snapshot: RUNNING }));
  render(AddFolder, { onChanged });
  await press();
  await waitFor(() => expect(calls('job_status').length).toBeGreaterThan(0));
  await new Promise((r) => setTimeout(r, 0));
  expect(screen.queryByTestId('tree-add-folder-error')).toBeNull();
});

test('a scan refused with nothing running shows the sentence', async () => {
  startReply = () => Promise.reject('x');
  render(AddFolder, { onChanged });
  await press();
  await waitFor(() => expect(screen.getByTestId('tree-add-folder-error').textContent).toBe('x'));
});

test('a second press while the first is busy does nothing', async () => {
  open.mockReturnValue(new Promise(() => {}));
  render(AddFolder, { onChanged });
  await press();
  await press();
  expect(open).toHaveBeenCalledTimes(1);
});

test('a new press clears the message', async () => {
  addReply = () => Promise.reject(new Error('nope'));
  render(AddFolder, { onChanged });
  await press();
  await waitFor(() => expect(screen.getByTestId('tree-add-folder-error')).toBeTruthy());
  open.mockReturnValue(new Promise(() => {}));
  await press();
  expect(screen.queryByTestId('tree-add-folder-error')).toBeNull();
});

test('the label follows the locale', async () => {
  setLocale('uk');
  render(AddFolder, { onChanged });
  expect(button().getAttribute('aria-label')).toBe('Додати теку');
  expect(button().getAttribute('title')).toBe('Додати теку');
  setLocale('en');
  await waitFor(() => expect(button().getAttribute('aria-label')).toBe('Add folder'));
  expect(button().getAttribute('title')).toBe('Add folder');
});

// ---------------------------------------------------------------------------
// Progress: the ring, the queued scan, the tree reloads.
// ---------------------------------------------------------------------------
const counts = (done: number, total: number) =>
  ({ done, total, skipped: 0, refused: 0, contended: 0, secondsLeft: null });
const reading = (done: number, total: number): ScanSnapshot =>
  ({ kind: 'running', cancellable: true, phase: { kind: 'reading', rootIndex: 1, rootCount: 1, rootPath: '/r', counts: counts(done, total) } });
const embeddingAt = (done: number, total: number): ScanSnapshot =>
  ({ kind: 'running', cancellable: true, phase: { kind: 'embedding', counts: counts(done, total) } });

// The `scan-progress` handler the component registered, as the emitter would call it.
async function emit(s: ScanState) {
  await waitFor(() => expect(listen).toHaveBeenCalled());
  const [name, handler] = listen.mock.calls[0];
  expect(name).toBe('scan-progress');
  handler({ payload: s });
  await new Promise((r) => setTimeout(r, 0));
}
const ring = () => screen.queryByTestId('tree-add-folder-ring');

// Mounted, pressed, refused because a scan runs: the press is queued.
async function queued() {
  startReply = () => Promise.reject('a job is already running');
  statusReply = () => Promise.resolve(state({ revision: 1, snapshot: RUNNING }));
  render(AddFolder, { onChanged });
  await press();
  await waitFor(() => expect(calls('start_scan_job')).toHaveLength(1));
  await new Promise((r) => setTimeout(r, 0));
}

test('a queued scan starts once when jobsDone grows', async () => {
  await queued();
  startReply = () => Promise.resolve();
  await emit(state({ revision: 2, jobsDone: 1 }));
  await waitFor(() => expect(calls('start_scan_job')).toHaveLength(2));
  await emit(state({ revision: 3, jobsDone: 2 }));
  expect(calls('start_scan_job')).toHaveLength(2);
  expect(calls('start_scan_job')[1][1]).toEqual({ entry: 'full' });
});

test('a queued scan tries again after a second refusal while running', async () => {
  await queued();
  statusReply = () => Promise.resolve(state({ revision: 3, jobsDone: 1, snapshot: RUNNING }));
  await emit(state({ revision: 2, jobsDone: 1 }));
  await waitFor(() => expect(calls('start_scan_job')).toHaveLength(2));
  startReply = () => Promise.resolve();
  await emit(state({ revision: 4, jobsDone: 2 }));
  await waitFor(() => expect(calls('start_scan_job')).toHaveLength(3));
});

test('a refusal with nothing running does not queue', async () => {
  startReply = () => Promise.reject('x');
  render(AddFolder, { onChanged });
  await press();
  await waitFor(() => expect(screen.getByTestId('tree-add-folder-error')).toBeTruthy());
  await emit(state({ revision: 5, jobsDone: 1 }));
  expect(calls('start_scan_job')).toHaveLength(1);
});

test('an older revision is ignored', async () => {
  await queued();
  await emit(state({ revision: 9, snapshot: reading(5, 10) }));
  await emit(state({ revision: 8, jobsDone: 1 }));
  expect(calls('start_scan_job')).toHaveLength(1);
  expect(ring()?.getAttribute('data-fraction')).toBe('0.25');
});

test('the ring shows only while a scan reads or embeds', async () => {
  render(AddFolder, { onChanged });
  await emit(state({ revision: 1 }));
  expect(ring()).toBeNull();
  await emit(state({ revision: 2, snapshot: reading(5, 10) }));
  expect(ring()?.getAttribute('data-fraction')).toBe('0.25');
  await emit(state({ revision: 3, snapshot: { kind: 'running', cancellable: false, phase: { kind: 'removing', rootPath: '/r' } } }));
  expect(ring()).toBeNull();
});

test('the ring shows on mount in the middle of a scan', async () => {
  statusReply = () => Promise.resolve(state({ revision: 4, snapshot: reading(0, 0) }));
  render(AddFolder, { onChanged });
  await waitFor(() => expect(ring()?.getAttribute('data-fraction')).toBe('spin'));
});

test('the tree reloads on readSeq, jobsDone and files read, not on a bare revision', async () => {
  render(AddFolder, { onChanged });
  await emit(state({ revision: 1, snapshot: reading(1, 10) }));
  onChanged.mockClear();
  await emit(state({ revision: 2, snapshot: reading(1, 10) }));
  expect(onChanged).not.toHaveBeenCalled();
  await emit(state({ revision: 3, snapshot: reading(2, 10) }));
  expect(onChanged).toHaveBeenCalledTimes(1);
  await emit(state({ revision: 4, readSeq: 1, snapshot: embeddingAt(0, 3) }));
  expect(onChanged).toHaveBeenCalledTimes(2);
  await emit(state({ revision: 5, readSeq: 1, snapshot: embeddingAt(1, 3) }));
  expect(onChanged).toHaveBeenCalledTimes(2);
  await emit(state({ revision: 6, readSeq: 1, jobsDone: 1 }));
  expect(onChanged).toHaveBeenCalledTimes(3);
});

test('scan events do not clear the message', async () => {
  addReply = () => Promise.reject(new Error('nope'));
  render(AddFolder, { onChanged });
  await press();
  await waitFor(() => expect(screen.getByTestId('tree-add-folder-error')).toBeTruthy());
  await emit(state({ revision: 7, jobsDone: 1 }));
  expect(screen.getByTestId('tree-add-folder-error').textContent).toBe('nope');
});

test('unmounting before listen resolves still unlistens', async () => {
  let resolve!: (u: () => void) => void;
  listen.mockReturnValue(new Promise((r) => { resolve = r; }));
  const unlisten = vi.fn();
  const { unmount } = render(AddFolder, { onChanged });
  await waitFor(() => expect(listen).toHaveBeenCalled());
  unmount();
  resolve(unlisten);
  await new Promise((r) => setTimeout(r, 0));
  expect(unlisten).toHaveBeenCalledTimes(1);
});

test('the tooltip carries the phase and its counts', async () => {
  render(AddFolder, { onChanged });
  await emit(state({ revision: 1, snapshot: embeddingAt(5, 10) }));
  expect(button().getAttribute('title')).toMatch(/^Add folder — .+ 5\/10$/);
  expect(button().getAttribute('aria-label')).toBe('Add folder');
});

// An emission between the first read and the listener's registration reaches
// nobody (`scan-progress` has no replay), so the state is read again once the
// listener is in — as `settings/jobs.ts` does.
test('the state is read again once the listener is registered', async () => {
  let resolve!: (u: () => void) => void;
  listen.mockReturnValue(new Promise((r) => { resolve = r; }));
  statusReply = () => Promise.resolve(state({ revision: 1, snapshot: reading(1, 10) }));
  render(AddFolder, { onChanged });
  await waitFor(() => expect(ring()).not.toBeNull());
  statusReply = () => Promise.resolve(state({ revision: 2, jobsDone: 1 }));
  resolve(() => {});
  await waitFor(() => expect(ring()).toBeNull());
  expect(calls('job_status')).toHaveLength(2);
});

test('a subscription that cannot be registered says so', async () => {
  listen.mockRejectedValue(new Error('no events'));
  render(AddFolder, { onChanged });
  await waitFor(() => expect(screen.getByTestId('tree-add-folder-error').textContent).toBe('no events'));
});

test('a subscription that fails after unmount says nothing and throws nothing', async () => {
  let reject!: (e: unknown) => void;
  listen.mockReturnValue(new Promise((_, r) => { reject = r; }));
  const { unmount } = render(AddFolder, { onChanged });
  await waitFor(() => expect(listen).toHaveBeenCalled());
  unmount();
  reject(new Error('gone'));
  await new Promise((r) => setTimeout(r, 0));
  expect(screen.queryByTestId('tree-add-folder-error')).toBeNull();
});

// ---------------------------------------------------------------------------
// Final review: the queue must not be lost (findings 1, 3, 4).
// ---------------------------------------------------------------------------
test('a queued scan starts when a later press ends without starting one', async () => {
  await queued();
  let answer!: (p: string | null) => void;
  open.mockReturnValue(new Promise((r) => { answer = r; }));
  await press();
  startReply = () => Promise.resolve();
  await emit(state({ revision: 2, jobsDone: 1 }));
  expect(calls('start_scan_job')).toHaveLength(1);
  answer(null);
  await waitFor(() => expect(calls('start_scan_job')).toHaveLength(2));
});

test('a refusal judged on a stale job_status still starts the queued scan', async () => {
  startReply = () => Promise.reject('a job is already running');
  render(AddFolder, { onChanged });
  await waitFor(() => expect(calls('job_status')).toHaveLength(2));
  let reply!: (s: ScanState) => void;
  statusReply = () => new Promise((r) => { reply = r; });
  await press();
  await waitFor(() => expect(calls('job_status')).toHaveLength(3));
  await emit(state({ revision: 5, jobsDone: 1 }));
  startReply = () => Promise.resolve();
  reply(state({ revision: 1, snapshot: RUNNING }));
  await waitFor(() => expect(calls('start_scan_job')).toHaveLength(2));
  expect(screen.queryByTestId('tree-add-folder-error')).toBeNull();
});

test('a queued start refused as the blocker ends starts once and shows nothing', async () => {
  await queued();
  let reply!: (s: ScanState) => void;
  statusReply = () => new Promise((r) => { reply = r; });
  await emit(state({ revision: 2, jobsDone: 1, snapshot: RUNNING }));
  await waitFor(() => expect(calls('start_scan_job')).toHaveLength(1));
  await emit(state({ revision: 3, jobsDone: 1 }));
  await waitFor(() => expect(calls('start_scan_job')).toHaveLength(2));
  await emit(state({ revision: 4, jobsDone: 2 }));
  expect(calls('start_scan_job')).toHaveLength(2);
  startReply = () => Promise.resolve();
  reply(state({ revision: 2, jobsDone: 1, snapshot: RUNNING }));
  await waitFor(() => expect(calls('start_scan_job')).toHaveLength(3));
  await new Promise((r) => setTimeout(r, 0));
  expect(calls('start_scan_job')).toHaveLength(3);
  expect(screen.queryByTestId('tree-add-folder-error')).toBeNull();
});

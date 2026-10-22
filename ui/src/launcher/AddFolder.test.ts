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

let onChanged: ReturnType<typeof vi.fn>;
beforeEach(() => {
  setLocale('en');
  invoke.mockReset().mockImplementation(route);
  open.mockReset().mockResolvedValue('/Users/me/Books');
  listen.mockReset().mockResolvedValue(() => {});
  addReply = () => Promise.resolve(7);
  startReply = () => Promise.resolve();
  statusReply = () => Promise.resolve(state());
  onChanged = vi.fn();
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

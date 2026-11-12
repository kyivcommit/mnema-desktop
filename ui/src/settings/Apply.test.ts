import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import { get } from 'svelte/store';
import Apply from './Apply.svelte';
import { createJobController } from './jobs';
import { unapplied } from './unapplied';
import { setLocale } from '../i18n';
import type { ScanState } from '../lib/ipc';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...a: unknown[]) => invoke(...a),
  Channel: class {},
}));
let deliver: ((state: ScanState) => void) | null = null;
vi.mock('@tauri-apps/api/event', () => ({
  listen: (_name: string, cb: (e: { payload: ScanState }) => void) => {
    deliver = (state: ScanState) => cb({ payload: state });
    return Promise.resolve(() => {});
  },
}));

const IDLE: ScanState = {
  revision: 0, files: 0, readSeq: 0, jobsDone: 0, lastReading: null, snapshot: { kind: 'idle' },
};

beforeEach(() => {
  deliver = null;
  unapplied.set(false);
  invoke.mockReset();
  invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === 'job_status' ? IDLE : undefined));
});
afterEach(() => {
  cleanup();
  setLocale('en');
});

function renderMounted() {
  const jobs = createJobController();
  jobs.mount();
  render(Apply, { props: { jobs } });
}

// Owner, 2026-11-12: under the masks, the last thing in the section.
test('Apply changes shows only while a stored change waits for a scan', async () => {
  setLocale('en');
  renderMounted();
  expect(screen.queryByRole('button', { name: 'Apply changes' })).toBeNull();

  unapplied.set(true);

  expect(await screen.findByRole('button', { name: 'Apply changes' })).toBeTruthy();
});

test('pressing it starts a full scan, and it goes once a scan runs', async () => {
  setLocale('uk');
  unapplied.set(true);
  renderMounted();

  await fireEvent.click(await screen.findByRole('button', { name: 'Застосувати зміни' }));
  expect(invoke).toHaveBeenCalledWith('start_scan_job', { entry: 'full' });

  await waitFor(() => expect(deliver).not.toBeNull());
  deliver!({ ...IDLE, revision: 1, snapshot: { kind: 'running', cancellable: true, phase: { kind: 'embedding', counts: { done: 0, total: 0, skipped: 0, refused: 0, contended: 0, secondsLeft: null } } } });

  await waitFor(() => expect(screen.queryByRole('button', { name: 'Застосувати зміни' })).toBeNull());
  expect(get(unapplied)).toBe(false);
});

// PR #103 review 1: a progress tick re-emits a running snapshot. A change
// stored DURING a scan must keep its reminder through those ticks and after
// the scan ends — only the move INTO running clears it.
test('a change stored while a scan runs keeps Apply changes through its progress and after it ends', async () => {
  setLocale('en');
  renderMounted();
  await waitFor(() => expect(deliver).not.toBeNull());
  const running = (revision: number): ScanState => ({
    ...IDLE, revision,
    snapshot: { kind: 'running', cancellable: true, phase: { kind: 'embedding', counts: { done: revision, total: 9, skipped: 0, refused: 0, contended: 0, secondsLeft: null } } },
  });
  deliver!(running(1));

  unapplied.set(true);
  deliver!(running(2)); // a progress tick
  deliver!({ ...IDLE, revision: 3 }); // the scan ends

  expect(get(unapplied)).toBe(true);
  expect(await screen.findByRole('button', { name: 'Apply changes' })).toBeTruthy();
});

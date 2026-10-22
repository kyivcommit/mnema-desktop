import { get, writable, type Readable } from 'svelte/store';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { addWatchedFolder, jobStatus, listenScanProgress, startScanJob, type ScanState } from '../lib/ipc';
import { apply } from '../settings/jobs';

// The launcher's «+»: add a folder and scan it at once (owner, 2026-10-22),
// unlike Settings, where adding starts no scan (Task 8 PR 9). One controller
// behind both buttons — the Documents pane's and the header's — so there is one
// subscription, one queue and one message, whichever button is on screen.

/// What the buttons draw. `changes` grows whenever the tree should read the
/// index again: a folder added, a job ended, a reading pass ended, files read.
export type AddFolderView = { scan: ScanState | null; error: string | null; changes: number };

export type AddFolderController = Readable<AddFolderView> & {
  press(): Promise<void>;
  /// Subscribes and reads the state; the returned function undoes it.
  start(): () => void;
};

export function createAddFolder(): AddFolderController {
  const view = writable<AddFolderView>({ scan: null, error: null, changes: 0 });
  const set = (patch: Partial<AddFolderView>) => view.update((v) => ({ ...v, ...patch }));
  const changed = () => view.update((v) => ({ ...v, changes: v.changes + 1 }));
  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
  const readingDone = (s: ScanState) =>
    s.snapshot.kind === 'running' && s.snapshot.phase.kind === 'reading' ? s.snapshot.phase.counts.done : null;

  // One press at a time, from the dialog to the scan's answer.
  let busy = false;
  // A scan wanted once nothing runs, and one `start_scan_job` on the wire at a time.
  let scanAfter = false;
  let starting = false;

  // The tree reloads when a job ends (`jobsDone`, not the snapshot leaving
  // `running`: that edge can go unseen, `ipc.ts`) and as files are read.
  function accept(incoming: ScanState) {
    const prev = get(view).scan;
    const next = prev === null ? incoming : apply(prev, incoming);
    if (next === prev) return;
    set({ scan: next });
    if (prev !== null && (next.jobsDone > prev.jobsDone || next.readSeq > prev.readSeq
      || readingDone(next) !== readingDone(prev))) changed();
    kick();
  }

  // The queue is a level, not an edge: it is looked at after every state and
  // after every press, so an ending seen while a press or a start was busy is
  // not lost with it.
  function kick() {
    if (scanAfter && !busy && !starting && get(view).scan?.snapshot.kind !== 'running') void startScan();
  }

  // A refusal reaches the window as text (`error.rs`), so whether the slot was
  // busy is asked of `job_status` rather than read off the words.
  async function startScan() {
    starting = true;
    try {
      await startScanJob('full');
      scanAfter = false;
    } catch (e) {
      try {
        const now = await jobStatus();
        accept(now);
        // Judged on the reply itself: a stale «running» keeps the scan wanted,
        // and `kick` below starts it if a newer state already says it ended.
        scanAfter = now.snapshot.kind === 'running';
        if (!scanAfter) set({ error: message(e) });
      } catch {
        scanAfter = false;
        set({ error: message(e) });
      }
    } finally {
      starting = false;
      kick();
    }
  }

  async function press() {
    if (busy) return;
    busy = true;
    set({ error: null });
    try {
      let path: string | null;
      try {
        path = await open({ directory: true });
      } catch (e) {
        set({ error: message(e) });
        return;
      }
      if (path === null) return;
      try {
        await addWatchedFolder(path);
      } catch (e) {
        set({ error: message(e) });
        return;
      }
      changed();
      await startScan();
    } finally {
      busy = false;
      kick();
    }
  }

  function start() {
    let gone = false;
    let unlisten: UnlistenFn | null = null;
    const read = () => jobStatus().then((s) => { if (!gone) accept(s); }, () => {});
    // A launcher opened mid-scan shows the ring before the next event.
    read();
    listenScanProgress((s) => { if (!gone) accept(s); })
      .then((u) => {
        if (gone) { u(); return; }
        unlisten = u;
        // `scan-progress` has no replay: an emission before this point reached
        // nobody, so ask again (`settings/jobs.ts` does the same).
        read();
      })
      .catch((e) => { if (!gone) set({ error: message(e) }); });
    return () => { gone = true; unlisten?.(); };
  }

  return { subscribe: view.subscribe, press, start };
}

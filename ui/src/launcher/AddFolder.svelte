<script lang="ts">
  import { onMount } from 'svelte';
  import type { UnlistenFn } from '@tauri-apps/api/event';
  import { open } from '@tauri-apps/plugin-dialog';
  import { locale, t } from '../i18n';
  import { addWatchedFolder, jobStatus, listenScanProgress, startScanJob, type ScanState } from '../lib/ipc';
  import { apply, phaseLabel } from '../settings/jobs';
  import { ringFraction } from './scanRing';

  // The Documents pane's «+»: add a folder and scan it at once (owner, 2026-10-22),
  // unlike Settings, where adding starts no scan (Task 8 PR 9).
  let { onChanged }: { onChanged: () => void } = $props();

  // One press at a time, from the dialog to the scan's answer.
  let busy = false;
  // A scan wanted after the one running now ends.
  let scanAfter = false;
  let error = $state<string | null>(null);

  // Every scan, whoever started it, in `revision` order (`apply`).
  let scan = $state<ScanState | null>(null);

  const fraction = $derived(scan === null ? null : ringFraction(scan.snapshot));
  const label = $derived.by(() => { void $locale; return t('tree_add_folder'); });
  const title = $derived.by(() => {
    void $locale;
    const s = scan?.snapshot;
    if (fraction === null || s?.kind !== 'running') return label;
    const p = s.phase;
    if (p.kind !== 'reading' && p.kind !== 'embedding') return label;
    const l = phaseLabel(p);
    return `${label} — ${t(l.key, l.params)} ${p.counts.done}/${p.counts.total}`;
  });
  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
  const readingDone = (s: ScanState) =>
    s.snapshot.kind === 'running' && s.snapshot.phase.kind === 'reading' ? s.snapshot.phase.counts.done : null;

  // The ending is `jobsDone` growing, not the snapshot leaving `running`: that
  // edge can go unseen (`ipc.ts`, `jobsDone`). Files appear as they are read.
  function accept(incoming: ScanState) {
    const prev = scan;
    const next = prev === null ? incoming : apply(prev, incoming);
    if (next === prev) return;
    scan = next;
    if (prev === null) return;
    if (next.jobsDone > prev.jobsDone) {
      onChanged();
      if (scanAfter && !busy) void startScan();
    } else if (next.readSeq > prev.readSeq || readingDone(next) !== readingDone(prev)) {
      onChanged();
    }
  }

  // A refusal reaches the window as text (`error.rs`), so whether the slot was
  // busy is asked of `job_status` rather than read off the words.
  async function startScan() {
    try {
      await startScanJob('full');
      scanAfter = false;
    } catch (e) {
      const now = await jobStatus();
      accept(now);
      scanAfter = now.snapshot.kind === 'running';
      if (!scanAfter) error = message(e);
    }
  }

  onMount(() => {
    let gone = false;
    let unlisten: UnlistenFn | null = null;
    void listenScanProgress(accept).then((u) => { if (gone) u(); else unlisten = u; });
    // A launcher opened mid-scan shows the ring before the next event.
    jobStatus().then(accept, () => {});
    return () => { gone = true; unlisten?.(); };
  });

  async function press() {
    if (busy) return;
    busy = true;
    error = null;
    try {
      const path = await open({ directory: true });
      if (path === null) return;
      try {
        await addWatchedFolder(path);
      } catch (e) {
        error = message(e);
        return;
      }
      onChanged();
      await startScan();
    } finally {
      busy = false;
    }
  }
</script>

<button type="button" class="tb add-folder" data-testid="tree-add-folder"
  aria-label={label} title={title} onclick={press}>
  {#if fraction !== null}
    <svg class="scan-ring" class:spin={fraction === 'spin'} data-testid="tree-add-folder-ring"
      data-fraction={String(fraction)} viewBox="0 0 26 26" aria-hidden="true">
      <circle cx="13" cy="13" r="11.5" pathLength="1"
        stroke-dasharray={fraction === 'spin' ? '0.25 1' : `${fraction} 1`} />
    </svg>
  {/if}
  <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor"
    stroke-width="1.4" stroke-linecap="round" aria-hidden="true"><path d="M8 3.5v9M3.5 8h9"/></svg>
</button>
{#if error !== null}
  <p class="tree-note" role="alert" data-testid="tree-add-folder-error">{error}</p>
{/if}

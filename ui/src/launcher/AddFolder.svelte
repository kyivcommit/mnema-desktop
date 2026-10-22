<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { locale, t } from '../i18n';
  import { addWatchedFolder, jobStatus, startScanJob } from '../lib/ipc';

  // The Documents pane's «+»: add a folder and scan it at once (owner, 2026-10-22),
  // unlike Settings, where adding starts no scan (Task 8 PR 9).
  let { onChanged }: { onChanged: () => void } = $props();

  // One press at a time, from the dialog to the scan's answer.
  let busy = false;
  // A scan wanted after the one running now ends.
  let scanAfter = false;
  let error = $state<string | null>(null);

  const label = $derived.by(() => { void $locale; return t('tree_add_folder'); });
  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  // A refusal reaches the window as text (`error.rs`), so whether the slot was
  // busy is asked of `job_status` rather than read off the words.
  async function startScan() {
    try {
      await startScanJob('full');
      scanAfter = false;
    } catch (e) {
      scanAfter = (await jobStatus()).snapshot.kind === 'running';
      if (!scanAfter) error = message(e);
    }
  }

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
  aria-label={label} title={label} onclick={press}>
  <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor"
    stroke-width="1.4" stroke-linecap="round" aria-hidden="true"><path d="M8 3.5v9M3.5 8h9"/></svg>
</button>
{#if error !== null}
  <p class="tree-note" role="alert" data-testid="tree-add-folder-error">{error}</p>
{/if}

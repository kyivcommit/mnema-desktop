<script lang="ts">
  // Owner, 2026-11-12: a stored tick or mask does nothing until a scan, and
  // nothing on screen said so. The last thing in the folders section, under
  // the masks: "Apply changes" starts a full scan, and goes once one runs.
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import type { JobController } from './jobs';
  import { get } from 'svelte/store';
  import { unapplied, foldersWaiting, applyAction } from './unapplied';

  let { jobs }: { jobs: JobController } = $props();
  // The controller lives for the window; read once, as `JobStrip.svelte` does.
  // svelte-ignore state_referenced_locally
  const jobState = jobs.state;
  const scanRunning = $derived($jobState.scan.snapshot.kind === 'running');
  let applying = $state(false);

  // A scan from anywhere (the tray, the Scanning section) applies the change
  // too — on the move INTO running only. A progress tick re-emits a running
  // snapshot, and a change stored during the scan must keep its reminder
  // (PR #103 review 1).
  onMount(() => {
    let wasRunning = get(jobs.state).scan.snapshot.kind === 'running';
    return jobs.state.subscribe(({ scan }) => {
      const running = scan.snapshot.kind === 'running';
      if (running && !wasRunning) unapplied.set(false);
      wasRunning = running;
    });
  });

  const label = $derived.by(() => { void $locale; return t('settings_folders_apply'); });

  async function apply() {
    applying = true;
    try {
      const action = get(applyAction);
      await (action === null ? jobs.scan('full') : action());
    } finally {
      applying = false;
    }
  }
</script>

{#if $unapplied || $foldersWaiting}
  <div class="apply">
    <button
      type="button"
      data-testid="folders-apply"
      disabled={applying || scanRunning}
      onclick={apply}>{label}</button>
  </div>
{/if}

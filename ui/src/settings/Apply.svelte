<script lang="ts">
  // Owner, 2026-11-12: a stored tick or mask does nothing until a scan, and
  // nothing on screen said so. The last thing in the folders section, under
  // the masks: "Apply changes" starts a full scan, and goes once one runs.
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import type { JobController } from './jobs';
  import { unapplied } from './unapplied';

  let { jobs }: { jobs: JobController } = $props();
  // The controller lives for the window; read once, as `JobStrip.svelte` does.
  // svelte-ignore state_referenced_locally
  const jobState = jobs.state;
  const scanRunning = $derived($jobState.scan.snapshot.kind === 'running');
  let applying = $state(false);

  // A scan from anywhere (the tray, the Scanning section) applies the change too.
  onMount(() => jobs.state.subscribe(({ scan }) => {
    if (scan.snapshot.kind === 'running') unapplied.set(false);
  }));

  const label = $derived.by(() => { void $locale; return t('settings_folders_apply'); });

  async function apply() {
    applying = true;
    try {
      await jobs.scan('full');
    } finally {
      applying = false;
    }
  }
</script>

{#if $unapplied}
  <div class="apply">
    <button
      type="button"
      data-testid="folders-apply"
      disabled={applying || scanRunning}
      onclick={apply}>{label}</button>
  </div>
{/if}

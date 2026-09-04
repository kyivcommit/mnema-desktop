<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import Models from './Models.svelte';
  import Folders from './Folders.svelte';
  import Masks from './Masks.svelte';
  import JobStrip from './JobStrip.svelte';
  import Indexing from './Indexing.svelte';
  import Application from './Application.svelte';
  import { createJobController } from './jobs';

  // All four sections render; hiding one not yet built would make the window
  // claim the product has fewer sections than the spec does. Order matches the
  // spec and the mockup's `snav` column.
  type SectionId = 'models' | 'folders' | 'indexing' | 'application';
  const SECTIONS: SectionId[] = ['models', 'folders', 'indexing', 'application'];

  let section = $state<SectionId>('models');

  // 🔴 ONE controller, here, above every section — not inside the one that
  // starts the job. Four nav items make Folders -> Models -> Folders two
  // clicks, and the channel a job reports on belongs to whoever started it
  // (`bridge.rs`): a controller living in `Folders.svelte` would be destroyed
  // by the first of those clicks, taking the counters AND the Cancel button
  // with it. `cancel_job` needs no channel at all, so that Cancel would be lost
  // for nothing. The strip renders outside the panel for the same reason — a
  // running job stays visible and stoppable from every section. WHERE outside
  // is the live run's finding 3; see the markup below.
  const jobs = createJobController();

  // The settings window can be opened in the middle of a run it never started,
  // and `set_embedding_model` takes the same slot without ever sending an
  // ending. `job_status` is the only honest answer to "is something running",
  // and the controller is careful to write only where it cannot destroy
  // something better.
  onMount(() => { void jobs.syncFromStatus(); });

  const modelsLabel = $derived.by(() => { void $locale; return t('settings_nav_models'); });
  const foldersLabel = $derived.by(() => { void $locale; return t('settings_nav_folders'); });
  const indexingLabel = $derived.by(() => { void $locale; return t('settings_nav_indexing'); });
  const applicationLabel = $derived.by(() => { void $locale; return t('settings_nav_application'); });

  function labelFor(id: SectionId): string {
    switch (id) {
      case 'models': return modelsLabel;
      case 'folders': return foldersLabel;
      case 'indexing': return indexingLabel;
      case 'application': return applicationLabel;
    }
  }
</script>

<main>
  <!-- Live run, finding 3, and it is a correction to this plan's own ruling.
       Task 8 put the strip OUTSIDE the section conditional so a job survives
       Folders -> Models -> Folders with its counters and its Cancel; that half
       is right and is untouched here. What it did not weigh is WHERE outside:
       rendered last, the strip sat under whatever the panel ended with, so a
       person standing on a section read that section's closing line and,
       immediately below it, the full indexing report.
       The strip is the WINDOW's status line, not a section's content, so it is
       drawn before the nav and the panel both. Nothing about the controller
       moved: it is still created above every section, and `cancel_job` still
       needs no channel.
       ⚠️ The component outside every `{#if}` is this one, `<JobStrip>`.
       `<Indexing>` is INSIDE the conditional below and depends on being there:
       its per-mount `refresh()` is the only re-read of `model_settings` after a
       change made while that section was off screen, and the store subscription
       it opens is torn down by the unmount a nav change causes. Hoisting it out
       would quietly cost both — the sentence that stood here said the opposite
       and would have invited exactly that.
       `.scols` exists so the CSS that lands later cannot make this a THIRD
       column beside the nav and the panel: the pair is the row, the status line
       is not part of it. -->
  <JobStrip {jobs} />
  <div class="scols">
    <nav class="snav">
      {#each SECTIONS as id (id)}
        <button
          type="button"
          class="item"
          data-testid={`settings-nav-${id}`}
          aria-pressed={section === id}
          onclick={() => (section = id)}
        >{labelFor(id)}</button>
      {/each}
    </nav>

    <div class="spane">
      {#if section === 'models'}
        <h2>{modelsLabel}</h2>
        <Models {jobs} />
      {:else if section === 'folders'}
        <h2>{foldersLabel}</h2>
        <Folders {jobs} />
        <!-- Beside the folder list, never inside a folder row (§9.2, D-c): a
             mask is global to the index, so drawing it under one root would
             say it belongs to that root. It takes no `jobs` — nothing here
             starts a job. -->
        <Masks />
      {:else if section === 'indexing'}
        <h2>{indexingLabel}</h2>
        <!-- §9.3 — what the index holds. It takes `jobs` to hear an ending,
             which is the one moment its numbers can have changed; it starts
             nothing itself. The running pass is the strip above the nav. -->
        <Indexing {jobs} />
      {:else if section === 'application'}
        <h2>{applicationLabel}</h2>
        <!-- §9.4 — the shortcut, autostart, and the version. It takes no
             `jobs`: this section starts nothing and shares no controller. -->
        <Application />
      {/if}
    </div>
  </div>
</main>

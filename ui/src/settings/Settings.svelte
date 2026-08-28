<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import Models from './Models.svelte';
  import Folders from './Folders.svelte';
  import Indexing from './Indexing.svelte';
  import { createJobController } from './jobs';

  // All four sections render; hiding the two not yet built would make the
  // window claim the product has two sections when the spec says four.
  // Order matches the spec and the mockup's `snav` column.
  type SectionId = 'models' | 'folders' | 'indexing' | 'application';
  const SECTIONS: { id: SectionId; disabled: boolean }[] = [
    { id: 'models', disabled: false },
    { id: 'folders', disabled: false },
    { id: 'indexing', disabled: true },
    { id: 'application', disabled: true },
  ];

  let section = $state<SectionId>('models');

  // 🔴 ONE controller, here, above every section — not inside the one that
  // starts the job. Four nav items make Folders -> Models -> Folders two
  // clicks, and the channel a job reports on belongs to whoever started it
  // (`bridge.rs`): a controller living in `Folders.svelte` would be destroyed
  // by the first of those clicks, taking the counters AND the Cancel button
  // with it. `cancel_job` needs no channel at all, so that Cancel would be lost
  // for nothing. The strip renders below the panel for the same reason — a
  // running job stays visible and stoppable from every section.
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
  // One catalogue sentence, shared by both unbuilt sections. An unbuilt
  // section's button is `aria-describedby` this sentence, so the reason its
  // panel is empty reaches a screen reader as the button's own description.
  // The condition is the same one that renders the sentence: the reference is
  // set only while the element it names exists, never dangling.
  // `aria-disabled` was removed here on the owner's ruling — these buttons are
  // fully operable, and telling assistive technology they are disabled meant
  // its users would never press them and so never hear the sentence at all.
  const NOT_READY_ID = 'section-not-ready';
  const notReadyLabel = $derived.by(() => { void $locale; return t('settings_section_not_ready'); });

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
  <nav class="snav">
    {#each SECTIONS as item (item.id)}
      <button
        type="button"
        class="item"
        data-testid={`settings-nav-${item.id}`}
        aria-pressed={section === item.id}
        aria-describedby={section === item.id && item.disabled ? NOT_READY_ID : undefined}
        onclick={() => (section = item.id)}
      >{labelFor(item.id)}</button>
    {/each}
  </nav>

  <div class="spane">
    {#if section === 'models'}
      <h2>{modelsLabel}</h2>
      <Models />
    {:else if section === 'folders'}
      <h2>{foldersLabel}</h2>
      <Folders {jobs} />
    {:else if section === 'indexing'}
      <h2>{indexingLabel}</h2>
      <p id={NOT_READY_ID}>{notReadyLabel}</p>
    {:else if section === 'application'}
      <h2>{applicationLabel}</h2>
      <p id={NOT_READY_ID}>{notReadyLabel}</p>
    {/if}
  </div>

  <Indexing {jobs} />
</main>

<script lang="ts">
  import { locale, t } from '../i18n';

  // D-b, settled: all four sections render; hiding the two not yet built
  // would make the window claim the product has two sections when the spec
  // says four. Order matches the spec and the mockup's `snav` column.
  type SectionId = 'models' | 'folders' | 'indexing' | 'application';
  const SECTIONS: { id: SectionId; disabled: boolean }[] = [
    { id: 'models', disabled: false },
    { id: 'folders', disabled: false },
    { id: 'indexing', disabled: true },
    { id: 'application', disabled: true },
  ];

  let section = $state<SectionId>('models');

  const modelsLabel = $derived.by(() => { void $locale; return t('settings_nav_models'); });
  const foldersLabel = $derived.by(() => { void $locale; return t('settings_nav_folders'); });
  const indexingLabel = $derived.by(() => { void $locale; return t('settings_nav_indexing'); });
  const applicationLabel = $derived.by(() => { void $locale; return t('settings_nav_application'); });
  // D-b: one catalogue sentence, shared by both unbuilt sections.
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
        aria-disabled={item.disabled}
        onclick={() => (section = item.id)}
      >{labelFor(item.id)}</button>
    {/each}
  </nav>

  <div class="spane">
    <!-- Models and Folders: a heading and nothing else — Models.svelte and
         Folders.svelte mount here in later tasks (4-8). -->
    {#if section === 'models'}
      <h2>{modelsLabel}</h2>
    {:else if section === 'folders'}
      <h2>{foldersLabel}</h2>
    {:else if section === 'indexing'}
      <h2>{indexingLabel}</h2>
      <p>{notReadyLabel}</p>
    {:else if section === 'application'}
      <h2>{applicationLabel}</h2>
      <p>{notReadyLabel}</p>
    {/if}
  </div>
</main>

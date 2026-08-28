<script lang="ts">
  import { locale, t } from '../i18n';
  import Models from './Models.svelte';

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
    <!-- Folders: a heading and nothing else — Folders.svelte mounts here in a
         later task (7/8). Models is real content as of Task 4. -->
    {#if section === 'models'}
      <h2>{modelsLabel}</h2>
      <Models />
    {:else if section === 'folders'}
      <h2>{foldersLabel}</h2>
    {:else if section === 'indexing'}
      <h2>{indexingLabel}</h2>
      <p id={NOT_READY_ID}>{notReadyLabel}</p>
    {:else if section === 'application'}
      <h2>{applicationLabel}</h2>
      <p id={NOT_READY_ID}>{notReadyLabel}</p>
    {/if}
  </div>
</main>

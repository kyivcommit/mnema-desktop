<script lang="ts">
  import { onMount } from 'svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import { locale, t } from '../i18n';
  import { listTree, addWatchedFolder, removeWatchedFolder, type TreeRoot } from '../lib/ipc';

  // §9.2, Task 7 — the minimum folder surface: add, list, remove. Running a
  // scan (`start_walk_job`) is Task 8's own control, deliberately absent from
  // this file: D-c reserves the scan for a button pressed once a folder's own
  // configuration (subfolder exclusion, PR 8) is finished, and a walk that
  // starts on `add_watched_folder` would run before that chance exists. This
  // component simply never imports the command that would let it happen.
  //
  // Fixture question: `TreeRoot` (`ipc.ts`) carries `files: TreeFile[]` and no
  // flag for whether a walk has ever run. A folder just added and a folder
  // walked to genuine completion with nothing in it are the SAME value on
  // this wire — so this screen never says a folder is "empty"; it states the
  // count `indexed_documents` gives, which is true of both states, and stops
  // there. Telling the two apart belongs to the task that runs the walk.
  let roots = $state<TreeRoot[]>([]);
  // Set on a rejected `list_tree`, at mount or on a later re-read. Held apart
  // from `actionError` (Models.svelte's own precedent) because this one means
  // the list on screen cannot be trusted at all, not that one action failed.
  let loadError = $state<string | null>(null);
  // Set on a rejected add/remove. The backend's own sentence, shown verbatim
  // beside no lead-in of this component's own — §10: a rejection arrives as
  // a sentence, never a kind, and no re-read is implied by it.
  let actionError = $state<string | null>(null);

  async function refresh() {
    const listing = await listTree();
    roots = listing.roots;
    loadError = null; // a successful read is proof the earlier one is stale
  }

  onMount(() => {
    refresh().catch((e) => {
      loadError = e instanceof Error ? e.message : String(e);
    });
  });

  async function addFolder() {
    actionError = null;
    const selected = await open({ directory: true });
    if (selected === null) return; // cancelled dialog — calls nothing further
    try {
      await addWatchedFolder(selected);
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
      return;
    }
    // Outside the action's own try: a rejection here is list_tree's own
    // sentence, about the list being unreadable, not about the add having
    // failed — the add already succeeded by this point.
    try {
      await refresh();
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
  }

  async function removeFolder(rootId: number) {
    actionError = null;
    try {
      await removeWatchedFolder(rootId);
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
      return;
    }
    try {
      await refresh();
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
  }

  const emptyLabel = $derived.by(() => { void $locale; return t('settings_folders_empty'); });
  const addLabel = $derived.by(() => { void $locale; return t('settings_folders_add'); });
  const removeLabel = $derived.by(() => { void $locale; return t('settings_folders_remove'); });
  const loadFailedLabel = $derived.by(() => { void $locale; return t('settings_folders_load_failed'); });

  // Rows carry their own count label, not a bare `t()` call in the markup —
  // a call inside `{#each}` reads `get(locale)` non-reactively (`i18n/index.ts:11`)
  // and would not update on a language switch. The whole array is rebuilt
  // under `void $locale` instead, the shape `launcher/Tree.svelte`'s own
  // `recents` derived already uses for the same reason.
  //
  // `settings_folders_indexed`, not the shared `indexed_documents` (P2-4
  // review): a bare "0 documents" beside a folder path reads as a claim
  // about the FOLDER, and D-c means every newly added folder shows zero
  // forever, not transiently — this key names the INDEX as the subject
  // ("Indexed: 0 documents") instead. `removeAriaLabel` carries the same
  // row's path so two "Remove" buttons in a two-folder list stay
  // distinguishable to a screen reader (P2-5); the visible button text
  // stays the plain `removeLabel` above.
  const rows = $derived.by(() => {
    void $locale;
    return roots.map((root) => ({
      root,
      countLabel: t('settings_folders_indexed', { count: root.files.length }),
      removeAriaLabel: t('settings_folders_remove_named', { path: root.absolutePath }),
    }));
  });
</script>

<div class="folders">
  {#if loadError}
    <p>{loadFailedLabel}</p>
    <p data-testid="folders-load-reason">{loadError}</p>
  {:else if rows.length === 0}
    <p>{emptyLabel}</p>
  {:else}
    <ul>
      {#each rows as { root, countLabel, removeAriaLabel } (root.rootId)}
        <li data-testid={`folder-row-${root.rootId}`}>
          <span>{root.absolutePath}</span>
          <span>{countLabel}</span>
          <button type="button" aria-label={removeAriaLabel} onclick={() => removeFolder(root.rootId)}>{removeLabel}</button>
        </li>
      {/each}
    </ul>
  {/if}
  <button type="button" onclick={addFolder}>{addLabel}</button>
  {#if actionError}<p data-testid="folders-action-error">{actionError}</p>{/if}
</div>

<script lang="ts">
  import { untrack } from 'svelte';
  import { locale, t } from '../i18n';
  import { formatIndexedAt } from '../i18n/recency';
  import { deleteHistoryEntry, listHistory } from '../lib/ipc';
  import type { HistoryEntry } from '../lib/ipc';

  // §4: the conversations, beside the transcript. A row is the conversation's
  // first question (one line, clipped), how long ago it was asked, and × —
  // `delete_history_entry` has no other caller. Grouping by date ("today /
  // yesterday / last 7 days") earns its space past ten entries; not in v1.
  //
  // `activeId` is the conversation `historyId` names — the row a fresh answer
  // just wrote, or an opened one. `gen` is the launcher's signal that the list
  // may have changed. `error` is an open that failed in the launcher: it
  // belongs beside the list the person clicked, not under the composer.
  let {
    activeId = null,
    gen = 0,
    onOpen,
    onDelete,
    error = null,
  }: {
    activeId?: number | null;
    gen?: number;
    onOpen: (id: number) => void;
    onDelete?: (id: number) => void;
    error?: 'historyUnreadable' | 'historyFailed' | null;
  } = $props();

  // `null` until the first read lands, so an empty list is not drawn before the
  // question of whether it is empty has been answered.
  let entries = $state<HistoryEntry[] | null>(null);
  // A first read that failed, while nothing is on screen. A read that fails
  // after a list is already shown keeps the list, as `list_tree` does.
  let failed = $state(false);

  // Two reads can be in flight at once, and the one that started last counts:
  // a reply to an older read is not applied over a newer answer, but a read
  // still on the wire does not stop a newer one from being asked for.
  let asked = 0;
  let applied = 0;
  function load() {
    const mine = ++asked;
    listHistory()
      .then((h) => {
        if (mine > applied) { applied = mine; entries = h; failed = false; }
      })
      .catch((e) => {
        console.error('list_history failed', e);
        if (entries === null) failed = true;
      });
  }

  // Read when the list may have changed: on mount, when the window regains
  // focus (the conversation may have been written in another window, and this
  // one is hidden, not closed), and when `gen` moves. The effect depends on the
  // number, not on the prop read: a parent that re-sets its props must not read
  // the list again, and an equal value does not propagate through `$derived`.
  // It runs once on mount too, which is the first read.
  const generation = $derived(gen);
  $effect(() => {
    void generation;
    untrack(load);
  });

  // The row goes once the delete has resolved and the list has been read
  // again, so a delete that fails leaves the row where it was. Nothing is
  // confirmed and nothing is opened.
  function remove(id: number) {
    deleteHistoryEntry(id)
      .then(() => { onDelete?.(id); load(); })
      .catch((e) => console.error('delete_history_entry failed', e));
  }

  const emptyLabel = $derived.by(() => { void $locale; return t('history_empty'); });
  const failedLabel = $derived.by(() => { void $locale; return t('recents_failed'); });
  const errorLabel = $derived.by(() => {
    void $locale;
    return error === 'historyUnreadable' ? t('history_unreadable') : error === 'historyFailed' ? t('history_open_failed') : '';
  });

  // Each row says HOW LONG AGO it was asked. The whole list is rebuilt inside
  // one `$derived.by` that reads `$locale` — `formatIndexedAt` goes through
  // `t()` and is not reactive on its own (the house pattern). The clock is read
  // here rather than ticked: these labels are recomputed when the list changes,
  // when the language changes, and whenever the window regains focus, which is
  // the only moment a person is reading them.
  const rows = $derived.by(() => {
    void $locale;
    const now = Date.now();
    return (entries ?? []).map((entry) => ({
      entry,
      asked: formatIndexedAt(entry.askedAt, now),
      // Named for its own question: a screen reader hears the row, not a count.
      removeName: t('history_remove', { query: entry.query }),
    }));
  });
</script>

<svelte:window onfocus={load} />

<div class="recents-body">
  {#if error}
    <p class="guard" role="alert" data-testid="recents-error">{errorLabel}</p>
  {/if}
  {#if failed}
    <p data-testid="recents-failed">{failedLabel}</p>
  {:else if entries !== null && entries.length === 0}
    <p data-testid="recents-empty">{emptyLabel}</p>
  {:else}
    <ul>
      {#each rows as { entry, asked, removeName } (entry.id)}
        <li>
          <!-- The row is a span, not a button: its two buttons are the actions.
               `aria-current` sits on the row because the style keys on it there,
               and the row is what the launcher's `historyId` marks. -->
          <span class="history-row" data-testid={`recents-row-${entry.id}`}
            aria-current={entry.id === activeId ? 'true' : undefined}>
            <button type="button" class="history-open" data-testid={`recents-open-${entry.id}`}
              onclick={() => onOpen(entry.id)}>
              <span data-testid="recents-query" class="history-query">{entry.query}</span>
              <span data-testid="recents-asked" class="history-asked">{asked}</span>
            </button>
            <!-- Shown on hover and on focus, through opacity and never through
                 `visibility`: a hidden control cannot take focus, and this one
                 must always be reachable from a keyboard. -->
            <button type="button" class="history-remove" data-testid={`recents-remove-${entry.id}`}
              aria-label={removeName}
              onclick={() => remove(entry.id)}>×</button>
          </span>
        </li>
      {/each}
    </ul>
  {/if}
</div>

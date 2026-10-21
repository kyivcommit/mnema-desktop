<script lang="ts">
  import { untrack } from 'svelte';
  import { locale, t } from '../i18n';
  import { dayBucket, type DayBucket } from '../i18n/recency';
  import { deleteHistoryEntry, listHistory } from '../lib/ipc';
  import type { HistoryEntry } from '../lib/ipc';

  // §4: the conversations, beside the transcript. A row is the conversation's
  // first question and one clipped line of its answer; when it was asked is
  // said once, by the date group it sits in, rather than on every row. × is
  // `delete_history_entry`'s only caller.
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
    onNew,
    newDisabled = false,
    error = null,
  }: {
    activeId?: number | null;
    gen?: number;
    onOpen: (id: number) => void;
    onDelete?: (id: number) => void;
    onNew?: () => void;
    newDisabled?: boolean;
    error?: 'historyUnreadable' | 'historyFailed' | null;
  } = $props();

  // `null` until the first read lands, so an empty list is not drawn before the
  // question of whether it is empty has been answered.
  let entries = $state<HistoryEntry[] | null>(null);
  // A first read that failed, while nothing is on screen. A read that fails
  // after a list is already shown keeps the list, as `list_tree` does — unless
  // the filter moved since that list was read, in which case the rows on screen
  // answer a different question and saying so is better than showing them.
  let failed = $state(false);
  // What the person is looking for. Sent to the backend, which matches the
  // question and the whole stored answer — the drawn preview is one line of it.
  let filter = $state('');
  // The filter the rows on screen were actually read with; the empty sentence
  // keys off this, not off the live field, or clearing it would flash "your
  // questions will appear here" over the previous result.
  let appliedFilter = $state('');

  // Two reads can be in flight at once, and only the one that started LAST may
  // write anything — success or failure. A read still on the wire does not stop
  // a newer one from being asked for, and a reply from any other read is dropped
  // whichever way it went: a stale success would paint rows for a filter the
  // field no longer holds, and a stale failure would hide a list that a newer
  // read had just painted.
  let asked = 0;
  function load() {
    const mine = ++asked;
    // Captured once, at the request: a reply must be stamped with the filter it
    // was asked for, not with whatever the field says when it lands.
    const requested = filter.trim();
    listHistory(requested || undefined)
      .then((h) => {
        if (mine === asked) { entries = h; appliedFilter = requested; failed = false; }
      })
      .catch((e) => {
        console.error('list_history failed', e);
        if (mine === asked && (entries === null || requested !== appliedFilter)) failed = true;
      });
  }

  // Read when the list may have changed: on mount, when the window regains
  // focus (the conversation may have been written in another window, and this
  // one is hidden, not closed), and when `gen` moves. The effect depends on the
  // number, not on the prop read: a parent that re-sets its props must not read
  // the list again, and an equal value does not propagate through `$derived`.
  // It runs once on mount too, which is the first read.
  //
  // A keystroke reads again, and without a debounce: the backend matches fifty
  // in-memory rows, and `asked`/`applied` above already keep an older reply
  // from landing over a newer one.
  const generation = $derived(gen);
  $effect(() => {
    void generation;
    void filter;
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
  // A filtered list that came back empty is not an empty history: the old
  // sentence would say there are no questions while they sit one word away.
  const nothingFoundLabel = $derived.by(() => { void $locale; return t('history_nothing_found'); });
  const failedLabel = $derived.by(() => { void $locale; return t('recents_failed'); });
  const errorLabel = $derived.by(() => {
    void $locale;
    return error === 'historyUnreadable' ? t('history_unreadable') : error === 'historyFailed' ? t('history_open_failed') : '';
  });
  const labels = $derived.by(() => {
    void $locale;
    return {
      title: t('toggle_recents'), compose: t('new_conversation'),
      search: t('history_search_placeholder'), searchName: t('history_search_label'),
    };
  });

  // The order the headings are drawn in, newest first; an empty bucket is not drawn.
  const ORDER: DayBucket[] = ['today', 'yesterday', 'week', 'earlier'];
  const GROUP_LABEL = {
    today: 'history_group_today',
    yesterday: 'history_group_yesterday',
    week: 'history_group_week',
    earlier: 'history_group_earlier',
  } as const;

  type Row = { entry: HistoryEntry; removeName: string };

  // The whole list is rebuilt inside one `$derived.by` that reads `$locale` —
  // `t()` and `dayBucket`'s labels are not reactive on their own (the house
  // pattern). The clock is read here rather than ticked: a row only changes
  // group when the list is read again or the language changes, which are the
  // moments this block recomputes. Entries arrive newest first, so each bucket
  // keeps that order without sorting.
  const groups = $derived.by(() => {
    void $locale;
    const now = Date.now();
    const buckets = ORDER.map((bucket) => ({ bucket, label: t(GROUP_LABEL[bucket]), rows: [] as Row[] }));
    for (const entry of entries ?? []) {
      buckets.find((b) => b.bucket === dayBucket(entry.askedAt, now))!.rows.push({
        entry,
        // Named for its own question: a screen reader hears the row, not a count.
        removeName: t('history_remove', { query: entry.query }),
      });
    }
    return buckets.filter((b) => b.rows.length > 0);
  });
</script>

<svelte:window onfocus={load} />

<!-- Pinned above the list: the panel's name, and the second way to start a
     conversation. It must not scroll away with fifty rows. -->
<header class="recents-head">
  <h2 class="recents-title" data-testid="recents-title">{labels.title}</h2>
  <button type="button" class="tb" data-testid="recents-new"
    aria-label={labels.compose} title={labels.compose} disabled={newDisabled}
    onclick={() => onNew?.()}>
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10.9 2.6a1.3 1.3 0 0 1 1.9 0l.6.6a1.3 1.3 0 0 1 0 1.9L6.3 12.2l-3 .7.7-3z"/></svg>
  </button>
</header>

<!-- The filter sits with the header, outside the body that scrolls: a field
     that scrolls away from the list it filters is not usable over fifty rows.
     The label is drawn for a screen reader only — the placeholder carries the
     same word on screen, and a visible label above a 34px field adds nothing. -->
<div class="recents-search">
  <label for="recents-filter">{labels.searchName}</label>
  <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor"
    stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
    <circle cx="7" cy="7" r="4.5"/><path d="M10.4 10.4 14 14"/>
  </svg>
  <input id="recents-filter" type="search" data-testid="recents-filter"
    placeholder={labels.search} bind:value={filter} />
</div>

<div class="recents-body">
  {#if error}
    <p class="guard" role="alert" data-testid="recents-error">{errorLabel}</p>
  {/if}
  {#if failed}
    <p data-testid="recents-failed">{failedLabel}</p>
  {:else if entries !== null && entries.length === 0}
    {#if appliedFilter}
      <p data-testid="recents-nothing-found">{nothingFoundLabel}</p>
    {:else}
      <p data-testid="recents-empty">{emptyLabel}</p>
    {/if}
  {:else}
    {#each groups as group (group.bucket)}
      <section class="recents-group" data-testid={`recents-group-${group.bucket}`}>
        <h3 id={`recents-group-${group.bucket}-label`}>{group.label}</h3>
        <ul aria-labelledby={`recents-group-${group.bucket}-label`}>
          {#each group.rows as { entry, removeName } (entry.id)}
            <li>
              <!-- The row is a span, not a button: its two buttons are the actions.
                   `aria-current` sits on the row because the style keys on it there,
                   and the row is what the launcher's `historyId` marks. -->
              <span class="history-row" data-testid={`recents-row-${entry.id}`}
                aria-current={entry.id === activeId ? 'true' : undefined}>
                <button type="button" class="history-open" data-testid={`recents-open-${entry.id}`}
                  onclick={() => onOpen(entry.id)}>
                  <span data-testid="recents-query" class="history-query">{entry.query}</span>
                  <!-- An answer with nothing to preview draws one line, not a blank second one. -->
                  {#if entry.preview}
                    <span data-testid="recents-preview" class="history-preview">{entry.preview}</span>
                  {/if}
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
      </section>
    {/each}
  {/if}
</div>

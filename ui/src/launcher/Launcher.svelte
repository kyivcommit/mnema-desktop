<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { locale, t } from '../i18n';
  import { ask, modelSettings } from '../lib/ipc';
  import { checkQuery, stateFromAnswer, providerReady, type LauncherState } from './state';
  import Arms from './Arms.svelte';
  import SearchLine from './SearchLine.svelte';
  import Cards from './Cards.svelte';

  let query = $state('');
  let echo = $state('');
  let pinned = $state(false);
  let launcherState = $state<LauncherState>({ kind: 'idle' });
  let provider = $state(false);
  let textOn = $state(true);
  let contentOn = $state(false);

  const appWindow = getCurrentWebviewWindow();
  const pinLabel = $derived.by(() => { void $locale; return `${t('pin')} 📌`; });

  onMount(() => {
    // Seed the arms row once. Non-fatal: on failure the row stays on its
    // text-only default rather than blocking the launcher — log, do not
    // swallow.
    modelSettings()
      .then((s) => {
        provider = providerReady(s);
        if (s.index.kind === 'read') { textOn = s.index.searchTextArm; contentOn = s.index.searchContentArm; }
      })
      .catch((e) => console.error('model_settings failed', e));
  });

  // The owner validates and calls ask — the whole machine goes through
  // state.ts. A rejected ask becomes a visible error, never a silent reset:
  // an eaten error is easy to miss.
  async function runSearch(raw: string) {
    if (launcherState.kind === 'inFlight') return; // one ask at a time
    echo = '';
    const check = checkQuery(raw);
    if (!check.ok) { launcherState = { kind: 'error', reason: check.reason }; return; }
    launcherState = { kind: 'inFlight', query: check.query };
    try {
      const answer = await ask(check.query);
      launcherState = stateFromAnswer(check.query, answer);
      // §7: line clears on ready — but only if it still holds the submitted
      // query. A draft typed while the ask was in flight is kept, not wiped
      // (Codex #3).
      if (query === raw) query = '';
      echo = check.query;  // §7: query echoes as a chat bubble
    } catch (e) {
      console.error('ask failed', e); // query stays in the line for a retry
      launcherState = { kind: 'error', reason: 'askFailed' };
    }
  }

  // Hide, never close: a hidden webview keeps state, so `query` and results
  // survive dismissal (§7.3).
  function hide() { appWindow.hide(); }
  function onKeydown(event: KeyboardEvent) { if (event.key === 'Escape') hide(); }
  function onBlur() { if (!pinned) hide(); }
</script>

<svelte:window onkeydown={onKeydown} onblur={onBlur} />

<main>
  <SearchLine bind:query state={launcherState} onSubmit={runSearch} />
  <Arms bind:textOn bind:contentOn {provider} />

  {#if echo}
    <div class="query-echo" data-testid="query-echo">{echo}</div>
  {/if}

  <Cards state={launcherState} query={echo} />

  <button
    class="pin"
    class:active={pinned}
    aria-pressed={pinned}
    aria-label={pinLabel}
    onclick={() => (pinned = !pinned)}>📌</button>
</main>

<script lang="ts">
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { locale, t } from '../i18n';
  import { ask } from '../lib/ipc';
  import { checkQuery, stateFromAnswer, type LauncherState } from './state';
  import SearchLine from './SearchLine.svelte';

  let query = $state('');
  let pinned = $state(false);
  let launcherState = $state<LauncherState>({ kind: 'idle' });

  const appWindow = getCurrentWebviewWindow();
  const pinLabel = $derived.by(() => { void $locale; return `${t('pin')} 📌`; });

  // The owner validates and calls ask — the whole machine goes through state.ts
  // (Findings 1/3). A rejected ask becomes a visible error, never a silent reset
  // (the F2/PR#19 lesson: an eaten error is a class the owner has already caught).
  async function runSearch(raw: string) {
    if (launcherState.kind === 'inFlight') return; // one ask at a time (Finding 5)
    const check = checkQuery(raw);
    if (!check.ok) { launcherState = { kind: 'error', reason: check.reason }; return; }
    launcherState = { kind: 'inFlight', query: check.query };
    try {
      const answer = await ask(check.query);
      launcherState = stateFromAnswer(check.query, answer);
    } catch (e) {
      console.error('ask failed', e);
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

  {#if launcherState.kind === 'generated'}
    <div data-testid="answer-stub">{launcherState.answer.answer}</div>
  {:else if launcherState.kind === 'citationsOnly'}
    <div data-testid="citations-stub">{launcherState.answer.citations.length}</div>
  {/if}

  <button
    class="pin"
    class:active={pinned}
    aria-pressed={pinned}
    aria-label={pinLabel}
    onclick={() => (pinned = !pinned)}>📌</button>
</main>

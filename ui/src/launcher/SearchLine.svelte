<script lang="ts">
  import { locale, t } from '../i18n';
  import { refusalText } from '../i18n/refusal';
  import { MAX_ASK_QUERY, type LauncherState } from './state';

  let { state, onSubmit, query = $bindable('') }: {
    state: LauncherState;
    onSubmit: (raw: string) => void;
    query?: string;
  } = $props();

  const placeholder = $derived.by(() => { void $locale; return t('search_placeholder'); });

  // Every message is driven by the machine's state, not a local guard — so a
  // rejected `ask` (askFailed) is as visible as a too-long one.
  // void $locale so the text follows a live language switch.
  const errorText = $derived.by(() => {
    void $locale;
    if (state.kind !== 'error') return '';
    if (state.reason === 'tooLong') return t('query_too_long', { limit: MAX_ASK_QUERY });
    return t('query_failed'); // askFailed
  });
  const refusalMessage = $derived.by(() => {
    void $locale;
    return state.kind === 'refused' ? refusalText(state.reason.kind) : '';
  });

  function onKeydown(event: KeyboardEvent) {
    // A held Enter auto-repeats keydown; only the press submits, or the repeat
    //, or a failed answer (which keeps the query) would be asked again by the repeat.
    if (event.key === 'Enter' && !event.repeat) onSubmit(query);
  }

  // Every show of the launcher (⌥Space, the tray, single-instance) ends in
  // `set_focus`, which reaches the webview as a window `focus`: the cursor goes
  // to the line, so the person can type without a click first.
  let input: HTMLInputElement;
</script>

<svelte:window onfocus={() => input.focus()} />

<div class="search-line">
  <input type="text" bind:this={input} bind:value={query} placeholder={placeholder} aria-busy={state.kind === 'inFlight' ? 'true' : undefined} onkeydown={onKeydown} />
  <!-- A too-long message belongs to the line that was too long: stale once emptied. -->
  {#if state.kind === 'error' && !(state.reason === 'tooLong' && query.trim() === '')}
    <p class="guard" role="alert">{errorText}</p>
  {:else if state.kind === 'refused'}
    <p class="refusal" role="status">{refusalMessage}</p>
  {/if}
</div>

<script lang="ts">
  import { locale, t } from '../i18n';
  import { refusalText } from '../i18n/refusal';
  import type { LauncherState } from './state';

  let { state, onSubmit, query = $bindable('') }: {
    state: LauncherState;
    onSubmit: (raw: string) => void;
    query?: string;
  } = $props();

  const placeholder = $derived.by(() => { void $locale; return t('search_placeholder'); });

  // Every message is driven by the machine's state, not a local guard — so a
  // rejected `ask` (askFailed) is as visible as a blank query (Findings 1/3).
  // void $locale so the text follows a live language switch.
  const errorText = $derived.by(() => {
    void $locale;
    if (state.kind !== 'error') return '';
    if (state.reason === 'blank') return t('query_blank');
    if (state.reason === 'tooLong') return t('query_too_long', { limit: 2048 });
    return t('query_failed'); // askFailed
  });
  const refusalMessage = $derived.by(() => {
    void $locale;
    return state.kind === 'refused' ? refusalText(state.reason.kind) : '';
  });

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') onSubmit(query);
  }
</script>

<div class="search-line">
  <input type="text" bind:value={query} placeholder={placeholder} onkeydown={onKeydown} />
  {#if state.kind === 'error'}
    <p class="guard" role="alert">{errorText}</p>
  {:else if state.kind === 'refused'}
    <p class="refusal" role="status">{refusalMessage}</p>
  {/if}
</div>

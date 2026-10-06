<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { locale, t } from '../i18n';
  import {
    mnemaAvailable, providerChoice, setProviderChoice, localModels, downloadModel, cancelDownload,
    removeModel, listenLocalModelProgress, modelSettings,
    type RetiredSpace, type ExistingVectors,
    type ProviderChoice, type LocalModelId, type LocalModelState, type DownloadError,
  } from '../lib/ipc';

  // The OpenRouter key and model controls: shown while OpenRouter is the
  // provider, replaced by the local models' rows while Mnema is.
  let { children }: { children?: Snippet } = $props();

  let available = $state(false);
  let choice = $state<ProviderChoice>('openRouter');

  // `provider.rs`'s `LOCAL_EMBED_MODEL`: the one name both providers embed under.
  const LOCAL_EMBED_MODEL = 'baai/bge-m3';
  const IDS: LocalModelId[] = ['embed', 'chat'];
  let states = $state<Record<LocalModelId, LocalModelState>>({
    embed: { kind: 'absent' }, chat: { kind: 'absent' },
  });
  // What belongs to a row and not to the re-read: a download this window has in
  // flight, how far it has got, and why the last one stopped. A re-read of
  // `local_models` must not erase the reason, so it lives apart from `states`.
  let active = $state<Record<LocalModelId, boolean>>({ embed: false, chat: false });
  let progress = $state<Record<LocalModelId, number>>({ embed: 0, chat: 0 });
  let failure = $state<Record<LocalModelId, DownloadError | null>>({ embed: null, chat: null });

  async function readRows() {
    try {
      for (const row of await localModels()) states[row.id] = row.state;
    } catch (e) {
      console.error('local_models failed', e);
    }
  }

  async function download(id: LocalModelId) {
    failure[id] = null;
    progress[id] = 0;
    active[id] = true;
    try {
      await downloadModel(id);
    } catch (e) {
      const err = e as DownloadError;
      if (err && typeof err === 'object' && 'kind' in err) {
        if (err.kind !== 'cancelled') failure[id] = err;
      } else {
        failure[id] = { kind: 'failed', message: e instanceof Error ? e.message : String(e) };
      }
    }
    active[id] = false;
    await readRows();
  }

  // A row can say "downloading" for a download this window never started (one
  // from an earlier Settings window), so no `download()` call is waiting to
  // re-read it: the cancel re-reads the real state itself.
  async function cancel(id: LocalModelId) {
    try {
      await cancelDownload(id);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
    await readRows();
  }

  async function remove(id: LocalModelId) {
    try {
      await removeModel(id);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
    await readRows();
  }

  // Progress only means something while the rows are on screen.
  $effect(() => {
    if (!(available && choice === 'mnema')) return;
    let stop: (() => void) | undefined;
    let gone = false;
    void listenLocalModelProgress((p) => {
      progress[p.id] = p.total > 0 ? Math.round((p.done * 100) / p.total) : 0;
    }).then((un) => { if (gone) un(); else stop = un; });
    return () => { gone = true; stop?.(); };
  });

  onMount(() => {
    void (async () => {
      try {
        available = await mnemaAvailable();
        choice = await providerChoice();
      } catch (e) {
        console.error('provider read failed', e);
      }
      await readRows();
    })();
  });

  let error = $state<string | null>(null);

  // What the one count-based question needs: the index's embeddings in every
  // space (`embeddedChunksEverywhere`, the number `Models.svelte` also asks
  // about) while Mnema is being proposed over some other embedding model.
  let pending = $state<{ count: number } | null>(null);
  let retired = $state<RetiredSpace[] | null>(null);
  // The radios are native controls the person has already moved by the time a
  // refusal arrives; re-creating them is how the DOM goes back to `choice`.
  let radioRev = $state(0);

  async function commit(next: ProviderChoice, existing: ExistingVectors) {
    pending = null;
    try {
      const done = await setProviderChoice(next, existing);
      choice = done.choice;
      retired = done.retired.length > 0 ? done.retired : null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      radioRev += 1;
    }
  }

  async function choose(next: ProviderChoice) {
    if (next === choice) return;
    error = null;
    retired = null;
    if (next === 'mnema') {
      // The prediction is the backend's rule, not a second one: with bge-m3
      // already active `adopt_retiring_whatever_blocks` under `Keep` cannot
      // refuse (space.rs:1259-1261), so only another model asks.
      // The local process embeds with bge-m3 alone: from OpenRouter's bge-m3
      // nothing moves, from any other model the vectors cannot come along.
      try {
        const index = (await modelSettings()).index;
        if (index.kind === 'read' && index.embeddedChunksEverywhere > 0
          && index.embeddingModel !== LOCAL_EMBED_MODEL) {
          pending = { count: index.embeddedChunksEverywhere };
          return;
        }
      } catch (e) {
        error = e instanceof Error ? e.message : String(e);
        radioRev += 1;
        return;
      }
    }
    await commit(next, 'keep');
  }

  function cancelPending() {
    pending = null;
    radioRev += 1;
  }

  const retiredLabel = $derived.by(() => {
    void $locale;
    return t('provider_retired', { count: (retired ?? []).reduce((n, r) => n + r.embeddedChunks, 0) });
  });
  const estimateLabel = $derived.by(() => { void $locale; return t('models_embedding_confirm_estimate', { count: pending?.count ?? 0 }); });
  const lossLabel = $derived.by(() => { void $locale; return t('models_embedding_confirm_loss'); });
  const discardLabel = $derived.by(() => { void $locale; return t('models_embedding_discard'); });
  const keepLabel = $derived.by(() => { void $locale; return t('models_embedding_cancel'); });
  const confirmTitle = $derived.by(() => { void $locale; return t('models_embedding_confirm_title'); });
  const label = $derived.by(() => { void $locale; return t('provider_choice_label'); });
  const openRouterLabel = $derived.by(() => { void $locale; return t('provider_openrouter'); });
  const mnemaLabel = $derived.by(() => { void $locale; return t('provider_mnema'); });
  // Gigabytes as the person reads them: one decimal, in their own language.
  const gb = (bytes: number) => new Intl.NumberFormat($locale, { maximumFractionDigits: 1 }).format(bytes / 1e9);
  function reason(id: LocalModelId, state: LocalModelState): string {
    void $locale;
    const f = failure[id];
    if (f?.kind === 'noSpace') return t('provider_no_space', { needed: gb(f.needed), free: gb(f.free) });
    if (f?.kind === 'failed') return f.message;
    return state.kind === 'failed' ? state.message : '';
  }
  const rowName = (id: LocalModelId) => { void $locale; return t(id === 'embed' ? 'provider_model_embed' : 'provider_model_chat'); };
  const downloadingLabel = (id: LocalModelId) => { void $locale; return t('provider_downloading', { name: rowName(id) }); };
  const downloadLabel = $derived.by(() => { void $locale; return t('provider_download'); });
  const readyLabel = $derived.by(() => { void $locale; return t('provider_local_ready'); });
  const retryLabel = $derived.by(() => { void $locale; return t('provider_retry'); });
  const cancelLabel = $derived.by(() => { void $locale; return t('provider_cancel'); });
  const removeLabel = $derived.by(() => { void $locale; return t('provider_remove'); });
  const readyMark = $derived.by(() => { void $locale; return t('provider_row_ready'); });
  const hint = $derived.by(() => { void $locale; return t('provider_mnema_hint'); });
</script>

<!-- Adjacent blocks with no whitespace between them: the section's text must read
     the same as before this component wrapped it when Mnema is unavailable. -->
{#if available}<div class="provider">
{#key radioRev}
  <fieldset>
    <legend>{label}</legend>
    <label><input type="radio" name="provider" checked={choice === 'openRouter'} onchange={() => choose('openRouter')} />{openRouterLabel}</label>
    <label><input type="radio" name="provider" checked={choice === 'mnema'} onchange={() => choose('mnema')} />{mnemaLabel}</label>
  </fieldset>
{/key}
{#if error}<p role="alert">{error}</p>{/if}
{#if retired}<p>{retiredLabel}</p>{/if}
{#if pending}
  <div class="group" role="group" aria-label={confirmTitle}>
    <p>{confirmTitle}</p>
    <p>{estimateLabel}</p>
    <p>{lossLabel}</p>
    <div class="row">
      <button type="button" onclick={() => commit('mnema', 'discard')}>{discardLabel}</button>
      <button type="button" onclick={cancelPending}>{keepLabel}</button>
    </div>
  </div>
{/if}
{#if choice === 'mnema'}
  <p>{hint}</p>
  {#each IDS as id (id)}
    {@const state = states[id]}
    <div role="group" aria-label={rowName(id)} class="row">
      <span>{rowName(id)}</span>
      {#if active[id] || state.kind === 'downloading'}
        <div role="progressbar" aria-label={downloadingLabel(id)} aria-valuemin="0" aria-valuemax="100"
          aria-valuenow={progress[id]}><span class="fill" style:width="{progress[id]}%"></span></div>
        <button type="button" onclick={() => cancel(id)}>{cancelLabel}</button>
      {:else if state.kind === 'ready'}
        <span>{readyMark}</span>
        <button type="button" onclick={() => remove(id)}>{removeLabel}</button>
      {:else if failure[id] || state.kind === 'failed'}
        <span>{reason(id, state)}</span>
        <button type="button" onclick={() => download(id)}>{retryLabel}</button>
      {:else}
        <button type="button" onclick={() => download(id)}>{downloadLabel}</button>
      {/if}
    </div>
  {/each}
  {#if states.embed.kind === 'ready' && states.chat.kind === 'ready'}
    <span role="status" class="dot" aria-label={readyLabel}></span>
  {/if}
{/if}
</div>{/if}{#if !(available && choice === 'mnema')}{@render children?.()}{/if}

<style>
  .provider fieldset { display: flex; gap: 16px; border: 0; padding: 0; margin: 0 0 8px; }
  .provider legend { padding: 0; margin-bottom: 4px; }
  .provider label { display: flex; gap: 6px; align-items: center; }
  [role='progressbar'] { width: 140px; height: 6px; border-radius: 3px; background: var(--surface-3); overflow: hidden; }
  .fill { display: block; height: 100%; background: var(--accent); }
  .dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: var(--ok); }
</style>

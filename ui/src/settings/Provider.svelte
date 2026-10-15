<script lang="ts">
  import { onMount, setContext, type Snippet } from 'svelte';
  import Picker from './Picker.svelte';
  import { locale, t } from '../i18n';
  import {
    mnemaAvailable, localModels, downloadModel, cancelDownload,
    removeModel, listenLocalModelProgress, modelSettings,
    type Configured, type ProviderChoice, type LocalModelId, type LocalModelState, type DownloadError,
  } from '../lib/ipc';

  // The provider picker and the setup card of the provider it shows. Picking
  // writes nothing: it only chooses which card is on screen; the roles' own
  // providers are set on the model tabs.
  let { children }: { children?: Snippet } = $props();

  let available = $state(false);
  // The picker below IS the section's Provider row, so `Models` leaves its own,
  // disabled, OpenRouter-only one out.
  setContext('provider-row-owned', true);
  let choice = $state<ProviderChoice>('openRouter');
  let configured = $state<Configured>({ openRouter: false, mnemaEmbed: false, mnemaChat: false });
  // `Models` owns the key and tells us when it changes, so the dot never lags a save.
  setContext('provider-card', {
    get view() { return choice; },
    keyPresent(present: boolean) { configured.openRouter = present; },
  });

  const LABEL_ID = 'model-provider-label';
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

  // After a change to the local files: the rows, then what they make configured.
  async function reread() {
    await readRows();
    try {
      configured = (await modelSettings()).configured;
    } catch (e) {
      console.error('model_settings failed', e);
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
    await reread();
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
    await reread();
  }

  async function remove(id: LocalModelId) {
    try {
      await removeModel(id);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
    await reread();
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
        const settings = await modelSettings();
        configured = settings.configured;
        choice = available && settings.roles.chat === 'mnema' ? 'mnema' : 'openRouter';
      } catch (e) {
        console.error('provider read failed', e);
      }
      await readRows();
    })();
  });

  let error = $state<string | null>(null);

  const label = $derived.by(() => { void $locale; return t('models_provider_label'); });
  const openRouterLabel = $derived.by(() => { void $locale; return t('provider_openrouter'); });
  const mnemaLabel = $derived.by(() => { void $locale; return t('provider_mnema'); });
  const configuredLabel = $derived.by(() => { void $locale; return t('models_dot_configured'); });
  const notConfiguredLabel = $derived.by(() => { void $locale; return t('models_dot_not_configured'); });
  const options = $derived([
    { id: 'openRouter', label: openRouterLabel },
    ...(available ? [{ id: 'mnema', label: mnemaLabel }] : []),
  ]);
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
  const retryLabel = $derived.by(() => { void $locale; return t('provider_retry'); });
  const cancelLabel = $derived.by(() => { void $locale; return t('provider_cancel'); });
  const removeLabel = $derived.by(() => { void $locale; return t('provider_remove'); });
  const readyMark = $derived.by(() => { void $locale; return t('provider_row_ready'); });
  const hint = $derived.by(() => { void $locale; return t('provider_mnema_hint'); });
</script>

<div class="row">
  <span id={LABEL_ID}>{label}</span>
  <Picker options={options} value={choice} onchange={(id) => (choice = id as ProviderChoice)}
    placeholder={openRouterLabel} disabled={!available} labelledby={LABEL_ID}>
    {#snippet marker(o)}
      {@const on = o.id === 'mnema' ? configured.mnemaEmbed || configured.mnemaChat : configured.openRouter}
      <span class="mdot" data-configured={on ? 'true' : 'false'}><span class="mdot-mark" aria-hidden="true"></span></span>
    {/snippet}
    {#snippet markerAfter(o)}
      {@const on = o.id === 'mnema' ? configured.mnemaEmbed || configured.mnemaChat : configured.openRouter}<span class="sr-only">, {on ? configuredLabel : notConfiguredLabel}</span>
    {/snippet}
  </Picker>
</div>
{#if error}<p role="alert">{error}</p>{/if}
{#if available && choice === 'mnema'}
  <div class="group" data-testid="provider-card">
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
  </div>
{/if}
{@render children?.()}

<style>
  [role='progressbar'] { width: 140px; height: 6px; border-radius: 3px; background: var(--surface-3); overflow: hidden; }
  .fill { display: block; height: 100%; background: var(--accent); }
</style>

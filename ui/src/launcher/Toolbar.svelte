<script lang="ts">
  import { locale, t } from '../i18n';
  import { openSettings, type ProviderChoice, type ProviderStatus } from '../lib/ipc';

  // The header's right cluster: the cloud and the settings button. The panel
  // toggles live on `Shell` now (§2), beside the panel each one owns.
  let { status, provider = 'openRouter', local = false }: {
    status: ProviderStatus | null; // null: not asked yet, or the ask failed
    provider?: ProviderChoice; // which provider `status` speaks for
    local?: boolean; // the chat model runs on this machine: a laptop, not a cloud
  } = $props();

  const cloudTitle = $derived.by(() => {
    void $locale;
    if (!status) return '';
    switch (status.kind) {
      case 'ok': return t(provider === 'mnema' ? 'provider_ok_mnema' : 'provider_ok');
      case 'unreachable':
        return t(provider === 'mnema' ? 'provider_unreachable_mnema' : 'provider_unreachable', { reason: status.reason });
      case 'notConfigured':
        return t(
          status.missing === 'key' ? 'provider_missing_key'
            : status.missing === 'localModels' ? 'provider_missing_local_models'
              : status.missing === 'role' ? 'provider_missing_role'
              : status.missing === 'chatModel' ? 'provider_missing_chat_model'
              : 'provider_missing_model',
        );
    }
  });

  // Only the missing configuration has somewhere to go; the other two states
  // are a report, so the cloud stays a button for the label and does nothing.
  const fixable = $derived(status?.kind === 'notConfigured');
  function onCloud() {
    if (fixable) openSettings('models').catch((e) => console.error('open_settings failed', e));
  }
</script>

<div class="toolbar">
  {#if status}
    <button class="tb cloud" data-testid="provider-cloud" data-status={status.kind}
      aria-disabled={!fixable} aria-label={cloudTitle} title={cloudTitle} onclick={onCloud}>
      <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" data-icon={local ? 'laptop' : 'cloud'}>
        {#if local}<rect x="3" y="3.5" width="10" height="7" rx="1"/><path d="M1.5 12.5h13"/>
        {:else}<path d="M4.5 12.5a3 3 0 0 1-.4-5.97A4 4 0 0 1 11.9 5.6 3.45 3.45 0 0 1 11.5 12.5z"/>{/if}
      </svg>
    </button>
  {/if}
</div>

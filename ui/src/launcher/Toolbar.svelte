<script lang="ts">
  import { locale, t } from '../i18n';
  import { openSettings, type ProviderChoice, type ProviderStatus } from '../lib/ipc';
  import type { Heat } from './state';

  let { heat, left = $bindable(false), right = $bindable(false), pinned = $bindable(false), status, provider = 'openRouter' }: {
    heat: Heat;
    left?: boolean;
    right?: boolean;
    pinned?: boolean;
    status: ProviderStatus | null; // null: not asked yet, or the ask failed
    provider?: ProviderChoice; // which provider `status` speaks for
  } = $props();

  // Cold shows no side panel, so a toggle there would change nothing visible:
  // it is disabled and reads as off whatever `left`/`right` hold.
  const cold = $derived(heat === 'cold');

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
              : 'provider_missing_model',
        );
    }
  });
  const leftLabel = $derived.by(() => { void $locale; return t('toolbar_left'); });
  const rightLabel = $derived.by(() => { void $locale; return t('toolbar_right'); });
  const settingsLabel = $derived.by(() => { void $locale; return t('toolbar_settings'); });
  // U1: `pin` keeps its glyph in the label, as the tests that find it by name expect.
  const pinLabel = $derived.by(() => { void $locale; return `${t('pin')} 📌`; });

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
      <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4.5 12.5a3 3 0 0 1-.4-5.97A4 4 0 0 1 11.9 5.6 3.45 3.45 0 0 1 11.5 12.5z"/></svg>
    </button>
  {/if}
  <button class="tb" data-testid="toggle-left" disabled={cold} aria-pressed={!cold && left}
    aria-label={leftLabel} title={leftLabel} onclick={() => (left = !left)}>
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" aria-hidden="true"><rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M6.5 3v10"/></svg>
  </button>
  <button class="tb" data-testid="toggle-right" disabled={cold} aria-pressed={!cold && right}
    aria-label={rightLabel} title={rightLabel} onclick={() => (right = !right)}>
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" aria-hidden="true"><rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M9.5 3v10"/></svg>
  </button>
  <!-- U1: a stable hook for `i18n/wiring.test.ts`, which reads this button's
       aria-label to prove the locale switch reached the DOM. The accessible name
       cannot be the selector when it is the thing under test. -->
  <button class="tb pin" data-testid="pin" aria-pressed={pinned}
    aria-label={pinLabel} title={pinLabel} onclick={() => (pinned = !pinned)}>
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9.5 2.5l4 4-2 .8-2.2 2.2.4 2.3-1 1-2.4-2.4-3.4 3.4M5.3 7.2l1-1 2.3.4L10.8 4.4z"/></svg>
  </button>
  <button class="tb" data-testid="settings" aria-label={settingsLabel} title={settingsLabel}
    onclick={() => openSettings().catch((e) => console.error('open_settings failed', e))}>
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" aria-hidden="true"><circle cx="8" cy="8" r="2.2"/><path d="M8 1.8v2M8 12.2v2M1.8 8h2M12.2 8h2M3.6 3.6l1.4 1.4M11 11l1.4 1.4M3.6 12.4L5 11M11 5l1.4-1.4"/></svg>
  </button>
</div>

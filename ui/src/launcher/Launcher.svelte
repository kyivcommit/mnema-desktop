<script lang="ts">
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { locale, t } from '../i18n';

  let query = $state('');
  let pinned = $state(false);

  const appWindow = getCurrentWebviewWindow();

  // `void $locale` establishes the reactive dependency so this recomputes on a locale change;
  // `t` itself stays a plain function (it reads the current locale internally). The 📌 emoji
  // stays out of the catalog — it is not translatable content.
  const pinLabel = $derived.by(() => { void $locale; return `${t('pin')} 📌`; });

  // Hide, never close: a hidden webview keeps this component's state, so `query`
  // (and, from PR 6, the results) survive dismissal (§7.3).
  function hide() {
    appWindow.hide();
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') hide();
  }

  function onBlur() {
    if (!pinned) hide();
  }
</script>

<svelte:window onkeydown={onKeydown} onblur={onBlur} />

<main>
  <input type="text" bind:value={query} />
  <button
    class="pin"
    class:active={pinned}
    aria-pressed={pinned}
    aria-label={pinLabel}
    onclick={() => (pinned = !pinned)}>📌</button>
</main>

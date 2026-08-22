<script lang="ts">
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

  let query = $state('');
  let pinned = $state(false);

  const appWindow = getCurrentWebviewWindow();

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
    aria-label="Пін 📌"
    onclick={() => (pinned = !pinned)}>📌</button>
</main>

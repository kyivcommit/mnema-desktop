<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { locale, t } from '../i18n';
  import { ask, modelSettings } from '../lib/ipc';
  import { checkQuery, stateFromAnswer, providerReady, DRAG_GRAB_WINDOW_MS, type LauncherState } from './state';
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
      // §7: the query echoes as a chat bubble. The bubble itself is drawn by
      // `Answer` inside the centre card (Task 8b) — the launcher used to draw a
      // second one of its own here, and in state B both were on screen at once.
      echo = check.query;
    } catch (e) {
      console.error('ask failed', e); // query stays in the line for a retry
      launcherState = { kind: 'error', reason: 'askFailed' };
    }
  }

  // Hide, never close: a hidden webview keeps state, so `query` and results
  // survive dismissal (§6, `…interface-design.md:186` — §7.3 is the dismissal
  // gestures, and does not speak about state).
  function hide() { appWindow.hide(); }
  function onKeydown(event: KeyboardEvent) { if (event.key === 'Escape') hide(); }

  // D155: on some window managers (mutter on X11) the drag of a frameless
  // window is a pointer+keyboard grab that takes focus for the whole move and
  // hands it back at the end. The webview sees that as `blur` a few
  // milliseconds after the press on the handle. A blur that close to a press
  // on the drag handle is the drag, not a dismissal. Measured on the Ubuntu
  // stand: press-to-blur landed at 392 / 516 / 504 ms, so the window stays
  // armed for a full second — a release means it was a click, not a drag,
  // and disarms it immediately; where the drag keeps focus (macOS/Windows)
  // the release reaches the webview the same way and a real blur after a
  // mere click must still hide.
  let handlePressedAt = -Infinity;
  function onPointerDown(event: PointerEvent) {
    // No `event.button` check: this project's test environment (jsdom has no
    // real `PointerEvent` constructor — `@testing-library/dom` falls back to
    // a plain `Event`, which drops `button`) cannot exercise it, and
    // `JobStrip.svelte`'s own document-level pointerdown listener (the only
    // other one in this codebase) does not check it either. A right-button
    // press on the handle can arm the window for nothing, but nothing acts on
    // it unless a blur follows within the window, and it self-clears either
    // way (a release, or expiry).
    const target = event.target as Element | null;
    const onHandle = !!target?.closest('.searchbar')
      && !target.closest('button, input, label, a, select, textarea, [role="button"]');
    if (onHandle) handlePressedAt = Date.now();
  }
  // A release means it was a click, not a drag: the next blur is a dismissal
  // again. During a window-manager move grab no release reaches the webview.
  function onPointerUp() { handlePressedAt = -Infinity; }
  function onBlur() {
    if (pinned) return;
    if (Date.now() - handlePressedAt < DRAG_GRAB_WINDOW_MS) return;
    hide();
  }
</script>

<svelte:window onkeydown={onKeydown} onpointerdown={onPointerDown} onpointerup={onPointerUp} onblur={onBlur} />

<main class="panels">
  <!-- D155: the search panel is the drag handle. "deep" drags from any
       click inside it except the input, the pin and the Arms labels — Tauri's
       own drag script skips clickable tags. The other panels select text. -->
  <div class="searchbar" data-tauri-drag-region="deep">
    <div class="sb-row">
      <SearchLine bind:query state={launcherState} onSubmit={runSearch} />
      <!-- U1: a stable hook for `i18n/wiring.test.ts`, which reads this button's
           aria-label to prove the locale switch reached the DOM. It used to find the
           button as "the first element with any aria-label", which was true only
           while no labelled card rendered — and the cards are now labelled in five
           of six states. The accessible name cannot be the selector when it is the
           thing under test. -->
      <button
        class="pin"
        data-testid="pin"
        class:active={pinned}
        aria-pressed={pinned}
        aria-label={pinLabel}
        onclick={() => (pinned = !pinned)}>📌</button>
    </div>
    <Arms bind:textOn bind:contentOn {provider} />
  </div>
  <Cards state={launcherState} query={echo} />
</main>

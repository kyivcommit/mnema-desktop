<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { locale, t } from '../i18n';
  import { ask, listenLauncherCold, modelSettings, openSettings, setLauncherLayout } from '../lib/ipc';
  import { checkQuery, heatAfter, stateFromAnswer, providerReady, DRAG_GRAB_WINDOW_MS, type Heat, type LauncherState } from './state';
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
  // Cold: the search column alone. The first answer with something to show
  // makes it hot and opens both side panels; after that the person's choice
  // stands across questions. `launcher-cold` is the only way back.
  let heat = $state<Heat>('cold');
  let left = $state(false);
  let right = $state(false);

  // What is on screen, not what was asked for: cold shows no side panel
  // whatever the switches say, and the switches themselves are left alone.
  const showLeft = $derived(heat === 'hot' && left);
  const showRight = $derived(heat === 'hot' && right);
  const cols = $derived(`${showLeft ? 'l' : ''}s${showRight ? 'r' : ''}`);

  // Derived booleans, so this runs once per change of what is shown and not
  // once per write to `left`, `right` or `heat`.
  $effect(() => {
    setLauncherLayout(showLeft, showRight).catch((e) => console.error('set_launcher_layout failed', e));
  });

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

    // The window was hidden past the threshold: forget the answer, keep the
    // text in the line (the person may still want to ask it).
    let unlisten: (() => void) | null = null;
    let gone = false;
    listenLauncherCold(() => {
      coldGen += 1;
      heat = 'cold';
      left = right = false;
      launcherState = { kind: 'idle' };
      echo = '';
    })
      .then((u) => { if (gone) u(); else unlisten = u; })
      .catch((e) => console.error('listen launcher-cold failed', e));
    return () => { gone = true; unlisten?.(); };
  });

  // The owner validates and calls ask — the whole machine goes through
  // state.ts. A rejected ask becomes a visible error, never a silent reset:
  // an eaten error is easy to miss.
  // Bumped by `launcher-cold`: an ask that was in flight when the launcher went
  // cold belongs to an answer the person has been told is forgotten.
  let coldGen = 0;

  async function runSearch(raw: string) {
    if (launcherState.kind === 'inFlight') return; // one ask at a time
    echo = '';
    const check = checkQuery(raw);
    if (!check.ok) { launcherState = { kind: 'error', reason: check.reason }; return; }
    launcherState = { kind: 'inFlight', query: check.query };
    const gen = coldGen;
    try {
      const answer = await ask(check.query);
      if (gen !== coldGen) return; // went cold meanwhile: drop the answer
      launcherState = stateFromAnswer(check.query, answer);
      // Only the cold-to-hot step opens the panels; a hot launcher keeps
      // whichever the person switched off.
      const warmed = heat === 'cold' && heatAfter(heat, launcherState) === 'hot';
      if (warmed) { heat = 'hot'; left = right = true; }
      // §7: line clears on ready — but only if it still holds the submitted
      // query. A draft typed while the ask was in flight is kept, not wiped
      // (Codex #3).
      if (query === raw) query = '';
      // §7: the query echoes as a chat bubble. The bubble itself is drawn by
      // `Answer` inside the centre card (Task 8b) — the launcher used to draw a
      // second one of its own here, and in state B both were on screen at once.
      echo = check.query;
    } catch (e) {
      if (gen !== coldGen) return;
      console.error('ask failed', e); // query stays in the line for a retry
      launcherState = { kind: 'error', reason: 'askFailed' };
    }
  }

  // Hide, never close: a hidden webview keeps state, so `query` and results
  // survive dismissal (§6, `…interface-design.md:186` — §7.3 is the dismissal
  // gestures, and does not speak about state).
  function hide() { appWindow.hide(); }
  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') hide();
    // ⌘, on macOS, Ctrl+, elsewhere. The launcher carries no menu bar (the app
    // is an Accessory while only it is up), so no menu item can own the key.
    if (event.key === ',' && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      openSettings().catch((e) => console.error('open_settings failed', e));
    }
  }

  // D155: on some window managers (mutter on X11) the drag of a frameless
  // window is a pointer+keyboard grab that takes focus for the whole move and
  // hands it back at the end. The webview sees that as `blur` a few
  // milliseconds after the press on the handle. A blur that close to a press
  // on the drag handle is the drag, not a dismissal. Measured on the Ubuntu
  // stand: press-to-blur landed at 392 / 516 / 504 ms, so the window stays
  // armed for a full second — a release means it was a click, not a drag,
  // and disarms it immediately. On macOS the release is expected to reach the
  // webview after the drag; on Windows the modal move loop may consume it — either
  // way no blur arrives during the move, and a stuck arm simply expires
  // after one second (the Windows live check confirms which). A real blur
  // after a mere click must still hide.
  let handlePressedAt = -Infinity;
  function onPointerDown(event: PointerEvent) {
    // Tauri's drag script starts a move only for the primary button, so a
    // right- or middle-button press is never a drag. `!event.button`, not
    // `=== 0`: jsdom's `fireEvent.pointerDown` leaves it `undefined`.
    const target = event.target as Element | null;
    const onHandle = !event.button && !!target?.closest('.searchbar')
      && !target.closest('button, input, label, a, select, textarea, [role="button"]');
    // Any press that is not a primary press on the handle disarms on its
    // own: on X11 the drag ends with no release reaching the webview, so an
    // arming can outlive its drag.
    handlePressedAt = onHandle ? Date.now() : -Infinity;
  }
  // A release means it was a click, not a drag: the next blur is a dismissal
  // again. Once a window-manager move grab is up, no release reaches the
  // webview until it ends.
  function onPointerUp() { handlePressedAt = -Infinity; }
  function onBlur() {
    if (pinned) return;
    if (Date.now() - handlePressedAt < DRAG_GRAB_WINDOW_MS) return;
    hide();
  }
</script>

<svelte:window onkeydown={onKeydown} onpointerdown={onPointerDown} onpointerup={onPointerUp} onblur={onBlur} />

<main class="panels" data-cols={cols}>
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
  <Cards state={launcherState} query={echo} {left} {right} {heat} />
</main>

<script lang="ts">
  import { locale, t } from '../i18n';
  import { refusalText } from '../i18n/refusal';
  import { MAX_ASK_QUERY, type LauncherState } from './state';

  // §5: the field that asks. Enter sends, Shift+Enter is a newline, and the
  // guard and refusal messages have their one place here.
  // The prop is `state`; the LOCAL binding is renamed. Svelte reads `$state` as
  // a subscription to a local called `state` when one exists, so a component
  // that takes a `state` prop cannot declare reactive state of its own until
  // this binding is out of the way (`store_rune_conflict`).
  let { value = $bindable(''), state: launcherState, scopeOff = false, onSubmit }: {
    // Every folder is unticked: nothing can be searched, so nothing is sent.
    value?: string;
    state: LauncherState;
    scopeOff?: boolean;
    onSubmit: (raw: string) => void;
  } = $props();

  const placeholder = $derived.by(() => { void $locale; return t('search_placeholder'); });
  const sendLabel = $derived.by(() => { void $locale; return t('composer_send'); });

  // The button and Enter agree on when a question may leave; `runSearch` still
  // judges the text itself (a blank Enter does nothing there too).
  const canSend = $derived(value.trim() !== '' && launcherState.kind !== 'inFlight' && !scopeOff);

  // Every message is driven by the machine's state, not a local guard — so a
  // rejected `ask` (askFailed) is as visible as a too-long one.
  const errorText = $derived.by(() => {
    void $locale;
    if (launcherState.kind !== 'error') return '';
    // An open that failed was clicked in the sidebar, and it is said there
    // (`Recents.svelte`), not under a composer the person did not touch.
    if (launcherState.reason === 'historyUnreadable' || launcherState.reason === 'historyFailed') return '';
    if (launcherState.reason === 'tooLong') {
      // A too-long message belongs to the line that was too long: stale once emptied.
      return value.trim() === '' ? '' : t('query_too_long', { limit: MAX_ASK_QUERY });
    }
    return t('query_failed'); // askFailed
  });
  const refusalMessage = $derived.by(() => {
    void $locale;
    if (launcherState.kind !== 'refused') return '';
    // Stale once any folder is ticked again; the live line covers the off case.
    if (launcherState.reason.kind === 'noFoldersSearched' && !scopeOff) return '';
    return refusalText(launcherState.reason.kind);
  });

  const noFolders = $derived.by(() => { void $locale; return t('refusal_no_folders_searched'); });

  let box = $state<HTMLTextAreaElement | null>(null);

  // The box grows with its text up to the stylesheet's five-row ceiling, then
  // scrolls. A box that cannot be measured (jsdom, or one still hidden) keeps
  // its single row instead of collapsing to nothing.
  // shortcut: a width change re-wraps the text without refitting until the next
  // keystroke, add a ResizeObserver if the splitters make that visible.
  $effect(() => {
    void value;
    if (!box || box.scrollHeight === 0) return;
    box.style.height = 'auto';
    box.style.height = `${box.scrollHeight}px`;
  });

  function onKeydown(event: KeyboardEvent) {
    // A held Enter auto-repeats keydown; only the press submits.
    if (event.key !== 'Enter' || event.shiftKey || event.repeat) return;
    // Enter never leaves a newline in the box, whether or not it sends.
    event.preventDefault();
    if (canSend) onSubmit(value);
  }

  // §7.3: the window is hidden, not closed. Bringing it back must not pull the
  // cursor out of the tree, the conversation list, a splitter or the file the
  // person was reading — only a window with no focus inside it gets the field.
  function onWindowFocus() {
    const active = document.activeElement;
    if (active === null || active === document.body) box?.focus();
  }
</script>

<!-- Every show of the window ends in `set_focus`, and a window that had no
     focus inside it gets the cursor in the field, so the person can type
     without a click first. -->
<svelte:window onfocus={onWindowFocus} />

<div class="composer">
  <div class="composer-row">
    <textarea
      bind:this={box}
      bind:value
      rows="1"
      {placeholder}
      aria-busy={launcherState.kind === 'inFlight' ? 'true' : undefined}
      onkeydown={onKeydown}></textarea>
    <button type="button" class="composer-send" aria-label={sendLabel} disabled={!canSend} onclick={() => onSubmit(value)}>↵</button>
  </div>
  {#if errorText}
    <p class="guard" role="alert">{errorText}</p>
  {:else if scopeOff}
    <p class="guard" role="status">{noFolders}</p>
  {:else if refusalMessage}
    <p class="refusal" role="status">{refusalMessage}</p>
  {/if}
</div>

<script lang="ts">
  import { locale, t } from '../i18n';
  import Splitter from './Splitter.svelte';
  import {
    MIN, clampLayout, defaultWidth, isOverlay, loadLayout, maxWidth, saveLayout, type PanelState,
  } from './layout';
  import type { Snippet } from 'svelte';

  // §2: one row of three areas — [recents L][transcript 1fr][right W] — and the
  // right area is [tree T][file W − T]. The panels are toggles and the widths
  // are the person's; nothing here is read from Rust, and a hidden panel hands
  // its width to its neighbour instead of shrinking the window.
  //
  // `panels` is bindable because a later answer opens the right area from
  // outside (Task 7), and because the layout is remembered across runs. It
  // holds the person's widths, never a layout squeezed to fit the window.
  let {
    panels = $bindable(loadLayout()),
    fileOpen = false,
    tools,
    recents,
    transcript,
    tree,
    file,
  }: {
    panels?: PanelState;
    // No file open means no file stripe at all: the tree takes the whole W and
    // the tree|file splitter is not rendered (§2).
    fileOpen?: boolean;
    // The header's right cluster: the cloud and settings stay with the launcher.
    tools?: Snippet;
    recents?: Snippet;
    transcript?: Snippet;
    tree?: Snippet;
    file?: Snippet;
  } = $props();

  // The window can be resized at any moment (minimum 900×600). `view` is what is
  // drawn inside the room available; `overlay` is decided by the widths the
  // person chose, so resizing cannot silently rewrite them.
  let windowWidth = $state(window.innerWidth);
  const view = $derived(clampLayout(panels, windowWidth));
  const overlay = $derived(isOverlay(panels, windowWidth));

  // The overlay keeps the transcript's 520 by not being a column at all: the
  // third track collapses to 0 and the right area paints over the transcript
  // at the person's W — capped by `maxWidth` to the room beside the recents
  // pane, whose splitter is the only way back from an overlay grown too wide.
  const trackW = $derived(view.right && !overlay ? view.W : 0);
  // With no file open the tree IS the area's content, so it stays on screen
  // whatever its own toggle says: a hidden tree beside no file would leave a
  // 480 px panel holding one button. The toggle becomes meaningful the moment a
  // file gives it something to hand the width to.
  const treeVisible = $derived(view.tree || !fileOpen);
  const treeW = $derived(treeVisible ? (fileOpen ? view.T : view.W) : 0);
  // Written as inline tracks rather than as custom properties: the stylesheet
  // token guard (`styles/tokens.test.ts`) allows `var(--…)` only for the
  // palette, and these are one window's numbers, not tokens.
  const shellStyle = $derived(
    `grid-template-columns: ${view.left ? view.L : 0}px minmax(0, 1fr) ${trackW}px`,
  );
  const rightStyle = $derived(`grid-template-columns: ${treeW}px minmax(0, 1fr)`);

  // A drag writes the pane it moved and nothing else. Clamping the neighbour is
  // the layout's business; storing the squeezed copy would lose the person's
  // width for good the first time they drag in a narrow window.
  function write(pane: 'L' | 'W' | 'T', value: number) {
    const widest = maxWidth(panels, pane, windowWidth);
    panels = { ...panels, [pane]: Math.max(MIN[pane], Math.min(value, widest)) };
  }
  function toggle(pane: 'left' | 'right' | 'tree') {
    panels = { ...panels, [pane]: !view[pane] };
  }

  function onKeydown(event: KeyboardEvent) {
    // Esc closes the overlay and nothing else: in an ordinary window it is not
    // a hide key any more (§1).
    if (event.key === 'Escape' && overlay) panels = { ...panels, right: false };
  }

  $effect(() => saveLayout(panels));

  const labels = $derived.by(() => {
    void $locale;
    return { recents: t('toggle_recents'), files: t('toggle_files'), tree: t('toggle_tree') };
  });
</script>

<svelte:window onresize={() => (windowWidth = window.innerWidth)} onkeydown={onKeydown} />

<main class="shell" data-testid="shell" data-overlay={overlay} data-left={view.left} style={shellStyle}>
  {#if view.left}
    <aside class="recents" data-testid="col-recents" aria-label={labels.recents}>
      {@render recents?.()}
      <Splitter
        label={labels.recents}
        value={view.L}
        min={MIN.L}
        max={maxWidth(panels, 'L', windowWidth)}
        onResize={(d) => write('L', view.L + d)}
        onReset={() => write('L', defaultWidth('L'))} />
    </aside>
  {/if}

  <section class="centre" data-testid="col-transcript">
    <!-- The drag handle, and the place the recents toggle stands beside the
         traffic lights. macOS draws those over the content
         (`titleBarStyle: Overlay`): when the sidebar is shut this row is under
         them and keeps clear of them (`main.shell[data-left="false"]`). -->
    <header class="head" data-tauri-drag-region="deep">
      <button type="button" class="tb" data-testid="toggle-left"
        aria-pressed={view.left} aria-label={labels.recents} title={labels.recents}
        onclick={() => toggle('left')}>
        <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" aria-hidden="true"><rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M6.5 3v10"/></svg>
      </button>
      {@render tools?.()}
      <button type="button" class="tb" data-testid="toggle-right"
        aria-pressed={view.right} aria-label={labels.files} title={labels.files}
        onclick={() => toggle('right')}>
        <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" aria-hidden="true"><rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M9.5 3v10"/></svg>
      </button>
    </header>
    <div class="centre-body">
      {@render transcript?.()}
    </div>
  </section>

  {#if overlay}
    <!-- Clicking anywhere beside the overlay closes it; Esc is the keyboard
         path and the reason this is not in the tab order. -->
    <button type="button" class="scrim" data-testid="overlay-scrim" tabindex="-1"
      aria-hidden="true" onclick={() => (panels = { ...panels, right: false })}></button>
  {/if}

  <!-- Hidden, never unmounted: `Tree` refetches on mount and drops the folders
       the person opened, and the overlay below 1060 is closed constantly.
       The track goes to 0 and `[hidden]` takes it out of the layout. -->
  <aside class="right" class:overlay hidden={!view.right}
    style:width={overlay ? `${view.W}px` : undefined}
    data-testid="col-right" aria-label={labels.files}>
    {#if !overlay}
      <Splitter
        label={labels.files}
        value={view.W}
        min={MIN.W}
        max={maxWidth(panels, 'W', windowWidth)}
        onResize={(d) => write('W', view.W - d)}
        onReset={() => write('W', defaultWidth('W'))} />
    {/if}
    <div class="right-head">
      <button type="button" class="tb" data-testid="toggle-tree"
        aria-pressed={treeVisible} aria-label={labels.tree} title={labels.tree}
        disabled={!fileOpen}
        onclick={() => toggle('tree')}>
        <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 4h4M3 8h7M3 12h5"/></svg>
      </button>
    </div>
    <div class="right-body" style={rightStyle}>
      <div class="tree-pane" data-testid="col-tree" hidden={!treeVisible}>
        <div class="pane-body">{@render tree?.()}</div>
        {#if fileOpen}
          <Splitter
            label={labels.tree}
            value={view.T}
            min={MIN.T}
            max={maxWidth(panels, 'T', windowWidth)}
            onResize={(d) => write('T', view.T + d)}
            onReset={() => write('T', defaultWidth('T'))} />
        {/if}
      </div>
      {#if fileOpen}
        <div class="file-pane" data-testid="col-file"><div class="pane-body">{@render file?.()}</div></div>
      {/if}
    </div>
  </aside>
</main>

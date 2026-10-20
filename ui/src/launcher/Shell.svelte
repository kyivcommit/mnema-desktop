<script lang="ts">
  import { locale, t } from '../i18n';
  import Splitter from './Splitter.svelte';
  import {
    MIN, clampLayout, isOverlay, loadLayout, resetPane, saveLayout, type PanelState,
  } from './layout';
  import type { Snippet } from 'svelte';

  // §2: one row of three areas — [recents L][transcript 1fr][right W] — and the
  // right area is [tree T][file W − T]. The panels are toggles and the widths
  // are the person's; nothing here is read from Rust, and a hidden panel hands
  // its width to its neighbour instead of shrinking the window.
  //
  // `panels` is bindable because a later answer opens the right area from
  // outside (Task 7), and because the layout is remembered across runs.
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

  // The window can be resized at any moment (minimum 900×600), so a saved width
  // is read through the clamp on every render — never written back on a resize.
  let windowWidth = $state(window.innerWidth);
  const view = $derived(clampLayout(panels, windowWidth));
  const overlay = $derived(isOverlay(view, windowWidth));

  // The overlay keeps the transcript's 520 by not being a column at all: the
  // third track collapses to 0 and the right area paints over the transcript
  // at the same width W.
  const trackW = $derived(view.right && !overlay ? view.W : 0);
  const treeW = $derived(view.tree ? (fileOpen ? view.T : view.W) : 0);
  // Written as inline tracks rather than as custom properties: the stylesheet
  // token guard (`styles/tokens.test.ts`) allows `var(--…)` only for the
  // palette, and these are one window's numbers, not tokens.
  const shellStyle = $derived(
    `grid-template-columns: ${view.left ? view.L : 0}px minmax(0, 1fr) ${trackW}px`,
  );
  const rightStyle = $derived(`grid-template-columns: ${treeW}px minmax(0, 1fr)`);

  function resize(pane: 'L' | 'W' | 'T', value: number) {
    panels = clampLayout({ ...view, [pane]: value }, windowWidth, pane === 'T' ? undefined : pane);
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

<main class="shell" data-testid="shell" data-overlay={overlay} style={shellStyle}>
  {#if view.left}
    <aside class="recents" data-testid="col-recents" aria-label={labels.recents}>
      {@render recents?.()}
      <Splitter
        label={labels.recents}
        value={view.L}
        min={MIN.L}
        onResize={(d) => resize('L', view.L + d)}
        onReset={() => (panels = resetPane(view, 'L', windowWidth))} />
    </aside>
  {/if}

  <section class="centre" data-testid="col-transcript">
    <!-- The drag handle, and the place the recents toggle stands beside the
         traffic lights. macOS draws those over the content
         (`titleBarStyle: Overlay`), so the header is theirs on the left. -->
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

  {#if view.right}
    {#if overlay}
      <!-- Clicking anywhere beside the overlay closes it; Esc is the keyboard
           path and the reason this is not in the tab order. -->
      <button type="button" class="scrim" data-testid="overlay-scrim" tabindex="-1"
        aria-hidden="true" onclick={() => (panels = { ...panels, right: false })}></button>
    {/if}
    <aside class="right" class:overlay style:width={overlay ? `${view.W}px` : undefined}
      data-testid="col-right" aria-label={labels.files}>
      {#if !overlay}
        <Splitter
          label={labels.files}
          value={view.W}
          min={MIN.W}
          onResize={(d) => resize('W', view.W - d)}
          onReset={() => (panels = resetPane(view, 'W', windowWidth))} />
      {/if}
      <div class="right-head">
        <button type="button" class="tb" data-testid="toggle-tree"
          aria-pressed={view.tree} aria-label={labels.tree} title={labels.tree}
          onclick={() => toggle('tree')}>
          <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 4h4M3 8h7M3 12h5"/></svg>
        </button>
      </div>
      <div class="right-body" style={rightStyle}>
        {#if view.tree}
          <div class="tree-pane" data-testid="col-tree">
            {@render tree?.()}
            {#if fileOpen}
              <Splitter
                label={labels.tree}
                value={view.T}
                min={MIN.T}
                onResize={(d) => resize('T', view.T + d)}
                onReset={() => (panels = resetPane(view, 'T', windowWidth))} />
            {/if}
          </div>
        {/if}
        {#if fileOpen}
          <div class="file-pane" data-testid="col-file">{@render file?.()}</div>
        {/if}
      </div>
    </aside>
  {/if}
</main>

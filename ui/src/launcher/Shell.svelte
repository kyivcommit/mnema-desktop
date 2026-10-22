<script lang="ts">
  import { locale, t } from '../i18n';
  import Splitter from './Splitter.svelte';
  import {
    MIN, defaultWidth, layoutFor, loadLayout, maxWidth, saveLayout, type PanelState,
  } from './layout';
  import type { Snippet } from 'svelte';

  // §2: one row of three areas — [recents L][transcript 1fr][right W] — and the
  // right area is [tree T][file W − T]. The panels are toggles and the widths
  // are the person's; nothing here is read from Rust, and a hidden panel hands
  // its width to its neighbour instead of shrinking the window.
  //
  // `panels` holds the person's choices, never a layout squeezed to fit the
  // window: `layoutFor` decides what is drawn at the current width, so a pane
  // that yields to a narrow window comes back when the window grows without its
  // toggle ever moving.
  let {
    panels = $bindable(loadLayout()),
    fileOpen = false,
    fileTitle = null,
    tools,
    recents,
    transcript,
    tree,
    file,
  }: {
    panels?: PanelState;
    // No file open means the file stripe stays empty: the tree keeps its own
    // T beside the reserved room, as if the file were there (owner, 2026-10-20).
    fileOpen?: boolean;
    // The open file's path, from its root: it stands in the right head, which
    // never scrolls, so it stays in view while the file does.
    fileTitle?: string | null;
    // The header's right cluster: the cloud and settings stay with the launcher.
    tools?: Snippet;
    recents?: Snippet;
    transcript?: Snippet;
    tree?: Snippet;
    file?: Snippet;
  } = $props();

  // The window can be resized at any moment (minimum 900×600). `view` is what is
  // drawn inside the room available; the toggles and widths stay the person's,
  // so resizing cannot silently rewrite them.
  let windowWidth = $state(window.innerWidth);
  // The conversations pane asked for at a width where it cannot sit beside the
  // panel: the file panel yields, and the open file comes back with the room.
  // Not stored — it is this window's answer, not a preference.
  let pinned = $state(false);
  const view = $derived(layoutFor(panels, windowWidth, pinned));
  // Auto-hidden is not the same as toggled off: the toggle keeps showing the
  // person's choice, so a press can bring the pane back instead of turning the
  // choice away.
  const autoLeft = $derived(panels.left && view.left === 0);
  const autoRight = $derived(panels.right && view.right === 0);
  // §2: the composer is never covered, so no pane is ever positioned over the
  // transcript; the tracks here are the whole layout.
  const treeW = $derived(panels.tree ? view.tree : 0);
  // Written as inline tracks rather than as custom properties: the stylesheet
  // token guard (`styles/tokens.test.ts`) allows `var(--…)` only for the
  // palette, and these are one window's numbers, not tokens.
  const shellStyle = $derived(`grid-template-columns: ${view.left}px minmax(0, 1fr) ${view.right}px`);
  const rightStyle = $derived(`grid-template-columns: ${treeW}px minmax(0, 1fr)`);

  // A drag writes the pane it moved and nothing else. Clamping the neighbour is
  // the layout's business; storing the squeezed copy would lose the person's
  // width for good the first time they drag in a narrow window.
  function write(pane: 'L' | 'W' | 'T', value: number) {
    const widest = maxWidth(panels, pane, windowWidth, pinned);
    panels = { ...panels, [pane]: Math.max(MIN[pane], Math.min(value, widest)) };
  }
  function toggleLeft() {
    // A pane the window took, not the person: the press brings it back, and the
    // file panel yields for it (§2: transcript > file panel > conversations).
    if (autoLeft) { pinned = true; return; }
    const next = { ...panels, left: !panels.left };
    // Decided from what THIS press produces, not from the state before it. A
    // press that turns the preference on at a width where the pane still has no
    // `share` would otherwise store a choice `layoutFor` does not draw: the
    // toggle would read pressed with no pane beside it, and a second press would
    // clear the choice again. Pin it, so one press always shows what it asked for.
    pinned = next.left && layoutFor(next, windowWidth, false).left === 0;
    panels = next;
  }
  function toggleRight() {
    // Same reading from the other side, where the file panel has priority: the
    // press takes the room back from a pinned conversations pane.
    if (autoRight) { pinned = false; return; }
    panels = { ...panels, right: !panels.right };
    pinned = false;
  }
  function toggleTree() {
    panels = { ...panels, tree: !panels.tree };
  }

  $effect(() => saveLayout(panels));

  const labels = $derived.by(() => {
    void $locale;
    return {
      recents: t('toggle_recents'),
      files: t('toggle_files'),
      tree: t('toggle_tree'),
    };
  });
</script>

<svelte:window onresize={() => (windowWidth = window.innerWidth)} />

<main class="shell" data-testid="shell" data-left={view.left > 0} style={shellStyle}>
  {#if view.left}
    <aside class="recents" data-testid="col-recents" aria-label={labels.recents}>
      {@render recents?.()}
      <Splitter
        label={labels.recents}
        value={view.left}
        min={MIN.L}
        max={maxWidth(panels, 'L', windowWidth, pinned)}
        onResize={(d) => write('L', view.left + d)}
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
        aria-pressed={panels.left} aria-label={labels.recents} title={labels.recents}
        onclick={toggleLeft}>
        <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" aria-hidden="true"><rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M6.5 3v10"/></svg>
      </button>
      {@render tools?.()}
      <button type="button" class="tb" data-testid="toggle-right"
        aria-pressed={panels.right} aria-label={labels.files} title={labels.files}
        onclick={toggleRight}>
        <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" aria-hidden="true"><rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M9.5 3v10"/></svg>
      </button>
    </header>
    <div class="centre-body">
      {@render transcript?.()}
    </div>
  </section>

  <!-- Hidden, never unmounted: `Tree` refetches on mount and drops the folders
       the person opened, and the file pane holds the open document. The track
       goes to 0 and `[hidden]` takes the area out of the layout; the open file
       stays in state and is back with the room. -->
  <aside class="right" hidden={view.right === 0} data-testid="col-right" aria-label={labels.files}>
    <Splitter
      label={labels.files}
      value={view.right}
      min={MIN.W}
      max={maxWidth(panels, 'W', windowWidth, pinned)}
      onResize={(d) => write('W', view.right - d)}
      onReset={() => write('W', defaultWidth('W'))} />
    <div class="right-head">
      <button type="button" class="tb" data-testid="toggle-tree"
        aria-pressed={panels.tree} aria-label={labels.tree} title={labels.tree}
        onclick={toggleTree}>
        <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 4h4M3 8h7M3 12h5"/></svg>
      </button>
      {#if fileTitle !== null}
        <!-- Cut at the START when it does not fit (rtl + ellipsis), so the file
             name stays visible; the inner span keeps the text itself left to right. -->
        <span class="file-path" data-testid="file-header" title={fileTitle}><span>{fileTitle}</span></span>
      {/if}
    </div>
    <div class="right-body" style={rightStyle}>
      <div class="tree-pane" data-testid="col-tree" hidden={!panels.tree}>
        <div class="pane-body">{@render tree?.()}</div>
        <Splitter
          label={labels.tree}
          value={view.tree}
          min={MIN.T}
          max={maxWidth(panels, 'T', windowWidth, pinned)}
          onResize={(d) => write('T', view.tree + d)}
          onReset={() => write('T', defaultWidth('T'))} />
      </div>
      {#if fileOpen}
        <div class="file-pane" data-testid="col-file"><div class="pane-body">{@render file?.()}</div></div>
      {/if}
    </div>
  </aside>
</main>

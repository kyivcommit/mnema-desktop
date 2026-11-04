<script module lang="ts">
  // The width asked of the renderer: the pane's width in device pixels, rounded
  // UP to a multiple of 200 and capped at the backend's own ceiling. Rounding
  // keeps a drag of the splitter from asking for a new picture on every pixel.
  // An unmeasured pane (width 0) asks for the smallest step, not for nothing.
  export function widthFor(clientWidth: number, ratio: number): number {
    return Math.min(3000, Math.max(200, Math.ceil((clientWidth * ratio) / 200) * 200));
  }
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import { renderPdfPage } from '../lib/ipc';
  import type { DocumentLocation, DocumentView, PdfPageView } from '../lib/ipc';

  // One page of a PDF drawn as the file has it, with the cited passage boxed
  // (the page mode of the file pane; the text mode is `FilePane`'s own card).
  // `view` is the index's read of the document: it says which page the citation
  // is on and which spans fall on a page — the picture itself comes from
  // `render_pdf_page`, which finds the file through the index.
  let { location, view, onShowText }: {
    location: DocumentLocation;
    view: Extract<DocumentView, { kind: 'file' }>;
    onShowText: () => void;
  } = $props();

  // The person's own turn of the page; until then the page is the cited one.
  // A new open or a re-read-with-reveal remounts this component (FilePane clears
  // `view` first), so a stale choice never outlives its document.
  let chosen = $state<number | null>(null);
  const focusPage = $derived(
    view.blocks.find((b) => b.blockId === view.focusBlockIds[0])?.pageNo ?? 1,
  );
  const page = $derived(chosen ?? focusPage);
  // A PDF has one block per page, so a span belongs to the page of its block.
  const ranges = $derived.by(() => {
    const onPage = new Set(view.blocks.filter((b) => b.pageNo === page).map((b) => b.blockId));
    return view.spans.filter((s) => onPage.has(s.blockId));
  });

  let box = $state<HTMLElement | null>(null);
  // The smallest step until `onMount` measures the pane, which is before the
  // first request goes out.
  let width = $state(200);
  onMount(() => {
    if (box === null) return;
    const el = box;
    const measure = () => { width = widthFor(el.clientWidth, window.devicePixelRatio || 1); };
    measure();
    // `width` is already rounded, so an equal value does not re-run the request.
    if (typeof ResizeObserver === 'undefined') return;
    const watcher = new ResizeObserver(measure);
    watcher.observe(el);
    return () => watcher.disconnect();
  });

  // The latest answer, kept while the next page loads: the old picture stays up
  // instead of flashing the loading line on every turn.
  let shown = $state<PdfPageView | null>(null);
  // Monotonic: a late answer for an older request must not be painted over a
  // newer one (a person turns pages faster than the renderer answers).
  let request = 0;
  $effect(() => {
    // A re-read of the document (the window came back) hands a new `view`, so
    // `ranges` is a new array and the same page is drawn again.
    const asked = { place: location, no: page, w: width, spans: ranges };
    const id = ++request;
    renderPdfPage(asked.place, asked.no, asked.w, asked.spans).then(
      (got) => { if (id === request) shown = got; },
      (e) => {
        if (id !== request) return;
        console.error('render_pdf_page failed', e);
        shown = { kind: 'unavailable', reason: 'failed' };
      },
    );
  });

  const pages = $derived(shown?.kind === 'page' ? shown.pages : 0);
  function turn(by: number) {
    const to = page + by;
    if (to >= 1 && (pages === 0 || to <= pages)) chosen = to;
  }
  function onKeydown(e: KeyboardEvent) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    if (e.key === 'ArrowLeft') { e.preventDefault(); turn(-1); }
    else if (e.key === 'ArrowRight') { e.preventDefault(); turn(1); }
  }

  const note = $derived.by(() => {
    void $locale;
    if (shown?.kind !== 'page') return '';
    switch (shown.highlight) {
      case 'changed': return t('page_note_changed');
      case 'scan': return t('page_note_scan');
      case 'shown': return '';
      case 'noText': return '';
    }
    const unreachable: never = shown.highlight;
    return unreachable;
  });
  const pageOf = $derived.by(() => {
    void $locale;
    return shown?.kind === 'page' ? t('page_of', { n: page, m: shown.pages }) : '';
  });
  const unavailable = $derived.by(() => {
    void $locale;
    if (shown?.kind !== 'unavailable') return '';
    return shown.reason === 'fileMissing' ? t('page_unavailable_missing') : t('page_unavailable');
  });
  const L = $derived.by(() => {
    void $locale;
    return {
      label: t('pdf_view_label'), showText: t('page_show_text'), prev: t('page_prev'),
      next: t('page_next'), alt: t('page_image_alt'), loading: t('file_loading'),
    };
  });
</script>

<!-- tabindex -1: a click on the page puts the keyboard here, so ←/→ turn it. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="pdf-page" data-testid="pdf-page" tabindex="-1" bind:this={box}
  aria-label={L.label} onkeydown={onKeydown}>
  {#if shown === null}
    <p data-testid="file-loading">{L.loading}</p>
  {:else if shown.kind === 'unavailable'}
    <p role="status" data-testid="page-unavailable">{unavailable}</p>
    <button type="button" class="page-show-text" data-testid="page-show-text" onclick={onShowText}>
      {L.showText}
    </button>
  {:else}
    {#if note !== ''}
      <p role="status" data-testid="page-note">{note}</p>
    {/if}
    <div class="page-nav">
      <button type="button" data-testid="page-prev" disabled={page <= 1}
        aria-label={L.prev} onclick={() => turn(-1)}>‹</button>
      <span data-testid="page-of">{pageOf}</span>
      <button type="button" data-testid="page-next" disabled={page >= shown.pages}
        aria-label={L.next} onclick={() => turn(1)}>›</button>
    </div>
    <div class="page-sheet">
      <img src={shown.image} alt={L.alt} data-testid="page-image" />
      {#each shown.rects as r}
        <span class="page-box" data-testid="page-box" data-primary={r.primary ? 'true' : undefined}
          style:left="{r.x * 100}%" style:top="{r.y * 100}%"
          style:width="{r.w * 100}%" style:height="{r.h * 100}%"></span>
      {/each}
    </div>
  {/if}
</section>

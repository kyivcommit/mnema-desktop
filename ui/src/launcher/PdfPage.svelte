<script module lang="ts">
  // The width asked of the renderer: the pane's width in device pixels, rounded
  // UP to a multiple of 200 and capped at the backend's own ceiling. Rounding
  // keeps a drag of the splitter from asking for a new picture on every pixel.
  // An unmeasured pane (width 0) asks for the smallest step, not for nothing.
  export function widthFor(clientWidth: number, ratio: number): number {
    return Math.min(3000, Math.max(200, Math.ceil((clientWidth * ratio) / 200) * 200));
  }

  // The page is drawn as tall as the pane, less the page line above it, and
  // scrolls sideways (owner, 2026-11-04: fitted to the width, a dictionary
  // page was unreadable). Never shorter than this, on a very low window.
  // `above` is how far down the pane the page strip starts — the page line,
  // and a note when there is one (a scan's note is two lines). Until the strip
  // has been laid out it is guessed as the page line alone.
  const ABOVE_GUESS_PX = 44;
  const BELOW_PX = 12; // `.pane-body`'s own bottom padding
  const MIN_HEIGHT_PX = 240;
  export function heightFor(paneHeight: number, above: number = ABOVE_GUESS_PX): number {
    return Math.max(MIN_HEIGHT_PX, paneHeight - above - BELOW_PX);
  }
</script>

<script lang="ts">
  import { onMount, tick } from 'svelte';
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
  let strip = $state<HTMLElement | null>(null);
  // The pane's height in CSS pixels, measured on mount and on every resize of
  // the pane's scroller (the window, outside the launcher's own layout).
  let paneHeight = $state(0);
  let ratio = $state(1);
  // Width over height of the last page drawn; a book's pages share it, so the
  // first page's guess (portrait A4/letter) is usually the answer already.
  let aspect = $state(0.75);
  let above = $state<number | null>(null);
  const tall = $derived(heightFor(paneHeight, above ?? undefined));
  // Rounded, so an equal value does not re-run the request.
  const width = $derived(widthFor(tall * aspect, ratio));
  onMount(() => {
    if (box === null) return;
    const scroller = box.closest<HTMLElement>('.pane-body');
    const measure = () => {
      paneHeight = scroller?.clientHeight ?? window.innerHeight;
      ratio = window.devicePixelRatio || 1;
    };
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const watcher = new ResizeObserver(measure);
    watcher.observe(scroller ?? box);
    return () => watcher.disconnect();
  });

  // The latest answer, kept while the next page loads: the old picture stays up
  // instead of flashing the loading line on every turn.
  let shown = $state<PdfPageView | null>(null);
  // Monotonic: a late answer for an older request must not be painted over a
  // newer one (a person turns pages faster than the renderer answers).
  let request = 0;
  function accept(got: PdfPageView) {
    shown = got;
    if (got.kind === 'page' && got.heightPx > 0) aspect = got.widthPx / got.heightPx;
  }
  $effect(() => {
    // A re-read of the document (the window came back) hands a new `view`, so
    // `ranges` is a new array and the same page is drawn again.
    const asked = { place: location, no: page, w: width, spans: ranges };
    const id = ++request;
    renderPdfPage(asked.place, asked.no, asked.w, asked.spans).then(
      (got) => { if (id === request) accept(got); },
      (e) => {
        if (id !== request) return;
        console.error('render_pdf_page failed', e);
        shown = { kind: 'unavailable', reason: 'failed' };
      },
    );
  });

  // A new page opens on its citation: the strip scrolls sideways to centre the
  // first primary box. Only once per page — a redraw of the same page (window
  // focus, resize) leaves where the person scrolled.
  let centred = 0;
  $effect(() => {
    const got = shown;
    if (got?.kind !== 'page' || page === centred) return;
    centred = page;
    const r = got.rects.find((x) => x.primary) ?? got.rects[0];
    if (r === undefined) return;
    void tick().then(() => {
      if (strip === null) return;
      const w = strip.scrollWidth;
      strip.scrollLeft = Math.max(0, r.x * w - (strip.clientWidth - r.w * w) / 2);
    });
  });

  // Where the strip starts, measured once the lines above it are laid out: a
  // scan's note pushed a page fitted to a guess below the pane's bottom. A
  // layout-less DOM reports 0, which keeps the guess.
  $effect(() => {
    void shown;
    void note;
    void tick().then(() => {
      if (strip === null || box === null) return;
      const scroller = box.closest<HTMLElement>('.pane-body');
      const origin = scroller === null
        ? -window.scrollY
        : scroller.getBoundingClientRect().top - scroller.scrollTop;
      const at = strip.getBoundingClientRect().top - origin;
      if (at > 0) above = at;
    });
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
    <div class="page-strip" data-testid="page-strip" bind:this={strip}>
    <div class="page-sheet">
      <img src={shown.image} alt={L.alt} data-testid="page-image" style:height="{tall}px" />
      {#each shown.rects as r}
        <span class="page-box" data-testid="page-box" data-primary={r.primary ? 'true' : undefined}
          style:left="{r.x * 100}%" style:top="{r.y * 100}%"
          style:width="{r.w * 100}%" style:height="{r.h * 100}%"></span>
      {/each}
    </div>
    </div>
  {/if}
</section>

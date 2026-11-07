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
  // `above` is how far down the pane the page strip starts — the page line
  // above it. Until the strip has been laid out it is guessed.
  const ABOVE_GUESS_PX = 44;
  const BELOW_PX = 12; // `.pane-body`'s own bottom padding
  const MIN_HEIGHT_PX = 240;
  export function heightFor(paneHeight: number, above: number = ABOVE_GUESS_PX): number {
    return Math.max(MIN_HEIGHT_PX, paneHeight - above - BELOW_PX);
  }

  // The page strip. Every page's slot is `tall` high whatever has been drawn,
  // so where a page starts is arithmetic, true before its picture arrives.
  export const GAP_PX = 12;
  export function pageTop(n: number, tall: number, gap: number = GAP_PX): number {
    return (n - 1) * (tall + gap);
  }

  // The page with the most of it inside [scrollTop, scrollTop + viewport];
  // a tie goes to the page above.
  export function pageAt(
    scrollTop: number, viewport: number, tall: number, pages: number, gap: number = GAP_PX,
  ): number {
    const step = tall + gap;
    const first = Math.max(1, Math.floor(scrollTop / step) + 1);
    const bottom = scrollTop + viewport;
    let best = Math.min(first, Math.max(1, pages));
    let most = -1;
    for (let n = first; n <= pages && pageTop(n, tall, gap) < bottom; n++) {
      const top = pageTop(n, tall, gap);
      const seen = Math.min(bottom, top + tall) - Math.max(scrollTop, top);
      if (seen > most) { most = seen; best = n; }
    }
    return best;
  }

  // The pages to draw around `at`, in the order they are asked for.
  export function wanted(at: number, pages: number): number[] {
    return [at, at - 1, at + 1].filter((n) => n >= 1 && n <= pages);
  }

  // What to start now: the wanted pages not drawn and not on their way, while
  // fewer than `limit` are on their way (each is a worker process; scans are heavy).
  export function nextRequests(
    want: number[], inFlight: ReadonlySet<number>, cached: ReadonlySet<number>, limit = 2,
  ): number[] {
    const start: number[] = [];
    for (const n of want) {
      if (inFlight.size + start.length >= limit) break;
      if (!inFlight.has(n) && !cached.has(n)) start.push(n);
    }
    return start;
  }

  // The pictures to let go of past `cap`: the farthest from `at`, not the
  // oldest — going back two pages must be instant. Equal distance: the page
  // above goes first (reading runs down).
  export function evict(cached: number[], at: number, cap: number): number[] {
    const order = [...cached].sort((a, b) => Math.abs(b - at) - Math.abs(a - at) || a - b);
    return order.slice(0, Math.max(0, cached.length - cap));
  }
</script>

<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import { locale, t } from '../i18n';
  import { renderPdfPage } from '../lib/ipc';
  import type { DocumentLocation, DocumentView, PdfPageView } from '../lib/ipc';

  // A PDF drawn as the file has it: a strip of pages, one under another, each
  // as tall as the pane, the cited passage boxed (the page mode of the file
  // pane; the text mode is `FilePane`'s own card). `view` is the index's read of
  // the document: it says which page the citation is on and which spans fall on
  // a page — the pictures come from `render_pdf_page`, which finds the file
  // through the index. Only the pages around the one in view are drawn.
  let { location, view, onShowText }: {
    location: DocumentLocation;
    view: Extract<DocumentView, { kind: 'file' }>;
    onShowText: () => void;
  } = $props();

  // Pictures kept at once: ~0.8 MB of the web view's memory each (measured,
  // 2026-11-07); more buys nothing for a strip that draws its view ± 1.
  const CAP = 12;

  const focusPage = $derived(
    view.blocks.find((b) => b.blockId === view.focusBlockIds[0])?.pageNo ?? 1,
  );
  // A PDF has one block per page, so a span belongs to the page of its block.
  function rangesFor(n: number) {
    const onPage = new Set(view.blocks.filter((b) => b.pageNo === n).map((b) => b.blockId));
    return view.spans.filter((s) => onPage.has(s.blockId));
  }

  let box = $state<HTMLElement | null>(null);
  let strip = $state<HTMLElement | null>(null);
  // The pane's scroller; FilePane on its own (tests) has none, and the window scrolls.
  let scroller: HTMLElement | null = null;
  // The pane's height in CSS pixels, measured on mount and on every resize of
  // the pane's scroller (the window, outside the launcher's own layout).
  let paneHeight = $state(0);
  let ratio = $state(1);
  // Width over height of the cited page; a book's pages share it, so the
  // first guess (portrait A4/letter) is usually the answer already. Taken from
  // the cited page alone: from every page, a mixed book would change the width
  // asked, and so redraw the strip, on every landscape page.
  let aspect = $state(0.75);
  let above = $state<number | null>(null);
  const tall = $derived(heightFor(paneHeight, above ?? undefined));
  // Rounded, so an equal value does not re-run the requests.
  const width = $derived(widthFor(tall * aspect, ratio));

  // Every page count is learnt from the first answer; until then the strip is
  // only the cited page, and it is the first one asked for.
  let pages = $state(0);
  // The page most in view; 0 until the person scrolls.
  let at = $state(0);
  const here = $derived(at || focusPage);
  // A drawing per page, tagged with the generation it was asked in. A new
  // `view` (the window came back) or a new width starts a new generation: the
  // old pictures stay up until their page is drawn again, but count as not
  // drawn, and a late answer for an old generation is dropped.
  const slots = new SvelteMap<number, { got: PdfPageView; gen: number }>();
  let gen = 0;
  let inFlight = new Set<number>();
  // The file itself is gone (or the index lost its path): no page can be drawn.
  let gone = $state<PdfPageView | null>(null);

  function scrollTop() {
    return scroller !== null ? scroller.scrollTop : document.documentElement.scrollTop;
  }
  function setScrollTop(px: number) {
    if (scroller !== null) scroller.scrollTop = px;
    else document.documentElement.scrollTop = px;
  }
  function viewport() {
    return scroller !== null ? scroller.clientHeight : window.innerHeight;
  }
  // The strip's own coordinates: the page line sits sticky over the top
  // `above` pixels of the pane, so page n's slot fills the pane below it when
  // the scroller is at `pageTop(n)`.
  function onScroll() {
    if (pages === 0) return;
    at = pageAt(scrollTop(), viewport() - (above ?? ABOVE_GUESS_PX), tall, pages);
    pump();
  }

  onMount(() => {
    if (box === null) return;
    scroller = box.closest<HTMLElement>('.pane-body');
    const measure = () => {
      paneHeight = scroller?.clientHeight ?? window.innerHeight;
      ratio = window.devicePixelRatio || 1;
    };
    measure();
    const target: HTMLElement | Window = scroller ?? window;
    target.addEventListener('scroll', onScroll, { passive: true });
    const watcher = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    watcher?.observe(scroller ?? box);
    return () => {
      target.removeEventListener('scroll', onScroll);
      watcher?.disconnect();
    };
  });

  $effect(() => {
    void view;
    void width;
    untrack(() => {
      gen += 1;
      inFlight = new Set();
      pump();
    });
  });

  function pump() {
    const want = pages === 0 ? [focusPage] : wanted(here, pages);
    const drawn = new Set<number>();
    for (const [n, s] of slots) if (s.gen === gen) drawn.add(n);
    for (const n of nextRequests(want, inFlight, drawn)) ask(n);
  }

  function ask(n: number) {
    const g = gen;
    inFlight.add(n);
    renderPdfPage(location, n, width, rangesFor(n)).then(
      (got) => settle(n, g, got),
      (e) => {
        if (g === gen) console.error('render_pdf_page failed', e);
        settle(n, g, { kind: 'unavailable', reason: 'failed' });
      },
    );
  }

  function settle(n: number, g: number, got: PdfPageView) {
    if (g !== gen) return;
    inFlight.delete(n);
    if (got.kind === 'unavailable' && (got.reason !== 'failed' || pages === 0)) {
      // About the file, not the page — or the first page failed and the strip
      // has nothing to stand on.
      gone = got;
      return;
    }
    // A page drawn in this generation means the file is back: a whole-document
    // failure is never final (a missing file restored, a first page that timed out).
    if (got.kind === 'page') gone = null;
    slots.set(n, { got, gen: g });
    const asked = width;
    if (got.kind === 'page') {
      pages = got.pages;
      if (n === focusPage && got.heightPx > 0) aspect = got.widthPx / got.heightPx;
      if (n === focusPage && !placed) place(got);
    }
    for (const k of evict([...slots.keys()], here, CAP)) slots.delete(k);
    // A new shape is a new width: the generation it starts asks for the pages.
    if (width === asked) pump();
  }

  // The strip opens on its citation: the cited page under the page line, and
  // scrolled sideways to centre the first primary box. Once per mount — a
  // redraw (window focus, resize) leaves where the person scrolled.
  let placed = false;
  function place(got: Extract<PdfPageView, { kind: 'page' }>) {
    placed = true;
    void tick().then(() => {
      setScrollTop(pageTop(focusPage, tall));
      const r = got.rects.find((x) => x.primary) ?? got.rects[0];
      if (r === undefined || strip === null) return;
      const w = strip.scrollWidth;
      strip.scrollLeft = Math.max(0, r.x * w - (strip.clientWidth - r.w * w) / 2);
    });
  }

  // A new page height (the strip's top measured, the pane resized) moves every
  // slot below the first: the scroll is rescaled to stay on the same page —
  // left alone, a citation hundreds of pages down opened pages away from it.
  let laidStep = 0;
  $effect(() => {
    const step = tall + GAP_PX;
    untrack(() => {
      if (laidStep > 0 && step !== laidStep) setScrollTop((scrollTop() * step) / laidStep);
      laidStep = step;
    });
  });

  // Where the strip starts, measured once the line above it is laid out. A
  // layout-less DOM reports 0, which keeps the guess.
  $effect(() => {
    void pages;
    void gone;
    void tick().then(() => {
      if (strip === null || box === null) return;
      const origin = scroller === null
        ? -window.scrollY
        : scroller.getBoundingClientRect().top - scroller.scrollTop;
      const top = strip.getBoundingClientRect().top - origin;
      if (top > 0) above = top;
    });
  });

  const numbers = $derived(Array.from({ length: pages }, (_, i) => i + 1));

  // The note of a page that has a citation on it; a page with nothing cited
  // has nothing to fail to mark.
  function noteFor(n: number, got: Extract<PdfPageView, { kind: 'page' }>): string {
    void $locale;
    if (rangesFor(n).length === 0) return '';
    switch (got.highlight) {
      case 'changed': return t('page_note_changed');
      case 'scan': return t('page_note_scan');
      case 'unplaced': return t('page_note_unplaced');
      case 'shown': return '';
      case 'noText': return '';
    }
    const unreachable: never = got.highlight;
    return unreachable;
  }
  const pageOf = $derived.by(() => {
    void $locale;
    return pages > 0 ? t('page_of', { n: here, m: pages }) : '';
  });
  const unavailable = $derived.by(() => {
    void $locale;
    if (gone?.kind !== 'unavailable') return '';
    return gone.reason === 'fileMissing' ? t('page_unavailable_missing') : t('page_unavailable');
  });
  const L = $derived.by(() => {
    void $locale;
    return {
      label: t('pdf_view_label'), showText: t('page_show_text'),
      failed: t('page_unavailable'), loading: t('file_loading'),
    };
  });
  function altFor(n: number): string {
    void $locale;
    return t('page_image_alt', { n });
  }
</script>

<!-- tabindex -1: a click on the page puts the keyboard here, so the page keys scroll the pane. -->
<section class="pdf-page" data-testid="pdf-page" tabindex="-1" bind:this={box} aria-label={L.label}>
  {#if gone !== null}
    <p role="status" data-testid="page-unavailable">{unavailable}</p>
    <button type="button" class="page-show-text" data-testid="page-show-text" onclick={onShowText}>
      {L.showText}
    </button>
  {:else if pages === 0}
    <p data-testid="file-loading">{L.loading}</p>
  {:else}
    <div class="page-line" data-testid="page-of">{pageOf}</div>
    <div class="page-strip" data-testid="page-strip" bind:this={strip}>
      {#each numbers as n (n)}
        {@const got = slots.get(n)?.got}
        <div class="page-slot" data-testid="page-slot" data-page={n}
          style:height="{tall}px" style:margin-bottom="{GAP_PX}px">
          {#if got?.kind === 'page'}
            {@const note = noteFor(n, got)}
            {#if note !== ''}
              <p role="status" data-testid="page-note">{note}</p>
            {/if}
            <!-- The sheet takes the page's own shape, so its width is the picture's
                 and the boxes, in fractions of it, land on the words. -->
            <div class="page-sheet" data-testid="page-sheet" style:aspect-ratio="{got.widthPx} / {got.heightPx}">
              <img src={got.image} alt={altFor(n)} data-testid="page-image" />
              {#each got.rects as r}
                <span class="page-box" data-testid="page-box" data-primary={r.primary ? 'true' : undefined}
                  style:left="{r.x * 100}%" style:top="{r.y * 100}%"
                  style:width="{r.w * 100}%" style:height="{r.h * 100}%"></span>
              {/each}
            </div>
          {:else if got?.kind === 'unavailable'}
            <p role="status" data-testid="page-failed">{L.failed}</p>
          {:else}
            <div class="page-sheet page-wait" data-testid="page-wait" aria-busy="true"
              style:width="{tall * aspect}px"></div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</section>

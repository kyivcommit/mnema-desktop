<script lang="ts" module>
  // The types `render_doc` draws as HTML (`doc_view.rs::reader_for`).
  export const DOC_PAGE_MIMES: ReadonlySet<string> = new Set(['text/markdown', 'text/html', 'application/epub+zip']);
</script>

<script lang="ts">
  // A document drawn as HTML — a Markdown or HTML file whole, an EPUB one
  // chapter at a time (D199). `render_doc` reads the
  // file through the index and answers allow-listed HTML with the citation in
  // `<mark>`; this component puts it into the pane as it is, scrolls the clicked
  // citation in, and says when the marks cannot be trusted.
  import { tick, untrack } from 'svelte';
  import { locale, t } from '../i18n';
  import { renderDoc } from '../lib/ipc';
  import type { DocPageView, DocumentLocation, DocumentView, PaneSpan } from '../lib/ipc';
  import { scrollWithin } from '../lib/scroll';

  let { location, view, onShowText }: {
    location: DocumentLocation;
    view: Extract<DocumentView, { kind: 'file' }>;
    onShowText: () => void;
  } = $props();

  let got = $state<DocPageView | null>(null);
  let failed = $state(false);
  let box = $state<HTMLElement | null>(null);
  // Monotonic, as in `FilePane.read`: an older answer is dropped, not painted
  // over a newer one.
  let request = 0;
  // The EPUB chapter on the page, or asked for by a chapter button: a refresh
  // draws it again. Unset for a new place, where the backend picks the cited one.
  let chapter = $state<number | undefined>(undefined);
  // An EPUB's chapters are the page numbers its blocks sit on; a whole file has none.
  // Only chapters with text count: a cover or a plate has no block, cannot be
  // opened (`noChapter`), and "Chapter N of M" counts what can be read — so N and
  // M are positions in this list, not spine numbers (`pageNo`, the reply's `pages`).
  const chapters = $derived(view.mime === 'application/epub+zip'
    ? [...new Set(view.blocks.map((b) => b.pageNo))].sort((a, b) => a - b)
    : []);
  const at = $derived(chapter === undefined ? -1 : chapters.indexOf(chapter));

  // The pane's scroller: `.file-card`'s parent (`FilePane.scroller`).
  const pane = () => box?.parentElement?.parentElement ?? null;

  // `reveal` is a new place or new citations: clear the page, then scroll the
  // clicked citation in. A refresh (the window came back, the file may have
  // been edited) keeps the page up while it is drawn again and leaves the
  // scroll alone — FilePane puts it back after its own read, and a second
  // restore here would undo a scroll made while this one was in flight.
  async function draw(place: DocumentLocation, spans: PaneSpan[], reveal: boolean, pageNo?: number) {
    const id = ++request;
    if (reveal) {
      got = null;
      failed = false;
    }
    let answer: DocPageView;
    try {
      answer = await renderDoc(place, spans, pageNo);
    } catch (e) {
      if (id !== request) return;
      console.error('render_doc failed', e);
      // A thrown refresh keeps the page it has (FilePane's rule); an answer —
      // a page or a refusal — always replaces it.
      if (reveal) failed = true;
      return;
    }
    if (id !== request) return;
    // An answer ends a failure, as in `PdfPage.settle`: one that failed
    // before is never final.
    failed = false;
    got = answer;
    if (answer.kind === 'page') chapter = answer.pageNo ?? undefined;
    await tick();
    const scroller = pane();
    if (id !== request || !reveal || box === null || scroller === null) return;
    // The clicked citation, else any; a document opened with none stays at its top.
    const target = box.querySelector<HTMLElement>('mark[data-primary="true"]')
      ?? box.querySelector<HTMLElement>('mark');
    if (target) scrollWithin(scroller, target, 'center');
    else scroller.scrollTop = 0;
  }

  function go(step: -1 | 1) {
    const next = chapters[at + step];
    if (next === undefined) return;
    chapter = next;
    void draw(location, view.spans, true, next);
  }

  // Every `view` the pane reads is drawn: the pane re-reads the document when
  // the window comes back (§7.3), and the file may have changed under it. Only
  // a new place or new citations is a reveal.
  let drawnFor = '';
  $effect(() => {
    const place = location;
    const spans = view.spans;
    const key = JSON.stringify([place, spans]);
    untrack(() => {
      const reveal = key !== drawnFor;
      drawnFor = key;
      if (reveal) chapter = undefined;
      void draw(place, spans, reveal, chapter);
    });
  });

  const L = $derived.by(() => {
    void $locale;
    return {
      label: t('page_view_label'), showText: t('page_show_text'),
      failed: t('page_unavailable'), loading: t('file_loading'),
      prev: t('page_prev_chapter'), next: t('page_next_chapter'),
      chapter: at < 0 ? '' : t('page_chapter_of', { n: at + 1, m: chapters.length }),
    };
  });
  // A note only where a citation was asked for: a file opened at its start has
  // nothing to fail to mark.
  const note = $derived.by(() => {
    void $locale;
    if (got?.kind !== 'page' || view.spans.length === 0) return '';
    switch (got.highlight) {
      case 'changed': return t('page_note_changed');
      case 'unplaced': return t('page_note_unplaced');
      case 'shown': case 'noText': return '';
    }
  });
  const unavailable = $derived.by(() => {
    void $locale;
    if (got?.kind !== 'unavailable') return '';
    switch (got.reason) {
      case 'fileMissing': return t('page_unavailable_missing');
      case 'tooLarge': return t('page_unavailable_too_large');
      case 'noChapter': return t('page_unavailable_no_chapter');
      case 'noPath': case 'failed': return t('page_unavailable');
    }
  });
</script>

{#snippet chapterBar()}
  <nav class="doc-chapters" aria-label={L.chapter}>
    <button type="button" class="page-show-text" data-testid="doc-chapter-prev" aria-label={L.prev}
      title={L.prev} disabled={at <= 0} onclick={() => go(-1)}>&lt;</button>
    <span data-testid="doc-chapter-label">{L.chapter}</span>
    <button type="button" class="page-show-text" data-testid="doc-chapter-next" aria-label={L.next}
      title={L.next} disabled={at >= chapters.length - 1} onclick={() => go(1)}>&gt;</button>
  </nav>
{/snippet}

<section class="doc-page" data-testid="doc-page" aria-label={L.label} bind:this={box}>
  {#if at >= 0}{@render chapterBar()}{/if}
  {#if failed || got?.kind === 'unavailable'}
    <p role="status" data-testid="page-unavailable">{failed ? L.failed : unavailable}</p>
    <button type="button" class="page-show-text" data-testid="page-show-text" onclick={onShowText}>
      {L.showText}
    </button>
  {:else if got === null}
    <p data-testid="file-loading">{L.loading}</p>
  {:else}
    {#if note !== ''}
      <p role="status" data-testid="page-note">{note}</p>
    {/if}
    <!-- Allow-listed by the worker (`doc_view/sanitize.rs`); the CSP is the second line. -->
    <div class="doc-body" data-testid="doc-body">{@html got.html}</div>
  {/if}
</section>

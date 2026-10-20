<script module lang="ts">
  import type { PaneSpan } from '../lib/ipc';

  /** One run of a block: either plain text or a highlighted stretch. */
  export type Piece = { mark: boolean; text: string; primary: boolean };

  // 🔴 The arithmetic, ported from the source card this pane replaces:
  // `blockStart` is the offset into the text of the block the span names, and
  // `length` is the slice of the chunk (`tree.rs`'s `PaneSpan`). The unit is
  // Unicode scalar values, not UTF-16 units — one emoji earlier in the
  // paragraph moves the highlight with no error anywhere. `state.ts:15` counts
  // the query limit the same way (D131).
  //
  // Spans that touch or overlap become ONE mark; a merged run is primary when
  // any span it absorbed was.
  export function paintBlock(text: string, spans: PaneSpan[]): Piece[] {
    const chars = [...text];
    const runs: { start: number; end: number; primary: boolean }[] = [];

    for (const span of [...spans].sort((a, b) => a.blockStart - b.blockStart)) {
      const start = Math.max(0, Math.min(span.blockStart, chars.length));
      const end = Math.max(start, Math.min(span.blockStart + span.length, chars.length));
      if (end === start) continue;
      const last = runs[runs.length - 1];
      if (last && start <= last.end) {
        last.end = Math.max(last.end, end);
        last.primary = last.primary || span.primary;
      } else {
        runs.push({ start, end, primary: span.primary });
      }
    }

    const pieces: Piece[] = [];
    let at = 0;
    for (const run of runs) {
      if (run.start > at) pieces.push({ mark: false, text: chars.slice(at, run.start).join(''), primary: false });
      pieces.push({ mark: true, text: chars.slice(run.start, run.end).join(''), primary: run.primary });
      at = run.end;
    }
    if (at < chars.length) pieces.push({ mark: false, text: chars.slice(at).join(''), primary: false });
    return pieces;
  }

  // A markdown headline's text is the file's literal slice, so it arrives with
  // its own marker (spec §3). `headline` also comes from the HTML and docx
  // readers, where the text IS the heading and `#1 Priority` is the document's
  // own line — so only what markdown itself put there is stripped:
  //   - an ATX marker, `## Heading`, and its optional closing run,
  //     `## Heading ##`, which counts as one only when the leading marker
  //     already proved the text is ATX.
  //   - a Setext underline, the `====`/`----` line under a multi-line heading.
  // The stored text is never touched: this is a display copy. Span offsets
  // count from the stored text, so they move by exactly the stripped prefix,
  // and a span that starts inside the marker keeps its end where it was.
  export function heading(text: string, spans: PaneSpan[]): { text: string; spans: PaneSpan[] } {
    const atx = /^ {0,3}#{1,6}(?:[ \t]+|$)/.exec(text);
    const shown = atx
      ? text.slice(atx[0].length).replace(/[ \t]+#+[ \t]*$/, '')
      : text.replace(/\n {0,3}(?:=+|-+)[ \t]*$/, '');
    const lead = [...(atx?.[0] ?? '')].length;
    const length = [...shown].length;
    return {
      text: shown,
      spans: spans.map((s) => {
        const start = Math.max(0, Math.min(s.blockStart - lead, length));
        const end = Math.max(start, Math.min(s.blockStart + s.length - lead, length));
        return { ...s, blockStart: start, length: end - start };
      }),
    };
  }
</script>

<script lang="ts">
  import { tick } from 'svelte';
  import { locale, t } from '../i18n';
  import { openDocument } from '../lib/ipc';
  import type { DocumentFocus, DocumentLocation, DocumentView, Freshness } from '../lib/ipc';

  // The whole of one document, read out of the index through `open_document`
  // (spec §3) — never off the disk: the file is always newer than the index,
  // and reading it would show text the citation no longer matches with no sign
  // that anything is wrong.
  //
  // `focus` is one citation's passage. The pane scrolls the first of its blocks
  // into view and paints `spans`; `onFocusHandled` lets the caller drop the
  // focus it passed, or every later render would scroll there again.
  let { location, focus, onFocusHandled }: {
    location: DocumentLocation;
    focus: DocumentFocus | null;
    onFocusHandled: () => void;
  } = $props();

  let view = $state<DocumentView | null>(null);
  let failed = $state(false);
  let card = $state<HTMLElement | null>(null);
  // Monotonic: a person clicks faster than `open_document` answers, and an
  // older answer must be dropped rather than painted over a newer one.
  let request = 0;
  // The location on screen. A cleared `focus` is the caller acknowledging the
  // one just handled, not a request to read the same file again.
  let loadedFor: DocumentLocation | null = null;
  // The citation being painted, kept after the caller acknowledges it: a
  // refresh must re-read WITH it, or the highlight the person is looking at
  // disappears under them.
  let shownFocus: DocumentFocus | null = null;
  // A focus was sent and the backend accepted none of it — a reindex or a
  // reused chunk id leaves a citation pointing at a passage that is gone. The
  // pane says so instead of showing the file as if that were the citation.
  let focusGone = $state(false);

  // The card's parent is the pane's scroller (`.pane-body`), the only thing a
  // refresh has to put back. shortcut: one level up, revisit if the card moves.
  const scroller = () => card?.parentElement ?? null;

  // `reveal` is the person's own open: clear the card, then scroll the cited
  // block in and acknowledge the focus. A refresh keeps both — the text on
  // screen stays up while the read is in flight, and the scroll is theirs.
  async function read(place: DocumentLocation, at: DocumentFocus | null, reveal: boolean) {
    const id = ++request;
    if (reveal) {
      view = null;
      failed = false;
    }
    const box = scroller();
    const scroll = box?.scrollTop ?? 0;
    let got: DocumentView;
    try {
      got = await openDocument(place, at);
    } catch (e) {
      if (id !== request) return;
      console.error('open_document failed', e);
      // A failed refresh keeps the file it already has: the read failed, not
      // the text on screen (Tree.svelte's rule for the same event).
      if (reveal) failed = true;
      return;
    }
    if (id !== request) return;
    view = got;
    loadedFor = place;
    shownFocus = at;
    focusGone = at !== null && got.kind === 'file' && got.focusBlockIds.length === 0;
    await tick();
    if (id !== request) return;
    if (!reveal) {
      if (box !== null) box.scrollTop = scroll;
      return;
    }
    const first = got.kind === 'file' ? got.focusBlockIds[0] : undefined;
    if (first !== undefined) {
      // A read that worked is painted even where there is no scrolling to do
      // (a DOM without layout); the file must never be called failed for it.
      card?.querySelector<HTMLElement>(`[data-block-id="${first}"]`)
        ?.scrollIntoView?.({ block: 'center' });
    }
    if (at !== null) onFocusHandled();
  }

  $effect(() => {
    const place = location;
    const at = focus;
    if (at === null && place === loadedFor) return;
    void read(place, at, true);
  });

  // §7.3: the window is hidden, not closed, so the file the pane shows may have
  // been edited in another application while it was out of sight. Re-read it
  // when the window comes back — the same location and the same citation. Not
  // while a read is in flight: that one is the newer answer, and dropping it
  // would drop the focus acknowledgement it owes the caller.
  function onWindowFocus() {
    if (loadedFor === null || view === null) return;
    void read(loadedFor, shownFocus, false);
  }

  // Everything is a paragraph unless its `block.type` says otherwise — page
  // headers and footers included: the pane is the file, and hiding parts of it
  // would make a short file look whole.
  const painted = $derived.by(() => {
    const got = view;
    if (got?.kind !== 'file') return [];
    return got.blocks.map((b) => {
      const spans = got.spans.filter((s) => s.blockId === b.blockId);
      const shown = b.kind === 'headline' ? heading(b.text, spans) : { text: b.text, spans };
      return {
        blockId: b.blockId,
        tag: b.kind === 'headline' ? 'h4' : b.kind === 'code' ? 'pre' : 'p',
        pieces: paintBlock(shown.text, shown.spans),
      };
    });
  });

  const loadingLabel = $derived.by(() => { void $locale; return t('file_loading'); });
  const failedLabel = $derived.by(() => { void $locale; return t('file_failed'); });
  const focusGoneLabel = $derived.by(() => { void $locale; return t('file_focus_gone'); });
  // The header names the place on disk. A citation may carry none — the backend
  // resolves the document's sole `path` row for freshness either way, and the
  // view answers with the path it was resolved for — so the title follows that
  // instead of staying blank.
  const title = $derived(
    location.relativePath ?? (view?.kind === 'file' ? view.relativePath : null),
  );
  const badge = $derived.by(() => {
    void $locale;
    const got = view;
    return got?.kind === 'file' ? freshnessText(got.freshness) : '';
  });

  // The badge speaks only when it carries news (owner, 2026-10-20): `current`
  // is silence, not «Up to date» — nothing happened and nobody needs telling.
  // `noPath` stays silent too: it has three causes and deletion is only one
  // of them (spec §3), so there is nothing honest to say and the file still
  // opens.
  function freshnessText(f: Freshness): string {
    switch (f.kind) {
      case 'current': return '';
      case 'noPath': return '';
      case 'reindexed': return t('fresh_reindexed');
      case 'fileChanged': return t('fresh_file_changed');
      case 'fileMissing': return t('fresh_file_missing');
    }
    // 🔴 No default: a pane that draws a badge for an unmatched variant is how
    // a stale file gets shown as fresh. Add a sixth variant and this line stops
    // compiling.
    const unreachable: never = f;
    return unreachable;
  }

  function unavailableText(reason: Extract<DocumentView, { kind: 'unavailable' }>['reason']): string {
    switch (reason) {
      case 'noSuchDocument': return t('file_unavailable_no_such_document');
      case 'noBlocks': return t('file_unavailable_no_blocks');
      case 'rebuilding': return t('file_unavailable_rebuilding');
      case 'tooLarge': return t('file_unavailable_too_large');
    }
    const unreachable: never = reason;
    return unreachable;
  }
</script>

<svelte:window onfocus={onWindowFocus} />

<div class="file-card" data-testid="file-body" bind:this={card}>
  {#if title !== null}
    <h3 data-testid="file-header">{title}</h3>
  {/if}

  {#if failed}
    <p data-testid="file-failed">{failedLabel}</p>
  {:else if view === null}
    <p data-testid="file-loading">{loadingLabel}</p>
  {:else if view.kind === 'unavailable'}
    <p role="status" data-testid="file-unavailable">{unavailableText(view.reason)}</p>
  {:else}
    {#if badge !== ''}
      <p role="status" data-testid="file-badge">{badge}</p>
    {/if}
    {#if focusGone}
      <p role="status" data-testid="file-focus-gone">{focusGoneLabel}</p>
    {/if}
    {#each painted as block (block.blockId)}
      <svelte:element this={block.tag} data-testid="file-block" data-block-id={block.blockId}
        >{#each block.pieces as piece}{#if piece.mark}<mark data-testid="hl"
              data-primary={piece.primary ? 'true' : undefined}>{piece.text}</mark
            >{:else}{piece.text}{/if}{/each}</svelte:element>
    {/each}
  {/if}
</div>

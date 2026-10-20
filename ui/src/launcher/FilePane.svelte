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

  // A headline's text is the file's literal slice, so it arrives with its own
  // `#` markers (spec §3). They are stripped for display only, and the span
  // offsets are moved by exactly the stripped prefix — those offsets count from
  // the stored text, marker included.
  export function heading(text: string, spans: PaneSpan[]): { text: string; spans: PaneSpan[] } {
    const shown = text.replace(/^#{1,6}\s*/, '');
    const cut = [...text].length - [...shown].length;
    return { text: shown, spans: spans.map((s) => ({ ...s, blockStart: Math.max(0, s.blockStart - cut) })) };
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

  $effect(() => {
    const place = location;
    const at = focus;
    if (at === null && place === loadedFor) return;
    const id = ++request;
    view = null;
    failed = false;
    openDocument(place, at)
      .then(async (got) => {
        if (id !== request) return;
        view = got;
        loadedFor = place;
        await tick();
        if (id !== request) return;
        const first = got.kind === 'file' ? got.focusBlockIds[0] : undefined;
        if (first !== undefined) {
          card?.querySelector<HTMLElement>(`[data-block-id="${first}"]`)
            ?.scrollIntoView({ block: 'center' });
        }
        if (at !== null) onFocusHandled();
      })
      .catch((e) => {
        if (id !== request) return;
        console.error('open_document failed', e);
        failed = true;
      });
  });

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
  const badge = $derived.by(() => {
    void $locale;
    const got = view;
    // No badge when the location is unknown: `noPath` has three causes and
    // deletion is only one of them (spec §3), so there is nothing honest to
    // say and the file still opens.
    return got?.kind === 'file' && got.freshness.kind !== 'noPath' ? freshnessText(got.freshness) : '';
  });

  function freshnessText(f: Freshness): string {
    switch (f.kind) {
      case 'current': return t('fresh_current');
      case 'reindexed': return t('fresh_reindexed');
      case 'fileChanged': return t('fresh_file_changed');
      case 'fileMissing': return t('fresh_file_missing');
      case 'noPath': return t('fresh_no_path');
    }
    // 🔴 No default: a pane that draws `Current` for an unmatched variant is how
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

<div class="file-card" data-testid="file-body" bind:this={card}>
  {#if location.relativePath !== null}
    <h3 data-testid="file-header">{location.relativePath}</h3>
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
    {#each painted as block (block.blockId)}
      <svelte:element this={block.tag} data-testid="file-block" data-block-id={block.blockId}
        >{#each block.pieces as piece}{#if piece.mark}<mark data-testid="hl"
              data-primary={piece.primary ? 'true' : undefined}>{piece.text}</mark
            >{:else}{piece.text}{/if}{/each}</svelte:element>
    {/each}
  {/if}
</div>

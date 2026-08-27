<script module lang="ts">
  import type { SourceBlock, WireSegment } from '../lib/ipc';

  /** A span resolved into the coordinates of the block it paints. */
  type Painted = { blockId: number; start: number; length: number; primary: boolean };
  /** One run of a block: either plain text or a highlighted stretch. */
  type Piece = { mark: boolean; text: string; primary: boolean };

  // 🔴 The one fact this card turns on (`src-tauri/src/tree.rs:265-278`):
  // `blockStart` is the offset into the text of the block `blockId` names, and
  // that is where the highlight begins. `start`/`end` are offsets into the
  // CHUNK's own text, which never reaches the wire — their only use here is the
  // length `end - start`. Indexing the block with them is the wrong answer that
  // looks right.
  function toPainted(spans: WireSegment[], primary: boolean): Painted[] {
    return spans.map((s) => ({
      blockId: s.blockId,
      start: s.blockStart,
      length: s.end - s.start,
      primary,
    }));
  }

  // Decision 4: spans are unioned per block, so two that touch or overlap become
  // ONE mark. A merged run is primary when any span it absorbed was the clicked
  // citation's (Ruling V) — the union is what PR 10 styles, and it is the
  // clicked passage that must stand out inside it.
  //
  // 🔴 Every slice counts code points, not UTF-16 units (Ruling R): every offset
  // this pipeline emits comes from `text.chars().count()`, and one character
  // outside the BMP earlier in the paragraph moves the highlight with no error
  // anywhere. `state.ts:15` counts the query limit the same way (D131).
  export function paintBlock(text: string, spans: Painted[]): Piece[] {
    const chars = [...text];
    const runs: { start: number; end: number; primary: boolean }[] = [];

    for (const span of [...spans].sort((a, b) => a.start - b.start)) {
      const start = Math.max(0, Math.min(span.start, chars.length));
      const end = Math.max(start, Math.min(span.start + span.length, chars.length));
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

  // 🔴 The ONE place a span is matched to a block, and therefore the one place
  // that drops a span for a block the window does not hold. Siblings are
  // separate `source_around` round trips with their own, different windows, and
  // a span of theirs for a block outside this one must be dropped, never
  // appended: appending it would paint text the card is not showing and extend
  // a window the backend never called contiguous. A second copy of that rule in
  // the caller looked like defence in depth and was measurably not — removing it
  // left the whole suite green, because this line was doing the work either way.
  export function paintBlocks(blocks: SourceBlock[], spans: Painted[]) {
    return blocks.map((b) => ({
      blockId: b.blockId,
      pieces: paintBlock(b.text, spans.filter((s) => s.blockId === b.blockId)),
    }));
  }
</script>

<script lang="ts">
  import { locale, t } from '../i18n';
  import { citationLabel } from '../i18n/label';
  import { sourceAround } from '../lib/ipc';
  import type { AskCitation, Freshness, Hit, SourceAround } from '../lib/ipc';

  let { selected, siblings }: {
    selected: AskCitation | Hit;
    siblings: (AskCitation | Hit)[];
  } = $props();

  let answer = $state<SourceAround | null>(null);
  let failed = $state(false);
  // M2: the clicked excerpt came back naming a document other than the one the
  // citation names. See the check itself for why this exists at all.
  let mismatched = $state(false);
  let siblingSpans = $state<WireSegment[]>([]);
  // Monotonic. `ask` and `source_around` are separate IPC round trips and a
  // person clicks faster than they return, so an answer for an older click must
  // be dropped rather than painted over a newer one.
  let request = 0;
  // How many `source_around` round trips are still in flight, published on the
  // card as `data-pending`. 🔴 It is not decoration: the sibling rules below are
  // all about what does NOT appear, and every sibling round trip lands a
  // microtask after the clicked one. Without a signal saying all of them have
  // settled, an assertion that "the sibling did not paint" is satisfied by a
  // card that has not heard from the sibling yet — green for a reason unrelated
  // to what it claims. PR 10 gets a progress hook out of the same field.
  let pending = $state(0);

  // The occurrence identity PR 6a put on the wire — `documentId` + `ord` — plus
  // the query key, so the clicked citation is not also asked as its own sibling.
  // NUL as the separator (written as the escape `\0`, never as a raw byte: one
  // of those makes the whole file binary and invisible to `git diff`).
  const occurrence = (c: AskCitation | Hit) => `${c.documentId}\0${c.ord}\0${c.chunkId}`;

  // Keyed on `selected` alone: `siblings` is read inside the callback, outside
  // the tracked scope, so a new sibling list on its own does not refetch — the
  // card refetches when the person clicks a different citation.
  $effect(() => {
    const clicked = selected;
    const id = ++request;
    answer = null;
    failed = false;
    mismatched = false;
    siblingSpans = [];
    pending = 1;

    sourceAround(clicked)
      .then((got) => {
        if (id !== request) return;
        answer = got;

        // M1: the answer's kind is asked ONCE, and everything that depends on
        // it lives below. It used to be asked twice — once to decide whether to
        // build `toAsk`, again to leave the function — and the second copy could
        // be deleted with the whole suite green. It is not merely dead code
        // though: it carries the narrowing that `extra.documentId !==
        // got.documentId` needs, so the repair is to ask once, not to delete a
        // copy. Zeroing `pending` here is the job the first copy was doing.
        if (got.kind !== 'excerpt') {
          pending = 0;
          return;
        }

        // 🔴 M2 — deliberate defence against a guarantee that lives in another
        // repository. PR 6a pins `documentId` + `ord` in Rust and answers
        // `Gone { idReused }` when the clicked citation and the chunk disagree,
        // so this branch is unreachable while that holds. It exists because this
        // component cannot see that invariant, and because the price of it
        // regressing is the worst thing this card could do: another document's
        // text under this document's name, badged as up to date. Treated exactly
        // as a `Gone` is — a reason shown, and NO text. Note the two sibling
        // filters below use two different reference values (the citation's
        // `documentId` before the call, the excerpt's after it); this is the one
        // place those two are compared with each other.
        if (got.documentId !== clicked.documentId) {
          mismatched = true;
          pending = 0;
          return;
        }

        // The clicked citation's excerpt IS the card: its blocks, its two
        // `hasMore*` flags, its freshness. Siblings contribute only spans —
        // never blocks, never flags, never a verdict — and `paintBlocks` above
        // is what confines those spans to the blocks this window shows.
        const asked = new Set([occurrence(clicked)]);
        const toAsk: (AskCitation | Hit)[] = [];
        for (const sibling of siblings) {
          // Ruling U, before any call: a citation in another document is not
          // even asked.
          if (sibling.documentId !== clicked.documentId) continue;
          const key = occurrence(sibling);
          if (asked.has(key)) continue;
          asked.add(key);
          toAsk.push(sibling);
        }
        // The clicked round trip is done and the sibling ones are counted in the
        // same step, so `pending` never dips to zero between the two.
        pending = toAsk.length;

        for (const sibling of toAsk) {
          sourceAround(sibling)
            .then((extra) => {
              // 🔴 I3: the sibling's OWN staleness guard, and it is not the
              // clicked one's. A sibling round trip from a previous selection
              // lands seconds late and would append straight into
              // `siblingSpans` — a highlight belonging to a citation the person
              // is no longer looking at. The document check below does not save
              // this: the stale sibling and the current excerpt can both be the
              // same document.
              if (id !== request) return;
              // A sibling answering `Gone` contributes nothing and changes no
              // verdict.
              if (extra.kind !== 'excerpt') return;
              // 🔴 Ruling U, on the answer: a sibling round trip happens seconds
              // after the clicked one and can come back naming a different
              // document (`tree.rs:155-166` carries `documentId` on the excerpt
              // for exactly this). Painting its spans would put a highlight on
              // text from another file — the one thing this whole PR exists to
              // prevent.
              if (extra.documentId !== got.documentId) return;
              siblingSpans = [...siblingSpans, ...extra.spans];
            })
            .catch((e) => {
              // Non-fatal: a sibling that cannot be read costs a highlight, not
              // the card.
              console.error('source_around (sibling) failed', e);
            })
            .finally(() => {
              // 🔴 I4: `data-pending` is the anchor every test in this card's
              // suite waits on. A stale sibling decrementing the CURRENT
              // selection's counter would make the card report itself settled
              // while its own round trip is still in flight — silently
              // re-opening the trap this field was added to close.
              if (id === request) pending -= 1;
            });
        }
      })
      .catch((e) => {
        if (id !== request) return;
        console.error('source_around failed', e);
        failed = true;
        pending = 0;
      });
  });

  // M2: a mismatched excerpt is not the card's text, so nothing downstream of
  // here — no block, no highlight, no ellipsis — can render it.
  const excerpt = $derived(
    !mismatched && answer !== null && answer.kind === 'excerpt' ? answer : null,
  );

  const painted = $derived(
    excerpt === null
      ? []
      : paintBlocks(excerpt.blocks, [
          ...toPainted(excerpt.spans, true),
          ...toPainted(siblingSpans, false),
        ]),
  );

  // Ruling S: the header takes the file's name from the CITATION's
  // `relativePath` — the excerpt deliberately carries none (`tree.rs:160-166`)
  // — through the one Decision 1 rule `Answer` also calls.
  const header = $derived.by(() => { void $locale; return citationLabel(selected); });
  const loadingLabel = $derived.by(() => { void $locale; return t('source_loading'); });
  const failedLabel = $derived.by(() => { void $locale; return t('source_failed'); });

  function freshnessText(f: Freshness): string {
    switch (f.kind) {
      case 'current': return t('fresh_current');
      case 'reindexed': return t('fresh_reindexed');
      case 'fileChanged': return t('fresh_file_changed');
      case 'fileMissing': return t('fresh_file_missing');
      case 'noPath': return t('fresh_no_path');
    }
    // 🔴 Ruling W. No `else`, no default: a card that draws `Current` for an
    // unmatched variant is how a stale excerpt gets shown as fresh
    // (`tree.rs:200-203`). No test can prove this — a sixth variant does not
    // exist to test with — so the proof is the compiler's: add one to the
    // `Freshness` union and this line stops compiling.
    const unreachable: never = f;
    return unreachable;
  }

  function goneText(reason: Extract<SourceAround, { kind: 'gone' }>['reason']): string {
    switch (reason.kind) {
      case 'noSuchChunk': return t('gone_no_such_chunk');
      case 'idReused': return t('gone_id_reused');
    }
    const unreachable: never = reason;
    return unreachable;
  }

  const badge = $derived.by(() => {
    void $locale;
    const got = answer;
    if (got === null) return '';
    // M2 outranks the freshness verdict: a verdict about the wrong document
    // would be true of that document and false of the one on the header.
    if (mismatched) return t('source_wrong_document');
    return got.kind === 'gone' ? goneText(got.reason) : freshnessText(got.freshness);
  });
</script>

<div class="source-card" data-testid="source-body" data-pending={pending}>
  <h3 data-testid="source-header">{header}</h3>

  {#if failed}
    <p data-testid="source-failed">{failedLabel}</p>
  {:else if answer === null}
    <p data-testid="source-loading">{loadingLabel}</p>
  {:else}
    <p role="status" data-testid="freshness">{badge}</p>
    {#if excerpt !== null}
      {#if excerpt.hasMoreBefore}<p data-testid="more-before">…</p>{/if}
      {#each painted as block (block.blockId)}
        <p data-testid="source-block">{#each block.pieces as piece}{#if piece.mark}<mark data-testid="hl" data-primary={piece.primary ? 'true' : undefined}>{piece.text}</mark>{:else}{piece.text}{/if}{/each}</p>
      {/each}
      {#if excerpt.hasMoreAfter}<p data-testid="more-after">…</p>{/if}
    {/if}
  {/if}
</div>

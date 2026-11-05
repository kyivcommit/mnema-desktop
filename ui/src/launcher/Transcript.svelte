<script lang="ts">
  import Selection from './Selection.svelte';
  import { locale, t } from '../i18n';
  import { refusalText } from '../i18n/refusal';
  import type { AskCitation, Hit } from '../lib/ipc';
  import type { CardAnswer, LauncherState, Turn } from './state';
  import type { Refusal } from '../lib/ipc';

  // §5: the transcript is the conversation — a turn is its question, pushed to
  // the right edge, and its answer, left in the text column with no bubble.
  // Role is told by that alignment alone: no speaker labels ("You" /
  // "Assistant"), and no hint of one here to fall out of step with the sheet.
  let { turns = [], state, onSelected = () => {} }: {
    turns?: Turn[];
    state: LauncherState;
    // Reported upward for the file pane (Task 7): a pressed source card moves
    // the panel to its citation. `Selection` needs a listener today, and a turn
    // drawn on its own reports into nothing.
    onSelected?: (selection: AskCitation | Hit | null) => void;
  } = $props();

  // The answer on screen that is not a turn — a failed one — is drawn last with
  // its own question. A turn is drawn once, and identity is how it is
  // recognised (the launcher keeps these as `$state.raw`). A refusal is a turn
  // too (owner, 2026-11-05): the question a person typed stays in the
  // conversation. Not the every-folder-off one: the composer's live line says
  // that, and says it only while it is true.
  type Item = { query: string; key: object; answer: CardAnswer | null; refusal: Refusal | null };
  const items = $derived.by(() => {
    const drawn: Item[] = turns.map((t) => 'answer' in t
      ? { query: t.query, key: t.answer, answer: t.answer, refusal: null }
      : { query: t.query, key: t.refusal, answer: null, refusal: t.refusal });
    const isDrawn = (key: object) => drawn.some((d) => d.key === key);
    if ((state.kind === 'generated' || state.kind === 'citationsOnly') && !isDrawn(state.answer)) {
      drawn.push({ query: state.query, key: state.answer, answer: state.answer, refusal: null });
    } else if (state.kind === 'refused' && state.reason.kind !== 'noFoldersSearched' && !isDrawn(state.reason)) {
      drawn.push({ query: state.query, key: state.reason, answer: null, refusal: state.reason });
    }
    return drawn;
  });

  // The question in flight is drawn at once, with dots for the answer being
  // written, and scrolled into view: the Enter is seen to have landed. The dots
  // are decoration; a screen reader hears the live line below, which is always
  // in the page so its text changing is announced (a busy field is not).
  const pending = $derived(state.kind === 'inFlight' ? state.query : null);

  const refusalLine = (r: Refusal) => { void $locale; return refusalText(r.kind); };
  const waiting = $derived.by(() => { void $locale; return pending === null ? '' : t('answer_pending'); });
  let scroller: HTMLDivElement | null = null; // plain: `$state` clashes with the `state` prop
  // When the answer arrives, its question goes to the top of the view, so it
  // is read from the start (owner, 2026-10-22). Only an ask that drew a new
  // exchange moves the view; one that drew nothing new leaves it be.
  let drawnBefore: number | null = null;
  $effect(() => {
    const count = items.length;
    if (!scroller) return;
    if (pending !== null) {
      drawnBefore ??= count;
      scroller.scrollTop = scroller.scrollHeight;
      return;
    }
    if (drawnBefore === null) return;
    const grew = count > drawnBefore;
    drawnBefore = null;
    const last = scroller.lastElementChild;
    if (!grew || !last) return;
    scroller.scrollTop += last.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
  });
</script>

<div class="transcript" data-testid="transcript" bind:this={scroller}>
  <p class="transcript-live" aria-live="polite" data-testid="answer-pending">{waiting}</p>{#each items as item (item.key)}
    {#if item.answer !== null}
      <div class="exchange" data-testid="turn">
        <Selection answer={item.answer} query={item.query} {onSelected} />
      </div>
    {:else if item.refusal !== null}
      <div class="exchange" data-testid="refused-turn">
        <div class="umsg">{item.query}</div>
        <p class="refusal" role="status">{refusalLine(item.refusal)}</p>
      </div>
    {/if}
  {/each}{#if pending !== null}
    <div class="exchange" data-testid="pending-turn">
      <div class="umsg">{pending}</div>
      <div class="typing" aria-hidden="true"><span></span><span></span><span></span></div>
    </div>
  {/if}</div>

<script lang="ts">
  import Selection from './Selection.svelte';
  import { locale, t } from '../i18n';
  import type { AskCitation, Hit } from '../lib/ipc';
  import type { LauncherState, Turn } from './state';

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
  // recognised (the launcher keeps these as `$state.raw`).
  const items = $derived.by(() => {
    const drawn = turns.map((t) => ({ answer: t.answer, query: t.query }));
    if (
      (state.kind === 'generated' || state.kind === 'citationsOnly')
      && !drawn.some((d) => d.answer === state.answer)
    ) {
      drawn.push({ answer: state.answer, query: state.query });
    }
    return drawn;
  });

  // The question in flight is drawn at once, with dots for the answer being
  // written, and scrolled into view: the Enter is seen to have landed. The dots
  // are decoration; a screen reader hears the live line below, which is always
  // in the page so its text changing is announced (a busy field is not).
  const pending = $derived(state.kind === 'inFlight' ? state.query : null);
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
  <p class="transcript-live" aria-live="polite" data-testid="answer-pending">{waiting}</p>{#each items as item (item.answer)}
    <div class="exchange" data-testid="turn">
      <Selection answer={item.answer} query={item.query} {onSelected} />
    </div>
  {/each}{#if pending !== null}
    <div class="exchange" data-testid="pending-turn">
      <div class="umsg">{pending}</div>
      <div class="typing" aria-hidden="true"><span></span><span></span><span></span></div>
    </div>
  {/if}</div>

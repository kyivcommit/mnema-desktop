<script lang="ts">
  import { untrack } from 'svelte';
  import Answer from './Answer.svelte';
  import Passages from './Passages.svelte';
  import type { AskCitation, Hit } from '../lib/ipc';
  import { firstCard, type CardAnswer } from './state';

  // 🔴 Ruling AC — why this component exists at all. The selection lives HERE,
  // and the transcript keys this component by the answer it draws. A
  // `let selected = $state(null)` declared one level up would sit OUTSIDE that
  // block and survive it untouched: the children would remount still holding the
  // previous answer's citation, and the right card would keep painting the first
  // answer's excerpt under the second answer's heading — "text the file no
  // longer contains", which is the failure this PR exists to prevent. Keying the
  // component that OWNS the state makes the reset structural rather than
  // remembered, with no frame in which a request can go out for a citation
  // already off screen.
  //
  // 🔴 Ruling AJ — this is also the whole of state E's selection. A passage
  // click IS a citation click: same component, same `{#key}`, same state tag,
  // and both carry `documentId`/`chunkId`/`ord`/`text`, which is everything the
  // pane is opened from (`DocumentLocation`/`DocumentFocus`). A second
  // selection path would be a second copy of the reset rule above, and the two
  // would drift.
  let { answer, query, onSelected }: {
    answer: CardAnswer;
    query: string;
    onSelected: (selection: AskCitation | Hit | null) => void;
  } = $props();

  // Narrowed to two consts rather than branched on `answer.kind` in the markup,
  // so each child receives its own member of the union by type — Ruling AG's
  // point is that a component handed the wrong member renders nothing and says
  // nothing, and the compiler is the only thing that catches that.
  const generatedAnswer = $derived(answer.kind === 'generated' ? answer : null);
  const passagesAnswer = $derived(answer.kind === 'citationsOnly' ? answer : null);

  // Starts on the answer's first card — the one the panel opens on when the
  // answer arrives (`Launcher`), so pressing that card has nothing to move.
  // The initial value is captured on purpose: this component sits under
  // `{#key}`, so a new answer is a new component.
  let selected = $state<AskCitation | Hit | null>(untrack(() => firstCard(answer)));

  // The file pane is deliberately outside the key (`Transcript.svelte`) and
  // therefore cannot read `selected` from in here, so it is reported upward
  // when the person PRESSES a card — that is what moves the panel.
  //
  // 🔴 The mount run reports nothing. Nothing here draws a selected mark
  // (`Answer`/`Passages` take no `selected`), so a mount report's only effect
  // would be the panel — and the answer already opens it from `Launcher`.
  // It would also fire for a stored answer a Recents entry draws, where nothing
  // arrived and the pane the person was reading must stay.
  //
  // Only `selected` is tracked: the callback is a fresh closure whenever the
  // transcript re-derives its turns, and re-reporting on that would let an older turn's
  // effect run after a newer turn's and take the selection back.
  let mounted = false;
  $effect(() => {
    const value = selected;
    if (!mounted) { mounted = true; return; }
    untrack(() => onSelected(value));
  });
</script>

<div class="turn">
  {#if generatedAnswer !== null}
    <Answer answer={generatedAnswer} {query} onSelect={(citation) => (selected = citation)} />
  {:else if passagesAnswer !== null}
    <Passages answer={passagesAnswer} onSelect={(passage) => (selected = passage)} />
  {/if}
</div>

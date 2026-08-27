<script lang="ts">
  import { locale, t } from '../i18n';
  import Answer from './Answer.svelte';
  import Source from './Source.svelte';
  import type { AskAnswer, AskCitation, Hit } from '../lib/ipc';

  // 🔴 Ruling AC — why this component exists at all. The selection lives HERE,
  // and `Cards` wraps this component in `{#key state}`. A
  // `let selected = $state(null)` declared one level up would sit OUTSIDE that
  // block and survive it untouched: the children would remount still holding the
  // previous answer's citation, and the right card would keep painting the first
  // answer's excerpt under the second answer's heading — "text the file no
  // longer contains", which is the failure this PR exists to prevent. Keying the
  // component that OWNS the state makes the reset structural rather than
  // remembered, with no frame in which a request can go out for a citation
  // already off screen.
  let { answer, query, onSelected }: {
    answer: Extract<AskAnswer, { kind: 'generated' }>;
    query: string;
    onSelected: (selection: AskCitation | Hit | null) => void;
  } = $props();

  let selected = $state<AskCitation | Hit | null>(null);

  // The tree is deliberately outside the key (`Cards.svelte`) and therefore
  // cannot read `selected` from in here, so it is reported upward. This fires on
  // mount too — which is exactly what lets go of the left card's mark when a new
  // answer recreates this component.
  $effect(() => {
    onSelected(selected);
  });

  const answerLabel = $derived.by(() => { void $locale; return t('card_answer'); });
  const sourceLabel = $derived.by(() => { void $locale; return t('card_source'); });
</script>

<section data-testid="card-centre" aria-label={answerLabel}>
  <Answer {answer} {query} onSelect={(citation) => (selected = citation)} />
</section>

{#if selected !== null}
  <!-- §7: the source card does not exist until something is selected, and
       `Source` takes a non-nullable selection (`Source.svelte:83-86`) — so this
       guard is what the type asks for as well as what the mockup shows.
       `siblings` is the whole citation list: `Source` drops the clicked one and
       everything in another document itself (Decision 4, Ruling U), and that
       rule stays in one place. -->
  <section data-testid="card-source" aria-label={sourceLabel}>
    <Source {selected} siblings={answer.citations} />
  </section>
{/if}

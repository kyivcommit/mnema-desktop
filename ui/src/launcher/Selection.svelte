<script lang="ts">
  import { locale, t } from '../i18n';
  import Answer from './Answer.svelte';
  import Passages from './Passages.svelte';
  import Source from './Source.svelte';
  import type { AskAnswer, AskCitation, Hit } from '../lib/ipc';

  // The two answers that draw cards (Task 9). `refused` is not one of them and
  // the type says so, so a refusal cannot reach this component by accident.
  type CardAnswer = Extract<AskAnswer, { kind: 'generated' | 'citationsOnly' }>;

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
  //
  // 🔴 Ruling AJ — this is also the whole of state E's selection. A passage
  // click IS a citation click: same component, same `{#key}`, same state tag,
  // and `Source` already takes `AskCitation | Hit` (`Source.svelte:83-86`) so a
  // `Hit` needs no adaptation. A second selection path would be a second copy of
  // the reset rule above, and the two would drift.
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

  let selected = $state<AskCitation | Hit | null>(null);

  // The tree is deliberately outside the key (`Cards.svelte`) and therefore
  // cannot read `selected` from in here, so it is reported upward. This fires on
  // mount too — which is exactly what lets go of the left card's mark when a new
  // answer recreates this component.
  $effect(() => {
    onSelected(selected);
  });

  // 🔴 One `<section>`, two names. The centre card is one region, but it is not
  // one FACT: in state E it announced itself by the answer card's name, in both
  // locales, while the first sentence inside it said generation is unavailable.
  // That is the window stating what is not so — Ruling AF's failure — moved
  // down into the layer
  // where a person using a screen reader cannot see the contradiction and
  // correct for it. Only the NAME switches; the element, its `card-centre`
  // testid and the `{#key}` above it are untouched.
  //
  // The label stays HERE rather than moving into `Answer` and `Passages`: the
  // `<section>` belongs to this component, and splitting one region's identity
  // across two files would duplicate the `void $locale` pattern a third time —
  // the drift Rulings AG and AJ exist to stop.
  //
  // 🔴 Exhaustive, not a ternary, and for Ruling W's own reason one card over
  // (`Source.svelte`'s `freshnessText`/`goneText`): a default branch is how a
  // wrong verdict gets drawn for a variant nobody added a case for. A third
  // member of `CardAnswer` would compile against `x ? a : b`, render an empty
  // card — the markup below has `{#if}/{:else if}` and no `else` — and announce
  // it under the passages name. No test can prove this, because the third
  // member does not exist to test with; the proof is the compiler's. Add one to
  // `CardAnswer` and this line stops compiling.
  function labelFor(a: CardAnswer): string {
    switch (a.kind) {
      case 'generated': return t('card_answer');
      case 'citationsOnly': return t('card_passages');
    }
    const unreachable: never = a;
    return unreachable;
  }

  const centreLabel = $derived.by(() => { void $locale; return labelFor(answer); });
  const sourceLabel = $derived.by(() => { void $locale; return t('card_source'); });
</script>

<section data-testid="card-centre" aria-label={centreLabel}>
  {#if generatedAnswer !== null}
    <Answer answer={generatedAnswer} {query} onSelect={(citation) => (selected = citation)} />
  {:else if passagesAnswer !== null}
    <Passages answer={passagesAnswer} onSelect={(passage) => (selected = passage)} />
  {/if}
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

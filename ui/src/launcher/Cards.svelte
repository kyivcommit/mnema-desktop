<script lang="ts">
  import { locale, t } from '../i18n';
  import Selection from './Selection.svelte';
  import Source from './Source.svelte';
  import Tree from './Tree.svelte';
  import type { Snippet } from 'svelte';
  import type { AskCitation, Hit } from '../lib/ipc';
  import type { CardAnswer, Heat, LauncherState, Turn } from './state';

  // The prop is `state`; the LOCAL binding is renamed. Svelte reads `$state`
  // as a subscription to a local called `state` when one exists, so a component
  // that takes a `state` prop cannot declare reactive state of its own until
  // this binding is out of the way (`store_rune_conflict`).
  let { state: launcherState, query, left, right, heat, search, historyId = null, historyGen = 0, onOpenHistory, turns = [] }: {
    state: LauncherState;
    query: string;
    left: boolean;
    right: boolean;
    heat: Heat;
    // The conversation so far, oldest first; the answer in `state` joins it as the
    // last card when it is not one of them (a failed turn).
    turns?: Turn[];
    // Recents, forwarded to the tree unchanged (see `Tree.svelte`).
    historyId?: number | null;
    historyGen?: number;
    onOpenHistory?: (id: number) => void;
    // The search panel. It lives in the centre column with the results, so
    // that no card spans two grid rows (see `launcher.css`).
    search?: Snippet;
  } = $props();

  // The two ANSWER cards appear for the two states that HAVE an answer to show:
  // `generated` (state B) and `citationsOnly` (state E, Task 9). idle, inFlight,
  // refused and error draw none of them, matching the mockup — a refusal has no
  // passages and no prose, and the search line already carries its message.
  //
  // Narrowed rather than a bare boolean so `.answer` below is one of those two
  // members by type, not by assertion: `Selection` takes exactly that union and
  // hands each member to the component that can render it (Ruling AG).
  const answerState = $derived(
    launcherState.kind === 'generated' || launcherState.kind === 'citationsOnly'
      ? launcherState
      : null,
  );

  // 🔴 Controller rulings C1 and I-B: the TREE card stays up in every state but
  // `idle`. Its content is the INDEX, not the answer, so it has no reason to
  // depend on which answer came back — or on whether one came back at all — and
  // `runSearch` sets `inFlight` before EVERY ask (`Launcher.svelte:42`). Gating
  // it on `generated` unmounted it in the middle of every question, refetched
  // `list_tree` and snapped shut every folder the person had opened: measured
  // through the real launcher as `list_tree` = 2 and `aria-expanded="false"` on
  // the second question, and again on the third state — a refusal destroyed the
  // whole card. That is the outcome Ruling AC forbids, reached with no `{#key}`
  // anywhere.
  //
  // ONE condition, and it is grounded in the one thing §7's state table actually
  // says (`…interface-design.md:196-205`): row A is the only row whose "shows"
  // column carries the word "only" — only the search line. Row D names a
  // placeholder and a progress indication, which the search line no longer
  // draws (the input carries `aria-busy` instead); row F names a quiet
  // message; neither asks for
  // anything to be torn down. (The spec is in Ukrainian and the guard reads this
  // file, so the wording is glossed rather than quoted — `guard.test.ts:19-21`.)
  //
  // `error` is in deliberately: `askFailed` is an answer state and it is exactly
  // the moment a person retries, so losing their folders on the failure they are
  // retrying would be the same defect one gate over. `citationsOnly` is Task 9's
  // and this condition already covers it — a bonus, not a decision made here.
  //
  // 🔴 Ruling I-C — the known cost of taking `error` whole, stated rather than
  // discovered. `error` carries four reasons (`state.ts`). `askFailed` and the
  // two Recents ones (`historyUnreadable`, `historyFailed`) keep the answer on
  // screen the way `askFailed` does. `tooLong` comes from `checkQuery` BEFORE any
  // ask, so an over-long first Enter is `error` with no answer behind it. That Enter
  // mounts the tree, and mounting fires `list_tree`; in the cold launcher the
  // section is `hidden` below, so nothing is drawn. A blank Enter is not an
  // error at all (owner, 2026-10-08): it returns before the machine moves.
  //
  // Narrowing to `reason === 'askFailed'` was considered and rejected: an
  // over-long query typed from state B is ALSO an `error`, so a gate keyed on
  // the reason would tear the tree down when a person with three cards on
  // screen mistypes an Enter — C1's exact defect, reintroduced through the gate
  // that was widened to fix it. One condition, no state, cost declared.
  //
  // MOUNTING and SEEING are two decisions. Mounting is this condition alone;
  // seeing is the heat and the left switch, a `hidden` attribute on the section
  // below. A switch that
  // unmounted the tree would refetch it and shut every folder on the way back
  // on — C1 again, reached through a button.
  // A hot launcher may be idle (after «New chat»): the left panel stays, the rest is empty.
  const showTree = $derived(launcherState.kind !== 'idle' || heat === 'hot');

  // Every turn drawn in the centre, oldest first, each keyed by its own answer
  // so a turn already on screen is never remounted (and never loses its
  // selection) when a later one arrives. The answer on screen that is not a turn
  // (a failed one) comes last, with the echo as its question.
  const items = $derived.by(() => {
    const drawn = turns.map((t) => ({ answer: t.answer as CardAnswer, query: t.query }));
    if (answerState !== null && !drawn.some((d) => d.answer === answerState.answer)) {
      drawn.push({ answer: answerState.answer, query });
    }
    return drawn;
  });
  const showSource = $derived(heat === 'hot' && right);

  // What `Selection` reports up, tagged with the answer it belongs to. Read by
  // the tree and the source card, which live outside the keyed blocks and so
  // cannot read the selection from the component that owns it.
  //
  // 🔴 The tag is the `{#key}`'s defence applied to the one copy that cannot be
  // keyed: a `Selection` is destroyed by the `{#each}` below and never reports
  // `null` on its way out, so a plain mirror would keep a vanished answer's row
  // marked. A mark is trusted only while the answer that produced it is still
  // on screen.
  //
  // ⚠️ `$state.raw`, not `$state`, and it is load-bearing: plain `$state` DEEP
  // PROXIES whatever is assigned into it, so `===` against the answer is false
  // forever and the mark would never appear at all. Measured, not guessed.
  let reported = $state.raw<{ answer: CardAnswer; value: AskCitation | Hit | null } | null>(null);
  const selection = $derived(
    reported !== null && reported.value !== null && items.some((d) => d.answer === reported!.answer)
      ? { answer: reported.answer, value: reported.value }
      : null,
  );
  const selected = $derived(selection?.value ?? null);

  // 🔴 One `<section>`, two names. The centre card is one region, but it is not
  // one FACT: in state E it announced itself by the answer card's name, in both
  // locales, while the first sentence inside it said generation is unavailable.
  // That is the window stating what is not so — Ruling AF's failure — moved
  // down into the layer
  // where a person using a screen reader cannot see the contradiction and
  // correct for it. Only the NAME switches; the element, its `card-centre`
  // testid and the `{#key}` above it are untouched.
  //
  // The label lives with the `<section>`, which belongs to this component, and splitting one region's identity
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

  const centreLabel = $derived.by(() => { void $locale; return items.length === 0 ? '' : labelFor(items[items.length - 1].answer); });

  const treeLabel = $derived.by(() => { void $locale; return t('card_tree'); });
  const sourceLabel = $derived.by(() => { void $locale; return t('card_source'); });
</script>

<!-- 🔴 Ruling AC + C1: the tree is NOT keyed AND it is not gated on the answer.
     Both halves are needed — either one alone recreates it on every question,
     and this is the card whose whole purpose is browsing the cited file's folder
     neighbours (§7). -->
{#if showTree}
  <!-- `.col-side` sets no `display`, so the UA rule for `[hidden]` applies. -->
  <section
    class="float col-side"
    data-testid="card-tree"
    aria-label={treeLabel}
    hidden={!(heat === 'hot' && left)}>
    <Tree {selected} {historyId} {historyGen} {onOpenHistory} />
  </section>
{/if}

<!-- The centre column: the search panel above the results, one flex column in
     one grid track. The search panel is rendered outside the keyed block, so a
     new answer never remounts the input. -->
<div class="col-centre">
  {@render search?.()}
  {#if items.length > 0}
    <!-- One card for the whole conversation. Each turn is keyed by its own
         answer, on the component that owns the selection (Ruling AC) — see
         `Selection.svelte` for why a key around the cards alone resets
         nothing. A new answer is a new object, so a generated answer followed by
         a citations-only one recreates the selection just as two generated
         answers do. -->
    <section class="float results" data-testid="card-centre" aria-label={centreLabel}>
      {#each items as item (item.answer)}
        <Selection
          answer={item.answer}
          query={item.query}
          onSelected={(value) => (reported = { answer: item.answer, value })} />
      {/each}
    </section>
  {:else if heat === 'hot'}
    <!-- A column that is on screen with nothing to put in it is an opaque empty
         panel, not a hole in the window (owner, 2026-09-25): the window is as
         wide as its visible panels, and a transparent gap would show what is
         behind it. Hot only — the cold launcher is the search column alone. -->
    <section class="float results" data-testid="card-results-empty"></section>
  {/if}
</div>

{#if showSource}
  {#if selection !== null}
    <!-- §7: the source card needs a selection, and `Source` takes a
         non-nullable one, so this guard is what the type asks for as well as
         what the mockup shows. The answer's first card is preselected (it is
         reported on mount), so the card is normally drawn whenever the panel is
         on. `selected` is trusted only for the state on screen (see `reported`),
         and the key recreates `Source` for each answer. `siblings` is the whole
         citation list: `Source` drops the clicked one and everything in another
         document itself (Decision 4, Ruling U). -->
    {#key selection.answer}
      <section class="float doc" data-testid="card-source" aria-label={sourceLabel}>
        <Source selected={selection.value!} siblings={selection.answer.citations} />
      </section>
    {/key}
  {:else}
    <!-- Nothing to show, but the panel is on: an opaque empty one rather than a
         hole in the window. -->
    <section class="float doc" data-testid="card-source-empty"></section>
  {/if}
{/if}

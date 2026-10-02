<script lang="ts">
  import { locale, t } from '../i18n';
  import Selection from './Selection.svelte';
  import Source from './Source.svelte';
  import Tree from './Tree.svelte';
  import type { Snippet } from 'svelte';
  import type { AskCitation, Hit } from '../lib/ipc';
  import type { Heat, LauncherState } from './state';

  // The prop is `state`; the LOCAL binding is renamed. Svelte reads `$state`
  // as a subscription to a local called `state` when one exists, so a component
  // that takes a `state` prop cannot declare reactive state of its own until
  // this binding is out of the way (`store_rune_conflict`).
  let { state: launcherState, query, left, right, heat, search }: {
    state: LauncherState;
    query: string;
    left: boolean;
    right: boolean;
    heat: Heat;
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
  // discovered. `error` carries three reasons (`state.ts:25`) and only
  // `askFailed` is an answer state: `blank` and `tooLong` come from `checkQuery`
  // BEFORE any ask, so one Enter on an empty line is `error` with no answer
  // behind it. That Enter mounts the tree, and mounting fires `list_tree`; in
  // the cold launcher the section is `hidden` below, so nothing is drawn, and
  // `launcher-cold` (which sets `idle`) is the way back to the bare line.
  //
  // Narrowing to `reason === 'askFailed'` was considered and rejected: a blank
  // query typed from state B is ALSO `error: 'blank'`, so a gate keyed on the
  // reason would tear the tree down when a person with three cards on screen
  // mistypes an Enter — C1's exact defect, reintroduced through the gate that
  // was widened to fix it. One condition, no state, cost declared.
  //
  // MOUNTING and SEEING are two decisions. Mounting is this condition alone;
  // seeing is the heat and the left switch, a `hidden` attribute on the section
  // below. A switch that
  // unmounted the tree would refetch it and shut every folder on the way back
  // on — C1 again, reached through a button.
  const showTree = $derived(launcherState.kind !== 'idle');
  const showSource = $derived(heat === 'hot' && right);

  // What `Selection` reports up, tagged with the state it belongs to. Read by
  // the tree ONLY — the tree lives outside the keyed block, so it cannot read
  // the selection from the component that owns it.
  //
  // 🔴 The tag is the `{#key}`'s defence applied to the one copy that cannot be
  // keyed, and it is C1's other half: `Selection` is destroyed by the `{#if}`
  // above and never reports `null` on its way out, so a plain mirror would keep
  // the previous answer's row marked for the whole length of the next ask now
  // that the tree survives state D. A mark is trusted only while the exact state
  // that produced it is still on screen.
  //
  // ⚠️ `$state.raw`, not `$state`, and it is load-bearing: plain `$state` DEEP
  // PROXIES whatever is assigned into it, so `reported.state` came back as a
  // proxy OF `launcherState` and `===` was false forever — the tag would never
  // match and the tree's mark would never appear at all. Measured, not guessed.
  let reported = $state.raw<{ state: LauncherState; value: AskCitation | Hit | null } | null>(null);
  const selected = $derived(
    reported !== null && reported.state === launcherState ? reported.value : null,
  );

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
    <Tree {selected} />
  </section>
{/if}

<!-- The centre column: the search panel above the results, one flex column in
     one grid track. The search panel is rendered outside the keyed block, so a
     new answer never remounts the input. -->
<div class="col-centre">
  {@render search?.()}
  {#if answerState !== null}
    <!-- Only the answer card is keyed, and the key is on the component that
         owns the selection (Ruling AC) — see `Selection.svelte` for why a key
         around the cards alone resets nothing. The key is the STATE object, so
         a generated answer followed by a citations-only one recreates the
         selection just as two generated answers do. -->
    {#key answerState}
      <Selection
        answer={answerState.answer}
        {query}
        onSelected={(value) => (reported = { state: launcherState, value })} />
    {/key}
  {:else if heat === 'hot'}
    <!-- A column that is on screen with nothing to put in it is an opaque empty
         panel, not a hole in the window (owner, 2026-09-25): the window is as
         wide as its visible panels, and a transparent gap would show what is
         behind it. Hot only — the cold launcher is the search column alone. -->
    <section class="float results" data-testid="card-results-empty"></section>
  {/if}
</div>

{#if showSource}
  {#if answerState !== null && selected !== null}
    <!-- §7: the source card needs a selection, and `Source` takes a
         non-nullable one, so this guard is what the type asks for as well as
         what the mockup shows. The answer's first card is preselected (it is
         reported on mount), so the card is normally drawn whenever the panel is
         on. `selected` is trusted only for the state on screen (see `reported`),
         and the key recreates `Source` for each answer. `siblings` is the whole
         citation list: `Source` drops the clicked one and everything in another
         document itself (Decision 4, Ruling U). -->
    {#key answerState}
      <section class="float doc" data-testid="card-source" aria-label={sourceLabel}>
        <Source {selected} siblings={answerState.answer.citations} />
      </section>
    {/key}
  {:else}
    <!-- Nothing to show, but the panel is on: an opaque empty one rather than a
         hole in the window. -->
    <section class="float doc" data-testid="card-source-empty"></section>
  {/if}
{/if}

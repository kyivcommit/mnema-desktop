<script lang="ts">
  import { locale, t } from '../i18n';
  import Selection from './Selection.svelte';
  import Tree from './Tree.svelte';
  import type { AskCitation, Hit } from '../lib/ipc';
  import type { LauncherState } from './state';

  // The prop is `state`; the LOCAL binding is renamed. Svelte reads `$state`
  // as a subscription to a local called `state` when one exists, so a component
  // that takes a `state` prop cannot declare reactive state of its own until
  // this binding is out of the way (`store_rune_conflict`).
  let { state: launcherState, query }: { state: LauncherState; query: string } = $props();

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
  // column carries the word "only" — only the search line. Row D names spinner,
  // placeholder and phases; row F names a quiet message; neither asks for
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
  // BEFORE any ask. So one Enter on an empty line, as the first thing a person
  // does, draws this card and fires `list_tree` — and `idle` is assigned in
  // exactly one place, the initial value at `Launcher.svelte:14`, with no path
  // back, while §6 keeps state across a hide (`…interface-design.md:186`, not §7.3, which is
  // dismissal mechanics only). **State A's bareness therefore
  // ends on a stray Enter and does not come back that session.** That is the
  // price, and it is accepted, not overlooked.
  //
  // Narrowing to `reason === 'askFailed'` was considered and rejected: a blank
  // query typed from state B is ALSO `error: 'blank'`, so a gate keyed on the
  // reason would tear the tree down when a person with three cards on screen
  // mistypes an Enter — C1's exact defect, reintroduced through the gate that
  // was widened to fix it. A "has ever shown cards" flag buys the bareness back
  // at the price of another reset to get wrong, and this task has already spent
  // two rounds on one. One condition, no state, cost declared.
  const showTree = $derived(launcherState.kind !== 'idle');

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
</script>

<!-- 🔴 Ruling AC + C1: the tree is NOT keyed AND it is not gated on the answer.
     Both halves are needed — either one alone recreates it on every question,
     and this is the card whose whole purpose is browsing the cited file's folder
     neighbours (§7). -->
{#if showTree}
  <section data-testid="card-tree" aria-label={treeLabel}>
    <Tree {selected} />
  </section>
{/if}

{#if answerState !== null}
  <!-- Only the answer-and-source pair is keyed, and the key is on the component
       that owns the selection (Ruling AC) — see `Selection.svelte` for why a key
       around the cards alone resets nothing. The key is the STATE object, so a
       generated answer followed by a citations-only one recreates the selection
       just as two generated answers do. -->
  {#key answerState}
    <Selection
      answer={answerState.answer}
      {query}
      onSelected={(value) => (reported = { state: launcherState, value })} />
  {/key}
{/if}

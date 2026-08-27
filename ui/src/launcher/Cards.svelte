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

  // The two ANSWER cards appear only for a generated answer (state B) — idle,
  // citations-only, refused and error draw none of the three, matching the
  // mockup. citationsOnly (state E) is Task 9's; it stays card-less here (task
  // brief header). Narrowed rather than a bare boolean so `.answer` below is the
  // generated member by type, not by assertion.
  const generatedState = $derived(launcherState.kind === 'generated' ? launcherState : null);

  // 🔴 Controller ruling C1: the TREE card also stays up through state D. Its
  // content is the INDEX, not the answer, so it has no reason to depend on which
  // answer is on screen — and `runSearch` sets `inFlight` before EVERY ask
  // (`Launcher.svelte:42`), so gating it on `generated` alone unmounts it in the
  // middle of every question, refetches `list_tree` and snaps shut every folder
  // the person opened. That is the outcome Ruling AC forbids, reached with no
  // `{#key}` anywhere: measured through the real launcher as `list_tree` = 2 and
  // a hand-opened folder back to `aria-expanded="false"` on the second question.
  // §7's state D row describes the SEARCH LINE — spinner, placeholder, the query
  // staying in the box — and never asks for the cards to be torn down.
  const showTree = $derived(generatedState !== null || launcherState.kind === 'inFlight');

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

{#if generatedState !== null}
  <!-- Only the answer-and-source pair is keyed, and the key is on the component
       that owns the selection (Ruling AC) — see `Selection.svelte` for why a key
       around the cards alone resets nothing. -->
  {#key generatedState}
    <Selection
      answer={generatedState.answer}
      {query}
      onSelected={(value) => (reported = { state: launcherState, value })} />
  {/key}
{/if}

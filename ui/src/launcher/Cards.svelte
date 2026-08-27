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

  // Cards only appear for a generated answer (state B) — every other state
  // (idle, in flight, citations-only, refused, error) draws none of the
  // three, matching the mockup. citationsOnly (state E) is Task 9's; it
  // stays card-less here (task brief header). Narrowed rather than a bare
  // boolean so `state.answer` below is the generated member by type, not by
  // assertion.
  const generatedState = $derived(launcherState.kind === 'generated' ? launcherState : null);

  // What `Selection` reports up. Read by the tree ONLY — the tree is the one
  // card that must not be recreated when the answer changes, so it cannot read
  // the selection from inside the keyed block that owns it.
  let selected = $state<AskCitation | Hit | null>(null);

  const treeLabel = $derived.by(() => { void $locale; return t('card_tree'); });
</script>

{#if generatedState !== null}
  <!-- 🔴 Ruling AC: the tree is NOT keyed. `state` changes twice per question
       (`Launcher.svelte:41,44`), so a keyed tree would refetch `list_tree` twice
       and snap shut every folder the person opened — in the card whose whole
       purpose is browsing the cited file's folder neighbours (§7). -->
  <section data-testid="card-tree" aria-label={treeLabel}>
    <Tree {selected} />
  </section>

  <!-- Only the answer-and-source pair is keyed, and the key is on the component
       that owns the selection (Ruling AC) — see `Selection.svelte` for why a key
       around the cards alone resets nothing. -->
  {#key generatedState}
    <Selection answer={generatedState.answer} {query} onSelected={(s) => (selected = s)} />
  {/key}
{/if}

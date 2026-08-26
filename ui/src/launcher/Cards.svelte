<script lang="ts">
  import { locale, t } from '../i18n';
  import type { LauncherState } from './state';

  // `query` is unused here (Ruling C): it becomes a real prop the moment
  // Task 6 mounts `Answer` inside the centre card, and taking it now keeps
  // this signature stable across that later task.
  let { state, query: _query }: { state: LauncherState; query: string } = $props();

  // Cards only appear for a generated answer (state B) — every other state
  // (idle, in flight, citations-only, refused, error) draws none of the
  // three, matching the mockup. citationsOnly (state E) is Task 9's; it
  // stays card-less here (task brief header).
  const showCards = $derived(state.kind === 'generated');

  const treeLabel = $derived.by(() => { void $locale; return t('card_tree'); });
  const answerLabel = $derived.by(() => { void $locale; return t('card_answer'); });
  const sourceLabel = $derived.by(() => { void $locale; return t('card_source'); });
</script>

{#if showCards}
  <section data-testid="card-tree" aria-label={treeLabel}></section>
  <section data-testid="card-centre" aria-label={answerLabel}></section>
{/if}

<script lang="ts">
  import { locale, t } from '../i18n';
  import { citationLabel } from '../i18n/label';
  import type { AskAnswer, Hit } from '../lib/ipc';

  // 🔴 Ruling AG — why this is a component of its own rather than a widened
  // `Answer`. Task 6's review measured the alternative: handed a `citationsOnly`
  // answer, an `Answer` whose prop was the whole union rendered its two headings
  // over an empty body, silently and with no throw, because `answer.answer` and
  // `answer.citations[n].anchor` simply are not there. State E's centre card is
  // a different thing — a banner and ranked passages, no prose and no anchors —
  // so it is a different component, and `Answer`'s prop stays narrowed to the
  // generated member.
  let { answer, onSelect }: {
    answer: Extract<AskAnswer, { kind: 'citationsOnly' }>;
    onSelect: (passage: Hit) => void;
  } = $props();

  // 🔴 Ruling AF — the banner names NO cause, and that is a decision, not an
  // omission. This state is opened whenever chat readiness is not `Ready`
  // (`bridge.rs:536-540`), and readiness has three non-ready variants
  // (`bridge.rs:293-302`) of which the wire shape (`bridge.rs:476-480`) carries
  // none. A banner saying "no provider key" would be false in two cases out of
  // three, and this card cannot tell which one it is in. `content` is not the
  // missing fact either: `ContentArmReport` reports the content SEARCH arm,
  // filled before readiness is ever consulted. The wire would need the readiness
  // reason before any card could name one — a PR 7 question, since that is where
  // provider configuration lives.
  //
  // 🔴 Review I1 — and this is why the banner has two forms rather than one.
  // Its second clause is a promise about what follows ("these are the
  // passages…", and the Ukrainian form promises them just as plainly), and with
  // zero hits it was printed directly above the sentence saying there are none:
  // a card denying, one line down,
  // what it had just asserted. That is this card's own failure mode turned on
  // itself, and `toContain` could not see it. The empty form drops the clause it
  // cannot keep; it does not qualify or soften it.
  const banner = $derived.by(() => {
    void $locale;
    return t(answer.citations.length === 0 ? 'citations_only_banner_empty' : 'citations_only_banner');
  });

  // Ruling AK: zero passages is an ANSWER — the search ran and found nothing —
  // so it gets a sentence rather than an empty list. Its own sentence: not the
  // tree's "nothing is indexed" and not the source card's failure text.
  const emptyText = $derived.by(() => { void $locale; return t('citations_only_empty'); });

  // 🔴 Ruling AH: the rank is a NEUTRAL ORDINAL. A `Hit` has no `anchor`
  // (`ipc.ts:33-42`) — the anchor is the model's own reference into its prose,
  // and there is no prose here — so the rank is this row's position and nothing
  // about the passage is derivable from it, nor it from the passage. That is why
  // the testid is `rank-N` and deliberately not `preview-N`: they are different
  // things and one must never be mistaken for the other.
  //
  // Ruling D / D130: `citationLabel()` reads the locale store at call time
  // through `t()` and `formatLocator()` but is not reactive on its own, so the
  // whole list is rebuilt inside one `$derived.by` that reads `$locale` — the
  // house pattern (`Answer.svelte:40-43`, `Arms.svelte:11-12`).
  const ranks = $derived.by(() => {
    void $locale;
    return answer.citations.map((passage, index) => ({
      passage,
      rank: index + 1,
      label: citationLabel(passage),
    }));
  });
</script>

<div class="passages-card">
  <p role="status" data-testid="citations-banner">{banner}</p>

  {#if ranks.length === 0}
    <p data-testid="citations-empty">{emptyText}</p>
  {:else}
    <ul>
      {#each ranks as { passage, rank, label } (rank)}
        <li>
          <button type="button" data-testid={`rank-${rank}`} onclick={() => onSelect(passage)}>
            <!-- `passage-label`, NOT `rank-label`: the rank testids are queried
                 as a namespace (`getAllByTestId(/^rank-/)`) and a second id in
                 it would be counted as a row. -->
            <span data-testid="passage-label">{label}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

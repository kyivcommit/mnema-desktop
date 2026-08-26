<script lang="ts">
  import { locale, t } from '../i18n';
  import { formatLocator } from '../i18n/locator';
  import { splitAnchors } from '../lib/anchors';
  import type { AskAnswer, AskCitation } from '../lib/ipc';

  // Narrowed to the `generated` member (review I1): `LauncherState.generated`
  // (`state.ts:22`) already carries this exact type, and Task 8b mounts
  // `Answer` from that state — the compiler should enforce the pairing, not
  // let a wrong-state mount render two headings over an empty card. Callers
  // holding a wider `AskAnswer` narrow the way `Cards.test.ts:23` already
  // does, through `stateFromAnswer`.
  let { answer, query, onSelect }: {
    answer: Extract<AskAnswer, { kind: 'generated' }>;
    query: string;
    onSelect: (citation: AskCitation) => void;
  } = $props();

  const answerHeading = $derived.by(() => { void $locale; return t('answer_heading'); });
  const citationsHeading = $derived.by(() => { void $locale; return t('citations_heading'); });

  // `known` must come from the citations' own anchor values (Decision 5) —
  // an anchor number that has no matching citation stays literal text.
  const known = $derived(new Set(answer.citations.map((c) => c.anchor)));
  const segments = $derived(splitAnchors(answer.answer, known));

  function citationFor(n: number): AskCitation {
    const found = answer.citations.find((c) => c.anchor === n);
    if (!found) throw new Error(`Answer: no citation for anchor ${n}`);
    return found;
  }

  // Ruling D: `t()` and `formatLocator()` both read the locale store at call
  // time but neither is reactive on its own, so the whole preview list is
  // rebuilt inside one $derived.by that reads $locale — the house pattern
  // (Arms.svelte:11-12, Cards.svelte:16-18).
  const previews = $derived.by(() => {
    void $locale;
    return answer.citations.map((c) => {
      const parts = [c.relativePath, formatLocator(c.coordinate)].filter(
        (p): p is string => !!p,
      );
      const label = parts.length > 0 ? parts.join(' · ') : t('no_path_on_disk');
      return { citation: c, label };
    });
  });
</script>

<div class="answer-card">
  <div data-testid="query-echo">{query}</div>

  <h3>{answerHeading}</h3>
  <p data-testid="answer-body">{#each segments as segment}{#if segment.kind === 'text'}{segment.text}{:else}<button type="button" onclick={() => onSelect(citationFor(segment.n))}>[{segment.n}]</button>{/if}{/each}</p>

  <h3>{citationsHeading}</h3>
  <ul>
    {#each previews as { citation, label } (citation.anchor)}
      <li>
        <button
          type="button"
          data-testid={`preview-${citation.anchor}`}
          onclick={() => onSelect(citation)}
        >
          <span data-testid="preview-label">{label}</span>
        </button>
      </li>
    {/each}
  </ul>
</div>

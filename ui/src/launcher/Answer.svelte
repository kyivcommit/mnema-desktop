<script lang="ts">
  import { locale, t } from '../i18n';
  import { formatLocator } from '../i18n/locator';
  import { splitAnchors } from '../lib/anchors';
  import type { AskAnswer, AskCitation } from '../lib/ipc';

  // `answer` is typed as the full `AskAnswer` union — not narrowed to the
  // `generated` member — because the fixtures it is fed from (`lib/fixtures.ts`)
  // are themselves declared `AskAnswer`, and this component (Task 6) is only
  // ever mounted for state B (kind `generated`); Cards.svelte does that
  // narrowing at the call site in Task 8b.
  let { answer, query, onSelect }: {
    answer: AskAnswer;
    query: string;
    onSelect: (citation: AskCitation) => void;
  } = $props();

  const answerHeading = $derived.by(() => { void $locale; return t('answer_heading'); });
  const citationsHeading = $derived.by(() => { void $locale; return t('citations_heading'); });

  const citations = $derived(answer.kind === 'generated' ? answer.citations : []);
  const answerText = $derived(answer.kind === 'generated' ? answer.answer : '');

  // `known` must come from the citations' own anchor values (Decision 5) —
  // an anchor number that has no matching citation stays literal text.
  const known = $derived(new Set(citations.map((c) => c.anchor)));
  const segments = $derived(splitAnchors(answerText, known));

  function citationFor(n: number): AskCitation {
    const found = citations.find((c) => c.anchor === n);
    if (!found) throw new Error(`Answer: no citation for anchor ${n}`);
    return found;
  }

  // Ruling D: `t()` and `formatLocator()` both read the locale store at call
  // time but neither is reactive on its own, so the whole preview list is
  // rebuilt inside one $derived.by that reads $locale — the house pattern
  // (Arms.svelte:11-12, Cards.svelte:16-18).
  const previews = $derived.by(() => {
    void $locale;
    return citations.map((c) => {
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

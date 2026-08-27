<script lang="ts">
  import { locale, t } from '../i18n';
  import { citationLabel } from '../i18n/label';
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

  // Ruling D: `citationLabel()` reads the locale store at call time (through
  // `t()` and `formatLocator()`) but is not reactive on its own, so the whole
  // preview list is rebuilt inside one $derived.by that reads $locale — the
  // house pattern (Arms.svelte:11-12, Cards.svelte:16-18).
  // Ruling S: the rule itself lives in `i18n/label.ts` and `Source.svelte`
  // calls the same function — one rule, one place. What stays here is the
  // reactivity, which is per-component.
  const previews = $derived.by(() => {
    void $locale;
    return answer.citations.map((c) => ({ citation: c, label: citationLabel(c) }));
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
          <!-- 🔴 Owner review on PR #24, P2. This preview used to render the
               label ALONE, so the card listing an answer's citations was a
               list of file paths: a person reading `[7]` in the prose above
               found nothing in the list carrying that number, and nothing of
               the passage the sentence rests on. Three parts, in reading
               order: which anchor this is, what it says, where it came from.

               The anchor is bracketed to match the form it takes in the prose
               (`[{segment.n}]` above) — the two are the same reference and a
               person has to be able to pair them by sight.

               `citation-anchor`/`citation-text`/`citation-label`, NOT
               `preview-*`: the preview testids are queried as a namespace
               (`queryAllByTestId(/^preview-/)`) and a second id inside it is
               counted as a preview row. Held by ONE test — `the preview
               namespace holds one id per citation and nothing else`,
               `Answer.test.ts` — and by nothing else; the same rule
               `Passages.svelte` follows for its own row. -->
          <span data-testid="citation-anchor">[{citation.anchor}]</span>
          <span data-testid="citation-text">{citation.text}</span>
          <span data-testid="citation-label">{label}</span>
        </button>
      </li>
    {/each}
  </ul>
</div>

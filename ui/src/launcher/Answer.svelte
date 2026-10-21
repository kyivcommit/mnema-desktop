<script lang="ts">
  import { locale, t } from '../i18n';
  import { citationLabel } from '../i18n/label';
  import { splitAnchors } from '../lib/anchors';
  import type { AskAnswer, AskCitation } from '../lib/ipc';
  import { renderMarkdown, type Span } from '../lib/markdown';

  // Narrowed to the `generated` member (review I1): `LauncherState.generated`
  // (`state.ts:22`) already carries this exact type, and Task 8b mounts
  // `Answer` from that state — the compiler should enforce the pairing, not
  // let a wrong-state mount render two headings over an empty card. Callers
  // holding a wider `AskAnswer` narrow the way the transcript's turns already
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
  const viaText = $derived.by(() => { void $locale; return t(answer.via === 'conversation' ? 'answer_via_conversation' : 'answer_via_search'); });

  const known = $derived(new Set(answer.citations.map((c) => c.anchor)));
  // The markdown layer is pure and knows nothing of citations; the anchors are
  // split out of each span below, so `[n]` works inside a paragraph, a list
  // item, a heading and a fenced block alike.
  const blocks = $derived(renderMarkdown(answer.answer));

  function citationFor(n: number): AskCitation {
    const found = answer.citations.find((c) => c.anchor === n);
    if (!found) throw new Error(`Answer: no citation for anchor ${n}`);
    return found;
  }

  // Ruling D: `citationLabel()` reads the locale store at call time (through
  // `t()` and `formatLocator()`) but is not reactive on its own, so the whole
  // preview list is rebuilt inside one $derived.by that reads $locale — the
  // house pattern (Arms.svelte:11-12, Shell.svelte).
  // Ruling S: the rule itself lives in `i18n/label.ts` and `Passages.svelte`
  // calls the same function — one rule, one place. What stays here is the
  // reactivity, which is per-component.
  const previews = $derived.by(() => {
    void $locale;
    return answer.citations.map((c) => ({ citation: c, label: citationLabel(c) }));
  });
</script>

<div class="answer-card">
  <div class="umsg" data-testid="query-echo">{query}</div>

  <h3>{answerHeading}</h3>
  <span class="via" data-testid="answer-via">{viaText}</span>
  <div class="answer-body" data-testid="answer-body">
    {#each blocks as block}
      {#if block.kind === 'code'}<pre>{@render anchors(block.text)}</pre>
      {:else if block.kind === 'h'}<h4>{@render inline(block.spans)}</h4>
      {:else if block.kind === 'p'}<p>{@render inline(block.spans)}</p>
      {:else}<svelte:element this={block.kind} start={block.kind === 'ol' ? block.start : undefined}>{#each block.items as item}<li>{@render inline(item)}</li>{/each}</svelte:element>
      {/if}
    {/each}
  </div>

  {#if previews.length > 0}
    <h3>{citationsHeading}</h3>
    <ul>
      {#each previews as { citation, label } (citation.anchor)}
        <li>
          <button
            type="button"
            class="prev"
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
                 (`[{segment.n}]` in the `anchors` snippet below) — the two are the
                 same reference and a person has to be able to pair them by sight.

                 `citation-anchor`/`citation-text`/`citation-label`, NOT
                 `preview-*`: the preview testids are queried as a namespace
                 (`queryAllByTestId(/^preview-/)`) and a second id inside it is
                 counted as a preview row. Held by ONE test — `the preview
                 namespace holds one id per citation and nothing else`,
                 `Answer.test.ts` — and by nothing else; the same rule
                 `Passages.svelte` follows for its own row. -->
            <span class="tag" data-testid="citation-anchor">[{citation.anchor}]</span>
            <span class="q" data-testid="citation-text">{citation.text}</span>
            <span class="path" data-testid="citation-label">{label}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

{#snippet inline(spans: Span[])}
  {#each spans as span}{#if span.kind === 'strong'}<strong>{@render anchors(span.text)}</strong>{:else if span.kind === 'code'}<code>{@render anchors(span.text)}</code>{:else}{@render anchors(span.text)}{/if}{/each}
{/snippet}

{#snippet anchors(text: string)}
  {#each splitAnchors(text, known) as segment}{#if segment.kind === 'text'}{segment.text}{:else}<button type="button" onclick={() => onSelect(citationFor(segment.n))}>[{segment.n}]</button>{/if}{/each}
{/snippet}


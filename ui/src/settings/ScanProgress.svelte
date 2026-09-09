<script lang="ts">
  import { locale, t } from '../i18n';
  import type { Phase } from '../lib/ipc';
  import { phaseLabel, progressShape } from './jobs';

  // Task 5 — the running phase, drawn once so `JobStrip.svelte` (the bottom
  // disclosure) and `Scanning.svelte` (§9.3) can show the SAME projection of
  // the SAME snapshot rather than two components each answering "what is this
  // phase doing" on their own. A pure prop, on purpose: no store subscription,
  // no IPC, no controller — both callers already hold their own subscription
  // to `jobs.state` and hand down the one `phase` they read from it, and a
  // second reader here would be a second place for the two to disagree.
  let { phase }: { phase: Phase } = $props();

  // The counts of the phase that has any. `removing` carries none at all and
  // `other` is a job nobody asked for, so there is nothing to draw for either
  // — not a zero, which would read as a run that has done nothing.
  const counts = $derived(phase.kind === 'reading' || phase.kind === 'embedding' ? phase.counts : null);

  // A fresh embedding pass reporting zero of zero reads as "nothing to do"
  // while a run is genuinely under way — the same trap `progressShape`'s own
  // "countingUp" shape exists for on a reading pass, except an embedding pass
  // has no such shape of its own to fall into, so it gets a sentence instead
  // of a line of counts.
  const embedStartingZero = $derived(
    phase.kind === 'embedding' && phase.counts.total === 0 && phase.counts.done === 0,
  );

  // `phaseLabel` (`jobs.ts`) decides WHICH sentence, once, for every reader of
  // a `Phase`; this only formats it. Reading also carries its own
  // rootIndex/rootCount/rootPath inside that very sentence
  // (`indexing_reading_root`), so nothing here has to draw them a second time.
  const label = $derived.by(() => {
    void $locale;
    const l = phaseLabel(phase);
    return t(l.key, l.params);
  });

  const countsLabel = $derived.by(() => {
    void $locale;
    if (counts === null || embedStartingZero) return null;
    const shape = progressShape(counts);
    const common = { done: counts.done, skipped: counts.skipped, refused: counts.refused };
    return shape.kind === 'ratio'
      ? t('indexing_counts_ratio', { ...common, total: shape.total })
      : t('indexing_counts_counting', common);
  });

  // Takes the counts line's own place, for the one shape above — never beside
  // it.
  const embedStartingLabel = $derived.by(() => {
    void $locale;
    return embedStartingZero ? t('indexing_embed_starting_zero') : null;
  });

  // Drawn only while a reading phase is running and has actually met the
  // lock. The SAME catalogue key answers `scan.lastReading.contended` once
  // the pass has ended (`JobStrip.svelte`'s own `readingBlock`) — the fact it
  // explains is the same fact either way.
  const contendedLabel = $derived.by(() => {
    void $locale;
    if (counts === null || counts.contended === 0) return null;
    return t('indexing_counts_contended');
  });

  const etaLabel = $derived.by(() => {
    void $locale;
    if (counts === null) return null;
    // Not `seconds ? …` — zero seconds left is a number, and the nullish
    // check is the one this field's `Option<u64>` actually asks for.
    return counts.secondsLeft === null
      ? t('indexing_eta_unknown')
      : t('indexing_eta', { seconds: counts.secondsLeft });
  });
</script>

<p data-testid="indexing-pass">{label}</p>
{#if phase.kind === 'reading' || phase.kind === 'embedding'}
  {@const shape = progressShape(phase.counts)}
  <progress
    aria-label={label}
    max={shape.kind === 'ratio' ? shape.total : undefined}
    value={shape.kind === 'ratio' ? shape.done : undefined}
  ></progress>
{/if}
{#if embedStartingLabel}
  <p data-testid="indexing-counts">{embedStartingLabel}</p>
{:else if countsLabel}
  <p data-testid="indexing-counts">{countsLabel}</p>
{/if}
{#if contendedLabel}<p data-testid="indexing-contended">{contendedLabel}</p>{/if}
{#if etaLabel}<p data-testid="indexing-eta">{etaLabel}</p>{/if}

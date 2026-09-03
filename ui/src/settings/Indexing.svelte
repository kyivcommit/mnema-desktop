<script lang="ts">
  import { locale, t } from '../i18n';
  import type { Key } from '../i18n/catalog';
  import type { Frozen, FrozenReason } from '../lib/ipc';
  import { progressShape, type Ending, type JobController, type OutcomeKind } from './jobs';

  // §9.2 / Task 8 — the minimum indexing surface: one line saying what is
  // happening, what it ended as, and a control to stop it.
  //
  // The controller is a PROP, not something this component builds: it is
  // created once by `Settings.svelte`, above every section, because the channel
  // a job reports on belongs to whoever started it (`bridge.rs`). A controller
  // built here would die with the section and take the counters and the Cancel
  // button with it — and `cancel_job` needs no channel at all, so that Cancel
  // would be lost for nothing.
  let { jobs }: { jobs: JobController } = $props();
  // Read once, on purpose: the controller is created above this component and
  // its identity never changes for the life of the window, which is the whole
  // point of it living there. `$jobState` is then ordinary store
  // auto-subscription.
  // svelte-ignore state_referenced_locally
  const jobState = jobs.state;

  // A `Record` over the outcome kinds, not a `switch` with a default arm: a
  // default that draws "completed" for an unmatched state is exactly how a
  // failed pass reads as a finished one, and a `Record` makes a new kind a
  // compile error instead.
  //
  // The four kinds after `failed` get sentences of their own because they are
  // not malfunctions — `job.rs` says reporting them as `failed` tells a person
  // something broke when instead a folder is unreadable, an exclusion rule did
  // not take, or a volume may have gone missing.
  const WALK_ENDED: Record<OutcomeKind, Key> = {
    completed: 'indexing_walk_ended_completed',
    partlyRead: 'indexing_walk_ended_partly_read',
    cancelled: 'indexing_walk_ended_cancelled',
    failed: 'indexing_walk_ended_failed',
    brokenWorker: 'indexing_walk_ended_broken_worker',
    rulesNotApplied: 'indexing_walk_ended_rules_not_applied',
    rootUnavailable: 'indexing_walk_ended_root_unavailable',
    volumeMissing: 'indexing_walk_ended_volume_missing',
  };

  // The embedding pass has its own table, and every sentence in it is about the
  // WHOLE index: the pass takes no root (`embed_job.rs`), so a sentence
  // borrowed from the walk would promise it embedded only the folder that was
  // pressed. `walk_job.rs` is the only writer of the last four kinds, so they
  // cannot arrive here — but they still get a sentence, carrying the state's
  // own name, rather than a branch that quietly draws one of the three real
  // ones.
  const EMBED_ENDED: Record<OutcomeKind, Key> = {
    completed: 'indexing_embed_ended_completed',
    cancelled: 'indexing_embed_ended_cancelled',
    failed: 'indexing_embed_ended_failed',
    partlyRead: 'indexing_embed_ended_unexpected',
    brokenWorker: 'indexing_embed_ended_unexpected',
    rulesNotApplied: 'indexing_embed_ended_unexpected',
    rootUnavailable: 'indexing_embed_ended_unexpected',
    volumeMissing: 'indexing_embed_ended_unexpected',
  };

  const FROZEN_WHY: Record<FrozenReason, Key> = {
    symlinkedSubtree: 'indexing_frozen_symlinked_subtree',
    emptyDirectory: 'indexing_frozen_empty_directory',
    unreadableDirectory: 'indexing_frozen_unreadable_directory',
  };

  function outcomeSentence(table: Record<OutcomeKind, Key>, ending: Ending): string {
    return t(table[ending.outcome.kind], { reason: ending.outcome.kind });
  }

  // `message` is shown, not dropped: a broken pool, a missing worker binary and
  // a panic all arrive as `failed`, and this field is the only thing that tells
  // them apart (`job.rs`). It is `Option<String>` on the wire, so its absence
  // is a shape this has to survive rather than a case that cannot happen.
  function failureMessage(ending: Ending): string | null {
    const outcome = ending.outcome;
    if (outcome.kind !== 'failed' || outcome.message === null) return null;
    return t('indexing_failure_message', { message: outcome.message });
  }

  function frozenRows(frozen: Frozen[]) {
    return frozen.map((f) => ({
      prefix: f.prefix,
      text: t('indexing_frozen_row', { prefix: f.prefix, why: t(FROZEN_WHY[f.reason]) }),
    }));
  }

  const phase = $derived($jobState.phase);
  const walk = $derived($jobState.walk);
  const note = $derived($jobState.note);

  // Nothing to say, nothing on screen. A strip that is always there, saying it
  // is idle, is noise on a window somebody opened to change a model.
  const anything = $derived(phase.kind !== 'idle' || walk !== null || note !== null);

  // Offered for every phase in which a job may still be running — including the
  // one this window has no channel for, because `cancel_job` needs none either.
  const cancellable = $derived(
    phase.kind === 'starting' || phase.kind === 'running' || phase.kind === 'runningUnobserved',
  );

  const passLabel = $derived.by(() => {
    void $locale;
    if (phase.kind === 'starting') {
      return t(phase.pass === 'walk' ? 'indexing_walk_starting' : 'indexing_embed_starting');
    }
    if (phase.kind === 'running') {
      return t(phase.pass === 'walk' ? 'indexing_walk_running' : 'indexing_embed_running');
    }
    return null;
  });

  const countsLabel = $derived.by(() => {
    void $locale;
    if (phase.kind !== 'running') return null;
    const counts = phase.counts;
    const shape = progressShape(counts);
    const common = { done: counts.done, skipped: counts.skipped, refused: counts.refused };
    return shape.kind === 'ratio'
      ? t('indexing_counts_ratio', { ...common, total: shape.total })
      : t('indexing_counts_counting', common);
  });

  // Drawn only when the walk actually met the lock. `contended` counts files
  // that are journalled as skips a moment later, so this line EXPLAINS part of
  // the skipped number on the counts line above it and adds nothing to it —
  // that line is left exactly as it was.
  //
  // It promises the next scan and says nothing about the file having been
  // recorded, because the skip write meets the same lock and can fail too
  // (`job::Progress::contended`, and `mnema-ingest`'s two contention fixtures).
  const contendedLabel = $derived.by(() => {
    void $locale;
    if (phase.kind !== 'running' || phase.counts.contended === 0) return null;
    return t('indexing_counts_contended');
  });

  const etaLabel = $derived.by(() => {
    void $locale;
    if (phase.kind !== 'running') return null;
    const seconds = phase.counts.secondsLeft;
    // Not `seconds ? …` — zero seconds left is a number, and the nullish check
    // is the one this field's `Option<u64>` actually asks for.
    return seconds === null
      ? t('indexing_eta_unknown')
      : t('indexing_eta', { seconds });
  });

  const unobservedLabel = $derived.by(() => { void $locale; return t('indexing_unobserved'); });
  const cancelLabel = $derived.by(() => { void $locale; return t('indexing_cancel'); });

  const walkLines = $derived.by(() => {
    void $locale;
    if (walk === null) return null;
    return {
      sentence: outcomeSentence(WALK_ENDED, walk),
      failure: failureMessage(walk),
      result: t('indexing_walk_result', {
        indexed: walk.indexed, unchanged: walk.unchanged, skipped: walk.skipped, removed: walk.removed,
      }),
      frozenHeading: t('indexing_frozen_heading'),
      frozen: frozenRows(walk.frozen),
    };
  });

  const embedLines = $derived.by(() => {
    void $locale;
    if (phase.kind !== 'ended' || phase.pass !== 'embed') return null;
    const ending = phase.ending;
    return {
      sentence: outcomeSentence(EMBED_ENDED, ending),
      failure: failureMessage(ending),
      result: t('indexing_embed_result', {
        done: ending.done, total: ending.total, refused: ending.refused,
      }),
    };
  });

  const noteLabel = $derived.by(() => {
    void $locale;
    if (note === null) return null;
    switch (note.kind) {
      case 'noKey': return t('indexing_note_no_key');
      case 'noModel': return t('indexing_note_no_model');
      case 'rejected': return t('indexing_note_rejected');
    }
  });
</script>

{#if anything}
  <div class="indexing" data-testid="indexing">
    {#if phase.kind === 'runningUnobserved'}
      <p data-testid="indexing-unobserved">{unobservedLabel}</p>
    {/if}
    {#if passLabel}<p data-testid="indexing-pass">{passLabel}</p>{/if}
    {#if countsLabel}<p data-testid="indexing-counts">{countsLabel}</p>{/if}
    {#if contendedLabel}<p data-testid="indexing-contended">{contendedLabel}</p>{/if}
    {#if etaLabel}<p data-testid="indexing-eta">{etaLabel}</p>{/if}
    {#if cancellable}
      <button type="button" data-testid="indexing-cancel" onclick={() => jobs.cancel()}>{cancelLabel}</button>
    {/if}
    {#if walkLines}
      <div data-testid="indexing-walk-outcome">
        <span>{walkLines.sentence}</span>
        {#if walkLines.failure}<span data-testid="indexing-walk-failure">{walkLines.failure}</span>{/if}
      </div>
      <p data-testid="indexing-walk-result">{walkLines.result}</p>
      {#if walkLines.frozen.length > 0}
        <div data-testid="indexing-frozen">
          <p>{walkLines.frozenHeading}</p>
          <ul>
            <!-- Unkeyed on purpose. Two prefixes in one report CAN be equal:
                 `walk.rs` skips the climb when an existing entry covers
                 `parent`, but pushes `resolve_ancestor`'s answer, which is a
                 different string when `parent` itself is not on disk — so two
                 parents can resolve to one prefix and both be reported. Keying
                 by it would throw and take the whole section down. The rows
                 carry no state of their own, so there is nothing to keep. -->
            {#each walkLines.frozen as row}<li>{row.text}</li>{/each}
          </ul>
        </div>
      {/if}
    {/if}
    {#if embedLines}
      <div data-testid="indexing-embed-outcome">
        <span>{embedLines.sentence}</span>
        {#if embedLines.failure}<span data-testid="indexing-embed-failure">{embedLines.failure}</span>{/if}
      </div>
      <p data-testid="indexing-embed-result">{embedLines.result}</p>
    {/if}
    {#if noteLabel}
      <p data-testid="indexing-note">{noteLabel}</p>
      {#if note?.kind === 'rejected'}<p data-testid="indexing-rejection">{note.sentence}</p>{/if}
    {/if}
  </div>
{/if}

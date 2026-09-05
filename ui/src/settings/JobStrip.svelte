<script lang="ts">
  import { locale, t } from '../i18n';
  import type { Key } from '../i18n/catalog';
  import type { EndReason } from '../lib/ipc';
  import { progressShape, type JobController } from './jobs';

  // §9.2 — the minimum indexing surface: one line saying what is happening,
  // what it ended as, and a control to stop it.
  //
  // 🔴 This is the WINDOW's job strip, not the Indexing section. It is drawn
  // above the nav and outside every `{#if section === …}` (`Settings.svelte`),
  // so a running scan stays visible and stoppable from every section — the
  // live run's finding 3. The §9.3 section, which says what the index HOLDS
  // rather than what a pass is doing, is `Indexing.svelte`, a different file
  // mounted inside the panel.
  //
  // ⚠️ **This is the SMALLEST thing that can be drawn from the new snapshot,
  // and Task 7 is what rewrites it.** Task 6 replaced the channel this
  // component used to read — a stream of edges — with `ScanState`, and the
  // controller was the whole of that task. What is missing here is deliberate
  // and is Task 7's, by name: the reading outcome from `scan.lastReading` (the
  // per-root rows, the frozen prefixes, the folders-read count), the embedding
  // block from `report.embedding`, the removal sentence, and the continue
  // button from `continueAction`. Nothing below pretends to any of them: a
  // phase this build has no words for draws no line at all, rather than
  // borrowing one written about something else.
  //
  // The controller is a PROP, not something this component builds: it is
  // created once by `Settings.svelte`, above every section, because the
  // subscription and the Stop button would otherwise die with the section.
  let { jobs }: { jobs: JobController } = $props();
  // Read once, on purpose: the controller is created above this component and
  // its identity never changes for the life of the window, which is the whole
  // point of it living there. `$jobState` is then ordinary store
  // auto-subscription.
  // svelte-ignore state_referenced_locally
  const jobState = jobs.state;

  // A `Record` over the wire's own reasons, not a `switch` with a default arm:
  // a default that draws "completed" for an unmatched state is exactly how a
  // failed scan reads as a finished one, and a `Record` makes a new reason a
  // compile error instead.
  //
  // The four after `failed` get sentences of their own because they are not
  // malfunctions — `job.rs` says reporting them as `failed` tells a person
  // something broke when instead a folder is unreadable, an exclusion rule did
  // not take, or a volume may have gone missing.
  //
  // ⚠️ The sentences are the ones written for a WALK's ending, reused whole.
  // They are about the right subject — the reading is what a scan mostly is —
  // but `completed` here cannot claim the archive was seen in full, because
  // that fact lives on `scan.lastReading.complete` and drawing it is Task 7's.
  // `indexing_walk_ended_partly_read` is therefore unused for now, and
  // deliberately: this build has not read the fact that would justify it.
  const ENDED: Record<EndReason, Key> = {
    completed: 'indexing_walk_ended_completed',
    cancelled: 'indexing_walk_ended_cancelled',
    failed: 'indexing_walk_ended_failed',
    brokenWorker: 'indexing_walk_ended_broken_worker',
    rulesNotApplied: 'indexing_walk_ended_rules_not_applied',
    rootUnavailable: 'indexing_walk_ended_root_unavailable',
    volumeMissing: 'indexing_walk_ended_volume_missing',
  };

  const snapshot = $derived($jobState.scan.snapshot);
  const note = $derived($jobState.note);

  // The counts of the phase that has any. `removing` carries none at all and
  // `other` is a job nobody asked for (`scan_state::Phase`), so there is
  // nothing here to draw for either — not a zero, which would read as a run
  // that has done nothing.
  const counts = $derived.by(() => {
    if (snapshot.kind !== 'running') return null;
    const phase = snapshot.phase;
    return phase.kind === 'reading' || phase.kind === 'embedding' ? phase.counts : null;
  });

  // Offered exactly when the core says the job may be interrupted, and never
  // inferred from the phase: `cancellable` is fixed for the life of the job and
  // a removal is not stoppable, so a strip deciding for itself would offer a
  // button that does nothing.
  const cancellable = $derived(snapshot.kind === 'running' && snapshot.cancellable);

  const passLabel = $derived.by(() => {
    void $locale;
    if (snapshot.kind !== 'running') return null;
    switch (snapshot.phase.kind) {
      case 'reading': return t('indexing_walk_running');
      case 'embedding': return t('indexing_embed_running');
      // Task 7 gives these their own sentences — a removal names the folder it
      // is emptying, and a probe says nothing at all. Until then the strip
      // stays silent rather than borrowing a reading's words for a removal.
      case 'removing': return null;
      case 'other': return null;
    }
  });

  const countsLabel = $derived.by(() => {
    void $locale;
    if (counts === null) return null;
    const shape = progressShape(counts);
    const common = { done: counts.done, skipped: counts.skipped, refused: counts.refused };
    return shape.kind === 'ratio'
      ? t('indexing_counts_ratio', { ...common, total: shape.total })
      : t('indexing_counts_counting', common);
  });

  // Drawn only when the reading actually met the lock. `contended` counts files
  // that are journalled as skips a moment later, so this line EXPLAINS part of
  // the skipped number on the counts line above it and adds nothing to it —
  // that line is left exactly as it was.
  //
  // It promises the next scan and says nothing about the file having been
  // recorded, because the skip write meets the same lock and can fail too
  // (`job::Progress::contended`, and `mnema-ingest`'s two contention fixtures).
  const contendedLabel = $derived.by(() => {
    void $locale;
    if (counts === null || counts.contended === 0) return null;
    return t('indexing_counts_contended');
  });

  const etaLabel = $derived.by(() => {
    void $locale;
    if (counts === null) return null;
    // Not `seconds ? …` — zero seconds left is a number, and the nullish check
    // is the one this field's `Option<u64>` actually asks for.
    return counts.secondsLeft === null
      ? t('indexing_eta_unknown')
      : t('indexing_eta', { seconds: counts.secondsLeft });
  });

  const cancelLabel = $derived.by(() => { void $locale; return t('indexing_cancel'); });

  // 🔴 An ending is a STATE now, not an event: a scan that finished stays
  // finished until the next one claims the slot, so this line is still here for
  // a window opened a minute later (`scan_state.rs`). `message` is shown, not
  // dropped: a broken pool, a missing worker binary and a panic all arrive as
  // `failed`, and that field is the only thing that tells them apart.
  const endedLines = $derived.by(() => {
    void $locale;
    if (snapshot.kind !== 'ended') return null;
    const report = snapshot.report;
    return {
      sentence: t(ENDED[report.reason]),
      failure: report.message === null ? null : t('indexing_failure_message', { message: report.message }),
    };
  });

  // A rejected command crosses the IPC as text and nothing here reads its shape
  // (`error.rs`): one lead-in, then the backend's own sentence verbatim.
  const noteLabel = $derived.by(() => { void $locale; return note === null ? null : t('indexing_note_rejected'); });

  // Nothing to say, nothing on screen. A strip that is always there, saying it
  // is idle, is noise on a window somebody opened to change a model — and an
  // empty box during a phase this build has no words for is the same noise with
  // less in it.
  const anything = $derived(
    passLabel !== null || countsLabel !== null || endedLines !== null || noteLabel !== null || cancellable,
  );
</script>

{#if anything}
  <div class="indexing" data-testid="indexing">
    {#if passLabel}<p data-testid="indexing-pass">{passLabel}</p>{/if}
    {#if countsLabel}<p data-testid="indexing-counts">{countsLabel}</p>{/if}
    {#if contendedLabel}<p data-testid="indexing-contended">{contendedLabel}</p>{/if}
    {#if etaLabel}<p data-testid="indexing-eta">{etaLabel}</p>{/if}
    {#if cancellable}
      <button type="button" data-testid="indexing-cancel" onclick={() => jobs.cancel()}>{cancelLabel}</button>
    {/if}
    {#if endedLines}
      <div data-testid="indexing-ended">
        <span>{endedLines.sentence}</span>
        {#if endedLines.failure}<span data-testid="indexing-ended-failure">{endedLines.failure}</span>{/if}
      </div>
    {/if}
    {#if noteLabel}
      <p data-testid="indexing-note">{noteLabel}</p>
      <p data-testid="indexing-rejection">{note}</p>
    {/if}
  </div>
{/if}

<script lang="ts">
  import { locale, t } from '../i18n';
  import type { Key } from '../i18n/catalog';
  import type { EndReason, Frozen, FrozenReason, IndexRead, RootOutcome } from '../lib/ipc';
  import { continueAction, progressShape, type JobController } from './jobs';

  // §9.2 — the settings window's status line: what a pass is doing, what the
  // last READING came to (which outlives the pass that made it), what the
  // report says about the embedding half, and the one button that offers to
  // carry on.
  //
  // 🔴 This is the WINDOW's job strip, not the Indexing section. It is drawn
  // above the nav and outside every `{#if section === …}` (`Settings.svelte`),
  // so a running scan stays visible and stoppable from every section — the
  // live run's finding 3. The §9.3 section, which says what the index HOLDS
  // rather than what a pass is doing, is `Indexing.svelte`, a different file
  // mounted inside the panel.
  //
  // 🔴 Task 6 gave this component the smallest thing drawable from the new
  // `ScanState` snapshot and left the rest for this task, by name: the reading
  // outcome from `scan.lastReading` (the per-root rows, the frozen prefixes,
  // the folders-read count, the partly-read sentence), the embedding block
  // from `report.embedding`, the removal sentence, and the continue button
  // from `continueAction`. This is that rewrite.
  //
  // The controller is a PROP, not something this component builds: it is
  // created once by `Settings.svelte`, above every section, because the
  // subscription and the Stop button would otherwise die with the section.
  // `read` is the second prop for the same shape of reason — `continueAction`
  // needs the index's own markers when a report names no resumption, and a
  // read taken inside a section would not survive the section's own unmount.
  // `null` is a real value here, not a loading placeholder to wait out: with
  // it the strip can only ever show ITS OWN row (`continueAction`'s `read ===
  // null` arm returns `null`, never a guess), which is the correct
  // degradation for a window whose model settings never loaded at all.
  let { jobs, read }: { jobs: JobController; read: IndexRead | null } = $props();
  // Read once, on purpose: the controller is created above this component and
  // its identity never changes for the life of the window, which is the whole
  // point of it living there. `$jobState` is then ordinary store
  // auto-subscription.
  // svelte-ignore state_referenced_locally
  const jobState = jobs.state;

  // The eight things a READING can honestly be said to have come to. Not
  // `EndReason` alone: `completed` splits in two depending on whether phase 1
  // ever saw the whole tree (`scan.lastReading.complete`), and `reason` alone
  // cannot say that — a scan that met an unreadable subfolder still ends
  // `completed`. `readingKind` is D-e's own rule, kept in one place so the
  // strip and (later) any other reader of `lastReading` cannot each answer it
  // differently.
  type OutcomeKind =
    | 'completed' | 'partlyRead' | 'cancelled' | 'failed'
    | 'brokenWorker' | 'rulesNotApplied' | 'rootUnavailable' | 'volumeMissing';

  function readingKind(r: { reason: EndReason; complete: boolean }): OutcomeKind {
    return r.reason === 'completed' ? (r.complete ? 'completed' : 'partlyRead') : r.reason;
  }

  // A `Record` over the outcome kinds, not a `switch` with a default arm: a
  // default that draws "completed" for an unmatched state is exactly how a
  // failed pass reads as a finished one, and a `Record` makes a new kind a
  // compile error instead.
  //
  // The four after `failed` are not malfunctions — `job.rs` says reporting
  // them as `failed` tells a person something broke when instead a folder is
  // unreadable, an exclusion rule did not take, or a volume may have gone
  // missing.
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

  // The embedding pass's own table, keyed by `EndReason` rather than
  // `OutcomeKind`: the embedding phase has no roots and so no `complete` of
  // its own, and `report.reason` is the wire's own `EndReason`, seven
  // variants. The four `walk_job.rs`-only reasons cannot reach a report whose
  // `embedding` is `ran`, but they still get a sentence, carrying the state's
  // own name — a default branch that drew one of the three real ones would be
  // exactly the silent collapse `WALK_ENDED`'s own doc comment refuses.
  const EMBED_ENDED: Record<EndReason, Key> = {
    completed: 'indexing_embed_ended_completed',
    cancelled: 'indexing_embed_ended_cancelled',
    failed: 'indexing_embed_ended_failed',
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

  // One row per root whose reading did not simply complete. `null` for a root
  // that did — `readingBlock` below filters those out, so this only has to
  // answer for the seven kinds that remain.
  function rootRowText(root: RootOutcome): string | null {
    const kind = readingKind(root);
    if (kind === 'completed') return null;
    if (kind === 'partlyRead') return t('indexing_root_partly_read', { rootPath: root.rootPath });
    if (kind === 'rootUnavailable') return t('indexing_root_unavailable', { rootPath: root.rootPath });
    if (kind === 'volumeMissing') return t('indexing_root_volume_missing', { rootPath: root.rootPath });
    // `failed`, `brokenWorker`, and the two `walk_job.rs`-only kinds a root has
    // no sentence of its own for (`cancelled`, `rulesNotApplied`): all four
    // fall back to the message this root actually carries, and — because
    // `message` is `Option<String>` on the wire — to the table's own sentence
    // for the kind when there is none, so a row is never blank.
    return t('indexing_root_message', { rootPath: root.rootPath, message: root.message ?? t(WALK_ENDED[kind]) });
  }

  // A frozen entry PREFIXED with the root it belongs to: `Frozen.prefix` is
  // relative to its own watched root (`ipc.ts`), and a reading pass now
  // covers several roots, so the bare prefix alone cannot say which folder a
  // row is under. No new catalogue key for this — `indexing_frozen_row`'s own
  // `{prefix}` takes the joined path whole.
  function frozenRow(rootPath: string, f: Frozen): { prefix: string; text: string } {
    const prefix = `${rootPath}/${f.prefix}`;
    return { prefix, text: t('indexing_frozen_row', { prefix, why: t(FROZEN_WHY[f.reason]) }) };
  }

  const scan = $derived($jobState.scan);
  const snapshot = $derived(scan.snapshot);
  const note = $derived($jobState.note);

  // The running phase, or `null` for every other snapshot. Read once here so
  // the several derivations below do not each re-narrow `snapshot.kind`.
  const phase = $derived(snapshot.kind === 'running' ? snapshot.phase : null);

  // The counts of the phase that has any. `removing` carries none at all and
  // `other` is a job nobody asked for (`scan_state::Phase`), so there is
  // nothing here to draw for either — not a zero, which would read as a run
  // that has done nothing.
  const counts = $derived(
    phase !== null && (phase.kind === 'reading' || phase.kind === 'embedding') ? phase.counts : null,
  );

  // Offered exactly when the core says the job may be interrupted, and never
  // inferred from the phase: `cancellable` is fixed for the life of the job and
  // a removal is not stoppable, so a strip deciding for itself would offer a
  // button that does nothing.
  const cancellable = $derived(snapshot.kind === 'running' && snapshot.cancellable);

  const passLabel = $derived.by(() => {
    void $locale;
    if (phase === null) return null;
    switch (phase.kind) {
      case 'reading':
        // One-based on the wire (`scan_job.rs`: "3 of 7" is what a person
        // reads), so nothing here adds or subtracts one.
        return t('indexing_reading_root', {
          rootIndex: phase.rootIndex, rootCount: phase.rootCount, rootPath: phase.rootPath,
        });
      case 'embedding':
        return t('indexing_embed_running');
      case 'removing':
        return t('indexing_removing', { rootPath: phase.rootPath });
      // A probe or a model adoption (`scan_state::OtherJob`): nobody asked for
      // either, and this build has no words for them — Stop alone still
      // shows, because `cancellable` does not depend on having a sentence.
      case 'other':
        return null;
    }
  });

  // A fresh embedding pass reporting zero of zero reads as "nothing to do"
  // while a run is genuinely under way — the same trap `progressShape`'s own
  // "countingUp" shape exists for on a reading pass, except an embedding pass
  // has no such shape of its own to fall into, so it gets a sentence instead
  // of a line of counts.
  const embedStartingZero = $derived(
    phase !== null && phase.kind === 'embedding' && phase.counts.total === 0 && phase.counts.done === 0,
  );

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
  // it, which is why both are drawn into the same slot in the markup below.
  const embedStartingLabel = $derived.by(() => {
    void $locale;
    return embedStartingZero ? t('indexing_embed_starting_zero') : null;
  });

  // Drawn only when the reading actually met the lock, LIVE, while a reading
  // phase is running. `scan.lastReading`'s own copy of this fact is
  // `readingBlock.contended` below — the same catalogue key answers both,
  // because the fact it explains (part of the skipped count) is the same
  // fact either way, and it is what lets the sentence survive past the
  // ending.
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

  // The reading block: `scan.lastReading` whenever it exists, in `idle` and
  // `ended` alike (D-e) — during a run the live phase above replaces it, so
  // this is `null` exactly when `snapshot.kind === 'running'` as well as when
  // no reading has ever finished.
  const readingBlock = $derived.by(() => {
    void $locale;
    const reading = scan.lastReading;
    if (reading === null || snapshot.kind === 'running') return null;
    const kind = readingKind(reading);
    const rows = reading.roots
      .map((root) => ({ rootPath: root.rootPath, text: rootRowText(root) }))
      .filter((row): row is { rootPath: string; text: string } => row.text !== null);
    const frozen = reading.roots.flatMap((root) => root.frozen.map((f) => frozenRow(root.rootPath, f)));
    return {
      sentence: t(WALK_ENDED[kind]),
      rootsReadLine: reading.rootCount > 0
        ? t('indexing_roots_read', { rootsRead: reading.rootsRead, rootCount: reading.rootCount })
        : null,
      result: t('indexing_walk_result', {
        indexed: reading.indexed, unchanged: reading.unchanged, skipped: reading.skipped, removed: reading.removed,
      }),
      rows,
      frozenHeading: frozen.length > 0 ? t('indexing_frozen_heading') : null,
      frozen,
      contended: reading.contended > 0 ? t('indexing_counts_contended') : null,
    };
  });

  // The embedding block: `report.embedding`, drawn only once the job has
  // ended — the phase's own live counts are `countsLabel`/`embedStartingLabel`
  // above, while it is still running.
  const embedBlock = $derived.by(() => {
    void $locale;
    if (snapshot.kind !== 'ended') return null;
    const report = snapshot.report;
    const embedding = report.embedding;
    // A report that ended in the READING phase says nothing here, whatever
    // `embedding` holds — that ending is the reading block's own
    // (`lastReading.reason` names the very same event), and a `notReached`
    // sitting beside it is ordinary (the embedding phase was never reached),
    // not a second thing to announce. Taken FIRST, before `embedding.kind` is
    // read at all: a `ran`/`skipped` outcome cannot outrank it either
    // (review, Important 1).
    if (report.endedIn !== 'embedding') return null;
    // `notReached` here means the phase was claimed and then stopped before a
    // chunk was offered to a provider — `scan_job.rs` writes this only with
    // `cancelled` or `failed`, most often a Stop pressed while the credential
    // store was still being read. Task 6's strip drew `ENDED[report.reason]`
    // unconditionally and said so; this is that sentence, restored for the
    // one phase whose ending would otherwise go unstated.
    if (embedding.kind === 'notReached') {
      return { sentence: t(EMBED_ENDED[report.reason]), result: null as string | null };
    }
    if (embedding.kind === 'skipped') {
      const why = embedding.why;
      if (why.kind === 'noKey') return { sentence: t('indexing_note_no_key'), result: null as string | null };
      if (why.kind === 'noModel') return { sentence: t('indexing_note_no_model'), result: null as string | null };
      return {
        sentence: t('indexing_embed_not_started_store', { message: why.message }),
        result: null as string | null,
      };
    }
    // `ran`.
    return {
      sentence: t(EMBED_ENDED[report.reason]),
      result: t('indexing_embed_result', {
        done: embedding.done, total: embedding.total, refused: embedding.refused,
      }),
    };
  });

  // `report.message`, shown once for the whole ended report rather than
  // duplicated inside the reading or the embedding block: a broken pool, a
  // missing worker binary and a panic all arrive as `failed` (`job.rs`), and
  // this field is the only thing that tells them apart.
  const failureLabel = $derived.by(() => {
    void $locale;
    if (snapshot.kind !== 'ended' || snapshot.report.message === null) return null;
    return t('indexing_failure_message', { message: snapshot.report.message });
  });

  // D-m's table, decided once in `jobs.ts` so the strip and the section cannot
  // answer it differently. Rendered here only when it names THIS strip —
  // `where: 'section'` is the Indexing section's own offer (Task 8), and a
  // strip that drew it too would put two buttons in front of one decision.
  const action = $derived(continueAction(scan, read));
  const stripAction = $derived(action !== null && action.where === 'strip' ? action : null);
  const continueLabel = $derived.by(() => {
    void $locale;
    if (stripAction === null) return null;
    // `label` follows the reason, not the entry point (`jobs.ts`): a person
    // who pressed Stop is resuming, one whose scan failed is retrying.
    return t(stripAction.label === 'resume' ? 'indexing_resume' : 'indexing_retry');
  });

  // Nothing to say, nothing on screen. A strip that is always there, saying it
  // is idle, is noise on a window somebody opened to change a model — and an
  // empty box during a phase this build has no words for is the same noise
  // with less in it.
  const anything = $derived(
    passLabel !== null || countsLabel !== null || embedStartingLabel !== null
    || readingBlock !== null || embedBlock !== null || failureLabel !== null
    || note !== null || stripAction !== null || cancellable,
  );
</script>

{#if anything}
  <div class="indexing" data-testid="indexing">
    {#if passLabel}<p data-testid="indexing-pass">{passLabel}</p>{/if}
    {#if embedStartingLabel}
      <p data-testid="indexing-counts">{embedStartingLabel}</p>
    {:else if countsLabel}
      <p data-testid="indexing-counts">{countsLabel}</p>
    {/if}
    {#if contendedLabel}<p data-testid="indexing-contended">{contendedLabel}</p>{/if}
    {#if etaLabel}<p data-testid="indexing-eta">{etaLabel}</p>{/if}
    {#if cancellable}
      <button type="button" data-testid="indexing-cancel" onclick={() => jobs.cancel()}>{cancelLabel}</button>
    {/if}
    {#if readingBlock}
      <div data-testid="indexing-walk-outcome">
        <span>{readingBlock.sentence}</span>
      </div>
      {#if readingBlock.rootsReadLine}<p data-testid="indexing-roots-read">{readingBlock.rootsReadLine}</p>{/if}
      <p data-testid="indexing-walk-result">{readingBlock.result}</p>
      {#if readingBlock.contended}<p data-testid="indexing-contended">{readingBlock.contended}</p>{/if}
      {#each readingBlock.rows as row (row.rootPath)}
        <p data-testid="indexing-root-row">{row.text}</p>
      {/each}
      {#if readingBlock.frozenHeading}
        <div data-testid="indexing-frozen">
          <p>{readingBlock.frozenHeading}</p>
          <ul>
            <!-- Unkeyed on purpose. Two prefixes CAN be equal even after the
                 root-path prefix above: `walk.rs` skips the climb when an
                 existing entry covers `parent`, but pushes `resolve_ancestor`'s
                 answer, a different string whenever `parent` is not itself on
                 disk — so two parents under the same root can resolve to one
                 prefix and both be reported. Keying by it would throw and take
                 the whole strip down; the rows carry no state of their own, so
                 there is nothing to keep across a re-render. -->
            {#each readingBlock.frozen as row}<li>{row.text}</li>{/each}
          </ul>
        </div>
      {/if}
    {/if}
    {#if embedBlock}
      <div data-testid="indexing-embed-outcome">
        <span>{embedBlock.sentence}</span>
      </div>
      {#if embedBlock.result}<p data-testid="indexing-embed-result">{embedBlock.result}</p>{/if}
    {/if}
    {#if failureLabel}<p data-testid="indexing-ended-failure">{failureLabel}</p>{/if}
    {#if stripAction}
      <button
        type="button"
        data-testid="indexing-continue"
        onclick={() => jobs.scan(stripAction.entry)}
      >{continueLabel}</button>
    {/if}
    <!-- A rejected command crosses the IPC as text (`error.rs`) and nothing
         here branches on it: the backend's own sentence, verbatim, and no
         lead-in of this window's own — Task 7 drops `indexing_note_rejected`,
         which was the only remaining reader of that heading key. -->
    {#if note !== null}
      <p data-testid="indexing-rejection">{note}</p>
    {/if}
  </div>
{/if}

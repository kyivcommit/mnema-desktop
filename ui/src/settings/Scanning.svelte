<script lang="ts">
  import { locale, t } from '../i18n';
  import type { Key } from '../i18n/catalog';
  import { formatIndexedAt, formatIndexedDate } from '../i18n/recency';
  import type { ModelSettings, UnreadableCause } from '../lib/ipc';
  import { continueAction, type JobController } from './jobs';
  import ScanProgress from './ScanProgress.svelte';

  // §9.3 — the Scanning SECTION: what the index HOLDS, when it last grew, the
  // ONE Scan control, and the continue row `continueAction` (`jobs.ts`)
  // offers when the index still carries a marker a report did not name.
  //
  // 🔴 Renamed from `Indexing.svelte` (Task 8), which is the second rename this
  // file's name has cost: `JobStrip.svelte` — the window's status line — was
  // itself called `Indexing.svelte` until Task 6 moved that name off it, and
  // Task 5 moved the strip itself: it is drawn after `.scols`, at the bottom
  // of the window, outside every section, not above the nav any more. This
  // file has always been the one that lives inside the panel; Task 5 gave it
  // its own `<ScanProgress>` too — the same running-phase projection the
  // strip draws, off the same snapshot — but what it says on its own is
  // still what the index HOLDS, never a second account of what a pass is
  // doing.
  //
  // The controller arrives as a PROP for the same reason every other section
  // takes it that way (`Settings.svelte`): it is created once, above every
  // section, because the channel a job reports on belongs to whoever started
  // it. This section starts a scan through it now — the Scan control
  // below — but the running pass itself is still drawn on the strip above the
  // nav, not here.
  //
  // 🔴 `settings`/`loadError`, not a `read` this component fetches itself. Task
  // 7 left this section holding its OWN `model_settings` poll — its own mount,
  // its own `refresh()`, its own subscription to `jobs.state` — racing the poll
  // `Settings.svelte` already kept for `JobStrip.svelte`, with no ordering
  // between the two and no shared answer. `Settings.svelte` is now the window's
  // SINGLE reader of `model_settings`; what it read is handed down here as a
  // prop, and this component calls `modelSettings()` nowhere — no mount, no
  // subscription, no `refresh()` of its own to keep in step with the other one.
  let { jobs, settings, loadError }: {
    jobs: JobController; settings: ModelSettings | null; loadError: string | null;
  } = $props();
  // Read once, on purpose: the controller's identity never changes for the life
  // of the window. `$jobState` below is then ordinary auto-subscription.
  // svelte-ignore state_referenced_locally
  const jobState = jobs.state;

  // §10: the union is discriminated HERE, before any field of `IndexRead` is
  // touched. Everything below reads `read` or `unreadable`, each of which is
  // null on the other arm — so no line can be drawn from a field that does not
  // exist on the branch the backend actually sent.
  const index = $derived(settings === null ? null : settings.index);
  const read = $derived(index !== null && index.kind === 'read' ? index : null);
  const unreadable = $derived(index !== null && index.kind === 'unreadable' ? index : null);

  // The moment the index last grew, as the backend states it. `null` is not a
  // missing value — it is `MAX(ingest_stage.updated_at)` over an index in which
  // nothing has ever finished, and it gets a sentence of its own rather than a
  // default. A `?? 0` here would render 1 January 1970 beside a relative phrase
  // counting twenty thousand days, and both of those look like measurements.
  const lastIndexedAt = $derived(read === null ? null : read.lastIndexedAt);

  const filesLine = $derived.by(() => {
    void $locale;
    if (read === null) return null;
    return t('indexing_index_files', { count: read.indexedFiles });
  });

  // Two lines, not one sentence, and they answer two different questions. The
  // date is what a person compares against the file they edited this morning;
  // the phrase is what they feel. Neither stands in for the other, which is why
  // §9.3 asks for the date and this project already had the phrase.
  const dateLine = $derived.by(() => {
    void $locale;
    if (lastIndexedAt === null) return null;
    return t('indexing_index_updated', { date: formatIndexedDate(lastIndexedAt, $locale) });
  });
  // Wrapped in a catalogue sentence rather than rendered bare. `formatIndexedAt`
  // answers "how long ago" and nothing else — the Recents card can print it
  // alone because a filename sits beside it supplying the subject. Last on this
  // panel it had neither subject nor full stop while every line above it had
  // both, and read as a fragment somebody forgot to finish.
  const agoLine = $derived.by(() => {
    void $locale;
    if (lastIndexedAt === null) return null;
    return t('indexing_index_updated_ago', { ago: formatIndexedAt(lastIndexedAt, Date.now()) });
  });
  const neverLine = $derived.by(() => {
    void $locale;
    if (read === null || lastIndexedAt !== null) return null;
    return t('indexing_index_never');
  });

  // A `Record` over the two causes rather than a ternary, for the reason
  // `JobStrip.svelte`'s own tables give: a third cause added to
  // `UnreadableCause` becomes a compile error instead of silently drawing one
  // of the two sentences that already exist.
  const UNREADABLE: Record<UnreadableCause, Key> = {
    notOpen: 'indexing_index_unreadable_not_open',
    readFailed: 'indexing_index_unreadable_read_failed',
  };

  const unreadableLines = $derived.by(() => {
    void $locale;
    if (unreadable === null) return null;
    return {
      sentence: t(UNREADABLE[unreadable.cause]),
      // 🔴 VERBATIM, and deliberately the opposite of the Models section's rule
      // (`Models.svelte:177-186` never shows it). `IndexSettings::Unreadable`'s
      // own doc says `reason` "stays verbatim, for showing" and `cause` is what
      // anything branches on (`models.rs:932`). This is the one screen whose
      // job is to tell a person what is wrong with their index; the text is
      // shown to the person who owns the machine it names, and by decision
      // there is no service between this product and any other (D22).
      reason: t('indexing_index_unreadable_reason', { reason: unreadable.reason }),
    };
  });

  // 🔴 The two scopes, owed since PR 7. `IndexRead::failed_chunks` is
  // cumulative for the SPACE; `job::Progress::refused` is what the run that has
  // just ended gave up on (`job.rs:22-44` holds them apart and says whichever
  // surface shows them owes each its own words). Two sentences with two
  // subjects, never one key drawn twice.
  const failedChunksLine = $derived.by(() => {
    void $locale;
    if (read === null || read.failedChunks === 0) return null;
    return t('indexing_index_failed_chunks', { count: read.failedChunks });
  });

  // The run's own number. Gated on the count rather than on the pass: `refused`
  // is written only by `embed_job.rs` and is always `0` for a walk (`job.rs:25`),
  // so a non-zero value already names the pass this sentence names — a
  // `pass === 'embed'` beside it would be a condition that cannot be false.
  //
  // 🔴 And deliberately NOT gated on `read`, which is a decision rather than an
  // omission (review, Minor 5). The pass really did refuse those chunks, and
  // that fact does not stop being true when the next read of the index fails.
  // The sequence is a real one and not a contrived pairing: a pass ends, the
  // ending triggers the re-read, and the re-read comes back `Unreadable`. Gated
  // on `read`, the window would answer "the index could not be read" and delete,
  // in the same breath, the only surviving report of what the pass just did.
  // The subject is a pass, not the index, so the sentence stands on its own —
  // and the cumulative sentence beside it does not, because that one IS about
  // the index and has no arm to be read from.
  const refusedRunLine = $derived.by(() => {
    void $locale;
    const snapshot = $jobState.scan.snapshot;
    if (snapshot.kind !== 'ended') return null;
    // The refusals of the RUN are the embedding pass's own, and only a pass
    // that `ran` has any: `notReached` and `skipped` offered no chunk to a
    // provider at all (`scan_state::EmbedOutcome`), so reading a count off them
    // would be inventing one.
    const embedding = snapshot.report.embedding;
    if (embedding.kind !== 'ran' || embedding.refused === 0) return null;
    return t('indexing_index_refused_run', { count: embedding.refused });
  });

  const loadFailedLabel = $derived.by(() => { void $locale; return t('indexing_index_load_failed'); });

  // 🔴 Task 8: the folder buttons are gone (`Folders.svelte`) and adding a
  // folder starts nothing (owner's ruling — a configuration move, excluding
  // subfolders among them, still has to happen first). This is now the ONE way
  // into a scan: shown whenever the slot is free, regardless of what the index
  // read above says — an unreadable index or a failed `model_settings` read are
  // both reasons a person might press this to find out whether scanning fixes
  // it, not reasons to hide the only button that starts one.
  const showScanButton = $derived($jobState.scan.snapshot.kind !== 'running');
  const scanButtonLabel = $derived.by(() => { void $locale; return t('scanning_scan'); });

  // Task 5 — the same running-phase projection the bottom disclosure draws,
  // from the SAME `jobs.state` snapshot this section already reads for
  // `showScanButton` above: one component, not two readers of the phase free
  // to disagree about what it says.
  const phase = $derived($jobState.scan.snapshot.kind === 'running' ? $jobState.scan.snapshot.phase : null);

  // D-m's table (`jobs.ts`), decided once so the strip and this section cannot
  // answer it differently. Rendered here only when it names THIS section —
  // `where: 'strip'` is `JobStrip.svelte`'s own offer, drawn from a report that
  // named its own resumption; a section that drew that one too would put two
  // buttons in front of one decision, one of them stale the instant the other
  // is pressed.
  const action = $derived(continueAction($jobState.scan, read));
  const sectionAction = $derived(action !== null && action.where === 'section' ? action : null);
  // Always `resume`, never `retry`: `jobs.ts` gives a `where: 'section'` result
  // only from the index's own markers, neither of which ever carries a
  // failure to retry — a marker says work is owed, not that anything failed.
  //
  // F2 (Task 10 live run): the two `where: 'section'` arms are not one offer.
  // The marker arm (`entry === 'full'`) resumes a half-read archive — the
  // same word the strip's own resume button uses (`indexing_resume`) — but
  // the queue arm (`entry === 'embedOnly'`) resumes ONLY the embedding pass,
  // and the shared label promised the wrong half of the work to a person who
  // had nothing left to read. `scanning_continue_embedding` is that arm's own
  // key.
  const continueLabel = $derived.by(() => {
    void $locale;
    return t(sectionAction !== null && sectionAction.entry === 'embedOnly'
      ? 'scanning_continue_embedding'
      : 'indexing_resume');
  });
  const incompleteLabel = $derived.by(() => { void $locale; return t('scanning_incomplete'); });
  // The existing queue sentence, drawn only for the `embedOnly` offer — the
  // `full` offer's own sentence is `incompleteLabel` above, and the two never
  // share a row: `continueAction` never returns both at once for one state.
  const pendingLine = $derived.by(() => {
    void $locale;
    if (sectionAction === null || sectionAction.entry !== 'embedOnly' || read === null) return null;
    return t('indexing_index_pending_chunks', { count: read.pendingChunks });
  });
</script>

<!-- The failed read leads, and it does NOT gate what follows: this is an `{#if}`,
     not an `{:else}`. On the FIRST read's rejection there is nothing below it
     anyway, because `settings` is still null. On a refused RE-read the previous
     answer is still there and stays on screen — `Settings.svelte`'s own ruling,
     carried down unchanged from `refresh()`'s doc comment there — so the
     sentence sits over the numbers it could not confirm, which is the whole of
     what it is for. Do not turn this into a gate: blanking the panel would take
     away a count that was true a moment ago and probably still is. -->
{#if phase}<ScanProgress {phase} />{/if}
{#if loadError}
  <p data-testid="indexing-index-load-failed">{loadFailedLabel}</p>
  <p data-testid="indexing-index-load-error">{loadError}</p>
{/if}
{#if unreadableLines}
  <p data-testid="indexing-index-unreadable">{unreadableLines.sentence}</p>
  <p data-testid="indexing-index-unreadable-reason">{unreadableLines.reason}</p>
{/if}
{#if filesLine}<p data-testid="indexing-index-files">{filesLine}</p>{/if}
{#if dateLine}<p data-testid="indexing-index-date">{dateLine}</p>{/if}
{#if agoLine}<p data-testid="indexing-index-ago">{agoLine}</p>{/if}
{#if neverLine}<p data-testid="indexing-index-never">{neverLine}</p>{/if}
{#if failedChunksLine}<p data-testid="indexing-index-failed-chunks">{failedChunksLine}</p>{/if}
{#if refusedRunLine}<p data-testid="indexing-index-refused-run">{refusedRunLine}</p>{/if}
{#if sectionAction}
  {#if sectionAction.entry === 'full'}
    <p data-testid="scanning-incomplete">{incompleteLabel}</p>
  {:else if pendingLine}
    <p data-testid="indexing-index-pending-chunks">{pendingLine}</p>
  {/if}
  <button
    type="button"
    data-testid="scanning-continue"
    onclick={() => jobs.scan(sectionAction.entry)}
  >{continueLabel}</button>
{/if}
{#if showScanButton}
  <button type="button" data-testid="scanning-scan" onclick={() => jobs.scan('full')}>{scanButtonLabel}</button>
{/if}

<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { locale, t } from '../i18n';
  import type { Key } from '../i18n/catalog';
  import { formatIndexedAt, formatIndexedDate } from '../i18n/recency';
  import { modelSettings, type ModelSettings, type UnreadableCause } from '../lib/ipc';
  import type { JobController, JobPhase } from './jobs';

  // §9.3 — the Indexing SECTION: what the index HOLDS, and when it last grew.
  //
  // 🔴 Not to be confused with `JobStrip.svelte`, which was called
  // `Indexing.svelte` until this commit. That one is the window's status line,
  // drawn above the nav and outside every section, and it says what a pass is
  // doing right now. This one lives inside the panel and says nothing about a
  // running pass — it reads `model_settings` and draws the index's own numbers.
  //
  // The controller arrives as a PROP for the same reason every other section
  // takes it that way (`Settings.svelte`): it is created once, above every
  // section, because the channel a job reports on belongs to whoever started
  // it. This section does not start anything; it only listens, because an
  // ending is the one moment the numbers below can have changed.
  let { jobs }: { jobs: JobController } = $props();
  // Read once, on purpose: the controller's identity never changes for the life
  // of the window. `$jobState` below is then ordinary auto-subscription.
  // svelte-ignore state_referenced_locally
  const jobState = jobs.state;

  let settings = $state<ModelSettings | null>(null);
  // A rejected read of `model_settings`. §10: a rejection arrives as a
  // SENTENCE, never as a kind — shown verbatim beside a catalogue lead-in, and
  // never branched on. Held apart from the index's own `Unreadable` arm, which
  // is a state the backend describes rather than a call that failed.
  let loadError = $state<string | null>(null);

  // A newer request always wins over an older one that resolves later. Copied
  // by name from `Models.svelte:75-80` rather than re-derived: every call that
  // writes `settings` stamps itself with the sequence current at the moment it
  // was ISSUED, and applies its answer only while that stamp is still the
  // latest. Two reads can be in flight here whenever endings arrive faster than
  // the IPC answers, and without the stamp the older reply repaints the screen
  // with numbers taken before the pass that triggered the newer one.
  let settingsSeq = 0;

  // Both exits live here rather than in the callers, and that is the point:
  // there are two callers (the mount and every ending), a rejection is possible
  // on both, and an error handled at one call site only is a section that goes
  // quiet exactly when a re-read starts failing. The stamp guards BOTH
  // directions — an older rejection must not put a failure sentence on a screen
  // a newer read has already repainted, and an older success must not take one
  // away that a newer rejection has just earned.
  //
  // A successful read clears the sentence. Without that line the failure
  // outlives the state it describes: one refused re-read would leave "the state
  // of the index could not be read" standing over numbers that were re-read
  // successfully a second later.
  //
  // The numbers themselves are KEPT across a failed re-read, which is
  // `Tree.svelte`'s ruling and not a new one: a count that was true a moment
  // ago probably still is, and blanking the panel costs a person information
  // they had. What the sentence adds is that it is no longer confirmed.
  async function refresh() {
    const seq = ++settingsSeq;
    try {
      const s = await modelSettings();
      if (seq !== settingsSeq) return; // a newer read has already spoken
      settings = s;
      loadError = null;
    } catch (e) {
      if (seq !== settingsSeq) return; // superseded before this rejection arrived
      loadError = e instanceof Error ? e.message : String(e);
    }
  }

  onMount(() => {
    // 🔴 Subscribed BEFORE the first read is issued, and the order is the
    // guard. An ending that lands while the initial `model_settings` is in
    // flight would otherwise be heard by nobody, and the screen would sit on
    // numbers taken before the pass finished — D130's F1 in another component
    // (`requirements.md:977`).
    //
    // Compared by phase IDENTITY, not by kind: the controller writes a fresh
    // phase object per event, so a progress report changes the object without
    // ever being an ending. Seeded with what the store already holds, so a
    // section switch back does not re-read on the same mount.
    let seen: JobPhase = get(jobs.state).phase;
    const stop = jobs.state.subscribe(({ phase }) => {
      if (phase === seen) return;
      seen = phase;
      if (phase.kind === 'ended') void refresh();
    });
    void refresh();
    // 🔴 Returned, so Svelte tears the subscription down on destroy. This
    // section is inside `Settings.svelte`'s `{#if section === …}` chain, so
    // every nav change unmounts it: without this line each visit leaves a live
    // listener behind and one ending fans out to all of them.
    return stop;
  });

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
    const phase = $jobState.phase;
    if (phase.kind !== 'ended' || phase.ending.refused === 0) return null;
    return t('indexing_index_refused_run', { count: phase.ending.refused });
  });

  // F4 (spec §9.3, amended 2026-09-04): the embedding queue —
  // `IndexRead.pendingChunks`, `Db::queued_chunk_count`'s own count. A tray
  // Stop mid-pass, then a restart, left thousands of chunks un-embedded with
  // nothing on any screen saying so; the only resume was the Scan button
  // beside the right folder happening to chain into an embed.
  //
  // Gated on the phase, not only on the count: the number on screen is a
  // moment-old read and does not shrink as a resumed run works through the
  // queue, so once a pass is under way the strip above owns that state and
  // this line and its button step aside rather than show a count a running
  // pass is already changing.
  const showPending = $derived(
    read !== null && read.pendingChunks > 0
      && ($jobState.phase.kind === 'idle' || $jobState.phase.kind === 'ended'),
  );
  const pendingLine = $derived.by(() => {
    void $locale;
    if (!showPending || read === null) return null;
    return t('indexing_index_pending_chunks', { count: read.pendingChunks });
  });
  const resumeEmbeddingLabel = $derived.by(() => {
    void $locale;
    return t('indexing_index_resume_embedding');
  });

  const loadFailedLabel = $derived.by(() => { void $locale; return t('indexing_index_load_failed'); });

  // Through the controller, never `startEmbedJob` directly — `Models.svelte`'s
  // own `reembed` (`:468-484`) argues why: the pass this button starts belongs
  // on the window's strip, where its progress and its Cancel stay reachable
  // from every section. Nothing is caught here for the same reason that
  // argument gives: a refusal is the controller's to report, in the same words
  // and the same place as every other refused command.
  function resumeEmbedding() {
    void jobs.embed();
  }
</script>

<!-- The failed read leads, and it does NOT gate what follows: this is an `{#if}`,
     not an `{:else}`. On the FIRST read's rejection there is nothing below it
     anyway, because `settings` is still null. On a refused RE-read the previous
     answer is still there and stays on screen — the ruling recorded beside
     `refresh()` above — so the sentence sits over the numbers it could not
     confirm, which is the whole of what it is for. Do not turn this into a
     gate: blanking the panel would take away a count that was true a moment ago
     and probably still is. -->
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
{#if showPending}
  <p data-testid="indexing-index-pending-chunks">{pendingLine}</p>
  <button type="button" data-testid="indexing-resume-embedding" onclick={resumeEmbedding}>{resumeEmbeddingLabel}</button>
{/if}

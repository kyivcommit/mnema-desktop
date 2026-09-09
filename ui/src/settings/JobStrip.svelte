<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import type { Key } from '../i18n/catalog';
  import type { EndReason, Frozen, FrozenReason, IndexRead, RootOutcome } from '../lib/ipc';
  import { continueAction, phaseLabel, type JobController } from './jobs';
  import ScanProgress from './ScanProgress.svelte';

  // §9.2 — the settings window's status line: what a pass is doing, what the
  // last READING came to (which outlives the pass that made it), what the
  // report says about the embedding half, and the one button that offers to
  // carry on.
  //
  // 🔴 This is the WINDOW's job strip, not the Scanning section. It is drawn
  // after `.scols` (`Settings.svelte`), so a running scan stays visible and
  // stoppable from every section — the live run's finding 3, and Task 5's own
  // correction of where "outside the section" actually means (the bottom of
  // the window, not the top — see the geometry note there). The §9.3 section,
  // which says what the index HOLDS rather than what a pass is doing, is
  // `Scanning.svelte`, a different file mounted inside the panel.
  //
  // Task 5 turned this into a non-modal DISCLOSURE: one line always on
  // screen, in every section, and the report itself behind a `<details>` a
  // person opens on purpose rather than a card permanently taking up the
  // window's height. `ScanProgress.svelte` now draws the running phase — the
  // sentence, the counts, the estimate, the busy note — as the same
  // projection `Scanning.svelte` draws from the same snapshot; what stays here
  // is the reading outcome from `scan.lastReading` (the per-root rows, the
  // frozen prefixes, the folders-read count, the partly-read sentence), the
  // embedding block from `report.embedding`, the removal sentence, Stop, and
  // the continue button from `continueAction`.
  //
  // The controller is a PROP, not something this component builds: it is
  // created once by `Settings.svelte`, above every section, because the
  // subscription and the Stop button would otherwise die with the section.
  // `read` is the second prop for the same shape of reason — `continueAction`
  // needs the index's own markers when a report names no resumption, and a
  // read taken inside a section would not survive the section's own unmount.
  // `focusFallback` is the third: where focus goes when the whole disclosure
  // disappears out from under it (the job ended with nothing left to say) —
  // `Settings.svelte` is the one place that knows which nav button is
  // pressed, so it is the one place that can answer this.
  let { jobs, read, focusFallback, section }: {
    jobs: JobController; read: IndexRead | null; focusFallback: () => void; section: string;
  } = $props();
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
    // F6 (Task 10 live run): `cancelled` used to fall to the generic message
    // below, whose own fallback is `WALK_ENDED['cancelled']` — the SAME
    // sentence `readingBlock.sentence` already draws once for the whole
    // reading, so a cancelled root's own row repeated the top sentence
    // verbatim rather than saying anything about that root in particular.
    if (kind === 'cancelled') return t('indexing_root_cancelled', { rootPath: root.rootPath });
    // `failed`, `brokenWorker`, and `rulesNotApplied` — the one
    // `walk_job.rs`-only kind still with no sentence of its own: it falls
    // back to the message this root actually carries, and — because
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
  // the several derivations below do not each re-narrow `snapshot.kind`, and
  // handed to `ScanProgress` whole — it is the one component that draws it.
  const phase = $derived(snapshot.kind === 'running' ? snapshot.phase : null);

  const cancelLabel = $derived.by(() => { void $locale; return t('indexing_cancel'); });

  // The reading block: `scan.lastReading` whenever it exists, in `idle` and
  // `ended` alike (D-e) — during a run the live phase above replaces it, so
  // this is `null` exactly when `snapshot.kind === 'running'` as well as when
  // no reading has ever finished.
  //
  // ⚠️ **It describes the LAST READING PASS, not the index as it stands now,
  // and after a folder removal those are different things.** `lastReading` is
  // replaced only by the next reading phase, and a removal ends the slot with
  // `finish(Idle, Some(files))` — so the rows below can go on naming a folder
  // the list in the section beneath no longer has, its `indexing_root_partly_read`
  // row included. Accepted rather than filtered: the direction is the
  // safe one (a partial-read warning that over-warns costs a re-read of
  // unchanged files; one that under-warns leaves a person believing an archive
  // is fully indexed), and dropping the rows for roots absent from the current
  // listing would need this component to read that listing, which is a second
  // reader of `list_tree` on the window's status line. The sentence a person
  // reads is about a pass, and the pass really did meet that folder.
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
  // ended — the phase's own live counts are `ScanProgress`'s, while it is
  // still running.
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
  //
  // 🔴 **`rulesNotApplied` gets a second sentence appended: where to go.**
  // `scan_job.rs`'s own comment on the preflight refusal says the person's
  // next step is the Folders section, not a retry — but the preflight ending
  // never reaches a folder to read, so it never populates `scan.lastReading`, and
  // `readingBlock` (built from exactly that) draws nothing for it. This
  // message is therefore the ONLY sentence a preflight refusal shows, and it
  // was the Rust refusal's own text alone, with no pointer anywhere on
  // screen. The walk-time `rulesNotApplied` ending reaches this same
  // `message` field too (`scan_job.rs`'s `read_every_root` sets it from the
  // stopping root's own text) and reads it beside `readingBlock`'s generic
  // `indexing_walk_ended_rules_not_applied` sentence, which itself has no
  // pointer either — so both arms were missing it, and one shared key here
  // fixes both at once rather than two copies free to drift apart.
  //
  // `reason` alone is the whole guard, with no `endedIn` half: `rulesNotApplied`
  // can only ever end a reading. `after_root` (`scan_job.rs`) is the only place
  // that breaks the walk on it, which is the reading loop; `resume_for`
  // (`scan_job.rs`) folds `rulesNotApplied` into the same `None` arm regardless
  // of `endedIn`, so the `(rulesNotApplied, embedding)` row its own exhaustive
  // test table enumerates is that table checking a function argument no
  // production caller ever passes together with this reason — not a real
  // ending this pointer would otherwise misfire on.
  const failureLabel = $derived.by(() => {
    void $locale;
    if (snapshot.kind !== 'ended' || snapshot.report.message === null) return null;
    const message = t('indexing_failure_message', { message: snapshot.report.message });
    if (snapshot.report.reason === 'rulesNotApplied') {
      return `${message} ${t('indexing_rules_not_applied_pointer')}`;
    }
    return message;
  });

  // D-m's table, decided once in `jobs.ts` so the strip and the section cannot
  // answer it differently. Rendered here only when it names THIS strip —
  // `where: 'section'` is the Scanning section's own offer (Task 8), and a
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
  // is idle, is noise on a window somebody opened to change a model. `phase`
  // alone stands in for the three counts-shaped things `ScanProgress` now
  // draws (they are never non-null unless it is), and Task 5 gave `other` a
  // name too, so a probe or a model adoption no longer needs `cancellable`
  // named separately here — every running snapshot carries a phase.
  const anything = $derived(
    phase !== null || readingBlock !== null || embedBlock !== null
    || failureLabel !== null || note !== null || stripAction !== null,
  );

  // Task 5 — the ended report's OWN top-line sentence, picked by which phase
  // it ended IN rather than assembled from scratch: `embedBlock.sentence`
  // already carries "the embedding pass broke off/was stopped/finished" for
  // every `EndReason`, `readingBlock.sentence` carries the matching table for
  // a reading. Picking by `endedIn` is what keeps a failure that happened
  // AFTER a clean reading from disappearing behind that reading's own
  // `indexing_walk_ended_completed` sentence — the failure is the embedding
  // block's own sentence, not the reading's, and only `endedIn` says which
  // block is the report's own voice.
  const endedSentence = $derived(
    snapshot.kind !== 'ended'
      ? null
      : snapshot.report.endedIn === 'embedding'
        ? (embedBlock?.sentence ?? readingBlock?.sentence ?? null)
        : (readingBlock?.sentence ?? null),
  );

  // The disclosure's one-line summary — §4.2's own priority order. A command
  // note (a refusal) outranks everything, because it is about the press the
  // person just made; a running phase is the next most current fact; an ended
  // report's own sentence next; and `scan.lastReading` last, for a window that
  // opened onto a pass that finished before it existed, or whose report has
  // since been superseded by an idle snapshot with nothing else to say.
  // `indexing_summary_fallback` is the floor under all four — reachable only
  // if `anything` is true for a reason none of the four names, which nothing
  // built today does.
  const summaryText = $derived.by(() => {
    void $locale;
    if (note !== null) return note;
    if (phase !== null) {
      const l = phaseLabel(phase);
      return t(l.key, l.params);
    }
    if (endedSentence !== null) return endedSentence;
    if (readingBlock !== null) return readingBlock.sentence;
    return t('indexing_summary_fallback');
  });

  // ---------------------------------------------------------------------------
  // The disclosure itself: non-modal, closed by default, and never trapping
  // focus. It closes on Escape, on a click or a focus move outside it, on a
  // section change, and when the DOM update that follows a state change
  // removes the element focus was actually on — never on an ordinary count
  // tick, which changes none of those things.
  // ---------------------------------------------------------------------------
  let disclosure: HTMLDetailsElement | undefined = $state();
  let summaryEl: HTMLElement | undefined = $state();
  let open = $state(false);

  // The element inside the panel that last held focus, tracked so a DOM
  // update that removes it (the report reshaped, or the whole panel went)
  // can decide where focus goes next instead of leaving it on `<body>`.
  let lastFocused: HTMLElement | null = null;

  function withinPanel(node: EventTarget | null): boolean {
    return disclosure !== undefined && node instanceof Node && disclosure.contains(node);
  }

  // Document-level, because the panel closing is a reaction to focus or a
  // press LANDING OUTSIDE it — an outside element's own handler owes this
  // component nothing, and this must not stop it from also getting the click
  // or the focus it asked for (no `preventDefault`, no `stopPropagation` on
  // either of these two).
  function onDocumentFocusIn(e: FocusEvent) {
    if (!open) return;
    if (withinPanel(e.target)) {
      lastFocused = e.target as HTMLElement;
      return;
    }
    open = false;
  }

  function onDocumentPointerDown(e: PointerEvent) {
    if (open && !withinPanel(e.target)) open = false;
  }

  // On the `<details>` itself, not `document` — Escape is this panel's own
  // business only while focus is somewhere inside it, which is exactly what
  // a listener on the element itself already guarantees, and it must not
  // reach a window-level Escape handler (`Application.svelte`'s recorder,
  // `Launcher.svelte`'s own hide) meant for something else entirely.
  function onDetailsKeydown(e: KeyboardEvent) {
    if (e.key !== 'Escape' || !open) return;
    e.stopPropagation();
    open = false;
    summaryEl?.focus();
  }

  // The summary drives `open` itself, one-way, rather than trusting the
  // native toggle to report back through `bind:open` — a browser's own
  // "activation behaviour" fires a `toggle` event to say so, and this
  // project's test environment does not implement that event at all, which
  // would leave every OTHER close path here (Escape, an outside focus, a
  // section change) fighting a component state the DOM had already moved
  // past without ever telling it. `preventDefault` refuses the native toggle
  // outright, so there is exactly one writer of `open`.
  function onSummaryClick(e: MouseEvent) {
    e.preventDefault();
    open = !open;
  }

  onMount(() => {
    document.addEventListener('focusin', onDocumentFocusIn);
    document.addEventListener('pointerdown', onDocumentPointerDown);
    return () => {
      document.removeEventListener('focusin', onDocumentFocusIn);
      document.removeEventListener('pointerdown', onDocumentPointerDown);
    };
  });

  // A section change closes the overlay without touching the report itself —
  // `jobs.state` is untouched by a nav click, so the reading block, the
  // embedding block and the continue button are all still there the next
  // time this opens.
  // svelte-ignore state_referenced_locally
  let seenSection = section;
  $effect(() => {
    if (section !== seenSection) {
      seenSection = section;
      open = false;
    }
  });

  // Collapsed again once there is nothing left to show, so the next report —
  // whenever the slot is next claimed — starts closed rather than reopening
  // stale from before.
  $effect(() => {
    if (!anything) open = false;
  });

  // Focus restoration. Runs after every state change (`$jobState` is read
  // for exactly that), by which point Svelte has already applied whatever DOM
  // update the new state called for. `lastFocused` no longer being in the
  // document, with focus having fallen all the way to `<body>` rather than
  // somewhere the person chose, is what tells the two apart: a DOM update
  // that removed the element focus was on, from a person who moved focus
  // away on their own (handled by `onDocumentFocusIn` above, which already
  // cleared `lastFocused` on its way out).
  $effect(() => {
    void $jobState;
    if (lastFocused === null) return;
    if (document.body.contains(lastFocused)) return;
    if (document.activeElement !== document.body) return;
    lastFocused = null;
    // `anything`, not `disclosure !== undefined`: whether Svelte has already
    // reset the `bind:this` reference by the time this runs is an ordering
    // detail between this effect and the `{#if anything}` block's own
    // teardown, and not one this file may lean on — `anything` is the same
    // fact the markup itself gates on, read directly instead.
    if (anything) summaryEl?.focus();
    else focusFallback();
  });
</script>

{#if anything}
  <!-- A live region OUTSIDE `<details>` on purpose: a browser hides every
       child but `<summary>` while the disclosure is closed, taking a nested
       one out of the accessibility tree along with it, so an announcement
       tied to state/phase/result (never to a count tick — `summaryText`
       never reads `counts` at all) needs its own element the collapse cannot
       hide. -->
  <p aria-live="polite" class="sr-only" data-testid="indexing-live">{summaryText}</p>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <details class="job-disclosure" data-testid="indexing" bind:this={disclosure} {open} onkeydown={onDetailsKeydown}>
    <summary data-testid="job-summary" bind:this={summaryEl} onclick={onSummaryClick}>{summaryText}</summary>
    <div class="job-details">
      {#if phase}<ScanProgress {phase} />{/if}
      {#if snapshot.kind === 'running' && snapshot.cancellable}
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
           lead-in of this window's own. -->
      {#if note !== null}
        <p data-testid="indexing-rejection">{note}</p>
      {/if}
    </div>
  </details>
{/if}

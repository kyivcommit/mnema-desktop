<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { locale, t } from '../i18n';
  import Models from './Models.svelte';
  import Folders from './Folders.svelte';
  import Masks from './Masks.svelte';
  import JobStrip from './JobStrip.svelte';
  import Scanning from './Scanning.svelte';
  import Application from './Application.svelte';
  import { createJobController } from './jobs';
  import { modelSettings, type ModelSettings } from '../lib/ipc';

  // All four sections render; hiding one not yet built would make the window
  // claim the product has fewer sections than the spec does. Order matches the
  // spec and the mockup's `snav` column.
  type SectionId = 'models' | 'folders' | 'indexing' | 'application';
  const SECTIONS: SectionId[] = ['models', 'folders', 'indexing', 'application'];

  let section = $state<SectionId>('models');

  // Task 5 — where the bottom disclosure's focus goes when the whole panel
  // disappears out from under it (the job ended with nothing left to say).
  // Scoped to this window's own `.snav`, not a bare `document.querySelector`:
  // a Tauri window is its own document, so nothing outside this one could
  // ever match, but a ref is what says so to a reader instead of asking them
  // to know that about the runtime.
  let navEl: HTMLElement | undefined = $state();
  function focusActiveNav() {
    navEl?.querySelector<HTMLButtonElement>('[aria-pressed="true"]')?.focus();
  }

  // 🔴 ONE controller, here, above every section — not inside the one that
  // starts the job. A controller living in a SECTION dies with that section:
  // `mount` opens a `scan-progress` subscription and `destroy` closes it, so
  // the counters AND the Stop button would go with a nav click. `cancel_job`
  // needs nothing but the command, so that Stop would be lost for nothing.
  //
  // 🔴 F10 (Task 10 live run) did NOT weaken this. `<Folders>` is the one
  // section that now survives a nav change — it stays mounted and `hidden`,
  // see the markup — and it is tempting to read that as "so a controller
  // could live in it after all". It could not: what a section owns is
  // destroyed when the WINDOW decides, and three of the four sections are
  // still torn down by every nav click. The controller is the window's,
  // because the strip that draws it is the window's.
  //
  // The argument used to be the CHANNEL's — a job reported on a channel
  // belonging to whoever started it — and the conclusion outlived it: what
  // dies with a section now is the subscription, not the only way to hear the
  // job. The strip renders outside the panel for the same reason, so a running
  // scan stays visible and stoppable from every section. WHERE outside is the
  // live run's finding 3; see the markup below.
  const jobs = createJobController();

  // The settings window can be opened in the middle of a run it never started,
  // and the tray can start one while it is open. `mount` opens the window's
  // subscription to `scan-progress` and takes the first snapshot; it is
  // synchronous and returns a synchronous teardown, which is exactly what
  // Svelte's `onMount` calls on destroy — an `async` callback would return a
  // promise Svelte keeps and never calls, leaving the listener behind.
  onMount(() => jobs.mount());

  // 🔴 Task 8's controller ruling: `Settings.svelte` is the window's SINGLE
  // reader of `model_settings`, not a second one racing `Scanning.svelte`'s own
  // (Task 7 wrote this window's read alone and left Task 8's own note here
  // saying so — `Scanning.svelte` used to poll on its own mount and its own
  // subscription, with no ordering between the two reads at all). `settings`
  // and `loadError` are what used to live inside that section; they live here
  // now and go down as props, alongside the narrower `read` `JobStrip.svelte`
  // has always taken.
  let settings = $state<ModelSettings | null>(null);
  // A rejected `model_settings`. §10: a rejection arrives as a SENTENCE, never
  // a kind — shown verbatim by `Scanning.svelte`, beside its own catalogue
  // lead-in, and never branched on here.
  let loadError = $state<string | null>(null);

  // A newer request always wins over an older one that resolves later, the
  // same stamp `Models.svelte` and the old `Scanning.svelte` each carried on
  // their own: every call that writes `settings` stamps itself with the
  // sequence current at the moment it was ISSUED, and applies its answer only
  // while that stamp is still the latest. Two reads can be in flight here
  // whenever endings arrive faster than the IPC answers.
  //
  // 🔴 Fix round 1, Minor 1. Restored onto this function, which is where they
  // belong now that the state moved here: `Scanning.svelte`'s own comment on
  // its load-failure banner cites "the ruling recorded beside `refresh()`
  // [here]", and `Application.svelte` cites the same ruling from the other
  // side of the tree — both had been pointing at paragraphs the lift to
  // `Settings.svelte` had dropped.
  //
  // A successful read clears the sentence. Without that line the failure
  // outlives the state it describes: one refused re-read would leave "the
  // state of the index could not be read" standing over numbers that were
  // re-read successfully a second later. Guarded by *a live success after a
  // rejection takes the failure banner away and shows the new numbers*
  // (`Settings.test.ts`, fix round 1, Important 1).
  //
  // The numbers themselves are KEPT across a failed re-read, which is
  // `Tree.svelte`'s ruling and not a new one: a count that was true a moment
  // ago probably still is, and blanking the panel costs a person information
  // they had. What the sentence adds is that it is no longer confirmed.
  // Guarded by *a live rejection of a re-read shows the failure banner beside
  // the numbers it could not confirm* (same file, same round).
  let settingsSeq = 0;

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

  // `read`, derived from `settings` for `JobStrip.svelte`'s narrower need:
  // `continueAction` (`jobs.ts`) falls back to the index's own markers —
  // `scanIncomplete`, `pendingChunks` — exactly when a report names no
  // resumption of its own. `null` until the read answers, and `null` again on
  // a rejection or an `Unreadable` index: all three are the strip's correct
  // degradation to showing only its own row, never a guess about markers this
  // window has not actually read.
  const read = $derived(
    settings !== null && settings.index.kind === 'read' ? settings.index : null,
  );

  onMount(() => {
    // Fired when `scan.readSeq` grows, `scan.files` changes, or `scan.jobsDone`
    // changes — deliberately not "every ending" alone, the rule the old
    // `Scanning.svelte` kept: a reading pass can end and hand the phase to
    // embedding without any job ending at all (`readSeq` is `scan_state.rs`'s
    // own count of reading passes that have ENDED), and that is exactly the
    // moment `scanIncomplete`/`indexedFiles` can have moved. An `embedOnly`
    // run's own ending moves `pendingChunks`/`failedChunks` without moving
    // `readSeq` at all, which is why the count of endings has to trigger this
    // on its own. The other two conditions are documented where they are
    // checked, below.
    //
    // Compared by snapshot IDENTITY, not by kind: the controller replaces the
    // whole state on every change, so a progress tick changes the object
    // without ever being a `readSeq` change or an ending. Seeded with what the
    // store already holds, so mounting this window mid-run does not treat its
    // very first snapshot as a change.
    let seenSnapshot = get(jobs.state).scan.snapshot;
    let seenReadSeq = get(jobs.state).scan.readSeq;
    // 🔴 F1/F9 (Task 10 live run). A folder removal ends the slot with
    // `finish(Terminal::Idle, Some(files))` (`bridge.rs:182`) — straight to a
    // bare `idle` snapshot, never `ended`, and it is not a reading pass so it
    // never bumps `readSeq` either: the two conditions above are both blind to
    // it. `files` (`ScanState.files`, the index's own re-read count) is the
    // field a removal moves WHENEVER it deletes anything (Minor 2, fix round
    // 1 — "always" overclaimed: a removal of a watched folder holding no
    // indexed file leaves `indexed_file_count` where it was, and this trigger
    // rightly does not fire for it, since nothing in the section's numbers
    // changed either). It is watched the same way as the two above — seeded
    // here for the same reason `seenSnapshot`/`seenReadSeq` are.
    let seenFiles = get(jobs.state).scan.files;
    // 🔴 Independent review, finding 3. The fourth trigger USED to be «the
    // snapshot left `running`», and that is an EDGE: it needs the previous
    // snapshot this window saw to have been `running`, and two reachable
    // sequences deny it that. A model adoption can start and end inside the
    // window between `jobs.mount` opening the subscription and its first
    // `jobStatus` answering, so the only snapshot delivered is the terminal
    // one; and a terminal snapshot can arrive AHEAD of the older `running` one
    // that `apply` (`jobs.ts`) then correctly drops. The ending was accepted in
    // both, `readSeq` and `files` stood still, and the section kept the state
    // from before the adoption — no queue row, and nothing offering to continue
    // the embedding (`scanning_continue_embedding`) — until some unrelated scan
    // happened to end.
    //
    // `ScanState.jobsDone` is the same fact stated monotonically: how many jobs
    // have ENDED in this process, moved by `JobSlot::finish` for every terminal
    // and by its `Drop` (`state.rs`). Whatever this window last saw, a number
    // it has not acted on says a job has ended since — there is no edge to
    // miss. Seeded from the store for the reason the three above are: a job
    // that ended before this window existed is not one that ended under it.
    let seenJobsDone = get(jobs.state).scan.jobsDone;
    const stop = jobs.state.subscribe(({ scan }) => {
      if (scan.snapshot === seenSnapshot) return;
      // 🔴 The fourth trigger, and the one that names no fact about which job
      // it was: ANY job having ENDED since this window last acted.
      // `set_embedding_model` claims the single slot as
      // `Other { ModelAdoption }` and, having no `finish`, releases it through
      // `Drop` — which for `Other` writes `Idle`, never `Ended`. It is not a
      // reading pass, so `readSeq` stands still; it deletes no `path` row, so
      // `files` stands still. All three triggers above are blind to it, and
      // what it moves is exactly what this section draws: adopting a different
      // embedding model creates a NEW SPACE, so `pendingChunks` goes from
      // nought to the whole archive and `failedChunks` drops to the new space's
      // nought. The section went on offering nothing over a queue that now
      // covers everything, and showed a stale `indexing_index_failed_chunks`
      // count from the space that had just been retired — correcting itself
      // only when some unrelated scan ended.
      //
      // Written as a count of endings rather than as a list of `OtherJob`s on
      // purpose: it covers `ModelAdoption`, the probe, and any job this
      // application gains later, without this file having to know their names.
      // What it costs is one extra `model_settings` after a probe, which
      // changes nothing and rewrites the same numbers invisibly. The three
      // triggers above are kept: `readSeq` moves mid-run, where no job has
      // ended at all, and `files` moves on an ending this counter also catches
      // — kept because it is the field the SECTION draws, and a re-read owed to
      // a number that changed must not depend on how it came to change.
      const jobsDoneChanged = scan.jobsDone !== seenJobsDone;
      seenJobsDone = scan.jobsDone;
      seenSnapshot = scan.snapshot;
      const readSeqChanged = scan.readSeq !== seenReadSeq;
      seenReadSeq = scan.readSeq;
      const filesChanged = scan.files !== seenFiles;
      seenFiles = scan.files;
      // 🔴 There is no `ended` arm any more, and dropping it is part of the
      // same fix rather than tidying. `ScanSnapshot::Ended` has exactly two
      // writers, `JobSlot::finish` and `JobSlot::drop` (`state.rs`), and each
      // bumps `jobsDone` inside the SAME locked write — so an `ended` snapshot
      // that reaches this window always carries a `jobsDone` it has not acted
      // on, and the arm could no longer be the reason anything happened. A
      // condition that cannot be false reads as a guard and guards nothing.
      //
      // Each of the three left has a state the other two cannot reach:
      // `readSeq` moves mid-run with no job ending, `files` moves without a job
      // at all (`AppState::set_files`, the boot's count), and `jobsDone` moves
      // on every ending including the ones that touch neither.
      if (readSeqChanged || filesChanged || jobsDoneChanged) {
        void refresh();
      }
    });
    void refresh();
    // Returned, so Svelte tears the subscription down when this window closes
    // — this is the WINDOW's own subscription, unlike the per-section ones a
    // nav click destroys and rebuilds, so it lives for as long as `jobs.mount`
    // does above.
    return stop;
  });

  const modelsLabel = $derived.by(() => { void $locale; return t('settings_nav_models'); });
  const foldersLabel = $derived.by(() => { void $locale; return t('settings_nav_folders'); });
  // `settings_nav_scanning`, not `settings_nav_indexing` (Task 8): the LABEL
  // renamed, not the `SectionId` — `'indexing'` is a machine id, not prose, and
  // nothing reads it as a word.
  const scanningLabel = $derived.by(() => { void $locale; return t('settings_nav_scanning'); });
  const applicationLabel = $derived.by(() => { void $locale; return t('settings_nav_application'); });

  function labelFor(id: SectionId): string {
    switch (id) {
      case 'models': return modelsLabel;
      case 'folders': return foldersLabel;
      case 'indexing': return scanningLabel;
      case 'application': return applicationLabel;
    }
  }
</script>

<main>
  <!-- Live run, finding 3 — and Task 5 corrects WHERE outside the section
       conditional actually means. Task 8 read it as "above the pair of
       columns", so a person standing on a section read that section's own
       closing line and, immediately below it, the full indexing report — no
       better than under it, just moved. The strip is the WINDOW's status line,
       not a section's content, and Task 5 turned it into a bottom disclosure —
       one line always on screen, in every section, opened on purpose rather
       than a card permanently taking the window's height — so it is drawn
       AFTER `.scols`, at the bottom, the same place a running job's own Stop
       has to stay reachable from. Nothing about the controller moved: it is
       still created above every section, and `cancel_job` still needs no
       channel.
       ⚠️ Two components sit outside the `{#if}` chain below, and for reasons
       that are not the same one. `<JobStrip>` is outside because it is the
       WINDOW's status line — it must be readable and stoppable from every
       section, and it is drawn once, here, after the pair of columns. The
       folders panel — `<Folders>` and the `<Masks>` editor beside it — is
       outside because of F10: both keep state a person built by hand, so the
       panel stays mounted and is merely `hidden` while another section is
       shown (see it below). Neither is a precedent for the other: a
       section hidden in place still costs its subscriptions and its polling
       for the window's whole life, which is exactly why the other three keep
       their `{#if}`.
       `<Scanning>` is INSIDE the conditional below, and Task 8 changed what
       that placement costs: the re-read of `model_settings` no longer lives in
       that section's own mount at all (`refresh()` above is this window's, not
       a per-section one a nav click destroys and rebuilds) — what a nav change
       still tears down is only `<Scanning>`'s own rendering of whatever `read`
       the window already holds, not the read itself. Hoisting `<Scanning>` out
       here beside `<JobStrip>` would cost something else now: it would show the
       §9.3 numbers over every other section's own content, which nobody asked
       for.
       `.scols` exists so the CSS cannot make this a THIRD column beside the
       nav and the panel: the pair is the row, the bottom disclosure is not
       part of it — it is flex-none, under both, the full width of `main`. -->
  <div class="scols">
    <nav class="snav" bind:this={navEl}>
      {#each SECTIONS as id (id)}
        <button
          type="button"
          class="item"
          data-testid={`settings-nav-${id}`}
          aria-pressed={section === id}
          onclick={() => (section = id)}
        >{labelFor(id)}</button>
      {/each}
    </nav>

    <div class="spane">
      <!-- 🔴 F10 (Task 10 live run), extended to Models by Task 4 (review
           P2-1). Both sections below are MOUNTED for the window's life and
           hidden with the `hidden` attribute, where the remaining two
           (Scanning, Application) are mounted and destroyed by every nav
           click. What a person builds by hand, or a question this build is
           still waiting to hear the answer to, is the reason for each: an
           expanded folder panel is one `list_subfolders` per level and an
           exclude question is a press waiting for an answer; Models carries
           the very same shape of question — an embedding change the person
           has picked but not yet confirmed or cancelled, and (once they have)
           a command whose result is not back yet, or a retirement report and
           a rejection sentence about the one that just landed. Unmounting
           took all of that away without a word: a person who picked a
           different embedding model, looked at Scanning to see the pass
           finish and came back used to meet a fresh `Models` instance with no
           memory of the question or the answer. Scanning and Application are
           not worth this: they draw what a read already answered, and
           re-drawing it costs nothing a person can notice.
           `[hidden]` is the browser's own rule (`display: none`), and it
           takes the section out of the accessibility tree with it, so nothing
           here is read out or reachable by keyboard while another section is
           shown. There is no CSS in this project to say it a second time.
           Two consequences are handled rather than hoped away for Folders,
           and the first is stated narrowly on purpose (fix round 1, Minor 2 —
           an earlier draft said the subscription "keeps it current", which is
           more than the subscription does). `Folders.svelte` re-reads
           `list_tree` on ITS mount, which now happens once per window instead
           of once per visit. What its `jobs.state` subscription adds after
           that is exactly two triggers: a re-read when `readSeq` grows or the
           snapshot becomes `ended`, and a withdrawal of the pending questions
           when `readSeq` grows. Both fire while the section is hidden, which
           is the half this change had to keep. A tick that only moves counts
           fires neither — the one progress tick that is more than that, the
           embedding phase's first `running` snapshot of a `full` scan, is
           exactly where `readSeq` has just grown, so the withdrawal above
           already covers it. Short of that one tick, the counts in this panel
           can be as stale as a reading pass is long, and the pass's ending is
           what re-reads them. That was already true while the section was
           shown; hiding it changes nothing about it, and nothing here claims a
           hidden panel follows a running scan. And its subscription is now
           open for the window's life, which is what a person expects of a
           question that is still waiting for them.
           Models' own subscription (`jobs.state.subscribe`, `Models.svelte`)
           is the same shape again — nothing new is added here to keep it
           firing while hidden, because it already does. A side effect of
           mounting it permanently: its key draft (`draftKey`/`editingKey`)
           now survives a section switch too, the same way the folder tree's
           expanded state and the mask editor's own draft already do — not a
           new store, just the same component staying alive. -->
      <div data-testid="settings-panel-folders" hidden={section !== 'folders'}>
        <h2>{foldersLabel}</h2>
        <Folders {jobs} />
        <!-- Beside the folder list, never inside a folder row (§9.2, D-c): a
             mask is global to the index, so drawing it under one root would
             say it belongs to that root.
             🔴 It takes `jobs`, and the reason is the independent review's
             second finding rather than a control of its own — this editor
             starts no job and stops none. What it needs the store for is the
             same thing `<Folders>` beside it needs it for: a mask question
             carries a `mask_preview` result frozen at the press, and a reading
             pass ending underneath it makes that number wrong. Before this it
             was the one half of the permanently-mounted panel that could not
             hear the pass end.
             🔴 Mounted under the same `hidden` as the folder list, and by the
             owner's ruling on fix round 1 rather than by this file's own
             reading of the scope. The first round left it behind an `{#if}`
             inside the permanent section and wrote down what that cost: a
             mask typed but not yet added, and a question waiting to be
             confirmed, were still lost by a nav click. That is F10's own
             finding one component to the left — the draft is the sharper case,
             because nothing on screen says it went and a person reads the
             empty field as their own mistake — so the editor keeps its state
             the same way the list beside it does. Its `list_masks` therefore
             runs once per window, not once per visit; nothing else changes
             about it. -->
        <Masks {jobs} />
      </div>
      <!-- Task 4, review P2-1: Models joins Folders/Masks above rather than
           the `{#if}` chain below. Its own subscription is `jobs.state.subscribe`
           (`Models.svelte`), no polling of its own — an unconfirmed embedding
           choice, a command still in flight, and a retirement report or
           rejection about one that just landed are all the same class of
           "a question this build is waiting on an answer for" F10 already
           names, and hoisting only their state into `Settings.svelte` (a
           handful of booleans and strings duplicated from the component that
           already owns them) would have been the bigger diff for no extra
           property gained. -->
      <div data-testid="settings-panel-models" hidden={section !== 'models'}>
        <h2>{modelsLabel}</h2>
        <Models {jobs} />
      </div>
      {#if section === 'indexing'}
        <h2>{scanningLabel}</h2>
        <!-- §9.3 — what the index holds, and the one Scan control (Task
             8). `settings`/`loadError` are this window's own read, handed down
             rather than fetched again; `jobs` is for the button and for reading
             the running phase this section gates on. Task 5: the running phase
             itself is now drawn HERE too, through `<ScanProgress>` — the same
             projection the bottom disclosure draws off the same snapshot, not
             a second account of it. -->
        <Scanning {jobs} {settings} {loadError} />
      {:else if section === 'application'}
        <h2>{applicationLabel}</h2>
        <!-- §9.4 — the shortcut, autostart, and the version. It takes no
             `jobs`: this section starts nothing and shares no controller. -->
        <Application />
      {/if}
    </div>
  </div>
  <JobStrip {jobs} {read} {section} focusFallback={focusActiveNav} />
</main>

<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { get } from 'svelte/store';
  import { locale, t } from '../i18n';
  import {
    modelSettings, setKey, forgetKey, providerModels, setChatModel,
    setEmbeddingModel, jobStatus,
    type ModelSettings, type KeyRemoval, type Catalogue,
    type ModelEntry, type ModelRefusal, type UnreadableRecord,
    type ExistingVectors, type RetiredSpace, type ModelRole,
  } from '../lib/ipc';
  import type { JobController } from './jobs';
  import type { ScanSnapshot } from '../lib/ipc';
  // Reused rather than re-derived: `providerReady` is the exact PR 3 ruling
  // this section's green dot owes ("provider + key + a chosen embedding
  // model, fail-safe on null/undefined"), already written and tested for the
  // launcher's Arms row. A second copy of this boolean is the "two truths,
  // one message" class this project has paid for 22 times in one cycle — one
  // of the two would eventually read a fixed set of fields differently.
  import { providerReady } from '../launcher/state';

  // The controller, as a PROP — the same rule `Folders` and `Scanning` already
  // follow, and this section was the one left out. It matters here for two
  // separate things this component used to keep to itself: the pass it starts
  // reports on a channel that belongs to whoever started it (`bridge.rs`), so a
  // listener living in this component is destroyed by the next click on another
  // section while the backend job runs on — the window-level strip stayed idle,
  // and the progress and the Cancel went with the listener.
  //
  // Required, not optional: a section that quietly starts a pass nothing is
  // watching is exactly the shape this is here to remove.
  let { jobs }: { jobs: JobController } = $props();
  // Read once. The controller is created above this component and its identity
  // never changes for the life of the window, which is the whole point of it
  // living there; `$jobState` below is then ordinary auto-subscription.
  // svelte-ignore state_referenced_locally
  const jobState = jobs.state;

  // §9.1, Task 4 — the provider/key row and the platform-dependent note.
  // Task 5 adds the two model tabs, their lists, and the green-dot rule; the
  // full index-state card (documents, last update) is §9.3, PR 9. Reading the
  // fixture question first: `index` is rendered ONLY on its `Unreadable`
  // branch, which carries no `IndexRead` at all — the `Read` branch's numbers
  // are out of scope here by construction, not by an unchecked assumption.
  let settings = $state<ModelSettings | null>(null);
  // Whether the key field is open for editing. Always effectively "open" when
  // there is no key to hide; explicit only for the Present → Change path.
  let editingKey = $state(false);
  // The text a person is currently typing. Cleared the instant it is handed
  // to `setKey`, not when the call resolves — a snapshot taken while the
  // request is still in flight must already show nothing (Step 5).
  let draftKey = $state('');
  // The backend's own sentence for a rejected set_key/forget_key/set_chat_model,
  // shown verbatim beside the control — never branched on, only displayed
  // (§10 / the umbrella rejection rule).
  let actionError = $state<string | null>(null);
  // A rejected read of `model_settings`. `refresh()` below is the only
  // function that ever writes this — every caller (the mount read, the
  // scan-ended re-read, and `commitEmbedding`'s recovery re-read) reports
  // through it by construction, not by each one remembering to. It is
  // bounded but real: the command itself cannot fail (`model_settings`
  // returns `ModelSettings`, not `Result`), so what arrives here is an
  // IPC-layer failure. Held apart from `actionError` because nothing on this
  // screen offers a manual retry for it — the scan-ended re-read is the only
  // thing that ever tries again, on its own schedule, not a button a person
  // presses.
  //
  // A read that succeeds takes this away with it: `refresh()` clears it on
  // its success branch, the same rule `Settings.svelte:95-104` already keeps
  // for its own copy of this state (mutation-guarded there, `pr9-ui.sh`,
  // "a read that succeeds must take the failure sentence away with it"). A
  // mount that fails followed by a later scan-ended re-read that succeeds
  // must not leave a stale "could not be read" sentence beside a panel a
  // newer read has already confirmed — a claim outliving its own guard.
  let loadError = $state<string | null>(null);
  let removal = $state<KeyRemoval['kind'] | null>(null);
  // Task 9 (owner's ruling, live run 2026-09-10): Forget used to destroy the
  // saved credential on the first press. `false` is "no question is being
  // asked" — the same non-modal-confirmation shape `pendingEmbedding` below
  // and Folders' own `removeQuestion` already use for an irreversible press.
  let forgetQuestion = $state(false);
  // Review round 1, Minor 5: true from the moment Confirm starts its
  // `forget_key` round until that round (and the refresh after it) has
  // settled — mirrors `changeBusy` below, for the same reason: a second
  // press cannot start a second, overlapping round while the first is still
  // an open question the backend has not answered.
  let forgetBusy = $state(false);
  // Bound so Cancel can put focus back on the exact control that opened the
  // question, rather than losing it to the document body.
  let forgetButtonEl = $state<HTMLButtonElement | undefined>(undefined);
  // Bound to the whole key group, not to one control inside it: what the
  // group shows NEXT after a confirmed Forget depends on what the re-read
  // found (ordinarily the Absent branch's key field), and the group's own
  // markup is what decides that shape, not this function.
  let keyGroupEl = $state<HTMLDivElement | undefined>(undefined);

  // A newer request always wins over an older one that resolves later — the
  // ordering hazard booked to this task (umbrella `:525`). Every call that
  // writes `settings` stamps itself with the sequence current at the moment
  // it was ISSUED, and only applies its answer while that stamp is still the
  // latest issued: an older `model_settings` read that settles after a
  // `set_chat_model` round has already refreshed the screen must not repaint
  // the model that round just chose, and — the other direction — a
  // `model_settings` read that happens to settle first must not be the last
  // word once `set_chat_model`'s own refresh comes in after it.
  let settingsSeq = 0;

  // §10: a rejection arrives as a sentence, never as a kind, and this is the
  // ONE place that ever happens — the mount read, the scan-ended re-read, and
  // `commitEmbedding`'s recovery re-read all call this same function, so a
  // rejection is reported here regardless of which of them triggered it. A
  // call site's own `.catch(...)` exists only to keep the rethrow below from
  // becoming an unhandled rejection; it never needs to inspect or report the
  // error itself, because by the time it runs `loadError` is already set (or
  // deliberately left alone — see the stamp check).
  //
  // 🔴 Review Critical 1: an earlier version of this reported the error in
  // the CALLER's `.catch`, outside `refresh()`, with no `settingsSeq` stamp of
  // its own — so an OLDER read's rejection could overwrite a NEWER read's
  // success, the rejection-side twin of the resolution ordering guard below.
  // The stamp has to be checked here, inside `refresh()`, because `seq` is
  // this call's own local variable; a caller has no way to know whether it is
  // still the latest by the time its `.catch` runs.
  async function refresh() {
    const seq = ++settingsSeq;
    try {
      const s = await modelSettings();
      if (seq !== settingsSeq) return; // superseded before this reply arrived
      settings = s;
      loadError = null;
      // Task 4: ANY read that succeeds is the newer, more authoritative word
      // — mount's own, a scan-ended re-read, or a mutation's own — so it
      // always reconciles the visible model back onto `settings` and takes
      // a stale `writeOutcome` away with it, not only the read a command
      // itself triggered.
      writeOutcome = null;
      // Review round 1, Important 2: a standing Forget question is a
      // question about THIS key. A read that finds the key no longer
      // `present` — Absent (the ordinary outcome of a confirmed Forget
      // elsewhere) or Unreadable (a keychain relock mid-question) — answers
      // it either way, and leaving `forgetQuestion` true would aim a stale
      // question at whatever key shows up here NEXT, unasked, the moment the
      // group is present-and-not-editing again.
      if (s.key.kind !== 'present') forgetQuestion = false;
    } catch (e) {
      // A superseded read's rejection says nothing about the CURRENT state —
      // a newer read already settled, resolved or refused, and that answer is
      // the one standing. Silently dropped here, the same as the resolution
      // side dropping a superseded `settings = s` two lines up.
      if (seq === settingsSeq) loadError = e instanceof Error ? e.message : String(e);
      throw e; // callers that need the rejection (e.g. commitEmbedding) still get it
    }
  }

  onMount(() => {
    // Empty on purpose: `refresh()` already reported the failure above (when
    // it was not superseded); this only stops the rethrow from surfacing as
    // an unhandled promise rejection.
    refresh().catch(() => {});
    void loadCatalogue('embedding');
    // The index is asked again whenever a scan ends, because an ending is the
    // one moment the counts `degraded` is read from can have changed. The
    // listener that used to do this belonged to a channel this component
    // opened, so it only ever heard a pass THIS section started, and only
    // while this section was mounted. Through the controller it hears every
    // ending — a scan started from Folders or from the tray adds chunks to
    // `totalChunks` and fills the space — which is the same asymmetry argument
    // `Folders.svelte` makes for its own row: a re-read that finds the same
    // numbers rewrites them invisibly, a missed one leaves a falsehood on
    // screen.
    //
    // Compared by snapshot IDENTITY, not by kind: the controller replaces the
    // whole state on every change, so a progress tick changes the object
    // without ever being an ending. Seeded with what the store already
    // holds — this component mounts once for the window's whole life (Task
    // 4, review P2-1: mounted-hidden, the same as Folders), so the seed's
    // only job is to keep the FIRST snapshot this subscription is handed —
    // whatever a scan already in progress happens to be at — from reading as
    // a change from nothing and firing a re-read nobody asked for.
    let seen: ScanSnapshot = get(jobs.state).scan.snapshot;
    return jobs.state.subscribe(({ scan }) => {
      if (scan.snapshot === seen) return;
      seen = scan.snapshot;
      // Empty on purpose, same as the mount call above: `refresh()` already
      // reported the failure; this only stops the rethrow from surfacing as
      // an unhandled promise rejection.
      if (scan.snapshot.kind === 'ended') void refresh().catch(() => {});
    });
  });

  function startEditing() {
    editingKey = true;
    actionError = null;
    removal = null;
  }
  function cancelEditing() {
    editingKey = false;
    draftKey = '';
    // A failed Save leaves its sentence on screen; Cancel takes the field away
    // with it, so the sentence would otherwise sit beside a state it no longer
    // describes.
    actionError = null;
  }

  async function saveKey() {
    const value = draftKey;
    draftKey = ''; // gone from component state before the request even lands
    actionError = null;
    removal = null;
    try {
      await setKey(value);
      editingKey = false;
      await refresh();
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
    }
  }

  // The press asks; it forgets nothing. `forget_key` is called only from the
  // confirm button below.
  function askForget() {
    if (forgetBusy) return;
    actionError = null;
    removal = null;
    forgetQuestion = true;
  }

  async function cancelForget() {
    // Review round 1, Minor 5: guarded the same way `onModelSelect` guards
    // `changeBusy` — a script-dispatched click is not stopped by `disabled`,
    // only a real one is, so the busy check belongs in the handler too.
    if (forgetBusy) return;
    forgetQuestion = false;
    await tick();
    forgetButtonEl?.focus();
  }

  async function confirmForget() {
    if (forgetBusy) return;
    // Left standing (not cleared here) for the whole round: clearing it
    // before the request settles put the ORIGINAL Forget button back on
    // screen while `forget_key` was still in flight, and a second press
    // could start a second, overlapping round with no `settingsSeq`-style
    // stamp to tell which one's answer should win. `forgetBusy` disables
    // both buttons the confirmation shows meanwhile; the question itself
    // closes only once this round is done, in the `finally` below.
    forgetBusy = true;
    actionError = null;
    try {
      const result = await forgetKey();
      removal = result.kind;
      await refresh();
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
    } finally {
      forgetBusy = false;
    }
    forgetQuestion = false;
    // Whatever the group shows now — ordinarily the Absent branch's key
    // field — is where focus belongs next; a stale ref to the button this
    // question replaced would be exactly the "claim outlives its guard"
    // class this project already pays for elsewhere.
    await tick();
    keyGroupEl?.querySelector<HTMLElement>('input, button')?.focus();
  }

  const providerLabel = $derived.by(() => { void $locale; return t('models_provider_label'); });
  // The one guard on this screen no test can tell from its absence: the
  // provider name is the same string in both locales — a brand, not a
  // translation — so removing `void $locale` here changes nothing observable.
  // Kept deliberately, and said out loud rather than left looking defended.
  const providerName = $derived.by(() => { void $locale; return t('models_provider_name'); });
  const keyLabel = $derived.by(() => { void $locale; return t('models_key_label'); });
  const savedLabel = $derived.by(() => { void $locale; return t('models_key_saved'); });
  const absentHint = $derived.by(() => { void $locale; return t('models_key_absent_hint'); });
  const changeLabel = $derived.by(() => { void $locale; return t('models_key_change'); });
  const forgetLabel = $derived.by(() => { void $locale; return t('models_key_forget'); });
  const saveLabel = $derived.by(() => { void $locale; return t('models_key_save'); });
  const cancelLabel = $derived.by(() => { void $locale; return t('models_key_cancel'); });
  const loadFailureLabel = $derived.by(() => { void $locale; return t('models_load_failed'); });
  const indexLabel = $derived.by(() => { void $locale; return t('models_index_label'); });

  const forgetConfirmQuestionLabel = $derived.by(() => { void $locale; return t('models_key_forget_confirm'); });

  const removalLabel = $derived.by(() => {
    void $locale;
    if (removal === 'removed') return t('models_key_removed');
    if (removal === 'nothingToRemove') return t('models_key_nothing_to_remove');
    return null;
  });

  // The index half: rendered on its Unreadable branch alone, from `cause` —
  // never from `reason`, and never by reading into `Read`'s fields, which do
  // not exist on this branch of the type.
  const indexFailure = $derived.by(() => {
    void $locale;
    if (!settings || settings.index.kind !== 'unreadable') return null;
    return settings.index.cause === 'notOpen'
      ? t('models_index_not_open')
      : t('models_index_read_failed');
  });

  // The key half's Unreadable branch: four causes, three actions — Locked
  // names both situations and claims neither, Duplicate and Defect each name
  // one action, and Refused is the one value with no action to name
  // (models.rs:718-746). Exhaustive over the four; `reason` never appears.
  const keyFailure = $derived.by(() => {
    void $locale;
    if (!settings || settings.key.kind !== 'unreadable') return null;
    switch (settings.key.cause) {
      case 'locked': return t('models_key_locked');
      case 'duplicate': return t('models_key_duplicate');
      case 'refused': return t('models_key_refused');
      case 'defect': return t('models_key_defect');
    }
  });

  // Whether the editable field is what's on screen: always for Absent (there
  // is nothing to hide behind a mask), and for Present only once Change
  // was pressed. Unreadable shows neither — the store would not say whether a
  // key exists at all, so offering to add, change or forget one would be a
  // claim this build cannot back.
  const showInput = $derived(
    settings?.key.kind === 'absent' || (settings?.key.kind === 'present' && editingKey),
  );

  // ---------------------------------------------------------------------
  // Task 5 — the two model tabs, their catalogues, and the green-dot rule.
  // ---------------------------------------------------------------------

  type Tab = 'embedding' | 'chat';
  // rerank and verify stay hidden (D123/D124) — two tabs, not four.
  // `set_rerank_model` exists on the Rust side and stays uncalled here.
  let activeTab = $state<Tab>('embedding');
  let catalogues = $state<Record<Tab, Catalogue | null>>({ embedding: null, chat: null });
  let catalogueErrors = $state<Record<Tab, string | null>>({ embedding: null, chat: null });
  // Same ordering hazard as `settings`, one instance per tab: a stale
  // `provider_models` answer for a role a person has since left, and come
  // back to, must not overwrite the one that belongs to the click that is
  // actually still in flight.
  let catalogueSeq: Record<Tab, number> = { embedding: 0, chat: 0 };

  async function loadCatalogue(role: Tab) {
    const seq = ++catalogueSeq[role];
    try {
      const c = await providerModels(role);
      if (seq !== catalogueSeq[role]) return;
      catalogues = { ...catalogues, [role]: c };
      catalogueErrors = { ...catalogueErrors, [role]: null };
    } catch (e) {
      if (seq !== catalogueSeq[role]) return;
      catalogueErrors = { ...catalogueErrors, [role]: e instanceof Error ? e.message : String(e) };
    }
  }

  function selectTab(role: Tab) {
    activeTab = role;
    // A confirmation is about a press on THIS tab. Left standing across a tab
    // change it would sit under the chat list offering to discard embeddings
    // for a model that is not on screen any more.
    pendingEmbedding = null;
    // Same rule, Task 9: the Forget question is about the key group, which
    // stands regardless of which tab is open — but a tab switch is still the
    // existing rule for a standing question, so it closes here too rather
    // than surviving under a role it was never asked about.
    forgetQuestion = false;
    // And so are the two sentences a press leaves behind. "The change discarded
    // 4 embeddings…" and a rejection are reports on an act performed from the
    // embedding list; under the chat list they are a report about nothing the
    // person can see. The degraded notice deliberately stays: it is a fact about
    // the product's state rather than about a press, and it carries the one
    // control that repairs it.
    retiredReport = null;
    changeError = null;
    jobRunning = false;
    // Nothing to reset for the re-embedding pass any more: it is read from the
    // controller, and a tab click does not end a job. A local flag cleared here
    // said "no pass is running" about a pass that was.
    void loadCatalogue(role);
  }

  async function chooseChatModel(model: string) {
    actionError = null;
    changeBusy = true;
    // Command and refresh get their OWN try/catch (review P2-1/umbrella):
    // a command that SUCCEEDS and is followed by a refresh that THROWS is
    // not a rejected choice, and must not be reported as one. No manual
    // `settingsSeq` bump here: it is set, synchronously, on `refresh()`'s
    // own first line right below — with no `await` between this function
    // trusting its own result and that bump, no read already in flight can
    // land in the gap and be mistaken for the newer word.
    try {
      await setChatModel(model);
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
      writeOutcome = { kind: 'unknown', role: 'chat' };
      // `refresh()` already reported this through `loadError`, and — on
      // success — already reconciled `writeOutcome` away with the fresh
      // answer; nothing here needs to inspect which happened.
      await refresh().catch(() => {});
      changeBusy = false;
      return;
    }
    writeOutcome = { kind: 'acknowledged', role: 'chat', model };
    await refresh().catch(() => {});
    changeBusy = false;
  }

  // The index's own answer, or `null` when it had none to give — one binding
  // rather than the same `kind === 'read'` test written at each field, because
  // Task 6 reads four of them and a fifth reader spelling the test slightly
  // differently is how two of these end up disagreeing.
  const indexRead = $derived(settings && settings.index.kind === 'read' ? settings.index : null);
  const indexUnreadableCause = $derived(
    settings && settings.index.kind === 'unreadable' ? settings.index.cause : null,
  );
  const currentEmbeddingModel = $derived(indexRead ? indexRead.embeddingModel : null);
  // `?? null`: the field is optional on the wire type (see `ipc.ts`), and
  // "not stated by this fixture" and "the index says no chat model" read the
  // same way here — neither marks anything as chosen.
  const currentChatModel = $derived(indexRead ? (indexRead.chatModel ?? null) : null);

  // Task 4 — after an IPC round the sources of truth are its OWN confirmed
  // result and the next read, never a value this component cached before
  // either happened (§10, and the umbrella's "an error does not mean a model
  // rollback"). `acknowledged` is what a successful `set_*_model` reply just
  // confirmed — shown even while the read that follows it has not landed, or
  // has failed outright — and `unknown` is what a REJECTED command leaves:
  // the old model is not assumed to have survived a failed adoption either
  // (`set_embedding_model`'s own doc names the state a failed one can leave
  // behind), so nothing is shown as current until a read says so.
  //
  // Cleared the moment a read SUCCEEDS, whichever branch set it: a
  // successful `model_settings` is always the newer, more authoritative
  // answer (`settingsSeq` below still drops a stale one), so from that
  // point on the visible model is `settings`' own again, not a memory of
  // what a command claimed.
  type WriteOutcome =
    | null
    | { kind: 'acknowledged'; role: ModelRole; model: string }
    | { kind: 'unknown'; role: ModelRole };
  let writeOutcome = $state<WriteOutcome>(null);
  // True from the moment a model choice starts its command until the read
  // that reconciles it (or fails to) has settled — the select stays
  // `disabled` for that whole stretch, so a second choice cannot start while
  // the first is still an open question the backend has not answered.
  let changeBusy = $state(false);

  function visibleModelFor(role: Tab): string | null {
    if (writeOutcome && writeOutcome.role === role) {
      return writeOutcome.kind === 'acknowledged' ? writeOutcome.model : null;
    }
    return role === 'chat' ? currentChatModel : currentEmbeddingModel;
  }
  const visibleEmbeddingModel = $derived(visibleModelFor('embedding'));
  const visibleChatModel = $derived(visibleModelFor('chat'));
  const visibleActiveModel = $derived(activeTab === 'chat' ? visibleChatModel : visibleEmbeddingModel);

  // Whether an index READ actually said "no role model" (`notChosen`) or
  // this build cannot currently say either way (`unknown` — no read has
  // landed, the index is unreadable, or the last command's own outcome is
  // itself unknown). The select's placeholder option reads one of the two;
  // conflating them would claim "nothing is set" about a state this build
  // cannot see into at all.
  function statusFor(role: Tab): 'notChosen' | 'unknown' {
    if (writeOutcome && writeOutcome.role === role && writeOutcome.kind === 'unknown') return 'unknown';
    if (!settings || settings.index.kind !== 'read') return 'unknown';
    return 'notChosen';
  }

  // The per-role configured dot (P2-1's "configuration_dots_use_their_own_role"):
  // green needs a present key, a READ index, and THIS role's own model — read
  // straight off `visibleModelFor`, not off a shared "ready" boolean, so a
  // dot never answers for a role it was not drawn for. `loadError` and a
  // post-mutation `unknown` both fail it safe rather than green: neither is
  // grounds to claim a role is configured.
  function dotState(role: Tab): 'true' | 'false' | 'unknown' {
    if (writeOutcome && writeOutcome.role === role && writeOutcome.kind === 'unknown') return 'unknown';
    if (!settings || loadError) return 'unknown';
    if (settings.key.kind !== 'present') return 'false';
    if (settings.index.kind !== 'read') return 'unknown';
    return visibleModelFor(role) !== null ? 'true' : 'false';
  }
  const embeddingDotState = $derived(dotState('embedding'));
  const chatDotState = $derived(dotState('chat'));

  function dotLabel(state: 'true' | 'false' | 'unknown'): string {
    if (state === 'true') return t('models_dot_configured');
    if (state === 'false') return t('models_dot_not_configured');
    return t('models_dot_unknown');
  }
  const embeddingDotLabel = $derived.by(() => { void $locale; return dotLabel(embeddingDotState); });
  const chatDotLabel = $derived.by(() => { void $locale; return dotLabel(chatDotState); });

  const ready = $derived(!!settings && providerReady(settings));
  const readyLabel = $derived.by(() => { void $locale; return t('models_status_ready'); });
  const notReadyLabel = $derived.by(() => { void $locale; return t('models_status_not_ready'); });

  const embeddingTabLabel = $derived.by(() => { void $locale; return t('models_tab_embedding'); });
  const chatTabLabel = $derived.by(() => { void $locale; return t('models_tab_chat'); });

  const activeCatalogue = $derived(catalogues[activeTab]);
  const activeCatalogueError = $derived(catalogueErrors[activeTab]);

  // `catalogue.rs`'s `Refusal`, exhaustively: a fixed catalogue sentence per
  // variant, never the provider's own `raw` text — one of the five variants
  // (`limitNotUnderstood`) carries one, and this project treats
  // provider-sourced strings as untrusted wherever they would otherwise be
  // rendered (`catalogue.rs`'s own doc on `InputLimit::NotUnderstood`/
  // `Price::NotAPrice`/`Refusal::LimitNotUnderstood`), so the sentence names
  // the SITUATION and stops there, the same choice already made for
  // `KeyState::Unreadable.reason`.
  //
  // Owner's ruling (live run, 2026-09-10): a refused entry used to render as
  // a disabled option with this sentence folded into its own label. It no
  // longer reaches the select at all (`selectableEntries` below drops it
  // outright) — instead every entry sharing this same reason is folded into
  // one count, and this sentence — with that count — is what
  // `hiddenReasons` puts on its own line under the select.
  //
  // The `never` arm is exhaustiveness twice over: `tsc` refuses to compile if
  // a variant is added to `ModelRefusal` without a matching `case` here, and
  // a value that reaches this function some other way — the catalogue mirror
  // test below constructs one from a Rust source list, not from this
  // union — throws instead of silently falling through, so a sixth variant
  // added to `catalogue.rs` cannot pass this section silently in either
  // direction.
  function hiddenReasonLabel(r: ModelRefusal, count: number): string {
    switch (r.kind) {
      case 'inputTooSmall':
        return t('models_hidden_input_too_small', { count, floor: r.floor });
      case 'noStatedLimit':
        return t('models_hidden_no_stated_limit', { count });
      case 'limitNotUnderstood':
        return t('models_hidden_limit_not_understood', { count });
      case 'noStatedOutputModalities':
        return t('models_hidden_no_stated_output_modalities', { count });
      case 'noTextOutput':
        return t('models_hidden_no_text_output', { count });
      default: {
        const exhaustive: never = r;
        throw new Error(`unhandled model refusal kind: ${(exhaustive as { kind: string }).kind}`);
      }
    }
  }

  // The group two refusals fold into: the SAME kind, and — for
  // `inputTooSmall` alone — the SAME floor. The owner's ruling states floor
  // explicitly ("entries with different floors are different reasons"),
  // never limit, so a build-wide floor stays one line even across many
  // entries while a fixture that gives two different floors is read as two
  // distinct reasons rather than one that would silently report only
  // whichever floor it happened to see first.
  function hiddenReasonKey(r: ModelRefusal): string {
    return r.kind === 'inputTooSmall' ? `inputTooSmall:${r.floor}` : r.kind;
  }

  // `catalogue.rs`'s `RecordId`, exhaustively — same shape as
  // `hiddenReasonLabel` and for the same reason: `Absent`, `NotAString` and
  // `Known` are three different facts about a record this build could not
  // turn into a model, and folding them together would be false about at
  // least one of them.
  function unreadableRecordLabel(rec: UnreadableRecord): string {
    switch (rec.id.kind) {
      case 'absent':
        return t('models_catalogue_unreadable_record_absent', { index: rec.index });
      case 'notAString':
        return t('models_catalogue_unreadable_record_not_a_string', { index: rec.index });
      case 'known':
        return t('models_catalogue_unreadable_record_known', { index: rec.index, id: rec.id.id });
      default: {
        const exhaustive: never = rec.id;
        throw new Error(`unhandled record id kind: ${(exhaustive as { kind: string }).kind}`);
      }
    }
  }

  // Review round 1, Minor 3: `selectableEntries` and `hiddenReasons` used to
  // read this off two SEPARATE conditions (`refusal === null` and
  // `!entry.refusal`) that only happen to agree because `ModelEntry.refusal`
  // is typed as `ModelRefusal | null` and every object is truthy — any wire
  // value this build did not expect (an empty object, say) would hide the
  // entry from the select while `hiddenReasons` silently skipped it too,
  // with no line explaining where it went. One predicate now, read by both.
  function isRefused(entry: ModelEntry): entry is ModelEntry & { refusal: ModelRefusal } {
    return entry.refusal !== null;
  }

  // The options the select actually offers: unrefused entries only.
  // Indifferent to locale — an entry's `refusal` field does not change on a
  // language switch, so unlike `hiddenReasons` below this needs no
  // `void $locale` of its own to stay live.
  const selectableEntries = $derived.by(() => {
    const cat = activeCatalogue;
    if (!cat) return [];
    return cat.entries.filter((entry: ModelEntry) => !isRefused(entry));
  });

  // `void $locale` here, not on `hiddenReasonLabel`/`unreadableRecordLabel`
  // themselves: those are plain functions called from markup, and Svelte's
  // fine-grained reactivity only re-runs an expression when a signal IT reads
  // changes — `entry.refusal` does not change on a language switch, so a
  // `$derived` reading `$locale` is what makes the surrounding recomputation
  // happen at all (`t()` itself reads `get(locale)` non-reactively,
  // `i18n/index.ts:11`).
  //
  // Grouped by `hiddenReasonKey`. Two records sharing an id both count, same
  // as they both would have as two separate disabled options before this
  // ruling — no dedup here either.
  const hiddenReasons = $derived.by(() => {
    void $locale;
    const cat = activeCatalogue;
    if (!cat) return [];
    const groups = new Map<string, { refusal: ModelRefusal; count: number }>();
    cat.entries.forEach((entry: ModelEntry) => {
      if (!isRefused(entry)) return;
      const key = hiddenReasonKey(entry.refusal);
      const existing = groups.get(key);
      if (existing) existing.count += 1;
      else groups.set(key, { refusal: entry.refusal, count: 1 });
    });
    // Order: count descending — the owner's ruling. Ties keep first-
    // appearance order for free and need no field or comparator of their
    // own for it (review round 1, Minor 4): `Map` iterates in insertion
    // order, and `Array.prototype.sort` has been a STABLE sort since
    // ES2019 (all engines this build targets), so entries tied on count
    // never move relative to each other.
    return [...groups.entries()]
      .sort(([, a], [, b]) => b.count - a.count)
      .map(([key, g]) => ({ key, label: hiddenReasonLabel(g.refusal, g.count) }));
  });

  const activeUnreadableRecords = $derived.by(() => {
    void $locale;
    const cat = activeCatalogue;
    if (!cat) return [];
    return cat.unreadableRecords.map((rec) => ({ rec, label: unreadableRecordLabel(rec) }));
  });

  // ---------------------------------------------------------------------
  // Task 6 — choosing an embedding model: the one act on this screen that
  // destroys data on purpose, and the four things it owes afterwards.
  // ---------------------------------------------------------------------

  // The model a press has proposed and nobody has confirmed yet. `null` is
  // "no question is being asked", which is also the state a cancel returns to.
  let pendingEmbedding = $state<string | null>(null);
  // What the change reported it actually threw away — `AdoptedModel.retired`,
  // measured by the index at the moment of destruction. A different number
  // from the estimate shown before the act, and never a re-rendering of it.
  let retiredReport = $state<RetiredSpace[] | null>(null);
  // The backend's own sentence for a rejected `set_embedding_model` or
  // `start_scan_job`, shown verbatim and never branched on.
  let changeError = $state<string | null>(null);
  // What `job_status` answered after a rejection. Read, never inferred from
  // the sentence: a rejection crosses the IPC as text, and `claim_job` is a
  // compare-and-exchange that leaves the running job's owner untouched — so a
  // refusal must not draw that job as cancelled or finished.
  let jobRunning = $state(false);
  // Where the embedding phase has got to, read off the CONTROLLER rather than
  // held here. Two things follow, and both are the point. It survives a section
  // switch, because the controller does — the pass and the sentence about it no
  // longer disappear from a window whose backend is still running the job. And
  // it is true of a scan this section did not start: the sentences below are
  // about the state of the index, not about who pressed what, so a scan started
  // from Folders or from the tray is the same news to a person standing here.
  const snapshot = $derived($jobState.scan.snapshot);
  const embedPassUnderWay = $derived(
    snapshot.kind === 'running' && snapshot.phase.kind === 'embedding',
  );
  // `endedIn` and not the embedding OUTCOME: a scan stopped inside the phase
  // embedded nothing and still got that far, which is the news this sentence
  // is about (`scan_state::EmbedOutcome`'s own warning).
  const embedPassEnded = $derived(snapshot.kind === 'ended' && snapshot.report.endedIn === 'embedding');

  function chooseEmbeddingModel(model: string) {
    // The model the SELECT is currently showing is not a change — compared
    // against `visibleEmbeddingModel`, not `currentEmbeddingModel` (review,
    // Important 1). The latter is only the LAST READ: after an acknowledged
    // adoption whose own re-read then failed, `settings` still names the OLD
    // model while the select (rightly) shows the new one, so comparing
    // against `settings` would treat a pick of the OLD model as "no change"
    // and silently swallow it — and after a rejection whose own re-read also
    // failed, the select shows blank while `settings` still names whatever it
    // last confirmed, so comparing against `settings` would treat a pick of
    // THAT model as a no-op too, when this build does not actually know the
    // select agrees. Checked BEFORE anything is cleared, for the same
    // reason: a pick that turns out to be a real no-op must leave the
    // previous round's own report and error exactly as they were, not wipe
    // them first and decide afterwards that nothing needed to happen.
    if (model === visibleEmbeddingModel) return;
    changeError = null;
    retiredReport = null;
    // With no readable index there is no estimate to state, so there is
    // nothing to confirm — and `Keep` is the value that refuses rather than
    // destroys, so pressing cannot cost anything. This is the recovering act
    // `set_embedding_model`'s own doc names for the state a failed adoption
    // leaves behind: choosing a model again succeeds and rewrites the pointer.
    //
    // An index that holds no embeddings ANYWHERE takes the same path, and for
    // the same reason: there is nothing to lose, so there is nothing to ask
    // about. `Keep` is what goes on the wire —
    // `Db::refuse_unless_every_other_space_is_empty` refuses it exactly when
    // some other space is non-empty, which is the count this branch has just
    // read as zero, so the value that would refuse is the value that cannot
    // refuse here.
    if (!indexRead || estimatedEmbeddings === 0) {
      void commitEmbedding(model, 'keep');
      return;
    }
    pendingEmbedding = model;
  }

  // The change handler for the native `<select>` — the SAME mechanism
  // `Application.svelte`'s `onLanguageSelect` uses, and for the same reason
  // (read that handler's own comment): Svelte's `value={selectValue}`
  // compiles to a dirty check against the LAST value Svelte itself wrote, not
  // against what the DOM currently shows, so a pick this build does not
  // confirm (Cancel, a rejection, an unreadable read) would otherwise leave
  // the user's own click showing on screen forever — and a SECOND pick of
  // that same candidate would then fire no `change` event at all, since the
  // DOM element's `.value` is already equal to it (review P2-2). Reading the
  // candidate and writing `currentTarget.value` back to the CONFIRMED value
  // immediately, before either model function is even called, keeps the DOM
  // in sync on every change regardless of what Svelte thinks moved; the
  // candidate itself lives only in `pendingEmbedding` (or is handed straight
  // to `chooseChatModel`) until an IPC round says otherwise.
  function onModelSelect(e: Event) {
    const select = e.currentTarget as HTMLSelectElement;
    const candidate = select.value;
    select.value = selectValue;
    // `disabled` is the ordinary guard a person meets; it does not stop a
    // script-dispatched `change` (disabled only withholds events the BROWSER
    // would generate from a real interaction). Checked here too, so a second
    // write genuinely cannot start while the first command is still an open
    // question the backend has not answered — including across a section
    // switch, since this component now survives one (review P2-1).
    if (changeBusy) return;
    if (activeTab === 'chat') {
      void chooseChatModel(candidate);
    } else {
      chooseEmbeddingModel(candidate);
    }
  }

  const selectValue = $derived(visibleActiveModel ?? '');
  const selectionLabel = $derived.by(() => { void $locale; return t('models_selection_label'); });

  // The placeholder option this select needs whenever its own catalogue
  // entries cannot stand for the current state on their own — never a
  // fallback onto the first `<option>`, which is the native element's own
  // default and would silently claim that entry chosen
  // (`unknown_model_is_not_the_first_option`). Two different situations
  // reach it, and they are not the same claim: no confirmed id at all (its
  // own label distinguishes "the index read says none" from "this build
  // cannot currently tell"), or a confirmed id the active catalogue does not
  // list — a stale pointer at a model this provider stopped naming, which
  // still deserves its OWN id on screen rather than vanishing into the same
  // blank placeholder as "nothing chosen". A THIRD way to reach it as of the
  // owner's ruling above: a confirmed id the catalogue still lists, but now
  // refused — `selectableEntries` has already dropped it, so it is
  // indistinguishable from a retired model here, and correctly so (both are
  // "this build cannot offer it"); `hiddenReasons` below is what still says
  // why, on its own line.
  const selectPlaceholder = $derived.by(() => {
    void $locale;
    const model = visibleActiveModel;
    if (model === null) {
      const status = statusFor(activeTab);
      return { value: '', label: status === 'unknown' ? t('models_selection_unknown') : t('models_selection_not_chosen') };
    }
    if (selectableEntries.some((entry) => entry.id === model)) return null;
    return { value: model, label: t('models_selection_absent', { id: model }) };
  });

  async function commitEmbedding(model: string, existingVectors: ExistingVectors) {
    pendingEmbedding = null;
    changeError = null;
    changeBusy = true;
    // Command and refresh get their OWN try/catch: a command that SUCCEEDS
    // and is followed by a refresh that THROWS is not a rejected write, and
    // `writeOutcome` must go on showing the model it just acknowledged, not
    // fall back to whatever `settings` held before it (review P2-1,
    // `successful_write_with_failed_refresh_keeps_acknowledged_model`).
    try {
      const adopted = await setEmbeddingModel(model, existingVectors);
      retiredReport = adopted.retired;
      jobRunning = false;
      writeOutcome = { kind: 'acknowledged', role: 'embedding', model };
      await refresh().catch(() => {});
    } catch (e) {
      changeError = e instanceof Error ? e.message : String(e);
      // A rejected adoption does not restore the model this build cached
      // from before it either (`failed_adoption_does_not_restore_cached_model`):
      // `set_embedding_model`'s own doc names a state a failed adoption can
      // leave behind where the old space is already gone, so nothing is
      // shown as current until the read below says what actually is.
      writeOutcome = { kind: 'unknown', role: 'embedding' };
      // §10: a rejection arrives as a sentence, not as a kind. What the screen
      // says next is decided by re-reading the state — `model_settings` for
      // what the index is in, `job_status` for whether a job is still going —
      // and never by matching on the message text. `refresh()` already
      // reported the rejection through `loadError`, and — on success —
      // already reconciled `writeOutcome` away with the fresh answer.
      await refresh().catch(() => {});
      jobRunning = await jobStatus()
        .then((s) => s.snapshot.kind === 'running')
        .catch(() => false);
    }
    changeBusy = false;
  }

  // 🔴 Through the controller, not through `startScanJob` directly. The scan
  // this section starts is a job like any other: it belongs on the window's
  // strip, where its progress and its Stop stay reachable from every section.
  // Started here with a listener of this component's own — which is how it was
  // done before the channel went — it reported to nobody the moment somebody
  // clicked another section: the strip stayed idle while the backend job ran
  // on, and there was no way to stop it.
  //
  // Nothing is caught here. A refusal is the controller's to report, in the
  // same words and the same place as every other refused command; a second
  // sentence about it beside the button would be the same truth written twice,
  // and the `job_status` re-read this used to do is the controller's job too.
  async function reembed() {
    // The report of the change that CAUSED this state is about the previous
    // act; once a repair has been asked for, its failure sentence is stale.
    changeError = null;
    // `embedOnly` is the entry point for exactly this: an index whose reading
    // pass is done and whose chunks are not embedded (`scan_state::Entry`).
    // Re-reading every folder to reach the same queue would be work nobody
    // asked for.
    await jobs.scan('embedOnly');
  }

  // **The number before the act, and it is `embeddedChunksEverywhere`.** Not
  // `embeddedChunks`, which counts the active space alone: the change retires
  // every space in its way, and a space abandoned by an earlier change still
  // holds whatever it held — so the active count understates the bill by
  // exactly the spaces it forgets. `models.rs` says so on the field itself,
  // and `the_settings_tell_the_active_space_apart_from_the_whole_index` is
  // where the two numbers are held apart.
  const estimatedEmbeddings = $derived(indexRead ? indexRead.embeddedChunksEverywhere : 0);

  // Semantic search is dark: the index points at a vector space, it holds
  // documents, and that space holds nothing for them.
  //
  // 🔴 **Every conjunct is read from the backend, and that is the fix.** This
  // used to open with `changeLanded`, a flag set when a change landed IN THIS
  // COMPONENT — and this component is destroyed by every section switch. A
  // person who discarded their vectors, went to Folders and came back met a
  // fresh instance with the flag at false and an index the backend still
  // reported as empty: the warning and the button that repairs it were gone,
  // and the connected dot was left as the last word on a search that had gone
  // dark. Derived from durable state instead, the window can re-read the fact
  // rather than having to remember it — through a remount, through a reopened
  // window, through a change made in an earlier session.
  //
  // `currentEmbeddingModel !== null` is "an active space exists": `models.rs`
  // warns on `embedded_chunks` itself that zero with no active space means the
  // question does not arise rather than that nothing is embedded, and
  // `read_settings` derives the model and the space from one call, so this
  // field is that test. `totalChunks > 0` is the other half — an index holding
  // nothing has nothing to embed, and its empty space is not a loss.
  //
  // It is WIDER than the flag was, deliberately: an index with a model, with
  // documents, and with no vectors is dark whether this session made it so or
  // not, and the sentence and the repair are right in every one of those
  // states. The flag was narrower only by being forgetful.
  const degraded = $derived(
    !!indexRead && currentEmbeddingModel !== null
    && indexRead.totalChunks > 0 && indexRead.embeddedChunks === 0,
  );

  const confirmLabels = $derived.by(() => {
    void $locale;
    return {
      title: t('models_embedding_confirm_title'),
      estimate: t('models_embedding_confirm_estimate', { count: estimatedEmbeddings }),
      // The loss, named BEFORE it happens, which is the whole question this
      // window is here to answer honestly. The same fact used to be stated only
      // by `models_embedding_degraded`, rendered after the irreversible act —
      // a report, not a warning.
      loss: t('models_embedding_confirm_loss'),
      discard: t('models_embedding_discard'),
      cancel: t('models_embedding_cancel'),
    };
  });

  // The sentence AFTER the act, built from what the index measured as it
  // destroyed things — never from `estimatedEmbeddings`, which was read at a
  // different moment and about a different question.
  const retiredLabel = $derived.by(() => {
    void $locale;
    if (!retiredReport) return null;
    if (retiredReport.length === 0) return t('models_embedding_retired_none');
    const count = retiredReport.reduce((n, r) => n + r.embeddedChunks, 0);
    return t('models_embedding_retired', { count, spaces: retiredReport.length });
  });

  const degradedLabels = $derived.by(() => {
    void $locale;
    return {
      sentence: t('models_embedding_degraded'),
      reembed: t('models_embedding_reembed'),
      started: t('models_embedding_reembed_started'),
      // Only ever rendered inside the degraded block, which is what makes the
      // second half of this sentence true whenever it is on screen: a pass that
      // had filled the space would have cleared `degraded` and taken the
      // sentence with it.
      ended: t('models_embedding_reembed_ended'),
    };
  });

  const changeFailureLabel = $derived.by(() => {
    void $locale;
    return t('models_embedding_change_failed');
  });
  const jobRunningLabel = $derived.by(() => {
    void $locale;
    return t('models_job_running');
  });
  const indexRecoverLabel = $derived.by(() => {
    void $locale;
    return t('models_index_recover');
  });

  // A stated zero is never a promise the list is complete (umbrella `:529`) —
  // this sentence is the promise, and it is the one thing on this branch that
  // must NOT render when the count is zero.
  const unreadableSentence = $derived.by(() => {
    void $locale;
    const cat = activeCatalogue;
    if (!cat || cat.unreadable === 0) return null;
    return t('models_catalogue_unreadable', { count: cat.unreadable });
  });

  // Review P2-10: this sentence used to be conditioned on `entries.length === 0`
  // alone, so a catalogue of two records this build could not read rendered
  // "2 records could not be read." and then "The provider does not currently
  // list any models for this role." — untrue about the provider, who sent two,
  // and it sends the person to look at the provider instead of at the defect.
  // The sentence is a claim about what the provider listed, so it may only be
  // made when nothing was dropped on the way here.
  const emptyCatalogueSentence = $derived.by(() => {
    void $locale;
    const cat = activeCatalogue;
    if (!cat || cat.entries.length !== 0 || cat.unreadable !== 0) return null;
    return t('models_catalogue_empty');
  });
</script>

{#if settings}
  <div class="row">
    <label for="model-provider">{providerLabel}</label>
    <select id="model-provider" disabled>
      <option selected>{providerName}</option>
    </select>
  </div>
  <!-- Step 5, and the review that followed it: the section is grouped by
       subject, and the one sentence a person can act on comes before the ones
       they cannot. The Key group is second, immediately under the provider it
       belongs to; the Index group is LAST, because its only sentence is a
       defect report nobody reading this window can act on. The previous order
       put that defect report between the provider row and the key rows, and
       left the single actionable instruction at the bottom, unlabelled. -->
  <div class="group" data-testid="model-key-group" bind:this={keyGroupEl}>
    <!-- One occurrence of the word, not two: when the editable field is on
         screen the group's subject heading IS that field's label, so
         `getByLabelText('Key')` still resolves to exactly one control. -->
    {#if showInput}
      <label class="fl" for="model-key-input" data-testid="model-key-label">{keyLabel}</label>
    {:else}
      <span class="fl" data-testid="model-key-label">{keyLabel}</span>
    {/if}
    <!-- The sentence about the key's state, directly under its own subject. -->
    {#if keyFailure}
      <p data-testid="model-key-failure">{keyFailure}</p>
    {:else if settings.key.kind === 'absent'}
      <p data-testid="model-key-absent-hint">{absentHint}</p>
    {:else if settings.key.kind === 'present' && !editingKey}
      <span data-testid="model-key-saved">{savedLabel}</span>
    {/if}
    {#if showInput}
      <div class="row">
        <input id="model-key-input" type="password" bind:value={draftKey} />
        <button type="button" onclick={saveKey}>{saveLabel}</button>
        {#if settings.key.kind === 'present'}
          <button type="button" onclick={cancelEditing}>{cancelLabel}</button>
        {/if}
      </div>
    {:else if forgetQuestion && !keyFailure}
      <!-- Task 9: the same non-modal-confirmation shape as Folders'
           `removeConfirm` and the embedding discard question below — a
           sentence, then the one act and the one refusal, no third option
           between them. Replaces the Change/Forget row rather than sitting
           beside it: both buttons say "Forget" (the owner's ruling states the
           word verbatim for each), and a row that kept the original visible
           too would leave two controls on screen answering to the same name.

           Review round 1, Important 1: `&& !keyFailure`, not merely ordered
           after it — a re-read that turns the key Unreadable while this
           question stands (a keychain relock, `refresh()` runs on every
           scan-ended) must not go on offering a live `forget_key` button over
           a state Unreadable's OWN branch says nothing may be offered for.
           `refresh()` already clears `forgetQuestion` on exactly that
           transition (Important 2) — this guard is the belt to that braces:
           the render itself cannot show this block for a key state it was
           never asked about, regardless of whether every future writer of
           `forgetQuestion` remembers to. -->
      <div class="group">
        <p data-testid="model-key-forget-confirm-question">{forgetConfirmQuestionLabel}</p>
        <div class="row">
          <button
            type="button"
            data-testid="model-key-forget-confirm"
            disabled={forgetBusy}
            onclick={confirmForget}
          >{forgetLabel}</button>
          <button
            type="button"
            data-testid="model-key-forget-cancel"
            disabled={forgetBusy}
            onclick={cancelForget}
          >{cancelLabel}</button>
        </div>
      </div>
    {:else if !keyFailure}
      <!-- Unreadable offers nothing to press: the store would not say whether a
           key exists at all, so add/change/forget would be a claim this build
           cannot back. -->
      <div class="row">
        <button type="button" onclick={startEditing}>{changeLabel}</button>
        <button
          type="button"
          bind:this={forgetButtonEl}
          disabled={forgetBusy}
          onclick={askForget}
        >{forgetLabel}</button>
      </div>
    {/if}
    {#if removalLabel}<p data-testid="model-key-removal">{removalLabel}</p>{/if}
  </div>
{/if}
<!-- Outside `{#if settings}` on purpose: a mount that rejects never sets
     `settings`, and an error paragraph inside that block could not be shown in
     exactly the case it exists for. -->
{#if loadError}
  <p data-testid="model-load-failure">{loadFailureLabel}</p>
  <p data-testid="model-load-reason">{loadError}</p>
{/if}
{#if actionError}<p data-testid="model-action-error">{actionError}</p>{/if}

<!-- Task 5 — outside `{#if settings}` too: `provider_models` is public and
     needs neither a key nor an open index, so browsing and choosing a chat
     model does not have to wait on either. -->
<div class="mtabs">
  <!-- Task 9 (owner's ruling, live run 2026-09-10): the dot moves inside its
       own tab button, as its LAST child, and the state word it used to show
       beside the tab goes `sr-only` (`settings.css:106`) rather than off the
       button entirely — the button's own accessible name still reads
       "Embedding, Configured", it is just nobody sighted who reads the second
       half any more. The per-role predicate itself is unchanged (review
       P2-1): read off `visibleModelFor('embedding')` alone, never off the
       combined `ready` boolean below, which answers for the ACTIVE role only.

       Review round 1, Important 3: the sr-only span's own text is `, {label}`,
       not the bare word — without a literal separator the accessible name ran
       the two halves together with nothing between them ("EmbeddingConfigured"),
       which is not the sentence the comment above already claimed.

       Review round 1, Minor 4: the visible label is its own `<span
       class="mtab-label">` now, so a test can assert what a SIGHTED reader
       sees, exactly, apart from the accessible-only half beside it. -->
  <button
    type="button"
    class="mtab"
    data-testid="model-tab-embedding"
    aria-pressed={activeTab === 'embedding'}
    onclick={() => selectTab('embedding')}
  ><span class="mtab-label">{embeddingTabLabel}</span><span class="mdot" data-testid="model-dot-embedding" data-configured={embeddingDotState}><span class="mdot-mark" aria-hidden="true"></span><span class="sr-only">, {embeddingDotLabel}</span></span></button>
  <button
    type="button"
    class="mtab"
    data-testid="model-tab-chat"
    aria-pressed={activeTab === 'chat'}
    onclick={() => selectTab('chat')}
  ><span class="mtab-label">{chatTabLabel}</span><span class="mdot" data-testid="model-dot-chat" data-configured={chatDotState}><span class="mdot-mark" aria-hidden="true"></span><span class="sr-only">, {chatDotLabel}</span></span></button>
</div>

{#if activeCatalogueError}
  <p data-testid="model-catalogue-failure">{activeCatalogueError}</p>
{:else if activeCatalogue}
  {#if unreadableSentence}<p data-testid="model-catalogue-unreadable">{unreadableSentence}</p>{/if}
  {#each activeUnreadableRecords as { rec, label } (rec.index)}
    <p data-testid={`model-unreadable-record-${rec.index}`}>{label}</p>
  {/each}
  {#if emptyCatalogueSentence}
    <p data-testid="model-catalogue-empty">{emptyCatalogueSentence}</p>
  {:else if activeCatalogue.entries.length > 0}
    <div class="row">
      <label class="fl" for="model-selection" data-testid="model-selection-label">{selectionLabel}</label>
      <!-- One native `<select>` for the active role (Task 4, review P1/P2-1/
           P2-2) rather than the frozen list's row of buttons: only `refusal
           === null` entries (`selectableEntries`) become an `<option>` now
           (owner's ruling, live run, 2026-09-10) — a refused entry no longer
           gets a disabled row at all, it disappears from the list and its
           reason moves to `hiddenReasons` below. Duplicates are preserved
           exactly as the old list preserved them — unkeyed, for the same
           reason Task 8 made the frozen list unkeyed: `catalogue.rs`
           enforces no uniqueness over `id`, and a keyed `{#each}` throws on a
           repeat (`each_key_duplicate`), taking the whole section down with
           it. `option.value` is the model id; the backend's own
           `set_*_model` still validates the choice. -->
      <select
        id="model-selection"
        data-testid="model-selection"
        disabled={changeBusy}
        value={selectValue}
        onchange={onModelSelect}
      >
        {#if selectPlaceholder}
          <option value={selectPlaceholder.value} disabled>{selectPlaceholder.label}</option>
        {/if}
        {#each selectableEntries as entry}
          <option value={entry.id}>{entry.name}</option>
        {/each}
      </select>
    </div>
    <!-- One line per DISTINCT reason (owner's ruling above), each naming how
         many entries it folded together — never one line per hidden entry,
         which would repeat the same sentence as many times as this build
         happened to refuse the same thing. -->
    {#each hiddenReasons as { key, label } (key)}
      <p data-testid="model-hidden-reason">{label}</p>
    {/each}
  {/if}
{/if}

<!-- ABOVE everything Task 6 renders, and that is a correction rather than a
     layout preference. The dot answers "provider, key and a chosen embedding
     model are all set" — which stays true through a change that has just taken
     semantic search away — so drawn last it had the final word, and the section
     ended "Search by meaning is unavailable…" followed by "Connected". A change
     that returns to a green dot and says nothing is a promise the product
     cannot keep; the sentences about what just happened come after it now, and
     the last word on the screen is the loss rather than the reassurance. -->
<p data-testid="model-status-dot" data-active={ready ? 'true' : 'false'}>{ready ? readyLabel : notReadyLabel}</p>

<!-- Task 6. The question comes before the act, the report after it, and they
     carry two different numbers about two different moments — the estimate is
     read from the index now, and what actually went is measured by the index
     as it went. -->
{#if pendingEmbedding}
  {@const chosen = pendingEmbedding}
  <div class="group" data-testid="model-embedding-confirm">
    <p data-testid="model-embedding-confirm-title">{confirmLabels.title}</p>
    <p data-testid="model-embedding-estimate">{confirmLabels.estimate}</p>
    <!-- The consequence, in the window that can still be cancelled. It is the
         Global Constraint this task is measured against: what a person loses by
         picking a different embedding model, said BEFORE it happens. -->
    <p data-testid="model-embedding-confirm-loss">{confirmLabels.loss}</p>
    <div class="row">
      <!-- One act and one refusal, and no `Keep` between them. The index will
           not honour `Keep` here: `refuse_unless_every_other_space_is_empty`
           enumerates every space but the requested one — which is `None` for a
           model that has no space yet — so any non-empty space anywhere refuses,
           and that set is exactly the estimate above being above zero. Offering
           it handed the cautious person a rejection and a raw backend sentence
           instead of the safety they reached for. `ExistingVectors` still has no
           `Default` and no `#[serde(default)]`: the value is named at each call
           site, and this component never lets a library choose it. -->
      <button
        type="button"
        data-testid="model-embedding-discard"
        onclick={() => commitEmbedding(chosen, 'discard')}>{confirmLabels.discard}</button>
      <button
        type="button"
        data-testid="model-embedding-cancel"
        onclick={() => (pendingEmbedding = null)}>{confirmLabels.cancel}</button>
    </div>
  </div>
{/if}

{#if retiredLabel}<p data-testid="model-embedding-retired">{retiredLabel}</p>{/if}

{#if degraded}
  <div class="group" data-testid="model-embedding-degraded">
    <p data-testid="model-embedding-degraded-note">{degradedLabels.sentence}</p>
    <button
      type="button"
      data-testid="model-embedding-reembed"
      onclick={reembed}>{degradedLabels.reembed}</button>
    {#if embedPassUnderWay}
      <p data-testid="model-embedding-reembed-started">{degradedLabels.started}</p>
    {:else if embedPassEnded}
      <p data-testid="model-embedding-reembed-ended">{degradedLabels.ended}</p>
    {/if}
  </div>
{/if}

{#if changeError}
  <p data-testid="model-embedding-failed">{changeFailureLabel}</p>
  <!-- The backend's own sentence, verbatim. Nothing on this screen reads it. -->
  <p data-testid="model-embedding-error">{changeError}</p>
  {#if jobRunning}<p data-testid="model-job-running">{jobRunningLabel}</p>{/if}
{/if}

<!-- Last, and outside `{#if settings}` only because `indexFailure` already
     answers `null` without settings: the index sentence is a defect report a
     person reading this window cannot act on, so it follows every sentence
     they can. Its subject label sits immediately above it — the fault Step 5
     inherited was this sentence sitting unlabelled between two other
     subjects. -->
{#if indexFailure}
  <div class="row" data-testid="model-index-group">
    <span class="fl" data-testid="model-index-label">{indexLabel}</span>
    <p data-testid="model-index-failure">{indexFailure}</p>
    <!-- `readFailed` only. The sentence above calls this a defect worth
         reporting, and `set_embedding_model`'s own doc says that sentence is
         wrong about the cause in exactly one state — the one a retirement that
         committed and an adoption that did not leaves behind. It needs no
         repair step and recovers through the ordinary act, so the section says
         which act. `notOpen` gets no such offer: nothing is broken there. -->
    {#if indexUnreadableCause === 'readFailed'}
      <p data-testid="model-index-recover">{indexRecoverLabel}</p>
    {/if}
  </div>
{/if}

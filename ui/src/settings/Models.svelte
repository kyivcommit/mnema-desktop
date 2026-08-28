<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import {
    modelSettings, setKey, forgetKey, providerModels, setChatModel,
    setEmbeddingModel, startEmbedJob, jobStatus,
    type ModelSettings, type KeyRemoval, type Catalogue,
    type ModelEntry, type ModelRefusal, type UnreadableRecord,
    type ExistingVectors, type RetiredSpace,
  } from '../lib/ipc';
  // Reused rather than re-derived: `providerReady` is the exact PR 3 ruling
  // this section's green dot owes ("provider + key + a chosen embedding
  // model, fail-safe on null/undefined"), already written and tested for the
  // launcher's Arms row. A second copy of this boolean is the "two truths,
  // one message" class this project has paid for 22 times in one cycle — one
  // of the two would eventually read a fixed set of fields differently.
  import { providerReady } from '../launcher/state';

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
  // A rejected read of `model_settings`. Everything below is gated on
  // `settings`, so a rejection on mount used to leave the panel literally
  // empty — the failure went to the console, which nobody on the other side of
  // this window opens. It is bounded but real: the command itself cannot fail
  // (`model_settings` returns `ModelSettings`, not `Result`), so what arrives
  // here is an IPC-layer failure. Held apart from `actionError` because it
  // survives no re-read: nothing on this screen can retry it.
  let loadError = $state<string | null>(null);
  let removal = $state<KeyRemoval['kind'] | null>(null);

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

  async function refresh() {
    const seq = ++settingsSeq;
    const s = await modelSettings();
    if (seq !== settingsSeq) return; // superseded before this reply arrived
    settings = s;
  }

  onMount(() => {
    // §10: a rejection arrives as a sentence, never as a kind. Shown verbatim,
    // beside a catalogue sentence naming what failed; never branched on.
    refresh().catch((e) => {
      loadError = e instanceof Error ? e.message : String(e);
    });
    void loadCatalogue('embedding');
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

  async function doForget() {
    actionError = null;
    try {
      const result = await forgetKey();
      removal = result.kind;
      await refresh();
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
    }
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
  const macNote = $derived.by(() => { void $locale; return t('models_mac_keychain_note'); });
  const loadFailureLabel = $derived.by(() => { void $locale; return t('models_load_failed'); });
  const indexLabel = $derived.by(() => { void $locale; return t('models_index_label'); });

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
    // And so are the two sentences a press leaves behind. "The change discarded
    // 4 embeddings…" and a rejection are reports on an act performed from the
    // embedding list; under the chat list they are a report about nothing the
    // person can see. The degraded notice deliberately stays: it is a fact about
    // the product's state rather than about a press, and it carries the one
    // control that repairs it.
    retiredReport = null;
    changeError = null;
    jobRunning = false;
    void loadCatalogue(role);
  }

  async function chooseChatModel(model: string) {
    actionError = null;
    try {
      await setChatModel(model);
      await refresh();
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
    }
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
  // The `never` arm is exhaustiveness twice over: `tsc` refuses to compile if
  // a variant is added to `ModelRefusal` without a matching `case` here, and
  // a value that reaches this function some other way — the catalogue mirror
  // test below constructs one from a Rust source list, not from this
  // union — throws instead of silently falling through, so a sixth variant
  // added to `catalogue.rs` cannot pass this section silently in either
  // direction.
  function refusalReason(r: ModelRefusal): string {
    switch (r.kind) {
      case 'inputTooSmall':
        return t('models_refusal_input_too_small', { limit: r.limit, floor: r.floor });
      case 'noStatedLimit':
        return t('models_refusal_no_stated_limit');
      case 'limitNotUnderstood':
        return t('models_refusal_limit_not_understood');
      case 'noStatedOutputModalities':
        return t('models_refusal_no_stated_output_modalities');
      case 'noTextOutput':
        return t('models_refusal_no_text_output');
      default: {
        const exhaustive: never = r;
        throw new Error(`unhandled model refusal kind: ${(exhaustive as { kind: string }).kind}`);
      }
    }
  }

  // `catalogue.rs`'s `RecordId`, exhaustively — same shape as `refusalReason`
  // and for the same reason: `Absent`, `NotAString` and `Known` are three
  // different facts about a record this build could not turn into a model,
  // and folding them together would be false about at least one of them.
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

  // `void $locale` here, not on `refusalReason`/`unreadableRecordLabel`
  // themselves: those are plain functions called from markup, and Svelte's
  // fine-grained reactivity only re-runs an expression when a signal IT reads
  // changes — `entry.refusal` does not change on a language switch, so a
  // `$derived` reading `$locale` is what makes the surrounding recomputation
  // happen at all (`t()` itself reads `get(locale)` non-reactively,
  // `i18n/index.ts:11`).
  const activeEntries = $derived.by(() => {
    void $locale;
    const cat = activeCatalogue;
    if (!cat) return [];
    return cat.entries.map((entry: ModelEntry) => ({
      entry,
      reason: entry.refusal ? refusalReason(entry.refusal) : null,
    }));
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
  // Whether a change has landed in this session. Without it the degraded
  // notice below would fire on every index that simply has not been embedded
  // yet, which is not a loss and not something this screen caused.
  let changeLanded = $state(false);
  // The backend's own sentence for a rejected `set_embedding_model` or
  // `start_embed_job`, shown verbatim and never branched on.
  let changeError = $state<string | null>(null);
  // What `job_status` answered after a rejection. Read, never inferred from
  // the sentence: a rejection crosses the IPC as text, and `claim_job` is a
  // compare-and-exchange that leaves the running job's owner untouched — so a
  // refusal must not draw that job as cancelled or finished.
  let jobRunning = $state(false);
  // Where the re-embedding pass this section started has got to. `ended` is the
  // pass's own message and not a timer: the section used to set `started` and
  // stop, with no listener and no poll, so a pass that finished left the
  // degraded sentence standing for the rest of the session and a pass that
  // failed said nothing at all.
  let reembedPhase = $state<'idle' | 'started' | 'ended'>('idle');

  function chooseEmbeddingModel(model: string) {
    changeError = null;
    retiredReport = null;
    reembedPhase = 'idle';
    // The model the index is already on is not a change. It asks nothing and
    // calls nothing: `set_embedding_model` would find the space rather than
    // mint one and retire nothing, and a confirmation offering to discard
    // embeddings for a press that moves nothing is a question with no honest
    // answer.
    if (model === currentEmbeddingModel) return;
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

  async function commitEmbedding(model: string, existingVectors: ExistingVectors) {
    pendingEmbedding = null;
    changeError = null;
    reembedPhase = 'idle';
    try {
      const adopted = await setEmbeddingModel(model, existingVectors);
      retiredReport = adopted.retired;
      changeLanded = true;
      jobRunning = false;
      await refresh();
    } catch (e) {
      changeError = e instanceof Error ? e.message : String(e);
      // §10: a rejection arrives as a sentence, not as a kind. What the screen
      // says next is decided by re-reading the state — `model_settings` for
      // what the index is in, `job_status` for whether a job is still going —
      // and never by matching on the message text.
      await refresh().catch(() => {});
      jobRunning = await jobStatus()
        .then((s) => s.running)
        .catch(() => false);
    }
  }

  async function reembed() {
    changeError = null;
    try {
      // The ending is listened for, not assumed. `degraded` is read from the
      // index's own count, so the sentence clears itself once the pass has
      // refilled the space — but only if something asks the index again, and
      // nothing did: the pass reported to a channel whose messages were
      // dropped. A pass that ENDS is the one moment that count can have
      // changed, so it is the one moment worth re-reading it.
      // `startEmbedJob` forwards the whole `JobEvent` (Task 8): only an
      // ENDING may re-read the index — a re-read per progress report is one
      // command every 250 ms for the length of the run.
      await startEmbedJob((event) => {
        if (event.event === 'ended') void passEnded();
      });
      reembedPhase = 'started';
    } catch (e) {
      changeError = e instanceof Error ? e.message : String(e);
      jobRunning = await jobStatus()
        .then((s) => s.running)
        .catch(() => false);
    }
  }

  // What an ended pass changes on this screen, and it is one thing: the index
  // is asked again. A pass that filled the space clears `degraded` and takes
  // this whole block with it; a pass that did not leaves the block standing and
  // says so, which is more than the nothing it used to say.
  async function passEnded() {
    reembedPhase = 'ended';
    await refresh().catch(() => {});
  }

  // **The number before the act, and it is `embeddedChunksEverywhere`.** Not
  // `embeddedChunks`, which counts the active space alone: the change retires
  // every space in its way, and a space abandoned by an earlier change still
  // holds whatever it held — so the active count understates the bill by
  // exactly the spaces it forgets. `models.rs` says so on the field itself,
  // and `the_settings_tell_the_active_space_apart_from_the_whole_index` is
  // where the two numbers are held apart.
  const estimatedEmbeddings = $derived(indexRead ? indexRead.embeddedChunksEverywhere : 0);

  // Semantic search is dark: a change has landed and the space the index now
  // points at holds nothing. Read from the active space's own count, because
  // that is the space `retrieve` hands the KNN.
  const degraded = $derived(changeLanded && !!indexRead && indexRead.embeddedChunks === 0);

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
  <div class="field">
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
  <div class="group" data-testid="model-key-group">
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
    <!-- Owner's ruling, 2026-08-28: the note is shown in EVERY key state, `absent`
         included, and carries no condition on `key`. A review read it as claiming
         a key exists ("its own key") where none has been entered; it does not —
         it explains a system prompt the person is about to meet the first time
         they save one, and forward-looking information is not a false claim.
         Settled; do not add a `key` condition here. It sits inside this group
         because it is a sentence ABOUT the key, and a sentence loose between two
         subjects is the fault Step 5 was opened to fix. -->
    {#if settings.platform === 'mac'}<p data-testid="model-mac-note">{macNote}</p>{/if}
    {#if showInput}
      <div class="field">
        <input id="model-key-input" type="password" bind:value={draftKey} />
        <button type="button" onclick={saveKey}>{saveLabel}</button>
        {#if settings.key.kind === 'present'}
          <button type="button" onclick={cancelEditing}>{cancelLabel}</button>
        {/if}
      </div>
    {:else if !keyFailure}
      <!-- Unreadable offers nothing to press: the store would not say whether a
           key exists at all, so add/change/forget would be a claim this build
           cannot back. -->
      <div class="field">
        <button type="button" onclick={startEditing}>{changeLabel}</button>
        <button type="button" onclick={doForget}>{forgetLabel}</button>
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
  <button
    type="button"
    data-testid="model-tab-embedding"
    aria-pressed={activeTab === 'embedding'}
    onclick={() => selectTab('embedding')}>{embeddingTabLabel}</button>
  <button
    type="button"
    data-testid="model-tab-chat"
    aria-pressed={activeTab === 'chat'}
    onclick={() => selectTab('chat')}>{chatTabLabel}</button>
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
  {:else if activeEntries.length > 0}
    <ul data-testid="model-entry-list">
      {#each activeEntries as { entry, reason } (entry.id)}
        <li>
          {#if entry.refusal}
            <!-- `None` means selectable; anything else is shown, greyed, with
                 its reason (catalogue.rs:66-69) — a model the provider lists
                 and this build hides sends a person looking for a fault here
                 instead. Not a button: there is nothing this click could do. -->
            <span data-testid={`model-entry-${entry.id}`} class="unavailable">{entry.name}</span>
            <span data-testid={`model-entry-reason-${entry.id}`}>{reason}</span>
          {:else if activeTab === 'chat'}
            <button
              type="button"
              data-testid={`model-entry-${entry.id}`}
              aria-pressed={entry.id === currentChatModel}
              onclick={() => chooseChatModel(entry.id)}>{entry.name}</button>
          {:else}
            <!-- `aria-current` and not `aria-pressed`, which the chat tab uses:
                 this marks the one model the INDEX is on, a fact about a set,
                 while the chat rows are a choice being toggled. Keeping them
                 different is also what stops one shared "is this the chosen
                 one" test from answering for both roles. -->
            <button
              type="button"
              data-testid={`model-entry-${entry.id}`}
              aria-current={entry.id === currentEmbeddingModel ? 'true' : undefined}
              onclick={() => chooseEmbeddingModel(entry.id)}>{entry.name}</button>
          {/if}
        </li>
      {/each}
    </ul>
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
    <div class="field">
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
    {#if reembedPhase === 'started'}
      <p data-testid="model-embedding-reembed-started">{degradedLabels.started}</p>
    {:else if reembedPhase === 'ended'}
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
  <div class="field" data-testid="model-index-group">
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

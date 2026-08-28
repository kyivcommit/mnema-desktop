<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import {
    modelSettings, setKey, forgetKey, providerModels, setChatModel,
    type ModelSettings, type KeyRemoval, type Catalogue,
    type ModelEntry, type ModelRefusal, type UnreadableRecord,
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

  // The current embedding model is read-only here — choosing one is Task 6's
  // `set_embedding_model`, which retires vector spaces and takes the job
  // slot; nothing in this component calls it.
  const currentEmbeddingModel = $derived(
    settings && settings.index.kind === 'read' ? settings.index.embeddingModel : null,
  );
  // `?? null`: the field is optional on the wire type (see `ipc.ts`), and
  // "not stated by this fixture" and "the index says no chat model" read the
  // same way here — neither marks anything as chosen.
  const currentChatModel = $derived(
    settings && settings.index.kind === 'read' ? (settings.index.chatModel ?? null) : null,
  );

  const ready = $derived(!!settings && providerReady(settings));
  const readyLabel = $derived.by(() => { void $locale; return t('models_status_ready'); });
  const notReadyLabel = $derived.by(() => { void $locale; return t('models_status_not_ready'); });

  const embeddingTabLabel = $derived.by(() => { void $locale; return t('models_tab_embedding'); });
  const chatTabLabel = $derived.by(() => { void $locale; return t('models_tab_chat'); });
  const emptyCatalogueLabel = $derived.by(() => { void $locale; return t('models_catalogue_empty'); });

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

  // A stated zero is never a promise the list is complete (umbrella `:529`) —
  // this sentence is the promise, and it is the one thing on this branch that
  // must NOT render when the count is zero.
  const unreadableSentence = $derived.by(() => {
    void $locale;
    const cat = activeCatalogue;
    if (!cat || cat.unreadable === 0) return null;
    return t('models_catalogue_unreadable', { count: cat.unreadable });
  });
</script>

{#if settings}
  <div class="field">
    <label for="model-provider">{providerLabel}</label>
    <select id="model-provider" disabled>
      <option selected>{providerName}</option>
    </select>
  </div>
  {#if indexFailure}
    <div class="field">
      <span class="fl" data-testid="model-index-label">{indexLabel}</span>
      <p data-testid="model-index-failure">{indexFailure}</p>
    </div>
  {/if}
  <!-- Owner's ruling, 2026-08-28: the note is shown in EVERY key state, `absent`
       included, and carries no condition on `key`. A review read it as claiming
       a key exists ("its own key") where none has been entered; it does not —
       it explains a system prompt the person is about to meet the first time
       they save one, and forward-looking information is not a false claim.
       Settled; do not add a `key` condition here. -->
  {#if settings.platform === 'mac'}<p data-testid="model-mac-note">{macNote}</p>{/if}
  {#if keyFailure}
    <div class="field">
      <span class="fl" data-testid="model-key-label">{keyLabel}</span>
      <p data-testid="model-key-failure">{keyFailure}</p>
    </div>
  {:else if showInput}
    {#if settings.key.kind === 'absent'}
      <p data-testid="model-key-absent-hint">{absentHint}</p>
    {/if}
    <div class="field">
      <label for="model-key-input">{keyLabel}</label>
      <input id="model-key-input" type="password" bind:value={draftKey} />
      <button type="button" onclick={saveKey}>{saveLabel}</button>
      {#if settings.key.kind === 'present'}
        <button type="button" onclick={cancelEditing}>{cancelLabel}</button>
      {/if}
    </div>
  {:else}
    <div class="field">
      <span class="fl">{keyLabel}</span>
      <span data-testid="model-key-saved">{savedLabel}</span>
      <button type="button" onclick={startEditing}>{changeLabel}</button>
      <button type="button" onclick={doForget}>{forgetLabel}</button>
    </div>
  {/if}
  {#if removalLabel}<p data-testid="model-key-removal">{removalLabel}</p>{/if}
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
  {#if activeEntries.length === 0}
    <p data-testid="model-catalogue-empty">{emptyCatalogueLabel}</p>
  {:else}
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
            <!-- Read-only here: choosing an embedding model is Task 6's
                 `set_embedding_model`. A row with no action is a row, not a
                 button with nothing to press (the same reasoning `Tree.svelte`
                 already applies to a source-card row that opens nothing). -->
            <span
              data-testid={`model-entry-${entry.id}`}
              aria-current={entry.id === currentEmbeddingModel ? 'true' : undefined}>{entry.name}</span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
{/if}

<p data-testid="model-status-dot" data-active={ready ? 'true' : 'false'}>{ready ? readyLabel : notReadyLabel}</p>

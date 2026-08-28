<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import { modelSettings, setKey, forgetKey, type ModelSettings, type KeyRemoval } from '../lib/ipc';

  // §9.1, Task 4 — the provider/key row and the platform-dependent note. The
  // two model tabs (embedding/chat) and the green-dot rule are Task 5's; the
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
  // The backend's own sentence for a rejected set_key/forget_key, shown
  // verbatim beside the control — never branched on, only displayed (§10 /
  // the umbrella rejection rule).
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

  async function refresh() {
    settings = await modelSettings();
  }

  onMount(() => {
    // §10: a rejection arrives as a sentence, never as a kind. Shown verbatim,
    // beside a catalogue sentence naming what failed; never branched on.
    refresh().catch((e) => {
      loadError = e instanceof Error ? e.message : String(e);
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
</script>

{#if settings}
  <div class="field">
    <label for="model-provider">{providerLabel}</label>
    <select id="model-provider" disabled>
      <option selected>{providerName}</option>
    </select>
  </div>
  {#if indexFailure}<p data-testid="model-index-failure">{indexFailure}</p>{/if}
  <!-- Owner's ruling, 2026-08-28: the note is shown in EVERY key state, `absent`
       included, and carries no condition on `key`. A review read it as claiming
       a key exists ("its own key") where none has been entered; it does not —
       it explains a system prompt the person is about to meet the first time
       they save one, and forward-looking information is not a false claim.
       Settled; do not add a `key` condition here. -->
  {#if settings.platform === 'mac'}<p data-testid="model-mac-note">{macNote}</p>{/if}
  {#if keyFailure}
    <p data-testid="model-key-failure">{keyFailure}</p>
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

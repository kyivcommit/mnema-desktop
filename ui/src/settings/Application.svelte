<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import type { Key } from '../i18n/catalog';
  import { appPrefs, setHotkey, setAutostart, type AppPrefs, type AutostartState } from '../lib/ipc';
  import { formatShortcut, isModifierOnlyPress, shortcutFromEvent } from '../i18n/shortcut';

  // §9.4 — the Application section: the shortcut, autostart, and the version.
  //
  // No prop, unlike `Indexing.svelte`: this section starts no job and shares no
  // controller. It reads `app_prefs` once on mount and re-reads it again only
  // where D-b and D-c say a REJECTION carries no state of its own — a refused
  // `set_hotkey` or `set_autostart` crosses as a sentence alone, so what the
  // screen draws afterwards can only come from a fresh read, never from the
  // value the window held before the call.

  let prefs = $state<AppPrefs | null>(null);
  // A rejected `app_prefs` read. §10: a rejection is a SENTENCE, never a kind —
  // shown verbatim beside a catalogue lead-in, exactly as `Indexing.svelte`
  // does for `model_settings`.
  let loadError = $state<string | null>(null);

  // The newer of two reads in flight always wins — `Indexing.svelte:43`'s
  // guard and its reason: a rejected `set_hotkey` or `set_autostart` triggers a
  // second `appPrefs()` while the mount's own first read may still be in
  // flight, and the two can settle in either order.
  let seq = 0;

  async function refresh() {
    const my = ++seq;
    try {
      const p = await appPrefs();
      if (my !== seq) return; // superseded before this reply arrived
      prefs = p;
      loadError = null;
    } catch (e) {
      if (my !== seq) return;
      loadError = e instanceof Error ? e.message : String(e);
    }
  }

  onMount(() => { void refresh(); });

  // ---------------------------------------------------------------------------
  // The shortcut, as the operating system reports it (D-b).
  // ---------------------------------------------------------------------------

  const hotkey = $derived(prefs === null ? null : prefs.hotkey);
  const platform = $derived(prefs === null ? null : prefs.platform);
  // D-i: `platform` comes from the WIRE, chosen at compile time by
  // `Platform::of_this_build`, and never from `navigator.userAgent` — see
  // `shortcut.ts`'s own header for the measured reason.
  const shortcutText = $derived(
    hotkey === null || platform === null ? null : formatShortcut(hotkey.shortcut, platform),
  );

  // The union discriminated HERE, before `reason` is read from either arm —
  // `Indexing.svelte:104-106`'s pattern, for the same reason.
  const unavailable = $derived(hotkey !== null && hotkey.status.kind === 'unavailable' ? hotkey.status : null);

  const shortcutStatusText = $derived.by(() => {
    void $locale;
    if (hotkey === null) return null;
    // 🔴 Worded as REGISTERED, never "works" or "is yours" — D128 measured
    // macOS co-registering a shortcut another application already holds: both
    // register, both fire, so this sentence may not promise more than the
    // operating system reports.
    return hotkey.status.kind === 'registered'
      ? t('application_shortcut_registered')
      : t('application_shortcut_unavailable');
  });
  const shortcutReasonText = $derived.by(() => {
    void $locale;
    if (unavailable === null) return null;
    // VERBATIM, beside the catalogue lead-in: a refusal the BACKEND makes is
    // shown as it came, exactly as `Indexing.svelte`'s unreadable reason is.
    return t('application_shortcut_reason', { reason: unavailable.reason });
  });
  // A degraded state that offers no way forward is the state a person files a
  // bug about — the launcher stays reachable from the tray.
  const shortcutTrayText = $derived.by(() => { void $locale; return t('application_shortcut_tray'); });

  const shortcutLabelText = $derived.by(() => { void $locale; return t('application_shortcut_label'); });
  const recordLabel = $derived.by(() => { void $locale; return t('application_shortcut_record'); });
  const recordingText = $derived.by(() => { void $locale; return t('application_shortcut_recording'); });
  const notUsableText = $derived.by(() => { void $locale; return t('application_shortcut_not_usable'); });
  const shortcutFailedLabel = $derived.by(() => { void $locale; return t('application_shortcut_failed'); });

  let recording = $state(false);
  let notUsable = $state(false);
  // A rejected `set_hotkey`'s own sentence, shown VERBATIM beside the
  // catalogue lead-in above — never branched on, exactly as every other
  // rejection in this product.
  let hotkeyError = $state<string | null>(null);

  function startRecording() {
    recording = true;
    notUsable = false;
    hotkeyError = null;
  }

  // 🔴 Three refusals, told apart before `shortcutFromEvent` is even called —
  // `shortcut.ts`'s own doc on that function and on `isModifierOnlyPress`.
  // Holding a modifier on the way to a combination must not flash a refusal;
  // Escape is the recorder's own cancel and sends nothing; anything else the
  // map does not carry gets the WINDOW's catalogue sentence, never the
  // parser's, whose wording asks a person to open a GitHub issue.
  async function onRecorderKeydown(e: KeyboardEvent) {
    if (!recording) return;
    if (isModifierOnlyPress(e)) return;
    if (e.key === 'Escape' || e.code === 'Escape') {
      e.preventDefault();
      recording = false;
      notUsable = false;
      return;
    }
    const shortcut = shortcutFromEvent(e);
    if (shortcut === null) {
      e.preventDefault();
      notUsable = true;
      return;
    }
    e.preventDefault();
    notUsable = false;
    recording = false;
    hotkeyError = null;
    try {
      const reply = await setHotkey(shortcut);
      // The backend is the truth (D-b closing note): drawn from the REPLY, a
      // whole `HotkeyState`, never assembled from the string this window sent.
      if (prefs !== null) prefs = { ...prefs, hotkey: reply };
    } catch (err) {
      hotkeyError = err instanceof Error ? err.message : String(err);
      // 🔴 D-b: a rejection carries no `HotkeyState` at all — which of the
      // table's seven rows produced it is not recoverable from the sentence, so
      // the only honest source for what the screen draws next is a fresh read,
      // never the value this window held before the call.
      void refresh();
    }
  }

  // ---------------------------------------------------------------------------
  // Autostart (D-c): the answer comes from the OS every time, never echoed.
  // ---------------------------------------------------------------------------

  const autostart = $derived(prefs === null ? null : prefs.autostart);
  const autostartUnknown = $derived(autostart !== null && autostart.kind === 'unknown' ? autostart : null);
  const autostartIsEnabled = $derived(autostart !== null && autostart.kind === 'enabled');

  // A `Record` over the discriminant rather than a chain of ternaries, for
  // `Indexing.svelte`'s reason: a fourth arm added to `AutostartState` becomes
  // a compile error here instead of silently falling through to a sentence
  // that belongs to a different state.
  const AUTOSTART_SENTENCE: Record<AutostartState['kind'], Key> = {
    enabled: 'application_autostart_enabled',
    disabled: 'application_autostart_disabled',
    // 🔴 A third sentence, not the second one drawn twice: a failed READ shown
    // as "does not start" would put a switch on screen in the position
    // opposite to the one the machine is actually in.
    unknown: 'application_autostart_unknown',
  };
  const autostartStatusText = $derived.by(() => {
    void $locale;
    if (autostart === null) return null;
    return t(AUTOSTART_SENTENCE[autostart.kind]);
  });
  const autostartReasonText = $derived.by(() => {
    void $locale;
    if (autostartUnknown === null) return null;
    return t('application_autostart_reason', { reason: autostartUnknown.reason });
  });
  const autostartLabelText = $derived.by(() => { void $locale; return t('application_autostart_label'); });
  const autostartActionLabel = $derived.by(() => {
    void $locale;
    if (autostart === null) return null;
    return autostartIsEnabled ? t('application_autostart_disable') : t('application_autostart_enable');
  });
  const autostartFailedLabel = $derived.by(() => { void $locale; return t('application_autostart_failed'); });

  let autostartError = $state<string | null>(null);

  async function toggleAutostart() {
    if (autostart === null) return;
    // The opposite of the value ON SCREEN, not a constant — a second press
    // after the first one's reply follows what the OS has just reported.
    const target = !autostartIsEnabled;
    autostartError = null;
    try {
      const reply = await setAutostart(target);
      // Drawn from the REPLY, never from `target`: `set_autostart` re-reads the
      // OS after the change (D-c), and a request that could not be confirmed
      // must not render as though it had been.
      if (prefs !== null) prefs = { ...prefs, autostart: reply };
    } catch (err) {
      autostartError = err instanceof Error ? err.message : String(err);
      void refresh();
    }
  }

  // ---------------------------------------------------------------------------
  // The version (D-h): shown as it is, with no "up to date" claim beside it.
  // ---------------------------------------------------------------------------

  const versionText = $derived.by(() => {
    void $locale;
    if (prefs === null) return null;
    return t('application_version', { version: prefs.version });
  });

  const loadFailedLabel = $derived.by(() => { void $locale; return t('application_load_failed'); });
</script>

<!-- The failed read leads and does not gate what follows: on the FIRST read's
     rejection there is nothing below anyway, because `prefs` is still null. A
     refused RE-read (triggered by a rejected change) leaves the previous
     answer on screen, which is `Indexing.svelte`'s ruling and not a new one. -->
{#if loadError}
  <p data-testid="application-load-failed">{loadFailedLabel}</p>
  <p data-testid="application-load-error">{loadError}</p>
{/if}

{#if prefs}
  <p>
    {shortcutLabelText}
    <span data-testid="application-shortcut">{shortcutText}</span>
  </p>
  <p data-testid="application-shortcut-status">{shortcutStatusText}</p>
  {#if unavailable}
    <p data-testid="application-shortcut-reason">{shortcutReasonText}</p>
    <p data-testid="application-shortcut-tray">{shortcutTrayText}</p>
  {/if}
  {#if hotkeyError !== null}
    <p data-testid="application-shortcut-failed">{shortcutFailedLabel}</p>
    <p data-testid="application-shortcut-error">{hotkeyError}</p>
  {/if}
  <button
    type="button"
    data-testid="application-shortcut-record"
    onclick={startRecording}
    onkeydown={onRecorderKeydown}
  >{recordLabel}</button>
  {#if recording}
    <p data-testid="application-shortcut-recording">{recordingText}</p>
  {/if}
  {#if notUsable}
    <p data-testid="application-shortcut-not-usable">{notUsableText}</p>
  {/if}

  <p>{autostartLabelText}</p>
  <p data-testid="application-autostart-status">{autostartStatusText}</p>
  {#if autostartUnknown}
    <p data-testid="application-autostart-reason">{autostartReasonText}</p>
  {/if}
  {#if autostartError !== null}
    <p data-testid="application-autostart-failed">{autostartFailedLabel}</p>
    <p data-testid="application-autostart-error">{autostartError}</p>
  {/if}
  <button
    type="button"
    data-testid="application-autostart-toggle"
    onclick={toggleAutostart}
  >{autostartActionLabel}</button>

  <p data-testid="application-version">{versionText}</p>
{/if}

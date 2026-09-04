<script lang="ts">
  import { onMount } from 'svelte';
  import { locale, t } from '../i18n';
  import type { Key } from '../i18n/catalog';
  import { appPrefs, setHotkey, setAutostart, type AppPrefs, type AutostartState } from '../lib/ipc';
  import { formatShortcut, isModifierOnlyPress, shortcutFromEvent, MODIFIER_KEY_NAME } from '../i18n/shortcut';

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
  //
  // 🔴 **One stamp PER FIELD, not one for the section** (final review, D-I1).
  // Three writers share this guard and they write disjoint fields, so a single
  // stamp is field-blind: a `setAutostart` that succeeds cancels the corrective
  // `appPrefs()` a refused `setHotkey` started, and nothing about autostart says
  // anything about the shortcut. The state that leaves wrong is D-b's
  // persist-failure row — `set_hotkey` rejects with `Error::Prefs` while the
  // operating system IS holding the new shortcut, so the window would go on
  // drawing the old one beside "the shortcut was not changed". A read claims
  // both stamps because it re-reads both fields, and applies each field only
  // where its own stamp survived.
  let hotkeySeq = 0;
  let autostartSeq = 0;

  // Answers with the `hotkey` this read actually WROTE to the screen, or `null`
  // when it wrote none — superseded by a later writer, or rejected outright.
  // Only the caller that started a corrective read after a refused `set_hotkey`
  // reads the answer; see `shortcutNotSaved` for what it decides. The
  // distinction matters and it is the stamp's: a read whose hotkey the stamp
  // discarded never reached the screen, so it must not get to choose the
  // sentence drawn beside what did.
  async function refresh(): Promise<AppPrefs['hotkey'] | null> {
    const myHotkey = ++hotkeySeq;
    const myAutostart = ++autostartSeq;
    try {
      const p = await appPrefs();
      const takeHotkey = myHotkey === hotkeySeq;
      const takeAutostart = myAutostart === autostartSeq;
      // Superseded on every field this read could have written. `version` and
      // `platform` are decided at compile time and cannot have changed, so
      // there is nothing left for it to say.
      if (!takeHotkey && !takeAutostart) return null;
      prefs = {
        ...p,
        // A writer that landed while this read was in flight holds the truth
        // for ITS field: it carries the operating system's own reply, and this
        // read was issued before it.
        hotkey: takeHotkey || prefs === null ? p.hotkey : prefs.hotkey,
        autostart: takeAutostart || prefs === null ? p.autostart : prefs.autostart,
      };
      loadError = null;
      return takeHotkey ? p.hotkey : null;
    } catch (e) {
      // A rejection is about the read as a whole, so it is shown only where the
      // read still had something to say — the same test both directions get.
      if (myHotkey !== hotkeySeq && myAutostart !== autostartSeq) return null;
      loadError = e instanceof Error ? e.message : String(e);
      return null;
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
  // Platform-aware (review, Minor 5): the sentence used to say "the command
  // key" on every platform, which named the wrong key on Windows and Linux —
  // this window already knows `platform` from the wire, the same fact
  // `formatShortcut` draws its glyphs from.
  const notUsableText = $derived.by(() => {
    void $locale;
    if (platform === null) return null;
    return t('application_shortcut_not_usable', { mod: MODIFIER_KEY_NAME[platform] });
  });
  // 🔴 External review P3. Transition-table row 6 (`prefs.rs`): the operating
  // system registered the NEW combination and the write to `prefs.json` then
  // failed, so `set_hotkey` rejects with `Error::Prefs` while the shortcut is
  // in effect. The corrective re-read D-b requires then draws that new
  // shortcut — under a heading saying nothing was changed. Each half is true
  // alone; the pair is not, and what a person does about it differs: one is
  // "try again", the other is "it works until you restart".
  //
  // Decided from the RE-READ and never from the rejection's sentence. That
  // sentence is a free-text `Display` this window does not own, and every
  // wording of it is one refactor away from moving; the state the operating
  // system reports is the fact.
  let shortcutNotSaved = $state(false);
  const shortcutFailedLabel = $derived.by(() => {
    void $locale;
    return t(shortcutNotSaved ? 'application_shortcut_not_saved' : 'application_shortcut_failed');
  });

  let recording = $state(false);
  let notUsable = $state(false);
  // A rejected `set_hotkey`'s own sentence, shown VERBATIM beside the
  // catalogue lead-in above — never branched on, exactly as every other
  // rejection in this product.
  let hotkeyError = $state<string | null>(null);

  // 🔴 (review, Important 1) `onkeydown` below only ever reaches a FOCUSED
  // element, and a click does not focus a `<button>` on every platform — macOS
  // WebKit, which is what Tauri's WKWebView is, does not give a button focus
  // on click at all. Without this reference and the `.focus()` call below, the
  // recorder would listen on an element no real keypress on that platform ever
  // reaches: the sentence saying "press the combination you want" would stay
  // on screen through every keystroke, with no way out but a nav change.
  let recordButton: HTMLButtonElement | undefined = $state();
  // 🔴 (final review, D-I2) The in-flight guard its autostart sibling already
  // had, and the argument that deferred it here does not carry over: two presses
  // of the toggle send the same value, two recorded combinations send two
  // DIFFERENT ones. `change_hotkey` serialises the calls behind one critical
  // section (`prefs.rs`, `two_hotkey_changes_cannot_interleave`), so the
  // operating system keeps whichever went through the lock last while this
  // window would paint whichever reply resolved last — and those need not be the
  // same one. The screen would then name a shortcut the operating system is not
  // holding, which is the one thing D-b exists to prevent.
  let hotkeyBusy = $state(false);

  function startRecording() {
    // Both halves, like the toggle: `disabled` is what a person sees, and the
    // early return is what actually holds — a `click` dispatched at a disabled
    // element still reaches a listener.
    if (hotkeyBusy) return;
    recording = true;
    notUsable = false;
    hotkeyError = null;
    shortcutNotSaved = false;
    recordButton?.focus();
  }

  // Losing focus by any route other than a captured key — a click elsewhere,
  // the window losing focus itself — must not leave the "press a key" sentence
  // standing over a control nothing is listening to any more.
  function stopRecordingOnBlur() {
    recording = false;
    notUsable = false;
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
    shortcutNotSaved = false;
    hotkeyBusy = true;
    try {
      const reply = await setHotkey(shortcut);
      // 🔴 (review, Important 2) The stamp is bumped HERE too, not only inside
      // `refresh()`. Two writers share this guard: a rejected change earlier can
      // have started a `refresh()` still in flight when THIS change succeeds,
      // and without bumping the stamp that older read is not superseded — it
      // resolves later and overwrites the state this line is about to write
      // with the pre-change read, stating a shortcut the OS is not holding.
      //
      // The SHORTCUT's stamp and no other (final review, D-I1): what this reply
      // supersedes is what a read would have said about the field it just
      // changed, and a read still owes its answer about the other one.
      hotkeySeq++;
      // The backend is the truth (D-b closing note): drawn from the REPLY, a
      // whole `HotkeyState`, never assembled from the string this window sent.
      if (prefs !== null) prefs = { ...prefs, hotkey: reply };
    } catch (err) {
      hotkeyError = err instanceof Error ? err.message : String(err);
      // 🔴 D-b: a rejection carries no `HotkeyState` at all — which of the
      // table's seven rows produced it is not recoverable from the sentence, so
      // the only honest source for what the screen draws next is a fresh read,
      // never the value this window held before the call.
      //
      // And that read is what tells row 6 from the rows that changed nothing:
      // if it comes back naming the combination THIS call sent, the operating
      // system kept it and only the file did not. `applied === null` means the
      // read never reached the screen, and a read that wrote nothing decides
      // nothing — the heading stays the one that assumes the ordinary refusal.
      void refresh().then((applied) => {
        shortcutNotSaved = applied !== null && applied.shortcut === shortcut;
      });
    } finally {
      // Released whichever way the call went: a refusal that left the control
      // disabled would cost a person the only way to change the shortcut.
      hotkeyBusy = false;
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
  // (review, Minor 3) No in-flight guard meant a double press sent two
  // `set_autostart` calls — both carrying the same value, since the second
  // read the same unchanged on-screen state, so no WRONG state resulted, but
  // the OS was asked twice and a person got no sign the first press landed.
  let autostartBusy = $state(false);

  // 🔴 External review P2. `Unknown` is not `Disabled`: `prefs.rs` documents it
  // as "reading the OS state failed", so the login item may be registered or
  // may not. `autostartIsEnabled` answers `false` for it, and a single action
  // derived from that answer offers Enable and only Enable — so somebody whose
  // machine really does start Mnema, and whose READ merely failed, has no way
  // to turn it off. The two known states keep one toggle, because there the
  // opposite of what is on screen is a real answer; `Unknown` gets both
  // directions offered explicitly, because from there neither is.
  const autostartOffersBothDirections = $derived(autostartUnknown !== null);
  const autostartEnableLabel = $derived.by(() => { void $locale; return t('application_autostart_enable'); });
  const autostartDisableLabel = $derived.by(() => { void $locale; return t('application_autostart_disable'); });

  // Takes the target rather than deriving it, so the two buttons above can each
  // send the value they are named for. `toggleAutostart` keeps the derivation
  // it always had and hands it here.
  async function setAutostartTo(target: boolean) {
    if (autostart === null || autostartBusy) return;
    autostartError = null;
    autostartBusy = true;
    try {
      const reply = await setAutostart(target);
      // 🔴 (review, Important 2) Bumped here too, for the same reason as the
      // hotkey success path above: a `refresh()` from an earlier rejection can
      // still be in flight when this call lands — and, D-I1, only AUTOSTART's
      // stamp, because that is the only field this reply is about.
      autostartSeq++;
      // Drawn from the REPLY, never from `target`: `set_autostart` re-reads the
      // OS after the change (D-c), and a request that could not be confirmed
      // must not render as though it had been.
      if (prefs !== null) prefs = { ...prefs, autostart: reply };
    } catch (err) {
      autostartError = err instanceof Error ? err.message : String(err);
      void refresh();
    } finally {
      autostartBusy = false;
    }
  }

  function toggleAutostart() {
    // The opposite of the value ON SCREEN, not a constant — a second press
    // after the first one's reply follows what the OS has just reported. Only
    // reached from the `Enabled`/`Disabled` arm, where "the opposite" is a
    // question the screen can answer.
    void setAutostartTo(!autostartIsEnabled);
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
    bind:this={recordButton}
    disabled={hotkeyBusy}
    onclick={startRecording}
    onkeydown={onRecorderKeydown}
    onblur={stopRecordingOnBlur}
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
  {#if autostartOffersBothDirections}
    <!-- Both disabled by the one flag: a press on either asks the operating
         system once, and the other must not be able to ask again over it. -->
    <button
      type="button"
      data-testid="application-autostart-enable"
      disabled={autostartBusy}
      onclick={() => setAutostartTo(true)}
    >{autostartEnableLabel}</button>
    <button
      type="button"
      data-testid="application-autostart-disable"
      disabled={autostartBusy}
      onclick={() => setAutostartTo(false)}
    >{autostartDisableLabel}</button>
  {:else}
    <button
      type="button"
      data-testid="application-autostart-toggle"
      disabled={autostartBusy}
      onclick={toggleAutostart}
    >{autostartActionLabel}</button>
  {/if}

  <p data-testid="application-version">{versionText}</p>
{/if}

# Wayland shortcut block (D165, `wayland-shortcut`) — the RED probes run
# against `ui/src/settings/Application.svelte`, `ui/src/locale-choice.ts`
# and their catalogue keys, pinned so the mutant each one caught cannot come
# back unnoticed. All vitest, so this file's matrix leg carries `node: true`
# in `.github/workflows/ci.yml`. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/wayland-shortcut.sh
#
# What is here, by name:
#
#   the registered guard    — «in effect» is said only of a read whose own
#                              status is `registered`, never of one that
#                              merely names the same combination back
#   the pending mark         — a rejected recording leaves the outcome
#                              `pending` until its own corrective read answers
#   the outcome's own key    — the settled heading is a NEW announcement, not
#                              a rewrite of the pending one
#   the whole-sentence match — a refusal repeats the standing reason only when
#                              the two sentences are equal, never on overlap
#   the reverse containment  — a refusal that is a strict prefix of the
#                              standing reason is not treated as the same
#                              sentence either
#   autostart's own dedupe   — an autostart refusal repeating its own standing
#                              reason is quoted once, and only on a repeat
#   the reason description   — the shortcut control's `aria-describedby` names
#                              the reason paragraph once the quote is dropped
#   the standing heading     — the shortcut failure heading stays on screen
#                              whether or not its quote is dropped
#   the pending-change gate  — a shortcut change still `pending` is judged
#                              against its own answer, never the pre-change
#                              reason
#   the always-announced sentence — the assertive region carries an autostart
#                              refusal's own sentence whether or not its quote
#                              is dropped from the screen
#   the tray announcement    — an unavailable shortcut's way-out sentence is
#                              announced, not only shown
#   the tray's own region    — the tray paragraph names the live region that
#                              speaks it
#   the load failure's key   — a repeated load failure is announced again,
#                              node by node
#   the language failure's key — a repeated language-read failure is
#                              announced again, node by node
#   the autostart press origin — a rejected autostart change's corrective
#                              read is heard as the press's own answer
#   neither region silently   — a standing language failure lands in a live
#                              region the instant a section remounts, never
#                              in neither
#   the fresh instance's own read — a remounted section does not inherit the
#                              outgoing instance's press origin
#   the press-answered read   — a corrective read a press started is heard
#                              assertively, not politely
#   the mount read            — a section's very first read is heard
#                              politely, never as though a press started it
#   the store's own marker (x2) — only a read a press itself started marks
#                              its own failure as answering that press
#   the Retry button's press — the language Retry control's own read starts
#                              as a press, never the default mount origin
#   the press-time silence   — a Retry read writes nothing to the store
#                              until its own answer lands
#   the still-refused read   — a corrective read that is itself refused
#                              leaves a pending shortcut outcome pending
#   the superseded read's silence — a read a later one has already
#                              superseded writes nothing to the store
#   the recovery read's origin — a rejected change's own recovery read is
#                              never heard as a press
#
# Checked against the rest of this directory (`grep -rn` on each target line
# and on `repeatsReason`, `QuoteRepeats`, `shortcut-tray`, `loadStamp`,
# `languageReadStamp`, `languageMountStamp`, `loadFailureAnswersPress`,
# `readAnswersPress`, `loadLocaleChoice`, `opSeq`): no case below duplicates
# one already in `pr9-ui.sh`, `debt-ui-blind-tests.sh` or `pr10f-settings.sh`
# — the only file-name hits outside this file belong to
# `debt-ui-blind-tests.sh`'s own block-order cases, whose regex happens to
# quote these identifiers without judging them.
#
# Two probes are not cases here, because neither has a one-line form:
#
#   - a settle tied only to the hotkey catch's own corrective read. Every
#     read that takes the hotkey stamp settles a `pending` shortcut outcome
#     inside `refresh()`, and both the hotkey catch and the autostart catch
#     start that read the same way, `refresh('press')` — there is no single
#     line whose mutation ties the settle back to one catch alone.
#   - moving the language Retry marker to module scope. The marker is the
#     store's own `readAnswersPress` field (`ui/src/locale-choice.ts`), not a
#     component field, so it cannot be moved to a `<script module>` block the
#     way a component field can; the remount property that move would have
#     broken is pinned above by "the fresh instance's own read".

case_ "Application: the settled shortcut heading calls a combination «in effect» only when the read's own status is registered" \
  ui/src/settings/Application.svelte \
  "s~appliedHotkey\\.status\\.kind === 'registered' && appliedHotkey\\.shortcut === refusedShortcut~appliedHotkey.shortcut === refusedShortcut /* mutant: combination only */~" \
  '/* mutant: combination only */' \
  src/settings/Application.test.ts 'a combination the system still refuses is not called in effect, even when the read names it back' runner=vitest

case_ "Application: a rejected recording marks the shortcut outcome pending before its corrective read answers" \
  ui/src/settings/Application.svelte \
  "s~shortcutOutcome = 'pending';~// mutant: never marks the outcome pending~" \
  '// mutant: never marks the outcome pending' \
  src/settings/Application.test.ts 'until the corrective read answers, the heading claims neither «unchanged» nor «in effect»' runner=vitest

case_ "Application: the settled shortcut heading gets a fresh announcement key of its own" \
  ui/src/settings/Application.svelte \
  's~key: `shortcut-failed#\${shortcutOutcome}`,~key: "shortcut-failed" /* mutant: constant announcement key */,~' \
  '/* mutant: constant announcement key */' \
  src/settings/Application.test.ts 'the settled heading arrives as a new announcement, and the refusal sentence is not read again' runner=vitest

case_ "Application: a refusal repeats the standing reason only when the two sentences are equal, never on overlap" \
  ui/src/settings/Application.svelte \
  's~return reason !== null && error !== null && reason === error;~return reason !== null && error !== null && error.includes(reason); // mutant: substring match~' \
  '// mutant: substring match' \
  src/settings/Application.test.ts 'a refusal that differs from the standing reason by one full stop is quoted in full under the usual heading' runner=vitest

case_ "Application: a refusal that is a strict prefix of the standing reason is not treated as the same sentence, reverse containment" \
  ui/src/settings/Application.svelte \
  's~return reason !== null && error !== null && reason === error;~return reason !== null && error !== null && reason.includes(error); // mutant: reverse containment~' \
  '// mutant: reverse containment' \
  src/settings/Application.test.ts 'a refusal that is a strict prefix of the standing reason is quoted in full, not treated as the same sentence' runner=vitest

case_ "Application: an autostart refusal repeating its own standing reason is quoted once, not twice" \
  ui/src/settings/Application.svelte \
  "s~const autostartQuoteRepeats = \\\$derived\\(repeatsReason\\(autostartUnknown\\?\\.reason \\?\\? null, autostartError\\)\\);~const autostartQuoteRepeats = \\\$derived(false); // mutant: autostart never dedups~" \
  '// mutant: autostart never dedups' \
  src/settings/Application.test.ts 'autostart: a refusal whose sentence is the standing reason is not quoted twice either' runner=vitest

case_ "Application: the shortcut control is described by the reason paragraph once its refusal quote is dropped" \
  ui/src/settings/Application.svelte \
  "s~      : shortcutQuoteRepeats \\? 'application-shortcut-failed application-shortcut-reason'~      : false /* mutant: never the reason id */ ? 'application-shortcut-failed application-shortcut-reason'~" \
  '/* mutant: never the reason id */' \
  src/settings/Application.test.ts 'when the quote is left out, the control is described by the heading and the reason, and both exist' runner=vitest

case_ "Application: the shortcut failure heading stays on screen whether or not its quote is dropped" \
  ui/src/settings/Application.svelte \
  's~      <p id="application-shortcut-failed" data-testid="application-shortcut-failed" data-announced-by={ASSERTIVE_ID}>{shortcutFailedLabel}</p>~      {#if !shortcutQuoteRepeats}<p id="application-shortcut-failed" data-testid="application-shortcut-failed" data-announced-by={ASSERTIVE_ID}>{shortcutFailedLabel}</p>{/if} <!-- mutant: heading vanishes with the quote -->~' \
  '<!-- mutant: heading vanishes with the quote -->' \
  src/settings/Application.test.ts 'a refusal whose sentence is the standing reason is not quoted a second time, and the heading says so' runner=vitest

case_ "Application: an autostart refusal that differs from the standing reason keeps its own quote" \
  ui/src/settings/Application.svelte \
  "s~const autostartQuoteRepeats = \\\$derived\\(repeatsReason\\(autostartUnknown\\?\\.reason \\?\\? null, autostartError\\)\\);~const autostartQuoteRepeats = \\\$derived(autostartError !== null); // mutant: drops every autostart quote~" \
  '// mutant: drops every autostart quote' \
  src/settings/Application.test.ts 'autostart: a refusal different from the standing reason is quoted in full' runner=vitest

case_ "Application: a shortcut change still pending is judged against its own answer, never the pre-change reason" \
  ui/src/settings/Application.svelte \
  "s~shortcutOutcome === 'unchanged' && repeatsReason\\(unavailable\\?\\.reason \\?\\? null, hotkeyError\\),~repeatsReason(unavailable?.reason ?? null, hotkeyError), // mutant: no pending gate~" \
  '// mutant: no pending gate' \
  src/settings/Application.test.ts 'while the corrective read is out, a quote that repeats the reason stays under the neutral heading' runner=vitest

case_ "Application: the assertive region carries an autostart refusal's own sentence whether or not its quote is dropped from the screen" \
  ui/src/settings/Application.svelte \
  "s~      out\\.push\\(\\{ key: 'autostart-error', text: autostartError \\}\\);~      if (!autostartQuoteRepeats) out.push({ key: 'autostart-error', text: autostartError }); // mutant: silent when quote repeats~" \
  '// mutant: silent when quote repeats' \
  src/settings/Application.test.ts 'autostart: a refusal whose sentence is the standing reason is not quoted twice either' runner=vitest

case_ "Application: an unavailable shortcut's way-out sentence is announced, not only shown" \
  ui/src/settings/Application.svelte \
  "s~      out\\.push\\(\\{ key: 'shortcut-tray', text: shortcutTrayText \\}\\);~      // mutant: tray sentence never announced~" \
  '// mutant: tray sentence never announced' \
  src/settings/Application.test.ts 'an unavailable shortcut announces the way out, after the claim and its reason' runner=vitest

case_ "Application: the tray paragraph names the live region that speaks it" \
  ui/src/settings/Application.svelte \
  's~<p data-testid="application-shortcut-tray" data-announced-by={POLITE_ID}>{shortcutTrayText}</p>~<p data-testid="application-shortcut-tray">{shortcutTrayText}</p> <!-- mutant: tray names no region -->~' \
  '<!-- mutant: tray names no region -->' \
  src/settings/Application.test.ts 'every visible refusal paragraph names the region that speaks for it, and that region exists' runner=vitest

case_ "Application: a repeated load failure is announced again, node by node" \
  ui/src/settings/Application.svelte \
  's~      loadStamp \+= 1;~      // mutant: load failure key never advances~' \
  '// mutant: load failure key never advances' \
  src/settings/Application.test.ts 'a press whose corrective read also fails is heard again, in the assertive region only' runner=vitest

case_ "Application: a repeated language-read failure is announced again, node by node" \
  ui/src/settings/Application.svelte \
  's~\{ key: `language-failed#\${languageReadStamp}`, text: languageFailedLabel \},\n    \{ key: `language-error#\${languageReadStamp}`, text: languageReadError \},~{ key: `language-failed` /* mutant: constant key */, text: languageFailedLabel },\n    { key: `language-error`, text: languageReadError },~' \
  '/* mutant: constant key */' \
  src/settings/Application.test.ts 'a second rejected language change whose recovery read repeats the same refusal is heard again, politely' runner=vitest

case_ "Application: a rejected autostart change's corrective read is heard as the press's own answer" \
  ui/src/settings/Application.svelte \
  "s~autostartError = err instanceof Error \\? err\\.message : String\\(err\\);\\n      void refresh\\('press'\\);~autostartError = err instanceof Error ? err.message : String(err);\\n      void refresh('mount'); // mutant: autostart re-read is never a press~" \
  '// mutant: autostart re-read is never a press' \
  src/settings/Application.test.ts 'a corrective read refused after a rejected autostart change is announced at once' runner=vitest

case_ "Application: a standing language failure lands in a live region the instant a section remounts, never in neither" \
  ui/src/settings/Application.svelte \
  's~if \(!languageReadAnswersPress\) out\.push\(\.\.\.languageReadAnnouncements\);~if (!languageReadAnswersPress && languageReadStamp !== languageMountStamp) out.push(...languageReadAnnouncements); // mutant: drops from both regions~' \
  '// mutant: drops from both regions' \
  src/settings/Application.test.ts 'after a remount, a standing language read failure is reported politely again' runner=vitest

case_ "Application: a remounted section does not inherit the outgoing instance's press origin" \
  ui/src/settings/Application.svelte \
  's~languageReadStamp !== languageMountStamp,~true, // mutant: every instance inherits the old press~' \
  '// mutant: every instance inherits the old press' \
  src/settings/Application.test.ts 'after a remount, a standing language read failure is reported politely again' runner=vitest

case_ "Application: a corrective read a press started is heard assertively, not politely" \
  ui/src/settings/Application.svelte \
  "s~loadFailureAnswersPress = origin === 'press';~loadFailureAnswersPress = false; // mutant: a press's own re-read is never assertive~" \
  "// mutant: a press's own re-read is never assertive" \
  src/settings/Application.test.ts 'a corrective read refused after a press is announced at once, heading and sentence' runner=vitest

case_ "Application: a section's very first read is heard politely, never as though a press started it" \
  ui/src/settings/Application.svelte \
  "s~loadFailureAnswersPress = origin === 'press';~loadFailureAnswersPress = true; // mutant: every read is heard as a press~" \
  '// mutant: every read is heard as a press' \
  src/settings/Application.test.ts 'a rejected first read leaves no groups, but the polite region carries the load refusal' runner=vitest

case_ "locale-choice: only a read a press itself started marks its own failure as answering that press (never true)" \
  ui/src/locale-choice.ts \
  "s~readAnswersPress: origin === 'press',~readAnswersPress: true, // mutant: every failed read claims a press~" \
  '// mutant: every failed read claims a press' \
  src/locale-choice.test.ts 'a failed read started by a press sets readAnswersPress, a default one clears it' runner=vitest

case_ "locale-choice: only a read a press itself started marks its own failure as answering that press (never false)" \
  ui/src/locale-choice.ts \
  "s~readAnswersPress: origin === 'press',~readAnswersPress: false, // mutant: no failed read ever claims a press~" \
  '// mutant: no failed read ever claims a press' \
  src/locale-choice.test.ts 'a failed read started by a press sets readAnswersPress, a default one clears it' runner=vitest

case_ "Application: the language Retry button starts its read as a press" \
  ui/src/settings/Application.svelte \
  "s~onclick=\{\(\) => void loadLocaleChoice\('press'\)\}~onclick={() => void loadLocaleChoice() /* mutant: a Retry read is never a press */}~" \
  '/* mutant: a Retry read is never a press */' \
  src/settings/Application.test.ts 'pressing «Retry reading» for the language and being refused again is announced at once' runner=vitest

case_ "locale-choice: a Retry read writes nothing to the store until its own answer lands" \
  ui/src/locale-choice.ts \
  "s~(= 'mount'\): Promise<void> \{\n  if \(get\(state\)\.busy\) return;\n  const mine = \+\+opSeq;)~\$1 state.update((s) => ({ ...s, readAnswersPress: false })); // mutant: marks at press time~" \
  '// mutant: marks at press time' \
  src/settings/Application.test.ts "a second Retry press does not move the first press's own answer while its own read is in flight" runner=vitest

case_ "Application: a corrective read that is itself refused leaves a pending shortcut outcome pending" \
  ui/src/settings/Application.svelte \
  "s~      loadStamp \+= 1;~      loadStamp += 1; shortcutOutcome = 'unchanged'; // mutant: a refused read settles the outcome~" \
  '// mutant: a refused read settles the outcome' \
  src/settings/Application.test.ts 'a corrective read that is itself refused leaves the neutral heading standing, because it is still true' runner=vitest

case_ "locale-choice: a superseded read writes nothing to the store" \
  ui/src/locale-choice.ts \
  's~\} catch \(e\) \{\n    if \(mine !== opSeq\) return;\n    state\.update\(\(s\) => \(\{~} catch (e) {\n    // mutant: a superseded read still writes the store\n    state.update((s) => ({~' \
  '// mutant: a superseded read still writes the store' \
  src/settings/Application.test.ts "a language change made while a Retry read is in flight is answered, politely, by that change's own recovery read" runner=vitest

case_ "locale-choice: a rejected change's own recovery read is never heard as a press" \
  ui/src/locale-choice.ts \
  "s~    await loadLocaleChoice\(\);~    await loadLocaleChoice('press'); // mutant: recovery read claims a press~" \
  '// mutant: recovery read claims a press' \
  src/settings/Application.test.ts 'a recovery read refused after a rejected language change stays polite, even after an earlier Retry press' runner=vitest

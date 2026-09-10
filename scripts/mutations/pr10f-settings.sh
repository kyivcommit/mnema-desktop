# Mutation cases for PR 10f Task 3/4/5 — the model select's two-track-of-truth
# outcomes, the boot-revision guard for a direct (eventless) locale change,
# the partial-apply warning surviving a settings remount, and the job strip's
# non-modal disclosure (outside-focus close, focus restored when the panel
# itself disappears). Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr10f-settings.sh

# NOT the `writeOutcome = { kind: 'unknown', ... }` line itself when the
# recovery re-read SUCCEEDS (`failed_adoption_does_not_restore_cached_model`
# below) — that write is superseded, in this very catch block, by the
# `refresh()` call right after it: a successful re-read unconditionally sets
# `writeOutcome = null`, so whatever this line held never survives to render
# and mutating it here against THAT test reports STILL GREEN (checked, PR
# 10f Task 7 first pass). The line that actually reaches the screen
# unrebutted there is the one AFTER `refresh()` settles — so this case
# restores the just-picked model at that point instead, unconditionally.
case_ "Models.svelte: a rejected adoption restores the just-picked model anyway" \
  ui/src/settings/Models.svelte \
  "s~      await refresh\(\)\.catch\(\(\) => \{\}\);\n      jobRunning = await jobStatus\(\)~      await refresh().catch(() => {});\n      writeOutcome = { kind: 'acknowledged', role: 'embedding', model };\n      jobRunning = await jobStatus()~" \
  "writeOutcome = { kind: 'acknowledged', role: 'embedding', model };
      jobRunning = await jobStatus()" \
  src/settings/Models.test.ts 'failed_adoption_does_not_restore_cached_model' runner=vitest

# The counter-example the case above cannot reach, discharged rather than
# left disclosed: when the recovery re-read ALSO rejects, `refresh()`'s own
# catch branch never touches `writeOutcome` (only its success branch resets
# it), so the write's own `unknown` line IS what stays on screen — this
# mutates that line directly.
case_ "Models.svelte: a rejected adoption whose own re-read also fails restores a cached model anyway" \
  ui/src/settings/Models.svelte \
  "s~      writeOutcome = \{ kind: 'unknown', role: 'embedding' \};~      writeOutcome = { kind: 'acknowledged', role: 'embedding', model };~" \
  "writeOutcome = { kind: 'acknowledged', role: 'embedding', model };" \
  src/settings/Models.test.ts 'a rejected adoption whose own re-read also fails does not restore any cached model' runner=vitest

case_ "Models.svelte: a failed refresh erases the retirement report" \
  ui/src/settings/Models.svelte \
  's~      if \(seq === settingsSeq\) loadError = e instanceof Error \? e\.message : String\(e\);\n      throw e;~      if (seq === settingsSeq) loadError = e instanceof Error ? e.message : String(e);\n      if (seq === settingsSeq) retiredReport = null;\n      throw e;~' \
  'if (seq === settingsSeq) retiredReport = null;' \
  src/settings/Models.test.ts 'failed_read_does_not_erase_retirement_report' runner=vitest

case_ "i18n/index.ts: bootLocale drops the revision check" \
  ui/src/i18n/index.ts \
  's~  if \(localeRevision === revisionBeforeRead\) initLocale\(reply\.effective\);~  initLocale(reply.effective);~' \
  '  initLocale(reply.effective);' \
  src/i18n/boot.test.ts 'a direct change wins over an older boot snapshot without an event' runner=vitest

case_ "locale-choice.ts: loading the locale clears a partial-apply warning" \
  ui/src/locale-choice.ts \
  "s~    state\.update\(\(s\) => \(\{ \.\.\.s, snapshot: reply, error: null \}\)\);~    state.update((s) => ({ ...s, snapshot: reply, error: null, application: { kind: 'initial' } }));~" \
  "application: { kind: 'initial' } }));" \
  src/settings/Application.test.ts 'loading_locale_keeps_partial_application_warning' runner=vitest

case_ "JobStrip.svelte: an outside focus no longer closes the panel" \
  ui/src/settings/JobStrip.svelte \
  's~    if \(open\) open = false;~    if (false) open = false;~' \
  'if (false) open = false;' \
  src/settings/JobStrip.test.ts 'outside_focus_closes_the_panel' runner=vitest

case_ "JobStrip.svelte: a removed panel drops focus instead of returning it" \
  ui/src/settings/JobStrip.svelte \
  's~    if \(anything\) summaryEl\?\.focus\(\);\n    else focusFallback\(\);~    if (anything) summaryEl?.focus();~' \
  'if (anything) summaryEl?.focus();
  });' \
  src/settings/JobStrip.test.ts 'removed_panel_focus_returns_to_active_navigation' runner=vitest

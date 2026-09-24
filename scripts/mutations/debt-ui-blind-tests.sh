# Debt PR B (§15.5, 2026-09-17) — five UI tests that were green under the
# change they existed to catch, and the tests that replaced them. All vitest,
# so this file's matrix leg carries `node: true` in `.github/workflows/ci.yml`.
# Run with:
#
#   scripts/mutation-check.sh scripts/mutations/debt-ui-blind-tests.sh
#
# What is here, by name:
#
#   the dropped await     — bootLocale without `await listen(...)` registers
#                           the listener after the snapshot; the old mock
#                           assigned synchronously and could not see it
#   the four boot calls   — each entry point calls bootLocale() and
#                           bootTheme(); deleting any one left every suite
#                           green until main.test.ts mounted through them
#   the three orders      — a failure block moved past its control, or its
#                           two sentences swapped, in shortcut, appearance
#                           and startup (both control branches)
#   the missing value     — the embed sentence for a wire-only EndReason
#                           drawn without {reason} throws MissingValueError

case_ "bootLocale: the listener must be awaited before the snapshot is taken" \
  ui/src/i18n/index.ts \
  "s~  await listen<'uk' \\| 'en'>\\('locale-changed'~  void listen<'uk' | 'en'>('locale-changed'~" \
  "void listen<'uk' | 'en'>('locale-changed'" \
  src/i18n/boot.test.ts 'registers the locale-changed listener before taking the snapshot' runner=vitest

case_ "launcher/main.ts: bootLocale() must be called" \
  ui/src/launcher/main.ts \
  "s~bootLocale\\(\\)\\.catch\\(\\(err\\) => console\\.error\\('bootLocale failed', err\\)\\);~// mutant: no bootLocale~" \
  '// mutant: no bootLocale' \
  src/main.test.ts 'the launcher entry point boots the locale and the theme once each' runner=vitest

case_ "launcher/main.ts: bootTheme() must be called" \
  ui/src/launcher/main.ts \
  "s~bootTheme\\(\\)\\.catch\\(\\(err\\) => console\\.error\\('bootTheme failed', err\\)\\);~// mutant: no bootTheme~" \
  '// mutant: no bootTheme' \
  src/main.test.ts 'the launcher entry point boots the locale and the theme once each' runner=vitest

case_ "settings/main.ts: bootLocale() must be called" \
  ui/src/settings/main.ts \
  "s~bootLocale\\(\\)\\.catch\\(\\(err\\) => console\\.error\\('bootLocale failed', err\\)\\);~// mutant: no bootLocale~" \
  '// mutant: no bootLocale' \
  src/main.test.ts 'the settings entry point boots the locale and the theme once each' runner=vitest

case_ "settings/main.ts: bootTheme() must be called" \
  ui/src/settings/main.ts \
  "s~bootTheme\\(\\)\\.catch\\(\\(err\\) => console\\.error\\('bootTheme failed', err\\)\\);~// mutant: no bootTheme~" \
  '// mutant: no bootTheme' \
  src/main.test.ts 'the settings entry point boots the locale and the theme once each' runner=vitest

case_ "Application: the shortcut failure block must precede its control" \
  ui/src/settings/Application.svelte \
  's~(    \{#if hotkeyError !== null\}\n      <p id="application-shortcut-failed"[^\n]*\n      \{#if !shortcutQuoteRepeats\}\n        <p id="application-shortcut-error"[^\n]*\n      \{/if\}\n    \{/if\}\n)(    <button\n      type="button"\n      data-testid="application-shortcut-record"\n(?:[^\n]*\n)*?    >\{recordLabel\}</button>\n)~$2$1    <!-- mutant: shortcut failure after control -->\n~' \
  '<!-- mutant: shortcut failure after control -->' \
  src/settings/Application.test.ts 'the shortcut block reads label, status, failure, error, control — in that order' runner=vitest

case_ "Application: the shortcut failed line must precede the error line" \
  ui/src/settings/Application.svelte \
  's~(      <p id="application-shortcut-failed"[^\n]*\n)(      \{#if !shortcutQuoteRepeats\}\n)(        <p id="application-shortcut-error"[^\n]*\n)(      \{/if\}\n)~$2$3$4$1      <!-- mutant: shortcut error before failed -->\n~' \
  '<!-- mutant: shortcut error before failed -->' \
  src/settings/Application.test.ts 'the shortcut block reads label, status, failure, error, control — in that order' runner=vitest

case_ "Application: the theme failure block must precede its control" \
  ui/src/settings/Application.svelte \
  's~(    \{#if themeError !== null\}\n      <p id="application-theme-failed"[^\n]*\n      <p id="application-theme-error"[^\n]*\n    \{/if\}\n)((?:[^\n]*\n)*?    </div>\n\n)(?=    <p id="application-language-label">)~$2$1    <!-- mutant: theme failure after control -->\n~' \
  '<!-- mutant: theme failure after control -->' \
  src/settings/Application.test.ts 'the appearance block reads label, failure, error, control — in that order' runner=vitest

case_ "Application: the theme failed line must precede the error line" \
  ui/src/settings/Application.svelte \
  's~(      <p id="application-theme-failed"[^\n]*\n)(      <p id="application-theme-error"[^\n]*\n)~$2$1      <!-- mutant: theme error before failed -->\n~' \
  '<!-- mutant: theme error before failed -->' \
  src/settings/Application.test.ts 'the appearance block reads label, failure, error, control — in that order' runner=vitest

case_ "Application: the autostart failure block must precede the toggle" \
  ui/src/settings/Application.svelte \
  's~(    \{#if autostartError !== null\}\n      <p id="application-autostart-failed"[^\n]*\n      \{#if !autostartQuoteRepeats\}\n        <p id="application-autostart-error"[^\n]*\n      \{/if\}\n    \{/if\}\n)(    \{#if autostartOffersBothDirections\}\n(?:[^\n]*\n)*?    \{/if\}\n)~$2$1    <!-- mutant: autostart failure after control -->\n~' \
  '<!-- mutant: autostart failure after control -->' \
  src/settings/Application.test.ts 'the startup block reads label, status, failure, error, control — in that order, with one toggle' runner=vitest

case_ "Application: the autostart failure block must precede enable and disable" \
  ui/src/settings/Application.svelte \
  's~(    \{#if autostartError !== null\}\n      <p id="application-autostart-failed"[^\n]*\n      \{#if !autostartQuoteRepeats\}\n        <p id="application-autostart-error"[^\n]*\n      \{/if\}\n    \{/if\}\n)(    \{#if autostartOffersBothDirections\}\n(?:[^\n]*\n)*?    \{/if\}\n)~$2$1    <!-- mutant: autostart failure after control -->\n~' \
  '<!-- mutant: autostart failure after control -->' \
  src/settings/Application.test.ts 'the startup block reads label, status, reason, failure, error, control — in that order, with enable and disable' runner=vitest

case_ "Application: the autostart failed line must precede the error line" \
  ui/src/settings/Application.svelte \
  's~(      <p id="application-autostart-failed"[^\n]*\n)(      \{#if !autostartQuoteRepeats\}\n)(        <p id="application-autostart-error"[^\n]*\n)(      \{/if\}\n)~$2$3$4$1      <!-- mutant: autostart error before failed -->\n~' \
  '<!-- mutant: autostart error before failed -->' \
  src/settings/Application.test.ts 'the startup block reads label, status, failure, error, control — in that order, with one toggle' runner=vitest

case_ "JobStrip: the embed sentence must carry the reason it interpolates" \
  ui/src/settings/JobStrip.svelte \
  's~t\(EMBED_ENDED\[reason\], \{ reason \}\);~t(EMBED_ENDED[reason]); // mutant: no reason value~' \
  '// mutant: no reason value' \
  src/settings/JobStrip.test.ts 'a wire-only reason that ends the embedding phase is named in the sentence, not thrown' runner=vitest

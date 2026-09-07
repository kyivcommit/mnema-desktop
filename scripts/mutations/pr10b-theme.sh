# PR 10b — the theme: `src-tauri/src/theme.rs`, `ui/src/theme.ts`, and the
# segment in `ui/src/settings/Application.svelte`. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr10b-theme.sh
#
# What is here, by name:
#
#   the fallback        — an unknown or missing `theme` reads as System, not as
#                          whichever arm a refactor puts first
#   the merge            — a write keeps the locale and the shortcut beside it,
#                          and cannot take them with it
#   the atomic write     — a write that fails leaves the previous choice on disk
#   the forced map       — System tells the runtime "follow the OS" (`None`),
#                          never a frozen copy of what the OS says now
#   set_theme unregistered — the window cannot reach it if it is not wired in
#   the absence          — the system choice REMOVES `data-theme`; it does not
#                          write "system" into it
#   the boot order        — the listener is up before the snapshot, and the
#                          snapshot does not overwrite a live event
#   the pressed button    — follows the store, one direction only
#   the reply             — a successful `set_theme` moves the store; without
#                          that line the button waits on a broadcast the test
#                          never sends and the window would wait on its echo
#   the busy guard        — one call per change, however many presses
#   the sentence           — a refusal shows the backend's words
#
# Not here, and why: `apply_to_windows` calling `set_theme` on each window is
# invisible under the mock runtime (its `set_theme` is `Ok(())` and records
# nothing) — the same limit `prefs::set_hotkey`'s tray relabel runs into. The
# live run is the check.

case_ "theme: an unknown value must fall back to System, not to Dark" \
  src-tauri/src/theme.rs \
  's~        _ => ThemeChoice::System, // "system" and anything unknown~        _ => ThemeChoice::Dark, // mutant: unknown becomes dark~' \
  '_ => ThemeChoice::Dark, // mutant: unknown becomes dark' \
  mnema-desktop 'theme::tests::unreadable_or_unknown_prefs_fall_back_to_system' --lib

case_ "theme: a write that bypasses write_key drops the other keys" \
  src-tauri/src/theme.rs \
  's~    crate::prefs::write_key\(\n        data_dir,\n        THEME_KEY,\n        serde_json::Value::String\(choice_to_str\(choice\)\.into\(\)\),\n    \)~    std::fs::write(crate::paths::prefs_path(data_dir), format!("{{\\"theme\\":\\"{}\\"}}", choice_to_str(choice))) // mutant: whole-file write~' \
  'std::fs::write(crate::paths::prefs_path(data_dir), format!("{{\"theme\":\"{}\"}}", choice_to_str(choice))) // mutant: whole-file write' \
  mnema-desktop 'theme::tests::write_preserves_foreign_keys' --lib

case_ "theme: an in-place write overwrites the file even when the dir is read-only" \
  src-tauri/src/theme.rs \
  's~    crate::prefs::write_key\(\n        data_dir,\n        THEME_KEY,\n        serde_json::Value::String\(choice_to_str\(choice\)\.into\(\)\),\n    \)~    std::fs::write(crate::paths::prefs_path(data_dir), format!("{{\\"theme\\":\\"{}\\"}}", choice_to_str(choice))) // mutant: whole-file write~' \
  'std::fs::write(crate::paths::prefs_path(data_dir), format!("{{\"theme\":\"{}\"}}", choice_to_str(choice))) // mutant: whole-file write' \
  mnema-desktop 'theme::tests::failed_write_keeps_the_previous_choice' --lib

case_ "theme: System must be None, not a forced copy of the current OS theme" \
  src-tauri/src/theme.rs \
  's~        ThemeChoice::System => None,~        ThemeChoice::System => Some(Theme::Light), // mutant: frozen~' \
  'ThemeChoice::System => Some(Theme::Light), // mutant: frozen' \
  mnema-desktop 'theme::tests::system_forces_nothing_and_each_explicit_choice_forces_itself' --lib

case_ "theme: set_theme unregistered — the window cannot reach it" \
  src-tauri/src/lib.rs \
  's~        theme::set_theme,\n~~' \
  '        theme::get_theme,
        prefs::app_prefs,' \
  mnema-desktop 'set_theme_persists_the_choice_and_get_theme_reads_it_back_through_the_ipc' --test commands

case_ "theme.ts: the system choice must remove the attribute, not write it" \
  ui/src/theme.ts \
  "s~  if \(choice === 'system'\) delete document\.documentElement\.dataset\.theme;\n  else document\.documentElement\.dataset\.theme = choice;~  document.documentElement.dataset.theme = choice; // mutant: system is written~" \
  "document.documentElement.dataset.theme = choice; // mutant: system is written" \
  src/theme.test.ts 'an explicit choice sets data-theme and the system choice removes it, not renames it' runner=vitest

case_ "theme.ts: the snapshot must not overwrite a live event that landed during boot" \
  ui/src/theme.ts \
  "s~  if \(!liveEventSeen\) theme\.set~  theme.set~" \
  "  theme.set(isThemeChoice(reply.choice) ? reply.choice : 'system');" \
  src/theme.test.ts 'a switch during boot wins over the stale snapshot reply' runner=vitest

case_ "theme.ts: the listener must go up before the snapshot is taken" \
  ui/src/theme.ts \
  "s~  let liveEventSeen = false;\n  await listen<string>\('theme-changed', \(e\) => \{\n    liveEventSeen = true;\n    theme\.set\(isThemeChoice\(e\.payload\) \? e\.payload : 'system'\);\n  \}\);\n  const reply = await invoke<\{ choice: string \}>\('get_theme'\);~  let liveEventSeen = false;\n  const reply = await invoke<{ choice: string }>('get_theme'); // mutant: snapshot before listener\n  await listen<string>('theme-changed', (e) => {\n    liveEventSeen = true;\n    theme.set(isThemeChoice(e.payload) ? e.payload : 'system');\n  });~" \
  "const reply = await invoke<{ choice: string }>('get_theme'); // mutant: snapshot before listener" \
  src/theme.test.ts 'registers the theme-changed listener before taking the snapshot' runner=vitest

case_ "Application: the pressed button must follow the store, not its negation" \
  ui/src/settings/Application.svelte \
  "s~      aria-pressed=\{\\\$theme === 'dark'\}~      aria-pressed={\\\$theme !== 'dark'} // mutant~" \
  "aria-pressed={\$theme !== 'dark'} // mutant" \
  src/settings/Application.test.ts 'the theme segment presses the button for the choice the store holds, and no other' runner=vitest

case_ "Application: a successful set_theme must move the store" \
  ui/src/settings/Application.svelte \
  "s~      theme\.set\(choice\);\n~~" \
  "      // wait on its own echo. One store, one attribute writer, same value.
    } catch (err) {" \
  src/settings/Application.test.ts 'choosing a theme sends that choice once, and the pressed button and the document follow the reply' runner=vitest

case_ "Application: the busy guard must hold, not just disable" \
  ui/src/settings/Application.svelte \
  "s~    if \(themeBusy\) return;\n~~" \
  "    // reaches a listener.
    themeError = null;" \
  src/settings/Application.test.ts 'the theme segment is busy while a change is in flight, so a double press sends one call' runner=vitest

case_ "Application: a refusal must show the backend sentence" \
  ui/src/settings/Application.svelte \
  "s~      themeError = err instanceof Error \? err\.message : String\(err\);~      void err; // mutant: swallowed~" \
  "void err; // mutant: swallowed" \
  src/settings/Application.test.ts 'a refused theme change shows the backend sentence and leaves the pressed button where it was' runner=vitest

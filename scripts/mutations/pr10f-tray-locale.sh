# Mutation cases for PR 10f Task 1/2/3 — install-before-publish, the tray's
# retry bookkeeping, its single Stop/Resume slot, and the locale apply
# pipeline (persist/commit/apply ordering, every-surface attempt, and the
# same-choice retry path). Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr10f-tray-locale.sh

case_ "tray: install_then_publish publishes before installing" \
  src-tauri/src/tray.rs \
  's~    install\(menu\)\?;\n    publish\(handles\);~    publish(handles);\n    install(menu)?;~' \
  'publish(handles);
    install(menu)?;' \
  mnema-desktop 'tray::tests::failed_install_does_not_publish_candidate' --lib

case_ "tray: a failed refresh forgets to owe a retry" \
  src-tauri/src/tray.rs \
  's~    fn record_failure\(&mut self\) \{\n        self\.retry = true;~    fn record_failure(&mut self) {\n        self.retry = false;~' \
  'fn record_failure(&mut self) {
        self.retry = false;' \
  mnema-desktop 'tray::tests::failed_refresh_retries_without_mode_change' --lib

case_ "tray: the action slot never answers None" \
  src-tauri/src/tray.rs \
  's~    \} else if resume_entry\(scan\)\.is_some\(\) \{\n        Some\(TrayAction::Resume\)\n    \} else \{\n        None\n    \}~    } else if resume_entry(scan).is_some() {\n        Some(TrayAction::Resume)\n    } else {\n        Some(TrayAction::Resume)\n    }~' \
  '} else if resume_entry(scan).is_some() {
        Some(TrayAction::Resume)
    } else {
        Some(TrayAction::Resume)
    }' \
  mnema-desktop 'tray::tests::tray_action_matches_the_snapshot' --lib

case_ "locale: apply_choice_with zeroes out apply_errors on the wire" \
  src-tauri/src/locale.rs \
  's~        apply_errors,\n    \}\)~        apply_errors: Vec::new(),\n    })~' \
  'apply_errors: Vec::new(),' \
  mnema-desktop 'locale::tests::saved_locale_survives_apply_failure' --lib

case_ "locale: apply_surfaces stops at the first refusal" \
  src-tauri/src/locale.rs \
  's~        if let Err\(message\) = run\(surface\) \{\n            errors\.push\(LocaleApplyError \{ surface, message \}\);\n        \}\n    \}~        if let Err(message) = run(surface) {\n            errors.push(LocaleApplyError { surface, message });\n            return errors;\n        }\n    }~' \
  'errors.push(LocaleApplyError { surface, message });
            return errors;' \
  mnema-desktop 'locale::tests::apply_attempts_every_surface' --lib

case_ "locale: apply_choice_with skips apply for a repeated choice" \
  src-tauri/src/locale.rs \
  's~    write_choice\(data_dir, choice\)\?; // a write failure surfaces \(spec §6\)\n    let saved = LocaleState \{\n        choice,\n        effective: resolve\(choice, os\),\n    \};\n    commit_state\(saved\);\n    let apply_errors = apply\(saved\.effective\);~    let previous = read_choice(data_dir);\n    write_choice(data_dir, choice)?; // a write failure surfaces (spec §6)\n    let saved = LocaleState {\n        choice,\n        effective: resolve(choice, os),\n    };\n    commit_state(saved);\n    let apply_errors = if previous == choice { Vec::new() } else { apply(saved.effective) };~' \
  'let apply_errors = if previous == choice { Vec::new() } else { apply(saved.effective) };' \
  mnema-desktop 'locale::tests::same_choice_retries_application' --lib

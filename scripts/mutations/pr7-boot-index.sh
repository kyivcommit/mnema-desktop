# The start-up open, and the answer it records. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr7-boot-index.sh
#
# The behaviour under test had no caller at all until this commit: `open_index`
# existed, was registered, and was invoked by nothing outside the test suite. So
# these cases are less "does the test still catch a regression" than "does any
# test notice this line going away" — which is the question the missing caller
# itself failed for the length of the project.
#
# Three cases, because the change is three claims and each fails on its own:
# the boot opens the index; a failed open is kept rather than logged and
# dropped; and what was kept is what the window is told.
#
# ⚠️ **The call site is deliberately NOT a case here, and that is a gap, not an
# oversight.** What proves `.setup` calls `boot_index` is
# `a_real_launch_creates_the_index_in_its_data_directory` in `launch_smoke.rs`,
# and this harness cannot run it: `mutation-check.sh` builds
# `cargo test -p <pkg> <args> -- --exact <test>` and owns the `--` itself, so no
# case can pass `--include-ignored` to libtest, and an `#[ignore]`d test selected
# by `--exact` reports `0 passed` — which this harness prints as
# `BASELINE FAILURE: no test named …`. The case would have failed as broken
# rather than as a killed mutant. Independently, the mutation job runs on
# `ubuntu-24.04`, the one platform `launch_smoke.rs` says its GUI tests must not
# run on. That guard is held by CI's macOS leg instead.

# The open becomes a fabricated success: nothing is created, nothing is
# connected, and `AppState::db` stays `None` exactly as it did before this
# commit — the P0 restored in one line, with the recording still in place so
# that only the opening is under test.
case_ "the boot must open the index, not merely claim it did" \
  src-tauri/src/lib.rs \
  's{    let outcome = state\.open_index\(\);}{    let outcome = Ok::<(std::path::PathBuf, i64), error::Error>((std::path::PathBuf::new(), 0));}' \
  '    let outcome = Ok::<(std::path::PathBuf, i64), error::Error>((std::path::PathBuf::new(), 0));' \
  mnema-desktop 'the_boot_opens_the_index' --test commands

# Log it and forget it — the shape the whole second half of this task exists to
# rule out. The index still fails to open, the terminal still says so, and the
# person reading the settings screen is told the ordinary start-up story.
case_ "a failed start-up open must be recorded, not just logged" \
  src-tauri/src/lib.rs \
  's{    state\.set_boot_open_error\(outcome\.err\(\)\.map\(\|e\| e\.to_string\(\)\)\);}{    state.set_boot_open_error(None);}' \
  '    state.set_boot_open_error(None);' \
  mnema-desktop 'a_failed_boot_open_reaches_the_window_as_read_failed' --test commands

# The record is kept and then thrown away at the point of use: `index_settings`
# reads it, matches on it, and reports `NotOpen` anyway. Anchored on the arm's
# own `reason,` line because `UnreadableCause::ReadFailed` also appears in
# `UnreadableCause::of`, one screen above.
case_ "what the boot recorded must reach the window as ReadFailed" \
  src-tauri/src/models.rs \
  's{cause: UnreadableCause::ReadFailed,\n                reason,}{cause: UnreadableCause::NotOpen,\n                reason,}' \
  'cause: UnreadableCause::NotOpen,
                reason,' \
  mnema-desktop 'a_failed_boot_open_reaches_the_window_as_read_failed' --test commands

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
# Five cases. The first three are the change's three claims, each failing on
# its own: the boot opens the index; a failed open is kept rather than logged
# and dropped; and what was kept is what the window is told. The last two
# strengthen the assertions the first two lean on, once a file existing on
# disk and a `cause` discriminant turned out not to be enough on their own:
# a boot that opens a connection nothing keeps still creates the file, and a
# `reason` nobody reads can be any string at all, including the one sentence
# this whole task exists to stop the window from showing at the wrong time.
#
# ⚠️ **The call site is deliberately NOT a case here, and that is a gap, not an
# oversight.** What proves `.setup` calls `boot_index` is
# `a_real_launch_survives_startup_and_opens_its_index` in `launch_smoke.rs`,
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

# A file on disk is not the same claim as `AppState::db` holding the
# connection: this routes the open through `open_job_index` instead of
# `open_index`, which still creates and migrates the same file — `open` does
# not know or care which caller asked — but returns a connection nothing
# stores, leaving `db` at `None` exactly as it was before this task. Only
# `the_boot_opens_the_index`'s second assertion, added past the file-exists
# check, can see this; the first cannot.
case_ "the boot must store the connection the window reads, not merely create the file" \
  src-tauri/src/lib.rs \
  's{    let outcome = state\.open_index\(\);}{    let outcome: Result<(std::path::PathBuf, i64), error::Error> = state.open_job_index().map(|_| (std::path::PathBuf::new(), 0));}' \
  '    let outcome: Result<(std::path::PathBuf, i64), error::Error> = state.open_job_index().map(|_| (std::path::PathBuf::new(), 0));' \
  mnema-desktop 'the_boot_opens_the_index' --test commands

# `cause: ReadFailed` survives untouched; only the sentence beside it changes,
# back to what a bare `IndexNotOpen` would have said instead of what the
# boot's own open actually failed on. A discriminant-only assertion cannot see
# this — it is exactly why `a_failed_boot_open_reaches_the_window_as_read_failed`
# now binds `reason` instead of matching it with `..`.
case_ "the reason shown must be the boot's own diagnosis, not IndexNotOpen's fixed sentence" \
  src-tauri/src/models.rs \
  's{cause: UnreadableCause::ReadFailed,\n                reason,}{cause: UnreadableCause::ReadFailed,\n                reason: e.to_string(),}' \
  'cause: UnreadableCause::ReadFailed,
                reason: e.to_string(),' \
  mnema-desktop 'a_failed_boot_open_reaches_the_window_as_read_failed' --test commands

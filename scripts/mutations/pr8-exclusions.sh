# The three commands that read and write a folder exclusion, and the walk
# that applies what they wrote — the cases runnable on any UNIX CI leg
# (review round 3, Minor N2: "any CI leg" was one word too strong — see
# below). Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr8-exclusions.sh
#
# Thirteen cases. Eleven against `list_exclusions`, `exclude_subfolder` or
# `include_subfolder` (`src-tauri/src/bridge.rs`) and the tests written for
# task-2 of PR 8a; two against `start_walk_job` (`src-tauri/src/walk_job.rs`)
# and the tests written for task-3, which is where a stored rule stops being
# a row and starts removing files. Twelve run in `tests/commands.rs`
# (`--test commands`) and one in `bridge.rs`'s own `mod tests` (`--lib`) —
# the last because the site it names, a per-entry `io::Error` from a
# directory listing, cannot be forced out of a real filesystem and so is
# reached through `entry_named`'s iterator parameter rather than through the
# IPC (review round 4, N3).
#
# ⚠️ **"Any unix leg", not "any CI leg" (review round 3, Minor N2).** Four of
# these thirteen cases name `#[cfg(unix)]` tests (the dangling-symlink root, and
# three of the `prefix_exists_on_disk` permission/kind cases) — harmless on
# this repository's two CI legs (`ubuntu-24.04`, `macos-14`, both unix), but
# a claim of "any CI leg" is false the moment a Windows leg exists, and would
# fail every case in this file the same way Important A did, for the same
# mechanism: the harness's baseline `--exact` selects nothing for a test that
# does not exist under that `#[cfg]`, reads as a `BASELINE FAILURE`, and
# exits 1 for the whole file before any mutation runs.
#
# ⚠️ **This file's sibling, `pr8-exclusions-macos.sh`, must be run too and is
# NOT a case here.** Its one case names a `#[cfg(target_os = "macos")]` test
# — review round 2, Important A: a case naming a test that does not exist on
# a platform makes the harness's baseline pass read `0 passed` for it and
# exit 1 for the WHOLE FILE before any mutation runs, so keeping that case
# here would silently drop every other case in this file on a non-macOS CI
# leg. Splitting is the fix, not a `#[cfg]`-agnostic rewrite of the test:
# ungating it would make the mutant it names equivalent on a case-sensitive
# filesystem (a naive `symlink_metadata` on a mismatched-case path is already
# `Err` there), so the case would report STILL GREEN on Linux instead of a
# clean baseline failure — the property really is macOS/Windows-only.
#
# ⚠️ **The narrower mutant round 1 asked for — narrowing the probe from the
# whole exclusion set back to the candidate alone — is deliberately NOT a
# case here.** `WalkRules::new` never builds an aggregate pattern set in the
# first place (it validates one prefix at a time, `rules.rs:200-205`), so
# "narrow the probe" and "the probe as written" are the same code today; a
# case built from that description would be born alive and killed by
# nothing. See `bridge::exclude_subfolder`'s own doc comment.
#
# ⚠️ **Review round 3's ruling: one classifier (`path_error_is_an_answer`),
# used at every site where a filesystem ERROR becomes `exists_on_disk`,
# rather than another one-off patch at another site.** Round 3's own
# enumeration of those sites then missed one (review round 4, N3: the
# per-entry `io::Result<DirEntry>` that `.flatten()` discarded), so round 4
# re-derived the set from the file rather than extending the list. It is
# enumerated in `prefix_exists_on_disk`'s doc comment and has FOUR entries
# there — `read_dir` failing, a single entry failing mid-iteration, a clean
# listing holding no such name, and the final `symlink_metadata` — plus a
# fifth in this file, `list_exclusions`'s root guard (`!root.is_dir()`),
# which deliberately does NOT call the classifier: its output is a refusal
# of the whole call rather than a boolean about a rule, so the harm the
# classifier prevents cannot arise there. Cases below cover entries 1, 2 and
# 4 individually; entry 3 needs no classifier and is covered by the
# whole-function case.

# The refusal itself, deleted. `if let Ok(_) = ... {}` still compiles and the
# `?` that used to propagate `RulesError` is gone, so a prefix `WalkRules`
# would refuse — `..` — is silently accepted and stored instead.
case_ "an invalid prefix must still be refused by WalkRules::new" \
  src-tauri/src/bridge.rs \
  's{    mnema_walk::WalkRules::new\(true, true, vec!\[relative_path\.clone\(\)\]\)\?;}{    let _ = mnema_walk::WalkRules::new(true, true, vec![relative_path.clone()]);}' \
  'let _ = mnema_walk::WalkRules::new(true, true, vec![relative_path.clone()]);' \
  mnema-desktop 'excluding_dotdot_is_refused_and_stores_nothing' --test commands

# The blank guard, disarmed. `validate_prefix` answers `Ok(None)` for the
# empty string — not an error — so with the guard gone `exclude_subfolder`
# would let a blank row through and it would sit in the list looking like
# protection (review round 1, P2).
case_ "the empty string must still be refused before it reaches Db::add_path_exclusion" \
  src-tauri/src/bridge.rs \
  's{    if relative_path\.is_empty\(\) \{}{    if false \{}' \
  'if false {' \
  mnema-desktop 'excluding_the_empty_string_is_refused_and_does_not_change_the_row_count' --test commands

# `Db::add_path_exclusion`'s own `ON CONFLICT DO NOTHING` is what makes
# pressing "exclude" twice one rule instead of an error. Removed, the second
# INSERT of the same prefix hits the partial unique index (`ux_ignore_rule_
# path`, migration 3) head-on and `add_path_exclusion` starts returning `Err`
# for the ordinary case of excluding an already-excluded folder.
case_ "excluding an already-excluded folder a second time must not become an error" \
  crates/mnema-index/src/write.rs \
  's{VALUES \(\?1, \?2, NULL\)\n             ON CONFLICT DO NOTHING",}{VALUES (?1, ?2, NULL)",}' \
  'VALUES (?1, ?2, NULL)",' \
  mnema-desktop 'excluding_the_same_subfolder_twice_is_idempotent' --test commands

# `prefix_exists_on_disk` collapsed to a constant. Kills on the fixture's
# rename half: a prefix whose folder was renamed away must read false, and a
# mutant that hands back `true` unconditionally gets it wrong regardless of
# what the byte-exact resolution or the permission-aware fallback below would
# have found.
case_ "existsOnDisk must come from a real filesystem lookup, not a constant" \
  src-tauri/src/bridge.rs \
  's~    let mut current = root\.to_path_buf\(\);\n    for component in prefix\.split\('"'"'/'"'"'\) \{\n        match std::fs::read_dir\(&current\)\.and_then\(\|entries\| entry_named\(entries, component\)\) \{\n            Ok\(Some\(path\)\) => current = path,\n            Ok\(None\) => return false,\n            Err\(e\) if path_error_is_an_answer\(e\.kind\(\)\) => return false,\n            Err\(_\) => return true,\n        \}\n    \}\n    match std::fs::symlink_metadata\(&current\) \{\n        Ok\(_\) => true,\n        Err\(e\) if path_error_is_an_answer\(e\.kind\(\)\) => false,\n        Err\(_\) => true,\n    \}~    /* mutant: always true */\n    true~' \
  '/* mutant: always true */' \
  mnema-desktop 'list_exclusions_reports_whether_each_stored_prefix_is_still_on_disk' --test commands

# Review round 3, N1. `path_error_is_an_answer`'s own classification, the
# ONE seam every error site in this file now shares, mutated at its centre:
# `NotADirectory` moved back off the "answer" side. An ancestor replaced by
# a file of the same name (`ENOTDIR`) would then read `existsOnDisk: true`
# again — the state that does not lift on its own, unlike the observer
# conditions this split exists to protect.
case_ "NotADirectory must classify as an answer about the path, not an observer condition" \
  src-tauri/src/bridge.rs \
  's{        std::io::ErrorKind::NotFound \| std::io::ErrorKind::NotADirectory}{        std::io::ErrorKind::NotFound}' \
  '        std::io::ErrorKind::NotFound' \
  mnema-desktop 'a_rule_under_an_ancestor_that_became_a_file_reports_not_on_disk' --test commands

# Review round 3, "not introduced by this diff but the lead should see it".
# `prefix_exists_on_disk`'s FINAL `symlink_metadata` call, reverted to the
# bare `.is_ok()` every error site used to share before this round's classifier —
# folding a `PermissionDenied` reached only at this last step (an ancestor
# that is listable but not traversable, mode `0o444`) back into `false`.
case_ "the final stat must classify its own errors too, not fold every one into false" \
  src-tauri/src/bridge.rs \
  's{    match std::fs::symlink_metadata\(&current\) \{\n        Ok\(_\) => true,\n        Err\(e\) if path_error_is_an_answer\(e\.kind\(\)\) => false,\n        Err\(_\) => true,\n    \}}{    std::fs::symlink_metadata(&current).is_ok()}' \
  'std::fs::symlink_metadata(&current).is_ok()' \
  mnema-desktop 'a_rule_whose_final_stat_needs_a_non_traversable_ancestor_reports_present_not_stale' --test commands

# Review round 1, Important 1. The root-unavailable guard, disarmed
# entirely. Without it, an unmounted drive or a moved folder makes every
# stored prefix answer `existsOnDisk: false` instead of refusing the whole
# call — the same field lying that a per-prefix `.unwrap_or(false)` produced
# before fix round 1, now for the entire list at once.
case_ "an unreachable watched root must refuse the whole list_exclusions call" \
  src-tauri/src/bridge.rs \
  's{    if !root_path\.is_dir\(\) \{}{    if false \{}' \
  'if false {' \
  mnema-desktop 'list_exclusions_refuses_when_the_root_itself_is_unreachable' --test commands

# Review round 2, Minor B. The guard is present but weakened back to what
# fix round 1 shipped: `symlink_metadata(..).is_err()` instead of
# `!root.is_dir()`. `symlink_metadata` resolves fine for a symlink whose
# TARGET is gone, so a dangling-symlink root would pass this weaker guard
# and land in Important 1's own failure mode one line later.
case_ "a dangling symlink root must be caught by is_dir(), not the weaker symlink_metadata check" \
  src-tauri/src/bridge.rs \
  's~    if !root_path\.is_dir\(\) \{~    if std::fs::symlink_metadata(root_path).is_err() {~' \
  'std::fs::symlink_metadata(root_path).is_err()' \
  mnema-desktop 'list_exclusions_refuses_when_the_root_is_a_dangling_symlink' --test commands

# Review round 2, Minor C. `read_dir`'s "I could not look" branch, reverted
# to the pre-fix "I could not look" == "it is not there" collapse: a rule
# under a listable-but-unreadable ancestor (`Work` at `--x--x--x`) would
# report `existsOnDisk: false` again, a live rule read as stale.
case_ "a rule under an unreadable ancestor must not read stale" \
  src-tauri/src/bridge.rs \
  's{            Err\(_\) => return true,}{            Err(_) => return false,}' \
  'Err(_) => return false,' \
  mnema-desktop 'a_rule_under_an_unreadable_ancestor_reports_present_not_stale' --test commands

# Review round 4, N3. The fourth site of `prefix_exists_on_disk`'s
# enumeration — the per-entry `io::Result<DirEntry>` — reverted to exactly
# what stood there through fix rounds 1, 2 and 3: `.flatten()`'s behaviour,
# an `Err` silently dropped. A directory entry that could not be read then
# becomes indistinguishable from a name that is not there, the loop falls to
# `Ok(None) => return false`, and a live rule reads as stale — the same
# under-exclusion direction as the three sites the earlier rounds fixed, one
# call deeper. `--lib`, not `--test commands`: a per-entry error cannot be
# forced out of a real filesystem deterministically, so the site is reached
# through `entry_named`'s iterator parameter, which is what that parameter
# is for.
case_ "a directory entry that could not be read must not answer as an absent name" \
  src-tauri/src/bridge.rs \
  's{        let entry = entry\?;}{        let Ok(entry) = entry else \{ continue \};}' \
  'let Ok(entry) = entry else { continue };' \
  mnema-desktop 'bridge::tests::a_directory_entry_that_cannot_be_read_is_an_error_not_an_absence' --lib

# `include_subfolder`'s answer, forced to `true` regardless of what
# `Db::remove_path_exclusion` actually found. Task 5's stale-rule control
# needs "removed" told apart from "there was nothing there"; a command that
# always says "removed" cannot tell them apart.
case_ "include_subfolder must report false when there was nothing to remove" \
  src-tauri/src/bridge.rs \
  's{    state\.with_index\(\|db\| db\.remove_path_exclusion\(root_id, &relative_path\)\)}{    state.with_index(|db| db.remove_path_exclusion(root_id, \&relative_path).map(|_| true))}' \
  'state.with_index(|db| db.remove_path_exclusion(root_id, &relative_path).map(|_| true))' \
  mnema-desktop 'including_a_subfolder_removes_the_rule_and_reports_whether_a_row_went' --test commands

# Task 3. The rules the walk runs with, reverted to exactly what stood in
# `walk_job.rs` until this task: the built-in list and `.gitignore`, and no
# user prefixes at all. Everything else still works — the commands still
# store the rule, `list_exclusions` still reports it, the walk still
# completes — and the rule does nothing. That is the shape this whole task
# exists to close, and it is invisible to every case above, all of which stop
# at the row in the database.
case_ "the walk must build its rules from the stored prefixes, not from an empty list" \
  src-tauri/src/walk_job.rs \
  's{    let rules = WalkRules::new\(true, true, user_prefixes\)\?;}{    let rules = WalkRules::new(true, true, Vec::new())?;}' \
  'let rules = WalkRules::new(true, true, Vec::new())?;' \
  mnema-desktop 'a_walk_applies_a_stored_exclusion_and_removes_what_it_now_covers' --test commands

# Task 3, and the risk the brief names as the whole risk of the task. The
# `RulesError` path, swallowed: `unwrap_or_default()` walks on with
# `WalkRules::default()` instead of refusing. A prefix stored by an older
# build, or written straight through `Db::add_path_exclusion` (which does not
# validate — that is the command's job), would then be silently absent from a
# walk that reports `completed`, and under D29 every file it should have
# covered goes to a third-party provider. `Vec::new()` above is the same
# failure by omission; this is it by exception handling, and no test above
# can tell either from a working walk.
case_ "a stored prefix that cannot become a rule must refuse the job, not be walked around" \
  src-tauri/src/walk_job.rs \
  's{    let rules = WalkRules::new\(true, true, user_prefixes\)\?;}{    let rules = WalkRules::new(true, true, user_prefixes).unwrap_or_default();}' \
  'let rules = WalkRules::new(true, true, user_prefixes).unwrap_or_default();' \
  mnema-desktop 'a_stored_exclusion_that_no_longer_validates_refuses_the_walk' --test commands

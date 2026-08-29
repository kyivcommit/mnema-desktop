# The three commands that read and write a folder exclusion — the portable
# cases, runnable on any CI leg. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr8-exclusions.sh
#
# Eight cases against `list_exclusions`, `exclude_subfolder` or
# `include_subfolder` (`src-tauri/src/bridge.rs`) and the `tests/commands.rs`
# tests written for task-2 of PR 8a.
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
  's~    let mut current = root\.to_path_buf\(\);\n    for component in prefix\.split\('"'"'/'"'"'\) \{\n        let entries = match std::fs::read_dir\(&current\) \{\n            Ok\(entries\) => entries,\n            Err\(e\) if e\.kind\(\) == std::io::ErrorKind::NotFound => return false,\n            Err\(_\) => return true,\n        \};\n        match entries\n            \.flatten\(\)\n            \.find\(\|entry\| entry\.file_name\(\) == std::ffi::OsStr::new\(component\)\)\n        \{\n            Some\(entry\) => current = entry\.path\(\),\n            None => return false,\n        \}\n    \}\n    std::fs::symlink_metadata\(&current\)\.is_ok\(\)~    /* mutant: always true */\n    true~' \
  '/* mutant: always true */' \
  mnema-desktop 'list_exclusions_reports_whether_each_stored_prefix_is_still_on_disk' --test commands

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

# `include_subfolder`'s answer, forced to `true` regardless of what
# `Db::remove_path_exclusion` actually found. Task 5's stale-rule control
# needs "removed" told apart from "there was nothing there"; a command that
# always says "removed" cannot tell them apart.
case_ "include_subfolder must report false when there was nothing to remove" \
  src-tauri/src/bridge.rs \
  's{    state\.with_index\(\|db\| db\.remove_path_exclusion\(root_id, &relative_path\)\)}{    state.with_index(|db| db.remove_path_exclusion(root_id, \&relative_path).map(|_| true))}' \
  'state.with_index(|db| db.remove_path_exclusion(root_id, &relative_path).map(|_| true))' \
  mnema-desktop 'including_a_subfolder_removes_the_rule_and_reports_whether_a_row_went' --test commands

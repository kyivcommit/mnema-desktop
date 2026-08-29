# The three commands that read and write a folder exclusion. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr8-exclusions.sh
#
# Seven cases, each against one of `list_exclusions`, `exclude_subfolder` or
# `include_subfolder` (`src-tauri/src/bridge.rs`) and the `tests/commands.rs`
# tests written for task-2 of PR 8a. Two of the seven (the root-unavailable
# refusal and the byte-exact directory resolution) were added in fix round 1,
# against Important findings 1 and 2 of the task review.
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
# what the byte-exact resolution below would have found.
case_ "existsOnDisk must come from a real filesystem lookup, not a constant" \
  src-tauri/src/bridge.rs \
  's~    let mut current = root\.to_path_buf\(\);\n    for component in prefix\.split\('"'"'/'"'"'\) \{\n        let found = std::fs::read_dir\(&current\)\.ok\(\)\.and_then\(\|entries\| \{\n            entries\n                \.flatten\(\)\n                \.find\(\|entry\| entry\.file_name\(\) == std::ffi::OsStr::new\(component\)\)\n        \}\);\n        match found \{\n            Some\(entry\) => current = entry\.path\(\),\n            None => return false,\n        \}\n    \}\n    std::fs::symlink_metadata\(&current\)\.is_ok\(\)~    /* mutant: always true */\n    true~' \
  '/* mutant: always true */' \
  mnema-desktop 'list_exclusions_reports_whether_each_stored_prefix_is_still_on_disk' --test commands

# Review round 1, Important 2. `prefix_exists_on_disk` resolves each path
# component against real `read_dir` entries with byte equality specifically
# because the filesystem's own lookup (what a bare `symlink_metadata` on the
# joined path uses) is case-INSENSITIVE on APFS and Windows, while `ignore`'s
# override matcher — what the walk itself applies — is case-sensitive. This
# mutant reverts to that naive, case-insensitive check: a stored prefix
# `private` against a real folder `Private` would then report
# `existsOnDisk: true` while the rule excludes nothing.
case_ "existsOnDisk must resolve each component with byte-exact equality, not the filesystem's own case-insensitive lookup" \
  src-tauri/src/bridge.rs \
  's~    let mut current = root\.to_path_buf\(\);\n    for component in prefix\.split\('"'"'/'"'"'\) \{\n        let found = std::fs::read_dir\(&current\)\.ok\(\)\.and_then\(\|entries\| \{\n            entries\n                \.flatten\(\)\n                \.find\(\|entry\| entry\.file_name\(\) == std::ffi::OsStr::new\(component\)\)\n        \}\);\n        match found \{\n            Some\(entry\) => current = entry\.path\(\),\n            None => return false,\n        \}\n    \}\n    std::fs::symlink_metadata\(&current\)\.is_ok\(\)~    /* mutant: naive case-insensitive stat */\n    std::fs::symlink_metadata(root.join(prefix)).is_ok()~' \
  '/* mutant: naive case-insensitive stat */' \
  mnema-desktop 'a_prefix_that_only_matches_the_folders_name_by_case_reports_not_on_disk' --test commands

# Review round 1, Important 1. The root-unavailable guard, disarmed. Without
# it, an unmounted drive or a moved folder makes every stored prefix answer
# `existsOnDisk: false` instead of refusing the whole call — the same field
# lying that a per-prefix `.unwrap_or(false)` produced before this fix round,
# now for the entire list at once.
case_ "an unreachable watched root must refuse the whole list_exclusions call" \
  src-tauri/src/bridge.rs \
  's{    if std::fs::symlink_metadata\(root_path\)\.is_err\(\) \{}{    if false \{}' \
  'if false {' \
  mnema-desktop 'list_exclusions_refuses_when_the_root_itself_is_unreachable' --test commands

# `include_subfolder`'s answer, forced to `true` regardless of what
# `Db::remove_path_exclusion` actually found. Task 5's stale-rule control
# needs "removed" told apart from "there was nothing there"; a command that
# always says "removed" cannot tell them apart.
case_ "include_subfolder must report false when there was nothing to remove" \
  src-tauri/src/bridge.rs \
  's{    state\.with_index\(\|db\| db\.remove_path_exclusion\(root_id, &relative_path\)\)}{    state.with_index(|db| db.remove_path_exclusion(root_id, \&relative_path).map(|_| true))}' \
  'state.with_index(|db| db.remove_path_exclusion(root_id, &relative_path).map(|_| true))' \
  mnema-desktop 'including_a_subfolder_removes_the_rule_and_reports_whether_a_row_went' --test commands

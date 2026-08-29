# The three commands that read and write a folder exclusion. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr8-exclusions.sh
#
# Five cases, each against one of `list_exclusions`, `exclude_subfolder` or
# `include_subfolder` (`src-tauri/src/bridge.rs`) and the `tests/commands.rs`
# tests written for task-2 of PR 8a.
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

# `exists_on_disk` collapsed to a constant. Kills on either half of the
# fixture: a prefix naming a FILE (`solo.txt`) and a prefix whose folder was
# renamed away both must read false, and a mutant that hands back `true`
# unconditionally — skipping the `symlink_metadata` call and its `unwrap_or`
# fallback both — gets both wrong.
case_ "existsOnDisk must come from a real symlink_metadata call, not a constant" \
  src-tauri/src/bridge.rs \
  's{            let exists_on_disk = std::fs::symlink_metadata\(&full\)\n                \.map\(\|meta\| meta\.is_dir\(\)\)\n                \.unwrap_or\(false\);}{            let exists_on_disk = true;}' \
  'let exists_on_disk = true;' \
  mnema-desktop 'list_exclusions_reports_whether_each_stored_prefix_still_names_a_directory' --test commands

# `include_subfolder`'s answer, forced to `true` regardless of what
# `Db::remove_path_exclusion` actually found. Task 5's stale-rule control
# needs "removed" told apart from "there was nothing there"; a command that
# always says "removed" cannot tell them apart.
case_ "include_subfolder must report false when there was nothing to remove" \
  src-tauri/src/bridge.rs \
  's{    state\.with_index\(\|db\| db\.remove_path_exclusion\(root_id, &relative_path\)\)}{    state.with_index(|db| db.remove_path_exclusion(root_id, \&relative_path).map(|_| true))}' \
  'state.with_index(|db| db.remove_path_exclusion(root_id, &relative_path).map(|_| true))' \
  mnema-desktop 'including_a_subfolder_removes_the_rule_and_reports_whether_a_row_went' --test commands

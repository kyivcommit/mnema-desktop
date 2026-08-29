# macOS-only sibling of `pr8-exclusions.sh`. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr8-exclusions-macos.sh
#
# One case, and it lives here rather than in `pr8-exclusions.sh` because the
# test it names — `a_prefix_that_only_matches_the_folders_name_by_case_
# reports_not_on_disk` (`src-tauri/tests/commands.rs`) — is itself
# `#[cfg(target_os = "macos")]`. The property under test (a case-insensitive
# filesystem answering a case-sensitive rule's own question) genuinely does
# not exist on Linux: on a case-sensitive filesystem the naive mutant this
# case reverts to (`symlink_metadata` on the joined path, ignoring case) is
# ALREADY wrong for a mismatched-case path, so the mutation would report
# STILL GREEN there rather than exercising anything — not a gap in coverage,
# a fact about the property. `mutation-check.sh`'s baseline pass runs each
# named test once with `--exact` and requires `test result: ok. 1 passed`;
# off macOS this test does not exist at all (`#[cfg]`, not `#[ignore]`), so
# `--exact` selects zero tests, the baseline reads `0 passed`, and the whole
# FILE would exit 1 before any mutation ran — for every case in it, not only
# this one (review round 2, Important A). Splitting the file is what keeps
# that failure from taking every other case in `pr8-exclusions.sh` down with
# it on a non-macOS CI leg.
#
# No count of those cases here, deliberately (review round 4). The number
# that stood in this sentence — "seven portable cases" — was written against
# an earlier state of that file and was wrong by one before round 4 added a
# case: that file held ten cases at the time, four of them naming
# `#[cfg(unix)]` tests, so the portable count was six. It has grown since —
# how often is itself not worth writing down here, which is the point: a
# count maintained in a DIFFERENT file from the thing it counts drifts
# silently, and this project's own rule is that a
# number is a definition. The count that has to be right lives in
# `pr8-exclusions.sh`'s own header, where the cases are.

case_ "existsOnDisk must resolve each component with byte-exact equality, not the filesystem's own case-insensitive lookup" \
  src-tauri/src/bridge.rs \
  's~    let mut current = root\.to_path_buf\(\);\n    for component in prefix\.split\('"'"'/'"'"'\) \{\n        match std::fs::read_dir\(&current\)\.and_then\(\|entries\| entry_named\(entries, component\)\) \{\n            Ok\(Some\(path\)\) => current = path,\n            Ok\(None\) => return false,\n            Err\(e\) if path_error_is_an_answer\(e\.kind\(\)\) => return false,\n            Err\(_\) => return true,\n        \}\n    \}\n    match std::fs::symlink_metadata\(&current\) \{\n        Ok\(_\) => true,\n        Err\(e\) if path_error_is_an_answer\(e\.kind\(\)\) => false,\n        Err\(_\) => true,\n    \}~    /* mutant: naive case-insensitive stat */\n    std::fs::symlink_metadata(root.join(prefix)).is_ok()~' \
  '/* mutant: naive case-insensitive stat */' \
  mnema-desktop 'a_prefix_that_only_matches_the_folders_name_by_case_reports_not_on_disk' --test commands

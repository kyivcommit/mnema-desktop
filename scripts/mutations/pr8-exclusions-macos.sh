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
# that failure from taking `pr8-exclusions.sh`'s other seven portable cases
# down with it on a non-macOS CI leg.

case_ "existsOnDisk must resolve each component with byte-exact equality, not the filesystem's own case-insensitive lookup" \
  src-tauri/src/bridge.rs \
  's~    let mut current = root\.to_path_buf\(\);\n    for component in prefix\.split\('"'"'/'"'"'\) \{\n        let entries = match std::fs::read_dir\(&current\) \{\n            Ok\(entries\) => entries,\n            Err\(e\) if e\.kind\(\) == std::io::ErrorKind::NotFound => return false,\n            Err\(_\) => return true,\n        \};\n        match entries\n            \.flatten\(\)\n            \.find\(\|entry\| entry\.file_name\(\) == std::ffi::OsStr::new\(component\)\)\n        \{\n            Some\(entry\) => current = entry\.path\(\),\n            None => return false,\n        \}\n    \}\n    std::fs::symlink_metadata\(&current\)\.is_ok\(\)~    /* mutant: naive case-insensitive stat */\n    std::fs::symlink_metadata(root.join(prefix)).is_ok()~' \
  '/* mutant: naive case-insensitive stat */' \
  mnema-desktop 'a_prefix_that_only_matches_the_folders_name_by_case_reports_not_on_disk' --test commands

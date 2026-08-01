# Mutation cases for the walk-level pieces the randomised harness (D47) now
# drives through `walk_root` rather than modelling by hand. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-13.sh
#
# Three cases, not the four the task brief named. The fourth — the line
# `walked.complete = false;` inside `enumerate`'s `entry.depth() == 0` arm
# (`crates/mnema-walk/src/lib.rs`) — is deliberately left out, and Task 1's
# own comment already says why a test was never written for it: the arm only
# runs when `root` stops being a directory in the narrow window between
# `enumerate`'s own `!root.is_dir()` check and the walker reaching the
# depth-0 entry, both of which are fresh, independent stats of the same
# path. Absent an actual race, they cannot disagree — this file's own harness
# is single-threaded and never mutates the filesystem while a call into
# `enumerate` is in flight, so it cannot reach this arm either, no matter how
# many seeds it draws. Worse than merely hard to reach: `Walked`'s public
# fields carry no record of WHICH of the five `complete = false` sites fired,
# so even a test that won a real race could not tell this arm's removal apart
# from any of the other four. Closing it soundly needs a test seam in
# `enumerate` itself, which is production code no test-only task should be
# the one to add — that is a call for whoever owns `mnema-walk`, not this one.

case_ "enumerate: the walker's per-directory sort is not the whole ordering" \
  crates/mnema-walk/src/lib.rs \
  's{    walked\.found\.sort_by\(\|a, b\| a\.relative\.cmp\(&b\.relative\)\);}{    // walked.found.sort_by(|a, b| a.relative.cmp(&b.relative));}' \
  '// walked.found.sort_by' \
  mnema-walk 'the_walk_is_ordered_by_relative_path' --test enumerate

# `require_git` defaults to TRUE in the `ignore` crate, and left alone it
# makes `.gitignore` apply only inside an actual git repository — silently,
# since nothing here counts a rule that quietly stopped matching. A watched
# folder is normally not a repository.
case_ "rules: gitignore must apply outside a git repository" \
  crates/mnema-walk/src/rules.rs \
  's{\.require_git\(false\)}{// .require_git(false) removed}' \
  '// .require_git(false) removed' \
  mnema-walk 'gitignore_applies_in_a_folder_that_is_not_a_repository' --test rules

# `record_skip`'s `ON CONFLICT` target has to name the schema's unique index
# on `skipped` by the same expression the index itself uses
# (`crates/mnema-index/src/schema.sql`'s own comment has the two failure
# modes this couples against — this is the loud one, an immediate SQLite
# error on the very first skip, not the silent double-counted row the other
# side of the same drift produces). Touch one side without the other and
# every skip in the index breaks at once.
case_ "journal: the ON CONFLICT target matches the schema's own index" \
  crates/mnema-index/src/journal.rs \
  's{ON CONFLICT \(COALESCE\(watched_root_id, -1\), relative_path, COALESCE\(page_no, -1\)\)}{ON CONFLICT (COALESCE(watched_root_id, -1), relative_path, page_no)}' \
  'ON CONFLICT (COALESCE(watched_root_id, -1), relative_path, page_no)' \
  mnema-index 'a_skipped_file_names_the_rule_that_fired' --test journal

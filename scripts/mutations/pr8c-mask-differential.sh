# §15.4 A — the preview-versus-walk differential harness. Four mutants of the
# product — three of the preview, one of the walk — and one of the harness
# itself; the harness is the only test named.
# The harness runs 12 seeds, so every case below must be one the DEFAULT run
# reaches — proven by hand when the harness was written, and re-proven here.

# The PR #27 review blocker: a preview that does not subtract what the stored
# rules already take. NOT "the candidate alone in `with_candidate`" — that
# mutant is equivalent (the preview skips every path `current` removes, so
# for the survivors the two predicates agree; Task 3 measured it green).
case_ "the harness must catch a preview that forgets what the stored masks already take (PR #27 review blocker)" \
  src-tauri/src/tree.rs \
  's~\.with_masks\(stored\.clone\(\)\)\?~.with_masks(vec![])? /* mutant: current without stored masks */~' \
  '/* mutant: current without stored masks */' \
  mnema-desktop 'the_preview_and_the_walk_agree_on_every_seed' --test mask_differential

case_ "the harness must catch a path already taken by a stored rule being charged to this press" \
  src-tauri/src/tree.rs \
  's~            if current\.removes_file\(&file\.relative_path\) \{\n                continue;~            if current.removes_file(\&file.relative_path) { /* mutant: charged anyway */\n                let _ = ();~' \
  '/* mutant: charged anyway */' \
  mnema-desktop 'the_preview_and_the_walk_agree_on_every_seed' --test mask_differential

case_ "the harness must catch documents counted by any taken path rather than by every surviving path" \
  src-tauri/src/tree.rs \
  's~\.filter\(\|\(surviving, taken\)\| surviving == taken\)~.filter(|(_surviving, taken)| *taken > 0) /* mutant: any taken path */~' \
  '/* mutant: any taken path */' \
  mnema-desktop 'the_preview_and_the_walk_agree_on_every_seed' --test mask_differential

# Kills on the harness's own setup assert, not the differential ones: with S
# never written for world A, `stored_rules(&w0)` (world 0, unmutated) no
# longer matches `a_rules` (what world A was meant to hold), and
# `assert_eq!(persisted, a_rules, "world A holds different rules")` at line
# 938 fires before the preview/walk comparison is ever reached. Still a red
# this mutant earns, and the one that proves the harness checks its own
# fixture rather than only the product under test. Measured follow-up: with
# that assert temporarily removed, the invariant itself killed the mutant
# ("the S+m walk removed 4 paths the S walk did not, the preview promised
# 1" — task-4-report.md, the Step 5 mutant).
case_ "the harness must go red on its own when world A forgets to store the rules" \
  src-tauri/tests/mask_differential.rs \
  's~    store_rules\(&a, &world\); // world A stores S~    /* mutant: world A never stores S */~' \
  '/* mutant: world A never stores S */' \
  mnema-desktop 'the_preview_and_the_walk_agree_on_every_seed' --test mask_differential

# The other side of the differential: the WALK stops applying masks to files
# while the preview's `removes_file` is untouched. Without this case every
# proof attacks the preview, and a walk-side gap would be undemonstrated.
#
# The `(|t| ...)` closure has to escape both the parens AND the pipes as
# literal characters (`\(`, `\|`) — an unescaped `|` inside this runner's
# perl-regex patterns is alternation, not the closure syntax it looks like
# (see the correctly-escaped `.filter(|(surviving, taken)| ...)` case above,
# and `pr8-masks.sh:128`, which is the same shape).
case_ "the harness must catch the walk no longer applying masks to files" \
  crates/mnema-walk/src/rules.rs \
  's~if entry\.file_type\(\)\.is_some_and\(\|t\| t\.is_file\(\)\) && masks\.matches\(name\) \{~if false \&\& masks.matches(name) { /* mutant: walk ignores masks */~' \
  '/* mutant: walk ignores masks */' \
  mnema-desktop 'the_preview_and_the_walk_agree_on_every_seed' --test mask_differential

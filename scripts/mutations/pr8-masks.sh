# The file masks: where they are stored, the commands that read and write one,
# the preview that says what one would take, and the walk that makes it happen.
# Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr8-masks.sh
#
# What is here, by the file each case mutates. `crates/mnema-index/src/
# migrations.rs` and `.../src/write.rs` are the storage — migration 4 and the
# three `Db` methods; `src-tauri/src/bridge.rs` is the three commands;
# `src-tauri/src/tree.rs` is `mask_preview`; `src-tauri/src/walk_job.rs` is
# where a stored mask stops being a row and starts removing files; and
# `crates/mnema-walk/src/rules.rs` holds the one guard that keeps a mask from
# pruning a directory that merely shares its name.
#
# ⚠️ **No count of the cases here, deliberately** — the sibling
# `pr8-exclusions.sh` has paid for a count twice. Re-derive them:
#
#   grep -c '^case_ ' scripts/mutations/pr8-masks.sh              # cases
#   grep -A1 '^case_ ' scripts/mutations/pr8-masks.sh \
#     | grep -E '^\s+(src-tauri|crates)/' | sort | uniq -c        # by file mutated
#
# Every case here runs on any platform: no test it names is behind a `#[cfg]`,
# so there is no `-macos` or `-linux` sibling to this file. That is a fact to
# check rather than inherit if a case is added —
#
#   for t in $(grep -oE "mnema-[a-z]+ '[^']+'" scripts/mutations/pr8-masks.sh \
#               | sed "s/.*'\(.*\)'/\1/" | sort -u); do
#     grep -rn -B3 "fn ${t}(" src-tauri crates | grep -q 'cfg(' && echo "$t"
#   done
#
# — because a case naming a test that does not exist on a platform makes the
# harness's baseline read `0 passed` and exit 1 for the WHOLE file before any
# mutation runs (`pr8-exclusions.sh`, review round 2, Important A).
#
# ⚠️ **"Narrow the probe from the whole mask set to the candidate" is NOT a
# case here**, and unlike the exclusion side the reason is not that the code
# already does one thing. `WalkRules::with_masks` really does take a set — but
# every mask compiles into its own `GlobMatcher` (`MaskLayer::globs`), so there
# is no aggregate compile step that a set can fail. A probe over the stored set
# plus the candidate would refuse exactly the same masks the candidate probe
# refuses, and the mutant would be born equivalent. This is also why the mask
# layer feeds nothing into `Walked::rules_applied`.

# ── Storage ───────────────────────────────────────────────────────────────────

# 🔴 D-b, and the reason this whole task has a shape. `file_mask` appended to
# migration 3 instead of taking migration 4: `to_latest` runs nothing on a
# database already at `user_version = 3` — which every index shipped by PR 8a
# is — so the table would exist on machines installed after the commit and on
# none installed before it, with every fresh-database test still green.
#
# The mutant is a real append (the two strings concatenated into one `M::up`),
# not a comment change, so it produces exactly the state the defect would. It
# is caught by the ABSENT-at-version-3 half of the test: the present-after-
# apply half passes under this mutant, because `to_version(.., 3)` runs this
# build's migration 3 rather than the one that shipped.
case_ "file_mask must be its own migration, never appended to migration 3" \
  crates/mnema-index/src/migrations.rs \
  's~        M::up\(ADD_IGNORE_RULE_UNIQUE\),\n        M::up\(ADD_FILE_MASK\),~        /* mutant: file_mask appended to migration 3 */\n        M::up(Box::leak(format!("{ADD_IGNORE_RULE_UNIQUE}{ADD_FILE_MASK}").into_boxed_str())),~' \
  '/* mutant: file_mask appended to migration 3 */' \
  mnema-index 'migrations::tests::file_mask_reaches_a_database_that_is_already_at_version_three' --lib

# `Db::add_mask`'s `ON CONFLICT DO NOTHING` is what makes adding the same mask
# twice one row instead of an error. Removed, the second INSERT hits the
# `pattern` PRIMARY KEY head-on and the ordinary case of a person pressing
# "add" again on a mask they already have becomes a rejection.
case_ "adding a mask that is already stored must not become an error" \
  crates/mnema-index/src/write.rs \
  's{INSERT INTO file_mask \(pattern\) VALUES \(\?1\)\n             ON CONFLICT DO NOTHING",}{INSERT INTO file_mask (pattern) VALUES (?1)",}' \
  'INSERT INTO file_mask (pattern) VALUES (?1)",' \
  mnema-index 'adding_the_same_mask_twice_writes_one_row' --test tree

# `Db::remove_mask`'s predicate, gone — every mask deleted whichever one was
# named. The parameter is KEPT (`WHERE ?1 IS NOT NULL`) deliberately: a bare
# `DELETE FROM file_mask` also drops the placeholder, and rusqlite then fails
# with `InvalidParameterCount` in every test that removes anything — a red that
# is about arity and not about the missing predicate. Measured: that form
# killed two tests for the wrong reason, this one kills the right test for the
# right one.
case_ "removing one mask must not remove the others" \
  crates/mnema-index/src/write.rs \
  's{"DELETE FROM file_mask WHERE pattern = \?1",}{"DELETE FROM file_mask WHERE ?1 IS NOT NULL",}' \
  'DELETE FROM file_mask WHERE ?1 IS NOT NULL' \
  mnema-index 'removing_one_mask_leaves_the_others_standing' --test tree

# ── The commands ──────────────────────────────────────────────────────────────

# The candidate probe, deleted. `let _ = ...` still compiles and the `?` that
# propagated `RulesError` is gone, so `logs/*.tmp` — which compiles fine and
# then matches nothing, because `MaskLayer` asks about a NAME — is stored and
# sits in the editor looking like a rule for the life of the index.
case_ "a mask WalkRules refuses must still be refused before it is stored" \
  src-tauri/src/bridge.rs \
  's{    mnema_walk::WalkRules::none\(\)\.with_masks\(vec!\[pattern\.clone\(\)\]\)\?;}{    let _ = mnema_walk::WalkRules::none().with_masks(vec![pattern.clone()]);}' \
  'let _ = mnema_walk::WalkRules::none().with_masks(vec![pattern.clone()]);' \
  mnema-desktop 'adding_a_malformed_mask_is_refused_and_stores_nothing' --test commands

# The blank guard, disarmed. `validate_mask` answers `Ok(None)` for the empty
# string — not an error — so with the guard gone `add_mask` writes a row that
# removes nothing and reads, in the editor, as protection.
case_ "the empty string must still be refused before it reaches Db::add_mask" \
  src-tauri/src/bridge.rs \
  's{    if pattern\.is_empty\(\) \{}{    if false \{ /* mutant: blank mask guard disarmed */}' \
  'if false { /* mutant: blank mask guard disarmed */' \
  mnema-desktop 'a_blank_and_a_whitespace_only_mask_are_refused_with_their_own_sentences' --test commands

# 🔴 The same guard, present but trimming — the mutant the case above cannot
# catch, and the one an implementer actually reaches for. `"   "` is a
# `RulesError::MaskSurroundingWhitespace`, with a sentence that tells the person
# what to change; trimming first hands them "a file mask cannot be empty"
# instead. Two sentences collapsed into the less useful one, and nothing
# refused that should have been stored, so nothing else in the suite moves.
case_ "the blank check must not trim: whitespace is the validator's refusal, not the blank one" \
  src-tauri/src/bridge.rs \
  's{    if pattern\.is_empty\(\) \{}{    if pattern.trim().is_empty() \{ /* mutant: blank check trims */}' \
  'if pattern.trim().is_empty() { /* mutant: blank check trims */' \
  mnema-desktop 'a_blank_and_a_whitespace_only_mask_are_refused_with_their_own_sentences' --test commands

# ── The preview ───────────────────────────────────────────────────────────────

# 🔴 D-d. Paths counted as documents — the grouping deleted outright. This
# OVERSTATES the loss: a document with two indexed copies, one of which the
# mask takes, is reported as a document that stops being findable when
# `forget_if_unnamed` will not touch it. An overstated disclosure is worse than
# none, because it is a claim a person acts on.
case_ "mask_preview must count documents by grouping, not by counting paths" \
  src-tauri/src/tree.rs \
  's~            let documents = per_document\n                \.values\(\)\n                \.filter\(\|\(seen, matched\)\| seen == matched && \*matched > 0\)\n                \.count\(\) as i64;~            /* mutant: paths counted as documents */\n            let documents = paths;~' \
  '/* mutant: paths counted as documents */' \
  mnema-desktop 'mask_preview_counts_paths_and_documents_apart' --test commands

# 🔴 Review Important 4.1, and the reason `mask_preview` lives on this side of
# the wire at all. Counted over a `read_dir` instead of over indexed paths.
# `MaskLayer::matches` asks about a NAME and nothing about the entry, while the
# walk asks `is_file() && matches(name)` — so over a disk listing the predicate
# says "removed" for a symlink, a FIFO, an entry whose `file_type()` cannot be
# read, and for every file the index does not hold at all. The skew runs one
# way only: the preview shows a person MORE files than the next walk will take.
case_ "mask_preview must count indexed paths, never a disk listing" \
  src-tauri/src/tree.rs \
  's~                for file in db\.indexed_files_under_root\(root\.id\)\? \{\n                    let entry = per_document\.entry\(file\.document_id\)\.or_insert\(\(0, 0\)\);\n                    entry\.0 \+= 1;\n                    if masks\.matches\(&file\.relative_path\) \{\n                        entry\.1 \+= 1;\n                        paths \+= 1;\n                    \}\n                \}~                /* mutant: counted over a disk listing */\n                for found in std::fs::read_dir(\&root.absolute_path).into_iter().flatten().flatten() {\n                    let name = found.file_name().to_string_lossy().into_owned();\n                    let entry = per_document.entry(name.clone()).or_insert((0, 0));\n                    entry.0 += 1;\n                    if masks.matches(\&name) {\n                        entry.1 += 1;\n                        paths += 1;\n                    }\n                }~' \
  '/* mutant: counted over a disk listing */' \
  mnema-desktop 'mask_preview_counts_the_index_and_not_the_disk' --test commands

# A mask is global, so the preview is too. Narrowed to the first watched root,
# it under-reports what a mask will take from every other one — the direction
# that lets a person save a rule believing it touches one folder.
case_ "mask_preview must count across every watched root, not the first" \
  src-tauri/src/tree.rs \
  's{            for root in db\.list_watched_roots\(\)\? \{}{            for root in db.list_watched_roots()?.into_iter().take(1) \{ /* mutant: first root only */}' \
  'for root in db.list_watched_roots()?.into_iter().take(1) { /* mutant: first root only */' \
  mnema-desktop 'mask_preview_counts_across_every_watched_root' --test commands

# The preview's own validation, removed. A malformed mask would then preview as
# `paths: 0, documents: 0` — which reads as "this rule would remove nothing",
# the opposite of true for a rule that cannot be stored at all, and the person
# finds out only when the save they were encouraged to make is refused.
case_ "mask_preview must validate the pattern rather than preview it as zero" \
  src-tauri/src/tree.rs \
  's{    let rules = WalkRules::none\(\)\.with_masks\(vec!\[pattern\]\)\?;}{    let rules = WalkRules::none().with_masks(vec![pattern]).unwrap_or_default(); /* mutant: preview does not validate */}' \
  '/* mutant: preview does not validate */' \
  mnema-desktop 'mask_preview_refuses_a_malformed_pattern_rather_than_answering_zero' --test commands

# ── The walk ──────────────────────────────────────────────────────────────────

# The mask read in `walk_job`, replaced by an empty set. The walk then runs
# with the exclusions it was given and none of the masks: every file a person
# masked stays indexed, the walk reports `completed`, and under D29 the text
# they asked to hold back keeps going to a third-party provider.
case_ "the walk must read the stored masks, not walk without them" \
  src-tauri/src/walk_job.rs \
  's{            db\.list_masks\(\)\?,}{            Vec::new(),}' \
  '            Vec::new(),' \
  mnema-desktop 'a_walk_applies_a_stored_mask_and_keeps_the_folder_that_shares_its_name' --test commands

# 🔴 D-c's directory ruling, end to end. The `is_file()` half of the mask
# predicate removed, so the mask prunes a DIRECTORY whose name matches it and
# takes everything inside with it — measured in the plan: with `*.pdf` as an
# override, a tree holding `archive.pdf/keep.txt` came back as `["notes.txt"]`
# alone. Task 9 closed this at the enumeration level; this case is what says the
# index agrees, through a real walk. Under the mutant the walk reports
# `removed: 2` where one file was masked.
case_ "a mask must never prune a directory that shares its name" \
  crates/mnema-walk/src/rules.rs \
  's{            if entry\.file_type\(\)\.is_some_and\(\|t\| t\.is_file\(\)\) && masks\.matches\(name\) \{}{            if masks.matches(name) \{ /* mutant: the mask prunes directories too */}' \
  'if masks.matches(name) { /* mutant: the mask prunes directories too */' \
  mnema-desktop 'a_walk_applies_a_stored_mask_and_keeps_the_folder_that_shares_its_name' --test commands

# The walk's refusal on a stored mask the validator no longer accepts,
# swallowed — the mask half of `pr8-exclusions.sh`'s "must refuse the job, not
# be walked around". `Db::add_mask` deliberately does not validate, so the state
# is reachable: a mask written straight through the `Db` method, or by an older
# build whose `validate_mask` was narrower. Under the mutant the walk runs with
# EVERY mask silently absent — not just the bad one — and reports `completed`,
# which under D29 is every file the person masked going to a provider.
case_ "a stored mask that no longer validates must refuse the walk, not be walked around" \
  src-tauri/src/walk_job.rs \
  's{    let rules = WalkRules::new\(true, true, user_prefixes\)\?\.with_masks\(masks\)\?;}{    let rules = \{ let r = WalkRules::new(true, true, user_prefixes)?; r.clone().with_masks(masks).unwrap_or(r) \};}' \
  'r.clone().with_masks(masks).unwrap_or(r)' \
  mnema-desktop 'a_stored_mask_that_no_longer_validates_refuses_the_walk' --test commands

# `remove_mask`'s answer, replaced by a constant `true`. The window renders
# "removed" and "there was nothing there" as different sentences — a second
# window having removed the same mask first is the ordinary way to reach the
# second one — and a pass-through that always says `true` satisfies every
# one-directional assertion about a removal that did happen.
case_ "remove_mask must report whether a row actually went, not always true" \
  src-tauri/src/bridge.rs \
  's{    state\.with_index\(\|db\| db\.remove_mask\(&pattern\)\)\n\}}{    state.with_index(|db| db.remove_mask(&pattern))?;\n    Ok(true) /* mutant: removal always reports a row */\n\}}' \
  'Ok(true) /* mutant: removal always reports a row */' \
  mnema-desktop 'the_mask_commands_store_list_and_remove_through_the_ipc' --test commands

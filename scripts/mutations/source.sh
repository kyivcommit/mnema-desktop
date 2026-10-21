# Mutation cases for the file pane's reads (PR 5, re-aimed at `open_document`).
# Run with:
#
#   scripts/mutation-check.sh scripts/mutations/source.sh
#
# This file began as `source_around`'s — the paragraphs-around-a-citation
# command — and that command is gone: the pane shows the whole file and paints
# the cited passage inside it (spec §3). `Db::reading_window`, which only that
# command read, went with it, and so did its cases. What survives is every case
# for the reads `open_document` still makes: `path_occupant`, `chunk_anchor`,
# `roots_holding_path` and `cited_occupant`. The IPC-level cases were re-aimed
# at the tests that now exercise those reads through `open_document`; the ones
# with no surviving fixture were deleted rather than retargeted.
#
# ⚠️ Every pattern below was counted against the tree it was written on and
# hits EXACTLY once, unless it carries `/g` — checked with `grep -c` before
# writing each case, not copied out of the plan's prose. `case_` refuses a
# substitution with more than one hit (or, for a `/g` case, fewer than one)
# and reports it as a BROKEN CASE.
#
# ── Not covered here, and why ─────────────────────────────────────────────
#
# Deleting the `WireSegment` conversion and sending `Segment` straight has no
# case: it needs two coordinated edits (`Vec<WireSegment>` and the `.map`), and
# `case_` takes one substitution. The value-permutation case below covers the
# same seam.
#
# Ordering clauses a primary key already walks in order (`indexed_files_under_root`'s
# `ORDER BY p.relative_path`, `tree.sh`'s own precedent) would be the same
# likely-equivalent shape here too; the one query in this file with a real
# order clause is `roots_holding_path`, cased below.

# ═══════════════════════════════════════════════════════════════════════════
# path_occupant: the root predicate

case_ "path_occupant: the root predicate is widened to <=" \
  crates/mnema-index/src/write.rs \
  's~WHERE p\.watched_root_id = \?1 AND p\.relative_path = \?2~WHERE p.watched_root_id <= ?1 AND p.relative_path = ?2~' \
  'WHERE p.watched_root_id <= ?1 AND p.relative_path = ?2' \
  mnema-index 'path_occupant_reports_the_row_as_it_stands' --test source

case_ "chunk_anchor: ord is hardcoded to 0 instead of read from the row" \
  crates/mnema-index/src/write.rs \
  's~ord: row\.get\(1\)\?,~ord: { let _: i64 = row.get(1)?; 0 },~' \
  'ord: { let _: i64 = row.get(1)?; 0 },' \
  mnema-index 'chunk_anchor_reports_the_identity_and_the_blocks_a_chunk_covers' --test source

# ═══════════════════════════════════════════════════════════════════════════
# roots_holding_path: the ambiguity-preserving scan collapses to one row

case_ "roots_holding_path: LIMIT 1 collapses the ambiguity" \
  crates/mnema-index/src/write.rs \
  's~SELECT watched_root_id FROM path WHERE relative_path = \?1 ORDER BY watched_root_id~SELECT watched_root_id FROM path WHERE relative_path = ?1 ORDER BY watched_root_id LIMIT 1~' \
  'ORDER BY watched_root_id LIMIT 1' \
  mnema-index 'roots_holding_path_returns_every_root' --test source

# ═══════════════════════════════════════════════════════════════════════════
# tree.rs: cited_occupant refuses an ambiguous location
#
# There used to be two branches here — narrow by document, else fall back to
# the location — and two cases for them. Owner review on PR #22 showed what
# the narrowing costs: two roots holding the SAME document at one path answer
# `noPath`, and then editing one copy leaves a single survivor, so the same
# citation flips to `current` — confident exactly when the cited copy may be
# the stale one. The two situations are shape-identical from the index, so the
# narrowing is gone and the blunt rule stands: more than one row at the cited
# path and we cannot say which copy was meant.
#
# `first()` is the mutation that matters now: it is the plausible "just pick
# one" a later reader would write, and it is precisely the unearned
# confidence the review removed.
#
# 🔴 Task 2 moved this line into the `None` arm of a `match cited_root_id`
# (`citedRootId` names one root directly and skips the ambiguity scan
# entirely), at a deeper indent — the pattern below is re-aimed at its new
# 12-space form, or it reports zero hits and a BROKEN CASE. Re-aimed
# 2026-08-26; the test it targets is unaffected: it never sends `citedRootId`,
# so it still exercises this exact fallback arm.

case_ "cited_occupant: an ambiguous location picks a root instead of refusing" \
  src-tauri/src/tree.rs \
  's~            let \[root\] = roots\.as_slice\(\) else \{~            let Some(root) = roots.first() else {~' \
  'let Some(root) = roots.first() else {' \
  mnema-desktop 'open_document_reports_no_path_when_two_roots_share_the_path_even_if_the_document_differs' --test commands

# The `Some(root)` arm — a citation that DOES name a root — had no case at
# all: every case above targets the `None` fallback. Collapsing it to `Ok(None)`
# throws away the whole point of naming a root (skip the ambiguity scan and
# resolve directly), so a citation that names root A among two roots sharing
# a path gets `noPath` instead of `current`. Found by a fix-round review.

case_ "cited_occupant: naming a root is ignored and answers no verdict" \
  src-tauri/src/tree.rs \
  's~Some\(root\) => db\.path_occupant\(root, relative_path\),~Some(_) => Ok(None),~' \
  'Some(_) => Ok(None),' \
  mnema-desktop 'open_document_uses_the_cited_root_to_resolve_the_occupant_when_two_roots_hold_the_path' --test commands

# ═══════════════════════════════════════════════════════════════════════════
# tree.rs: a block's missing section title ships as null, not an absent key

case_ "ViewBlock::section_title skips serialisation when None" \
  src-tauri/src/tree.rs \
  's~    pub section_title: Option<String>,~    #[serde(skip_serializing_if = "Option::is_none")]\n    pub section_title: Option<String>,~' \
  'skip_serializing_if' \
  mnema-desktop 'tree::tests::a_block_with_no_section_title_still_carries_the_key' --lib

# ═══════════════════════════════════════════════════════════════════════════
# tree.rs: the WireSegment conversion, permuted

case_ "WireSegment::from swaps block_start for start" \
  src-tauri/src/tree.rs \
  's~block_start: s\.block_start~block_start: s.start~' \
  'block_start: s.start' \
  mnema-desktop 'tree::tests::a_real_segment_crosses_the_wire_through_its_camel_case_mirror' --lib

# ═══════════════════════════════════════════════════════════════════════════
# tree.rs: the index reads leave their one read_snapshot
#
# 🔴 DELIBERATELY NOT A CASE, and it used to be one with a measured oracle.
# Read this before adding it back.
#
# The mutation is:
#
#   src-tauri/src/tree.rs
#   's~db\.read_snapshot\(\|db\| \{~Ok::<_, mnema_index::Error>(db).and_then(|db| {~'
#
# It runs the same closure with no transaction around it, so the block read and
# the `path` read inside `build_document_view` see whatever is committed at
# their own instant. The oracle was `source_around`'s race test: a second
# connection rebuilt the document between the pin and the window, and the test
# caught the tear in nine runs out of ten. Both the command and that test were
# deleted with `Source.svelte`, and nothing builds a document-rebuild race
# against `open_document` today — so the case would report STILL GREEN for a
# reason that has nothing to do with the mutation.
#
# This is a coverage loss, recorded rather than papered over: a fixture that
# rebuilds a document while `open_document` reads it is the thing to write
# first, and the case returns with it.

# ═══════════════════════════════════════════════════════════════════════════
# tree.rs: decide_freshness

case_ "decide_freshness: the Reindexed comparison never fires" \
  src-tauri/src/tree.rs \
  's~occupant\.current_document_id != document_id~false~' \
  'if false {' \
  mnema-desktop 'open_document_reports_reindexed_when_the_cited_path_now_names_another_document' --test commands

case_ "decide_freshness: only mtime is compared, size is dropped" \
  src-tauri/src/tree.rs \
  's~disk\.size_bytes != occupant\.size_bytes \|\| disk\.mtime != occupant\.mtime~disk.mtime != occupant.mtime~' \
  'if disk.mtime != occupant.mtime {' \
  mnema-desktop 'open_document_reports_file_changed_when_only_the_size_moved' --test commands

case_ "decide_freshness: only size is compared, mtime is dropped" \
  src-tauri/src/tree.rs \
  's~disk\.size_bytes != occupant\.size_bytes \|\| disk\.mtime != occupant\.mtime~disk.size_bytes != occupant.size_bytes~' \
  'if disk.size_bytes != occupant.size_bytes {' \
  mnema-desktop 'open_document_reports_file_changed_when_only_the_mtime_moved' --test commands

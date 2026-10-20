# Mutation cases for the file pane's reads (PR 5, re-aimed at `open_document`).
# Run with:
#
#   scripts/mutation-check.sh scripts/mutations/source.sh
#
# This file began as `source_around`'s — the paragraphs-around-a-citation
# command — and that command is gone: the pane shows the whole file and paints
# the cited passage inside it (spec §3). What survives is every case for the
# reads `open_document` still makes: `reading_window` was `source_around`'s
# window and is now uncalled by the product but still shipped and tested at the
# `mnema-index` layer, and `path_occupant`, `chunk_anchor`, `roots_holding_path`
# and `cited_occupant` are the same reads, unchanged. The IPC-level cases were
# re-aimed at the tests that now exercise those reads through `open_document`;
# the ones with no surviving fixture are deleted rather than retargeted.
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
# likely-equivalent shape here too, but no query in this file has that shape:
# every `ORDER BY` here either crosses a page boundary (`reading_window`, both
# get a case) or orders a column with no such index (`roots_holding_path`,
# also cased below).

# ═══════════════════════════════════════════════════════════════════════════
# reading_window: has_more_before / has_more_after (write.rs)

case_ "reading_window: has_more_before hardcoded false" \
  crates/mnema-index/src/write.rs \
  's~let has_more_before = before\.len\(\) as i64 > radius;~let has_more_before = false;~' \
  'let has_more_before = false;' \
  mnema-index 'reading_window_reports_more_before_when_exactly_one_block_is_out_of_range' --test source

case_ "reading_window: has_more_before hardcoded true" \
  crates/mnema-index/src/write.rs \
  's~let has_more_before = before\.len\(\) as i64 > radius;~let has_more_before = true;~' \
  'let has_more_before = true;' \
  mnema-index 'reading_window_reports_no_more_when_the_document_ends' --test source

case_ "reading_window: has_more_after hardcoded false" \
  crates/mnema-index/src/write.rs \
  's~let has_more_after = after\.len\(\) as i64 > radius;~let has_more_after = false;~' \
  'let has_more_after = false;' \
  mnema-index 'reading_window_returns_radius_blocks_each_side_in_document_reading_order' --test source

case_ "reading_window: has_more_after hardcoded true" \
  crates/mnema-index/src/write.rs \
  's~let has_more_after = after\.len\(\) as i64 > radius;~let has_more_after = true;~' \
  'let has_more_after = true;' \
  mnema-index 'reading_window_reports_more_before_when_exactly_one_block_is_out_of_range' --test source

# ─────────────────────────────────────────────────────────────────────────────
# reading_window: the LIMIT radius + 1 mechanism

case_ "reading_window: LIMIT radius + 1 loses the +1" \
  crates/mnema-index/src/write.rs \
  's~let limit = radius \+ 1;~let limit = radius;~' \
  'let limit = radius;' \
  mnema-index 'reading_window_reports_more_before_when_exactly_one_block_is_out_of_range' --test source

# ─────────────────────────────────────────────────────────────────────────────
# reading_window: the page term in each ORDER BY
#
# No case at the IPC layer for either of these two: every `open_document`
# fixture in `commands.rs` puts its document on ONE page, and no command
# reads a window any more, so nothing through the wire can falsify the page
# term the way the `mnema-index` three-page fixture does.

case_ "reading_window: before side loses its page term" \
  crates/mnema-index/src/write.rs \
  's~ORDER BY p\.page_no DESC, b\.reading_order DESC~ORDER BY b.reading_order DESC~' \
  'ORDER BY b.reading_order DESC' \
  mnema-index 'reading_window_returns_radius_blocks_each_side_in_document_reading_order' --test source

case_ "reading_window: after side loses its page term" \
  crates/mnema-index/src/write.rs \
  's~ORDER BY p\.page_no ASC, b\.reading_order ASC~ORDER BY b.reading_order ASC~' \
  'ORDER BY b.reading_order ASC' \
  mnema-index 'reading_window_returns_radius_blocks_each_side_in_document_reading_order' --test source

# ─────────────────────────────────────────────────────────────────────────────
# reading_window: the document term, widened away — "return another
# document's paragraphs under the user's citation", the hazard this whole
# command exists to refuse. Invisible without a decoy document inserted
# before the one under test (both fixtures below build one); with a single
# document in the index "every block" and "this document's blocks" are the
# same set, which is exactly how three mutants here survived Task 5.1's first
# gate.
#
# `/g`, 3 hits — the anchor, before and after statements each carry their own
# `b.document_id = ?1`.

case_ "reading_window: the document term is widened to match every document" \
  crates/mnema-index/src/write.rs \
  's~b\.document_id = \?1~(b.document_id = ?1 OR 1 = 1)~g' \
  '(b.document_id = ?1 OR 1 = 1)' \
  mnema-index 'reading_window_returns_radius_blocks_each_side_in_document_reading_order' --test source

# ─────────────────────────────────────────────────────────────────────────────
# reading_window (and chunk_anchor): p.page_no swapped for p.id throughout
#
# `/g`, 10 hits, deliberate — this is what makes the mutant self-consistent
# rather than merely broken: every place that WRITES a page number into a
# `SourceBlockRow` or a `ChunkAnchor` and every place that READS one back to
# scope a query now agree with each other, just on the wrong column. Pages
# inserted first and in document order get `page.id == page.page_no`, which is
# why a single-document fixture cannot see this at all; the decoy document
# inserted first in both fixtures below pushes the real document's page ids
# past its page numbers.

case_ "reading_window: p.page_no swapped for p.id throughout" \
  crates/mnema-index/src/write.rs \
  's~p\.page_no~p.id~g' \
  'p.id' \
  mnema-index 'reading_window_returns_radius_blocks_each_side_in_document_reading_order' --test source

# ═══════════════════════════════════════════════════════════════════════════
# path_occupant: the root predicate
#
# The path predicate's case used to sit here too; it now has its own section
# further down, because it was lost and restored separately.

case_ "path_occupant: the root predicate is widened to <=" \
  crates/mnema-index/src/write.rs \
  's~WHERE p\.watched_root_id = \?1 AND p\.relative_path = \?2~WHERE p.watched_root_id <= ?1 AND p.relative_path = ?2~' \
  'WHERE p.watched_root_id <= ?1 AND p.relative_path = ?2' \
  mnema-index 'path_occupant_reports_the_row_as_it_stands' --test source

case_ "chunk_anchor: MIN and MAX reading_order are swapped" \
  crates/mnema-index/src/write.rs \
  's~MIN\(b2\.reading_order\), MAX\(b2\.reading_order\)~MAX(b2.reading_order), MIN(b2.reading_order)~' \
  'MAX(b2.reading_order), MIN(b2.reading_order)' \
  mnema-index 'chunk_anchor_reports_the_page_and_reading_order_range_of_the_chunks_blocks' --test source

# `ChunkAnchor.ord` hardcoded to 0 instead of read from the row. Found by a
# fix-round review: before it, the only assertion on this field anywhere was
# `assert_eq!(anchor.ord, 0)`, and every IPC fixture cited `ord: 0` too, so a
# hardcoded zero survived. Only the `mnema-index` half remains: the IPC half
# was `source_around`'s, and `open_document`'s focus tests send `ord: 0` as
# well, so the wire case would be the same equivalent mutant.

case_ "chunk_anchor: ord is hardcoded to 0 instead of read from the row" \
  crates/mnema-index/src/write.rs \
  's~ord: row\.get\(1\)\?,~ord: { let _: i64 = row.get(1)?; 0 },~' \
  'ord: { let _: i64 = row.get(1)?; 0 },' \
  mnema-index 'chunk_anchor_reports_the_page_and_reading_order_range_of_the_chunks_blocks' --test source

# ═══════════════════════════════════════════════════════════════════════════
# roots_holding_path: the ambiguity-preserving scan collapses to one row

case_ "roots_holding_path: LIMIT 1 collapses the ambiguity" \
  crates/mnema-index/src/write.rs \
  's~SELECT watched_root_id FROM path WHERE relative_path = \?1 ORDER BY watched_root_id~SELECT watched_root_id FROM path WHERE relative_path = ?1 ORDER BY watched_root_id LIMIT 1~' \
  'ORDER BY watched_root_id LIMIT 1' \
  mnema-index 'roots_holding_path_returns_every_root' --test source

# ═══════════════════════════════════════════════════════════════════════════
# reading_window: whitespace-only blocks do not count against the radius
#
# The readers store a line of spaces as a block on purpose (mnema-extract's
# text reader treats it as content, not a separator), while chunk_blocks skips
# those **and more** — its rule is Unicode `str::trim`
# (`crates/mnema-chunk/src/lib.rs:121`), and this SQL set is a strict subset of
# it (see the note on `Db::reading_window`). Counting such blocks made `radius`
# mean stored rows instead of visible source. Found by owner review on PR #22.
#
# ⚠️ One case per occurrence, and the mutation is the SINGLE-argument `trim`,
# because that is the mistake that was actually made here: SQLite's `trim(X)`
# removes spaces and nothing else, so the first version of this fix let a
# tab-only block straight through. It is the likeliest way to break it again.
#
# ⚠️ **No case guards the *widening* direction.** `Db::reading_window`'s doc
# argues that the SQL set being a subset of the chunker's makes the predicate
# safe (nothing it excludes could have been a passage, so the anchor can never
# be emptied). Measured by review: adding a NON-whitespace character to the
# trim set leaves the whole suite green, because no fixture has a block that
# would then become empty. The subset property is argued, not measured — write
# a case for it only alongside a fixture that could go red, or it is decoration.
#
# ⚠️ The anchor-query case is gone with its fixture: it needed a chunk spanning
# *across* a blank block, and the only test that built one was `source_around`'s.
# No `mnema-index` test builds it (every one anchors a single block, where
# `BETWEEN n AND n` cannot contain a neighbour of any kind), so a case here
# would have no oracle. It belongs with a multi-block `open_document` fixture.

case_ "reading_window: the before query counts blank blocks" \
  crates/mnema-index/src/write.rs \
  "s~AND trim\(b\.text, ' ' \|\| char\(9, 10, 13, 11, 12\)\) <> '' AND \(p\.page_no, b\.reading_order\) <~AND trim(b.text) <> '' AND (p.page_no, b.reading_order) <~" \
  "AND trim(b.text) <> '' AND (p.page_no, b.reading_order) <" \
  mnema-index 'reading_window_skips_blocks_a_chunk_could_never_have_come_from' --test source

case_ "reading_window: the after query counts blank blocks" \
  crates/mnema-index/src/write.rs \
  "s~AND trim\(b\.text, ' ' \|\| char\(9, 10, 13, 11, 12\)\) <> '' AND \(p\.page_no, b\.reading_order\) >~AND trim(b.text) <> '' AND (p.page_no, b.reading_order) >~" \
  "AND trim(b.text) <> '' AND (p.page_no, b.reading_order) >" \
  mnema-index 'reading_window_skips_blocks_a_chunk_could_never_have_come_from' --test source

# ═══════════════════════════════════════════════════════════════════════════
# path_occupant: the path predicate
#
# ⚠️ Measured, so it is not re-attempted: for the ROOT predicate **above**,
# `OR 1 = 1` is NOT a usable mutant — SQLite still returns the correct row
# under it on every fixture in this file
# (`path_occupant_reports_the_row_as_it_stands`'s decoy root sorts BEFORE the
# real one, so both a correct query and an `OR 1 = 1` one land on the decoy's
# row only if the decoy is returned first, which it is not: the real root's own
# predicate already narrows to it). Only `<= ?1` bites there. For the PATH
# predicate in this section, `OR 1 = 1` does bite.
#
# 🔴 This case was deleted by mistake while removing roots_of_document_at's
# cases — it mutates path_occupant and has nothing to do with that method. It
# was verified still killing its test on the head it was deleted from.

case_ "path_occupant: the path predicate is widened to match everything" \
  crates/mnema-index/src/write.rs \
  "s~WHERE p\.watched_root_id = \?1 AND p\.relative_path = \?2~WHERE p.watched_root_id = ?1 AND (p.relative_path = ?2 OR 1 = 1)~" \
  'AND (p.relative_path = ?2 OR 1 = 1)' \
  mnema-desktop 'open_document_reports_reindexed_when_the_cited_path_now_names_another_document' --test commands

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

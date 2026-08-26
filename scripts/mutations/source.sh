# Mutation cases for `source_around` (PR 5) — the read-only paragraphs-around-
# a-citation command. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/source.sh
#
# `source_around` answers the launcher's right card (§7, §7.1): the paragraph
# before a cited passage, the passage's own paragraph, and the paragraph
# after, plus a refusal when the chunk id no longer names that passage and a
# freshness verdict when it does. Every mutation below is silent when broken —
# a wrong neighbour, a flag that never flips, a refusal that lets the wrong
# text through, a freshness verdict painted over stale text — so each case
# turns a named test red rather than trusting the query to fail loudly.
#
# ⚠️ Every pattern below was counted against the tree at 3ad20f0 (unchanged
# through 6d8e7d3, the branch tip this file was written against) and hits
# EXACTLY once, unless it carries `/g` — checked with `grep -c` before writing
# each case, not copied out of the plan's prose. `case_` refuses a
# substitution with more than one hit (or, for a `/g` case, fewer than one)
# and reports it as a BROKEN CASE.
#
# Where a mutation has a test at both the `mnema-index` layer (calling the
# method directly, `crates/mnema-index/tests/source.rs`) and the `commands.rs`
# layer (reaching it through the `source_around` command and the wire,
# `src-tauri/tests/commands.rs`), both cases are written — the second is the
# "does this actually reach the command a person calls" half, exactly as
# `tree.sh` does for PR 4. Some rows have only one layer: the two `ORDER BY`
# page-term drops have no multi-page fixture through the IPC; the anchor
# `BETWEEN` collapse has no multi-block fixture at the `mnema-index` layer;
# the `readingOrder` value is asserted only through the IPC.
#
# ── Not covered here, and why ─────────────────────────────────────────────
#
# `eq_ignore_ascii_case` on the identity pin (`anchor.text != passage_text`
# loosened to a case-insensitive compare): an EQUIVALENT mutant on these
# fixtures. ASCII case folding does not touch Cyrillic text, and every pin
# fixture in this file is Ukrainian, because the product is
# ([measure-the-product-configuration]). Recorded in the plan and here so it
# reads as seen, not missed.
#
# The race guard's own outcome distribution (`excerpt=N gone=M`) has no case
# at all, by decision: it is machine-dependent, so a person reads it into the
# ledger rather than a case asserting a number. `rebuilds > 0` — the one part
# of that test that is not distribution-dependent — is asserted in the test
# itself, not mutated here.
#
# Deleting the `WireSegment` conversion and sending `Segment` straight has no
# case either: it needs two coordinated edits (`Vec<WireSegment>` in two
# places and the `.map`), and `case_` takes one substitution. Case 21 below
# covers the same seam through a value permutation instead.
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

case_ "reading_window: has_more_before hardcoded false, end to end" \
  crates/mnema-index/src/write.rs \
  's~let has_more_before = before\.len\(\) as i64 > radius;~let has_more_before = false;~' \
  'let has_more_before = false;' \
  mnema-desktop 'source_around_returns_the_paragraphs_around_a_cited_passage' --test commands

case_ "reading_window: has_more_before hardcoded true" \
  crates/mnema-index/src/write.rs \
  's~let has_more_before = before\.len\(\) as i64 > radius;~let has_more_before = true;~' \
  'let has_more_before = true;' \
  mnema-index 'reading_window_reports_no_more_when_the_document_ends' --test source

case_ "reading_window: has_more_before hardcoded true, end to end" \
  crates/mnema-index/src/write.rs \
  's~let has_more_before = before\.len\(\) as i64 > radius;~let has_more_before = true;~' \
  'let has_more_before = true;' \
  mnema-desktop 'source_around_reports_more_after_but_not_before_at_the_start_of_a_document' --test commands

case_ "reading_window: has_more_after hardcoded false" \
  crates/mnema-index/src/write.rs \
  's~let has_more_after = after\.len\(\) as i64 > radius;~let has_more_after = false;~' \
  'let has_more_after = false;' \
  mnema-index 'reading_window_returns_radius_blocks_each_side_in_document_reading_order' --test source

case_ "reading_window: has_more_after hardcoded false, end to end" \
  crates/mnema-index/src/write.rs \
  's~let has_more_after = after\.len\(\) as i64 > radius;~let has_more_after = false;~' \
  'let has_more_after = false;' \
  mnema-desktop 'source_around_returns_the_paragraphs_around_a_cited_passage' --test commands

case_ "reading_window: has_more_after hardcoded true" \
  crates/mnema-index/src/write.rs \
  's~let has_more_after = after\.len\(\) as i64 > radius;~let has_more_after = true;~' \
  'let has_more_after = true;' \
  mnema-index 'reading_window_reports_more_before_when_exactly_one_block_is_out_of_range' --test source

case_ "reading_window: has_more_after hardcoded true, end to end" \
  crates/mnema-index/src/write.rs \
  's~let has_more_after = after\.len\(\) as i64 > radius;~let has_more_after = true;~' \
  'let has_more_after = true;' \
  mnema-desktop 'source_around_admits_a_byte_identical_passage_text' --test commands

# ─────────────────────────────────────────────────────────────────────────────
# reading_window: the LIMIT radius + 1 mechanism

case_ "reading_window: LIMIT radius + 1 loses the +1" \
  crates/mnema-index/src/write.rs \
  's~let limit = radius \+ 1;~let limit = radius;~' \
  'let limit = radius;' \
  mnema-index 'reading_window_reports_more_before_when_exactly_one_block_is_out_of_range' --test source

case_ "reading_window: LIMIT radius + 1 loses the +1, end to end" \
  crates/mnema-index/src/write.rs \
  's~let limit = radius \+ 1;~let limit = radius;~' \
  'let limit = radius;' \
  mnema-desktop 'source_around_returns_the_paragraphs_around_a_cited_passage' --test commands

# ─────────────────────────────────────────────────────────────────────────────
# reading_window: the page term in each ORDER BY
#
# No case at the IPC layer for either of these two: every `source_around`
# fixture in `commands.rs` puts its document on ONE page, so a window built
# from `list_tree`'s neighbouring command never crosses a page boundary
# through the wire and cannot falsify the page term the way the `mnema-index`
# three-page fixture does.

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
# reading_window: the anchor's own BETWEEN, collapsed to its first bound
#
# ⚠️ `+ 0 * ?4` keeps the placeholder count. `BETWEEN ?3 AND ?3` alone still
# binds four values to a four-parameter statement, so that shape would not
# trip the parameter-count trap this project has already paid for once
# ([test-stands-on-neighbouring-defence]) — but it is written this way anyway,
# matching the plan's own worked example, so the case documents the discipline
# rather than relying on this query's parameter count to save it by accident.
#
# No `mnema-index`-layer case: every `reading_window` test in `tests/source.rs`
# anchors a single-segment chunk, where `first_reading_order ==
# last_reading_order` and collapsing the range to its first bound changes
# nothing. Only a multi-block chunk — built solely through the IPC fixture —
# makes the second bound observable.

case_ "reading_window: the anchor's own BETWEEN collapses to its first bound" \
  crates/mnema-index/src/write.rs \
  's~AND b\.reading_order BETWEEN \?3 AND \?4~AND b.reading_order BETWEEN ?3 AND (?3 + 0 * ?4)~' \
  'AND b.reading_order BETWEEN ?3 AND (?3 + 0 * ?4)' \
  mnema-desktop 'source_around_covers_every_block_a_multi_block_chunk_spans' --test commands

# ─────────────────────────────────────────────────────────────────────────────
# reading_window: the reading_order column, in all three SELECTs at once
#
# `/g`, 3 hits — the anchor, before and after statements share this column
# list verbatim. No `mnema-index`-layer case: no test in `tests/source.rs`
# asserts on `SourceBlockRow::reading_order`'s VALUE (only `.text` and
# `.page_no`), so only the wire-level `readingOrder` assertion can see a
# window that shipped `block.id` in its place.

case_ "reading_window: reading_order swapped for the block's own rowid" \
  crates/mnema-index/src/write.rs \
  's~SELECT b\.id, b\.type, b\.text, p\.page_no, b\.reading_order~SELECT b.id, b.type, b.text, p.page_no, b.id~g' \
  'SELECT b.id, b.type, b.text, p.page_no, b.id' \
  mnema-desktop 'source_around_returns_the_paragraphs_around_a_cited_passage' --test commands

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

case_ "reading_window: the document term is widened to match every document, end to end" \
  crates/mnema-index/src/write.rs \
  's~b\.document_id = \?1~(b.document_id = ?1 OR 1 = 1)~g' \
  '(b.document_id = ?1 OR 1 = 1)' \
  mnema-desktop 'source_around_returns_the_paragraphs_around_a_cited_passage' --test commands

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

case_ "reading_window: p.page_no swapped for p.id throughout, end to end" \
  crates/mnema-index/src/write.rs \
  's~p\.page_no~p.id~g' \
  'p.id' \
  mnema-desktop 'source_around_returns_the_paragraphs_around_a_cited_passage' --test commands

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

# ═══════════════════════════════════════════════════════════════════════════
# roots_holding_path: the ambiguity-preserving scan collapses to one row

case_ "roots_holding_path: LIMIT 1 collapses the ambiguity" \
  crates/mnema-index/src/write.rs \
  's~SELECT watched_root_id FROM path WHERE relative_path = \?1 ORDER BY watched_root_id~SELECT watched_root_id FROM path WHERE relative_path = ?1 ORDER BY watched_root_id LIMIT 1~' \
  'ORDER BY watched_root_id LIMIT 1' \
  mnema-index 'roots_holding_path_returns_every_root' --test source

# ═══════════════════════════════════════════════════════════════════════════
# tree.rs: the identity pin

case_ "the identity pin never refuses" \
  src-tauri/src/tree.rs \
  's~anchor\.text != passage_text~false~' \
  'if false {' \
  mnema-desktop 'source_around_refuses_a_chunk_id_a_rebuild_has_handed_to_other_text' --test commands

case_ "the identity pin refuses everything" \
  src-tauri/src/tree.rs \
  's~anchor\.text != passage_text~true~' \
  'if true {' \
  mnema-desktop 'source_around_admits_a_byte_identical_passage_text' --test commands

# ═══════════════════════════════════════════════════════════════════════════
# tree.rs: the occurrence-identity pin (Task 2) — documentId and ord, beside
# the text
#
# The text pin above cannot see a reused id that lands on identical text: two
# documents whose middle paragraph happens to be byte-identical, or the same
# paragraph twice inside one document. `document_id` and `ord` are what catch
# each, and this drops only the `ord` half — `document_id` alone still refuses
# the CROSS-document case, so the marker is a symbol name (never a line
# number: line citations into this file have gone stale before) and the test
# below is chosen because it is the one case where `document_id` matches and
# only `ord` differs, so dropping `ord` and nothing else is what makes it
# redden.

case_ "the occurrence-identity pin drops its ord half" \
  src-tauri/src/tree.rs \
  's~anchor\.document_id != cited_document_id \|\| anchor\.ord != cited_ord~anchor.document_id != cited_document_id~' \
  'if anchor.document_id != cited_document_id {' \
  mnema-desktop 'source_around_refuses_a_reused_id_within_the_same_document_at_a_different_ord' --test commands

# This drops the OTHER half — `document_id` — and keeps only `ord`. Found by a
# fix-round review reproducing owner-Codex P1 a second way: every fixture that
# exercises the identity pin used to vary `document_id` and `ord` TOGETHER, so
# either half alone always agreed with the other about the verdict, and this
# mutation left 71 tests green. Fixed by giving `source_around_refuses_a_reused_id_whose_text_is_byte_identical`'s
# reused chunk the SAME `ord` its cited counterpart has (both 0) — the state
# where only `document_id` differs.

case_ "the occurrence-identity pin drops its document_id half" \
  src-tauri/src/tree.rs \
  's~anchor\.document_id != cited_document_id \|\| ~~' \
  'if anchor.ord != cited_ord {' \
  mnema-desktop 'source_around_refuses_a_reused_id_whose_text_is_byte_identical' --test commands

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
# The anchor case is the one that needed a fixture built for it: every test but
# one anchors on a single block, where `BETWEEN n AND n` cannot contain a
# neighbour of any kind. A chunk legitimately spans *across* a blank block, and
# `source_around_covers_every_block_a_multi_block_chunk_spans` now builds that.

case_ "reading_window: the anchor query counts blank blocks inside the quotation" \
  crates/mnema-index/src/write.rs \
  "s~AND trim\(b\.text, ' ' \|\| char\(9, 10, 13, 11, 12\)\) <> '' AND p\.page_no~AND trim(b.text) <> '' AND p.page_no~" \
  "AND trim(b.text) <> '' AND p.page_no" \
  mnema-desktop 'source_around_covers_every_block_a_multi_block_chunk_spans' --test commands

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
  mnema-desktop 'source_around_reports_reindexed_when_the_cited_path_now_names_another_document' --test commands

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
  mnema-desktop 'source_around_reports_no_path_when_two_roots_share_the_path_even_if_the_document_differs' --test commands

# ═══════════════════════════════════════════════════════════════════════════
# tree.rs: a missing section title ships no key, rather than an explicit null

case_ "Excerpt::section_title skips serialisation when None" \
  src-tauri/src/tree.rs \
  's~        section_title: Option<String>,~        #[serde(skip_serializing_if = "Option::is_none")]\n        section_title: Option<String>,~' \
  'skip_serializing_if' \
  mnema-desktop 'tree::tests::source_wire_shape_is_camel_case' --lib

# ═══════════════════════════════════════════════════════════════════════════
# tree.rs: the two hasMore flags, swapped where the excerpt is assembled
#
# Every OTHER `source_around` fixture in `commands.rs` returns the two flags
# with the same value, so this swap needs the one asymmetric window: nothing
# before the passage, more after it.

case_ "build_source_around: has_more_before and has_more_after are swapped" \
  src-tauri/src/tree.rs \
  's~has_more_before: window\.has_more_before,~has_more_before: window.has_more_after,~' \
  'has_more_before: window.has_more_after,' \
  mnema-desktop 'source_around_reports_more_after_but_not_before_at_the_start_of_a_document' --test commands

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
# 🔴 DELIBERATELY NOT A CASE HERE. Read this before adding one back.
#
# The mutation is:
#
#   src-tauri/src/tree.rs
#   's~db\.read_snapshot\(\|db\| \{~Ok::<_, mnema_index::Error>(db).and_then(|db| {~'
#
# It replaces the method with an equivalent-typed call that runs the same
# closure with no transaction around it, so `chunk_anchor`, `reading_window`
# and `path_occupant` each read whatever is committed at their own instant.
# It is the ONLY mutation that proves the race guard actually raced — the
# identity pin's own case does not, because removing the pin reddens that test
# after any rebuild with no interleaving at all.
#
# It is not a case because **its kill is statistical**, and measured as such
# rather than assumed. 2026-08-25, ten runs of
# `cargo test -p mnema-desktop --test commands a_rebuild_racing_the_ipc_source_around`:
#
#   this mutation form  RED RED RED RED RED
#   the wrapper removed RED RED RED GREEN RED
#
# Nine of ten. The harness runs each case once, so roughly one run in ten it
# would report STILL GREEN with nothing wrong. That is worse than no case at
# all: a signal that cries wolf teaches the next reader to discount
# STILL GREEN, and STILL GREEN is the one word in this harness that must never
# be discounted.
#
# So the proof is a controller ritual instead, recorded in the cycle ledger:
# apply the mutation by hand, run the race test five times, record how many
# reddened AND the failure text. Two texts appear, and both are tears —
# `paragraphs: ""` (the window found the chunk already deleted while the pin
# had found it alive) and `paragraphs: "…редакція N"` (the window found the
# rebuilt text). ⚠️ The second text also appears when the PIN is removed with
# the snapshot intact, where nothing tore at all, so the text alone does not
# discriminate: only the empty one is unambiguous, and its absence in a given
# run proves nothing either way.

# ═══════════════════════════════════════════════════════════════════════════
# tree.rs: decide_freshness

case_ "decide_freshness: the Reindexed comparison never fires" \
  src-tauri/src/tree.rs \
  's~occupant\.current_document_id != document_id~false~' \
  'if false {' \
  mnema-desktop 'source_around_reports_reindexed_when_the_cited_path_now_names_another_document' --test commands

case_ "decide_freshness: only mtime is compared, size is dropped" \
  src-tauri/src/tree.rs \
  's~disk\.size_bytes != occupant\.size_bytes \|\| disk\.mtime != occupant\.mtime~disk.mtime != occupant.mtime~' \
  'if disk.mtime != occupant.mtime {' \
  mnema-desktop 'source_around_reports_file_changed_when_only_the_size_moved' --test commands

case_ "decide_freshness: only size is compared, mtime is dropped" \
  src-tauri/src/tree.rs \
  's~disk\.size_bytes != occupant\.size_bytes \|\| disk\.mtime != occupant\.mtime~disk.size_bytes != occupant.size_bytes~' \
  'if disk.size_bytes != occupant.size_bytes {' \
  mnema-desktop 'source_around_reports_file_changed_when_only_the_mtime_moved' --test commands

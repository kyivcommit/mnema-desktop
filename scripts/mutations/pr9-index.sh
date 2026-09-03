# The settings window's two whole-index numbers — §9.3's file count and its
# last-indexed moment. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/pr9-index.sh
#
# Two files are mutated and they carry two different kinds of claim. The
# `crates/mnema-index/src/write.rs` cases pin the DEFINITIONS: each of the two
# helpers is one of the tree listing's own queries with a filter dropped, and
# every case here breaks exactly the term that makes it that query rather than
# a plausible neighbour. The `src-tauri/src/models.rs` cases pin the WIRING:
# that `read_settings` actually asks the index, rather than sending a constant
# the window would draw as a measured claim.
#
# 🔴 **The one that is not a bug-hunt but a decision guard.** `indexed_file_count`
# counts `path` rows, so one file present in two watched folders counts twice.
# That is D-e, argued and paid for: the Folders section draws one row per
# folder carrying that folder's own file count, and a whole-index number that
# de-duplicated by document would disagree with the rows one click away. The
# `COUNT(DISTINCT d.id)` case below is the mutant that makes that a decision
# instead of an accident — it is a perfectly reasonable query, and the only
# fixture that kills it is a document reachable from two roots.
#
# No count of the cases here, deliberately — `pr8-exclusions.sh` explains what
# that number costs when it goes stale. Re-derive:
#
#   grep -c '^case_ ' scripts/mutations/pr9-index.sh                     # cases
#   grep -oE "mnema-(index|desktop) '[^']+'" scripts/mutations/pr9-index.sh | sort -u
#
# ⚠️ **Every test named here is an INTEGRATION test**, so none of the names
# carries a `module::tests::` prefix and every case names its file with
# `--test`: `--test tree` for `crates/mnema-index/tests/tree.rs`, `--test
# commands` for `src-tauri/tests/commands.rs`. A name given the unit-test shape
# selects nothing, and the harness then reports a BASELINE FAILURE for the
# whole file rather than a result.
#
# ⚠️ **`\x27` is a single quote.** The SQL these cases mutate is full of them
# and a case file's expression is itself single-quoted, so the escape is used
# in both the pattern and the replacement rather than closing and reopening the
# shell quoting around every one.
#
# ── Disclosed, not shipped silently ──────────────────────────────────────────
#
# One guard this task added has no case here and cannot have one: that
# `indexedFiles`, `lastIndexedAt` and `failedChunks` are REQUIRED on
# `IndexSettings`' read arm (`ui/src/lib/ipc.ts`). Its oracle is
# `@ts-expect-error` in `ui/src/lib/ipc.test.ts`, which is a TYPE error — it is
# judged by `npm run check`, and this harness runs `cargo test` and `vitest`,
# neither of which type-checks. Making the field optional leaves vitest green,
# and a case for it here would be scored as a surviving mutant about a guard
# that in fact works. It was verified by hand instead, and the verification is
# repeatable: make `indexedFiles` optional in `ui/src/lib/ipc.ts` and run
# `npm run check`. Measured on this branch, it reports
# `ERROR "src/lib/ipc.test.ts" 481:3 "Unused '@ts-expect-error' directive."`,
# which is the guard going red. Written down here rather than in a task report,
# because a report is not a place a later change trips over.

# ── `indexed_file_count`: the definition ─────────────────────────────────────

# The status filter, gone: every path row counts, whatever the document behind
# it is. The screen would then claim files a person cannot search or cite — a
# pending document has nothing in it yet and a failed one never will.
case_ "a pending or failed document must not be counted as an indexed file" \
  crates/mnema-index/src/write.rs \
  's~               JOIN document d ON d\.id = p\.document_id\n              WHERE d\.status = \x27indexed\x27",~               JOIN document d ON d.id = p.document_id", // mutant: every path row counts~' \
  'JOIN document d ON d.id = p.document_id", // mutant: every path row counts' \
  mnema-index 'a_pending_or_failed_document_is_not_an_indexed_file' --test tree

# 🔴 D-e itself. A count of documents rather than of path rows — the tidier
# definition, and the one that disagrees with the folder rows beside it. Every
# other test in this file passes under this mutant; only a document reachable
# from two watched folders can tell the two definitions apart.
case_ "one file in two watched folders must count twice, matching its two folder rows" \
  crates/mnema-index/src/write.rs \
  's~"SELECT COUNT\(\*\)~"SELECT COUNT(DISTINCT d.id) /* mutant: documents, not files */~' \
  'SELECT COUNT(DISTINCT d.id) /* mutant: documents, not files */' \
  mnema-index 'a_document_reachable_from_two_folders_counts_once_for_each_folder_row' --test tree

# ── `last_indexed_at`: the definition ────────────────────────────────────────

# `created_at` instead of the completion checkpoint — the mistake
# `recent_indexed_documents`' own doc comment was written to stop being made
# again. `created_at` is stamped when the still-`pending` row is inserted and a
# rebuild keeps it, so it answers "when did this first enter the index", not
# "when did the index last finish anything".
case_ "the moment must be the completion checkpoint, not when the document first entered" \
  crates/mnema-index/src/write.rs \
  's~"SELECT MAX\(s\.updated_at\)~"SELECT MAX(d.created_at) /* mutant: entry, not completion */~' \
  'SELECT MAX(d.created_at) /* mutant: entry, not completion */' \
  mnema-index 'the_last_indexed_moment_is_the_one_at_the_top_of_recents' --test tree

# The oldest completion rather than the newest. Still a moment and still a real
# one, wrong by however long the index has been in use — a shape a fixture with
# one completed document could not tell apart, which is why the fixture judging
# it holds three at different times.
case_ "the moment must be the newest completion, not the oldest" \
  crates/mnema-index/src/write.rs \
  's~"SELECT MAX\(s\.updated_at\)~"SELECT MIN(s.updated_at) /* mutant: the oldest completion */~' \
  'SELECT MIN(s.updated_at) /* mutant: the oldest completion */' \
  mnema-index 'the_last_indexed_moment_is_the_one_at_the_top_of_recents' --test tree

# The status half of the stage predicate dropped, so any chunk stage counts as
# a finish. A rebuild in flight writes `rebuilding` over a finished stage with a
# fresh `updated_at` — the state `STATUS_REBUILDING` exists to make visible —
# and the window would report the index as up to date at the one moment it is
# being written again. The marker is double-quoted so it can carry a real
# single quote; it has to be the literal the mutation leaves behind, and a
# shorter prefix would be text the unmutated file already contains.
case_ "a rebuild in flight must not read as a completion" \
  crates/mnema-index/src/write.rs \
  's~AND s\.stage = \x27chunk\x27 AND s\.status = \x27done\x27\n              WHERE d\.status = \x27indexed\x27",~AND s.stage = \x27chunk\x27 /* mutant: rebuilding counts as done */\n              WHERE d.status = \x27indexed\x27",~' \
  "AND s.stage = 'chunk' /* mutant: rebuilding counts as done */" \
  mnema-index 'a_rebuild_in_flight_is_not_a_completion' --test tree

# ── `read_settings`: the wiring ──────────────────────────────────────────────
#
# Both helpers can be right and the window still draw a number nobody measured.
# `0` and `null` are the two values that look like an honest answer — an empty
# index, and an index that has never finished — so a constant here is invisible
# until somebody walks a folder, which is exactly what the test below does.

case_ "the file count on the wire must be read from the index, not sent as a constant" \
  src-tauri/src/models.rs \
  's~        indexed_files: db\.indexed_file_count\(\)\?,~        indexed_files: 0, // mutant: a constant that looks like an empty index~' \
  'indexed_files: 0, // mutant: a constant that looks like an empty index' \
  mnema-desktop 'the_settings_carry_the_whole_index_file_count_and_its_last_indexed_moment' --test commands

case_ "the moment on the wire must be read from the index, not sent as a constant" \
  src-tauri/src/models.rs \
  's~        last_indexed_at: db\.last_indexed_at\(\)\?,~        last_indexed_at: None, // mutant: a constant that looks like a fresh index~' \
  'last_indexed_at: None, // mutant: a constant that looks like a fresh index' \
  mnema-desktop 'the_settings_carry_the_whole_index_file_count_and_its_last_indexed_moment' --test commands

# ── `contended`: the seam, and the sentence it draws ─────────────────────────
#
# Three cases for one field crossing three layers. The first is the wire: the
# shell dropping the walk's own number. The other two are the strip, and they
# are the pair that makes the line a *statement about part of the skipped
# count* rather than a second count of its own — one for drawing it when there
# was no contention, one for folding it into the number it is supposed to
# explain.
#
# ⚠️ The first case's oracle takes about twenty seconds by construction: the
# only fixture that puts a non-zero `contended` on the wire holds the index's
# write lock through every busy retry. That is not slack in the test.

# The shell forwards `0` instead of what the walk reported. Everything else on
# the wire is unchanged, so nothing but a walk that actually met a held lock
# can tell the difference — the mirror case (`an_uncontended_walk_reports_no_
# contention_on_any_event`) passes under this mutant, which is the point of it.
case_ "the shell must forward the walk's own contention, not a zero" \
  src-tauri/src/walk_job.rs \
  's~                            contended: progress\.contended,~                            contended: 0, // mutant: the busy index never reaches the window~' \
  'contended: 0, // mutant: the busy index never reaches the window' \
  mnema-desktop 'a_walk_that_meets_a_busy_index_says_so_on_the_wire' --test commands

# The line drawn on every running pass. It reads as an explanation of the
# skipped number, so on a scan that met no lock at all it is simply false — and
# it would be on screen for the whole of every ordinary run.
case_ "the busy-index line must be drawn only when the scan actually met the lock" \
  ui/src/settings/JobStrip.svelte \
  "s~    if \(phase\.kind !== 'running' \|\| phase\.counts\.contended === 0\) return null;~    if (phase.kind !== 'running') return null; // mutant: drawn whether or not the index was busy~" \
  "if (phase.kind !== 'running') return null; // mutant: drawn whether or not the index was busy" \
  src/settings/JobStrip.test.ts 'a scan that met no busy index says nothing about one' runner=vitest

# 🔴 The decision guard. `contended` counts files that are journalled as skips
# a moment later and counted in `skipped` too, so adding the two counts one
# file twice — and the sentence beside the number would then be explaining a
# total that already contains what it is explaining. This mutant is the tidier
# arithmetic and the wrong one, and only an assertion that reads the whole
# rendered counts line can tell them apart. A testid could not.
case_ "the skipped number must not absorb the contended files it already counts" \
  ui/src/settings/JobStrip.svelte \
  's~    const common = \{ done: counts\.done, skipped: counts\.skipped, refused: counts\.refused \};~    const common = { done: counts.done, skipped: counts.skipped + counts.contended, refused: counts.refused }; // mutant: one file counted twice~' \
  'skipped: counts.skipped + counts.contended, refused: counts.refused }; // mutant: one file counted twice' \
  src/settings/JobStrip.test.ts 'a scan that met a busy index says so, in both languages, without touching the counts' runner=vitest

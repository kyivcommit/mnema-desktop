# Mutation cases for D64: the harness models the rebuild path. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-d64.sh
#
# Per D63 this task also runs `task-14.sh` and `task-13.sh`, which measure the
# same file from the two cycles before it.
#
# What is peculiar to this file: the machinery under test is only reachable
# through a worker that **succeeds**. Everything the harness had could do was
# refuse, so no corpus before this one drove `ingest_file` through a rebuild at
# all — and Task 14's corpus assertion, written precisely to expose an unreached
# class, passed at full strength over it. Half the cases below therefore break
# the **generator** and require the corpus assertion to notice; the other half
# break the **product** and require an invariant to.

# ------------------------------------------------ the rebuild is a rebuild

# C1. **The decision this task exists for.** A reader *version* that differs
# from the one recorded must rebuild the document rather than confirm it.
# Ignoring it means a release that improves a reader never re-reads anything:
# every file keeps the text the old reader produced, for ever, and the only sign
# is that nothing changes.
case_ "ingest: a different reader version rebuilds instead of confirming" \
  crates/mnema-ingest/src/lib.rs \
  's~            entry\.reader != document\.reader\n                \|\| entry\.reader_version != i64::from\(document\.reader_version\)~            entry.reader != document.reader // version ignored~' \
  'entry.reader != document.reader // version ignored' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# C2. The reader *name* half of the same comparison — the one a format changing
# hands moves, as `.html` did inside this cycle when it left the text reader.
case_ "ingest: a different reader name rebuilds too" \
  crates/mnema-ingest/src/lib.rs \
  's~            entry\.reader != document\.reader\n                \|\| entry\.reader_version != i64::from\(document\.reader_version\)~            entry.reader_version != i64::from(document.reader_version) // name ignored~' \
  'i64::from(document.reader_version) // name ignored' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# ------------------------------- what a half-written document may answer

# C3. **D61, and invariant 3f.** Slice 0 of a rebuild writes `rebuilding` over
# the `done` the document started with, which is what stops the cheap arm and
# every settled-document check from treating a half-replaced document as
# finished. Leaving the stage `done` means a rebuild cut between slices answers
# a search with the chunks that happened to land — half a contract, with nothing
# anywhere saying it is half.
case_ "ingest: a document being rebuilt does not still look finished" \
  crates/mnema-ingest/src/lib.rs \
  's~                    db\.record_stage\(&id, STAGE_CHUNK, STATUS_REBUILDING\)\?;~                    db.record_stage(\&id, STAGE_CHUNK, STATUS_DONE)?;~' \
  'db.record_stage(&id, STAGE_CHUNK, STATUS_DONE)?;' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# ----------------------------------------------------------- the generator

# C4. The sidecar stops announcing a different version, so every "the build
# learned to read better" step becomes an ordinary confirming walk. Every
# invariant still passes — nothing is wrong, nothing happened — and the corpus
# assertion is the only thing that can say the class was never reached.
#
# ⚠️ This case was **still green** on its first run, and the fault was in the
# harness: the class was recorded on `Verdict::Settled`, which folds
# `AlreadyIndexed` in with `Indexed`, so a confirming walk marked the rebuild as
# covered. A class recorded when it did not happen is worse than one never
# recorded — it reports coverage rather than absence.
case_ "harness: the corpus really drives a rebuild" \
  crates/mnema-ingest/tests/randomised.rs \
  's~            let version = 2 \+ \(self\.stricter_rotation % 3\) as u32;\n            let better = better_reader_worker\(self\.dir\.path\(\), version\);\n            self\.note\(format!\(\n                "  walk~            let version = 1;\n            let better = better_reader_worker(self.dir.path(), version);\n            self.note(format!(\n                "  walk~' \
  '            let version = 1;' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# C5. The interruption stops interrupting. Without a trigger that fires on a
# page the *second* slice writes, the rebuild completes in one go and the pass
# after it has nothing to finish.
case_ "harness: the corpus really cuts a rebuild between slices" \
  crates/mnema-ingest/tests/randomised.rs \
  's~WHEN new\.page_no > 20 BEGIN~WHEN new.page_no > 100000 BEGIN~' \
  'WHEN new.page_no > 100000 BEGIN' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# C6. The document stops being long enough to be cut. `PAGES_PER_TRANSACTION` is
# 20, so a document of two pages is written in a single transaction whatever
# else happens: there is no seam, and an interruption between slices is not a
# state this corpus can reach at all.
case_ "harness: the document a rebuild is cut in is long enough to have a seam" \
  crates/mnema-ingest/tests/randomised.rs \
  's~            let units = mnema_ingest::PAGES_PER_TRANSACTION \+ 2 \+ self\.rng\.below\(4\);~            let units = 2;~' \
  '            let units = 2;' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

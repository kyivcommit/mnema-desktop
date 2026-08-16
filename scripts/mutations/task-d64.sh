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
#
# ⚠️ **Green until `a_format_changes_hands` existed.** Every pass that changed
# the name also changed the version, so deleting this comparison changed no
# outcome: the version comparison beside it still answered. The case only means
# something against a corpus that can move one without the other, and reaching
# that needed a file recorded at exactly `markdown@1` — which needed an
# operation that builds its own.
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

# C4. The operation stops being drawn at all. Every invariant still passes —
# nothing is wrong, nothing happened — and the corpus assertion is the only
# thing that can say the class was never reached.
#
# ⚠️ **Written first as "make the sidecar announce version 1" and that case was
# green**, for a reason worth keeping: the *other* rebuild operation leaves its
# files recorded at version 2 or above, so offering version 1 over one of those
# is still a version change and still rebuilds. A mutation aimed at the value
# tested the arithmetic; aiming it at the draw tests the class.
case_ "harness: the corpus really drives a rebuild" \
  crates/mnema-ingest/tests/randomised.rs \
  's~            26 => self\.the_build_learned_to_read_better\(\),~            26 => self.run_walk(),~' \
  '26 => self.run_walk(),' \
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

# C7. The handover operation stops being drawn, so the name half of
# `stale_reading` has no generator again and C2 goes quiet with it.
case_ "harness: the corpus really moves a format between readers" \
  crates/mnema-ingest/tests/randomised.rs \
  's~            28 => self\.a_format_changes_hands\(\),~            28 => self.run_walk(),~' \
  '28 => self.run_walk(),' \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# ------------------------------------------------- the guard on the guard

# C8. 🔴 **The case invariant 3f did not have, and the reason it matters more
# than a missing case.** C3 above is labelled for 3f and cites it, and what it
# actually fires is **invariant 5** — the stage/status pair — with or without 3f
# present. So nothing in any mutation file would have noticed 3f being deleted or
# weakened, and 3f is the **only** thing in the suite protecting D61's decision
# that a document being written answers nothing.
#
# The guard behind that decision is one clause: `search_lexical` filters
# `AND document.status = 'indexed'`. Drop it and a document mid-rebuild answers
# searches with the chunks that happened to land. Measured by the reviewer:
# without this clause and with 3f disabled the whole suite is **green**; with 3f
# present it reddens on 3f by name.
case_ "index: a search does not answer from a document that is not indexed" \
  crates/mnema-index/src/search.rs \
  "s~SELECT chunk_fts\\.rowid FROM chunk_fts\\n               JOIN chunk ON chunk\\.id = chunk_fts\\.rowid\\n               JOIN document ON document\\.id = chunk\\.document_id\\n              WHERE chunk_fts MATCH \\?1 AND document.status = 'indexed'~SELECT chunk_fts.rowid FROM chunk_fts
               JOIN chunk ON chunk.id = chunk_fts.rowid
               JOIN document ON document.id = chunk.document_id
              WHERE chunk_fts MATCH ?1 ~" \
  "WHERE chunk_fts MATCH ?1" \
  mnema-ingest 'random_sequences_do_not_lose_data' --test randomised

# Mutation cases for D61 — a document that is being written answers no search.
# Run with:
#
#   scripts/mutation-check.sh scripts/mutations/task-d61.sh
#
# The change has two halves and they are useless apart, so both are mutated
# here. The predicate in `search_lexical` is what declines to answer with a
# document that is not `indexed`; the `pending` in `clear_document_content` is
# what makes `indexed` true of a document being rebuilt in the first place.
# Take either one out and one of the two write paths goes back to answering
# searches with part of a document — which is why the same mutation is run
# against two tests below rather than one.
#
# Every case anchors on a line of code rather than on prose around it: five
# cases were lost on this branch to a fix round that edited comments, and a
# sixth to one that widened the line it was anchored to.

# ------------------------------------------------------ the predicate itself

case_ "search: no predicate at all — a first indexing answers mid-write" \
  crates/mnema-index/src/search.rs \
  "s{AND document\\.status = 'indexed'}{AND 1 = 1 /* predicate removed */}" \
  "AND 1 = 1 /* predicate removed */" \
  mnema-index 'a_document_still_being_written_answers_no_search' --test visibility

# The same mutation, the other write path. This is the case the whole task was
# raised for: an interrupted rebuild leaving twenty of twenty-five sections, a
# search for a surviving one hitting and a search for a lost one returning
# nothing, with no way to tell the two apart from the window.
case_ "search: no predicate at all — a rebuild answers mid-write" \
  crates/mnema-index/src/search.rs \
  "s{AND document\\.status = 'indexed'}{AND 1 = 1 /* predicate removed */}" \
  "AND 1 = 1 /* predicate removed */" \
  mnema-ingest 'a_document_being_rebuilt_answers_no_search_until_it_is_whole_again' --test slice

# `<> 'pending'` passes every test about a document mid-write and lets a
# document whose indexing FAILED answer searches. The column has four values,
# and the predicate names the one that may be searched rather than the one that
# may not.
case_ "search: the predicate names the status that may be searched" \
  crates/mnema-index/src/search.rs \
  "s{AND document\\.status = 'indexed'}{AND document.status <> 'pending'}" \
  "AND document.status <> 'pending'" \
  mnema-index 'a_failed_or_skipped_document_is_not_searchable_either' --test visibility

# The other direction, and without it every case above is satisfied by a search
# that answers nothing at all: `= 'pending'` silences the finished documents
# instead of the unfinished ones.
case_ "search: a finished document still answers" \
  crates/mnema-index/src/search.rs \
  "s{AND document\\.status = 'indexed'}{AND document.status = 'pending'}" \
  "AND document.status = 'pending'" \
  mnema-index 'the_same_document_answers_once_it_is_declared_finished' --test visibility

# The join carries the predicate from a chunk to its document, and it is the
# one place where a wrong column returns rows rather than an error: `block_id`
# is an integer on the same table, and the search then answers with a DIFFERENT
# document's chunk — the failure the four-level model exists to prevent, one
# level up. Only visible because `write_document` leaves a block the chunker
# did not keep, so the two id sequences are out of step.
case_ "search: the join reaches a chunk's document, not a neighbour's" \
  crates/mnema-index/src/search.rs \
  's{JOIN chunk ON chunk\.id = chunk_fts\.rowid}{JOIN chunk ON chunk.block_id = chunk_fts.rowid}' \
  'JOIN chunk ON chunk.block_id = chunk_fts.rowid' \
  mnema-index 'a_finished_document_is_found_while_another_is_being_written' --test visibility

# ------------------------------------------- and the half that makes it true

# Without this line the predicate is real and the rebuild walks straight past
# it: `clear_document_content` empties the document and the row goes on saying
# `indexed`, because nothing between the previous checkpoint and the next one
# ever writes the column. Measured before it was added — the case above went
# green with the predicate in place.
case_ "clear: emptying a document takes it out of the search" \
  crates/mnema-index/src/write.rs \
  's{self\.set_document_status\(id, DocumentStatus::Pending\)\?;}{let _ = DocumentStatus::Pending; /* status left as it was */}' \
  'let _ = DocumentStatus::Pending; /* status left as it was */' \
  mnema-ingest 'a_document_being_rebuilt_answers_no_search_until_it_is_whole_again' --test slice

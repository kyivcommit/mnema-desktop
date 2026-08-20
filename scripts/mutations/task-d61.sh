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
  "s{SELECT chunk_fts\\.rowid FROM chunk_fts\\n               JOIN chunk ON chunk\\.id = chunk_fts\\.rowid\\n               JOIN document ON document\\.id = chunk\\.document_id\\n              WHERE chunk_fts MATCH \\?1 AND document\\.status = 'indexed'}{SELECT chunk_fts.rowid FROM chunk_fts
               JOIN chunk ON chunk.id = chunk_fts.rowid
               JOIN document ON document.id = chunk.document_id
              WHERE chunk_fts MATCH ?1 AND 1 = 1 /* predicate removed */}" \
  "AND 1 = 1 /* predicate removed */" \
  mnema-index 'a_document_still_being_written_answers_no_search' --test visibility

# The same mutation, the other write path. This is the case the whole task was
# raised for: an interrupted rebuild leaving twenty of twenty-five sections, a
# search for a surviving one hitting and a search for a lost one returning
# nothing, with no way to tell the two apart from the window.
case_ "search: no predicate at all — a rebuild answers mid-write" \
  crates/mnema-index/src/search.rs \
  "s{SELECT chunk_fts\\.rowid FROM chunk_fts\\n               JOIN chunk ON chunk\\.id = chunk_fts\\.rowid\\n               JOIN document ON document\\.id = chunk\\.document_id\\n              WHERE chunk_fts MATCH \\?1 AND document\\.status = 'indexed'}{SELECT chunk_fts.rowid FROM chunk_fts
               JOIN chunk ON chunk.id = chunk_fts.rowid
               JOIN document ON document.id = chunk.document_id
              WHERE chunk_fts MATCH ?1 AND 1 = 1 /* predicate removed */}" \
  "AND 1 = 1 /* predicate removed */" \
  mnema-ingest 'a_document_being_rebuilt_answers_no_search_until_it_is_whole_again' --test slice

# `<> 'pending'` passes every test about a document mid-write and lets a
# document whose indexing FAILED answer searches. The column has four values,
# and the predicate names the one that may be searched rather than the one that
# may not.
case_ "search: the predicate names the status that may be searched" \
  crates/mnema-index/src/search.rs \
  "s{SELECT chunk_fts\\.rowid FROM chunk_fts\\n               JOIN chunk ON chunk\\.id = chunk_fts\\.rowid\\n               JOIN document ON document\\.id = chunk\\.document_id\\n              WHERE chunk_fts MATCH \\?1 AND document\\.status = 'indexed'}{SELECT chunk_fts.rowid FROM chunk_fts
               JOIN chunk ON chunk.id = chunk_fts.rowid
               JOIN document ON document.id = chunk.document_id
              WHERE chunk_fts MATCH ?1 AND document.status <> 'pending'}" \
  "AND document.status <> 'pending'" \
  mnema-index 'a_failed_or_skipped_document_is_not_searchable_either' --test visibility

# The other direction, and without it every case above is satisfied by a search
# that answers nothing at all: `= 'pending'` silences the finished documents
# instead of the unfinished ones.
case_ "search: a finished document still answers" \
  crates/mnema-index/src/search.rs \
  "s{SELECT chunk_fts\\.rowid FROM chunk_fts\\n               JOIN chunk ON chunk\\.id = chunk_fts\\.rowid\\n               JOIN document ON document\\.id = chunk\\.document_id\\n              WHERE chunk_fts MATCH \\?1 AND document\\.status = 'indexed'}{SELECT chunk_fts.rowid FROM chunk_fts
               JOIN chunk ON chunk.id = chunk_fts.rowid
               JOIN document ON document.id = chunk.document_id
              WHERE chunk_fts MATCH ?1 AND document.status = 'pending'}" \
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
  's{SELECT chunk_fts\.rowid FROM chunk_fts\n               JOIN chunk ON chunk\.id = chunk_fts\.rowid}{SELECT chunk_fts.rowid FROM chunk_fts
               JOIN chunk ON chunk.block_id = chunk_fts.rowid}' \
  'JOIN chunk ON chunk.block_id = chunk_fts.rowid' \
  mnema-index 'a_finished_document_is_found_while_another_is_being_written' --test visibility

# The same mutation once more, against `ingest_file` rather than a fixture. The
# first case above pins the DEFAULT on `insert_document`; this one pins that a
# first indexing really does spend its whole write under it. A change setting
# `Indexed` anywhere before step 5 leaves that case green and this one red.
case_ "search: no predicate at all — a first indexing answers mid-write, end to end" \
  crates/mnema-index/src/search.rs \
  "s{SELECT chunk_fts\\.rowid FROM chunk_fts\\n               JOIN chunk ON chunk\\.id = chunk_fts\\.rowid\\n               JOIN document ON document\\.id = chunk\\.document_id\\n              WHERE chunk_fts MATCH \\?1 AND document\\.status = 'indexed'}{SELECT chunk_fts.rowid FROM chunk_fts
               JOIN chunk ON chunk.id = chunk_fts.rowid
               JOIN document ON document.id = chunk.document_id
              WHERE chunk_fts MATCH ?1 AND 1 = 1 /* predicate removed */}" \
  "AND 1 = 1 /* predicate removed */" \
  mnema-ingest 'a_document_being_indexed_for_the_first_time_answers_no_search' --test slice

# The predicate has to be in the same statement as the LIMIT, not applied to the
# rows that statement returned: `WHERE` runs before `LIMIT`, so the limit counts
# hits a person may be shown. Spend it first and a document being written pushes
# finished ones off the end — here, off it entirely.
#
# The first report on this task called the case impossible to anchor narrowly
# and said so in writing, which would have left the next session believing it.
# It is one substring, the same one four cases above already use, and the review
# of that report is where it came from. Narrow, and measured to be: the control
# it was checked against — `a_document_still_being_written_answers_no_search`,
# where nothing contends for the limit — stays green under it.
case_ "search: the limit is spent before the predicate (the Rust-filter shape)" \
  crates/mnema-index/src/search.rs \
  "s{SELECT chunk_fts\\.rowid FROM chunk_fts\\n               JOIN chunk ON chunk\\.id = chunk_fts\\.rowid\\n               JOIN document ON document\\.id = chunk\\.document_id\\n              WHERE chunk_fts MATCH \\?1 AND document\\.status = 'indexed'}{SELECT chunk_fts.rowid FROM chunk_fts
               JOIN chunk ON chunk.id = chunk_fts.rowid
               JOIN document ON document.id = chunk.document_id
              WHERE chunk_fts MATCH ?1 AND document.status = 'indexed' AND chunk_fts.rowid IN (SELECT rowid FROM chunk_fts WHERE chunk_fts MATCH ?1 ORDER BY rank LIMIT ?2)}" \
  "AND chunk_fts.rowid IN (SELECT rowid FROM chunk_fts WHERE chunk_fts MATCH ?1 ORDER BY rank LIMIT ?2)" \
  mnema-index 'a_document_being_written_does_not_spend_the_limit' --test visibility

# ------------------------------------------- and the half that makes it true

# Without this line the predicate is real and the rebuild walks straight past
# it: `clear_document_content` empties the document and the row goes on saying
# `indexed`, because nothing between the previous checkpoint and the next one
# ever writes the column. Measured before it was added — the case above went
# green with the predicate in place.
case_ "clear: emptying a document takes it out of the search" \
  crates/mnema-index/src/write.rs \
  's{crate::journal::write_document_status\(tx, id, DocumentStatus::Pending\)}{let _ = DocumentStatus::Pending; Ok(()) /* status left as it was */}' \
  'let _ = DocumentStatus::Pending; Ok(()) /* status left as it was */' \
  mnema-ingest 'a_document_being_rebuilt_answers_no_search_until_it_is_whole_again' --test slice

# …and the two statements are one write or neither. Two statements on
# `self.conn()` is what this method was between the D61 fix and the review of
# it: correct for the one caller it had, and leaving the state D61 abolishes —
# content gone, status still `indexed` — one statement away for the next one.
# The mutation is that exact shape, restored.
# ------------------------------------ and the connection the pair commits on

# `same_connection` replaced a doc comment that described a failure which does
# not happen: a foreign transaction was said to deadlock, and it returns Ok in
# 407 µs while the write commits with somebody else's unit of work. The check is
# what makes the widened atomicity true, so it is mutated in both directions —
# off, and inverted — and the inverted case is what says it does not simply fire
# on everything, which would take the product's own rebuild down with it.
case_ "connection: a foreign transaction is refused by the clear" \
  crates/mnema-index/src/write.rs \
  's{std::ptr::eq\(db\.conn\(\), &\*\*tx\)}{true /* connection check removed */}' \
  'true /* connection check removed */' \
  mnema-index 'a_transaction_from_another_connection::is_refused_by_clear_document_content_in' --test citation

case_ "connection: a foreign transaction is refused by the chunk write" \
  crates/mnema-index/src/write.rs \
  's{std::ptr::eq\(db\.conn\(\), &\*\*tx\)}{true /* connection check removed */}' \
  'true /* connection check removed */' \
  mnema-index 'a_transaction_from_another_connection::is_refused_by_insert_chunk_in' --test citation

case_ "connection: and this Db's own transaction is not" \
  crates/mnema-index/src/write.rs \
  's{std::ptr::eq\(db\.conn\(\), &\*\*tx\)}{!std::ptr::eq(db.conn(), &**tx)}' \
  '!std::ptr::eq(db.conn(), &**tx)' \
  mnema-index 'a_transaction_from_another_connection::but_this_db_s_own_transaction_goes_through' --test citation

case_ "clear: the delete and the status are one write or neither" \
  crates/mnema-index/src/write.rs \
  's{self\.transaction\(\|tx\| self\.clear_document_content_in\(tx, id\)\)}{{ self.conn().execute("DELETE FROM page WHERE document_id = ?1", params![id])?; crate::journal::write_document_status(self.conn(), id, DocumentStatus::Pending) }}' \
  'crate::journal::write_document_status(self.conn(), id, DocumentStatus::Pending)' \
  mnema-index 'emptying_a_document_and_taking_it_out_of_the_search_are_one_write' --test citation

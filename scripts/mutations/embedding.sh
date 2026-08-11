# Mutation cases for the indexing-and-embedding cycle.
#
# Each `case` breaks exactly one thing and names the test that must go red.
# Cases are added by the task that adds the test they protect, so this file
# grows through the cycle rather than being written once at the end.

case_ "write: a rebuild takes its document's vectors with it (D88)" \
  crates/mnema-index/src/write.rs \
  's{        crate::space::delete_vectors_for_document_in\(tx, id\)\?;\n}{}' \
  'table is the target of no foreign key, so nothing else reaches it.
        tx.execute("DELETE FROM page' \
  mnema-index 'clearing_a_document_takes_its_vectors' --test citation

# The doc comment above `clear_document_content_in` calls the ordering
# load-bearing, not stylistic: swept after the page delete instead of before
# it, `delete_vectors_for_document_in` runs its `WHERE chunk_id IN (SELECT id
# FROM chunk WHERE document_id = ?1)` once the cascade has already taken every
# chunk naming that document, so the subquery finds none and deletes nothing.
# Same file, same two lines, the other order — one case per test it must
# still catch, since `case_` names one test at a time.
case_ "write: the vector sweep must run before the page delete, not after (D88)" \
  crates/mnema-index/src/write.rs \
  's{        crate::space::delete_vectors_for_document_in\(tx, id\)\?;\n        tx\.execute\("DELETE FROM page WHERE document_id = \?1", params!\[id\]\)\?;\n}{        tx.execute("DELETE FROM page WHERE document_id = ?1", params![id])?;\n        crate::space::delete_vectors_for_document_in(tx, id)?;\n}' \
  'params![id])?;
        crate::space::delete_vectors_for_document_in(tx, id)?;' \
  mnema-index 'clearing_a_document_takes_its_vectors' --test citation

case_ "write: the vector sweep must run before the page delete, not after (D88, second test)" \
  crates/mnema-index/src/write.rs \
  's{        crate::space::delete_vectors_for_document_in\(tx, id\)\?;\n        tx\.execute\("DELETE FROM page WHERE document_id = \?1", params!\[id\]\)\?;\n}{        tx.execute("DELETE FROM page WHERE document_id = ?1", params![id])?;\n        crate::space::delete_vectors_for_document_in(tx, id)?;\n}' \
  'params![id])?;
        crate::space::delete_vectors_for_document_in(tx, id)?;' \
  mnema-index 'a_reused_chunk_id_gets_no_inherited_vector' --test citation

# vec0 has no ON CONFLICT (space.rs's own comment on upsert_vector names the
# grep that shows it), so the delete before the insert is what makes the
# second write a replace rather than a second row. Removing it turns
# upsert_vector into what insert_vector already is — and the row-count half
# of the test above cannot tell that apart, only the stored-vector half can:
# without the delete the second write's INSERT collides with the first row's
# primary key and the call itself fails, which the count assertion never
# reaches.
case_ "space: upsert_vector without the delete stops being a replace (D95a)" \
  crates/mnema-index/src/space.rs \
  's{            tx\.execute\(\n                &format!\("DELETE FROM \{\} WHERE chunk_id = \?1", space\.table\),\n                params!\[chunk_id\],\n            \)\?;\n}{}' \
  'self.transaction(|tx| {
            tx.execute(
                &format!(
                    "INSERT INTO' \
  mnema-index 'upserting_a_vector_replaces_the_previous_one' --test space

# Review round 1 (D95a): `embedded_chunk_count`'s own UNION counts a
# `chunk_embedding_state` row on its own, so a vector deleted or replaced
# without also clearing that row leaves a chunk misreported — counted as
# embedded after `delete_vector`, or still marked failed after a successful
# `upsert_vector`. Two cases, one per method, since each clears its own row.
case_ "space: upsert_vector without clearing chunk_embedding_state leaves a stale row (D95a, review 1)" \
  crates/mnema-index/src/space.rs \
  's{                params!\[chunk_id, as_blob\(v\)\],\n            \)\?;\n            tx\.execute\(\n                "DELETE FROM chunk_embedding_state WHERE space_id = \?1 AND chunk_id = \?2",\n                params!\[space_id, chunk_id\],\n            \)\?;\n}{                params![chunk_id, as_blob(v)],\n            )?;\n}' \
  'params![chunk_id, as_blob(v)],
            )?;
            Ok(())' \
  mnema-index 'upserting_a_vector_clears_a_stale_bookkeeping_row' --test space

case_ "space: delete_vector without clearing chunk_embedding_state leaves a stale row (D95a, review 1)" \
  crates/mnema-index/src/space.rs \
  's{                &format!\("DELETE FROM \{\} WHERE chunk_id = \?1", space\.table\),\n                params!\[chunk_id\],\n            \)\?;\n            tx\.execute\(\n                "DELETE FROM chunk_embedding_state WHERE space_id = \?1 AND chunk_id = \?2",\n                params!\[space_id, chunk_id\],\n            \)\?;\n}{                &format!("DELETE FROM {} WHERE chunk_id = ?1", space.table),\n                params![chunk_id],\n            )?;\n}' \
  'params![chunk_id],
            )?;
            Ok(())
        })
    }' \
  mnema-index 'deleting_a_vector_also_clears_its_bookkeeping_row' --test space

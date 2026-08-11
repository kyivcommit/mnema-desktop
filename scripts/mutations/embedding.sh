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

# ── Task 4 (D96g): a confirmed model change retires the old space ─────────────
#
# The dangerous one first. `ExistingVectors` is read backwards, so a change
# nobody confirmed retires the space instead of being refused — which is data
# loss with a confirmation dialog somewhere else entirely, and the whole reason
# this parameter exists rather than the command simply dropping what blocks.
case_ "models: a model change nobody confirmed must not retire a space (D96g)" \
  src-tauri/src/models.rs \
  's{if existing_vectors == ExistingVectors::Discard\n                && !retired}{if existing_vectors == ExistingVectors::Keep\n                && !retired}' \
  '}) if existing_vectors == ExistingVectors::Keep
                && !retired.iter().any(|r| r.space_id == space_id) =>' \
  mnema-desktop 'changing_the_model_without_confirmation_leaves_the_space_alone' --test model_commands

# The other half: with the drop gone the loop records a retirement it did not
# perform, the next pass meets the same refusal, and the guard returns it — so a
# confirmed change fails instead of happening. Nothing about the refusal path
# changes, which is why this case names the confirmed test and the one above
# names the refused one.
case_ "models: a confirmed change that retires nothing cannot happen at all (D96g)" \
  src-tauri/src/models.rs \
  's{                db\.drop_space\(space_id\)\?;\n}{}' \
  '                && !retired.iter().any(|r| r.space_id == space_id) =>
            {
                retired.push(RetiredSpace {' \
  mnema-desktop 'a_confirmed_model_change_retires_the_old_space_and_its_tables' --test model_commands

# The shortcut the loop exists instead of, written out: on confirmation, drop
# `active_space` and then adopt. It reads as the obvious implementation and it
# destroys an archive for a press that changed nothing — re-adopting the
# recorded model moves nothing, and it is also the only path that rewrites
# `credential_ref`, so the same call is how a new API key is recorded.
case_ "models: confirmation retires what blocks, not whatever is active (D96g)" \
  src-tauri/src/models.rs \
  's{    let mut retired: Vec<RetiredSpace> = Vec::new\(\);\n}{    let mut retired: Vec<RetiredSpace> = Vec::new();\n    let _ = db\n        .active_space()?\n        .filter(|_| existing_vectors == ExistingVectors::Discard)\n        .map(|active| db.drop_space(active))\n        .transpose()?;\n}' \
  '.filter(|_| existing_vectors == ExistingVectors::Discard)
        .map(|active| db.drop_space(active))' \
  mnema-desktop 'a_confirmed_change_to_the_model_the_index_is_already_on_retires_nothing' --test model_commands

# The silent leak, from the other crate. The `embedding_space` row goes and the
# `vec0` table and its four shadow tables stay: nothing counts the space, nothing
# lists it, and its vectors sit on the disk of somebody who was told the old
# model had been retired.
#
# Two cases for one mutation, the pattern this file already uses above: `case_`
# names one test at a time, and this `DROP` is claimed by two — `mnema-index`'s
# own, which is where the mechanism lives, and the desktop one, which is where
# the promise is made to a person. Checked rather than assumed: `grep -rn
# drop_space scripts/mutations/` had no case for either before these, so the
# index crate's test was protecting the line by assertion alone.
case_ "space: a retired space whose vec0 table stays behind leaks the disk (D96g)" \
  crates/mnema-index/src/space.rs \
  's{        tx\.execute_batch\(&format!\("DROP TABLE IF EXISTS \{table\};"\)\)\?;\n}{        let _ = &table;\n}' \
  'let _ = &table;
        tx.execute(
            "DELETE FROM embedding_space WHERE id = ?1",' \
  mnema-desktop 'a_confirmed_model_change_retires_the_old_space_and_its_tables' --test model_commands

case_ "space: the same DROP, against the index crate's own test (D96g)" \
  crates/mnema-index/src/space.rs \
  's{        tx\.execute_batch\(&format!\("DROP TABLE IF EXISTS \{table\};"\)\)\?;\n}{        let _ = &table;\n}' \
  'let _ = &table;
        tx.execute(
            "DELETE FROM embedding_space WHERE id = ?1",' \
  mnema-index 'dropping_a_space_removes_its_row_its_table_and_its_shadows' --test space

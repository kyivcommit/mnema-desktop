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
  's{                if let Err\(e\) = db\.drop_space\(space_id\) \{\n                    return Err\(failure_after_retiring\(e, retired\)\);\n                \}\n}{}' \
  '                // nothing had gone.
                retired.push(RetiredSpace {' \
  mnema-desktop 'a_confirmed_model_change_retires_the_old_space_and_its_tables' --test model_commands

# The shortcut the loop exists instead of, written out: on confirmation, drop
# `active_space` and then adopt. It reads as the obvious implementation and it
# destroys an archive for a call the index promises will change nothing —
# `refuse_if_the_move_would_orphan_anything` (`crates/mnema-index/src/space.rs:594-602`)
# exempts a call whose destination is where the pointer already stands, because
# it is not a transition at all.
#
# An earlier version of this comment said the same call is how a new API key is
# recorded. False, and corrected in review: the key goes to the OS credential
# store through `set_key`, and `model_config.credential_ref` holds the NAME the
# credential is filed under, not the key.
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

# ── Task 4, fix round 1 (D96g) ───────────────────────────────────────────────
#
# The defect review found: the retirements are thrown away on the failing exit,
# so a confirmed change that destroyed a space and then could not finish reaches
# the window as "the embedding model was not recorded" — a sentence that reads
# as *nothing happened*, to somebody whose embeddings have just gone. Nothing
# afterwards can mention it either: the space is no longer a row in
# `embedding_space`, which is exactly why nothing notices.
case_ "models: a failure after a retirement must not lose the retirement (D96g, review 1)" \
  src-tauri/src/models.rs \
  's{    if retired\.is_empty\(\) \{\n        return Error::Index\(cause\);\n    \}\n    Error::RetiredThenFailed \{\n        retired,\n        source: cause,\n    \}\n}{    let _ = &retired;\n    Error::Index(cause)\n}' \
  'let _ = &retired;
    Error::Index(cause)
}' \
  mnema-desktop 'models::tests::a_failure_that_destroyed_something_first_says_so_and_one_that_did_not_does_not' --lib

# The mirror, and it is not a lesser fault: every ordinary refusal — a model
# change nobody confirmed, overwhelmingly — would tell the person a vector space
# was deleted. A one-sided assertion is satisfied by exactly this.
case_ "models: an ordinary refusal must not claim a deletion (D96g, review 1)" \
  src-tauri/src/models.rs \
  's{    if retired\.is_empty\(\) \{\n        return Error::Index\(cause\);\n    \}\n}{}' \
  'fn failure_after_retiring(cause: mnema_index::Error, retired: Vec<RetiredSpace>) -> Error {
    Error::RetiredThenFailed {' \
  mnema-desktop 'models::tests::a_failure_that_destroyed_something_first_says_so_and_one_that_did_not_does_not' --lib

# The number the window's confirmation stands on. Hardcoded, the settings say
# "one space" over an index holding two, and the button offers to delete a
# number that is not the whole bill — which is exactly review 1's Important 2
# reopened, from the side the window cannot check.
case_ "models: the number of spaces must be measured, not asserted (D96g, review 1)" \
  src-tauri/src/models.rs \
  's{            space_count: db\.space_count\(\)\?,\n}{            space_count: 1,\n}' \
  '            space_count: 1,
            embedded_chunks_everywhere:' \
  mnema-desktop 'the_settings_tell_the_active_space_apart_from_the_whole_index' --test model_commands

# The argument that must be the caller's. Made optional, a window that sends
# nothing is handed one of the two answers by a library rather than refused —
# and only one of the two can be undone.
case_ "models: the answer about existing vectors must be required (D96g, review 1)" \
  src-tauri/src/models.rs \
  's{    existing_vectors: ExistingVectors,\n\) -> Result<AdoptedModel, Error> \{\n    let key = key\(&state\)\?;\n}{    existing_vectors: Option<ExistingVectors>,\n) -> Result<AdoptedModel, Error> \{\n    let existing_vectors = existing_vectors.unwrap_or(ExistingVectors::Keep);\n    let key = key(&state)?;\n}' \
  'let existing_vectors = existing_vectors.unwrap_or(ExistingVectors::Keep);' \
  mnema-desktop 'a_model_change_that_says_nothing_about_the_existing_vectors_is_refused' --test commands

# The count `space_count` answers, in the crate that owns the SQL. Counting only
# non-empty spaces passes every desktop test — the second space in that fixture
# is empty — and re-opens the same gap from underneath.
case_ "space: the number of spaces counts the empty ones too (D96g, review 1)" \
  crates/mnema-index/src/space.rs \
  's{            \.query_row\("SELECT count\(\*\) FROM embedding_space", \[\], \|r\| r\.get\(0\)\)\?\)\n}{            .query_row(\n                "SELECT count(*) FROM embedding_space WHERE id IN (SELECT space_id FROM chunk_embedding_state)",\n                [],\n                |r| r.get(0),\n            )?)\n}' \
  'WHERE id IN (SELECT space_id FROM chunk_embedding_state)' \
  mnema-index 'the_number_of_spaces_counts_the_empty_ones_too' --test space

# ── Task 4, fix round 2 (D96g) ───────────────────────────────────────────────
#
# Review round 2. The confirmation's number must be the whole index's, not the
# active space's share of it: an abandoned space is left behind by every model
# change (`adopt_embedding_model` mints and repoints and never removes what it
# moved off), and the change retires every space in the way. Collapsed to the
# active space, the button offers to delete less than it will.
case_ "models: the confirmation's number must count the index, not the active space (D96g, review 2)" \
  src-tauri/src/models.rs \
  's{            embedded_chunks_everywhere: db\.embedded_chunks_everywhere\(\)\?,\n}{            embedded_chunks_everywhere: db.embedded_chunk_count(active_space.unwrap_or(0)).unwrap_or(0),\n}' \
  'embedded_chunks_everywhere: db.embedded_chunk_count(active_space.unwrap_or(0)).unwrap_or(0),' \
  mnema-desktop 'the_settings_tell_the_active_space_apart_from_the_whole_index' --test model_commands

# The sum, in the crate that owns it. Assignment rather than addition answers
# whatever the last space happened to hold — a number, plausible, and not the
# one somebody is about to spend.
case_ "space: the embeddings everywhere must be summed, not overwritten (D96g, review 2)" \
  crates/mnema-index/src/space.rs \
  's{            total \+= self\.embedded_chunk_count\(space_id\)\?;\n}{            total = self.embedded_chunk_count(space_id)?;\n}' \
  '            total = self.embedded_chunk_count(space_id)?;' \
  mnema-index 'the_embeddings_everywhere_are_summed_over_spaces_and_distinct_within_one' --test space

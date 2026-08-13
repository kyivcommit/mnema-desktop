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

# ── Task 5: `embed`, the batch call the indexing queue will call ──────────────
#
# Without the count check, a short answer is zipped against the texts anyway:
# every embedding after the missing one lands on the wrong chunk, confidently
# and permanently, with nothing in the index looking broken — the exact trap
# `embed_refuses_a_short_answer` exists to catch.
case_ "provider: embed without the count check silently misattributes a short answer" \
  crates/mnema-provider/src/probe.rs \
  's{    if parsed\.data\.len\(\) != texts\.len\(\) \{\n        return Err\(Error::CountMismatch \{\n            asked: texts\.len\(\),\n            got: parsed\.data\.len\(\),\n        \}\);\n    \}\n\n}{}' \
  '.map_err(|e| unreadable_embeddings_answer(&answer, key, &e))?;

    // Placeholders, not' \
  mnema-provider 'embed_refuses_a_short_answer' --test probe

# ── Task 5, fix round 1 ────────────────────────────────────────────────────
#
# Critical 1: `embed` used to bind vectors by their position in the response
# array, which `check_embedding_model` can get away with (it is
# order-insensitive by construction) and `embed` cannot — a reordered answer
# passes the count check exactly and binds every vector to the wrong chunk.
# The fix binds by the row's own stated position instead. This mutation
# reverts placement to array order while leaving the position VALIDATION
# intact (duplicates/gaps/missing are still caught) — the only test that can
# tell that apart is the one whose response is deliberately out of order.
case_ "provider: embed must bind by the row's stated position, not by array order (fix round 1, Critical 1)" \
  crates/mnema-provider/src/probe.rs \
  's{out\[index\] = row\.embedding;}{out[filled.iter().filter(|f| **f).count() - 1] = row.embedding;}' \
  'out[filled.iter().filter(|f| **f).count() - 1] = row.embedding;' \
  mnema-provider 'embed_returns_one_vector_per_text_in_order' --test probe

# Minor 4: the only mutation on the count check deleted the whole `if`, which
# reddens both count tests and cannot tell `!=` apart from the weaker `<`. A
# guard written as `<` would let a long answer (more vectors than texts)
# through silently — this is the mutation that actually exercises the
# asymmetry the second test (`embed_refuses_a_long_answer_too`) was written
# to hold, and it must leave `embed_refuses_a_short_answer` green.
case_ "provider: embed's count check must catch a long answer too, not only a short one (fix round 1, Minor 4)" \
  crates/mnema-provider/src/probe.rs \
  's{if parsed\.data\.len\(\) != texts\.len\(\) \{}{if parsed.data.len() < texts.len() \{}' \
  'if parsed.data.len() < texts.len() {' \
  mnema-provider 'embed_refuses_a_long_answer_too' --test probe

# ── Task 5, fix round 2 ────────────────────────────────────────────────────
#
# Important A: a bare `#[serde(default)] Option<usize>` only falls back on a
# *missing* key — a present `index` in the wrong shape (a string, a float, a
# negative number) fails the WHOLE embeddings body, which turns a perfectly
# usable model into `check_embedding_model`'s `Malformed` and makes it
# unconfigurable. `PositionState`'s own `Deserialize` swallows that shape
# into `Unreadable` instead of propagating the parse error, which is exactly
# the property this mutation removes: it reverts the impl to the
# `Option<usize>`-equivalent behaviour (same type name, so the crate still
# compiles) while every other line stays untouched. The regression is
# `check_embedding_model` becoming unable to read a body it never even looks
# at `index` in — the named test is the one written to catch exactly that.
# Measured (fix round 2 re-review, question 3): `embed`'s own
# `embed_refuses_a_position_stated_in_a_shape_it_cannot_read` reddens under
# this same mutation too — the body's parse now fails outright instead of
# yielding `PositionMismatch`, which that test does not expect — while the
# other eight `embed_*` tests in this file stay green. Two oracles, not one;
# the named test is kept as this case's oracle because it is the one that
# shows the actual regression (a model `check_embedding_model` should have
# accepted, refused) rather than an assertion mismatch one level away from it.
case_ "provider: PositionState must not fail the whole body on a wrong-shaped index (fix round 2, Important A)" \
  crates/mnema-provider/src/probe.rs \
  's{        Ok\(match Value::deserialize\(deserializer\)\? \{\n            Value::Null => PositionState::Absent,\n            Value::Number\(n\) => n\n                \.as_u64\(\)\n                \.and_then\(\|n\| usize::try_from\(n\)\.ok\(\)\)\n                \.map\(PositionState::Stated\)\n                \.unwrap_or\(PositionState::Unreadable\),\n            _ => PositionState::Unreadable,\n        \}\)}{        Ok(match Option::<usize>::deserialize(deserializer)? \{\n            Some(n) => PositionState::Stated(n),\n            None => PositionState::Absent,\n        \})}' \
  'Ok(match Option::<usize>::deserialize(deserializer)? {' \
  mnema-provider 'check_embedding_model_is_inert_to_a_position_stated_as_a_string' --test probe

# ---------------------------------------------------------------- task 6
#
# The embedding pass: the crate that writes vectors, and the failure it exists
# to prevent is a chunk that reports as handled and is not in the index.
#
# ⚠️ **These cases do not name every test in `crates/mnema-embed/tests/queue.rs`.**
# The comment here once claimed they did, which read as a completeness guarantee
# and was not one. Its replacement then miscounted twice in the same breath —
# while explaining that a number is a definition — so this version carries no
# count at all. Anyone who wants one can take it from the code rather than from
# a sentence that goes stale on the next commit:
#
#   diff <(grep -o "^fn [a-z_]*" crates/mnema-embed/tests/queue.rs | sed 's/fn //' | sort) \
#        <(grep -o "'[a-z_]*' --test queue" scripts/mutations/embedding.sh \
#          | tr -d "'" | sed 's/ --test queue//' | sort -u)
#
# Run, not guessed at: it prints the queue.rs tests no case here names.
#
# What is true without counting anything: each case below breaks one thing and
# names one test that must go red. Some tests are reddened only as a side effect
# of a case aimed at something else, and the ordinary-path ones — a run that
# resumes, a cancellation that keeps its work, an empty queue, a refused batch
# that condemns nothing, a tally that agrees with the index — are covered that
# way rather than directly. That is a judgement, not a guarantee, and anybody
# adding a case is welcome to take one of them.

# The invariant the whole task exists to establish. There is no "filter by the
# active space" to remove — the space is the active one by construction,
# because `run` takes no `space_id` and reads `active_space` itself — so the
# mutation stands in for the only defect that shape still admits: the pass
# holding some other space's id. The fixture builds the idle space first, so it
# is the one with the lower id.
case_ "embed: the pass must write into the active space and no other (§2.6 f)" \
  crates/mnema-embed/src/lib.rs \
  's{let space = db\.active_space\(\)\?\.ok_or\(Error::NoActiveSpace\)\?;}{let space = db.active_space()?.ok_or(Error::NoActiveSpace)? - 1;}' \
  'let space = db.active_space()?.ok_or(Error::NoActiveSpace)? - 1;' \
  mnema-embed 'the_pass_writes_only_into_the_active_space' --test queue

# `active_space`'s own doc forbids itself a fallback and names this caller as
# the one that would be tempted to add one here instead. "Use the only space
# there is" is what that would look like.
case_ "embed: no active space is a refusal, not a guess at which space to use" \
  crates/mnema-embed/src/lib.rs \
  's{let space = db\.active_space\(\)\?\.ok_or\(Error::NoActiveSpace\)\?;}{let space = db.active_space()?.unwrap_or(1);}' \
  'let space = db.active_space()?.unwrap_or(1);' \
  mnema-embed 'a_pass_with_no_active_space_refuses_rather_than_guessing' --test queue

# The silent-disappearance case, from both sides. Without the row, the chunk is
# offered again on every run for ever; without the count, the refusal never
# reaches the person who could act on it. Two cases, because one test asserts
# both and each half fails it alone.
# ⚠️ Reddens at `queue.rs`'s `.expect("run")`, not at the `failed_rows`
# assertion this case is named for: with no row written the chunk stays in the
# queue, the mock runs out, and the run returns Err. The detection is real —
# look at the panic line the harness prints, not at the assertion in the title.
case_ "embed: a refused chunk must leave a failed row behind" \
  crates/mnema-embed/src/lib.rs \
  's{                if call\n                    \.db\n                    \.record_embedding_failure\(call\.space, pending\[0\]\.id\)\?\n                \{\n                    tally\.failed \+= 1;\n                \}\n}{                tally.failed += 1;\n}' \
  'was called.
                tally.failed += 1;' \
  mnema-embed 'a_refused_chunk_is_counted_as_failed_not_merely_missing' --test queue

case_ "embed: a refused chunk must be counted, not merely recorded" \
  crates/mnema-embed/src/lib.rs \
  's{                if call\n                    \.db\n                    \.record_embedding_failure\(call\.space, pending\[0\]\.id\)\?\n                \{\n                    tally\.failed \+= 1;\n                \}\n}{                call.db.record_embedding_failure(call.space, pending[0].id)?;\n}' \
  'call.db.record_embedding_failure(call.space, pending[0].id)?;
                Ok(())' \
  mnema-embed 'a_refused_chunk_is_counted_as_failed_not_merely_missing' --test queue

# A refusal on one text must not end the run: the chunks after it are not at
# fault and would never be reached again, because the run stops at the same
# place on every restart.
case_ "embed: one refused text must not stop the pass reaching the rest" \
  crates/mnema-embed/src/lib.rs \
  's{            if !speaks_only_about_these_texts\(&refusal\) \{\n                return Err\(Error::Provider\(refusal\)\);\n            \}\n}{            return Err(Error::Provider(refusal));\n            #[allow(unreachable_code)]\n}' \
  'return Err(Error::Provider(refusal));
            #[allow(unreachable_code)]' \
  mnema-embed 'a_refusal_neither_undoes_what_came_before_nor_stops_what_comes_after' --test queue

# The batch that is refused for something that could be about its texts is
# split into single calls; condemning the batch's first chunk instead — or all
# of them — takes good chunks out of vector search permanently, because a failed
# row is never reconsidered until its text changes.
case_ "embed: a refused batch must be split, not blamed on the chunk that happened to be first" \
  crates/mnema-embed/src/lib.rs \
  's{            \} else \{\n                one_at_a_time\(call, pending, cancel, on_progress, tally\)\n            \}}{            \} else \{\n                if call.db.record_embedding_failure(call.space, pending[0].id)? \{\n                    tally.failed += 1;\n                \}\n                Ok(())\n            \}}' \
  '} else {
                if call.db.record_embedding_failure(call.space, pending[0].id)? {' \
  mnema-embed 'a_batch_refused_for_its_content_is_re_sent_one_text_at_a_time' --test queue

# A document that is `pending` is one `clear_document_content` has just emptied
# and a rebuild is about to refill: its chunks are minutes from being deleted,
# and embedding them spends the user's money on rows that will not exist. Two
# cases, one per test, since `case_` names one test at a time — and the second
# is not the same claim: `failed` and `skipped` are outside `'indexed'` too,
# and a filter written as "not pending" would pass the first test and fail it.
case_ "index: the queue must skip documents that are not indexed (D95 d)" \
  crates/mnema-index/src/space.rs \
  's{          WHERE d\.status = \x27indexed\x27\n            AND c\.id NOT IN}{          WHERE c.id NOT IN}' \
  'WHERE c.id NOT IN (SELECT chunk_id FROM {table})' \
  mnema-embed 'chunks_of_a_document_being_rebuilt_are_not_embedded' --test queue

case_ "index: the queue must skip failed and skipped documents too, not only pending" \
  crates/mnema-index/src/space.rs \
  's{          WHERE d\.status = \x27indexed\x27\n            AND c\.id NOT IN}{          WHERE c.id NOT IN}' \
  'WHERE c.id NOT IN (SELECT chunk_id FROM {table})' \
  mnema-embed 'only_indexed_documents_are_embedded' --test queue

# Without this the pass spins on the chunk the provider will not take: it is
# offered again on the next batch, and on the next run, for as long as the
# archive is open, against a provider that charges for the attempt.
# ⚠️ Reddens at `.expect("run")` rather than at the count assertion: the chunk
# is offered again, the mock has nothing left, and the `599` ends the run.
case_ "index: a chunk this space gave up on must leave the queue" \
  crates/mnema-index/src/space.rs \
  's{\n            AND NOT EXISTS \(\{GIVEN_UP_ON_CURRENT_TEXT\}\)}{}' \
  'AND c.id NOT IN (SELECT chunk_id FROM {table})"' \
  mnema-embed 'a_refused_chunk_is_not_offered_again_while_its_text_is_unchanged' --test queue

# The half of that rule which keeps it from being a trap. The hash is what makes
# a failed row a statement about *a text* rather than about a chunk id: without
# it, editing the file never brings the chunk back, and `failed_chunk_count`
# goes on reporting a refusal about text that no longer exists anywhere.
case_ "index: a failed row must stop applying once the chunk's text changes" \
  crates/mnema-index/src/space.rs \
  's{\n                    AND s\.state = 2 AND s\.content_hash = c\.content_hash";}{\n                    AND s.state = 2";}' \
  'AND s.state = 2";' \
  mnema-embed 'an_edited_chunk_leaves_the_failed_number_and_is_tried_again' --test queue

# vec0 accepts a vector of the wrong width without complaint at some call
# shapes, and the damage shows up only as a confident wrong answer at query
# time. `check_rankable` does not close this: it asks whether a vector can be
# ranked at all, not whether it matches this space's width.
case_ "embed: a vector of the wrong width must be refused before it is stored" \
  crates/mnema-embed/src/lib.rs \
  's{    for vector in vectors \{\n        if vector\.len\(\) as i64 != call\.width \{\n            return Err\(Error::WidthMismatch \{\n                expected: call\.width,\n                got: vector\.len\(\) as i64,\n            \}\);\n        \}\n    \}\n}{}' \
  '    }
    for (chunk, vector) in pending.iter().zip(vectors) {' \
  mnema-embed 'a_vector_of_the_wrong_width_is_refused_before_it_is_stored' --test queue

# "Before the write" is a claim about the batch, not about each vector, and a
# batch of one cannot tell the two apart. Checked as each vector is stored, a
# mismatch at position two leaves position one already written into a space its
# own answer has just been declared incompatible with.
case_ "embed: the whole batch's widths must be checked before any of it is stored" \
  crates/mnema-embed/src/lib.rs \
  's{    for vector in vectors \{\n        if vector\.len\(\) as i64 != call\.width \{\n            return Err\(Error::WidthMismatch \{\n                expected: call\.width,\n                got: vector\.len\(\) as i64,\n            \}\);\n        \}\n    \}\n    for \(chunk, vector\) in pending\.iter\(\)\.zip\(vectors\) \{\n}{    for (chunk, vector) in pending.iter().zip(vectors) \{\n        if vector.len() as i64 != call.width \{\n            return Err(Error::WidthMismatch \{\n                expected: call.width,\n                got: vector.len() as i64,\n            \});\n        \}\n}' \
  '    for (chunk, vector) in pending.iter().zip(vectors) {
        if vector.len() as i64 != call.width {' \
  mnema-embed 'a_batch_carrying_one_bad_width_stores_none_of_it' --test queue

# The last hop of a binding that is carried by position the whole way down.
# Nothing above notices it swapped: `mnema-provider` has already placed the rows
# by the index the provider stated, the widths all match, the count is right,
# and the index stores whatever it is handed. What comes out is an archive that
# answers every question with its neighbour's citation.
case_ "embed: each chunk must get the vector made from its own text" \
  crates/mnema-embed/src/lib.rs \
  's{for \(chunk, vector\) in pending\.iter\(\)\.zip\(vectors\)}{for (chunk, vector) in pending.iter().rev().zip(vectors)}' \
  'for (chunk, vector) in pending.iter().rev().zip(vectors) {' \
  mnema-embed 'each_chunk_gets_the_vector_that_was_made_from_its_own_text' --test queue

# The provider must see what a person will read in the citation. The prepared
# copy is the lexical branch's — apostrophes unified, ґ→г, camelCase expanded —
# and it exists in `chunk_search`, one join away, which is what makes this a
# mutation somebody could write by accident.
case_ "index: the queue must hand over the original text, not the prepared copy" \
  crates/mnema-index/src/space.rs \
  's{"SELECT c\.id, c\.text, c\.content_hash \{queue\} ORDER BY c\.id LIMIT \?2"}{"SELECT c.id, (SELECT s2.text FROM chunk_search s2 WHERE s2.chunk_id = c.id), c.content_hash \{queue\} ORDER BY c.id LIMIT ?2"}' \
  'SELECT c.id, (SELECT s2.text FROM chunk_search s2 WHERE s2.chunk_id = c.id), c.content_hash' \
  mnema-embed 'the_provider_is_sent_the_original_text_and_not_the_prepared_copy' --test queue

# Between batches has to include before the first one. Checked after the batch
# instead, a person who presses Start and then Stop in the same second has
# already paid for a batch of embeddings.
case_ "embed: cancellation must be asked before the first batch, not only after it" \
  crates/mnema-embed/src/lib.rs \
  's{        if cancel\(\) \{\n            break;\n        \}\n        let pending}{        let pending}; s{        outcome\?;\n    \}\n    Ok\(tally\)}{        outcome?;\n        if cancel() \{\n            break;\n        \}\n    \}\n    Ok(tally)}' \
  '        outcome?;
        if cancel() {' \
  mnema-embed 'cancelling_before_the_first_batch_asks_the_provider_nothing' --test queue

# `total` is the queue measured once, at the start. Re-read each batch it
# answers a shrinking number — the bar sits at the same fraction from the first
# report to the last and never moves.
case_ "embed: progress total must be measured once, not re-read every batch" \
  crates/mnema-embed/src/lib.rs \
  's{            done: tally\.embedded,\n            total,\n}{            done: tally.embedded,\n            total: db.queued_chunk_count(space)? as u64,\n}' \
  'total: db.queued_chunk_count(space)? as u64,' \
  mnema-embed 'progress_counts_against_the_queue_as_it_was_when_the_run_began' --test queue

# And it is the queue, not the archive: `chunk_count` is the plausible wrong
# answer, and it stops the bar short by exactly the number of chunks that were
# already embedded when the run began — which `job::progress_is_due` reads as a
# job that never finished.
case_ "embed: progress total must be the queue, not every chunk in the index" \
  crates/mnema-embed/src/lib.rs \
  's{let total = db\.queued_chunk_count\(space\)\? as u64;}{let total = db.chunk_count()? as u64;}' \
  'let total = db.chunk_count()? as u64;' \
  mnema-embed 'progress_counts_against_the_queue_as_it_was_when_the_run_began' --test queue

# `upsert_vector` clears the chunk's `chunk_embedding_state` row in the same
# transaction as the write. With `insert_vector` a chunk that was refused once
# and has since been embedded goes on being counted among the failures — a
# number on the settings screen that nothing will ever clear.
case_ "index: storing a vector must clear the row that gave up on the chunk (D95a)" \
  crates/mnema-index/src/space.rs \
  's{            tx\.execute\(\n                "DELETE FROM chunk_embedding_state WHERE space_id = \?1 AND chunk_id = \?2",\n                params!\[space_id, chunk_id\],\n            \)\?;\n            Ok\(true\)}{            Ok(true)}' \
  '            Ok(true)
        })
    }' \
  mnema-embed 'an_edited_chunk_leaves_the_failed_number_and_is_tried_again' --test queue

# A batch of nothing asks the database for zero chunks and gets zero back, so
# the pass reports a finished archive it never touched.
case_ "embed: a batch size of zero must be refused rather than reported as done" \
  crates/mnema-embed/src/lib.rs \
  's{    if batch == 0 \{\n        return Err\(Error::EmptyBatch\);\n    \}\n}{}' \
  ') -> Result<EmbedTally, Error> {
    let space = db.active_space()' \
  mnema-embed 'a_batch_size_of_zero_is_refused_rather_than_looped_on' --test queue

# The model is the space's own and cannot be a parameter, because a space built
# for one model filled with another's vectors ranks nonsense and says nothing.
case_ "embed: the request must name the model the space was built for" \
  crates/mnema-embed/src/lib.rs \
  's{mnema_provider::embed\(call\.base, call\.key, call\.model, &texts\)}{mnema_provider::embed(call.base, call.key, "some/other-embedder", \&texts)}' \
  'mnema_provider::embed(call.base, call.key, "some/other-embedder", &texts)' \
  mnema-embed 'the_request_names_the_model_the_space_was_built_for' --test queue

# `batch` has to reach the query. Ignored, the whole archive goes into one
# request — nine thousand chunks in one body, past every provider's limit, and
# the run then fails on the first call it makes.
case_ "embed: the batch size must reach the query that fills the batch" \
  crates/mnema-embed/src/lib.rs \
  's{db\.chunks_needing_embedding\(space, batch\)\?}{db.chunks_needing_embedding(space, usize::MAX)?}' \
  'db.chunks_needing_embedding(space, usize::MAX)?' \
  mnema-embed 'chunks_go_out_in_batches_of_the_size_asked_for' --test queue

# A vector the index will not rank — non-finite components, or a norm that
# underflows to something vec0 ranks as NULL or -inf — arrives as an *index*
# error on a request that succeeded in every other way. Propagated, it stops the
# run at the same chunk on every restart, silently, and this was the second
# permanent stall the pass could reach. Inverting the id comparison is the
# smallest change that makes it propagate again.
case_ "embed: a vector the index refuses must fail its chunk, not the whole run" \
  crates/mnema-embed/src/lib.rs \
  's{\} if \*id == chunk_id}{\} if *id != chunk_id}' \
  '} if *id != chunk_id' \
  mnema-embed 'a_vector_the_index_will_not_rank_fails_its_chunk_and_not_the_run' --test queue

# And the direction that costs more if it is wrong. Widened to every index
# error, a database that will not take a write for a second turns a whole batch
# of good chunks into `failed` rows — and a `failed` row is not reconsidered
# until its chunk's text changes, so those chunks leave vector search for good.
case_ "embed: only a verdict on the vector may become a failed row, not any index error" \
  crates/mnema-embed/src/lib.rs \
  's{fn refuses_this_chunks_vector\(error: &mnema_index::Error, chunk_id: i64\) -> bool \{\n    matches!\(\n        error,\n        mnema_index::Error::NonFiniteVector \{\n            role: VectorRole::Stored\(id\),\n            \.\.\n        \} \| mnema_index::Error::UnrankableVector \{\n            role: VectorRole::Stored\(id\),\n            \.\.\n        \} if \*id == chunk_id\n    \)\n\}}{fn refuses_this_chunks_vector(error: \&mnema_index::Error, chunk_id: i64) -> bool \{\n    let _ = (error, chunk_id, VectorRole::Query);\n    true\n\}}' \
  'let _ = (error, chunk_id, VectorRole::Query);
    true
}' \
  mnema-embed 'an_index_error_that_is_not_about_the_vector_still_stops_the_run' --test queue

# ── Task 6, the attribution rule ─────────────────────────────────────────────
#
# The default arm. Every failure not on the list stops the run; turned round, a
# rate limit — the provider saying "later" in as many words — becomes a `failed`
# row, which means "never", about a chunk there is nothing wrong with. This is
# the case that keeps the safety from being a convention: with `_ => true` a
# variant nobody classified is attributed to whatever chunk was in flight.
case_ "embed: an unclassified provider failure must stop the run, not condemn a chunk" \
  crates/mnema-embed/src/lib.rs \
  's{        _ => false,\n    \}\n\}}{        _ => true,\n    \}\n\}}' \
  '        _ => true,
    }
}' \
  mnema-embed 'rate_limiting_stops_the_run_instead_of_condemning_the_chunk' --test queue

# The same rule from the batch side, and it needs its own mutation rather than
# the one above: `_ => true` cannot reach a `Provider { status }` at all, because
# the `Provider` arm matches first — measured, the case written that way stayed
# green. Widening that arm to the 5xx range is what a provider outage would need
# to be attributed, and then the batch is split into four more calls that each
# fail alone and are each written down as a chunk that cannot be embedded.
case_ "embed: a 5xx must not be attributed, and must not send the batch round one at a time" \
  crates/mnema-embed/src/lib.rs \
  's{matches!\(status, 400 \| 413 \| 422\)}{matches!(status, 400..=599)}' \
  'matches!(status, 400..=599)' \
  mnema-embed 'a_batch_refused_for_something_that_is_not_about_the_texts_is_not_split' --test queue

# `402 Payment Required` is what this provider answers for an exhausted account,
# and it is 4xx. The rule as first drafted was "any 4xx may be attributed", and
# this is the mutation that restores it: the moment a person's credit runs out
# mid-run, every chunk in the batch in flight is written down as impossible to
# embed, permanently, and the third number reports a failure that was never
# about them.
case_ "embed: the attributable statuses are three, not every 4xx (402 is an empty account)" \
  crates/mnema-embed/src/lib.rs \
  's{matches!\(status, 400 \| 413 \| 422\)}{matches!(status, 400..=499)}' \
  'matches!(status, 400..=499)' \
  mnema-embed 'an_exhausted_account_stops_the_run_and_condemns_nothing' --test queue

# Stop has to be heard inside the split too. A batch is one round trip; split it
# is as many as the batch is wide, and a person who pressed Stop would otherwise
# wait out every one of them and pay for them.
case_ "embed: cancellation must be heard between the single calls of a split" \
  crates/mnema-embed/src/lib.rs \
  's{    for chunk in pending \{\n        if cancel\(\) \{\n            cancelled = true;\n            break;\n        \}\n}{    for chunk in pending \{\n        let _ = &cancelled;\n}' \
  '    for chunk in pending {
        let _ = &cancelled;
        let texts = [chunk.text.clone()];' \
  mnema-embed 'cancelling_during_a_split_stops_it_between_the_single_calls' --test queue

# Inside the split, a single call that fails for something that is not about its
# text must still stop the run rather than condemn that chunk — the same rule as
# at the batch level, and a separate line of code, so a separate case.
case_ "embed: inside a split, an unclassified failure must stop the run too" \
  crates/mnema-embed/src/lib.rs \
  's{                if !speaks_only_about_these_texts\(&refusal\) \{\n                    return Err\(Error::Provider\(refusal\)\);\n                \}\n                condemned\.push\(chunk\.id\);}{                condemned.push(chunk.id);}' \
  'Err(refusal) => {
                condemned.push(chunk.id);' \
  mnema-embed 'a_split_that_meets_an_unclassified_failure_stops_and_condemns_nothing' --test queue

# ── Task 6, the corroboration rule ───────────────────────────────────────────
#
# A refusal is evidence about a text only if the same provider, in the same run,
# has answered some other text correctly. Without the guard, the first chunk of
# an archive is condemned on the first thing this provider says today — and then
# the next, and the next, to the end of the archive, with `run` reporting Ok.
case_ "embed: the first chunk of a run must not be condemned before anything has embedded" \
  crates/mnema-embed/src/lib.rs \
  's{                if tally\.embedded == 0 \{\n                    return Err\(Error::Provider\(refusal\)\);\n                \}\n}{}' \
  'anything will look at it again.
                // `content_hash` is read from the chunk inside' \
  mnema-embed 'the_first_chunk_of_a_run_is_not_condemned_on_no_evidence' --test queue

# The same rule where the corroboration is the split's own successes. `<` rather
# than a deleted block: the guard still exists, still compiles, and can simply
# never fire — which is what an "it is only a tidy-up" edit to it would look
# like.
case_ "embed: a split that succeeded at nothing must condemn nothing" \
  crates/mnema-embed/src/lib.rs \
  's{    if tally\.embedded == embedded_before \{}{    if tally.embedded < embedded_before \{}' \
  '    if tally.embedded < embedded_before {' \
  mnema-embed 'a_split_in_which_nothing_succeeded_condemns_nothing' --test queue

# And the boundary from the other side: with the held rows never written at all,
# a split that DID corroborate its refusals reports nothing failed, and the
# chunk the provider will not take goes back to being invisible — the silence
# this cycle exists to remove.
# ⚠️ Reddens at `.expect("run")` rather than at the `(embedded, failed)`
# assertion: with the held rows never written, the chunks come back on the next
# pass of the queue and the mock is exhausted.
case_ "embed: a corroborated split must still write the rows it held" \
  crates/mnema-embed/src/lib.rs \
  's{    for chunk_id in condemned \{\n        if call\.db\.record_embedding_failure\(call\.space, chunk_id\)\? \{\n            tally\.failed \+= 1;\n        \}\n    \}\n}{    let _ = &condemned;\n}' \
  '    let _ = &condemned;
    Ok(())' \
  mnema-embed 'one_success_in_a_split_corroborates_the_refusals_beside_it' --test queue

# ── Task 6, fix round 1 ──────────────────────────────────────────────────────
#
# M2: the filter that makes the queue *the* queue had no case of its own. This
# is the whole of "not done yet" — remove it and every chunk is offered again on
# every pass, for ever, against a paid provider.
case_ "index: the queue must exclude chunks that already have a vector" \
  crates/mnema-index/src/space.rs \
  's{\n            AND c\.id NOT IN \(SELECT chunk_id FROM \{table\}\)}{}' \
  'JOIN document d ON d.id = c.document_id
          WHERE d.status' \
  mnema-embed 'the_queue_is_the_chunks_with_no_vector' --test queue

# I1: chunk ids are reused, so a vector written by id alone can land on a chunk
# that was rebuilt while the request was in flight — search then answers with a
# citation quoting text the file no longer contains. The comparison and the
# write must be one transaction; this mutation keeps the write and drops the
# comparison, which is exactly what `upsert_vector` on its own does.
case_ "index: a vector must be written only onto the text it was made from (I1)" \
  crates/mnema-index/src/space.rs \
  's{            if still_this_text\.is_none\(\) \{\n                return Ok\(false\);\n            \}\n}{}' \
  'let still_this_text: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM chunk WHERE id = ?1 AND content_hash = ?2",
                    params![chunk_id, content_hash],
                    |r| r.get(0),
                )
                .optional()?;
            tx.execute(' \
  mnema-index 'a_vector_is_written_only_onto_the_text_it_was_made_from' --test space

# The same guard, from the other side: a chunk that has gone entirely. Separate
# test, separate case, because a comparison against a row that does not exist is
# a different SQL outcome from one against a row whose hash moved.
case_ "index: a vector for a chunk that has gone must not be written (I1)" \
  crates/mnema-index/src/space.rs \
  's{            if still_this_text\.is_none\(\) \{\n                return Ok\(false\);\n            \}\n}{}' \
  'let still_this_text: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM chunk WHERE id = ?1 AND content_hash = ?2",
                    params![chunk_id, content_hash],
                    |r| r.get(0),
                )
                .optional()?;
            tx.execute(' \
  mnema-index 'a_vector_for_a_chunk_that_has_gone_is_not_written' --test space

# M5: `record_embedding_failure` writes nothing when the chunk has gone. Saying
# it wrote makes the tally and `failed_chunk_count` disagree — the third number
# lying about itself, and that number is the whole safety argument for letting a
# chunk leave the queue.
case_ "index: recording a failure must report whether it actually wrote a row (M5)" \
  crates/mnema-index/src/space.rs \
  's{        Ok\(written > 0\)}{        let _ = written;\n        Ok(true)}' \
  '        let _ = written;
        Ok(true)' \
  mnema-index 'recording_a_failure_says_whether_it_wrote_one' --test space

# I2: a split is up to `batch` network round trips and reported nothing until it
# finished, so the bar froze exactly where the work is longest.
case_ "embed: a split must report progress as it goes, not once at the end (I2)" \
  crates/mnema-embed/src/lib.rs \
  's{        on_progress\(EmbedProgress \{\n            done: tally\.embedded,\n            total: call\.total,\n            failed: tally\.failed,\n        \}\);\n    \}\n    if tally\.embedded == embedded_before \{}{    \}\n    if tally.embedded == embedded_before \{}' \
  '        // read, so this reports what the database holds either way.
    }
    if tally.embedded == embedded_before {' \
  mnema-embed 'progress_moves_inside_a_split_not_only_between_batches' --test queue

# I3: the run must report the true counts before returning an error. The vectors
# from the failing batch are already in the index, so a bare `return` leaves the
# shell's last number short by up to a batch of embeddings that really are there.
case_ "embed: an aborting run must report its true counts before returning (I3)" \
  crates/mnema-embed/src/lib.rs \
  's{        let outcome = one_batch\(&call, &pending, cancel, on_progress, &mut tally\);}{        one_batch(&call, \&pending, cancel, on_progress, \&mut tally)?;\n        let outcome: Result<(), Error> = Ok(());}' \
  'one_batch(&call, &pending, cancel, on_progress, &mut tally)?;
        let outcome: Result<(), Error> = Ok(());' \
  mnema-embed 'the_last_number_is_true_even_when_the_run_stops' --test queue

# ── Task 6, fix round 2 ──────────────────────────────────────────────────────
#
# N1. `mnema-index` proves the checked write behaves; this proves the pass calls
# it. Measured before the test existed: with `store` reverted this way, every
# test in the crate stayed green and the harness said STILL GREEN — the guard
# against a vector landing on a rebuilt chunk was standing on nothing.
#
# `.map(|()| true)` keeps the match arms below it type-correct, so the mutation
# is the one line it claims to be rather than a compile failure wearing the
# same label.
case_ "embed: the pass must use the checked write, not upsert_vector (N1)" \
  crates/mnema-embed/src/lib.rs \
  's{        match call\n            \.db\n            \.upsert_vector_for_text\(call\.space, chunk\.id, &chunk\.content_hash, vector\)\n        \{}{        match call.db.upsert_vector(call.space, chunk.id, vector).map(|()| true) \{}' \
  'match call.db.upsert_vector(call.space, chunk.id, vector).map(|()| true) {' \
  mnema-embed 'a_vector_is_not_written_onto_a_chunk_rebuilt_while_the_request_was_in_flight' --test queue

# ── Task 7 (D95b): a space stops being forever `building` ─────────────────────
#
# Two writers in `mnema-index`, and `run`'s two calls into them. Fix round 1
# added a third method, `space_is_complete`, and collapsed the two-predicate
# `ready` condition (queue empty ∧ `failed_chunk_count == 0`, two different
# sets of chunks) into one call to it — see that method's own doc for why.

# Without this, `mark_space_ready` writes the value its sibling writes instead
# of its own — the two functions differ by one word each, and nothing but the
# test tells them apart.
#
# The marker carries the swapped value itself, inside `mark_space_ready`'s own
# body, not only the function name (M1, review round 1: the function-name-only
# marker used before this existed in the file, in the sibling function, even
# before the mutation ran, so the second guard could not tell a mutation that
# landed here from one that landed there — this one can).
case_ "space: mark_space_ready must write 'ready', not 'building' (D95b)" \
  crates/mnema-index/src/space.rs \
  's{"UPDATE embedding_space SET state = \x27ready\x27 WHERE id = \?1"}{"UPDATE embedding_space SET state = \x27building\x27 WHERE id = ?1"}' \
  'pub fn mark_space_ready(&self, space_id: i64) -> Result<(), Error> {
        let changed = self.conn().execute(
            "UPDATE embedding_space SET state = '"'"'building'"'"' WHERE id = ?1",' \
  mnema-index 'a_space_moves_between_building_and_ready' --test space

# The same swap from the other side, same reason for the stronger marker.
# Without it, `mark_space_building` cannot retract a `ready` claim at all — it
# would only ever restate one.
case_ "space: mark_space_building must write 'building', not 'ready' (D95b)" \
  crates/mnema-index/src/space.rs \
  's{"UPDATE embedding_space SET state = \x27building\x27 WHERE id = \?1"}{"UPDATE embedding_space SET state = \x27ready\x27 WHERE id = ?1"}' \
  'pub fn mark_space_building(&self, space_id: i64) -> Result<(), Error> {
        let changed = self.conn().execute(
            "UPDATE embedding_space SET state = '"'"'ready'"'"' WHERE id = ?1",' \
  mnema-index 'a_space_moves_between_building_and_ready' --test space

# ── Fix round 1 (D95b, I1/I2): `ready` collapsed into `space_is_complete` ──────
#
# Replaces the two cases that targeted the old `failed_chunk_count(space)? ==
# 0` condition — that code is gone, deleted along with the commit that removed
# it, so those cases would no longer apply against anything.

# The outer `NOT`, flipped. Without it a full space reads exactly as an empty
# one: `EXISTS` is true whenever a vector-less chunk exists, so a complete
# space — none do — answers `false` where `space_is_complete` must answer
# `true`.
case_ "space: space_is_complete's NOT EXISTS must stay NOT, not EXISTS (D95b fix 1)" \
  crates/mnema-index/src/space.rs \
  's{"SELECT NOT EXISTS \(}{"SELECT EXISTS (}' \
  'SELECT EXISTS (
                     SELECT 1 FROM chunk c' \
  mnema-embed 'a_space_becomes_ready_when_the_queue_empties' --test queue

# The finding itself. Without the missing document-status filter staying
# missing, this predicate goes back to covering only chunks behind an
# `indexed` document — exactly the queue's narrower scope, and exactly what
# let a document written but not yet `indexed` (the window
# `crates/mnema-ingest/src/lib.rs:546-598` names) read as `ready` with zero
# vectors. The marker anchors on the join text appearing *inside this
# function's own query*, not merely anywhere in the file — `the_embedding_queue`
# already has a join of its own, at different indentation, so a marker of the
# join alone would have existed before this mutation too.
case_ "space: space_is_complete must not filter by document status — I1 (D95b fix 1)" \
  crates/mnema-index/src/space.rs \
  's{                     SELECT 1 FROM chunk c\n                      WHERE c\.id NOT IN}{                     SELECT 1 FROM chunk c\n                      JOIN document d ON d.id = c.document_id\n                      WHERE d.status = \x27indexed\x27 AND c.id NOT IN}' \
  'SELECT 1 FROM chunk c
                      JOIN document d ON d.id = c.document_id
                      WHERE d.status' \
  mnema-embed 'a_space_with_chunks_behind_an_unindexed_document_does_not_become_ready' --test queue

# The same mutation, the mirror test — I2, the direction fix round 1 keeps
# rather than closes. A failed chunk behind a document that has since left
# `indexed` would become invisible to a scope-narrowed check exactly the way
# it is invisible to the queue, and the space would wrongly read `ready`.
case_ "space: the document-status filter would also hide a failed chunk — I2 (D95b fix 1)" \
  crates/mnema-index/src/space.rs \
  's{                     SELECT 1 FROM chunk c\n                      WHERE c\.id NOT IN}{                     SELECT 1 FROM chunk c\n                      JOIN document d ON d.id = c.document_id\n                      WHERE d.status = \x27indexed\x27 AND c.id NOT IN}' \
  'SELECT 1 FROM chunk c
                      JOIN document d ON d.id = c.document_id
                      WHERE d.status' \
  mnema-embed 'a_space_with_a_failed_chunk_behind_a_document_that_left_indexed_stays_building' --test queue

# The guard at the call site, not the predicate itself. Without it, every run
# that empties the queue marks `ready`, whatever `space_is_complete` would
# have answered — the exact confusion between "the pass finished" and "the
# space is complete" the state exists to keep apart.
case_ "embed: run must ask space_is_complete before marking a space ready (D95b fix 1)" \
  crates/mnema-embed/src/lib.rs \
  's{            if db\.space_is_complete\(space\)\? \{\n                db\.mark_space_ready\(space\)\?;\n            \}\n}{            db.mark_space_ready(space)?;\n}' \
  'db.mark_space_ready(space)?;
            break;' \
  mnema-embed 'a_space_with_failures_does_not_become_ready' --test queue

# Without the call at all, a fully embedded archive goes on reading exactly as
# it did the moment `create_space` wrote its first row — the whole defect this
# task exists to close.
#
# ⚠️ M1 (review round 1): a pure deletion, and no code-only marker can
# distinguish it from the file's unmutated state — the leftover `break;` and
# the `one_batch` call after it already sit there before the mutation runs.
# `git diff --quiet` is the guard doing the real work on this case; the
# second-line check is not credited with more than that.
case_ "embed: run must mark a space ready once its queue empties clean (D95b)" \
  crates/mnema-embed/src/lib.rs \
  's{            if db\.space_is_complete\(space\)\? \{\n                db\.mark_space_ready\(space\)\?;\n            \}\n}{}' \
  'break;
        }
        let outcome = one_batch(&call, &pending, cancel, on_progress, &mut tally);' \
  mnema-embed 'a_space_becomes_ready_when_the_queue_empties' --test queue

# Without this, a `ready` space never hears that new chunks arrived — the
# back-transition this task adds specifically because a one-way state would be
# the same lie the space started in, just arriving later and more
# convincingly.
#
# Regex updated for fix round 2: the condition this deletes moved from
# `total > 0` to `!db.space_is_complete(space)?` — see the case below for what
# that move was for.
#
# ⚠️ M1 (review round 1): also a pure deletion, same limitation as the case
# above — `let call = Call { ... }` is unmutated code that exists either way,
# so only `git diff --quiet` distinguishes a landed mutation from a no-op here.
case_ "embed: run must retract a ready claim when it starts against a non-empty queue (D95b)" \
  crates/mnema-embed/src/lib.rs \
  's{    if !db\.space_is_complete\(space\)\? \{\n        db\.mark_space_building\(space\)\?;\n    \}\n}{}' \
  'let call = Call {
        db,
        space,
        width,
        total,' \
  mnema-embed 'a_ready_space_goes_back_to_building_when_new_chunks_arrive' --test queue

# Fix round 2's own finding, reverted: without `space_is_complete` governing
# the retraction too, a chunk behind a document not yet `indexed` can never
# get a space out of a stale `ready` claim, because the queue never sees that
# chunk on any later run either — nothing would ever ask again. The marker is
# the reintroduced `total > 0` itself: this exact text existed nowhere in the
# file after fix round 2 landed, so its presence here can only mean the
# mutation.
case_ "embed: run must retract ready using space_is_complete, not the queue's total (D95b fix 2)" \
  crates/mnema-embed/src/lib.rs \
  's{    if !db\.space_is_complete\(space\)\? \{\n        db\.mark_space_building\(space\)\?;\n    \}\n}{    if total > 0 {\n        db.mark_space_building(space)?;\n    }\n}' \
  'if total > 0 {
        db.mark_space_building(space)?;' \
  mnema-embed 'a_ready_space_is_retracted_by_chunks_behind_an_unindexed_document_too' --test queue

# The property the controller named mandatory and forbade a carve-out for: a
# chunk this space gave up on has no vector, and `space_is_complete` must not
# look away from that. Reintroducing the queue's own exclusion
# (`GIVEN_UP_ON_CURRENT_TEXT`) here is the literal shape of the mistake —
# done properly this time, with the space id bound, so the mutation is a
# valid query that answers wrong rather than a broken one that would redden
# at a bind-parameter error instead of the assertion it names.
case_ "space: space_is_complete must not carve out chunks this space gave up on (D95b fix 2)" \
  crates/mnema-index/src/space.rs \
  's{"SELECT NOT EXISTS \(\n                     SELECT 1 FROM chunk c\n                      WHERE c\.id NOT IN \(SELECT chunk_id FROM \{table\}\)\n                 \)"\n            \),\n            \[\],}{"SELECT NOT EXISTS (\n                     SELECT 1 FROM chunk c\n                      WHERE c.id NOT IN (SELECT chunk_id FROM \{table\})\n                        AND NOT EXISTS (\{GIVEN_UP_ON_CURRENT_TEXT\})\n                 )"\n            ),\n            params![space_id],}' \
  'WHERE c.id NOT IN (SELECT chunk_id FROM {table})
                        AND NOT EXISTS ({GIVEN_UP_ON_CURRENT_TEXT})' \
  mnema-embed 'a_space_with_failures_does_not_become_ready' --test queue

# ── Task 8: the job in the shell, the command, and the third number ───────────
#
# ⚠️ **Nothing here can reach `ui/`.** `mutation-check.sh` runs `cargo test` and
# nothing else, so the window's half of this task — the settings line, the
# embedding job's own ending sentences, the two scopes those sentences keep
# apart — is held by `ui/render.test.js` and by review. The same limit this file
# already states twice; it is restated here because this task is the first one
# whose load-bearing requirement is a sentence on a screen.

# The inherited defect, reverted. `done == total` never fires for a run that
# ends with refusals — `mnema_embed::EmbedProgress`'s own doc says so — so the
# unthrottled last report goes missing on exactly the runs whose numbers matter
# most, and the bar is left wherever the timer last put it. The marker is the
# reintroduced `done == total`: that text exists nowhere in the file otherwise.
case_ "job: the last report must be forced by a resolved unit, not by done == total (T8)" \
  src-tauri/src/job.rs \
  's{    done \+ refused >= total \|\| last\.is_none_or}{    done == total || last.is_none_or}' \
  '    done == total || last.is_none_or(|last| now.duration_since(last) >= interval)' \
  mnema-desktop 'job::tests::the_report_that_resolves_the_last_unit_is_due_even_when_some_were_refused' --lib

# The third number, dropped at the seam it crosses. Without it a run that gave
# up on chunks and a run that finished cleanly reach the window as the same
# message — which is the shape `WalkReport::removed` was already lost in once,
# at this exact kind of seam.
case_ "embed_job: refusals must cross into the ending, not be zeroed there (T8)" \
  src-tauri/src/embed_job.rs \
  's{        skipped: 0,\n        refused: tally\.failed,}{        skipped: 0,\n        refused: 0,}' \
  '        skipped: 0,
        refused: 0,
        complete: true,' \
  mnema-desktop 'embed_job::tests::refusals_cross_the_seam_separately_from_what_was_embedded' --lib

# The same mutation, the test that reaches it through the command rather than
# through the translation function — a run against a real index and a provider
# that refuses one chunk. One case per test, since `case_` names one at a time.
case_ "embed_job: refusals must cross into the ending — through the command (T8)" \
  src-tauri/src/embed_job.rs \
  's{        skipped: 0,\n        refused: tally\.failed,}{        skipped: 0,\n        refused: 0,}' \
  '        skipped: 0,
        refused: 0,
        complete: true,' \
  mnema-desktop 'a_chunk_the_provider_refused_is_counted_where_a_person_can_read_it' --test model_commands

# The live half. A report that carries `done` and not the refusals leaves the
# bar climbing towards a total it will never reach, with nothing saying why.
case_ "embed_job: a live report must carry the refusals as well as the count (T8)" \
  src-tauri/src/embed_job.rs \
  's{        skipped: 0,\n        refused: progress\.failed,}{        skipped: 0,\n        refused: 0,}' \
  '        skipped: 0,
        refused: 0,
        seconds_left:' \
  mnema-desktop 'embed_job::tests::a_report_carries_the_refusals_as_well_as_the_count' --lib

# The failure path's own copy. The rows are already written when the pass gives
# up — `mnema_embed::run` records a refusal before it propagates anything — so
# reporting zero here takes a number off the screen that the database still
# holds.
case_ "embed_job: a failed run must still say how many were refused (T8)" \
  src-tauri/src/embed_job.rs \
  's{    Ended \{\n        refused,\n        \.\.Ended::failed\(done, total, message\)\n    \}}{    let _ = refused;\n    Ended::failed(done, total, message)}' \
  '    let _ = refused;
    Ended::failed(done, total, message)' \
  mnema-desktop 'embed_job::tests::a_failed_run_still_says_how_many_were_refused' --lib

# A run somebody stopped, reported as one that finished. `mnema_embed::run`
# answers `Ok` to both, so the flag is the only witness there is — and the
# direction of this mistake is the bad one: "finished" told to somebody who
# pressed Stop is a claim that the archive is fully embedded.
case_ "embed_job: a stopped run must not be reported as a finished one (T8)" \
  src-tauri/src/embed_job.rs \
  's{        reason: if cancelled \{\n            EndReason::Cancelled\n        \} else \{\n            EndReason::Completed\n        \},}{        reason: EndReason::Completed,}' \
  '        reason: EndReason::Completed,
        done: tally.embedded,' \
  mnema-desktop 'embed_job::tests::a_stopped_run_is_not_reported_as_a_finished_one' --lib

# The number the whole task exists to put in front of a person, removed at the
# last seam before the window. `Db::failed_chunk_count` goes back to having no
# production caller, and with it the justification for a chunk being allowed to
# leave the embedding queue for good.
case_ "models: the settings must read the refusal count, not report zero (T8)" \
  src-tauri/src/models.rs \
  's{            failed_chunks: match active_space \{\n                Some\(id\) => db\.failed_chunk_count\(id\)\?,\n                None => 0,\n            \},}{            failed_chunks: 0,}' \
  '            failed_chunks: 0,
            space_count: db.space_count()?,' \
  mnema-desktop 'a_chunk_the_provider_refused_is_counted_where_a_person_can_read_it' --test model_commands

# The ordering `start_walk_job` argues for and this command inherits: every
# fallible step before the slot is claimed. Reading the credential store can put
# a modal authorisation dialog on screen, so claiming first disables Start and
# refuses a walk for as long as somebody leaves it unanswered — for a call that
# then fails with `NoKey` anyway.
case_ "embed_job: the key must be read before the job slot is claimed (T8)" \
  src-tauri/src/embed_job.rs \
  's{    let key = crate::models::key\(&state\)\?;\n    let base = state\.provider_base\(\)\.to_string\(\);\n\n    let slot = state\.claim_job\(\)\?;}{    let slot = state.claim_job()?;\n\n    let key = crate::models::key(\&state)?;\n    let base = state.provider_base().to_string();}' \
  '    let slot = state.claim_job()?;

    let key = crate::models::key(&state)?;' \
  mnema-desktop 'the_key_is_read_before_the_job_slot_is_taken' --test model_commands

# The claim `set_embedding_model` takes although it is not a job. Without it a
# model change lands mid-run: `mnema_embed::run` holds the space id it read at
# the start, so with `Keep` over a still-empty space the pointer moves and every
# vector the run pays for goes into a space nothing points at — silently, with
# the settings screen reading 0 while the run's own line climbs.
#
# The named test uses a **probe** as the running job, which writes nothing: no
# space becomes non-empty, so `SpaceNotEmpty` — the neighbouring defence that
# covers the other timing — cannot stand in for the claim, and the assertion
# that fires is the one about the pointer having moved
# (`left: Some(2), right: Some(1)`), not the one about which error came back.
case_ "models: a model change must not be possible while a job holds the slot (T8)" \
  src-tauri/src/models.rs \
  's{    let _slot = state\.claim_job\(\)\?;}{    let _slot = ();}' \
  '    let _slot = ();' \
  mnema-desktop 'a_model_change_is_refused_while_a_job_holds_the_slot' --test model_commands

# The same removal, the test that drives it against a pass genuinely in flight.
# One case per test, since `case_` names one at a time.
case_ "models: a model change must not be possible while a pass is writing (T8)" \
  src-tauri/src/models.rs \
  's{    let _slot = state\.claim_job\(\)\?;}{    let _slot = ();}' \
  '    let _slot = ();' \
  mnema-desktop 'a_run_leaves_no_vectors_in_a_space_nothing_points_at' --test model_commands

# Both cases above replace the claim with `let _slot = ();` rather than deleting
# the line, and the marker is that text alone — code, and text that exists
# nowhere in the file unmutated. Review round 1, Minor 7: they quoted three lines
# of comment prose plus one line of code, because a pure deletion leaves no
# contiguous code the marker can anchor on (the doc comment sits between the two
# statements that would become adjacent). Substituting instead of deleting is
# what makes a code-only discriminator exist at all here.
#
# ⚠️ `let _slot` and not `let _`: the second drops the slot at once and holds
# nothing. No case here reddens on that one character — distinguishing them
# needs a second job started from another thread *during* the command, which is
# a race rather than a test. It is written on the line itself instead.

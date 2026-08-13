use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{Db, Error, INDEX_FORMAT_VERSION};

/// The distance metric every space is built with.
///
/// A constant rather than a parameter because it has to appear in two places
/// that must not drift: the `embedding_space.metric` column and the `vec0` DDL.
/// vec0 defaults to L2 when the DDL omits the metric, so a space whose row said
/// `cosine` while its table ranked by L2 was silent — and the two orderings
/// disagree on the first result, not on some tail.
const METRIC: &str = "cosine";

/// Whether this space has given up on a chunk **for the text it holds now** —
/// one SQL fragment, read by the two methods that must never disagree about it.
///
/// [`Db::chunks_needing_embedding`] excludes what this matches;
/// [`Db::failed_chunk_count`] counts it. Written once because the two are the
/// same sentence read in opposite directions, and a second copy of it would be
/// free to drift into saying that a chunk is both permanently failed and about
/// to be retried — a screen showing a failure the next run silently clears.
///
/// State `2` is `failed`; the hash comparison is what makes it "for the text it
/// holds now", since an edited chunk's refusal was about text that no longer
/// exists anywhere. Expects the outer query to expose the chunk as `c` and to
/// bind the space id as `?1`.
const GIVEN_UP_ON_CURRENT_TEXT: &str = "SELECT 1 FROM chunk_embedding_state s
                  WHERE s.space_id = ?1 AND s.chunk_id = c.id
                    AND s.state = 2 AND s.content_hash = c.content_hash";

/// The queue itself: the `FROM` and `WHERE` of "which chunks does this space
/// still owe a vector", shared by the query that hands them over and the count
/// that measures how many are left.
///
/// One string for the same reason [`GIVEN_UP_ON_CURRENT_TEXT`] is one string,
/// and a sharper one: a count that disagreed with the query it describes is a
/// progress bar that stops at 8 400 of 9 000 with an empty queue, and the
/// person reading it has no way to tell that from work that stalled.
///
/// `table` is a `vec_emb_<id>` name and never caller text —
/// [`Db::embedded_chunk_count`]'s own comment gives that argument in full.
/// Binds the space id as `?1`; a caller adding `LIMIT` uses `?2`.
///
/// **This depends on `NOT IN` never meeting a NULL, and that argument belongs
/// here.** A single NULL among `{table}.chunk_id` would make `NOT IN` answer
/// NULL for every row of the outer query, emptying this queue over a full
/// archive — silently, and in the direction nobody would notice. It cannot
/// happen: `chunk_id` is declared `INTEGER PRIMARY KEY` on the `vec0` table
/// (`Db::create_space`'s DDL, `space.rs:220`), and every writer —
/// [`Db::insert_vector`], [`Db::upsert_vector`], [`Db::upsert_vector_for_text`]
/// — binds it a real `i64`, never a NULL.
fn the_embedding_queue(table: &str) -> String {
    format!(
        "FROM chunk c
           JOIN document d ON d.id = c.document_id
          WHERE d.status = 'indexed'
            AND c.id NOT IN (SELECT chunk_id FROM {table})
            AND NOT EXISTS ({GIVEN_UP_ON_CURRENT_TEXT})"
    )
}

/// A chunk the embedding pass still has to send, as
/// [`Db::chunks_needing_embedding`] hands it over.
///
/// A struct and not the `(i64, String, String)` it would otherwise be: two of
/// the three fields are strings, one is what goes on the wire to a paid
/// provider and the other is what decides whether a refusal is ever
/// reconsidered, and nothing about a tuple would notice them swapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChunk {
    pub id: i64,
    /// `chunk.text` — the original, which the schema documents as "the
    /// original, for display", and **not** the [`crate::prepare_for_search`]
    /// copy. The prepared text exists for the lexical index; a vector is
    /// searched against what a person reads in the citation, so the provider
    /// has to see that.
    pub text: String,
    /// Carried out with the text so a caller can tell, without asking again,
    /// which version of it a refusal would be about. Nothing may pass it back
    /// in — [`Db::record_embedding_failure`] reads the hash itself, and its
    /// doc comment says why.
    pub content_hash: String,
}

/// What [`Db::adopt_embedding_model`] settled on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdoptedSpace {
    pub space_id: i64,
    pub model_config_id: i64,
    /// `false` when the space was already there — the ordinary case of choosing
    /// the same model twice.
    ///
    /// It reports what `create_space` did, and nothing else. "The active space
    /// changed" is a neighbouring fact that is true in almost the same cases,
    /// and answering both with one field is how a caller ends up told a space
    /// was created when the call found one.
    pub created: bool,
}

impl Db {
    /// Records a model configuration.
    ///
    /// There is no parameter for the API key, and there is not meant to be: the
    /// database may hold `credential_ref`, the name of an entry in the OS
    /// credential store, and never the secret itself.
    pub fn create_model_config(
        &self,
        name: &str,
        provider: &str,
        endpoint: Option<&str>,
        embed_model: &str,
        dim: i64,
    ) -> Result<i64, Error> {
        self.conn().execute(
            "INSERT INTO model_config (name, provider, endpoint, embed_model, dim)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, provider, endpoint, embed_model, dim],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// Creates the space row and its vector table — and, the first time either
    /// is needed here, records the vector library's version in `meta`. All of
    /// it or none of it: the three land in one transaction.
    ///
    /// The version is the third write and is easy to miss from outside, which
    /// is why it is in this sentence and not only in a comment inside the body.
    /// It is here rather than in the adoption path because
    /// [`crate::META_VEC_VERSION`] describes the version that created the first
    /// *space*, and this function is the only thing that creates one.
    ///
    /// The table name is derived from the immutable row id — never from a model
    /// or configuration name, because a vec0 table cannot be renamed: RENAME
    /// reports success and leaves the table unusable, and `integrity_check` does
    /// not notice. G7.0 §5.7.
    ///
    /// `dim` is checked against the model configuration's own. The two columns
    /// exist separately so a space can keep its width when the configuration is
    /// later edited, which means they are allowed to disagree afterwards and no
    /// CHECK can speak for them; agreeing at creation is this function's job.
    pub fn create_space(
        &self,
        model_config_id: i64,
        dim: i64,
        chunker_hash: &str,
    ) -> Result<i64, Error> {
        // IMMEDIATE, not the default DEFERRED: this reads the model config and
        // the id counter and then writes both a row and a table, and a deferred
        // transaction takes its write lock only at the first write — leaving a
        // window in which what it read has already changed under it.
        let tx = Transaction::new_unchecked(self.conn(), TransactionBehavior::Immediate)?;

        let config_dim: i64 = tx
            .query_row(
                "SELECT dim FROM model_config WHERE id = ?1",
                params![model_config_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::NoSuchModelConfig(model_config_id))?;
        if config_dim != dim {
            return Err(Error::SpaceDimMismatch {
                model_config_id,
                config_dim,
                space_dim: dim,
            });
        }

        // Reported before the INSERT rather than translated from the UNIQUE
        // violation afterwards, so the existing id can be handed back.
        if let Some(space_id) = existing_space_id(&tx, model_config_id, dim, chunker_hash)? {
            return Err(Error::SpaceAlreadyExists { space_id });
        }

        // The id must be known before the INSERT rather than read back after it:
        // the CHECK on `vec_table` compares the name against the id in the same
        // row, so there is no placeholder to insert first and correct later.
        //
        // It comes from `sqlite_sequence` and NOT from `max(id) + 1`, which
        // reuses: drop the newest space and the next one takes its id, drop them
        // all and numbering restarts at 1 — in both cases inheriting the dropped
        // space's table name too, so anything still holding the old id addresses
        // the new space instead of failing. `embedding_space.id` is declared
        // AUTOINCREMENT for exactly this: SQLite then keeps the high-water mark
        // in `sqlite_sequence`, advances it for explicitly supplied ids as well,
        // and leaves it alone on DELETE. The CHECK on `vec_table` is what would
        // say so if this ever disagreed with the id actually assigned.
        let id: i64 = tx.query_row(
            "SELECT ifnull((SELECT seq FROM sqlite_sequence WHERE name = 'embedding_space'), 0) + 1",
            [],
            |r| r.get(0),
        )?;
        let table = vec_table_name(id);

        tx.execute(
            "INSERT INTO embedding_space
                (id, model_config_id, dim, metric, index_format_version, chunker_hash,
                 vec_table, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'building')",
            params![
                id,
                model_config_id,
                dim,
                METRIC,
                INDEX_FORMAT_VERSION,
                chunker_hash,
                table
            ],
        )?;

        // `chunk_id` is the primary key because vec0 pre-filters a KNN query on
        // the primary key only, and a tag filter compiles to exactly that.
        //
        // The width is interpolated because it has to be — vec0 parses it out of
        // the DDL at CREATE time, where a bound parameter cannot reach. Both
        // interpolated values are this module's own: an integer it just wrote and
        // a constant above, never caller text.
        tx.execute_batch(&format!(
            "CREATE VIRTUAL TABLE {table} USING vec0(
                 chunk_id INTEGER PRIMARY KEY,
                 embedding float[{dim}] distance_metric={METRIC}
             );"
        ))?;

        // The vector library's version, recorded when the first space here
        // needs one — which is this function and not the adoption path, because
        // this one is public and a space created through it is as much a space
        // as any other. `META_VEC_VERSION`'s own doc says "the version that
        // created the first space"; written one level up, that sentence
        // described an event the code did not record, and a `create_space`
        // caller left no version at all. Measured before the move: the key was
        // `None` after a direct `create_space`.
        //
        // `sqlite-vec` exports no version constant — the crate declares
        // `sqlite3_vec_init` and nothing else — so the string comes from the
        // extension running on this connection. Measured: `v0.1.9`. Inside this
        // transaction, so a space and the version that built it land together.
        if self.meta_get(crate::META_VEC_VERSION)?.is_none() {
            let version: String = tx.query_row("SELECT vec_version()", [], |r| r.get(0))?;
            self.meta_set(crate::META_VEC_VERSION, &version)?;
        }

        tx.commit()?;
        Ok(id)
    }

    /// Stores one embedding.
    ///
    /// The vector is checked before vec0 sees it, because vec0 accepts a
    /// degenerate one without complaint and the damage only shows up as a
    /// confident wrong answer at query time. This function has no caller
    /// outside tests yet, and the check is here rather than at the caller for
    /// that reason: under D29 v1 ships no local models, so every embedding
    /// that ever reaches it will be a JSON response from a third party, and a
    /// truncated or failed one arriving as zeros will be ordinary rather than
    /// hypothetical.
    ///
    /// **`space_id` is a parameter, and writing a space that is not the active
    /// one is how the archive gets split.** Nothing here can stop it, and
    /// nothing should: building the new space beside the old one and filling it
    /// before switching is the sanctioned migration, and it is exactly this
    /// call into a non-active space. [`Db::adopt_embedding_model`] guards
    /// transitions — it refuses to move the index onto a space while another
    /// holds embeddings — and a writer that puts *new* chunks somewhere the
    /// index is not pointing never asks it anything, so no guard sees that at
    /// all. The obligation is the caller's: embed into [`Db::active_space`],
    /// unless you are rebuilding a space you are about to switch to. That
    /// obligation is written down now, and enforced — not here, but by the one
    /// caller that has it: `mnema_embed::run` takes **no** `space_id`, reads
    /// [`Db::active_space`] itself, and so has no way to name any other space.
    /// The enforcement is a test, `the_pass_writes_only_into_the_active_space`
    /// in `crates/mnema-embed/tests/queue.rs`, which stands an idle space
    /// beside the active one and fails if a vector lands in the wrong one.
    /// A test is all it can ever be, and the paragraph above is why: this
    /// function has to keep letting the obligation be broken.
    pub fn insert_vector(&self, space_id: i64, chunk_id: i64, v: &[f32]) -> Result<(), Error> {
        let space = self.space(space_id)?;
        check_rankable(v, &space.metric, VectorRole::Stored(chunk_id))?;
        self.conn().execute(
            &format!(
                "INSERT INTO {} (chunk_id, embedding) VALUES (?1, ?2)",
                space.table
            ),
            params![chunk_id, as_blob(v)],
        )?;
        Ok(())
    }

    /// Stores one embedding, replacing whatever this chunk already had in
    /// this space — and clearing any stale `chunk_embedding_state` row for
    /// it, in the same transaction.
    ///
    /// The check `insert_vector` documents applies here for the same reason
    /// and is not weakened: a degenerate vector is degenerate whether it is
    /// the first write for a chunk or the second. What differs is only the
    /// collision rule — a retry that already wrote half a batch must be able
    /// to finish without first asking which half landed.
    ///
    /// Delete then insert, rather than one statement that does both. Not for
    /// lack of an `ON CONFLICT`: measured directly (`INSERT INTO vec_emb_1 ...
    /// ON CONFLICT(chunk_id) DO UPDATE ...` against a live space), SQLite
    /// itself answers `"UPSERT not implemented for virtual table"` — this is
    /// SQLite refusing the statement before it ever reaches `vec0`'s own
    /// `xUpdate`, not a gap `vec0` could close. `vec0` does implement `UPDATE`
    /// on its own — `UPDATE vec_emb_1 SET embedding = ? WHERE chunk_id = ?`
    /// measured `Ok(1)` with the new bytes on read-back — so a single-`UPDATE`
    /// form is available and delete+insert is a choice, not a workaround: it
    /// is what makes "no row for this chunk yet" and "replacing this chunk's
    /// row" the same code path, rather than an `UPDATE` that silently touches
    /// zero rows the one time there was nothing to replace. `INSERT OR
    /// REPLACE` was checked too, and fails exactly like a plain `INSERT` —
    /// `"UNIQUE constraint failed on ... primary key"` — because `vec0`'s
    /// `xUpdate` (`vec0Update` → `vec0Update_Insert` →
    /// `vec0Update_InsertRowidStep`, `sqlite-vec.c` 0.1.9, the version
    /// `create_space` records into `meta`) never calls
    /// `sqlite3_vtab_on_conflict()`, so the `OR REPLACE` conflict-resolution
    /// mode it would need to read is never looked at.
    ///
    /// The `chunk_embedding_state` row is cleared for the same reason as
    /// [`Db::delete_vector`]'s own doc gives, in the direction that matters
    /// here: a row from before this write — `state = 1` from an interrupted
    /// earlier run, or `state != 1` recording a failure this write has now
    /// superseded — no longer describes the chunk once this call succeeds,
    /// and a caller reading that table (Task 6's queue) must not find a
    /// chunk marked failed the moment after it was embedded.
    ///
    /// **The narrower true sentence, now that Task 6 writes this table:**
    /// nothing in this crate writes `state = 1` — only `state = 2`
    /// ([`Db::record_embedding_failure`]) is ever written, `state = 0`
    /// deliberately never (that method's own doc says why) — so the `DELETE`
    /// below removes a `failed` row, not nothing. This method has no
    /// production caller of its own, but the identical `DELETE` in its
    /// sibling [`Db::upsert_vector_for_text`] does, and there the removal is
    /// load-bearing: it is what takes a refusal off
    /// [`Db::embedded_chunk_count`]'s third number the moment a chunk embeds.
    ///
    /// `insert_vector` is deliberately left alone. Its "exactly once" is a
    /// statement some caller may want enforced, and quietly turning it into
    /// a replace would remove an error worth seeing.
    ///
    /// ⚠️ **After the indexing cycle this method has no production call sites —
    /// `grep -rn upsert_vector crates src-tauri --include='*.rs'` finds only
    /// doc references and tests.** The embedding pass deliberately uses
    /// [`Db::upsert_vector_for_text`] instead, because writing by `chunk_id`
    /// alone binds a vector to whatever chunk holds that id *now*, and ids are
    /// reused. This one is kept rather than removed or narrowed: a full space
    /// migration — build the new space beside the old one and fill it — is
    /// deferred to its own cycle, and a bulk copy between spaces has no text to
    /// compare against and no reason to want one. It is written down here
    /// because an unchecked public write with no callers is the thing a later
    /// session reaches for first, and the reason not to is two functions away.
    pub fn upsert_vector(&self, space_id: i64, chunk_id: i64, v: &[f32]) -> Result<(), Error> {
        let space = self.space(space_id)?;
        check_rankable(v, &space.metric, VectorRole::Stored(chunk_id))?;
        self.transaction(|tx| {
            tx.execute(
                &format!("DELETE FROM {} WHERE chunk_id = ?1", space.table),
                params![chunk_id],
            )?;
            tx.execute(
                &format!(
                    "INSERT INTO {} (chunk_id, embedding) VALUES (?1, ?2)",
                    space.table
                ),
                params![chunk_id, as_blob(v)],
            )?;
            tx.execute(
                "DELETE FROM chunk_embedding_state WHERE space_id = ?1 AND chunk_id = ?2",
                params![space_id, chunk_id],
            )?;
            Ok(())
        })
    }

    /// [`Db::upsert_vector`], but only if this chunk still holds the text the
    /// vector was made from. Answers whether it wrote.
    ///
    /// **The window this closes.** An embedding pass reads `(chunk_id, text)`
    /// from [`Db::chunks_needing_embedding`], sends the text over a network,
    /// and comes back seconds later to write the answer by `chunk_id`. In
    /// between, a rebuild can delete that chunk and write a new one — and
    /// `chunk.id` is `INTEGER PRIMARY KEY` **without** `AUTOINCREMENT`, so the
    /// new chunk can be handed the very same id. `tests/citation.rs`'s
    /// `a_reused_chunk_id_gets_no_inherited_vector` asserts that the reuse
    /// happens rather than assuming it. The unguarded write then binds the
    /// vector for text A to a chunk that now holds text B, and vector search
    /// answers with a citation quoting text the file no longer contains.
    ///
    /// D88 does not cover this and was never meant to: it removes a vector that
    /// *outlived* its chunk, and here the vector arrives after the chunk is
    /// already gone — the rows it sweeps do not exist yet when it runs.
    ///
    /// **The comparison and the write are one transaction, and that is the
    /// whole mechanism.** A caller that reads the hash, compares it, and then
    /// calls [`Db::upsert_vector`] has rebuilt the same window one layer up:
    /// two statements with a gap between them is the thing being removed.
    ///
    /// **Today this is defensive, not load-bearing, and it matters that a later
    /// session can tell which.** `src-tauri/src/state.rs` holds a single
    /// `running` flag for the whole application, so no rebuild can be in flight
    /// while an embedding pass is; the race is unreachable and this check is
    /// expected to answer `true` every time it is asked. **It becomes
    /// load-bearing the day indexing work gets a database connection of its
    /// own** — already written down as deferred to the search cycle, and
    /// exactly the change that would let a rebuild and the queue run at once.
    /// Whoever makes that change should arrive at this sentence.
    ///
    /// `false` is not an error. The chunk is gone, or its text has moved on;
    /// either way this vector describes nothing that is there, and the computed
    /// queue offers the chunk again with its current text if it still exists.
    /// Nothing has to be recorded for that to happen — which is the same
    /// property that lets [`Db::clear_document_content`] write no replacement.
    pub fn upsert_vector_for_text(
        &self,
        space_id: i64,
        chunk_id: i64,
        content_hash: &str,
        v: &[f32],
    ) -> Result<bool, Error> {
        let space = self.space(space_id)?;
        check_rankable(v, &space.metric, VectorRole::Stored(chunk_id))?;
        self.transaction(|tx| {
            let still_this_text: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM chunk WHERE id = ?1 AND content_hash = ?2",
                    params![chunk_id, content_hash],
                    |r| r.get(0),
                )
                .optional()?;
            if still_this_text.is_none() {
                return Ok(false);
            }
            tx.execute(
                &format!("DELETE FROM {} WHERE chunk_id = ?1", space.table),
                params![chunk_id],
            )?;
            tx.execute(
                &format!(
                    "INSERT INTO {} (chunk_id, embedding) VALUES (?1, ?2)",
                    space.table
                ),
                params![chunk_id, as_blob(v)],
            )?;
            tx.execute(
                "DELETE FROM chunk_embedding_state WHERE space_id = ?1 AND chunk_id = ?2",
                params![space_id, chunk_id],
            )?;
            Ok(true)
        })
    }

    /// Removes one chunk's embedding from one space, and any
    /// `chunk_embedding_state` row for it, in one transaction. Absence of
    /// either is not an error: the caller retrying a partially failed batch
    /// cannot know which rows already landed, and a delete that refused on
    /// "nothing to delete" would turn every retry into a special case.
    ///
    /// The bookkeeping row has to go too, and atomically with the vector,
    /// because [`Db::embedded_chunk_count`]'s own doc explains why leaving it
    /// is not merely untidy: its `UNION` counts a `chunk_embedding_state` row
    /// with `state = 1` as an embedded chunk on its own, the same way
    /// `tests/adopt.rs`'s `bookkeeping_without_a_vector_also_makes_a_space_not_empty`
    /// demonstrates. A vector deleted without its bookkeeping would leave a
    /// chunk counted as embedded with nothing to show for it.
    pub fn delete_vector(&self, space_id: i64, chunk_id: i64) -> Result<(), Error> {
        let space = self.space(space_id)?;
        self.transaction(|tx| {
            tx.execute(
                &format!("DELETE FROM {} WHERE chunk_id = ?1", space.table),
                params![chunk_id],
            )?;
            tx.execute(
                "DELETE FROM chunk_embedding_state WHERE space_id = ?1 AND chunk_id = ?2",
                params![space_id, chunk_id],
            )?;
            Ok(())
        })
    }

    /// Nearest neighbours, optionally restricted to a set of chunk ids — which is
    /// what a tag filter becomes, since tags are many-to-many and cannot be a
    /// column on the vector table.
    ///
    /// The restriction is a `json_each` subquery and not an inline list, for a
    /// reason that only shows up at one list length: SQLite rewrites a
    /// one-element `IN (n)` into `n = …`, vec0 does not recognise that as a KNN
    /// pre-filter, and the query then returns *nothing at all* whenever `k` is
    /// smaller than the table. A filter naming a single chunk is not an unusual
    /// case, and silence is not a failure anyone would see.
    ///
    /// `NULLS LAST` is not decoration. vec0 answers `distance = NULL` for a row
    /// whose cosine distance is undefined, SQLite sorts NULLs first ascending,
    /// and the re-sort would therefore hand rank 1 to exactly the meaningless
    /// row — vec0's own ordering does not. Like the `IN` rewrite above, the
    /// fault hides at small k: measured, k = 1 and k = 2 were right and k = 3
    /// put the degenerate row on top.
    ///
    /// `k` is capped by vec0 at 4096; above that it errors with
    /// `k value in knn query too large`. A tag-filtered "everything under this
    /// tag" is the query that will meet it.
    pub fn knn(
        &self,
        space_id: i64,
        query: &[f32],
        k: i64,
        restrict_to: Option<&[i64]>,
    ) -> Result<Vec<i64>, Error> {
        let space = self.space(space_id)?;
        check_rankable(query, &space.metric, VectorRole::Query)?;
        let table = space.table;
        let blob = as_blob(query);

        let Some(ids) = restrict_to else {
            let mut stmt = self.conn().prepare(&format!(
                "SELECT chunk_id FROM {table}
                  WHERE embedding MATCH ?1 AND k = ?2
                  ORDER BY distance NULLS LAST"
            ))?;
            let rows = stmt.query_map(params![blob, k], first_id)?;
            return rows.collect::<Result<Vec<i64>, _>>().map_err(Error::from);
        };

        let list = serde_json::to_string(ids)?;
        let mut stmt = self.conn().prepare(&format!(
            "SELECT chunk_id FROM {table}
              WHERE embedding MATCH ?1 AND k = ?2
                AND chunk_id IN (SELECT value FROM json_each(?3))
              ORDER BY distance NULLS LAST"
        ))?;
        let rows = stmt.query_map(params![blob, k, list], first_id)?;
        rows.collect::<Result<Vec<i64>, _>>().map_err(Error::from)
    }

    /// Removes every vector — across every embedding space, not only the
    /// current one — that belongs to one document's chunks.
    ///
    /// A `vec0` table cannot be the target of a foreign key (G7.0 §5.7), so
    /// nothing about `document`'s `ON DELETE CASCADE` reaches these tables
    /// when a document goes: without this, a vector would outlive the chunk
    /// it embeds, silently, and nothing downstream would ever notice a
    /// `vec_emb_<n>` row naming a `chunk_id` no chunk owns any more. Called
    /// from `mnema-ingest`'s `forget_if_unnamed`, the one place that decides
    /// an ordinary edit or reconciliation has left a document with no path
    /// naming it, and — via this module's private `delete_vectors_for_document_in`,
    /// directly against an already-open transaction — from
    /// [`Db::delete_watched_root`], which decides the same question for a
    /// whole root at once and needs the sweep and the document's own
    /// deletion in one transaction rather than two calls that could be
    /// interrupted between them.
    ///
    /// Must run BEFORE the document (and, by cascade, its chunks) is
    /// deleted: once they are gone there is no `chunk.document_id` left to
    /// look their ids up by. Every space is swept, not only whichever is
    /// "current": an older space, left behind by a retired model
    /// configuration, can still hold vectors for chunks that were never
    /// re-embedded into a newer one — `drop_space` is the mechanism for
    /// retiring a whole space at once, this is the mechanism for one
    /// document leaving every space it was ever in.
    pub fn delete_vectors_for_document(&self, document_id: &str) -> Result<(), Error> {
        delete_vectors_for_document_in(self.conn(), document_id)
    }

    /// Drops the space and its vector table. Dropping is the mechanism for a
    /// model change; per-row DELETE is the mechanism for an edited file. G7.0 §5.7.
    pub fn drop_space(&self, space_id: i64) -> Result<(), Error> {
        let table = self.space(space_id)?.table;
        // IMMEDIATE here is consistency with `create_space`, not a guard, and no
        // test can tell the two apart: this transaction only writes, and its one
        // read happens before BEGIN, so a deferred one would take the same lock
        // a statement later and fail identically.
        let tx = Transaction::new_unchecked(self.conn(), TransactionBehavior::Immediate)?;
        // DROP takes the four shadow tables with it, so the id becomes reusable.
        tx.execute_batch(&format!("DROP TABLE IF EXISTS {table};"))?;
        tx.execute(
            "DELETE FROM embedding_space WHERE id = ?1",
            params![space_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Which space the product is working with, if one has been chosen.
    ///
    /// A value that does not parse as an id reads as "nothing chosen yet"
    /// rather than as an error. Only [`Db::adopt_embedding_model`] ever writes
    /// this key, and it writes an `i64`, so an unparsable one means the file
    /// was edited around this crate — and there is nothing a caller could do
    /// with the distinction that it cannot do with `None`.
    ///
    /// ⚠️ **This must stay a straight read of the stored value: no fallback.**
    /// "If the key is absent, use the only space there is" is the plausible,
    /// well-meant change that would break something two functions away:
    /// `refuse_if_the_move_would_orphan_anything` exempts a call from the
    /// split guard when this answer already equals the space about to be
    /// written, and that is sound only while the answer is decided by the
    /// stored value alone. With a fallback, this would report a space nobody
    /// chose, the exemption would fire on a call that really does move the
    /// index, and the guard would be skipped in silence. Anything derived
    /// belongs in the caller that wants it, not here.
    pub fn active_space(&self) -> Result<Option<i64>, Error> {
        Ok(self
            .meta_get(crate::META_ACTIVE_SPACE)?
            .and_then(|v| v.parse().ok()))
    }

    /// The model this space embeds with and how wide its vectors are — what the
    /// settings screen puts in front of the person who chose them.
    ///
    /// [`Error::NoSuchSpace`] and not `Ok(None)` for an id with no row, which is
    /// the answer [`Db::space_is_empty`] and [`Db::embedded_chunk_count`]
    /// already give and is the only one coherent with them: a single caller
    /// asking all three about one id must not be told the space is absent by two
    /// of them and empty by the third.
    ///
    /// The distinction it keeps is the caller's, not this crate's. Whoever
    /// reaches this is holding [`Db::active_space`], and the two facts in front
    /// of it are "nobody has chosen a model" — which is `active_space`
    /// answering `None`, one call earlier — and "the pointer names a space that
    /// is gone", which is a defect in this build. Folded together they draw an
    /// empty model picker over an index that may still hold vectors, and the
    /// person reading it chooses a model and pays to embed the archive again.
    ///
    /// Both columns are `NOT NULL` — `model_config.embed_model` and
    /// `embedding_space.dim` — so a row that exists always names both, and there
    /// is no third state for this to return.
    pub fn space_model(&self, space_id: i64) -> Result<(String, i64), Error> {
        self.conn()
            .query_row(
                "SELECT c.embed_model, s.dim FROM embedding_space s
                   JOIN model_config c ON c.id = s.model_config_id
                  WHERE s.id = ?1",
                params![space_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(Error::NoSuchSpace(space_id))
    }

    /// How many chunks the index holds at all — the denominator the settings
    /// screen shows [`Db::embedded_chunk_count`] against.
    ///
    /// ⚠️ **The two are not counted over the same population, and the
    /// difference has a direction.** A `vec_emb_<id>` row is referenced by no
    /// foreign key, so nothing forces it to go when the chunk it embeds does.
    /// [`Db::clear_document_content`] closed this for a rebuild (D88): it now
    /// sweeps a document's vectors, in every space, before the `chunk` rows
    /// and their `chunk_embedding_state` rows go with it. [`Db::delete_document`]
    /// has not — deleting a document outright still leaves its vectors behind.
    /// So the numerator can still exceed this denominator through that path,
    /// and whatever renders the pair has to be able to show that rather than a
    /// percentage above one hundred. Neither is zero today — D29 described a
    /// build with nothing embedded, and this branch built the thing that
    /// embeds — so the trap is no longer hypothetical: a document's vectors
    /// can genuinely outlive it through [`Db::delete_document`], and whatever
    /// renders the pair has to show a numerator above the denominator rather
    /// than clamp it away.
    pub fn chunk_count(&self) -> Result<i64, Error> {
        Ok(self
            .conn()
            .query_row("SELECT count(*) FROM chunk", [], |r| r.get(0))?)
    }

    /// How many vector spaces the index holds at all.
    ///
    /// **Not a diagnostic.** It exists so a caller holding
    /// [`Db::embedded_chunk_count`] for one space can say whether that space is
    /// the only one there is — and therefore whether the number it holds is the
    /// whole of what a model change would cost. Without it, a caller that can
    /// count one space has no way to tell "this is the bill" from "this is part
    /// of a bill I cannot read", and [`Error::SpaceNotEmpty`]'s own doc is where
    /// the difference is argued: the space that blocks need not be the active
    /// one, nor pointed at by anything.
    ///
    /// It counts rows in `embedding_space`, empty ones included. That is the
    /// question a caller asking "is what I can see all there is" is asking; an
    /// empty space is one this caller cannot see either.
    pub fn space_count(&self) -> Result<i64, Error> {
        Ok(self
            .conn()
            .query_row("SELECT count(*) FROM embedding_space", [], |r| r.get(0))?)
    }

    /// How many chunk embeddings the index holds **across every space**, which
    /// is what a model change with the old spaces retired actually costs.
    ///
    /// **Summed over spaces, distinct within one.** The same chunk embedded in
    /// two spaces is two embeddings, because two calls to a provider made them
    /// and two would have to be made again; the same chunk recorded twice inside
    /// one space is one, for the reason [`Db::embedded_chunk_count`]'s own doc
    /// gives. Built by calling that method per space rather than by a second
    /// query that does its own arithmetic: the `UNION` in there is load-bearing
    /// and a copy of it here could disagree with it, in a number somebody reads
    /// before deciding to destroy something.
    ///
    /// **Why this and not [`Db::embedded_chunk_count`] of the active space.** A
    /// space left behind by an earlier model change is still in
    /// `embedding_space` — `Db::adopt_embedding_model` mints and repoints, and
    /// never removes what it moved off, which `tests/adopt.rs`'s
    /// `returning_to_a_model_already_tried_creates_nothing` pins at two spaces —
    /// so "the active space's count" is not what a retiring change throws away
    /// the moment anybody has ever tried a second model.
    pub fn embedded_chunks_everywhere(&self) -> Result<i64, Error> {
        let ids: Vec<i64> = self
            .conn()
            .prepare("SELECT id FROM embedding_space")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut total = 0;
        for space_id in ids {
            total += self.embedded_chunk_count(space_id)?;
        }
        Ok(total)
    }

    /// Whether moving the index off this space would throw anything away.
    ///
    /// Reads **both** places a chunk can be recorded as embedded, because
    /// neither alone is trustworthy, in either direction. Nothing in this
    /// crate writes `state = 1` to `chunk_embedding_state` — only `state = 2`
    /// is, by [`Db::record_embedding_failure`]; [`Db::insert_vector`] writes
    /// only the `vec0` table — so a check reading that table alone would call
    /// a space full of vectors empty: an assertion satisfied by zero from one
    /// side. And a `vec0` table takes no foreign key, so a
    /// vector can still outlive the chunk it embeds and the
    /// `chunk_embedding_state` row that cascaded away with it —
    /// [`Db::clear_document_content`] closed that for a rebuild (D88), but
    /// [`Db::delete_document`] has not — so a check reading only `vec_emb_<id>`
    /// would be satisfied by zero from the other side instead.
    ///
    /// A space that does not exist is **not** empty — it is absent, and that
    /// arrives as [`Error::NoSuchSpace`] rather than as `Ok(true)`. Two facts
    /// leaving through one `bool` is how a caller comes to believe it asked
    /// about something.
    pub fn space_is_empty(&self, space_id: i64) -> Result<bool, Error> {
        Ok(self.embedded_chunk_count(space_id)? == 0)
    }

    /// How many chunks this space has an embedding recorded for — the number a
    /// refusal puts in front of the person deciding whether to rebuild, and the
    /// numerator the settings screen shows against [`Db::chunk_count`].
    ///
    /// **Distinct chunks, not rows, and that is the whole point of the
    /// `UNION`.** A `chunk_embedding_state` row with `state = 1` says "this
    /// chunk is embedded here" and the row in `vec_emb_<id>` is the embedding
    /// it is talking about: two records of *one* embedded chunk. Adding the two
    /// counts reports twice what a switch costs, in a sentence a person reads
    /// before deciding. Emptiness cannot tell the difference — zero and zero is
    /// zero however they are combined — so the arithmetic is visible only in
    /// the number, which is exactly why it needs its own test.
    ///
    /// **Reading only `chunk_embedding_state` would answer zero for a full
    /// space today**, which is why the `UNION` is load-bearing for the second
    /// caller as well as the first: [`Db::insert_vector`] writes the `vec0`
    /// table, and the only writer of `chunk_embedding_state` writes
    /// `state = 2` — `grep -rn chunk_embedding_state crates/*/src` now finds
    /// [`Db::record_embedding_failure`]'s `INSERT`, but nothing writes
    /// `state = 1`, which is the value this `UNION` selects from that table.
    /// The narrower query would put "0 of 900 embedded" in front of someone
    /// whose archive is embedded, and send them to pay for it again.
    ///
    /// An id with no row is [`Error::NoSuchSpace`] and not zero. It was zero
    /// for one commit, to keep a `meta.active_space` left dangling by
    /// [`Db::drop_space`] from becoming a dead end — and that special case is
    /// gone with the pointer it served: the refusal below enumerates
    /// `embedding_space`, so a dropped space is simply not among the ids it
    /// asks about. What is left is a public method being asked about an id
    /// nobody wrote, and the honest answer to that is which id was wrong.
    pub fn embedded_chunk_count(&self, space_id: i64) -> Result<i64, Error> {
        let table = self.space(space_id)?.table;
        // `table` is never caller text: it comes only from
        // `embedding_space.vec_table`, which the schema's own CHECK pins to
        // `'vec_emb_' || id` — the same reasoning `knn` and `insert_vector`
        // already rely on for interpolating a table name into SQL.
        Ok(self.conn().query_row(
            &format!(
                "SELECT count(*) FROM (
                     SELECT chunk_id FROM chunk_embedding_state
                      WHERE space_id = ?1 AND state = 1
                     UNION
                     SELECT chunk_id FROM {table}
                 )"
            ),
            params![space_id],
            |r| r.get(0),
        )?)
    }

    /// The next `limit` chunks this space has no embedding for — the computed
    /// queue, asked fresh, which is the shape [`Db::clear_document_content`]'s
    /// own doc comment already argued for before anything asked it.
    ///
    /// **Nothing is stored anywhere that says "this chunk is waiting".** A
    /// chunk is in the queue because there is no row for it in `vec_emb_<id>`,
    /// and for no other reason, so a document cleared for a rebuild rejoins it
    /// by the same route a brand new one arrives: the rows went, the question
    /// is asked again, the answer changed. `chunk_embedding_state` state `0`
    /// — the schema's `pending` — is never written by anybody, and this
    /// method is why it does not need to be.
    ///
    /// **Three filters, three different facts, and the one in the middle is
    /// the only one that is obvious.**
    ///
    /// - `document.status = 'indexed'`, because a document that is `pending`
    ///   is a document [`Db::clear_document_content`] has just emptied and a
    ///   rebuild is about to refill: its chunks are minutes from being deleted
    ///   and replaced, and embedding them spends the user's money on rows that
    ///   will not exist. This is the first reader in the product to depend on
    ///   that column's permitted values, which are `'pending'`, `'indexed'`,
    ///   `'failed'` and `'skipped'` (the schema's own CHECK) —
    ///   [`crate::DocumentStatus`] is the enum, and it is spelled out here
    ///   rather than compared against because this is SQL.
    /// - No row in the vector table. This is the whole of "not done yet".
    /// - No `chunk_embedding_state` row that is *still about this text* — see
    ///   [`GIVEN_UP_ON_CURRENT_TEXT`], which is the same string
    ///   [`Db::failed_chunk_count`] reads. A chunk refused once is out of the
    ///   queue until its text changes; a chunk whose text has since changed is
    ///   back in it, because the refusal was about text that no longer exists.
    ///
    /// **What that costs, said plainly rather than left to be discovered: a
    /// chunk refused for a reason that had nothing to do with it — a provider
    /// having a bad minute — is not tried again until somebody edits the file.**
    /// That is deliberate. The alternative is a pass that spins on the same
    /// chunk for as long as the archive is open, and the design pays for the
    /// standstill instead, on the condition that it is never silent:
    /// [`Db::failed_chunk_count`] is what puts the number in front of a person.
    /// Whatever eventually retries a failure needs to know what the provider
    /// answers to an over-long chunk, and nothing has measured that yet, so it
    /// is not decided here.
    ///
    /// `limit` is the caller's batch size, and this is the reason the caller
    /// may ask again in a loop without tracking anything: every chunk it takes
    /// leaves the queue by one of the two routes above before the next ask.
    pub fn chunks_needing_embedding(
        &self,
        space_id: i64,
        limit: usize,
    ) -> Result<Vec<PendingChunk>, Error> {
        let queue = the_embedding_queue(&self.space(space_id)?.table);
        let mut stmt = self.conn().prepare(&format!(
            "SELECT c.id, c.text, c.content_hash {queue} ORDER BY c.id LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![space_id, limit as i64], |r| {
            Ok(PendingChunk {
                id: r.get(0)?,
                text: r.get(1)?,
                content_hash: r.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Error::from)
    }

    /// How many chunks [`Db::chunks_needing_embedding`] has left to hand over —
    /// the denominator of one run, not of the archive.
    ///
    /// Built from the same `FROM`/`WHERE` as the query it counts, so the two
    /// cannot come to disagree about what "queued" means. A progress bar is the
    /// caller: it wants the number measured once, at the start, because the
    /// queue shrinks as the run empties it and a total re-read every batch is a
    /// bar that never moves.
    ///
    /// Not the same question as [`Db::chunk_count`] minus
    /// [`Db::embedded_chunk_count`]: that difference also contains every chunk
    /// this space has given up on, and every chunk of a document that is not
    /// `indexed`. Those are [`Db::failed_chunk_count`] and the pass's own
    /// business respectively, and folding all three into one subtraction is how
    /// "8 400 of 9 000" comes to mean four different things at once.
    pub fn queued_chunk_count(&self, space_id: i64) -> Result<i64, Error> {
        let queue = the_embedding_queue(&self.space(space_id)?.table);
        Ok(self.conn().query_row(
            &format!("SELECT count(*) {queue}"),
            params![space_id],
            |r| r.get(0),
        )?)
    }

    /// How many chunks this space has given up on — the third number, beside
    /// [`Db::embedded_chunk_count`] and [`Db::chunk_count`].
    ///
    /// It exists because `M − N` is a lie in exactly one state, and it is the
    /// state that matters: "embedded 8 400 of 9 000" reads as *not yet*, and
    /// for a chunk the provider refused it means *never*. That chunk is still
    /// in the database, the document still shows it, keyword search still
    /// finds it — and vector search will not return it again, with nothing
    /// anywhere saying why. This method is the only thing standing between
    /// that and silence, which is why [`Db::chunks_needing_embedding`] is
    /// allowed to be as unforgiving as it is.
    ///
    /// **A failed row whose `content_hash` no longer matches its chunk is not
    /// counted, and that is not tidiness.** The two methods share
    /// [`GIVEN_UP_ON_CURRENT_TEXT`] — one string, read by both — precisely so
    /// they cannot answer differently: a chunk that this method calls failed
    /// while the queue is about to hand it back would put a number on the
    /// screen that the very next run disproves.
    ///
    /// Counted over chunks that exist, whatever their document's status.
    /// "Given up on" and "will be reached" are two questions, and a chunk
    /// belonging to a document that is being rebuilt is out of the queue for a
    /// reason that has nothing to do with a refusal.
    pub fn failed_chunk_count(&self, space_id: i64) -> Result<i64, Error> {
        Ok(self.conn().query_row(
            &format!("SELECT count(*) FROM chunk c WHERE EXISTS ({GIVEN_UP_ON_CURRENT_TEXT})"),
            params![space_id],
            |r| r.get(0),
        )?)
    }

    /// Records that the provider refused this chunk's text, and counts the
    /// attempt.
    ///
    /// The hash is **read from the chunk row here** rather than accepted as a
    /// parameter. A caller passing one is a caller that can pass a stale one,
    /// and a stale hash does not fail loudly: the row it writes matches no
    /// chunk, [`Db::chunks_needing_embedding`] hands the chunk straight back,
    /// and a batching loop that trusts the queue to shrink runs forever
    /// against a paid provider. There is exactly one place the hash can come
    /// from, so there is nothing to keep in step.
    ///
    /// `INSERT … SELECT` and not `VALUES`, for the same reason: a chunk that
    /// has gone in the meantime selects no row, so this writes nothing and
    /// says so with `Ok`, rather than tripping the foreign key.
    ///
    /// State `2` is `failed` (the schema's own comment on the CHECK). State
    /// `0`, `pending`, is deliberately never written by anything: it would be
    /// a record of intent, and [`Db::upsert_vector`] clears this whole row
    /// unconditionally when it writes — so a `pending` marker left for a new
    /// text version would be erased by an in-flight embedding of the *old*
    /// text landing afterwards, and the chunk would then read as embedded,
    /// with the superseded vector, permanently.
    ///
    /// `attempts` is counted and nothing reads it as a threshold. There is no
    /// maximum, on purpose: what separates "this text can never be embedded"
    /// from "try again later" depends on what the provider actually answers to
    /// an over-long chunk, and nobody has measured that. The column is a
    /// record, not a policy.
    /// Answers **whether it wrote**, and the caller is expected to count that
    /// rather than the call.
    ///
    /// The `SELECT` finds no row when the chunk has gone, so this writes
    /// nothing and returns `Ok(false)` — no foreign key is tripped and there is
    /// nothing to report as an error. A caller that incremented a `failed`
    /// counter regardless would put a number on the settings screen that
    /// [`Db::failed_chunk_count`] disagrees with, and that number is the whole
    /// safety argument for a chunk being allowed to leave the queue: a failure
    /// count including rows nobody can find is the third number lying about
    /// itself.
    pub fn record_embedding_failure(&self, space_id: i64, chunk_id: i64) -> Result<bool, Error> {
        let written = self.conn().execute(
            "INSERT INTO chunk_embedding_state (space_id, chunk_id, content_hash, state, attempts)
             SELECT ?1, ?2, c.content_hash, 2, 1 FROM chunk c WHERE c.id = ?2
             ON CONFLICT(space_id, chunk_id) DO UPDATE
               SET content_hash = excluded.content_hash,
                   state        = 2,
                   attempts     = attempts + 1",
            params![space_id, chunk_id],
        )?;
        Ok(written > 0)
    }

    /// Whether every chunk that exists has a vector in this space — the one
    /// question [`Db::mark_space_ready`] is decided by (D95b, fix round 1).
    ///
    /// **Not the queue's question, and deliberately wider.**
    /// [`Db::chunks_needing_embedding`]'s queue is right to ignore a chunk
    /// behind a document whose `status` is not yet `'indexed'` — its own
    /// comment argues that case, and this method does not relitigate it. But
    /// "the queue found nothing to do" and "the space is complete" used to be
    /// treated as the same fact, and they are not: a document can have chunks
    /// written and no `'indexed'` status yet — the window
    /// `crates/mnema-ingest/src/lib.rs:546-598`'s own comment names, between
    /// writing chunks and writing the status, kept a separate transaction on
    /// purpose so a crash there "costs a re-index rather than a lie" — and
    /// those chunks exist, right now, with no vector, invisible to a queue
    /// that is asking a narrower question than this one.
    ///
    /// Same scope as [`Db::failed_chunk_count`]: every chunk that exists,
    /// whatever its document's status — not a third copy of that predicate,
    /// but the same one asked from the vector side instead of the failure
    /// side. A chunk this space gave up on has no vector by construction
    /// (nothing writes both a `failed` row and a vector for the same chunk),
    /// so this single question already answers what used to take two: no
    /// carve-out for a failed chunk is needed or added, and a space with one
    /// permanently refused chunk stays `building` for ever. That is correct,
    /// not a bug this state should hide — the space genuinely is not
    /// complete, and `failed_chunk_count` is what tells a person why, once
    /// something reads it.
    ///
    /// **This depends on `NOT IN` never meeting a NULL, the same argument
    /// [`the_embedding_queue`] makes for the same SQL shape.** A single NULL
    /// among `{table}.chunk_id` would make `NOT IN` answer NULL for every
    /// row, and `NOT EXISTS` of an always-NULL comparison is true — this
    /// method would then answer `true` over a space full of chunks with no
    /// vector, silently, which is the one direction this predicate must never
    /// be wrong in. It cannot happen, for the same reason it cannot for the
    /// queue: `chunk_id` is `INTEGER PRIMARY KEY` on the `vec0` table
    /// (`space.rs:220`), and every writer binds it a real `i64`.
    pub fn space_is_complete(&self, space_id: i64) -> Result<bool, Error> {
        let table = self.space(space_id)?.table;
        // `table` is never caller text: the same reasoning `embedded_chunk_count`
        // and `knn` already rely on for interpolating a table name into SQL.
        Ok(self.conn().query_row(
            &format!(
                "SELECT NOT EXISTS (
                     SELECT 1 FROM chunk c
                      WHERE c.id NOT IN (SELECT chunk_id FROM {table})
                 )"
            ),
            [],
            |r| r.get(0),
        )?)
    }

    /// Marks a space as holding a vector for every chunk it currently owes
    /// one — `ready`, one of the three values the schema's own `CHECK` on
    /// `embedding_space.state` allows (`schema.sql:356`) beside `building`
    /// and the `stale` nothing writes yet.
    ///
    /// A write, not a decision: `mnema_embed::run` is the one caller (D95b),
    /// and only once [`Db::space_is_complete`] has answered `true`. Not
    /// re-read here, so this cannot come to disagree with the fact that
    /// justified calling it.
    ///
    /// An id with no row is [`Error::NoSuchSpace`], the same answer every
    /// other space method here gives for one, rather than a silent no-op: an
    /// `UPDATE` matching zero rows is not an error SQLite raises on its own,
    /// and a caller holding a stale id deserves to be told which one was
    /// wrong rather than believe a write happened that did not.
    pub fn mark_space_ready(&self, space_id: i64) -> Result<(), Error> {
        let changed = self.conn().execute(
            "UPDATE embedding_space SET state = 'ready' WHERE id = ?1",
            params![space_id],
        )?;
        if changed == 0 {
            return Err(Error::NoSuchSpace(space_id));
        }
        Ok(())
    }

    /// Retracts the claim [`Db::mark_space_ready`] makes. `ready` says every
    /// chunk in the space has a vector, and a chunk that exists without one
    /// makes that false from the moment it exists — not from the moment some
    /// later run gets back around to embedding it. Without a way back, `state`
    /// could only ever move one direction, which is the lie this space was
    /// already in before D95b, arriving later and more convincingly: a screen
    /// reading the column would go on saying "complete" about an archive a
    /// second document had already outgrown.
    ///
    /// `mnema_embed::run` calls this unconditionally whenever it starts against
    /// a non-empty queue — not only when the row currently says `ready` —
    /// because checking first would make this crate a reader of a column it
    /// has no other need to read: the queue being non-empty already is the
    /// whole of the fact that matters, and writing `building` over a space
    /// that already says `building` costs nothing to be wrong about.
    ///
    /// ⚠️ Unconditional in the literal sense: this retracts *any* current
    /// value, `'stale'` included. Nothing writes `'stale'` today, so there is
    /// nothing this can erase yet — but the day something does, the next
    /// `run` against a non-empty queue overwrites it silently, with no
    /// carve-out and no warning.
    ///
    /// Same [`Error::NoSuchSpace`] convention as [`Db::mark_space_ready`].
    pub fn mark_space_building(&self, space_id: i64) -> Result<(), Error> {
        let changed = self.conn().execute(
            "UPDATE embedding_space SET state = 'building' WHERE id = ?1",
            params![space_id],
        )?;
        if changed == 0 {
            return Err(Error::NoSuchSpace(space_id));
        }
        Ok(())
    }

    /// The rule below, asked only of a call that would actually move the
    /// pointer — and asked of every call that would.
    ///
    /// A call whose destination is where the pointer already stands writes
    /// `active_space` with the value it already holds. It is not a transition,
    /// so there is nothing for a guard on transitions to decide: whatever was
    /// true of the database before it is still true after. Refusing there
    /// refused the middle of the migration this function's own documentation
    /// calls legal — the new space built and filled, the old one not yet
    /// dropped — and, since adoption is the only path that writes
    /// `credential_ref`, it made the API key unchangeable in exactly that
    /// state. It also told a caller who was not moving that the index could not
    /// move.
    ///
    /// ⚠️ `requested.is_some()` is load-bearing and not defensive. With it
    /// dropped, two `None`s compare equal — a fresh index with no pointer,
    /// asking for a space that does not exist yet — and the check would be
    /// skipped on precisely the call that mints the first space beside an
    /// already-full one. That is C1 reopened from the other side.
    ///
    /// This is the only place the pointer takes any part in the decision, and
    /// it is sound because the premise is decidable from the very value about
    /// to be written: [`Db::active_space`] is `parse(stored)` and the write is
    /// `space_id.to_string()`, so if the parse already equals `space_id` then
    /// `stored` is already that id in decimal and the write changes at most its
    /// spelling. Checked at the edges: `"+1"` parses to `Some(1)` and is the
    /// same space, `" 1"` does not parse and the rule is asked.
    ///
    /// ⚠️ **That argument dies the moment [`Db::active_space`] gains a
    /// fallback.** Something like "if the key is absent, use the only space
    /// there is" would be plausible and well meant, and it would make this
    /// exemption unsound: the premise would no longer be about the stored
    /// value, and a call that really does move the pointer from "nothing
    /// chosen" onto a space — while another one is full — would be waved
    /// through. `active_space` carries the same warning from its side.
    fn refuse_if_the_move_would_orphan_anything(
        &self,
        requested: Option<i64>,
    ) -> Result<(), Error> {
        if requested.is_some() && requested == self.active_space()? {
            return Ok(());
        }
        self.refuse_unless_every_other_space_is_empty(requested)
    }

    /// Refuses the move unless **every space except the requested one** is
    /// empty. The one rule, so that the two places which ask it cannot decide
    /// it differently.
    ///
    /// It asks what exists. It used to ask `meta.active_space`, and that was
    /// the wrong question by a whole class: the pointer is written by
    /// [`Db::adopt_embedding_model`] alone, while [`Db::create_space`] and
    /// [`Db::insert_vector`] are public and take a `space_id` parameter — so a
    /// full space that nothing points at is reachable through this crate's
    /// typed API with no raw SQL anywhere. Keyed on the pointer, the guard read
    /// `None`, ran no check at all, and let the index move off that space in
    /// silence. Measured, and the whole sequence is four public calls:
    /// `create_model_config`, `create_space`, `insert_vector`,
    /// `adopt_embedding_model`.
    ///
    /// Asking what exists removes two special cases instead of adding one. An
    /// absent pointer and an unreadable one stop differing, because neither is
    /// consulted. And a pointer left dangling by [`Db::drop_space`] repairs
    /// itself for free: the dropped space is no longer a row in
    /// `embedding_space`, so it is not among the ids asked about, nothing
    /// blocks, and the adoption that meets it rewrites the key.
    ///
    /// `requested` is skipped rather than counted, because adopting the model
    /// an index is already full of is the ordinary case and not a switch. It is
    /// `Option` because the space may not exist yet, and "no space asked for"
    /// then means every existing space has to be empty — which is right: a
    /// brand-new space is being minted, and anything already filled would be
    /// left behind by it.
    ///
    /// Whether this is asked at all is [`Db::refuse_if_the_move_would_orphan_anything`]'s
    /// question, and the two are separate functions because they are two
    /// contracts: this one is what the rule is, that one is who it applies to.
    fn refuse_unless_every_other_space_is_empty(
        &self,
        requested: Option<i64>,
    ) -> Result<(), Error> {
        let ids: Vec<i64> = self
            .conn()
            .prepare("SELECT id FROM embedding_space")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for space_id in ids {
            if Some(space_id) == requested {
                continue;
            }
            let embedded_chunks = match self.embedded_chunk_count(space_id) {
                Ok(n) => n,
                // Dropped between the listing and the count — the pre-flight
                // holds no lock, so the window is real, if microscopic. It is
                // inside the race this function's caller already declares, but
                // the outcome would be new: `NoSuchSpace(N)` handed to a caller
                // who named no space at all, about one that no longer exists.
                // The question here is "does any other space hold embeddings",
                // and a space that is gone is not one that does — its vectors
                // went with its table and its bookkeeping cascaded. So this is
                // the answer "nothing", not a swallowed error.
                //
                // Scoped to this loop deliberately. `space_is_empty` still
                // propagates, because there the caller did name the id, and
                // being told which one was wrong is the whole answer.
                //
                // Nothing exercises this line, and that is worth knowing before
                // relying on it: deleting it leaves the whole crate green —
                // measured, not assumed. A test would have to land a committed
                // `drop_space` between the `SELECT` above and the count below,
                // inside one call. That is not impossible:
                // `rusqlite::Connection::progress_handler` would give a seam
                // inside the `SELECT`, and no lock is in the way, since this
                // check holds none. It is machinery that exists
                // nowhere in this repository, built to witness a `continue`
                // against a microscopic window, so the choice is to say this
                // rather than to build it.
                Err(Error::NoSuchSpace(_)) => continue,
                Err(other) => return Err(other),
            };
            if embedded_chunks > 0 {
                return Err(Error::SpaceNotEmpty {
                    space_id,
                    embedded_chunks,
                });
            }
        }
        Ok(())
    }

    /// Records the chosen embedding model and makes its space the active one.
    ///
    /// One embedding model per index (spec §2.1). Choosing the same model again
    /// finds the space rather than minting a second one — [`Db::create_space`]
    /// is idempotent on `UNIQUE(model_config_id, dim, index_format_version,
    /// chunker_hash)` (`schema.sql:358`) and hands back the id it found.
    /// A call that would **move the index onto a different space** is refused
    /// while any other space holds anything: honouring it would leave the
    /// archive split across two spaces, only one of which search reads. The
    /// honest way to grant it is to build the new space, fill it, and then
    /// switch, and that is the indexing subsystem's work rather than this
    /// function's.
    ///
    /// "Would move" and not "names a different model", because those are not
    /// the same set and the difference is the middle of that very migration: a
    /// new space built and filled while the old one is still there. Re-adopting
    /// the model the index is **already on** moves nothing, so it is allowed
    /// there — which is also the only way to update `credential_ref`, this
    /// being the one path that writes it. A different *chunker* under the same
    /// model does move the index, and is refused like any other move.
    ///
    /// So what is guaranteed is about **transitions and not about states**: no
    /// adoption ever moves the index onto a space while another space holds
    /// embeddings. The stronger reading — "after any successful adoption only
    /// the active space holds anything" — is not true and must not be relied
    /// on, because the migration reaches two full spaces legitimately, through
    /// [`Db::insert_vector`] into a space that is not the active one, and no
    /// adoption took part in getting there. The obligation that follows from
    /// that — the embedder writes into [`Db::active_space`] — is stated on
    /// [`Db::insert_vector`], which is the call that can break it and so the
    /// doc its breaker is reading.
    ///
    /// What the refusal asks is what exists rather than what
    /// `meta.active_space` says, and `refuse_unless_every_other_space_is_empty`
    /// is where that is argued — the pointer version had a hole reachable from
    /// four public calls.
    ///
    /// `chunker_hash` arrives as a parameter and not as a call: this crate does
    /// not depend on `mnema-chunk` and is not to start. The shell supplies it,
    /// as the tests do.
    ///
    /// **What is atomic and what is not.** Three transactions, not one, and it
    /// cannot be one: `create_space` opens its own IMMEDIATE transaction and
    /// SQLite has no nested `BEGIN`.
    ///
    /// - The refusal is decided **twice**, and the two are not redundant. Once
    ///   before anything is written, so a refusal leaves behind no
    ///   configuration row, no space row, no `vec0` table and no advanced id
    ///   counter — writes that a refusal implies did not happen. Once again
    ///   inside the transaction that writes `active_space`, where it has to be:
    ///   counting and then repointing is check-then-act, and only holding the
    ///   write lock across both stops another connection inserting a vector in
    ///   between. In one thread the two are indistinguishable, which is why the
    ///   second has a two-connection test of its own
    ///   (`tests/adopt.rs`, `a_vector_written_while_a_switch_is_deciding_still_refuses_it`)
    ///   rather than a note admitting nothing exercises it. What makes that
    ///   test's red attributable to *this* check is its count of
    ///   `embedding_space`: both checks raise the same error, and only the
    ///   second one has a space already created behind it. The timing
    ///   assertion in it shows the interleave happened as designed; it is not
    ///   what carries the proof.
    /// - The model configuration and the space are written **outside** that
    ///   transaction. Interrupted there, the index keeps a space nothing points
    ///   at and a configuration nothing uses. Neither is reachable by search,
    ///   neither is data lost, and the next call with the same arguments adopts
    ///   both instead of duplicating them.
    /// - A refusal from the **second** check leaves that same debris, and this
    ///   is the one case where a refusal is not clean. Saying it plainly rather
    ///   than through the race below: by then the space exists, and unwinding
    ///   it would be a `DROP` in a transaction whose whole purpose is to have
    ///   changed nothing.
    /// - Left racy, deliberately and not by omission: two callers adopting two
    ///   different models at the same instant can both clear the pre-flight
    ///   check, and the loser's space becomes that debris. The caller is a
    ///   person choosing a model in a settings window.
    pub fn adopt_embedding_model(
        &self,
        model: &str,
        dim: i64,
        credential_ref: &str,
        chunker_hash: &str,
    ) -> Result<AdoptedSpace, Error> {
        let existing_config = self.model_config_for(model)?;

        // The width is checked here, and `create_space` checks it again, and
        // that is not duplication: the refusing path never reaches
        // `create_space`. Without this, a width disagreeing with the recorded
        // configuration made `requested` `None` — no space has that key — and
        // the caller was then told the *embedding model* could not be changed,
        // a cause nobody had raised. Measured before the fix: the same call
        // answered `SpaceDimMismatch` on an empty index and
        // `ActiveSpaceNotEmpty` on a full one.
        if let Some(config) = existing_config
            && config.dim != dim
        {
            return Err(Error::SpaceDimMismatch {
                model_config_id: config.id,
                config_dim: config.dim,
                space_dim: dim,
            });
        }

        // Which space is being asked for is answerable without creating it: no
        // configuration for this model means no space for it either.
        let requested = match existing_config {
            Some(config) => existing_space_id(self.conn(), config.id, dim, chunker_hash)?,
            None => None,
        };
        // Refuse before writing rather than after, so that a refusal leaves
        // nothing behind.
        self.refuse_if_the_move_would_orphan_anything(requested)?;

        let model_config_id = match existing_config {
            Some(config) => config.id,
            // The provider is a constant because v1 has exactly one (spec §2.2,
            // `mnema-provider/src/lib.rs:21`); the name is the model's own
            // because there is nothing else here to call it. The secret is not
            // a parameter and never will be — `credential_ref` names an entry
            // in the OS credential store.
            None => self.create_model_config(model, "openrouter", None, model, dim)?,
        };
        self.conn().execute(
            "UPDATE model_config SET credential_ref = ?2 WHERE id = ?1",
            params![model_config_id, credential_ref],
        )?;

        // `create_space` reports an existing space as an *error* carrying its
        // id, because the caller it was built for wanted get-or-create. That
        // error is the only place the difference between "found" and "created"
        // is stated, so `created` is read from here rather than inferred from
        // whether the active space moved — which is a different fact, and one
        // that answers "created" for a space that was already there.
        let (space_id, created) = match self.create_space(model_config_id, dim, chunker_hash) {
            Ok(id) => (id, true),
            Err(Error::SpaceAlreadyExists { space_id }) => (space_id, false),
            Err(other) => return Err(other),
        };

        self.transaction(|_tx| {
            // Asked again under the write lock, and now the destination is the
            // id `create_space` actually settled on rather than the one looked
            // up before it ran. Another connection can fill a space between the
            // check above and this one; nothing can fill one between this and
            // the write below.
            self.refuse_if_the_move_would_orphan_anything(Some(space_id))?;
            // `meta_put` and not `meta_set`: `meta_set` refuses this key
            // precisely so that it is written here, by the one path that can
            // first ask what replacing it would orphan.
            self.meta_put(crate::META_ACTIVE_SPACE, &space_id.to_string())?;
            Ok(())
        })?;

        Ok(AdoptedSpace {
            space_id,
            model_config_id,
            created,
        })
    }

    /// The configuration this crate recognises for a model name, if one is
    /// recorded.
    ///
    /// Carries the width as well as the id because the adoption path has to
    /// compare it before it can say what a mismatch is about; fetching the id
    /// alone is what left that question to a function the refusing path never
    /// reaches.
    fn model_config_for(&self, model: &str) -> Result<Option<ModelConfigRef>, Error> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id, dim FROM model_config WHERE embed_model = ?1",
                params![model],
                |r| {
                    Ok(ModelConfigRef {
                        id: r.get(0)?,
                        dim: r.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    /// Resolves a space id to what every vector operation needs, or says which id
    /// was wrong. Read straight through `query_row` an unknown id arrives as
    /// `QueryReturnedNoRows`, which reads like an empty index rather than a bug.
    fn space(&self, space_id: i64) -> Result<SpaceRef, Error> {
        self.conn()
            .query_row(
                "SELECT vec_table, metric FROM embedding_space WHERE id = ?1",
                params![space_id],
                |r| {
                    Ok(SpaceRef {
                        table: r.get(0)?,
                        metric: r.get(1)?,
                    })
                },
            )
            .optional()?
            .ok_or(Error::NoSuchSpace(space_id))
    }
}

/// [`Db::delete_vectors_for_document`]'s own DELETE loop, taken out from under
/// `&self` so it can run against either a bare connection or a transaction a
/// caller already has open.
///
/// Takes `&rusqlite::Connection` rather than `&Db`, the same reason
/// `write_search_row` in `search.rs` does: `rusqlite::Transaction` derefs to
/// `Connection`, so a `&Transaction` coerces at the call site and this one
/// body serves both callers. `Db::delete_vectors_for_document` passes
/// `self.conn()` and joins whatever transaction (if any) is already open on
/// it — mnema-ingest's `forget_if_unnamed` calls it from inside one today, and
/// nothing here may assume it is the only writer, so it must not open a
/// transaction of its own. `Db::delete_watched_root` passes its own open
/// transaction directly, so the sweep and the document's deletion land or
/// roll back together.
pub(crate) fn delete_vectors_for_document_in(
    conn: &rusqlite::Connection,
    document_id: &str,
) -> Result<(), Error> {
    let tables: Vec<String> = conn
        .prepare("SELECT vec_table FROM embedding_space")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for table in tables {
        // `table` is never caller text: it comes only from
        // `embedding_space.vec_table`, which the schema's own CHECK pins to
        // `'vec_emb_' || id` and only `create_space` ever writes — the same
        // reasoning `knn` and `insert_vector` already rely on for
        // interpolating a table name into SQL.
        conn.execute(
            &format!(
                "DELETE FROM {table}
                  WHERE chunk_id IN (SELECT id FROM chunk WHERE document_id = ?1)"
            ),
            params![document_id],
        )?;
    }
    Ok(())
}

/// The id of the space with exactly this key, if there is one.
///
/// Written once and asked from both sides on purpose. `create_space` asks it in
/// order to hand back the existing id instead of colliding;
/// `adopt_embedding_model` asks it *before writing anything*, to learn whether
/// the model being chosen is the one already active. A second copy of the
/// UNIQUE key (`schema.sql:358`) would be a second place to update, and the one
/// that fell behind would not fail loudly — the adoption path would quietly
/// stop recognising the active space as the requested one and start refusing
/// switches that are not switches.
///
/// Takes `&Connection` so a `&Transaction` coerces at the call site, the same
/// reason `delete_vectors_for_document_in` does.
fn existing_space_id(
    conn: &rusqlite::Connection,
    model_config_id: i64,
    dim: i64,
    chunker_hash: &str,
) -> Result<Option<i64>, Error> {
    Ok(conn
        .query_row(
            "SELECT id FROM embedding_space
              WHERE model_config_id = ?1 AND dim = ?2
                AND index_format_version = ?3 AND chunker_hash = ?4",
            params![model_config_id, dim, INDEX_FORMAT_VERSION, chunker_hash],
            |r| r.get::<_, i64>(0),
        )
        .optional()?)
}

/// A model configuration, as much of it as the adoption path asks for.
#[derive(Debug, Clone, Copy)]
struct ModelConfigRef {
    id: i64,
    dim: i64,
}

/// What a space is, to the code that reads and writes its vectors.
struct SpaceRef {
    table: String,
    metric: String,
}

/// Which side of a vector operation a rejected vector came from, so the error
/// can name the chunk whose embedding was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorRole {
    /// An embedding offered for storage against this chunk.
    Stored(i64),
    /// The query side of a nearest-neighbour search.
    Query,
}

impl std::fmt::Display for VectorRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stored(chunk_id) => write!(f, "the embedding for chunk {chunk_id}"),
            Self::Query => write!(f, "the query vector"),
        }
    }
}

/// Refuses a vector vec0 would accept and then fail to rank.
///
/// Three faults, all measured against sqlite-vec 0.1.9, and they do not fail in
/// the same direction:
///
/// - a non-finite component, or an all-zero vector, gives `distance = NULL`,
///   which SQLite sorts first ascending;
/// - a squared norm that underflows f32 gives `distance = -inf`, which also
///   sorts first and is *not* caught by `NULLS LAST`. Magnitude 1e-25 measured
///   at -inf, 1e-20 at -2.7e-6, both ahead of an exact match at 0.0;
/// - a squared norm that *overflows* f32 gives a finite, confident, wrong
///   `1.0` — the distance of an unrelated vector. Measured: a vector parallel
///   to the query at magnitude 1e20 is reported at 1.0 instead of 0.0. This is
///   the quietest of the three and the worst: nothing is null, nothing is out
///   of order, the exact match is simply buried, and no ordering rule
///   downstream can notice.
///
/// The norm test is a **conservative superset** of vec0's failure set, not a
/// characterisation of it. vec0 only truly breaks once the squared norm reaches
/// zero, so this refuses vectors it would have ranked correctly — one component
/// at 1e-21 gets a perfectly good +2.6e-4. Refusing them is still right, because
/// inside the subnormal band the error changes sign with no pattern to key on:
/// 1e-19 → -7.4e-10, 1e-20 → -2.7e-6, 1e-21 → **+**2.6e-4, 1e-22 → -9.7e-3. The
/// usable magnitudes are not separable from the broken ones, so the band goes
/// whole.
///
/// The norm is summed *and compared* in f32, because that is the width vec0
/// divides in, and it is load-bearing rather than incidental: compared in f64
/// the underflow this exists to catch does not happen and the guard waves
/// through exactly the vectors it is here to stop. The comparison is the half
/// that matters — summing wide and narrowing on the way out behaves
/// identically, since the cast back to f32 underflows in the same place.
///
/// The norm test applies only under cosine — with L2 a zero vector is an
/// ordinary point, measured at a real distance. A non-finite component is
/// refused under any metric: it is a corrupt embedding whatever the ranking
/// does with it.
///
/// What this deliberately does NOT chase: for two genuinely near-parallel
/// vectors vec0's f32 arithmetic wanders either side of zero by about 1e-8, so
/// a vector at -2e-9 can edge out an exact match at 0.0. That is noise at the
/// scale where the two really are the same distance, not a wrong answer.
fn check_rankable(v: &[f32], metric: &str, role: VectorRole) -> Result<(), Error> {
    if let Some(index) = v.iter().position(|f| !f.is_finite()) {
        return Err(Error::NonFiniteVector { role, index });
    }
    if metric != "cosine" {
        return Ok(());
    }
    // Summed in f32 because that is the width vec0 divides in; in f64 the
    // underflow this exists to catch would not happen.
    let squared_norm: f32 = v.iter().map(|f| f * f).sum();
    if !squared_norm.is_finite() || squared_norm < f32::MIN_POSITIVE {
        return Err(Error::UnrankableVector { role, squared_norm });
    }
    Ok(())
}

fn first_id(row: &rusqlite::Row<'_>) -> rusqlite::Result<i64> {
    row.get(0)
}

/// The vector table's name: `vec_emb_` and the id as SQLite renders it.
///
/// Unpadded, because `embedding_space.vec_table` CHECKs the name against
/// `'vec_emb_' || id` and SQLite concatenates an integer without padding. A
/// zero-padded form is rejected by the schema at insert time.
fn vec_table_name(space_id: i64) -> String {
    format!("vec_emb_{space_id}")
}

/// A vector as vec0 stores it: float32, host byte order.
///
/// Host order and not explicitly little-endian because that is what vec0
/// requires: it `memcpy`s the blob into a `float *`, so the bytes are read back
/// in the order the reading machine uses, and writing little-endian on a
/// big-endian host would store garbage.
///
/// This is safe only while every target we ship is little-endian, and it is a
/// standing condition rather than a settled one: §5.6, §9 and D33 exist because
/// one person indexes a folder and the others copy the database, so the file
/// does travel between machines. Adding a big-endian target would silently
/// invalidate every vector table copied from a little-endian one.
fn as_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_ne_bytes()).collect()
}

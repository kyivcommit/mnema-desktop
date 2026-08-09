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

    /// Creates the space row and its vector table, or neither.
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
    pub fn active_space(&self) -> Result<Option<i64>, Error> {
        Ok(self
            .meta_get(crate::META_ACTIVE_SPACE)?
            .and_then(|v| v.parse().ok()))
    }

    /// Whether moving the index off this space would throw anything away.
    ///
    /// Reads **both** places a chunk can be recorded as embedded, because they
    /// are allowed to disagree: a `vec0` table cannot be the target of a
    /// foreign key, so a vector outlives the chunk it embeds and the
    /// bookkeeping row that cascaded away with it. A check that read one of
    /// them would be an assertion satisfied by zero from the wrong side.
    ///
    /// A space that does not exist is **not** empty — it is absent, and that
    /// arrives as [`Error::NoSuchSpace`] rather than as `Ok(true)`. Two facts
    /// leaving through one `bool` is how a caller comes to believe it asked
    /// about something.
    pub fn space_is_empty(&self, space_id: i64) -> Result<bool, Error> {
        Ok(self.embedded_chunk_count(space_id)? == 0)
    }

    /// How many chunks this space has an embedding recorded for — the number a
    /// refusal puts in front of the person deciding whether to rebuild.
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
    /// An id with no row is [`Error::NoSuchSpace`] and not zero. It was zero
    /// for one commit, to keep a `meta.active_space` left dangling by
    /// [`Db::drop_space`] from becoming a dead end — and that special case is
    /// gone with the pointer it served: the refusal below enumerates
    /// `embedding_space`, so a dropped space is simply not among the ids it
    /// asks about. What is left is a public method being asked about an id
    /// nobody wrote, and the honest answer to that is which id was wrong.
    fn embedded_chunk_count(&self, space_id: i64) -> Result<i64, Error> {
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

    /// Refuses the move unless **every space except the requested one** is
    /// empty. The one refusal, so that the two places which decide it cannot
    /// decide it differently.
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
            let embedded_chunks = self.embedded_chunk_count(space_id)?;
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
    /// Choosing a *different* one is refused while **any other space** holds
    /// anything: honouring it would leave the archive split across two spaces,
    /// only one of which search reads. The honest way to grant it is to build
    /// the new space, fill it, and then switch, and that is the indexing
    /// subsystem's work rather than this function's. What the refusal asks is
    /// what exists rather than what `meta.active_space` says, and
    /// [`Db::refuse_unless_every_other_space_is_empty`] is where that is
    /// argued — the pointer version had a hole reachable from four public
    /// calls.
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
    ///   rather than a note admitting nothing exercises it.
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
        self.refuse_unless_every_other_space_is_empty(requested)?;

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
            // Asked again under the write lock, and now the requested space is
            // known exactly rather than looked up. Another connection can fill
            // a space between the check above and this one; nothing can fill
            // one between this and the write below.
            self.refuse_unless_every_other_space_is_empty(Some(space_id))?;
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

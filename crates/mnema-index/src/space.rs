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
        if let Some(space_id) = tx
            .query_row(
                "SELECT id FROM embedding_space
                  WHERE model_config_id = ?1 AND dim = ?2
                    AND index_format_version = ?3 AND chunker_hash = ?4",
                params![model_config_id, dim, INDEX_FORMAT_VERSION, chunker_hash],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
        {
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

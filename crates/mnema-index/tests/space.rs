//! Embedding spaces: one `vec0` table per (model, dimension, index format,
//! chunker), created lazily and never altered afterwards.
//!
//! Several of these tests are written against a failure mode that returns *no
//! rows* rather than an error, which is the same answer a correct query gives
//! when there are no neighbours. Wherever that is possible the assertion names a
//! specific id at a specific rank, and the fixture puts nearer, excluded
//! vectors in the way — so a filter that is silently ignored, or applied after
//! the k cut instead of before it, produces a different list rather than a
//! shorter one.

use mnema_core::{Block, BlockType, Coordinate, Locator, Segment, SourceKind};
use mnema_index::{Db, DocumentStatus, open, register_vector_extension};

mod support;

fn fresh(dir: &tempfile::TempDir) -> Db {
    register_vector_extension().unwrap();
    open(&dir.path().join("index.sqlite")).unwrap()
}

fn vec_of(n: usize, seed: f32) -> Vec<f32> {
    (0..n).map(|i| ((i as f32) * 0.001 + seed).sin()).collect()
}

/// A vector pointing along axis 0, tilted into axis 1 by `tilt`. Under both
/// cosine and L2, a larger tilt is strictly further from `tilted(n, 0.0)`, so
/// the rankings asserted below are arithmetic rather than luck.
fn tilted(n: usize, tilt: f32) -> Vec<f32> {
    let mut v = vec![0.0; n];
    v[0] = 1.0;
    v[1] = tilt;
    v
}

/// The vec0 tables themselves, without the four shadow tables each one brings
/// (`_chunks`, `_info`, `_rowids`, `_vector_chunks00`) — those are counted where
/// they matter, in the drop test.
fn vector_tables(db: &Db) -> Vec<String> {
    db.conn()
        .prepare(
            "SELECT name FROM sqlite_master
              WHERE name LIKE 'vec_emb_%' AND sql LIKE 'CREATE VIRTUAL TABLE%'
              ORDER BY name",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn a_space_is_created_lazily_and_searchable() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let cfg = db
        .create_model_config("default", "openrouter", None, "baai/bge-m3", 1024)
        .unwrap();
    // Lazily: the model configuration alone must not have built anything.
    assert_eq!(vector_tables(&db), Vec::<String>::new());

    let space = db.create_space(cfg, 1024, "chunker-v1").unwrap();
    assert_eq!(vector_tables(&db), vec![format!("vec_emb_{space}")]);

    for id in 1..=5i64 {
        db.insert_vector(space, id, &vec_of(1024, id as f32))
            .unwrap();
    }

    let hits = db.knn(space, &vec_of(1024, 3.0), 3, None).unwrap();
    assert_eq!(hits.len(), 3);
    assert_eq!(hits[0], 3, "the nearest vector is the one we queried with");
}

#[test]
fn dimensionality_is_a_property_of_the_space_not_a_constant() {
    // D30: 1024 is a default, not a schema constant. A second space at 1536 must
    // coexist, because that is the hook a code-specific model would use.
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let a = db
        .create_model_config("prose", "openrouter", None, "baai/bge-m3", 1024)
        .unwrap();
    let b = db
        .create_model_config("code", "mistral", None, "codestral-embed", 1536)
        .unwrap();
    let sa = db.create_space(a, 1024, "chunker-v1").unwrap();
    let sb = db.create_space(b, 1536, "chunker-v1").unwrap();

    db.insert_vector(sa, 1, &vec_of(1024, 1.0)).unwrap();
    db.insert_vector(sb, 1, &vec_of(1536, 1.0)).unwrap();

    assert_eq!(db.knn(sa, &vec_of(1024, 1.0), 1, None).unwrap(), vec![1]);
    assert_eq!(db.knn(sb, &vec_of(1536, 1.0), 1, None).unwrap(), vec![1]);

    // Both spaces hold a chunk 1, so the two assertions above would also pass if
    // the spaces shared one table. These say they do not: two tables, and each
    // refuses the other's shape. Without them the test survives a `create_space`
    // that ignores `dim` entirely.
    assert_eq!(
        vector_tables(&db),
        vec![format!("vec_emb_{sa}"), format!("vec_emb_{sb}")]
    );
    assert!(
        db.insert_vector(sa, 2, &vec_of(1536, 2.0)).is_err(),
        "the 1024 space must refuse a 1536 vector"
    );
    assert!(
        db.knn(sa, &vec_of(1536, 1.0), 1, None).is_err(),
        "a query of the wrong width must fail rather than return no neighbours"
    );
}

#[test]
fn a_wrong_sized_vector_is_rejected_loudly() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 1024)
        .unwrap();
    let space = db.create_space(cfg, 1024, "chunker-v1").unwrap();

    let err = db
        .insert_vector(space, 1, &vec_of(768, 1.0))
        .expect_err("vec0 must reject a dimension mismatch rather than truncate");
    // Naming the message keeps the test from passing on some unrelated failure —
    // a missing table, a space id that does not exist — which is what a bare
    // `is_err()` would accept.
    assert!(
        err.to_string().contains("Dimension mismatch"),
        "expected a dimension complaint, got: {err}"
    );

    // And the right width goes in, so the rejection above was about the width.
    db.insert_vector(space, 1, &vec_of(1024, 1.0))
        .expect("the declared width is accepted");
    assert_eq!(db.knn(space, &vec_of(1024, 1.0), 1, None).unwrap(), vec![1]);
}

#[test]
fn knn_can_be_restricted_to_a_set_of_chunks() {
    // This is what a tag filter compiles down to, and it only works because
    // chunk_id is the vector table's primary key. G7.0 §1.3.
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 1024)
        .unwrap();
    let space = db.create_space(cfg, 1024, "chunker-v1").unwrap();
    // Ordered by construction: chunk 1 is nearest to the query, chunk 5 furthest.
    for id in 1..=5i64 {
        db.insert_vector(space, id, &tilted(1024, 0.01 * id as f32))
            .unwrap();
    }
    let query = tilted(1024, 0.0);

    let hits = db.knn(space, &query, 5, Some(&[2, 4])).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|id| *id == 2 || *id == 4));

    // The three cases the assertion above cannot see, because with k >= the row
    // count a filter applied *after* the k cut gives the same answer as one
    // applied before it:
    //
    // 1. k smaller than the number of excluded but nearer vectors. Post-filtering
    //    would return nothing here, having spent all of k on chunks 1 and 2.
    assert_eq!(
        db.knn(space, &query, 2, Some(&[3, 4, 5])).unwrap(),
        vec![3, 4]
    );
    // 2. A single id. SQLite rewrites a one-element `IN` list into `=`, which vec0
    //    does not treat as a KNN pre-filter at all — the query then answers with
    //    silence, and only for filters of exactly one chunk.
    assert_eq!(db.knn(space, &query, 1, Some(&[4])).unwrap(), vec![4]);
    // 3. Restricting to nothing is not the same request as not restricting.
    assert_eq!(
        db.knn(space, &query, 5, Some(&[])).unwrap(),
        Vec::<i64>::new()
    );
    assert_eq!(db.knn(space, &query, 5, None).unwrap(), vec![1, 2, 3, 4, 5]);
}

#[test]
fn no_model_mode_is_a_valid_database() {
    // No space, no vector table, lexical half fully functional.
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let n: i64 = db
        .conn()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name LIKE 'vec_emb_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
    assert_eq!(db.search_lexical("anything", 10).unwrap().len(), 0);

    // An empty database answers nothing to every query, so the line above holds
    // just as well when lexical search is broken. Index a chunk and find it.
    let doc = db
        .insert_document(&"a".repeat(64), "text/plain", 12, SourceKind::Document)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    let block = db
        .insert_block(
            page,
            &Block {
                block_type: BlockType::Paragraph,
                reading_order: 0,
                language: Some("uk".into()),
                text: "кошторис на ремонт".to_string(),
                line_start: None,
                line_end: None,
            },
        )
        .unwrap();
    let chunk = db
        .insert_chunk(
            &doc,
            0,
            "кошторис на ремонт",
            &Locator {
                spans: vec![Segment {
                    block_id: block,
                    start: 0,
                    end: 18,
                    block_start: 0,
                }],
                coordinate: Coordinate::None,
            },
            SourceKind::Document,
        )
        .unwrap();
    // The last act of an indexing job. Without it the document stays `pending`
    // and D61's predicate declines to answer with it, which would fail this
    // test for a reason that has nothing to do with embedding spaces.
    db.set_document_status(&doc, DocumentStatus::Indexed)
        .unwrap();

    assert_eq!(
        db.search_lexical("кошторис", 10).unwrap(),
        vec![chunk],
        "the lexical arm must work with no embedding space in the database"
    );
    assert_eq!(vector_tables(&db), Vec::<String>::new());
}

/// The name is `vec_emb_` followed by the id as SQLite renders it, which is
/// unpadded: `embedding_space.vec_table` CHECKs the name against
/// `'vec_emb_' || id`, and a zero-padded form is rejected by the schema.
#[test]
fn the_vector_table_is_named_after_the_unpadded_space_id() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 8)
        .unwrap();

    let space = db.create_space(cfg, 8, "chunker-v1").unwrap();
    let recorded: String = db
        .conn()
        .query_row(
            "SELECT vec_table FROM embedding_space WHERE id = ?1",
            [space],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(recorded, format!("vec_emb_{space}"));
    assert!(!recorded.contains("vec_emb_0"), "padded: {recorded}");
    assert_eq!(vector_tables(&db), vec![recorded]);
}

/// `embedding_space.metric` says cosine, and vec0 builds an L2 table when the
/// DDL does not say otherwise. The two disagree on the *first* result, not on
/// some tail: a vector parallel to the query but ten times its length is the
/// nearest under cosine and the furthest under L2.
#[test]
fn the_table_ranks_by_the_metric_the_row_claims() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 4)
        .unwrap();
    let space = db.create_space(cfg, 4, "chunker-v1").unwrap();

    let metric: String = db
        .conn()
        .query_row(
            "SELECT metric FROM embedding_space WHERE id = ?1",
            [space],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(metric, "cosine");

    // Same direction as the query, ten times the length: cosine distance 0, L2 9.
    db.insert_vector(space, 1, &[10.0, 0.0, 0.0, 0.0]).unwrap();
    // Off by 45 degrees but close in length: cosine 0.29, L2 0.71.
    db.insert_vector(space, 2, &[0.5, 0.5, 0.0, 0.0]).unwrap();

    assert_eq!(
        db.knn(space, &[1.0, 0.0, 0.0, 0.0], 2, None).unwrap(),
        vec![1, 2],
        "ranked by L2 this is [2, 1]"
    );
}

/// `embedding_space.dim` duplicates `model_config.dim` so that a space keeps its
/// width when the configuration is later edited. They may drift afterwards; they
/// may not start out disagreeing, and no CHECK can say so across two tables.
#[test]
fn a_space_may_not_be_created_at_a_width_its_model_does_not_produce() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 1024)
        .unwrap();

    let err = db
        .create_space(cfg, 768, "chunker-v1")
        .expect_err("768 is not what this model emits");
    let message = err.to_string();
    assert!(
        message.contains("768") && message.contains("1024"),
        "the error must name both widths, got: {message}"
    );
    assert_eq!(vector_tables(&db), Vec::<String>::new());

    let err = db
        .create_space(cfg + 99, 1024, "chunker-v1")
        .expect_err("there is no such model configuration");
    assert!(
        err.to_string().contains("model config"),
        "expected a missing-configuration error, got: {err}"
    );
}

/// A row without its table, or a table without its row, is a space that fails
/// at the first insert instead of at creation. Both halves go in together.
#[test]
fn a_space_that_cannot_be_built_leaves_no_row_behind() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 8)
        .unwrap();
    let first = db.create_space(cfg, 8, "chunker-v1").unwrap();

    // Occupy the name the next space will derive, so the row goes in and the
    // CREATE fails after it.
    db.conn()
        .execute_batch(&format!("CREATE TABLE vec_emb_{} (x)", first + 1))
        .unwrap();

    db.create_space(cfg, 8, "chunker-v2")
        .expect_err("the table name is taken");

    let rows: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM embedding_space", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1, "the failed space left its row behind");
}

/// Dropping a space is the mechanism for a model change. vec0 keeps four shadow
/// tables per vector table; leaving them is what makes the id unusable next time.
#[test]
fn dropping_a_space_removes_its_row_its_table_and_its_shadows() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 8)
        .unwrap();
    let space = db.create_space(cfg, 8, "chunker-v1").unwrap();
    db.insert_vector(space, 1, &vec_of(8, 1.0)).unwrap();

    let named: i64 = db
        .conn()
        .query_row(
            &format!("SELECT count(*) FROM sqlite_master WHERE name LIKE 'vec_emb_{space}%'"),
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(named > 1, "expected shadow tables too, found {named}");

    db.drop_space(space).unwrap();

    let named: i64 = db
        .conn()
        .query_row(
            &format!("SELECT count(*) FROM sqlite_master WHERE name LIKE 'vec_emb_{space}%'"),
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        named, 0,
        "the vector table or its shadows survived the drop"
    );
    let rows: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM embedding_space", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}

/// `create_space` writes `state = 'building'` (space.rs:190) and nothing
/// before D95b ever moved it. `mark_space_ready` and `mark_space_building`
/// are the two writers now, and both directions are exercised here because a
/// state that this crate could only ever push one way would be the same lie
/// the space started in, just arriving later.
#[test]
fn a_space_moves_between_building_and_ready() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("e", "openrouter", None, "baai/bge-m3", 8)
        .unwrap();
    let space = db.create_space(cfg, 8, "chunker-v1").unwrap();
    assert_eq!(support::space_state(&db, space), "building");

    db.mark_space_ready(space).unwrap();
    assert_eq!(support::space_state(&db, space), "ready");

    db.mark_space_building(space).unwrap();
    assert_eq!(support::space_state(&db, space), "building");
}

/// Every entry point takes a space id, and an id that names nothing must say so.
/// Read through `query_row` it arrives as `QueryReturnedNoRows`, which is
/// indistinguishable from an empty index.
#[test]
fn an_unknown_space_is_named_in_the_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    for message in [
        db.insert_vector(404, 1, &vec_of(8, 1.0))
            .unwrap_err()
            .to_string(),
        db.knn(404, &vec_of(8, 1.0), 3, None)
            .unwrap_err()
            .to_string(),
        db.drop_space(404).unwrap_err().to_string(),
        db.mark_space_ready(404).unwrap_err().to_string(),
        db.mark_space_building(404).unwrap_err().to_string(),
    ] {
        assert!(
            message.contains("404") && message.contains("space"),
            "expected the missing space id, got: {message}"
        );
    }
}

/// vec0 answers `distance = NULL` for a vector whose cosine distance is
/// undefined, and SQLite sorts NULLs FIRST ascending — so a plain
/// `ORDER BY distance` hands rank 1 to precisely the meaningless row. vec0's own
/// ordering does not do this; the re-sort creates it.
///
/// Note the shape: the fault is invisible at k = 1 and k = 2 and appears at
/// k = 3, so a test with a small k passes. This one uses the whole table.
#[test]
fn an_undefined_distance_sorts_last_not_first() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 4)
        .unwrap();
    let space = db.create_space(cfg, 4, "chunker-v1").unwrap();

    db.insert_vector(space, 1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
    db.insert_vector(space, 2, &[1.0, 0.1, 0.0, 0.0]).unwrap();
    // Written past `insert_vector`, which now refuses it: the ordering has to
    // hold for a row that got in anyway — an older database, another writer.
    let table: String = db
        .conn()
        .query_row(
            "SELECT vec_table FROM embedding_space WHERE id = ?1",
            [space],
            |r| r.get(0),
        )
        .unwrap();
    let zeros: Vec<u8> = [0.0f32; 4].iter().flat_map(|f| f.to_ne_bytes()).collect();
    db.conn()
        .execute(
            &format!("INSERT INTO {table} (chunk_id, embedding) VALUES (3, ?1)"),
            [zeros],
        )
        .unwrap();

    let query = [1.0f32, 0.0, 0.0, 0.0];
    assert_eq!(
        db.knn(space, &query, 3, None).unwrap(),
        vec![1, 2, 3],
        "the row with no defined distance must sort last, not first"
    );
    // `knn` builds two different statements, and the ordering was wrong in both.
    // Without this the filtered one is defended by nothing.
    assert_eq!(
        db.knn(space, &query, 3, Some(&[1, 2, 3])).unwrap(),
        vec![1, 2, 3],
        "the filtered branch orders by the same rule as the unfiltered one"
    );
}

/// Codex round 2, Finding 2: `ORDER BY distance NULLS LAST` alone leaves a
/// tie to whatever order vec0 returns, which the same finding says can
/// differ between runs. Two chunks holding the *same* vector tie on cosine
/// distance exactly, not approximately. Pinned on order, repeated across
/// several calls, in both the filtered and unfiltered statements — the same
/// two-statement split `an_undefined_distance_sorts_last_not_first` above
/// already has to cover twice.
#[test]
fn a_distance_tie_breaks_by_chunk_id_and_is_stable_across_calls() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 4)
        .unwrap();
    let space = db.create_space(cfg, 4, "chunker-v1").unwrap();

    db.insert_vector(space, 1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
    // Chunks 3 and 2, inserted in that order, so an ordering that happened to
    // follow insertion or ascending-id order by coincidence cannot pass.
    db.insert_vector(space, 3, &[1.0, 0.1, 0.0, 0.0]).unwrap();
    db.insert_vector(space, 2, &[1.0, 0.1, 0.0, 0.0]).unwrap();

    let query = [1.0f32, 0.0, 0.0, 0.0];
    let first = db.knn(space, &query, 3, None).unwrap();
    for _ in 0..5 {
        assert_eq!(
            db.knn(space, &query, 3, None).unwrap(),
            first,
            "a distance tie must resolve the same way on every call"
        );
    }
    assert_eq!(
        first,
        vec![1, 2, 3],
        "tied rows must resolve by chunk_id ascending in the unfiltered statement"
    );

    let first_filtered = db.knn(space, &query, 3, Some(&[1, 2, 3])).unwrap();
    for _ in 0..5 {
        assert_eq!(
            db.knn(space, &query, 3, Some(&[1, 2, 3])).unwrap(),
            first_filtered,
            "a distance tie must resolve the same way on every call in the filtered branch too"
        );
    }
    assert_eq!(
        first_filtered,
        vec![1, 2, 3],
        "tied rows must resolve by chunk_id ascending in the filtered statement too"
    );
}

/// `create_space` reads twice — the duplicate check, then the id counter — and
/// only then writes. A DEFERRED transaction takes no lock at BEGIN, so those
/// reads see a snapshot another writer may already have moved past.
///
/// Observed as lock ordering, not as a race, so there is nothing here to time:
/// hold the writer lock on one connection, then call `create_space` on another
/// with an argument that would fail early for an unrelated reason. IMMEDIATE
/// takes the lock at BEGIN and never reaches the read; DEFERRED reaches it and
/// reports the missing model config instead.
#[test]
fn creating_a_space_takes_its_write_lock_before_it_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");
    register_vector_extension().unwrap();
    let holder = open(&path).unwrap();
    let prober = open(&path).unwrap();
    // rusqlite opens with a five-second busy timeout, which would turn the
    // IMMEDIATE case into a five-second wait and then the same answer.
    prober
        .conn()
        .busy_timeout(std::time::Duration::ZERO)
        .unwrap();

    holder.conn().execute_batch("BEGIN IMMEDIATE;").unwrap();
    holder
        .conn()
        .execute(
            "INSERT INTO model_config (name, provider, embed_model, dim)
             VALUES ('held', 'p', 'm', 8)",
            [],
        )
        .unwrap();

    let err = prober
        .create_space(999_999, 8, "chunker-v1")
        .expect_err("another connection holds the writer lock");
    assert!(
        err.to_string().contains("locked"),
        "the lock must be taken at BEGIN, before the first read; got: {err}"
    );
    assert!(
        !err.to_string().contains("model config"),
        "reaching the model config read means the lock was deferred: {err}"
    );

    holder.conn().execute_batch("ROLLBACK;").unwrap();
}

/// Which guard refused a vector. The two overlap: the squared norm of a vector
/// containing NaN or infinity is itself non-finite, so the norm test alone would
/// reject those too — and a test asserting only "some error" leaves the
/// component-level check unprotected and free to be deleted.
#[derive(Debug, PartialEq)]
enum Refusal {
    NonFinite(usize),
    UnusableNorm,
}

fn refusal(err: &mnema_index::Error) -> Refusal {
    match err {
        mnema_index::Error::NonFiniteVector { index, .. } => Refusal::NonFinite(*index),
        mnema_index::Error::UnrankableVector { .. } => Refusal::UnusableNorm,
        other => panic!("expected a refused vector, got: {other:?}"),
    }
}

/// The rejection above is only half of it: vec0 accepts a degenerate vector
/// happily, and every downstream reader then sees a plausible row. Refuse it at
/// the door, where the provider's response is still identifiable as the cause.
#[test]
fn a_vector_that_cannot_be_ranked_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 4)
        .unwrap();
    let space = db.create_space(cfg, 4, "chunker-v1").unwrap();

    // Ordinary vectors, including one of very small but representable norm, must
    // still go in — a guard that refuses everything would pass the rest of this
    // test on its own.
    for (id, v) in [
        (1i64, [1.0f32, 0.0, 0.0, 0.0]),
        (2, [0.3, -0.4, 0.5, 0.7]),
        (3, [1e-18, 0.0, 0.0, 0.0]),
    ] {
        db.insert_vector(space, id, &v)
            .unwrap_or_else(|e| panic!("{v:?} is a usable vector, but: {e}"));
    }

    // The non-finite components sit away from index 0 so the reported position
    // has to be looked up rather than guessed.
    for (label, v, expected) in [
        ("all zeros", [0.0f32, 0.0, 0.0, 0.0], Refusal::UnusableNorm),
        (
            "negative zeros",
            [-0.0, -0.0, -0.0, -0.0],
            Refusal::UnusableNorm,
        ),
        ("NaN", [1.0, f32::NAN, 0.0, 0.0], Refusal::NonFinite(1)),
        (
            "infinity",
            [1.0, 0.0, f32::INFINITY, 0.0],
            Refusal::NonFinite(2),
        ),
        // Its squared norm underflows f32 to zero, so vec0 divides by it and
        // answers distance = -inf. That is not NULL, it sorts ahead of an exact
        // match, and NULLS LAST cannot see it. Measured, not assumed.
        (
            "a norm that underflows",
            [1e-25, 0.0, 0.0, 0.0],
            Refusal::UnusableNorm,
        ),
        // The other end, and the quietest of the three: the squared norm
        // overflows f32 and vec0 answers a finite, confident, wrong 1.0 — the
        // distance of an unrelated vector. Nothing is null and nothing is out of
        // order, so the exact match is simply buried where no sort can find it.
        (
            "a norm that overflows",
            [1e20, 0.0, 0.0, 0.0],
            Refusal::UnusableNorm,
        ),
    ] {
        let err = db
            .insert_vector(space, 9, &v)
            .expect_err(&format!("{label} must be refused"));
        assert_eq!(
            refusal(&err),
            expected,
            "{label}: refused by the wrong guard"
        );
        assert!(
            err.to_string().contains("chunk 9"),
            "{label}: the error must name the chunk, got: {err}"
        );
        // The same vector is no better as a query: every distance comes back
        // NULL and the answer is an arbitrary k rows with no error anywhere.
        let err = db
            .knn(space, &v, 3, None)
            .expect_err(&format!("{label} must be refused as a query too"));
        assert_eq!(refusal(&err), expected, "{label} as a query");
        assert!(
            err.to_string().contains("query"),
            "{label}: a query-side refusal must say so, got: {err}"
        );
    }
}

/// vec0 caps k at 4096 and says so. Recorded because a tag-filtered "everything
/// under this tag" is the query that will meet it.
#[test]
fn k_above_vec0s_cap_is_loud() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 4)
        .unwrap();
    let space = db.create_space(cfg, 4, "chunker-v1").unwrap();
    db.insert_vector(space, 1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
    let q = [1.0f32, 0.0, 0.0, 0.0];

    assert_eq!(db.knn(space, &q, 4096, None).unwrap(), vec![1]);
    let err = db
        .knn(space, &q, 4097, None)
        .expect_err("4097 is over vec0's cap");
    assert!(
        err.to_string().contains("4096"),
        "expected the cap in the message, got: {err}"
    );
}

/// A dropped space's id must not come back. `max(id) + 1` hands it straight to
/// the next space, along with its table name, so anything holding the old id —
/// a cached handle, a setting — silently addresses the new space.
#[test]
fn a_dropped_space_never_lends_its_id_to_the_next_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 4)
        .unwrap();

    let mut seen = Vec::new();
    for chunker in ["v1", "v2", "v3"] {
        seen.push(db.create_space(cfg, 4, chunker).unwrap());
    }
    // Drop the newest, then everything: `max(id) + 1` reuses after either.
    db.drop_space(seen[2]).unwrap();
    let after_one = db.create_space(cfg, 4, "v4").unwrap();
    assert!(
        !seen.contains(&after_one),
        "id {after_one} was already used: {seen:?}"
    );
    seen.push(after_one);

    for id in [seen[0], seen[1], after_one] {
        db.drop_space(id).unwrap();
    }
    let after_all = db.create_space(cfg, 4, "v5").unwrap();
    assert!(
        !seen.contains(&after_all),
        "id {after_all} was already used: {seen:?}"
    );

    // And the table name follows the id, so it cannot be recycled either.
    assert_eq!(vector_tables(&db), vec![format!("vec_emb_{after_all}")]);
}

/// Indexing wants get-or-create, and matching on "UNIQUE constraint failed" text
/// is what a typed error exists to avoid. The variant carries the existing id,
/// which is the answer the caller was after.
#[test]
fn a_second_space_with_the_same_identity_names_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 4)
        .unwrap();
    let first = db.create_space(cfg, 4, "chunker-v1").unwrap();

    let err = db
        .create_space(cfg, 4, "chunker-v1")
        .expect_err("the identity tuple is already taken");
    assert!(
        matches!(err, mnema_index::Error::SpaceAlreadyExists { space_id } if space_id == first),
        "expected SpaceAlreadyExists({first}), got: {err:?}"
    );

    // A different chunker version is a different space, not a duplicate.
    db.create_space(cfg, 4, "chunker-v2")
        .expect("a new identity tuple");
}

// -------------------------------------------------- upsert and delete (D95a)

/// `insert_vector` refuses a second write for the same chunk; `upsert_vector`
/// is the retry-safe alternative a batch resumed after a partial failure
/// needs. The row count alone is not proof: a delete-then-insert that failed
/// halfway, or one that inserted without deleting, would also leave exactly
/// one row — just the wrong one. The stored vector's content is what tells
/// the two apart.
#[test]
fn upserting_a_vector_replaces_the_previous_one() {
    let db = support::temp_db();
    let space = support::space_1024(&db);
    let chunk = support::one_chunk(&db);

    db.upsert_vector(space, chunk, &support::unit_vector_1024())
        .expect("first");
    db.upsert_vector(space, chunk, &support::other_unit_vector_1024())
        .expect("second");

    assert_eq!(db.embedded_chunk_count(space).expect("count"), 1);
    assert_eq!(
        support::stored_vector(&db, space, chunk),
        support::other_unit_vector_1024(),
        "the second write did not replace the first"
    );
}

/// The other half of retry-safety: a batch that already deleted its half of
/// a failed run must be able to delete it again without first checking
/// whether the row is still there.
#[test]
fn deleting_a_vector_is_idempotent() {
    let db = support::temp_db();
    let space = support::space_1024(&db);
    let chunk = support::one_chunk(&db);
    db.upsert_vector(space, chunk, &support::unit_vector_1024())
        .expect("insert");

    db.delete_vector(space, chunk).expect("first delete");
    db.delete_vector(space, chunk)
        .expect("second delete on nothing");

    assert_eq!(db.embedded_chunk_count(space).expect("count"), 0);
}

/// `embedded_chunk_count`'s own `UNION` counts a `chunk_embedding_state` row
/// with `state = 1` as an embedded chunk on its own — `tests/adopt.rs`'s
/// `bookkeeping_without_a_vector_also_makes_a_space_not_empty` is exactly that
/// case. `delete_vector` used to touch only the vector table, so a
/// bookkeeping row surviving from an earlier run — Task 6's queue is what will
/// start writing them — would keep a chunk counted as embedded after its
/// vector was gone. Written by hand, the same way `tests/adopt.rs` does:
/// nothing in this crate writes that table yet.
#[test]
fn deleting_a_vector_also_clears_its_bookkeeping_row() {
    let db = support::temp_db();
    let space = support::space_1024(&db);
    let chunk = support::one_chunk(&db);
    db.upsert_vector(space, chunk, &support::unit_vector_1024())
        .expect("insert");
    db.conn()
        .execute(
            "INSERT INTO chunk_embedding_state (space_id, chunk_id, content_hash, state)
             VALUES (?1, ?2, 'hash', 1)",
            rusqlite::params![space, chunk],
        )
        .expect("bookkeeping row");

    db.delete_vector(space, chunk).expect("delete");

    assert_eq!(
        db.embedded_chunk_count(space).expect("count"),
        0,
        "a leftover chunk_embedding_state row kept the chunk counted as embedded"
    );
}

/// The other direction of the same fix: a stale row — success or failure —
/// no longer describes a chunk once `upsert_vector` gives it a fresh
/// embedding. Read directly against `chunk_embedding_state` rather than
/// through `embedded_chunk_count`: a `state = 1` row is already folded into
/// that count by the vector's own presence in the `UNION`, so this
/// particular leftover has no symptom through the public count yet — only
/// Task 6's queue will read the table directly enough for a stale row to be
/// wrong out loud.
#[test]
fn upserting_a_vector_clears_a_stale_bookkeeping_row() {
    let db = support::temp_db();
    let space = support::space_1024(&db);
    let chunk = support::one_chunk(&db);
    db.conn()
        .execute(
            "INSERT INTO chunk_embedding_state (space_id, chunk_id, content_hash, state)
             VALUES (?1, ?2, 'hash', 2)",
            rusqlite::params![space, chunk],
        )
        .expect("stale failure row");

    db.upsert_vector(space, chunk, &support::unit_vector_1024())
        .expect("upsert");

    let rows: i64 = db
        .conn()
        .query_row(
            "SELECT count(*) FROM chunk_embedding_state WHERE space_id = ?1 AND chunk_id = ?2",
            rusqlite::params![space, chunk],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(
        rows, 0,
        "a stale bookkeeping row survived a successful upsert"
    );
}

/// `space_count` answers the question a caller cannot answer from
/// `embedded_chunk_count`: whether the space it can count is the only one.
///
/// Three values and not one, because a count that always answered `1` — or
/// always the number of *non-empty* spaces — would satisfy a single case. The
/// empty space in the middle is the load-bearing one: it is invisible to
/// `embedded_chunk_count` from every side, and it is exactly the kind of space
/// a caller asking "is this all there is" must be told about.
#[test]
fn the_number_of_spaces_counts_the_empty_ones_too() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    assert_eq!(db.space_count().unwrap(), 0, "a fresh index has no spaces");

    let cfg = db
        .create_model_config("a", "openrouter", None, "vendor/a", 8)
        .unwrap();
    let full = db.create_space(cfg, 8, "chunker-v1").unwrap();
    db.insert_vector(full, 1, &vec_of(8, 1.0)).unwrap();
    assert_eq!(db.space_count().unwrap(), 1);

    // A second space with nothing in it. `embedded_chunk_count` says zero about
    // it and `space_is_empty` says true, so neither can tell a caller it exists.
    let other = db
        .create_model_config("b", "openrouter", None, "vendor/b", 8)
        .unwrap();
    let empty = db.create_space(other, 8, "chunker-v1").unwrap();
    assert_eq!(db.embedded_chunk_count(empty).unwrap(), 0);
    assert_eq!(
        db.space_count().unwrap(),
        2,
        "an empty space is still a space this index holds"
    );

    // And down again, so the count is read rather than accumulated.
    db.drop_space(full).unwrap();
    assert_eq!(db.space_count().unwrap(), 1);
}

/// The number a confirmed model change actually costs: every embedding in the
/// index, not the active space's share of them.
///
/// **The two spaces hold different counts, and neither is the total.** A sum
/// that read one space, or that answered the largest, or that counted spaces
/// instead of embeddings, all give something other than 5 here — which a fixture
/// with equal halves would not have caught.
///
/// The chunk ids overlap on purpose. Chunk 1 is embedded in both spaces and is
/// two embeddings, because two provider calls made them and two would have to be
/// made again; a sum written as `count(DISTINCT chunk_id)` over the union of the
/// tables would answer 4 and understate what a rebuild costs.
#[test]
fn the_embeddings_everywhere_are_summed_over_spaces_and_distinct_within_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    assert_eq!(
        db.embedded_chunks_everywhere().unwrap(),
        0,
        "a fresh index has nothing to rebuild"
    );

    // A real chunk, because the bookkeeping row at the end of this test has a
    // foreign key to `chunk` and the vector table has none. The other ids are
    // invented, which is what a `vec0` table permits and what
    // `Db::delete_vectors_for_document` exists because of.
    let shared = support::one_chunk(&db);

    let first = db
        .create_model_config("a", "openrouter", None, "vendor/a", 8)
        .unwrap();
    let a = db.create_space(first, 8, "chunker-v1").unwrap();
    for chunk in [shared, 101, 102] {
        db.insert_vector(a, chunk, &vec_of(8, chunk as f32))
            .unwrap();
    }
    assert_eq!(db.embedded_chunks_everywhere().unwrap(), 3);

    let second = db
        .create_model_config("b", "openrouter", None, "vendor/b", 8)
        .unwrap();
    let b = db.create_space(second, 8, "chunker-v1").unwrap();
    for chunk in [shared, 103] {
        db.insert_vector(b, chunk, &vec_of(8, chunk as f32))
            .unwrap();
    }

    assert_eq!(db.embedded_chunk_count(a).unwrap(), 3);
    assert_eq!(db.embedded_chunk_count(b).unwrap(), 2);
    assert_eq!(
        db.embedded_chunks_everywhere().unwrap(),
        5,
        "chunk 1 is embedded twice, by two models, and is two embeddings to pay for again"
    );

    // Two records of ONE embedded chunk stay one, which is the rule
    // `embedded_chunk_count` draws and this must not undo by summing rows.
    db.conn()
        .execute(
            "INSERT INTO chunk_embedding_state (space_id, chunk_id, content_hash, state)
             VALUES (?1, ?2, 'h', 1)",
            [a, shared],
        )
        .unwrap();
    assert_eq!(
        db.embedded_chunks_everywhere().unwrap(),
        5,
        "a bookkeeping row beside the vector it describes is not a second embedding"
    );

    db.drop_space(a).unwrap();
    assert_eq!(
        db.embedded_chunks_everywhere().unwrap(),
        2,
        "what a retired space held is no longer owed to anybody"
    );
}

// ------------------------------------------- writing onto the text you read

/// `chunk.id` is reused — `tests/citation.rs`'s
/// `a_reused_chunk_id_gets_no_inherited_vector` asserts the reuse rather than
/// assuming it — so an embedding pass that reads `(id, text)`, spends a network
/// round trip on the text, and writes back by `id` alone can bind text A's
/// vector to a chunk that now holds text B. This is the guard, and it is here
/// rather than in the pass because a read-then-write at the caller is the same
/// window one layer up.
#[test]
fn a_vector_is_written_only_onto_the_text_it_was_made_from() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let space = support::space_1024(&db);
    let chunk = support::one_chunk(&db);
    let hash: String = db
        .conn()
        .query_row(
            "SELECT content_hash FROM chunk WHERE id = ?1",
            [chunk],
            |r| r.get(0),
        )
        .unwrap();

    assert!(
        db.upsert_vector_for_text(space, chunk, &hash, &support::unit_vector_1024())
            .unwrap(),
        "the ordinary case — the chunk still holds the text — must write"
    );
    assert_eq!(db.embedded_chunk_count(space).unwrap(), 1);

    // The text moves on under the pass. Nothing else changes: same space, same
    // chunk id, a vector that was perfectly good a moment ago.
    db.conn()
        .execute(
            "UPDATE chunk SET text = 'інший текст', content_hash = 'a-different-hash' \
             WHERE id = ?1",
            [chunk],
        )
        .unwrap();

    assert!(
        !db.upsert_vector_for_text(space, chunk, &hash, &support::other_unit_vector_1024())
            .unwrap(),
        "a vector for text that is no longer there must not be written"
    );
    assert_eq!(
        support::stored_vector(&db, space, chunk),
        support::unit_vector_1024(),
        "the stale write replaced the vector that was already there"
    );
}

/// A chunk that has gone entirely, which is the other half of the same window:
/// the row the vector would name does not exist, and `false` says so rather
/// than an error — the queue simply never offers that chunk again.
#[test]
fn a_vector_for_a_chunk_that_has_gone_is_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let space = support::space_1024(&db);
    let chunk = support::one_chunk(&db);
    let hash: String = db
        .conn()
        .query_row(
            "SELECT content_hash FROM chunk WHERE id = ?1",
            [chunk],
            |r| r.get(0),
        )
        .unwrap();
    db.conn()
        .execute("DELETE FROM chunk WHERE id = ?1", [chunk])
        .unwrap();

    assert!(
        !db.upsert_vector_for_text(space, chunk, &hash, &support::unit_vector_1024())
            .unwrap()
    );
    assert_eq!(db.embedded_chunk_count(space).unwrap(), 0);
}

/// `record_embedding_failure` writes nothing when the chunk has gone — its
/// `INSERT … SELECT` finds no row — and the caller counts what it reports, not
/// what it called. A `failed` number that includes rows nobody can find is the
/// third number lying about itself, and that number is the entire reason a
/// chunk is allowed to leave the queue.
#[test]
fn recording_a_failure_says_whether_it_wrote_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let space = support::space_1024(&db);
    let chunk = support::one_chunk(&db);

    assert!(
        db.record_embedding_failure(space, chunk).unwrap(),
        "an ordinary refusal writes a row and must say so"
    );
    assert_eq!(db.failed_chunk_count(space).unwrap(), 1);

    db.conn()
        .execute("DELETE FROM chunk WHERE id = ?1", [chunk])
        .unwrap();

    assert!(
        !db.record_embedding_failure(space, chunk).unwrap(),
        "a chunk that has gone leaves no row, and the caller must not count one"
    );
    assert_eq!(db.failed_chunk_count(space).unwrap(), 0);
}

/// A document, page, block and one chunk under `id`, holding `text`.
/// Returns the chunk id.
fn write_one_chunk(db: &Db, id: &str, text: &str) -> i64 {
    db.insert_document(id, "text/plain", text.len() as i64, SourceKind::Document)
        .unwrap();
    rebuild_one_chunk(db, id, text)
}

/// What a rebuild does after `clear_document_content`: writes a fresh page,
/// block and chunk onto a document id that already exists. Returns the
/// chunk id.
fn rebuild_one_chunk(db: &Db, doc: &str, text: &str) -> i64 {
    let page = db.insert_page(doc, 1, "native:txt", None).unwrap();
    let block = db
        .insert_block(
            page,
            &Block {
                block_type: BlockType::Paragraph,
                reading_order: 0,
                language: Some("uk".into()),
                text: text.to_string(),
                line_start: None,
                line_end: None,
            },
        )
        .unwrap();
    db.insert_chunk(
        doc,
        0,
        text,
        &Locator {
            spans: vec![Segment {
                block_id: block,
                start: 0,
                end: text.chars().count() as u32,
                block_start: 0,
            }],
            coordinate: Coordinate::None,
        },
        SourceKind::Document,
    )
    .unwrap()
}

/// T4 (design §7): its two load-bearing checks are
/// `assert_eq!(second, first, ...)`, pinning that the id really was
/// reused, and the final `!upsert_vector_for_text(...)`, pinning that a
/// write under the pre-rebuild hash is refused for the reused id. The
/// LEFT JOIN loop below only catches a vector on a chunk id gone
/// entirely — `clearing_a_document_takes_its_vector_with_the_chunk`
/// already pins that directly, and by the time this loop runs the
/// vector table is empty either way, so `orphans == 0` here holds
/// vacuously and proves nothing about the reused-id case on its own.
#[test]
fn every_vector_names_a_chunk_that_still_holds_its_text() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let space = support::space_1024(&db);

    let chunk = write_one_chunk(&db, &"5".repeat(64), "first");
    let hash: String = db
        .conn()
        .query_row(
            "SELECT content_hash FROM chunk WHERE id = ?1",
            [chunk],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        db.upsert_vector_for_text(space, chunk, &hash, &support::unit_vector_1024())
            .unwrap()
    );

    let victim = write_one_chunk(&db, &"6".repeat(64), "victim");
    let victim_doc = "6".repeat(64);
    let victim_hash: String = db
        .conn()
        .query_row(
            "SELECT content_hash FROM chunk WHERE id = ?1",
            [victim],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        db.upsert_vector_for_text(
            space,
            victim,
            &victim_hash,
            &support::other_unit_vector_1024()
        )
        .unwrap()
    );
    db.delete_vectors_for_document(&victim_doc).unwrap();
    db.delete_document(&victim_doc).unwrap();

    let doc = "5".repeat(64);
    db.clear_document_content(&doc).unwrap();
    let reused = rebuild_one_chunk(&db, &doc, "second");
    assert_eq!(reused, chunk, "pointless unless the id was reused");
    assert!(
        !db.upsert_vector_for_text(space, reused, &hash, &support::unit_vector_1024())
            .unwrap(),
        "a write under the old text's hash must be refused for the reused id"
    );

    for table in vector_tables(&db) {
        let orphans: i64 = db
            .conn()
            .query_row(
                &format!(
                    "SELECT count(*) FROM {table} v
                       LEFT JOIN chunk c ON c.id = v.chunk_id
                      WHERE c.id IS NULL"
                ),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(orphans, 0, "{table} has a vector with no chunk row");
    }
}

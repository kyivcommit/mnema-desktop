//! One embedding model per index (spec §2.1), and the guard that makes the
//! split state unreachable rather than merely discouraged.

use mnema_index::{Error, META_ACTIVE_SPACE, META_VEC_VERSION};

mod support;
use support::temp_db;

const HASH: &str = "chunker-hash-for-tests";
const REF: &str = "openrouter";

#[test]
fn choosing_a_model_creates_a_space_and_makes_it_active() {
    let db = temp_db();
    let adopted = db
        .adopt_embedding_model("baai/bge-m3", 1024, REF, HASH)
        .expect("adopted");
    assert!(adopted.created);
    assert_eq!(
        db.meta_get(META_ACTIVE_SPACE).expect("read").as_deref(),
        Some(adopted.space_id.to_string().as_str())
    );
    assert!(
        db.meta_get(META_VEC_VERSION).expect("read").is_some(),
        "the vector library version is recorded when the first space appears"
    );
    // Which version, and not merely that something is there. The recorded
    // string has to be the vector library's own answer — `sqlite_vec` exports
    // no `VERSION` constant, so the only honest source is the extension
    // running on this connection. Asserting the two are equal is what would go
    // red if the string were ever taken from somewhere plausible instead: the
    // crate version, the pin in `Cargo.toml`, a literal.
    let from_the_library: String = db
        .conn()
        .query_row("SELECT vec_version()", [], |r| r.get(0))
        .expect("the extension answers");
    assert_eq!(
        db.meta_get(META_VEC_VERSION).expect("read").as_deref(),
        Some(from_the_library.as_str())
    );

    // The credential *reference* is recorded against the configuration the
    // adoption reports, which is both the witness that the reference is stored
    // at all and the witness that `model_config_id` names the right row. What
    // is stored is a name in the OS credential store and never a secret
    // (`schema.sql:324`); this call is given no secret to store.
    let recorded: Option<String> = db
        .conn()
        .query_row(
            "SELECT credential_ref FROM model_config WHERE id = ?1",
            rusqlite::params![adopted.model_config_id],
            |r| r.get(0),
        )
        .expect("read");
    assert_eq!(recorded.as_deref(), Some(REF));
}

#[test]
fn choosing_the_same_model_twice_is_not_a_second_space() {
    let db = temp_db();
    let first = db
        .adopt_embedding_model("baai/bge-m3", 1024, REF, HASH)
        .expect("first");
    let second = db
        .adopt_embedding_model("baai/bge-m3", 1024, REF, HASH)
        .expect("second");
    assert_eq!(first.space_id, second.space_id);
    assert!(
        !second.created,
        "the second call found the space rather than minting one"
    );
}

#[test]
fn a_different_model_is_allowed_while_nothing_is_embedded() {
    let db = temp_db();
    let first = db
        .adopt_embedding_model("baai/bge-m3", 1024, REF, HASH)
        .expect("first");
    let second = db
        .adopt_embedding_model("openai/text-embedding-3-small", 1536, REF, HASH)
        .expect("switching an empty index costs nothing");
    assert_ne!(first.space_id, second.space_id);
    assert_eq!(
        db.meta_get(META_ACTIVE_SPACE).expect("read").as_deref(),
        Some(second.space_id.to_string().as_str())
    );
}

#[test]
fn a_different_model_is_refused_once_a_vector_exists() {
    let db = temp_db();
    let first = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("first");
    let chunk_id = support::one_chunk(&db);
    db.insert_vector(first.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("vector");

    let err = db
        .adopt_embedding_model("openai/text-embedding-3-small", 1536, REF, HASH)
        .expect_err("a filled space may not be swapped out from under the index");
    match err {
        Error::ActiveSpaceNotEmpty {
            space_id,
            embedded_chunks,
        } => {
            assert_eq!(space_id, first.space_id);
            assert_eq!(
                embedded_chunks, 1,
                "the refusal must say how much would have been lost"
            );
        }
        other => panic!("got {other:?}"),
    }
    assert_eq!(
        db.active_space().expect("read"),
        Some(first.space_id),
        "a refused switch must leave the active space where it was"
    );

    // A refusal must also leave nothing behind. The check runs before the
    // writes rather than after them, so the rejected model gets no
    // configuration row and its space gets no row, no `vec0` table and no id
    // out of the counter. Both counts, not one: a check placed after
    // `create_model_config` but before `create_space` would satisfy the second
    // assertion on its own.
    assert_eq!(
        count(&db, "SELECT count(*) FROM embedding_space"),
        1,
        "a refused adoption must not mint a space"
    );
    assert_eq!(
        count(&db, "SELECT count(*) FROM model_config"),
        1,
        "a refused adoption must not record the model it refused"
    );
}

#[test]
fn the_same_model_is_still_accepted_once_vectors_exist() {
    let db = temp_db();
    let first = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("first");
    let chunk_id = support::one_chunk(&db);
    db.insert_vector(first.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("vector");
    let again = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("same model, same space");
    assert_eq!(again.space_id, first.space_id);
}

#[test]
fn a_chunker_change_is_the_same_refusal_as_a_model_change() {
    // The space is keyed on (model, dim, index format, chunker hash)
    // (`schema.sql:358`), so a chunker bump mints a NEW space for the SAME
    // model. Without the guard the active space would move to the empty one and
    // the full one would be orphaned in silence — spec §6, route 3.
    let db = temp_db();
    let first = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("first");
    let chunk_id = support::one_chunk(&db);
    db.insert_vector(first.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("vector");

    let err = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, "a-different-chunker-hash")
        .expect_err("the same model under a new chunker is still a new space");
    assert!(
        matches!(err, Error::ActiveSpaceNotEmpty { .. }),
        "got {err:?}"
    );
    assert_eq!(db.active_space().expect("read"), Some(first.space_id));
}

#[test]
fn a_space_is_empty_only_when_both_sources_say_so() {
    let db = temp_db();
    let space = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("space");
    assert!(db.space_is_empty(space.space_id).expect("read"));

    let chunk_id = support::one_chunk(&db);
    db.insert_vector(space.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("vector");
    // Nothing has been written to `chunk_embedding_state` at all — a vector can
    // outlive its bookkeeping, because a vec0 table cannot be a foreign key
    // target. A check that read only the bookkeeping would call this empty.
    assert!(!db.space_is_empty(space.space_id).expect("read"));
}

#[test]
fn bookkeeping_without_a_vector_also_makes_a_space_not_empty() {
    // The other direction, and it is not decoration: an interrupted run leaves
    // rows saying "done" for a space whose vector table is behind. One-sided
    // assertions are satisfied by zero from the wrong side (D50), so both
    // directions are asserted or neither is.
    //
    // ⚠️ Read this test for exactly what it is. Nothing in the product writes
    // `chunk_embedding_state` today — the embedding writer that will is not
    // built — so the state under test is one only this fixture can reach, and
    // this test would pass identically if the product could never produce it.
    // It pins the shape of the check for the day that writer arrives; it is no
    // evidence that the state occurs.
    let db = temp_db();
    let space = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("space");
    let chunk_id = support::one_chunk(&db);
    // No writer for this table exists yet — the indexing subsystem brings one —
    // so the test writes the row itself.
    db.conn()
        .execute(
            "INSERT INTO chunk_embedding_state (space_id, chunk_id, content_hash, state)
             VALUES (?1, ?2, 'hash', 1)",
            rusqlite::params![space.space_id, chunk_id],
        )
        .expect("bookkeeping row");
    assert!(!db.space_is_empty(space.space_id).expect("read"));
}

#[test]
fn one_chunk_recorded_in_both_places_is_counted_once() {
    // The bookkeeping row and the vector are two records of ONE embedded chunk,
    // so adding them tells the user a switch costs twice what it costs. The
    // number here is the one the refusal puts in front of a person deciding
    // whether to go ahead.
    //
    // Same caveat as the test above: the bookkeeping row is the fixture's,
    // because nothing in the product writes that table yet. What differs is
    // that the defect this catches is not waiting for that writer — it is in
    // the arithmetic today, and only a chunk recorded on both sides can show
    // it.
    let db = temp_db();
    let first = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("first");
    let chunk_id = support::one_chunk(&db);
    db.insert_vector(first.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("vector");
    db.conn()
        .execute(
            "INSERT INTO chunk_embedding_state (space_id, chunk_id, content_hash, state)
             VALUES (?1, ?2, 'hash', 1)",
            rusqlite::params![first.space_id, chunk_id],
        )
        .expect("bookkeeping row");

    let err = db
        .adopt_embedding_model("openai/text-embedding-3-small", 1536, REF, HASH)
        .expect_err("one embedded chunk is still enough to refuse");
    match err {
        Error::ActiveSpaceNotEmpty {
            embedded_chunks, ..
        } => assert_eq!(
            embedded_chunks, 1,
            "one chunk, recorded twice, is one chunk to rebuild"
        ),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn a_dropped_space_does_not_hold_the_index_hostage() {
    // `drop_space` leaves `meta.active_space` pointing at a space that is gone,
    // and it is the mechanism for a model change, so this is the ordinary way
    // in rather than a corrupt database. Whoever arrives here cannot repair the
    // key from outside this crate — `meta_set` refuses it — so a refusal would
    // be a dead end, and the raw failure would be `no such table: vec_emb_1`.
    //
    // A dropped space holds nothing: its vectors went with its table and its
    // bookkeeping cascaded from `embedding_space`. So the next choice is
    // allowed, and it repairs the dangling key on its way through.
    let db = temp_db();
    let first = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("first");
    let chunk_id = support::one_chunk(&db);
    db.insert_vector(first.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("vector");
    db.drop_space(first.space_id).expect("drop");

    let second = db
        .adopt_embedding_model("openai/text-embedding-3-small", 1536, REF, HASH)
        .expect("a space that no longer exists cannot lose anything");
    assert_ne!(second.space_id, first.space_id);
    assert_eq!(
        db.active_space().expect("read"),
        Some(second.space_id),
        "the dangling key is repaired by the adoption that met it"
    );
}

#[test]
fn returning_to_a_model_already_tried_creates_nothing() {
    // `created` says what `create_space` did. The neighbouring fact — "the
    // active space moved" — agrees with it everywhere except here: A, then B,
    // then A again puts the active space back on a space that has existed since
    // the first call. Read off the move, this third call reports having created
    // one. Nothing else in this file separates the two, which is the only
    // reason this test exists.
    let db = temp_db();
    let a = db
        .adopt_embedding_model("baai/bge-m3", 1024, REF, HASH)
        .expect("a");
    db.adopt_embedding_model("openai/text-embedding-3-small", 1536, REF, HASH)
        .expect("b");
    let again = db
        .adopt_embedding_model("baai/bge-m3", 1024, REF, HASH)
        .expect("a again");

    assert_eq!(again.space_id, a.space_id);
    assert!(
        !again.created,
        "the space has been there since the first call"
    );
    assert_eq!(
        count(&db, "SELECT count(*) FROM embedding_space"),
        2,
        "and no third space was minted"
    );
}

fn count(db: &mnema_index::Db, sql: &str) -> i64 {
    db.conn().query_row(sql, [], |r| r.get(0)).expect("count")
}

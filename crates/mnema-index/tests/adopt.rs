//! One embedding model per index (spec §2.1), and the guard that makes the
//! split state unreachable rather than merely discouraged.

use std::sync::mpsc;
use std::time::Duration;

use mnema_index::{Error, META_ACTIVE_SPACE, META_VEC_VERSION, open};

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
    // The sentence a person actually reads, and a substring rather than the
    // whole of it: the claim under test is that the count arrives in a form
    // that is grammatical at one as well as at many, not that the wording never
    // changes. It read "1 chunks" before.
    assert!(
        err.to_string().contains("1 of its chunks"),
        "the message has to count in a form that survives the number one, and reads {err}"
    );
    match err {
        Error::SpaceNotEmpty {
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
    // The third thing a refusal must not leave, and the one arrangement the two
    // counts above cannot see: create-then-undo puts both of them back to 1
    // while the id is spent, because `AUTOINCREMENT` keeps a high-water mark
    // that DELETE does not lower (`schema.sql:337-345`). Two comments claimed
    // this and nothing asked.
    assert_eq!(
        count(
            &db,
            "SELECT ifnull((SELECT seq FROM sqlite_sequence WHERE name = 'embedding_space'), 0)"
        ),
        1,
        "a refused adoption must not spend an id either"
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
    assert!(matches!(err, Error::SpaceNotEmpty { .. }), "got {err:?}");
    assert_eq!(db.active_space().expect("read"), Some(first.space_id));

    // This is the scenario in which the old message was false, so it is where
    // the replacement is pinned: nobody changed the model here — the chunker
    // moved, and the same model got a new space — and the refusal used to say
    // "the embedding model cannot be changed". Both directions, because the
    // absence of a wrong word is satisfied by a message that says nothing.
    let message = err.to_string();
    assert!(
        !message.contains("embedding model cannot be changed"),
        "the model did not change, only the chunker did, and the refusal reads {message}"
    );
    assert!(
        message.contains("cannot move to a different space"),
        "and it has to name what it is refusing rather than merely avoid the wrong word: {message}"
    );
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
        Error::SpaceNotEmpty {
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

#[test]
fn a_full_space_blocks_a_switch_even_though_nothing_points_at_it() {
    // The guard used to ask `meta.active_space` — a pointer only the adoption
    // path writes — instead of asking what exists. These four calls are all
    // public and none of them is raw SQL, and they leave a full space with no
    // pointer to it; read off the pointer, the guard saw `None`, ran no check
    // at all, and let the index move away in silence.
    let db = temp_db();
    let config = db
        .create_model_config("m", "openrouter", None, "baai/bge-m3", 4)
        .expect("config");
    let space = db.create_space(config, 4, HASH).expect("space");
    let chunk_id = support::one_chunk(&db);
    db.insert_vector(space, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("vector");
    assert_eq!(
        db.active_space().expect("read"),
        None,
        "nothing has written the pointer, which is the whole point of the test"
    );

    let err = db
        .adopt_embedding_model("openai/text-embedding-3-small", 1536, REF, HASH)
        .expect_err("a full space blocks the switch whether or not it is pointed at");
    match err {
        Error::SpaceNotEmpty {
            space_id,
            embedded_chunks,
        } => {
            assert_eq!(space_id, space);
            assert_eq!(embedded_chunks, 1);
        }
        other => panic!("got {other:?}"),
    }
    assert_eq!(
        db.active_space().expect("read"),
        None,
        "and the refusal wrote nothing"
    );
    // Which of the two checks refused matters here, and only these say so.
    // Both `requested` and the pointer are `None` in this scenario, so an
    // exemption written as `requested == active` instead of
    // `requested.is_some() && requested == active` skips the pre-flight on
    // exactly this call. The refusal still arrives — from the check under the
    // write lock, one space and one configuration later — and without these
    // counts the test passes on that neighbouring defence and the guard for
    // this scenario has no witness at all. Measured: it survived until they
    // were here.
    assert_eq!(
        count(&db, "SELECT count(*) FROM embedding_space"),
        1,
        "the refusal has to come before the space is minted, not after"
    );
    assert_eq!(
        count(&db, "SELECT count(*) FROM model_config"),
        1,
        "and before the refused model is recorded"
    );
}

/// The way out of the state the test above builds, and the only test in this
/// file that reaches `refuse_unless_every_other_space_is_empty`'s `continue`.
///
/// The same four public calls leave a space full of one model's vectors with
/// nothing pointing at it. Adopting **that model** must be allowed: the rule is
/// "every space EXCEPT the requested one is empty", and refusing here would
/// leave an archive nobody can reach — the pointer cannot be set, and adoption
/// is the only path that sets it.
///
/// ⚠️ Written because the mutation that makes the rule count the requested space
/// against itself left the whole crate green (Task 10). Every other test where
/// the requested space holds vectors is exempted one level up by
/// `refuse_if_the_move_would_orphan_anything` — `requested == active_space()` —
/// so the skip inside the rule was standing on the exemption, a defence that
/// answers a different question. Here the pointer is `None` and `requested` is
/// `Some`, so the exemption cannot fire and the rule is genuinely asked. That
/// combination exists nowhere else in this file.
#[test]
fn the_model_a_space_is_full_of_can_be_adopted_even_with_nothing_pointing_at_it() {
    let db = temp_db();
    let config = db
        .create_model_config("m", "openrouter", None, "baai/bge-m3", 4)
        .expect("config");
    let space = db.create_space(config, 4, HASH).expect("space");
    let chunk_id = support::one_chunk(&db);
    db.insert_vector(space, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("vector");
    assert_eq!(
        db.active_space().expect("read"),
        None,
        "nothing has written the pointer, which is what keeps the exemption out of this"
    );

    let adopted = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("the space being adopted is not one the rule may count against the call");

    assert_eq!(
        adopted.space_id, space,
        "it must find the space the vectors are already in, not mint a second one"
    );
    assert!(
        !adopted.created,
        "the space existed, and `created` is read from `create_space`'s own answer"
    );
    // And the point of allowing it: the archive is reachable again.
    assert_eq!(db.active_space().expect("read"), Some(space));
    assert_eq!(
        count(&db, "SELECT count(*) FROM embedding_space"),
        1,
        "no second space was minted beside the full one"
    );
}

#[test]
fn a_pointer_nobody_can_read_neither_unlocks_the_switch_nor_locks_the_index() {
    // Both directions, because either alone is satisfied by the wrong rule. An
    // unreadable pointer must not open the switch — that was the hole — and it
    // must not close the index either, which is what refusing to parse it would
    // have done: `meta_set` refuses this key, so nobody outside the crate could
    // repair it. Asking what exists answers both without a special case.
    let db = temp_db();
    let first = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("first");
    let chunk_id = support::one_chunk(&db);
    db.insert_vector(first.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("vector");
    db.conn()
        .execute(
            "UPDATE meta SET value = 'not-a-number' WHERE key = ?1",
            rusqlite::params![META_ACTIVE_SPACE],
        )
        .expect("scribble on the pointer");
    assert_eq!(
        db.active_space().expect("read"),
        None,
        "the pointer no longer reads as an id"
    );

    let err = db
        .adopt_embedding_model("openai/text-embedding-3-small", 1536, REF, HASH)
        .expect_err("a switch is still refused");
    assert!(matches!(err, Error::SpaceNotEmpty { .. }), "got {err:?}");

    let again = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("the model the index is already full of is not a switch");
    assert_eq!(again.space_id, first.space_id);
    assert_eq!(
        db.active_space().expect("read"),
        Some(first.space_id),
        "and adopting it again is what repairs the pointer"
    );
}

#[test]
fn a_full_space_that_is_not_the_active_one_still_blocks() {
    // Every call public, no raw SQL. Two spaces are adopted while both are
    // empty, which is allowed; the vectors then go into the one that is *not*
    // active, and the active one is dropped. A guard reading the pointer counts
    // the dropped space, finds nothing there, and moves the index off the full
    // one.
    let db = temp_db();
    let first = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("first");
    let second = db
        .adopt_embedding_model("openai/text-embedding-3-small", 1536, REF, HASH)
        .expect("switching an empty index costs nothing");
    let chunk_id = support::one_chunk(&db);
    db.insert_vector(first.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("vector into the space nothing points at");
    db.drop_space(second.space_id).expect("drop the active one");

    let err = db
        .adopt_embedding_model("mistral/embed", 1024, REF, HASH)
        .expect_err("the space holding the archive still blocks");
    match err {
        Error::SpaceNotEmpty {
            space_id,
            embedded_chunks,
        } => {
            assert_eq!(space_id, first.space_id);
            assert_eq!(embedded_chunks, 1);
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn a_width_that_disagrees_with_the_configuration_says_so_either_way() {
    // One call, one cause, and it must not change its story with the state of
    // the index. Asking for a recorded model at a width the configuration does
    // not have is a width problem; before this it was reported as a width
    // problem on an empty index and as "the embedding model cannot be changed"
    // on a full one, which named a cause nobody had raised.
    let empty = temp_db();
    empty
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("first");
    let on_empty = empty
        .adopt_embedding_model("baai/bge-m3", 1024, REF, HASH)
        .expect_err("the recorded configuration is 4 wide");

    let full = temp_db();
    let first = full
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("first");
    let chunk_id = support::one_chunk(&full);
    full.insert_vector(first.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("vector");
    let on_full = full
        .adopt_embedding_model("baai/bge-m3", 1024, REF, HASH)
        .expect_err("the recorded configuration is still 4 wide");

    for err in [&on_empty, &on_full] {
        assert!(
            matches!(
                err,
                Error::SpaceDimMismatch {
                    config_dim: 4,
                    space_dim: 1024,
                    ..
                }
            ),
            "got {err:?}"
        );
    }
}

#[test]
fn a_vector_written_while_a_switch_is_deciding_still_refuses_it() {
    // The check inside the transaction that repoints the index is the half of
    // the double check that nothing single-threaded can see: in one thread
    // nothing changes between the two, so deleting the second leaves the whole
    // crate green.
    //
    // Two connections make the interleave deterministic rather than hopeful.
    // The writer holds an open transaction with a vector in it; WAL readers see
    // the last commit, so the adoption's pre-flight check reads the space as
    // empty and passes. The adoption then blocks at its first write — and the
    // assertion that it is *still* blocked is what proves it got past the
    // pre-flight, since a check that only reads cannot block. Releasing the
    // lock lets it reach the second check, which sees the vector that landed
    // while it was deciding.
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("index.sqlite");
    let adopting = open(&path).expect("the connection that adopts");
    let writer = open(&path).expect("the connection that writes a vector");

    let first = adopting
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("first");
    let chunk_id = support::one_chunk(&adopting);

    writer
        .conn()
        .execute_batch("BEGIN IMMEDIATE")
        .expect("the writer takes the lock");
    writer
        .insert_vector(first.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("written, and not yet visible to anybody else");

    let (started_tx, started_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let switch = std::thread::spawn(move || {
        started_tx.send(()).expect("announce the start");
        let outcome =
            adopting.adopt_embedding_model("openai/text-embedding-3-small", 1536, REF, HASH);
        finished_tx.send(()).expect("announce the finish");
        (adopting, outcome)
    });

    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the adoption never started");
    assert!(
        finished_rx
            .recv_timeout(Duration::from_millis(500))
            .is_err(),
        "the adoption returned while the writer still held the lock, so it never reached \
         the write that has to block — and this test would then prove nothing"
    );

    writer
        .conn()
        .execute_batch("COMMIT")
        .expect("the vector becomes visible");
    let (adopting, outcome) = switch.join().expect("the adopting thread panicked");

    let err = outcome.expect_err("a vector that landed mid-decision must still be counted");
    match err {
        Error::SpaceNotEmpty {
            space_id,
            embedded_chunks,
        } => {
            assert_eq!(space_id, first.space_id);
            assert_eq!(embedded_chunks, 1);
        }
        other => panic!("got {other:?}"),
    }
    assert_eq!(
        adopting.active_space().expect("read"),
        Some(first.space_id),
        "the index did not move"
    );
    // The one case where a refusal is not clean, asserted rather than only
    // described: by the time the second check fires, the new space exists.
    assert_eq!(
        count(&adopting, "SELECT count(*) FROM embedding_space"),
        2,
        "a refusal from the second check keeps the space it had already created"
    );
}

#[test]
fn space_is_empty_does_not_call_a_missing_space_empty() {
    // "Empty" and "not there" are two facts, and a public method returning
    // `bool` can only carry one of them. It answered `Ok(true)` for any id at
    // all until the pointer this was propping up went away.
    let db = temp_db();
    let err = db
        .space_is_empty(9999)
        .expect_err("an id nobody wrote is not a fact about emptiness");
    assert!(matches!(err, Error::NoSuchSpace(9999)), "got {err:?}");
}

#[test]
fn creating_a_space_records_the_vector_library_version() {
    // `META_VEC_VERSION` says it is "the version that created the first space
    // here". Written by the adoption path, that sentence described an event
    // nothing recorded: a space created through the public `create_space` left
    // no version at all.
    let db = temp_db();
    assert_eq!(db.meta_get(META_VEC_VERSION).expect("read"), None);

    let config = db
        .create_model_config("m", "openrouter", None, "baai/bge-m3", 4)
        .expect("config");
    db.create_space(config, 4, HASH).expect("space");

    let from_the_library: String = db
        .conn()
        .query_row("SELECT vec_version()", [], |r| r.get(0))
        .expect("the extension answers");
    assert_eq!(
        db.meta_get(META_VEC_VERSION).expect("read").as_deref(),
        Some(from_the_library.as_str())
    );
}

#[test]
fn re_adopting_the_model_the_index_is_already_on_moves_nothing_and_is_allowed() {
    // Two non-empty spaces, which nothing else in this file builds — and that
    // is exactly why the rule could refuse this and no test noticed.
    //
    // The state is the sanctioned migration's own middle, the one
    // `adopt_embedding_model` documents as legal: the new space built and
    // filled while the old one is still there. A call naming the model the
    // index is already on writes the pointer with the value it already holds,
    // so nothing can be orphaned by it — and since adoption is the only path
    // that writes `credential_ref`, refusing it made the API key unchangeable
    // for as long as the migration lasted.
    let db = temp_db();
    let first = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("first");
    // One chunk, embedded into both spaces — which is what a migration is:
    // the same archive re-embedded into the new space beside the old.
    let chunk_id = support::one_chunk(&db);
    db.insert_vector(first.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("the archive, in the space the index is on");

    // The migration builds and fills the new space directly, because that is
    // what "build the new space, fill it, and then switch" is: adoption is the
    // switch, and it comes last.
    let next_config = db
        .create_model_config(
            "next",
            "openrouter",
            None,
            "openai/text-embedding-3-small",
            4,
        )
        .expect("config");
    let next_space = db
        .create_space(next_config, 4, HASH)
        .expect("the new space");
    db.insert_vector(next_space, chunk_id, &[0.0, 1.0, 0.0, 0.0])
        .expect("the archive, being rebuilt");
    assert_ne!(next_space, first.space_id, "two spaces, both non-empty");

    let again = db
        .adopt_embedding_model("baai/bge-m3", 4, "openrouter-key-2", HASH)
        .expect("a call that moves the index nowhere cannot orphan anything");
    assert_eq!(again.space_id, first.space_id);
    assert!(!again.created);
    assert_eq!(
        db.conn()
            .query_row(
                "SELECT credential_ref FROM model_config WHERE id = ?1",
                rusqlite::params![again.model_config_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .expect("read")
            .as_deref(),
        Some("openrouter-key-2"),
        "and the credential it came to update is updated"
    );

    // The other direction, and it is the whole reason the exemption is keyed on
    // "would this move the pointer" rather than on anything looser: in this
    // same state, a call that *does* move the index is still refused.
    let err = db
        .adopt_embedding_model("mistral/embed", 4, REF, HASH)
        .expect_err("a real switch still splits the archive");
    match err {
        Error::SpaceNotEmpty { space_id, .. } => assert!(
            space_id == first.space_id || space_id == next_space,
            "one of the two full spaces, got {space_id}"
        ),
        other => panic!("got {other:?}"),
    }
    assert_eq!(
        db.active_space().expect("read"),
        Some(first.space_id),
        "and the index has not moved"
    );
}

#[test]
fn a_space_holding_only_a_record_is_refused_in_words_that_are_true_of_it() {
    // The branch the word "recorded" exists for. A targeted edit of the message
    // from "recorded for" to "held for" passes all three assertions elsewhere
    // in this file — the count is still there, the forbidden phrase is still
    // absent — and is false only here, where the space holds no embedding at
    // all and only a row claiming one.
    //
    // Same caveat as `bookkeeping_without_a_vector_also_makes_a_space_not_empty`:
    // nothing in the product writes `chunk_embedding_state`, so this state is
    // one only the fixture can reach, and the test would pass identically if
    // the product could never produce it.
    let db = temp_db();
    let first = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("first");
    let chunk_id = support::one_chunk(&db);
    db.conn()
        .execute(
            "INSERT INTO chunk_embedding_state (space_id, chunk_id, content_hash, state)
             VALUES (?1, ?2, 'hash', 1)",
            rusqlite::params![first.space_id, chunk_id],
        )
        .expect("bookkeeping row");
    assert_eq!(
        count(
            &db,
            &format!("SELECT count(*) FROM vec_emb_{}", first.space_id)
        ),
        0,
        "no embedding exists here at all, which is what makes the word load-bearing"
    );

    let err = db
        .adopt_embedding_model("openai/text-embedding-3-small", 1536, REF, HASH)
        .expect_err("a record of an embedding is still something a switch would lose");
    let message = err.to_string();
    assert!(
        message.contains("recorded for"),
        "the space holds a record and not an embedding, and the message reads {message}"
    );
}

#[test]
fn a_refusal_over_a_space_that_already_exists_still_writes_nothing() {
    // The shape no other test in this file has: a refusal in which `requested`
    // is `Some`. In all nine of the others the model being adopted has no
    // configuration yet, or no space yet, so `requested` is `None` — and an
    // exemption weakened to "skip when a space was found" is invisible to every
    // one of them. Measured: weakening it that way at the pre-flight left the
    // whole crate green.
    //
    // Nothing is lost when that happens, because the check under the write lock
    // still refuses. What is lost is the only thing the pre-flight is for, and
    // it is not a count here — both spaces and both configurations already
    // exist, so the debris assertions elsewhere cannot see it. `credential_ref`
    // is the observable: a refusal that got past the pre-flight writes it on
    // the way to being refused.
    let db = temp_db();
    let first = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("first");
    let second = db
        .adopt_embedding_model("openai/text-embedding-3-small", 4, REF, HASH)
        .expect("switching an empty index costs nothing");
    let chunk_id = support::one_chunk(&db);
    db.insert_vector(second.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("the archive, in the space the index is now on");

    let before = credential_of(&db, first.model_config_id);
    assert_eq!(
        before.as_deref(),
        Some(REF),
        "the credential this call would overwrite, read before it runs"
    );

    let err = db
        .adopt_embedding_model("baai/bge-m3", 4, "key-that-should-not-land", HASH)
        .expect_err("moving back onto the empty space would strand the archive");
    match err {
        Error::SpaceNotEmpty {
            space_id,
            embedded_chunks,
        } => {
            assert_eq!(space_id, second.space_id);
            assert_eq!(embedded_chunks, 1);
        }
        other => panic!("got {other:?}"),
    }

    // ⚠️ This assertion reads "the pre-flight check refused", and it is only
    // entitled to, because this test is single-threaded. The write it watches
    // sits *between* the two checks, so strictly it says "the decisive check
    // refused" — and there is a path where the refusal is entirely correct and
    // the credential moves anyway, which is that check doing its job. The
    // inference holds here only because nothing can change between the two when
    // one thread runs them both. `a_vector_written_while_a_switch_is_deciding…`
    // in this same file breaks that condition on purpose, with a second
    // connection: adding concurrency here to "strengthen" this test would make
    // this assertion false without touching a line of what it guards.
    assert_eq!(
        credential_of(&db, first.model_config_id).as_deref(),
        Some(REF),
        "a refusal must not have written on its way to refusing"
    );
    assert_eq!(
        db.active_space().expect("read"),
        Some(second.space_id),
        "and the index has not moved"
    );
}

/// The three reads the settings screen is built from, each asserted against a
/// state where the other two would give a different answer.
///
/// Everything here is zero on a fresh index, so a fixture that only adopts a
/// model proves nothing about any of the queries: three functions returning a
/// literal zero would pass it. Each step below therefore moves exactly one of
/// the numbers.
#[test]
fn the_settings_read_names_the_model_the_width_and_what_is_embedded() {
    let db = temp_db();
    assert_eq!(
        db.chunk_count().expect("count"),
        0,
        "an empty index holds no chunks"
    );

    let adopted = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("adopted");
    assert_eq!(
        db.space_model(adopted.space_id).expect("read"),
        ("baai/bge-m3".to_string(), 4),
        "the space names the model it was built for and the width it was built at"
    );
    assert_eq!(
        db.embedded_chunk_count(adopted.space_id).expect("count"),
        0,
        "adopting a model embeds nothing"
    );

    // A chunk that exists is not a chunk that is embedded, and only these two
    // lines together say so: without the first, the count below is satisfied by
    // an index with nothing in it.
    let chunk_id = support::one_chunk(&db);
    assert_eq!(db.chunk_count().expect("count"), 1);
    assert_eq!(
        db.embedded_chunk_count(adopted.space_id).expect("count"),
        0,
        "a chunk with no vector is not embedded"
    );

    db.insert_vector(adopted.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("vector");
    assert_eq!(
        db.embedded_chunk_count(adopted.space_id).expect("count"),
        1,
        "a vector written straight into the space is what `insert_vector` records — and \
         the only record it writes, so a count that read `chunk_embedding_state` alone \
         would answer zero here"
    );
    assert_eq!(
        db.chunk_count().expect("count"),
        1,
        "embedding a chunk does not create one"
    );
}

/// A rebuild takes a chunk's vector with it, so the settings screen's
/// numerator can no longer run ahead of its denominator.
///
/// Until D88 this test asserted the opposite and said so in these words: "this
/// test stops passing the day somebody makes `clear_document_content` take the
/// vectors with it." That day is this one —
/// `Db::clear_document_content_in` (`write.rs`) now calls
/// `crate::space::delete_vectors_for_document_in` before the page delete that
/// starts the cascade, the same call `Db::delete_watched_root` already made —
/// and the test now pins the invariant it used to document the absence of.
#[test]
fn clearing_a_document_takes_its_vector_with_the_chunk() {
    let db = temp_db();
    let adopted = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("adopted");
    let chunk_id = support::one_chunk(&db);
    db.insert_vector(adopted.space_id, chunk_id, &[1.0, 0.0, 0.0, 0.0])
        .expect("vector");
    // Both directions, because the whole claim is that these two numbers move
    // together: without this, the pair below is satisfied by a fixture that
    // never had a chunk or never had a vector.
    assert_eq!(db.chunk_count().expect("count"), 1);
    assert_eq!(db.embedded_chunk_count(adopted.space_id).expect("count"), 1);

    // `support::one_chunk` files its document under `"a".repeat(64)` — a
    // content hash's worth of one character — and that is the only handle on it
    // this test needs.
    db.clear_document_content(&"a".repeat(64))
        .expect("the document's content is cleared");

    assert_eq!(
        db.chunk_count().expect("count"),
        0,
        "the chunk went with the page it came from"
    );
    assert_eq!(
        db.embedded_chunk_count(adopted.space_id).expect("count"),
        0,
        "and its vector went with it — a surviving vector here would again let \
         embedded exceed total, the state this test used to document"
    );
}

/// A space that is gone is not a model nobody chose.
///
/// `drop_space` leaves `meta.active_space` dangling — the state
/// `a_dropped_space_does_not_hold_the_index_hostage` above enters deliberately —
/// so a settings read can arrive here holding an id with no row behind it. Told
/// "no model is configured", the window would draw an empty picker over an index
/// that may still hold vectors, and the person reading it would choose a model
/// and pay to embed the archive a second time.
#[test]
fn reading_a_space_that_is_gone_names_the_id_rather_than_answering_nothing() {
    let db = temp_db();
    let adopted = db
        .adopt_embedding_model("baai/bge-m3", 4, REF, HASH)
        .expect("adopted");
    // Both directions: the same call answers the model while the space is
    // there, so the refusal below is about the space being gone and not about
    // the query never having worked.
    assert_eq!(
        db.space_model(adopted.space_id).expect("read").0,
        "baai/bge-m3"
    );

    db.drop_space(adopted.space_id).expect("drop");

    assert!(
        matches!(
            db.space_model(adopted.space_id),
            Err(Error::NoSuchSpace(id)) if id == adopted.space_id
        ),
        "a space that is gone was reported as a model nobody chose: {:?}",
        db.space_model(adopted.space_id)
    );
    assert_eq!(
        db.active_space().expect("read"),
        Some(adopted.space_id),
        "and the pointer is still there, which is what makes the read above reachable"
    );
}

fn credential_of(db: &mnema_index::Db, model_config_id: i64) -> Option<String> {
    db.conn()
        .query_row(
            "SELECT credential_ref FROM model_config WHERE id = ?1",
            rusqlite::params![model_config_id],
            |r| r.get(0),
        )
        .expect("read")
}

fn count(db: &mnema_index::Db, sql: &str) -> i64 {
    db.conn().query_row(sql, [], |r| r.get(0)).expect("count")
}

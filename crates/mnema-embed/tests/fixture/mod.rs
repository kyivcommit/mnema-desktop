//! A database, some chunks, and a provider that answers on demand.
//!
//! A directory rather than `tests/fixture.rs`, for the reason
//! `crates/mnema-index/tests/support/mod.rs` already sets out: cargo turns
//! every file sitting directly inside `tests/` into its own binary, and a
//! module one level down is not one of those files.
//!
//! This is a second fixture set, not a copy of that one — `mnema-index`'s
//! `tests/support` is a per-crate test module and cannot be imported from
//! here, and there is no shared fixture crate. What is duplicated is
//! deliberate and small (`unit_vector_1024`, `stored_vector`); what is not
//! duplicated is the interesting part, since nothing over there builds a
//! document with *many* chunks or an active space.
//!
//! The temporary directory is held inside [`TempDb`] on purpose: dropped
//! separately it takes the database file with it while the connection is still
//! open, and the failure that follows names SQLite rather than the test.

use mnema_core::{Block, BlockType, Coordinate, Locator, Segment, SourceKind};
use mnema_index::{Db, DocumentStatus, open, register_vector_extension};
use mnema_mock_provider::{MockServer, Reply};

/// The model the active space is built for. A constant because one test reads
/// it off the wire, and a literal in two places is a test that can agree with
/// itself while disagreeing with the database.
pub const ACTIVE_MODEL: &str = "baai/bge-m3";

/// The chunker identity every space here is built with. Its value is arbitrary;
/// what matters is that both spaces use the same one, so that
/// `UNIQUE(model_config_id, dim, index_format_version, chunker_hash)` separates
/// them by model configuration and by nothing else.
const CHUNKER: &str = "chunker-v1";

/// What every chunk `db_with_chunks` writes starts with, so a request body can
/// be asked how many texts it carries without parsing JSON.
pub const CHUNK_TEXT_PREFIX: &str = "чанк ";

/// A text whose `prepare_for_search` copy differs from it, which is the whole
/// of what the test using it needs.
///
/// Both differences are real folds and neither is cosmetic: `'` (U+0027)
/// becomes U+02BC MODIFIER LETTER APOSTROPHE, and `ґ` becomes `г` — see
/// `text_prep.rs`, which explains why each one exists. A text without them
/// would make that test pass against a pass that sends the prepared copy.
pub const TEXT_WHOSE_PREPARED_COPY_DIFFERS: &str = "перевірка п'ятого ґанку";

pub struct TempDb {
    db: Db,
    _dir: tempfile::TempDir,
}

impl std::ops::Deref for TempDb {
    type Target = Db;
    fn deref(&self) -> &Db {
        &self.db
    }
}

fn temp_db() -> TempDb {
    let dir = tempfile::tempdir().expect("a temporary directory");
    register_vector_extension().expect("register the vector extension");
    let db = open(&dir.path().join("index.sqlite")).expect("open the index");
    TempDb { db, _dir: dir }
}

/// One `indexed` document holding `count` chunks, each with its own block and
/// its own text.
///
/// One document rather than `count` of them because the queue does not care —
/// it joins through `document.status` and orders by `chunk.id` — and because a
/// single document is what `set_status` and `first_document` then have to
/// name. `count` may be zero, which is a document with no pages at all: the
/// state of an archive that has finished.
pub fn db_with_chunks(count: usize) -> TempDb {
    let db = temp_db();
    let doc = db
        .insert_document(&"a".repeat(64), "text/plain", 64, SourceKind::Document)
        .expect("document");
    for ord in 0..count {
        write_chunk(&db, &doc, ord as i64, &format!("{CHUNK_TEXT_PREFIX}{ord}"));
    }
    db.set_document_status(&doc, DocumentStatus::Indexed)
        .expect("status");
    db
}

/// `count` chunks written against a document that is left `'pending'` — the
/// default a fresh `insert_document` row gets (`schema.sql:71`) — rather than
/// advanced to `indexed`.
///
/// This is the window `crates/mnema-ingest/src/lib.rs:546-598`'s own comment
/// names: chunks written, the status write still to come, in a **separate**
/// transaction on purpose — "a crash before this point costs a re-index
/// rather than a lie". A crash there, or simply not having reached it yet,
/// leaves exactly this: chunks that exist and no vector for any of them,
/// invisible to the queue (`d.status = 'indexed'`, `space.rs:47`) the same way
/// a chunk the space gave up on is invisible to it — fix round 1's I1.
///
/// A fourth document identity: `"a"` is [`db_with_chunks`]'s, `"b"` is
/// [`db_with_chunks_in_two_documents`]'s second, `"c"` is
/// [`add_document_with_chunks`]'s, so `"d"` collides with none of them.
pub fn db_with_unindexed_chunks(count: usize) -> TempDb {
    let db = temp_db();
    let doc = db
        .insert_document(&"d".repeat(64), "text/plain", 64, SourceKind::Document)
        .expect("document");
    for ord in 0..count {
        write_chunk(&db, &doc, ord as i64, &format!("{CHUNK_TEXT_PREFIX}d{ord}"));
    }
    db
}

/// Adds one more document to a database a test already built — its chunks
/// written, its status left `'pending'`. [`db_with_unindexed_chunks`]'s
/// scenario, but appended to an existing database rather than a fresh one,
/// so a test can put it behind a space that is already `ready`: the ordinary
/// way an archive grows once a first pass has already finished it, through
/// exactly the window `crates/mnema-ingest/src/lib.rs:546-598`'s own comment
/// names.
///
/// Shares `db_with_unindexed_chunks`'s document identity, `"d"` — the two
/// are never called against the same database, so nothing collides.
pub fn add_unindexed_document_with_chunks(db: &Db, count: usize) -> String {
    let doc = db
        .insert_document(&"d".repeat(64), "text/plain", 64, SourceKind::Document)
        .expect("document");
    for ord in 0..count {
        write_chunk(db, &doc, ord as i64, &format!("{CHUNK_TEXT_PREFIX}d{ord}"));
    }
    doc
}

/// One `indexed` document holding one chunk whose text is `text` — for the
/// test that reads what went onto the wire.
pub fn db_with_display_text(text: &str) -> TempDb {
    let db = temp_db();
    let doc = db
        .insert_document(&"b".repeat(64), "text/plain", 64, SourceKind::Document)
        .expect("document");
    write_chunk(&db, &doc, 0, text);
    db.set_document_status(&doc, DocumentStatus::Indexed)
        .expect("status");
    db
}

fn write_chunk(db: &Db, doc: &str, ord: i64, text: &str) {
    let page = db
        .insert_page(doc, ord + 1, "native:txt", None)
        .expect("page");
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
        .expect("block");
    db.insert_chunk(
        doc,
        ord,
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
    .expect("chunk");
}

/// Adds one more `indexed` document, with `count` chunks of its own text, to a
/// database a test already built — the ordinary way an archive grows once an
/// earlier embedding pass has already finished it.
///
/// A third document identity: [`db_with_chunks`] uses `"a"` and
/// [`db_with_chunks_in_two_documents`]'s second document uses `"b"`, so `"c"`
/// here collides with neither.
pub fn add_document_with_chunks(db: &Db, count: usize) -> String {
    let doc = db
        .insert_document(&"c".repeat(64), "text/plain", 64, SourceKind::Document)
        .expect("document");
    for ord in 0..count {
        write_chunk(db, &doc, ord as i64, &format!("{CHUNK_TEXT_PREFIX}c{ord}"));
    }
    db.set_document_status(&doc, DocumentStatus::Indexed)
        .expect("status");
    doc
}

/// Two `indexed` documents, with `first` and `second` chunks in them.
///
/// Two rather than one because the test that needs this has to rebuild a
/// document *while a run is in flight* and still have something left that the
/// rebuild does not touch — the untouched one is what corroborates the split,
/// and without it the run would stop for a different reason than the one under
/// test.
pub fn db_with_chunks_in_two_documents(first: usize, second: usize) -> TempDb {
    let db = temp_db();
    for (marker, count) in [("a", first), ("b", second)] {
        let doc = db
            .insert_document(&marker.repeat(64), "text/plain", 64, SourceKind::Document)
            .expect("document");
        for ord in 0..count {
            write_chunk(
                &db,
                &doc,
                ord as i64,
                &format!("{CHUNK_TEXT_PREFIX}{marker}{ord}"),
            );
        }
        db.set_document_status(&doc, DocumentStatus::Indexed)
            .expect("status");
    }
    db
}

/// The documents in the index, in a stable order — `db_with_chunks_in_two_documents`
/// names them so that the first is the one it built first.
pub fn document_ids(db: &Db) -> Vec<String> {
    db.conn()
        .prepare("SELECT id FROM document ORDER BY id")
        .expect("prepare")
        .query_map([], |r| r.get(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<String>>>()
        .expect("rows")
}

/// The chunks of one document, in queue order.
pub fn chunk_ids_of(db: &Db, doc: &str) -> Vec<i64> {
    db.conn()
        .prepare("SELECT id FROM chunk WHERE document_id = ?1 ORDER BY id")
        .expect("prepare")
        .query_map([doc], |r| r.get(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<i64>>>()
        .expect("rows")
}

/// What a rebuild does: clear the document's content and write it again.
///
/// The real path, through [`Db::clear_document_content`], because the
/// interaction is the point — the clear takes the document's vectors with it
/// (D88), sets the document back to `pending`, and frees chunk ids that the new
/// chunks are then handed straight back. Anything that skipped the clear would
/// be a different situation wearing the same name.
pub fn rebuild_document(db: &Db, doc: &str, texts: &[&str]) {
    db.clear_document_content(doc).expect("clear");
    for (ord, text) in texts.iter().enumerate() {
        write_chunk(db, doc, ord as i64, text);
    }
    db.set_document_status(doc, DocumentStatus::Indexed)
        .expect("status");
}

/// A 1024-wide space that `meta.active_space` points at — what
/// `adopt_embedding_model` leaves behind, which is the only way the pointer is
/// ever written.
pub fn active_space_1024(db: &Db) -> i64 {
    db.adopt_embedding_model(ACTIVE_MODEL, 1024, "credential-ref", CHUNKER)
        .expect("adopt")
        .space_id
}

/// A 1024-wide space nothing points at, built the long way round on a model
/// configuration of its own so that it is a *different* space rather than the
/// same one found again.
///
/// Call it before [`active_space_1024`]: adoption refuses to move the pointer
/// onto a space while another holds embeddings, and this one is empty only
/// until the pass runs.
pub fn space_1024_not_active(db: &Db) -> i64 {
    let cfg = db
        .create_model_config("idle", "openrouter", None, "some/other-embedder", 1024)
        .expect("model config");
    db.create_space(cfg, 1024, CHUNKER).expect("space")
}

/// Every chunk in the database, in the order the queue hands them over.
pub fn chunk_ids(db: &Db) -> Vec<i64> {
    db.conn()
        .prepare("SELECT id FROM chunk ORDER BY id")
        .expect("prepare")
        .query_map([], |r| r.get(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<i64>>>()
        .expect("rows")
}

/// The document `db_with_chunks` built — there is exactly one.
pub fn first_document(db: &Db) -> String {
    db.conn()
        .query_row("SELECT id FROM document ORDER BY id LIMIT 1", [], |r| {
            r.get(0)
        })
        .expect("a document")
}

/// Sets `document.status` to a raw string.
///
/// Deliberately not [`Db::set_document_status`] and its enum: two of the four
/// values a test here needs to set — `failed` and `skipped` — arrive in the
/// product from paths this crate has nothing to do with, and the queue reads
/// the *column*. Writing the string is what asks the question the queue
/// answers.
pub fn set_status(db: &Db, doc: &str, status: &str) {
    db.conn()
        .execute(
            "UPDATE document SET status = ?2 WHERE id = ?1",
            rusqlite::params![doc, status],
        )
        .expect("status");
}

/// Changes a chunk's text and its `content_hash` together, in place.
///
/// **No product path does this**, and that is exactly why it is here. A rebuild
/// deletes the chunk and writes a new one, and the delete cascades the
/// `chunk_embedding_state` row away with it — so a test that edited a chunk the
/// way the product does would be asking the queue nothing: the failed row would
/// be gone whatever the predicate said, and would pass against a queue that
/// never compares hashes at all. The state under test is a `failed` row that
/// *survives* while the text it was about does not, and SQL is the only way to
/// build it.
///
/// The hash is the real sha256 of the new text — the same thing `write.rs`'s
/// `chunk_content_hash` computes — rather than an arbitrary different string,
/// so the row this leaves is one the product could have written.
pub fn rewrite_chunk_text(db: &Db, chunk_id: i64, text: &str) {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(text.as_bytes());
    let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let changed = db
        .conn()
        .execute(
            "UPDATE chunk SET text = ?2, content_hash = ?3 WHERE id = ?1",
            rusqlite::params![chunk_id, text, hash],
        )
        .expect("rewrite");
    assert_eq!(changed, 1, "no chunk with id {chunk_id}");
}

/// An active space whose row claims a width its vector table was not built
/// with: `embedding_space.dim` says 1024, the `vec0` table holds 512.
///
/// A hand-edited database, and it takes three tries to find one that asks the
/// right question. `PRAGMA query_only` makes *every* write fail, including the
/// `failed` row the pass would write, so the run ends in an error either way
/// and the test passes against a guard that swallows everything. Dropping the
/// vector table, or renaming it, breaks the queue query one call earlier, so
/// the run never reaches a write at all. This one leaves every read answering
/// and every write working — except the one `INSERT` into `vec0`, which is
/// exactly where an index error that is not about the vector has to arrive for
/// the question to be asked.
///
/// The pass's own width check passes, because it compares against the width the
/// row claims. `vec0` then refuses the blob for a reason of its own.
pub fn active_space_that_lies_about_its_width(db: &Db) -> i64 {
    let space = db
        .adopt_embedding_model(ACTIVE_MODEL, 512, "credential-ref", CHUNKER)
        .expect("adopt")
        .space_id;
    db.conn()
        .execute(
            "UPDATE embedding_space SET dim = 1024 WHERE id = ?1",
            [space],
        )
        .expect("widen the space row");
    db.conn()
        .execute("UPDATE model_config SET dim = 1024", [])
        .expect("widen the config row");
    space
}

/// Rows recording a refusal, counted raw — every `chunk_embedding_state` row
/// with state `2`, with no predicate of its own.
///
/// Deliberately blunter than [`Db::failed_chunk_count`], and read beside it: a
/// count that shares the method's own `WHERE` clause would agree with it about
/// a row that was never written.
pub fn failed_rows(db: &Db) -> i64 {
    db.conn()
        .query_row(
            "SELECT count(*) FROM chunk_embedding_state WHERE state = 2",
            [],
            |r| r.get(0),
        )
        .expect("count")
}

/// Whether this space holds a vector for this chunk — the raw fact, without
/// `embedded_chunk_count`'s `UNION` over `chunk_embedding_state`, which would
/// answer yes for a chunk that has only a state row.
pub fn has_vector(db: &Db, space_id: i64, chunk_id: i64) -> bool {
    let table = vec_table(db, space_id);
    db.conn()
        .query_row(
            &format!("SELECT count(*) FROM {table} WHERE chunk_id = ?1"),
            [chunk_id],
            |r| r.get::<_, i64>(0),
        )
        .expect("count")
        > 0
}

/// One stored vector, decoded exactly as `upsert_vector` wrote it: float32,
/// host byte order. Nothing public hands a vector back out of a space.
pub fn stored_vector(db: &Db, space_id: i64, chunk_id: i64) -> Vec<f32> {
    let table = vec_table(db, space_id);
    let blob: Vec<u8> = db
        .conn()
        .query_row(
            &format!("SELECT embedding FROM {table} WHERE chunk_id = ?1"),
            [chunk_id],
            |r| r.get(0),
        )
        .expect("vector exists");
    blob.chunks_exact(4)
        .map(|b| f32::from_ne_bytes(b.try_into().expect("4-byte chunk")))
        .collect()
}

fn vec_table(db: &Db, space_id: i64) -> String {
    db.conn()
        .query_row(
            "SELECT vec_table FROM embedding_space WHERE id = ?1",
            [space_id],
            |r| r.get(0),
        )
        .expect("space exists")
}

/// A unit vector along axis 0 — valid for a cosine space, and, unlike an
/// all-zero vector, one `check_rankable` accepts.
pub fn unit_vector_1024() -> Vec<f32> {
    let mut v = vec![0.0; 1024];
    v[0] = 1.0;
    v
}

pub fn mock(replies: Vec<Reply>) -> MockServer {
    MockServer::new(replies)
}

/// The raw `embedding_space.state` column — read the way `mark_space_ready`
/// and `mark_space_building` write it, and nowhere else: nothing public hands
/// this back out of a space, by design (D95b).
pub fn space_state(db: &Db, space_id: i64) -> String {
    db.conn()
        .query_row(
            "SELECT state FROM embedding_space WHERE id = ?1",
            [space_id],
            |r| r.get(0),
        )
        .expect("space exists")
}

/// A provider that answers one batch of three well-formed vectors — enough for
/// [`db_with_chunks`]`(3)` embedded in a single batch of `batch = 10`. Built
/// for the test proving a space becomes `ready` once the queue it was built
/// against empties with nothing given up on.
pub fn provider_returning_unit_vectors() -> MockServer {
    mock(vec![reply_with(3)])
}

/// A provider that embeds the first and third texts and refuses the second
/// with a `422` — squarely inside `speaks_only_about_these_texts`, the status
/// an over-long chunk is expected to arrive as. At `batch = 1`, each of
/// [`db_with_chunks`]`(3)`'s chunks is sent as its own request, so this makes
/// the middle one a `failed` row while its neighbours succeed — enough for the
/// test proving a space with any failure at all does not become `ready`.
pub fn provider_refusing_the_second_text() -> MockServer {
    mock(vec![
        reply_with(1),
        Reply::status(422, r#"{"error":"unusable"}"#),
        reply_with(1),
    ])
}

/// A well-formed answer for a request of `count` texts, at the width every
/// space here is built for.
pub fn reply_with(count: usize) -> Reply {
    reply_of_width(count, 1024)
}

/// The same, at a width the caller chooses — for the space-width refusal.
///
/// Row *i* is the unit vector along axis *i*, so the rows are distinguishable
/// from one another and a test can say which text a stored vector was made
/// from. That is only possible while `count <= width`, which every caller here
/// satisfies by a wide margin.
///
/// **Every row states its `index`.** `mnema_provider::embed` binds vectors by
/// the position the provider states rather than by array order, and refuses an
/// answer where a row states none — so a body built without it fails every test
/// it appears in, for a reason that has nothing to do with what the test is
/// about. `mnema_mock_provider::two_vectors` is the same shape at count 2.
pub fn reply_of_width(count: usize, width: usize) -> Reply {
    assert!(
        count <= width,
        "row {count} would need axis {count} of {width}"
    );
    let row = |hot: usize| -> String {
        (0..width)
            .map(|i| if i == hot { "1.0" } else { "0.0" })
            .collect::<Vec<_>>()
            .join(",")
    };
    let rows: Vec<String> = (0..count)
        .map(|i| format!(r#"{{"embedding":[{}],"index":{i}}}"#, row(i)))
        .collect();
    Reply::ok(&format!(r#"{{"data":[{}]}}"#, rows.join(",")))
}

/// A well-formed answer of `count` rows at the usual width, except that row
/// `degenerate` is all zeros.
///
/// The shape of a provider having a bad moment on one text: right status, right
/// count, right width, right stated positions — and a row the index cannot rank,
/// because vec0 divides by the norm and every way that division can go wrong is
/// silent. Nothing in `mnema-provider` looks at the numbers, so this reaches the
/// index untouched.
pub fn reply_with_a_degenerate_row(count: usize, degenerate: usize) -> Reply {
    let rows: Vec<String> = (0..count)
        .map(|i| {
            let components: Vec<&str> = (0..1024)
                .map(|c| {
                    if c == i && i != degenerate {
                        "1.0"
                    } else {
                        "0.0"
                    }
                })
                .collect();
            format!(r#"{{"embedding":[{}],"index":{i}}}"#, components.join(","))
        })
        .collect();
    Reply::ok(&format!(r#"{{"data":[{}]}}"#, rows.join(",")))
}

/// One row, at the usual width, hot along the axis named.
///
/// So that a test can say *which request* a stored vector came from. The
/// ordinary `reply_with` answers along axis 0 for a single text, which is
/// indistinguishable from every other single-text answer; a vector that must
/// not have been written needs to be recognisable when it has been.
pub fn reply_of_axis(axis: usize) -> Reply {
    let components: Vec<&str> = (0..1024)
        .map(|c| if c == axis { "1.0" } else { "0.0" })
        .collect();
    Reply::ok(&format!(
        r#"{{"data":[{{"embedding":[{}],"index":0}}]}}"#,
        components.join(",")
    ))
}

/// One row per entry in `widths`, each at the width named — so a single answer
/// can carry a good vector and a bad one, which is the only way to ask whether
/// the width check happens for the batch or for each vector as it is stored.
pub fn reply_of_mixed_widths(widths: &[usize]) -> Reply {
    let rows: Vec<String> = widths
        .iter()
        .enumerate()
        .map(|(i, width)| {
            let components: Vec<&str> = (0..*width)
                .map(|c| if c == i { "1.0" } else { "0.0" })
                .collect();
            format!(r#"{{"embedding":[{}],"index":{i}}}"#, components.join(","))
        })
        .collect();
    Reply::ok(&format!(r#"{{"data":[{}]}}"#, rows.join(",")))
}

/// How many chunk texts a captured request carries, counted by the marker
/// `db_with_chunks` puts at the front of every one of them.
pub fn texts_in(request: &str) -> usize {
    request.matches(CHUNK_TEXT_PREFIX).count()
}

/// What the lexical index would have stored for this text — the copy the
/// provider must *not* be sent.
pub fn prepared_form(text: &str) -> String {
    mnema_index::prepare_for_search(text, SourceKind::Document)
}

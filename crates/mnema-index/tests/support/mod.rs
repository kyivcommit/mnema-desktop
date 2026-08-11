//! Shared fixtures for the tests that need a database and a chunk in it.
//!
//! A directory rather than `tests/support.rs`, for the reason
//! `crates/mnema-ingest/tests/support/mod.rs` already sets out in full: Cargo
//! turns every file sitting directly inside `tests/` into its own binary, and a
//! module one level down is not one of those files.
//!
//! The temporary directory is held inside `TempDb` on purpose: dropped
//! separately it takes the database file with it while the connection is still
//! open, and the failure that follows names SQLite rather than the test.

use mnema_core::{Block, BlockType, Coordinate, Locator, Segment, SourceKind};
use mnema_index::{Db, DocumentStatus, open, register_vector_extension};

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

pub fn temp_db() -> TempDb {
    let dir = tempfile::tempdir().expect("a temporary directory");
    register_vector_extension().expect("register the vector extension");
    let db = open(&dir.path().join("index.sqlite")).expect("open the index");
    TempDb { db, _dir: dir }
}

/// A 1024-wide embedding space — the width `unit_vector_1024` and
/// `other_unit_vector_1024` are built for.
///
/// **It cannot be called from inside a `Db::transaction` either, though not
/// for `one_chunk`'s reason below.** `create_space` (`space.rs:80`) opens its
/// own IMMEDIATE transaction, and SQLite has no nested `BEGIN` — the same
/// failure shape, but a different function holding the outer `BEGIN`.
///
/// `#[allow(dead_code)]` for the same reason [`one_chunk`] carries it: not
/// every binary that declares `mod support;` calls this one.
#[allow(dead_code)]
pub fn space_1024(db: &Db) -> i64 {
    let cfg = db
        .create_model_config("default", "openrouter", None, "baai/bge-m3", 1024)
        .expect("model config");
    db.create_space(cfg, 1024, "chunker-v1").expect("space")
}

/// A unit vector along axis 0: valid for the cosine space [`space_1024`]
/// builds, and — unlike an all-zero vector — one the crate's own
/// `check_rankable` accepts.
#[allow(dead_code)]
pub fn unit_vector_1024() -> Vec<f32> {
    let mut v = vec![0.0; 1024];
    v[0] = 1.0;
    v
}

/// A second unit vector, along axis 1 rather than axis 0. Distinct from
/// [`unit_vector_1024`] in content and not only in identity, so a test can
/// tell a genuine replacement from one that only looks like it happened —
/// a row count of one is satisfied just as well by a write that silently
/// kept the first vector's numbers.
#[allow(dead_code)]
pub fn other_unit_vector_1024() -> Vec<f32> {
    let mut v = vec![0.0; 1024];
    v[1] = 1.0;
    v
}

/// Reads one stored vector back exactly as `insert_vector`/`upsert_vector`
/// wrote it: float32, host byte order — the layout `mnema_index::space`'s
/// private `as_blob` produces, decoded here because nothing public hands a
/// vector back out of a space.
#[allow(dead_code)]
pub fn stored_vector(db: &Db, space_id: i64, chunk_id: i64) -> Vec<f32> {
    let table: String = db
        .conn()
        .query_row(
            "SELECT vec_table FROM embedding_space WHERE id = ?1",
            [space_id],
            |r| r.get(0),
        )
        .expect("space exists");
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

/// A document, a page, a block and one chunk — the shortest path to a
/// `chunk_id` a vector may be attached to.
///
/// **It cannot be called from inside a `Db::transaction`.** `insert_chunk`
/// opens one of its own, SQLite has no nested `BEGIN`, and `Db::transaction`'s
/// own doc lists it among the methods that do. Build the chunk with
/// `insert_chunk_in` there instead.
///
/// What that costs is worth naming, because it is not what it looks like.
/// Three inserts run first — the document, the page and the block join the
/// open transaction — and then the chunk panics with "cannot start a
/// transaction within a transaction". The panic unwinds through
/// `Db::transaction`, whose `Transaction` rolls back on drop, so those three
/// rows go back out and the database is left holding nothing. Measured, both
/// halves: looking for a half-written document afterwards finds an empty
/// index. What is actually in front of you is a message naming a `BEGIN` you
/// never wrote, because both of them are hidden — one in `Db::transaction`,
/// one inside `insert_chunk`.
///
/// `dead_code` is allowed because this module is compiled into **every** test
/// binary that declares `mod support;`, and not all of them call this
/// particular function: `meta.rs` asks nothing about vectors, and
/// `citation.rs` builds its own chunk with caller-supplied text instead of
/// this one's fixed `"кошторис на ремонт"` — `adopt.rs` and `space.rs` are the
/// ones that call it today. It is written here rather than inside the binary
/// that will need it because the alternative is a second copy of it later,
/// and two answers to "what is the shortest real chunk" are a standing
/// invitation for one of them to drift.
#[allow(dead_code)]
pub fn one_chunk(db: &Db) -> i64 {
    let doc = db
        .insert_document(&"a".repeat(64), "text/plain", 12, SourceKind::Document)
        .expect("document");
    let page = db.insert_page(&doc, 1, "native:txt", None).expect("page");
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
        .expect("block");
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
        .expect("chunk");
    db.set_document_status(&doc, DocumentStatus::Indexed)
        .expect("status");
    chunk
}

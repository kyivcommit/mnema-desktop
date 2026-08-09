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

/// A document, a page, a block and one chunk — the shortest path to a
/// `chunk_id` a vector may be attached to.
///
/// **It cannot be called from inside a `Db::transaction`.** `insert_chunk`
/// opens one of its own, SQLite has no nested `BEGIN`, and `Db::transaction`'s
/// own doc names it as the one exception. Measured, because the way it fails
/// matters: the document, the page and the block go in first — they join
/// whatever transaction is open — and only the chunk fails, with "cannot start
/// a transaction within a transaction". So a test that reaches for this inside
/// a transaction does not get a clean refusal, it gets three rows and a panic.
/// Build the chunk with `insert_chunk_in` there instead.
///
/// `dead_code` is allowed because this module is compiled into **every** test
/// binary that declares `mod support;`, and the only one today — `meta.rs` —
/// asks nothing about vectors. It is written here rather than inside the binary
/// that will need it because the alternative is a second copy of it later, and
/// two answers to "what is the shortest real chunk" are a standing invitation
/// for one of them to drift.
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

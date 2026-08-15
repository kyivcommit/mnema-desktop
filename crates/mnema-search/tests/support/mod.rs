//! A database with an active space and chunks already embedded, and a rude
//! little HTTP server that answers with one of those same vectors — what
//! `content_arm`'s tests need on both sides of the network.
//!
//! A directory rather than `tests/support.rs`, for the reason
//! `crates/mnema-embed/tests/fixture/mod.rs` already sets out: cargo turns
//! every file sitting directly inside `tests/` into its own binary, and a
//! module one level down is not one of those files.

use mnema_core::{Block, BlockType, Coordinate, Locator, Segment, SourceKind};
use mnema_index::{Db, DocumentStatus, open, register_vector_extension};
use mnema_mock_provider::{MockServer, Reply};

const CHUNKER: &str = "chunker-v1";
const MODEL: &str = "baai/bge-m3";
const WIDTH: usize = 1024;

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

pub struct Fixture {
    pub db: TempDb,
    pub chunk_ids: Vec<i64>,
}

/// One active 1024-wide space, holding chunks that are each already
/// embedded — every chunk on its own axis, so a query vector equal to one
/// of them ranks that chunk first and no other.
pub fn indexed_space() -> Fixture {
    let dir = tempfile::tempdir().expect("a temporary directory");
    register_vector_extension().expect("register the vector extension");
    let db = open(&dir.path().join("index.sqlite")).expect("open the index");
    let doc = db
        .insert_document(&"a".repeat(64), "text/plain", 64, SourceKind::Document)
        .expect("document");
    let chunk_ids: Vec<i64> = (0..3)
        .map(|ord| write_chunk(&db, &doc, ord, &format!("чанк {ord}")))
        .collect();
    db.set_document_status(&doc, DocumentStatus::Indexed)
        .expect("status");
    let space = db
        .adopt_embedding_model(MODEL, WIDTH as i64, "credential-ref", CHUNKER)
        .expect("adopt")
        .space_id;
    for (axis, &chunk_id) in chunk_ids.iter().enumerate() {
        db.upsert_vector(space, chunk_id, &axis_vector(axis))
            .expect("vector");
    }
    Fixture {
        db: TempDb { db, _dir: dir },
        chunk_ids,
    }
}

/// A provider that answers with the exact vector already stored for
/// `chunk_id` in `f` — cosine distance zero, so `knn` ranks it ahead of
/// every other chunk `indexed_space` built.
pub fn mock_returning_vector_near(f: &Fixture, chunk_id: i64) -> MockServer {
    let axis = f
        .chunk_ids
        .iter()
        .position(|&id| id == chunk_id)
        .expect("chunk_id belongs to this fixture");
    let row: Vec<String> = axis_vector(axis).iter().map(|v| v.to_string()).collect();
    MockServer::new(vec![Reply::ok(&format!(
        r#"{{"data":[{{"embedding":[{}],"index":0}}]}}"#,
        row.join(",")
    ))])
}

fn axis_vector(axis: usize) -> Vec<f32> {
    let mut v = vec![0.0; WIDTH];
    v[axis] = 1.0;
    v
}

fn write_chunk(db: &Db, doc: &str, ord: i64, text: &str) -> i64 {
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
    .expect("chunk")
}

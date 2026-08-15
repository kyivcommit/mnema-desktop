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

/// A second model name, adopted and then never embedded into — the model
/// [`indexed_space_with_a_decoy_model`] wants recorded in the database next
/// to the real one, so a name genuinely present but not the active space's.
pub const DECOY_MODEL: &str = "openai/decoy-embedding";

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
    pub embedded_ids: Vec<i64>,
    pub space_model: String,
}

impl Fixture {
    /// Renames the `chunk` table so `Db::chunk_count` returns `Err` for the
    /// rest of this fixture's life — a count that cannot be read, not a
    /// table that is honestly empty.
    pub fn break_chunk_count(&self) {
        self.db
            .conn()
            .execute_batch("ALTER TABLE chunk RENAME TO chunk_hidden_for_test;")
            .expect("rename the chunk table");
    }

    /// Renames `chunk_embedding_state` so `Db::embedded_chunk_count` returns
    /// `Err` for the rest of this fixture's life, on the one table
    /// `Db::chunk_count` and `Db::knn` never read.
    pub fn break_embedded_count(&self) {
        self.db
            .conn()
            .execute_batch("ALTER TABLE chunk_embedding_state RENAME TO ces_hidden_for_test;")
            .expect("rename the chunk_embedding_state table");
    }
}

/// The shared build behind every fixture below: `total` chunks, the first
/// `embedded` of them given a vector, each on its own axis. `decoy`, if
/// given, is adopted first — while every space is still empty, the one
/// moment [`Db::adopt_embedding_model`] permits a second one to exist —
/// so it never gets a vector and the active space ends on the real model.
fn built_space(total: usize, embedded: usize, decoy: Option<&str>) -> Fixture {
    let dir = tempfile::tempdir().expect("a temporary directory");
    register_vector_extension().expect("register the vector extension");
    let db = open(&dir.path().join("index.sqlite")).expect("open the index");
    let doc = db
        .insert_document(&"a".repeat(64), "text/plain", 64, SourceKind::Document)
        .expect("document");
    let chunk_ids: Vec<i64> = (0..total as i64)
        .map(|ord| write_chunk(&db, &doc, ord, &format!("чанк {ord}")))
        .collect();
    db.set_document_status(&doc, DocumentStatus::Indexed)
        .expect("status");
    if let Some(decoy) = decoy {
        db.adopt_embedding_model(decoy, WIDTH as i64, "credential-ref", CHUNKER)
            .expect("adopt the decoy while every space is still empty");
    }
    let space = db
        .adopt_embedding_model(MODEL, WIDTH as i64, "credential-ref", CHUNKER)
        .expect("adopt")
        .space_id;
    let embedded_ids: Vec<i64> = chunk_ids[..embedded].to_vec();
    for (axis, &chunk_id) in embedded_ids.iter().enumerate() {
        db.upsert_vector(space, chunk_id, &axis_vector(axis))
            .expect("vector");
    }
    Fixture {
        db: TempDb { db, _dir: dir },
        chunk_ids,
        embedded_ids,
        space_model: MODEL.to_string(),
    }
}

/// One active 1024-wide space, holding chunks that are each already
/// embedded — every chunk on its own axis, so a query vector equal to one
/// of them ranks that chunk first and no other.
pub fn indexed_space() -> Fixture {
    built_space(3, 3, None)
}

/// The same space, with the last chunk left unembedded — a coverage count
/// that has a genuine gap to report instead of a full or empty index.
pub fn indexed_space_with_some_vectors_missing() -> Fixture {
    built_space(3, 2, None)
}

/// The same space as [`indexed_space`], plus [`DECOY_MODEL`] adopted and
/// left empty — a model genuinely on record that is not the active space's,
/// so a test can tell "read the space's own model" apart from "read some
/// model this database happens to know about."
pub fn indexed_space_with_a_decoy_model() -> Fixture {
    built_space(3, 3, Some(DECOY_MODEL))
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

/// A provider that answers any single request with a valid vector, keeping
/// that request's raw text for [`MockServer::request_if_any`] to hand back —
/// for a test that cares which model was asked for, not which chunk won.
pub fn mock_recording_requests() -> MockServer {
    let row: Vec<String> = axis_vector(0).iter().map(|v| v.to_string()).collect();
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

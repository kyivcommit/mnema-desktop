//! §7.1: the schema's own cascade runs from document to path, not the other
//! way (`schema.sql:79-86`), so removing a watched root on its own takes the
//! `path` rows and leaves the documents — named by nothing, still answering
//! searches, quoting a folder the user disconnected.

use mnema_core::{Block, BlockType, Coordinate, Locator, Segment, SourceKind};
use mnema_index::{Db, open, register_vector_extension};
use rusqlite::params;

fn fresh(dir: &tempfile::TempDir) -> Db {
    register_vector_extension().unwrap();
    open(&dir.path().join("index.sqlite")).unwrap()
}

/// Keeps the backing `TempDir` alive alongside the `Db` that opened a file
/// inside it — the same shape `tests/journal.rs`'s `Fixture` uses, and for the
/// same reason: dropping the directory out from under an open connection is
/// not something a fixture should risk.
struct Fixture {
    db: Db,
    _dir: tempfile::TempDir,
}

impl std::ops::Deref for Fixture {
    type Target = Db;
    fn deref(&self) -> &Db {
        &self.db
    }
}

fn fixture_db() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    Fixture { db, _dir: dir }
}

/// One document at one path under `root`, with a single page, block and chunk
/// carrying `text` — enough of the four-level ladder for `search_lexical` and
/// `document_exists` to answer through it.
///
/// The id is a counter rather than a real content hash: nothing in this file
/// reads it back as bytes, and the schema puts no CHECK on its shape — other
/// tests in this crate use the same convention (`"doc-1"`, `"a".repeat(64)`).
fn insert_document_with_chunk(db: &Db, root: i64, relative_path: &str, text: &str) -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let doc = format!(
        "{:064x}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );

    db.insert_document(&doc, "text/plain", text.len() as i64, SourceKind::Document)
        .unwrap();
    db.insert_path(root, relative_path, &doc, text.len() as i64, 1)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    let block = db
        .insert_block(
            page,
            &Block {
                block_type: BlockType::Paragraph,
                reading_order: 0,
                language: None,
                text: text.to_string(),
                line_start: None,
                line_end: None,
            },
        )
        .unwrap();
    db.insert_chunk(
        &doc,
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
    .unwrap();
    doc
}

/// One embedding space and one vector, against `doc`'s only chunk. Returns
/// the space id and the chunk id.
fn seed_one_vector(db: &Db, doc: &str) -> (i64, i64) {
    let cfg = db
        .create_model_config("d", "openrouter", None, "baai/bge-m3", 4)
        .unwrap();
    let space = db.create_space(cfg, 4, "chunker-v1").unwrap();
    let chunk: i64 = db
        .conn()
        .query_row(
            "SELECT id FROM chunk WHERE document_id = ?1",
            params![doc],
            |r| r.get(0),
        )
        .unwrap();
    db.insert_vector(space, chunk, &[0.1, 0.1, 0.1, 0.1])
        .unwrap();
    (space, chunk)
}

/// The cascade runs from document to path, not the other way (`schema.sql:79-86`),
/// so removing a watched root took the path rows and LEFT the documents — with
/// zero paths, still answering searches with a citation that has no path at all
/// (`Citation.relative_path` is `Option` for a different, legitimate reason).
/// The index would go on quoting a folder the user disconnected.
#[test]
fn removing_a_root_removes_the_documents_whose_last_path_it_held() {
    let db = fixture_db();
    let root = db.insert_watched_root("/tmp/one").unwrap();
    let doc = insert_document_with_chunk(&db, root, "a.txt", "unique marker text");
    assert!(!db.search_lexical("marker", 10).unwrap().is_empty());

    let removed = db.delete_watched_root(root).unwrap();

    assert_eq!(removed, 1);
    assert!(!db.document_exists(&doc).unwrap());
    assert!(db.search_lexical("marker", 10).unwrap().is_empty());
}

/// A document that also lives under another root survives — the same rule as a
/// second copy of a file.
#[test]
fn a_document_reachable_from_another_root_survives() {
    let db = fixture_db();
    let one = db.insert_watched_root("/tmp/one").unwrap();
    let two = db.insert_watched_root("/tmp/two").unwrap();
    let doc = insert_document_with_chunk(&db, one, "a.txt", "shared text");
    db.insert_path(two, "copy.txt", &doc, 11, 42).unwrap();

    db.delete_watched_root(one).unwrap();

    assert!(db.document_exists(&doc).unwrap());
    assert_eq!(db.path_count(&doc).unwrap(), 1);
}

/// A `vec0` table cannot be the target of a foreign key, so a vector outlives
/// the chunk it belongs to — recorded by the skeleton and unreachable until
/// now, because nothing deleted at scale. A walk deletes at scale. With chunk
/// ids reused on rebuild, the stale vector then points at different content.
#[test]
fn deleting_a_document_takes_its_vectors_with_it() {
    let db = fixture_db();
    let root = db.insert_watched_root("/tmp/one").unwrap();
    let doc = insert_document_with_chunk(&db, root, "a.txt", "text");
    let (space, chunk) = seed_one_vector(&db, &doc);
    assert_eq!(db.knn(space, &[0.1; 4], 5, None).unwrap().len(), 1);

    db.delete_vectors_for_document(&doc).unwrap();
    db.delete_document(&doc).unwrap();

    assert!(db.knn(space, &[0.1; 4], 5, None).unwrap().is_empty());
    let _ = chunk;
}

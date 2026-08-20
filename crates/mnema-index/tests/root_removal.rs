//! §7.1: the schema's own cascade runs from document to path, not the other
//! way (`schema.sql:79-86`), so removing a watched root on its own takes the
//! `path` rows and leaves the documents — named by nothing, still answering
//! searches, quoting a folder the user disconnected.

use mnema_core::{Block, BlockType, Coordinate, Locator, OnDisk, Segment, SourceKind};
use mnema_index::{Db, DocumentStatus, open, register_vector_extension};
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
/// `document_exists` to answer through it, and declared finished so that the
/// first of those two answers at all (D61: `insert_document` leaves a document
/// at `pending`, and a search does not answer with one of those).
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
    db.insert_path(
        root,
        relative_path,
        &doc,
        OnDisk {
            size_bytes: text.len() as i64,
            mtime: 1,
        },
        "text",
        1,
    )
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
    db.set_document_status(&doc, DocumentStatus::Indexed)
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
    // `chunk_fts` directly, because `search_lexical` now joins through
    // `document` (D61) and would answer nothing for a chunk whose document row
    // is gone even if the chunk itself survived — which is the failure this
    // test is named for. The search is asked too, but after the table that can
    // still say otherwise.
    let fts: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM chunk_fts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fts, 0, "the chunk outlived the root that reached it");
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
    db.insert_path(
        two,
        "copy.txt",
        &doc,
        OnDisk {
            size_bytes: 11,
            mtime: 42,
        },
        "text",
        1,
    )
    .unwrap();

    db.delete_watched_root(one).unwrap();

    assert!(db.document_exists(&doc).unwrap());
    assert_eq!(db.path_count(&doc).unwrap(), 1);
}

/// A `vec0` table cannot be the target of a foreign key, so a vector outlives
/// the chunk it belongs to — recorded by the skeleton and unreachable until
/// now, because nothing deleted at scale. A walk deletes at scale. With chunk
/// ids reused on rebuild, the stale vector then points at different content.
///
/// Calls `delete_vectors_for_document` and then `delete_document` directly
/// — Task 9's own regression test, kept here because it belongs in this
/// crate's suite rather than only in `mnema-ingest`'s. The explicit sweep
/// is redundant now that `delete_document` sweeps a document's vectors
/// itself (round-3, Finding 6) — `mnema-ingest`'s `forget_if_unnamed` calls
/// `delete_document` alone — but it costs nothing to keep here, and the
/// assertion below is the outcome either call would already guarantee on
/// its own. It does **not** exercise `delete_watched_root`'s call into the
/// same sweep: `removing_a_root_takes_its_documents_vectors_too`, below, is
/// the one that pins that path — this one alone left `delete_watched_root`'s
/// own `delete_vectors_for_document_in(&tx, id)` call
/// (`crates/mnema-index/src/write.rs`) free to be deleted with the whole
/// workspace suite staying green, since nothing here ever calls
/// `delete_watched_root`.
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

/// The path `deleting_a_document_takes_its_vectors_with_it` does not reach:
/// `delete_watched_root` sweeps a doomed document's vectors itself, inside its
/// own transaction, rather than leaving the caller to call
/// `delete_vectors_for_document` first. Without this test, deleting the
/// `delete_vectors_for_document_in(&tx, id)?` call at `write.rs`'s call site
/// left the whole workspace suite green — measured, not assumed, before this
/// test existed.
#[test]
fn removing_a_root_takes_its_documents_vectors_too() {
    let db = fixture_db();
    let root = db.insert_watched_root("/tmp/one").unwrap();
    let doc = insert_document_with_chunk(&db, root, "a.txt", "text");
    let (space, _chunk) = seed_one_vector(&db, &doc);
    assert_eq!(db.knn(space, &[0.1; 4], 5, None).unwrap().len(), 1);

    db.delete_watched_root(root).unwrap();

    assert!(db.knn(space, &[0.1; 4], 5, None).unwrap().is_empty());
}

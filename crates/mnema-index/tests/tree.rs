use mnema_core::{OnDisk, SourceKind};
use mnema_index::{Db, DocumentStatus, open, register_vector_extension};

fn fresh(dir: &tempfile::TempDir) -> Db {
    register_vector_extension().unwrap();
    open(&dir.path().join("index.sqlite")).unwrap()
}

#[test]
fn list_watched_roots_returns_every_root_in_add_order() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let a = db.insert_watched_root("/tmp/alpha").unwrap();
    let b = db.insert_watched_root("/tmp/beta").unwrap();

    let roots = db.list_watched_roots().unwrap();

    assert_eq!(roots.len(), 2);
    assert_eq!(
        (roots[0].id, roots[0].absolute_path.as_str()),
        (a, "/tmp/alpha")
    );
    assert_eq!(
        (roots[1].id, roots[1].absolute_path.as_str()),
        (b, "/tmp/beta")
    );
}

#[test]
fn indexed_files_under_root_lists_only_indexed_paths_sorted() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root = db.insert_watched_root("/tmp/root").unwrap();

    let indexed = db
        .insert_document(&"a".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    let pending = db
        .insert_document(&"b".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    db.set_document_status(&indexed, DocumentStatus::Indexed)
        .unwrap();
    // `pending` deliberately left at the default 'pending' status.

    // Insert two paths for the indexed doc out of sorted order, one for the pending doc.
    db.insert_path(
        root,
        "notes/z.txt",
        &indexed,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();
    db.insert_path(
        root,
        "a.txt",
        &indexed,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();
    db.insert_path(
        root,
        "hidden.txt",
        &pending,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();

    let files = db.indexed_files_under_root(root).unwrap();

    // Sorted, indexed-only: "a.txt" before "notes/z.txt"; "hidden.txt" (pending) absent.
    assert_eq!(
        files
            .iter()
            .map(|f| f.relative_path.as_str())
            .collect::<Vec<_>>(),
        vec!["a.txt", "notes/z.txt"],
    );
    assert!(files.iter().all(|f| f.document_id == indexed));
}

#[test]
fn recent_indexed_documents_orders_by_created_at_desc_indexed_only() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root = db.insert_watched_root("/tmp/root").unwrap();

    let older = "a".repeat(64);
    let newer = "b".repeat(64);
    let pending = "c".repeat(64);
    for id in [&older, &newer, &pending] {
        db.insert_document(id, "text/plain", 1, SourceKind::Document)
            .unwrap();
    }
    db.set_document_status(&older, DocumentStatus::Indexed)
        .unwrap();
    db.set_document_status(&newer, DocumentStatus::Indexed)
        .unwrap();
    // `pending` left pending.
    db.conn()
        .execute(
            "UPDATE document SET created_at = 1000 WHERE id = ?1",
            [&older],
        )
        .unwrap();
    db.conn()
        .execute(
            "UPDATE document SET created_at = 2000 WHERE id = ?1",
            [&newer],
        )
        .unwrap();
    db.conn()
        .execute(
            "UPDATE document SET created_at = 3000 WHERE id = ?1",
            [&pending],
        )
        .unwrap();
    db.insert_path(
        root,
        "old.txt",
        &older,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();
    db.insert_path(
        root,
        "new.txt",
        &newer,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();
    db.insert_path(
        root,
        "draft.txt",
        &pending,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();

    let recents = db.recent_indexed_documents(50).unwrap();

    // Newer first; the pending doc (created_at 3000) is absent despite being "newest".
    assert_eq!(
        recents
            .iter()
            .map(|d| d.document_id.as_str())
            .collect::<Vec<_>>(),
        vec![newer.as_str(), older.as_str()],
    );
    assert_eq!(recents[0].created_at, 2000);
    assert_eq!(
        (
            recents[0].relative_path.as_str(),
            recents[0].watched_root_id
        ),
        ("new.txt", root)
    );
}

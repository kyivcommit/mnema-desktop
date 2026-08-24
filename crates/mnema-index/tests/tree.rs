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

#[test]
fn recent_indexed_documents_dedupes_and_respects_limit() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root_a = db.insert_watched_root("/tmp/a").unwrap();
    let root_b = db.insert_watched_root("/tmp/b").unwrap();

    let older = "a".repeat(64);
    let newer = "b".repeat(64);
    db.insert_document(&older, "text/plain", 1, SourceKind::Document)
        .unwrap();
    db.insert_document(&newer, "text/plain", 1, SourceKind::Document)
        .unwrap();
    db.set_document_status(&older, DocumentStatus::Indexed)
        .unwrap();
    db.set_document_status(&newer, DocumentStatus::Indexed)
        .unwrap();
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

    // `older` is the SAME document (one sha256) indexed under TWO roots — the
    // real "same content in two folders" case. Its MIN relative_path, "a.txt",
    // lives under root_b; its other path, "z.txt", is under root_a.
    db.insert_path(
        root_a,
        "z.txt",
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
        root_b,
        "a.txt",
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
        root_a,
        "b.txt",
        &newer,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();

    // Dedup: `older` has two path rows under DIFFERENT roots but must appear
    // once, carrying the MIN relative_path AND that same row's watched_root_id.
    // "a.txt" (the MIN) is under root_b while "z.txt" is under root_a, so
    // asserting ("a.txt", root_b) proves the bare watched_root_id is taken from
    // the MIN row — not the other path's row that GROUP BY could have picked.
    let recents = db.recent_indexed_documents(50).unwrap();
    assert_eq!(recents.len(), 2);
    let older_row = recents
        .iter()
        .find(|d| d.document_id == older)
        .expect("the deduped document is present exactly once");
    assert_eq!(
        (older_row.relative_path.as_str(), older_row.watched_root_id),
        ("a.txt", root_b)
    );

    // LIMIT: two indexed documents exist, but limit=1 returns only the newest.
    let top = db.recent_indexed_documents(1).unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].document_id, newer);
}

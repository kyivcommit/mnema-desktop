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
fn recent_indexed_documents_orders_by_completion_desc_indexed_only() {
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
    // `pending` is left pending — but it carries a chunk/done stage all the
    // same, the state a document that was indexed and then downgraded is in,
    // and the *newest* completion here. So it would sort first if the
    // indexed-only filter were dropped: the WHERE clause, not the INNER JOIN, is
    // what must keep it out of recents.
    for id in [&older, &newer, &pending] {
        db.record_stage(id, "chunk", "done").unwrap();
    }
    // Recency is the chunk/done completion time. Set it directly on
    // `ingest_stage.updated_at`; `document.created_at` no longer decides order.
    db.conn()
        .execute(
            "UPDATE ingest_stage SET updated_at = 1000 WHERE content_hash = ?1 AND stage = 'chunk'",
            [&older],
        )
        .unwrap();
    db.conn()
        .execute(
            "UPDATE ingest_stage SET updated_at = 2000 WHERE content_hash = ?1 AND stage = 'chunk'",
            [&newer],
        )
        .unwrap();
    db.conn()
        .execute(
            "UPDATE ingest_stage SET updated_at = 3000 WHERE content_hash = ?1 AND stage = 'chunk'",
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

    // Newer completion first; the pending doc (completion 3000) is absent
    // despite finishing most recently.
    assert_eq!(
        recents
            .iter()
            .map(|d| d.document_id.as_str())
            .collect::<Vec<_>>(),
        vec![newer.as_str(), older.as_str()],
    );
    assert_eq!(recents[0].indexed_at, 2000);
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
    // Recency is the chunk/done completion time, set on `ingest_stage`.
    db.record_stage(&older, "chunk", "done").unwrap();
    db.record_stage(&newer, "chunk", "done").unwrap();
    db.conn()
        .execute(
            "UPDATE ingest_stage SET updated_at = 1000 WHERE content_hash = ?1 AND stage = 'chunk'",
            [&older],
        )
        .unwrap();
    db.conn()
        .execute(
            "UPDATE ingest_stage SET updated_at = 2000 WHERE content_hash = ?1 AND stage = 'chunk'",
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

/// Owner-review P1-2: recency is completion, not creation. A document keeps the
/// `created_at` from when its still-`pending` row was inserted, but the
/// chunk/done stage that marks it searchable is written later and refreshed on
/// every rebuild. Recents order by that completion time.
///
/// Doc A entered the index first (older `created_at`) but finished indexing last
/// (newer chunk/done `updated_at`); doc B is the reverse. Ordering by
/// `created_at` returns [B, A]; ordering by completion — what a person means by
/// "recent" — returns [A, B]. This is red against `ORDER BY d.created_at` and
/// green against `ORDER BY s.updated_at`, and the exposed `indexed_at` is the
/// completion time, never the older `created_at`.
#[test]
fn recents_recency_follows_completion_not_creation() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root = db.insert_watched_root("/tmp/root").unwrap();

    let a = "a".repeat(64);
    let b = "b".repeat(64);
    for (id, path) in [(&a, "a.txt"), (&b, "b.txt")] {
        db.insert_document(id, "text/plain", 1, SourceKind::Document)
            .unwrap();
        db.set_document_status(id, DocumentStatus::Indexed).unwrap();
        db.record_stage(id, "chunk", "done").unwrap();
        db.insert_path(
            root,
            path,
            id,
            OnDisk {
                size_bytes: 1,
                mtime: 1,
            },
            "text",
            1,
        )
        .unwrap();
    }

    // A: entered first, finished last. B: entered last, finished first — so
    // creation order and completion order disagree, and only one is "recent".
    db.conn()
        .execute("UPDATE document SET created_at = 1000 WHERE id = ?1", [&a])
        .unwrap();
    db.conn()
        .execute("UPDATE document SET created_at = 2000 WHERE id = ?1", [&b])
        .unwrap();
    db.conn()
        .execute(
            "UPDATE ingest_stage SET updated_at = 5000 WHERE content_hash = ?1 AND stage = 'chunk'",
            [&a],
        )
        .unwrap();
    db.conn()
        .execute(
            "UPDATE ingest_stage SET updated_at = 4000 WHERE content_hash = ?1 AND stage = 'chunk'",
            [&b],
        )
        .unwrap();

    let recents = db.recent_indexed_documents(50).unwrap();

    // Completion order, not creation order: A (finished last) before B.
    assert_eq!(
        recents
            .iter()
            .map(|d| d.document_id.as_str())
            .collect::<Vec<_>>(),
        vec![a.as_str(), b.as_str()],
    );
    // The exposed recency is the completion time, never the older created_at.
    assert_eq!(recents[0].indexed_at, 5000);
    assert_eq!(recents[1].indexed_at, 4000);
}

/// Owner-review P1-1: `list_tree` composes its listing from a roots-and-files
/// read and then a recents read, and the launcher's selection consumer (PR 6)
/// trusts that every recent's `(watched_root_id, relative_path)` is present in
/// some root's files. Those reads run on the window's connection while a
/// multi-hour indexing job commits `pending → indexed` on its OWN connection —
/// outside the window's mutex — so without one snapshot the recents read can see
/// a document the files read, taken one commit earlier, did not, and the listing
/// tears.
///
/// The oracle is `crates/mnema-index/tests/contention.rs`'s: a real second
/// connection commits a new indexed document, path and chunk/done stage in the
/// gap between the two phases, still inside the snapshot. Both halves are
/// asserted — the snapshot does not see the write, and a read taken after it
/// does — so this cannot pass over a database nobody wrote to. Run the two
/// phases WITHOUT the `read_snapshot` wrap (autocommit) and the recents phase
/// sees the intruder while the files phase did not: the coherence assertion
/// fails, which is the torn read this guards.
#[test]
fn the_tree_listing_reads_files_and_recents_from_one_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let reader = fresh(&dir);
    let writer = open(&dir.path().join("index.sqlite")).unwrap();

    // One indexed document with a path and a chunk/done checkpoint, so both
    // phases already have something coherent to return before the second
    // connection writes anything.
    let root = reader.insert_watched_root("/tmp/root").unwrap();
    let seeded = "a".repeat(64);
    reader
        .insert_document(&seeded, "text/plain", 1, SourceKind::Document)
        .unwrap();
    reader
        .set_document_status(&seeded, DocumentStatus::Indexed)
        .unwrap();
    reader.record_stage(&seeded, "chunk", "done").unwrap();
    reader
        .insert_path(
            root,
            "seeded.txt",
            &seeded,
            OnDisk {
                size_bytes: 1,
                mtime: 1,
            },
            "text",
            1,
        )
        .unwrap();

    // The document the SECOND connection commits between the two phases: a full
    // pending → indexed lifecycle, path and chunk/done stage.
    let intruder = "b".repeat(64);

    let (files, recents) = reader
        .read_snapshot(|db| {
            // Phase 1: every root's files, exactly as `build_tree_listing` reads
            // them, collected into the (root, path) set the recents must be a
            // subset of.
            let mut files: std::collections::HashSet<(i64, String)> =
                std::collections::HashSet::new();
            for r in db.list_watched_roots()? {
                for f in db.indexed_files_under_root(r.id)? {
                    files.insert((r.id, f.relative_path));
                }
            }

            // The indexing job's commit lands here, on its own connection,
            // before the recents read below — the gap `build_tree_listing`
            // leaves between its two phases.
            writer
                .insert_document(&intruder, "text/plain", 1, SourceKind::Document)
                .unwrap();
            writer
                .set_document_status(&intruder, DocumentStatus::Indexed)
                .unwrap();
            writer.record_stage(&intruder, "chunk", "done").unwrap();
            writer
                .insert_path(
                    root,
                    "intruder.txt",
                    &intruder,
                    OnDisk {
                        size_bytes: 1,
                        mtime: 1,
                    },
                    "text",
                    1,
                )
                .unwrap();

            // Phase 2: the recents.
            let recents = db.recent_indexed_documents(50)?;
            Ok((files, recents))
        })
        .expect("the snapshot closes cleanly");

    // Coherence: every recent the listing carries has its file in the set the
    // same listing built. Under one snapshot the intruder is in neither; in
    // autocommit it reaches the recents but not the files, and this fails.
    for rec in &recents {
        assert!(
            files.contains(&(rec.watched_root_id, rec.relative_path.clone())),
            "recents carries {:?} under root {}, absent from roots[].files — a torn read",
            rec.relative_path,
            rec.watched_root_id
        );
    }

    // The write really happened, so the coherence above is not over a database
    // nobody changed: a read taken after the snapshot sees the intruder.
    let after = reader.recent_indexed_documents(50).unwrap();
    assert!(
        after.iter().any(|d| d.document_id == intruder),
        "the second connection's commit never landed, so the snapshot proved nothing"
    );
}

// ---------------------------------------------------------------- exclusions

/// A rule belongs to the root it was set on. `ignore_rule.watched_root_id` has
/// always said so; nothing read or wrote the table until now, so this is the
/// first thing that would notice a query that dropped the root from its WHERE
/// clause — and a rule that leaked across roots would exclude a folder in a
/// place the person never asked, silently, until they went looking for a file.
#[test]
fn a_path_exclusion_belongs_to_one_root_only() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let a = db.insert_watched_root("/tmp/alpha").unwrap();
    let b = db.insert_watched_root("/tmp/beta").unwrap();

    assert!(db.add_path_exclusion(a, "Work/private").unwrap());

    assert_eq!(db.list_path_exclusions(a).unwrap(), vec!["Work/private"]);
    assert!(
        db.list_path_exclusions(b).unwrap().is_empty(),
        "the other root must not inherit the rule"
    );
}

/// Pressing "exclude" twice is one rule, and the second press says so rather
/// than failing: the caller is a window, and a person clicking again is not an
/// error state. `false` is the whole signal that nothing was written.
#[test]
fn adding_the_same_exclusion_twice_writes_one_row() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root = db.insert_watched_root("/tmp/alpha").unwrap();

    assert!(db.add_path_exclusion(root, "Photos").unwrap());
    assert!(
        !db.add_path_exclusion(root, "Photos").unwrap(),
        "the second add must report that it wrote nothing"
    );

    assert_eq!(db.list_path_exclusions(root).unwrap(), vec!["Photos"]);
}

/// Removing reports whether a row actually went. The window needs the two apart:
/// after a rename it offers to delete a rule whose folder is gone, and "there
/// was nothing there" is a different sentence from "removed".
#[test]
fn removing_an_exclusion_reports_whether_a_row_went() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root = db.insert_watched_root("/tmp/alpha").unwrap();
    db.add_path_exclusion(root, "Photos").unwrap();

    assert!(db.remove_path_exclusion(root, "Photos").unwrap());
    assert!(
        !db.remove_path_exclusion(root, "Photos").unwrap(),
        "removing a rule that is not there is not an error, and not a removal"
    );
    assert!(db.list_path_exclusions(root).unwrap().is_empty());
}

/// Several rules on one root come back in a stable order, so a window renders
/// the same list twice running.
#[test]
fn exclusions_come_back_sorted() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root = db.insert_watched_root("/tmp/alpha").unwrap();

    db.add_path_exclusion(root, "Work/private").unwrap();
    db.add_path_exclusion(root, "Archive").unwrap();
    db.add_path_exclusion(root, "Photos").unwrap();

    assert_eq!(
        db.list_path_exclusions(root).unwrap(),
        vec!["Archive", "Photos", "Work/private"]
    );
}

/// The cascade is the schema's (`schema.sql:52`), and it is asserted by
/// counting rows rather than by "no error": a cascade that silently stopped
/// working leaves rules pointing at a root that no longer exists, and the next
/// root to reuse that id inherits them.
#[test]
fn removing_a_watched_root_takes_its_exclusions_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let doomed = db.insert_watched_root("/tmp/alpha").unwrap();
    let kept = db.insert_watched_root("/tmp/beta").unwrap();
    db.add_path_exclusion(doomed, "Photos").unwrap();
    db.add_path_exclusion(kept, "Photos").unwrap();

    db.delete_watched_root(doomed).unwrap();

    let remaining: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM ignore_rule", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 1, "only the surviving root's rule may remain");
    assert_eq!(db.list_path_exclusions(kept).unwrap(), vec!["Photos"]);
}

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

/// 🔴 Fix round 2, A3. The row above cannot fail from the `ORDER BY` going: the
/// planner walks `ux_ignore_rule_path` (`migrations.rs:90-92`), which is
/// `(watched_root_id, path_prefix)`, so the rows come back sorted whether the
/// clause is there or not — `EXPLAIN QUERY PLAN` says
/// `SEARCH ignore_rule USING COVERING INDEX ux_ignore_rule_path`. Delete the
/// clause and the whole workspace stays at exit 0.
///
/// So the equivalence is **conditional on that index**, and this is the test
/// that says so out loud instead of a comment telling the next person the guard
/// cannot exist. Drop the index and the two orders come apart:
///
/// ```text
/// NO INDEX, no ORDER BY = ["a/deep", "a"]     <- insertion order
/// NO INDEX, ORDER BY    = ["a", "a/deep"]
/// ```
///
/// What rests on it: `SubfolderState::ExcludedByAncestor` promises the
/// **outermost** ancestor and gets it by taking the first match in this list
/// (`tree.rs:755-759`), and `Folders.svelte`'s `heldAbove` reads the same list
/// the same way to name the rule a person must remove first. A migration that
/// narrowed or dropped this index would make the clause load-bearing again with
/// nothing else noticing.
///
/// Descendant first, deliberately: with `["a", "a/deep"]` inserted in that
/// order, insertion order and sorted order agree and the fixture proves
/// nothing.
#[test]
fn exclusions_come_back_sorted_with_no_index_left_to_walk() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root = db.insert_watched_root("/tmp/alpha").unwrap();

    db.add_path_exclusion(root, "a/deep").unwrap();
    db.add_path_exclusion(root, "a").unwrap();

    db.conn()
        .execute_batch("DROP INDEX ux_ignore_rule_path;")
        .unwrap();

    assert_eq!(
        db.list_path_exclusions(root).unwrap(),
        vec!["a", "a/deep"],
        "with no index to walk, only the ORDER BY puts the outermost rule first"
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

// --------------------------------------------------------------------- masks

/// A mask is stored once however many times it is added, and the second add
/// says so rather than failing. The caller is a window and a person pressing
/// "add" again is not an error state; `false` is the whole signal that nothing
/// was written.
///
/// `pattern` is the table's PRIMARY KEY, so this is the schema's rule and not
/// the writer's — which is what lets `add_mask` be a bare `ON CONFLICT DO
/// NOTHING` with no target to keep in step with an index predicate.
#[test]
fn adding_the_same_mask_twice_writes_one_row() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    assert!(db.add_mask("*.pdf").unwrap());
    assert!(
        !db.add_mask("*.pdf").unwrap(),
        "the second add must report that it wrote nothing"
    );

    assert_eq!(db.list_masks().unwrap(), vec!["*.pdf"]);
}

/// 🔴 `*.PDF` and `*.pdf` are ONE rule to the mask layer, which folds case
/// (`caseless_form`, `mnema-walk/src/rules.rs`), and TWO rows to this table,
/// whose primary key compares bytes.
///
/// Both halves are asserted because both are the decision. Storing both is
/// harmless — they match exactly the same files — and deduplicating here would
/// mean rewriting or discarding what a person typed at the layer furthest from
/// them, with nothing on screen to say it happened. Telling them the two are
/// equivalent belongs to the editor (Task 11), which can say it in words.
#[test]
fn two_masks_differing_only_in_case_are_two_rows_and_neither_is_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    assert!(db.add_mask("*.pdf").unwrap());
    assert!(
        db.add_mask("*.PDF").unwrap(),
        "the key compares bytes, so the second spelling is a second row"
    );

    assert_eq!(
        db.list_masks().unwrap(),
        vec!["*.PDF", "*.pdf"],
        "both spellings must come back exactly as they were typed"
    );
}

/// Removing reports whether a row actually went — the same split the exclusion
/// trio makes, and for the same reason: "removed" and "there was nothing there"
/// are different sentences, and the second one is what a window shows after
/// another window removed the rule first.
#[test]
fn removing_a_mask_reports_whether_a_row_went() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    db.add_mask("*.pdf").unwrap();

    assert!(db.remove_mask("*.pdf").unwrap());
    assert!(
        !db.remove_mask("*.pdf").unwrap(),
        "removing a mask that is not there is not an error, and not a removal"
    );
    assert!(db.list_masks().unwrap().is_empty());
}

/// Removing one mask leaves the others alone. A `DELETE` that lost its `WHERE`
/// clause empties the whole table, and every test above would still pass:
/// each of them holds exactly one mask at the moment it removes one.
#[test]
fn removing_one_mask_leaves_the_others_standing() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    db.add_mask("*.pdf").unwrap();
    db.add_mask("*.tmp").unwrap();

    assert!(db.remove_mask("*.pdf").unwrap());

    assert_eq!(
        db.list_masks().unwrap(),
        vec!["*.tmp"],
        "the mask that was not named must still be there"
    );
}

/// Several masks come back in a stable order, so a window renders the same list
/// twice running.
///
/// ⚠️ **This test cannot fail from the `ORDER BY` going, and saying so is the
/// point.** It was written believing it could — `file_mask` is a rowid table, so
/// insertion order looked like the alternative — and the probe said otherwise:
/// deleting the clause left all 18 tests in this file green. `pattern` is the
/// PRIMARY KEY, SQLite builds an implicit unique index for it, and the query is
/// answered from that index already sorted. The masks are still inserted out of
/// order, because the test is about the promise to the window and not about the
/// clause; what the promise actually rests on is asserted by
/// `the_mask_listing_is_answered_from_the_index_that_sorts_it` below.
#[test]
fn masks_come_back_sorted() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    db.add_mask("*.tmp").unwrap();
    db.add_mask("*.pdf").unwrap();
    db.add_mask("*.log").unwrap();

    assert_eq!(
        db.list_masks().unwrap(),
        vec!["*.log", "*.pdf", "*.tmp"],
        "insertion order would put *.tmp first"
    );
}

/// 🔴 The premise `masks_come_back_sorted` silently rests on: the listing is
/// answered from the primary key's own index, which is what makes it sorted
/// with or without the `ORDER BY`.
///
/// This is here because the equivalence is conditional and the condition is
/// invisible. `exclusions_come_back_sorted_with_no_index_left_to_walk` can drop
/// its index and watch the two orders come apart; `sqlite_autoindex_file_mask_1`
/// is implicit and cannot be dropped, so the premise is read off the plan
/// instead. A migration that gave `file_mask` a surrogate `id` key, or reordered
/// a composite one, would make the clause load-bearing again — and this line is
/// what would say so, rather than the ordering quietly becoming insertion order
/// on somebody's screen.
///
/// `SCAN`, not merely "an index is mentioned": a plan that used the index for a
/// lookup and then sorted would say `USE TEMP B-TREE FOR ORDER BY`, and that is
/// the plan in which the clause is doing the work.
#[test]
fn the_mask_listing_is_answered_from_the_index_that_sorts_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let plan: String = db
        .conn()
        .query_row(
            "EXPLAIN QUERY PLAN SELECT pattern FROM file_mask ORDER BY pattern",
            [],
            |r| r.get(3),
        )
        .unwrap();

    assert!(
        plan.contains("SCAN file_mask USING COVERING INDEX sqlite_autoindex_file_mask_1"),
        "the listing must be answered from the primary key's own index, which is \
         why the ORDER BY is redundant: {plan}"
    );
    assert!(
        !plan.contains("TEMP B-TREE"),
        "a plan that sorts separately means the ORDER BY is load-bearing after \
         all, and masks_come_back_sorted is then the only thing holding it: {plan}"
    );
}

/// 🔴 A mask belongs to no watched root, and removing a root takes none with
/// it. This is D-c's storage half: `file_mask` carries no `watched_root_id`, so
/// there is no cascade to fire — and a person who removes one watched folder
/// keeps the rule that was hiding their temporary files everywhere else.
///
/// Both directions: the exclusion on the removed root goes (the cascade that
/// does exist still works), and the mask stays.
#[test]
fn removing_a_watched_root_leaves_the_masks_alone() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let doomed = db.insert_watched_root("/tmp/alpha").unwrap();
    db.add_path_exclusion(doomed, "Photos").unwrap();
    db.add_mask("*.pdf").unwrap();

    db.delete_watched_root(doomed).unwrap();

    let rules: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM ignore_rule", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rules, 0, "the root's own exclusion must go with it");
    assert_eq!(
        db.list_masks().unwrap(),
        vec!["*.pdf"],
        "a mask is global: it survives the removal of any one watched folder"
    );
}

// ── The settings window's two whole-index numbers (§9.3) ─────────────────────
//
// `indexed_file_count` and `last_indexed_at` are each defined as one of the
// two listings above with a filter dropped, and the tests below assert that
// relationship rather than only the number: an equality against
// `indexed_files_under_root` and an equality against `recent_indexed_documents`
// are what keep the settings screen from contradicting the folder list and the
// recents list drawn one click away.

/// One indexed document with one path under `root`, completed at `updated_at`.
///
/// `record_stage` stamps `unixepoch()`, so a test that wants a known moment
/// overwrites it — the same `UPDATE ingest_stage` the recents tests above use.
fn seed_indexed(db: &Db, root: i64, id: &str, relative_path: &str, updated_at: Option<i64>) {
    db.insert_document(id, "text/plain", 1, SourceKind::Document)
        .unwrap();
    db.set_document_status(id, DocumentStatus::Indexed).unwrap();
    db.insert_path(
        root,
        relative_path,
        id,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();
    if let Some(at) = updated_at {
        db.record_stage(id, "chunk", "done").unwrap();
        db.conn()
            .execute(
                "UPDATE ingest_stage SET updated_at = ?2 WHERE content_hash = ?1 AND stage = 'chunk'",
                rusqlite::params![id, at],
            )
            .unwrap();
    }
}

/// The whole-index count is the sum of the per-folder counts, not a number
/// arrived at some other way.
///
/// The second assertion is the load-bearing one: the first would be satisfied
/// by any query that happens to answer 5 on this fixture, and only the equality
/// against `indexed_files_under_root` — the very rows `Folders.svelte` counts
/// into its own label — says the two surfaces cannot disagree.
#[test]
fn the_whole_index_count_is_the_sum_of_every_folder_s_own_count() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let a = db.insert_watched_root("/tmp/a").unwrap();
    let b = db.insert_watched_root("/tmp/b").unwrap();

    for (i, (root, name)) in [
        (a, "a1.txt"),
        (a, "a2.txt"),
        (b, "b1.txt"),
        (b, "b2.txt"),
        (b, "b3.txt"),
    ]
    .iter()
    .enumerate()
    {
        seed_indexed(&db, *root, &format!("{i:064x}"), name, Some(1000));
    }

    let count = db.indexed_file_count().unwrap();

    assert_eq!(count, 5);
    assert_eq!(
        count,
        (db.indexed_files_under_root(a).unwrap().len()
            + db.indexed_files_under_root(b).unwrap().len()) as i64,
        "the settings number must be exactly the folder rows' numbers added up"
    );
}

/// 🔴 The deliberate departure from a `COUNT(*) FROM document` (D-e). One
/// document reachable from two watched folders draws TWO folder rows, each
/// claiming a file, so the whole-index number says two as well.
///
/// Asserted positively, with the number: a later change that de-duplicates by
/// document goes red here and has to argue with this comment rather than be
/// slipped in as a tidy-up.
#[test]
fn a_document_reachable_from_two_folders_counts_once_for_each_folder_row() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let a = db.insert_watched_root("/tmp/a").unwrap();
    let b = db.insert_watched_root("/tmp/b").unwrap();

    let shared = "a".repeat(64);
    seed_indexed(&db, a, &shared, "copy.txt", Some(1000));
    // The SAME document (one sha256) under the second root — two copies of one
    // file, which is one `document` row and two `path` rows.
    db.insert_path(
        b,
        "copy.txt",
        &shared,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();

    assert_eq!(
        db.indexed_file_count().unwrap(),
        2,
        "one document, two folder rows, two files claimed on the screen"
    );
    assert_eq!(
        db.indexed_file_count().unwrap(),
        (db.indexed_files_under_root(a).unwrap().len()
            + db.indexed_files_under_root(b).unwrap().len()) as i64,
    );
}

/// Only `indexed` counts: a pending document has nothing searchable in it yet
/// and a failed one never will, and neither is drawn under a folder.
///
/// Two assertions, because "the count is 1" alone is also satisfied by a
/// fixture that never inserted the two rows the filter is supposed to remove.
/// So the count of path rows is asserted first, and it can fail on its own: the
/// index holds 3, and the helper answers 1.
#[test]
fn a_pending_or_failed_document_is_not_an_indexed_file() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root = db.insert_watched_root("/tmp/root").unwrap();

    seed_indexed(&db, root, &"a".repeat(64), "done.txt", Some(1000));
    for (id, status) in [
        (&"b".repeat(64), DocumentStatus::Pending),
        (&"c".repeat(64), DocumentStatus::Failed),
    ] {
        db.insert_document(id, "text/plain", 1, SourceKind::Document)
            .unwrap();
        db.set_document_status(id, status).unwrap();
        db.insert_path(
            root,
            &format!("{status:?}.txt"),
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

    // The fixture really holds the state this test is about, asserted before the
    // number is: three path rows, of which the status filter must keep two out.
    // `assert_ne!(count, 3)` stood here and could not fail while the equality
    // below passed — it named the wrong answer without discriminating anything,
    // and a fixture whose two extra rows had silently failed to insert would
    // have satisfied it. This one goes red on that, which is the direction the
    // brief was asking for.
    let path_rows: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM path", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        path_rows, 3,
        "the fixture must hold three path rows for the filter to have work to do"
    );

    assert_eq!(
        db.indexed_file_count().unwrap(),
        1,
        "only the indexed document is a file on the screen, and the pending and \
         failed rows beside it are the other two of the {path_rows} path rows"
    );
}

/// An index nobody has walked yet answers both questions rather than failing
/// either — the state the settings window is first opened in.
#[test]
fn an_empty_index_reports_no_files_and_no_moment() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    assert_eq!(db.indexed_file_count().unwrap(), 0);
    assert_eq!(db.last_indexed_at().unwrap(), None);
}

/// 🔴 The reason `last_indexed_at` reuses `recent_indexed_documents`' join
/// rather than writing a second one: the moment the settings line names is the
/// moment the top of the Recents list carries. The second assertion is what
/// makes that a property instead of a coincidence of this fixture.
#[test]
fn the_last_indexed_moment_is_the_one_at_the_top_of_recents() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root = db.insert_watched_root("/tmp/root").unwrap();

    seed_indexed(&db, root, &"a".repeat(64), "old.txt", Some(1000));
    seed_indexed(&db, root, &"b".repeat(64), "newest.txt", Some(3000));
    seed_indexed(&db, root, &"c".repeat(64), "middle.txt", Some(2000));

    let last = db.last_indexed_at().unwrap();

    assert_eq!(last, Some(3000), "the newest completion, not the oldest");
    assert_eq!(
        last,
        Some(db.recent_indexed_documents(1).unwrap()[0].indexed_at),
        "the settings line and the top of Recents must name one moment"
    );
}

/// An `indexed` document with no chunk/done stage row is outside
/// `recent_indexed_documents`' INNER JOIN, and this number is outside it in
/// exactly the same way — the two surfaces stay in step even in the state
/// neither expects.
///
/// Both directions, and they are what makes this fixture distinguish anything:
/// the document IS present and IS indexed, so the count sees it; only the
/// moment is absent, because nothing ever recorded a completion.
#[test]
fn an_indexed_document_that_never_recorded_a_completion_names_no_moment() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root = db.insert_watched_root("/tmp/root").unwrap();

    seed_indexed(&db, root, &"a".repeat(64), "silent.txt", None);

    assert_eq!(
        db.indexed_file_count().unwrap(),
        1,
        "the document is there and it is indexed"
    );
    assert_eq!(
        db.last_indexed_at().unwrap(),
        None,
        "no chunk/done stage, so nothing has finished indexing"
    );
}

/// A rebuild in flight has not finished. `STATUS_REBUILDING` exists precisely
/// so a document whose rows are being written again cannot read as done, and
/// the settings line must not report its newer `updated_at` as the moment the
/// index was last brought up to date.
///
/// Two assertions, and the first can fail on its own: the rebuilding stage
/// really stands at 3000, so it was the newest moment available, and the helper
/// answers the completed document's 1000 anyway.
#[test]
fn a_rebuild_in_flight_is_not_a_completion() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root = db.insert_watched_root("/tmp/root").unwrap();

    seed_indexed(&db, root, &"a".repeat(64), "finished.txt", Some(1000));
    let rebuilding = "b".repeat(64);
    seed_indexed(&db, root, &rebuilding, "rebuilding.txt", None);
    db.record_stage(&rebuilding, "chunk", "rebuilding").unwrap();
    db.conn()
        .execute(
            "UPDATE ingest_stage SET updated_at = 3000 WHERE content_hash = ?1 AND stage = 'chunk'",
            [&rebuilding],
        )
        .unwrap();

    // The rebuilding stage really carries the newer moment, asserted before the
    // helper is asked anything. `assert_ne!(last, Some(3000))` stood here and
    // could not fail while the equality below passed; worse, it read as a guard
    // against a 3000 that a fixture whose UPDATE had quietly matched no row
    // would never have held in the first place. This assertion is the one that
    // goes red on that, and it is what makes the equality below mean "the newer
    // moment was there to be picked and was not picked".
    let rebuilding_at: i64 = db
        .conn()
        .query_row(
            "SELECT updated_at FROM ingest_stage WHERE content_hash = ?1 AND stage = 'chunk'",
            [&rebuilding],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        rebuilding_at, 3000,
        "the rebuild in flight must be the NEWEST stage row, or this test proves nothing"
    );

    assert_eq!(
        db.last_indexed_at().unwrap(),
        Some(1000),
        "the finished document's completion, not the rebuild's newer {rebuilding_at}"
    );
}

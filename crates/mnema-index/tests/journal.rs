//! The three journals `schema.sql` sets aside for work that did not make it
//! into the index, or that already has: `skipped`, `document.status` and
//! `ingest_stage`. Before this file none of the three had a writer anywhere —
//! `grep "INTO skipped"` over the crates returned nothing but the schema
//! itself.

use mnema_core::{OnDisk, SourceKind};
use mnema_index::{Db, DocumentStatus, SkipRule, open, register_vector_extension};

fn fresh(dir: &tempfile::TempDir) -> Db {
    register_vector_extension().unwrap();
    open(&dir.path().join("index.sqlite")).unwrap()
}

/// Keeps the backing `TempDir` alive alongside the `Db` that opened a file
/// inside it — the same shape `tests/citation.rs`'s `OnePage` uses, and for
/// the same reason: dropping the directory out from under an open connection
/// is not something a fixture should risk.
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

/// One watched root, id 1, and nothing else. `record_skip` takes a root id and
/// `skipped.watched_root_id` is a foreign key, so a skip cannot be recorded
/// against a root that was never inserted.
fn fixture_empty() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    db.insert_watched_root("/Volumes/Archive").unwrap();
    Fixture { db, _dir: dir }
}

/// `fixture_empty` plus one document, `"doc-1"`, for the status tests.
fn fixture_one_document() -> Fixture {
    let f = fixture_empty();
    f.db.insert_document("doc-1", "application/pdf", 1, SourceKind::Document)
        .unwrap();
    f
}

#[test]
fn a_skipped_file_names_the_rule_that_fired() {
    let db = fixture_empty();
    db.record_skip(
        1,
        "reports/broken.pdf",
        None,
        "worker died on SIGSEGV",
        SkipRule::Crash,
        None,
    )
    .unwrap();
    let rows = db.skips_for_root(1).unwrap();
    assert_eq!(rows[0].rule, "crash");
    assert_eq!(rows[0].page_no, None);
}

#[test]
fn a_page_without_a_text_layer_is_recorded_against_that_page() {
    let db = fixture_empty();
    db.record_skip(
        1,
        "scan.pdf",
        Some(4),
        "no text layer",
        SkipRule::NoTextLayer,
        None,
    )
    .unwrap();
    assert_eq!(db.skips_for_root(1).unwrap()[0].page_no, Some(4));
}

/// The two tests above between them pin only `Crash` and `NoTextLayer`, and
/// only through `page_no`/one literal — neither ever checks `Timeout`,
/// `Memory` or `Unsupported`, and neither compares `rows[0].rule` against
/// anything for the page-without-a-text-layer case. A mutation pass proved
/// the gap: renaming `SkipRule::NoTextLayer`'s string to `"crash"` left every
/// test in this file green. Each variant is asserted against its own string
/// here, so a rename or a collision between any two of the five is caught
/// regardless of which one moved.
#[test]
fn every_skip_rule_is_recorded_under_its_own_string() {
    let db = fixture_empty();
    let cases = [
        (SkipRule::Crash, "crash"),
        (SkipRule::Timeout, "timeout"),
        (SkipRule::Memory, "memory"),
        (SkipRule::Unsupported, "unsupported"),
        (SkipRule::NoTextLayer, "no_text_layer"),
        (SkipRule::Unreadable, "unreadable"),
        (SkipRule::TooLarge, "too_large"),
    ];
    for (i, (rule, _)) in cases.iter().enumerate() {
        db.record_skip(1, &format!("file-{i}.pdf"), None, "reason", *rule, None)
            .unwrap();
    }

    let rows = db.skips_for_root(1).unwrap();
    let got: Vec<&str> = rows.iter().map(|r| r.rule.as_str()).collect();
    let expected: Vec<&str> = cases.iter().map(|(_, s)| *s).collect();
    assert_eq!(got, expected);
}

/// Every `SkipRule` variant, sorted onto its side of `is_about_content`
/// explicitly rather than derived from a count or a default. The mis-sorting
/// this pins — `TooLarge`, a fact about the *setting* `PoolConfig::max_bytes`
/// rather than about the file, wrongly placed on the content side — was
/// caught only by the randomised harness in `mnema-ingest`, on a random seed,
/// which means it was caught *sometimes*. This is the deterministic form.
///
/// On its own it does not force a decision about a NEW variant, and it was
/// measured making exactly that mistake look covered: a variant added to the
/// enum with no line here left this whole suite green. What forces the
/// decision is that `is_about_content` is an exhaustive `match` — adding a
/// variant stops the crate compiling until someone picks a side. This list is
/// the other half: it says which side each existing variant is on, in one
/// place, where a wrong answer is legible.
#[test]
fn every_skip_rule_is_sorted_onto_its_side_of_is_about_content() {
    let cases = [
        (SkipRule::Crash, false),
        (SkipRule::Timeout, false),
        (SkipRule::Memory, false),
        (SkipRule::Unsupported, true),
        (SkipRule::NoTextLayer, true),
        (SkipRule::Unreadable, false),
        (SkipRule::TooLarge, false),
    ];
    for (rule, expected) in cases {
        assert_eq!(
            rule.is_about_content(),
            expected,
            "{rule:?} is on the wrong side of is_about_content"
        );
    }
}

/// The journal is a current state, not a history. Before this, `record_skip`
/// was an unconditional INSERT: a folder of a thousand scans grew a thousand
/// rows per walk, and every walk spent a worker process on each of them to
/// learn the same thing again.
#[test]
fn a_second_skip_of_the_same_file_replaces_the_first() {
    let db = fixture_empty();
    let root = db.insert_watched_root("/tmp/x").unwrap();

    db.record_skip(
        root,
        "a.pdf",
        None,
        "no text layer",
        SkipRule::NoTextLayer,
        None,
    )
    .unwrap();
    db.record_skip(
        root,
        "a.pdf",
        None,
        "still none",
        SkipRule::NoTextLayer,
        None,
    )
    .unwrap();

    let skips = db.skips_for_root(root).unwrap();
    assert_eq!(skips.len(), 1);
    assert_eq!(skips[0].reason, "still none");
}

/// What this test actually pins is `page_no` being *in* the unique key at
/// all, not the `COALESCE` wrapped around it. Drop `page_no` from the index
/// (and from `record_skip`'s `ON CONFLICT` arbiter) entirely and this fails
/// loudly — `left: 1, right: 2` — because the whole-file skip and the page-4
/// skip now share one key and the second overwrites the first. The COALESCE
/// trap proper — two whole-file skips of the same path silently failing to
/// dedup because SQLite treats NULL as DISTINCT from NULL — is what
/// `a_second_skip_of_the_same_file_replaces_the_first` above pins; this test
/// covers the other row that indexing shares, not that one.
#[test]
fn page_skips_and_file_skips_do_not_collide_but_each_still_dedups() {
    let db = fixture_empty();
    let root = db.insert_watched_root("/tmp/x").unwrap();

    db.record_skip(
        root,
        "a.pdf",
        None,
        "whole file",
        SkipRule::NoTextLayer,
        None,
    )
    .unwrap();
    db.record_skip(
        root,
        "a.pdf",
        Some(4),
        "page four",
        SkipRule::NoTextLayer,
        None,
    )
    .unwrap();
    db.record_skip(
        root,
        "a.pdf",
        Some(4),
        "page four again",
        SkipRule::NoTextLayer,
        None,
    )
    .unwrap();

    let skips = db.skips_for_root(root).unwrap();
    assert_eq!(skips.len(), 2);
}

/// Content rules remember the bytes; environmental rules must not. A crash is a
/// statement about the worker, not about the file, and every file in the walk
/// is subject to it — D44's own asymmetry, reused rather than invented twice.
#[test]
fn only_content_rules_remember_the_bytes() {
    let db = fixture_empty();
    let root = db.insert_watched_root("/tmp/x").unwrap();

    db.record_skip(
        root,
        "a.bin",
        None,
        "no reader",
        SkipRule::Unsupported,
        Some(OnDisk {
            size_bytes: 10,
            mtime: 99,
        }),
    )
    .unwrap();
    db.record_skip(
        root,
        "b.pdf",
        None,
        "worker died",
        SkipRule::Crash,
        Some(OnDisk {
            size_bytes: 10,
            mtime: 99,
        }),
    )
    .unwrap();

    assert_eq!(
        db.skip_entry(root, "a.bin").unwrap().unwrap().size_bytes,
        Some(10)
    );
    assert_eq!(
        db.skip_entry(root, "b.pdf").unwrap().unwrap().size_bytes,
        None
    );
}

#[test]
fn a_document_that_failed_says_so() {
    let db = fixture_one_document();
    db.set_document_status("doc-1", DocumentStatus::Failed)
        .unwrap();
    assert_eq!(db.document_status("doc-1").unwrap(), DocumentStatus::Failed);
}

/// Same gap, the other table: only `Failed` had ever been written and read
/// back. Checked against the raw stored string rather than through
/// `document_status()`, so a bug shared between `as_str` and `parse` — the
/// two swapping the same pair of variants — cannot cancel itself out the way
/// a pure round trip could.
#[test]
fn every_document_status_is_written_as_its_own_string() {
    let cases = [
        (DocumentStatus::Pending, "pending"),
        (DocumentStatus::Indexed, "indexed"),
        (DocumentStatus::Failed, "failed"),
        (DocumentStatus::Skipped, "skipped"),
    ];
    for (status, expected) in cases {
        let db = fixture_one_document();
        db.set_document_status("doc-1", status).unwrap();
        let stored: String = db
            .conn()
            .query_row("SELECT status FROM document WHERE id = 'doc-1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(stored, expected);
    }
}

/// `ingest_stage` is keyed on `(content_hash, stage)`, so re-recording the same
/// stage for the same document — exactly what happens when a multi-hour job is
/// resumed and repeats a stage it did not finish — must update the row rather
/// than fail with a uniqueness violation. Read back with raw SQL rather than
/// through `stage_status`, so that a bug shared between the writer and the
/// reader cannot cancel itself out.
#[test]
fn recording_a_stage_twice_updates_it_instead_of_failing() {
    let db = fixture_empty();
    let hash = db
        .insert_document(&"a".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    db.record_stage(&hash, "extract", "done").unwrap();
    db.record_stage(&hash, "extract", "failed").unwrap();

    let status: String = db
        .conn()
        .query_row(
            "SELECT status FROM ingest_stage WHERE content_hash = ?1 AND stage = ?2",
            rusqlite::params![hash, "extract"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "failed");

    let rows: i64 = db
        .conn()
        .query_row(
            "SELECT count(*) FROM ingest_stage WHERE content_hash = ?1 AND stage = ?2",
            rusqlite::params![hash, "extract"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1, "the second call must update the row, not add one");
}

/// Until now `ingest_stage` could only be written. A checkpoint nothing reads
/// is not a checkpoint: the whole reason D26 asks for the table is that a
/// second pass over the same folder can skip a document whose stages are
/// already finished, and skipping needs an answer to "did this one finish?".
///
/// `Option`, and the distinction is the point. A stage that was never recorded
/// and a stage recorded as `failed` demand opposite things of the next run —
/// do the work, and do not do it again — so they must not both arrive as some
/// falsy value.
#[test]
fn a_stage_can_be_read_back_and_an_unrecorded_one_is_absent() {
    let db = fixture_empty();
    let hash = db
        .insert_document(&"a".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();

    assert_eq!(
        db.stage_status(&hash, "chunk").unwrap(),
        None,
        "a document nobody has worked on has no stage recorded"
    );

    db.record_stage(&hash, "chunk", "done").unwrap();
    assert_eq!(
        db.stage_status(&hash, "chunk").unwrap().as_deref(),
        Some("done")
    );

    // The key is the pair, not the hash: another stage of the same document is
    // a different row and must not answer for this one.
    assert_eq!(db.stage_status(&hash, "embed").unwrap(), None);
    // …and neither must the same stage of another document.
    assert_eq!(db.stage_status(&"b".repeat(64), "chunk").unwrap(), None);
}

/// A checkpoint may not outlive the document it describes.
///
/// `ingest_stage` is keyed on the *content* hash, and content comes back: an
/// undo, a restore from backup, a file moved out and back. Without the cascade
/// a `done` stage sat waiting for the next document to carry that hash — and
/// since a fresh document is inserted at `status = 'pending'`, any interruption
/// before its own checkpoint left `done` over `pending` permanently, with every
/// future walk short-circuiting on the stage before it could repair the status.
///
/// Found twice, independently: by a review reasoning about what can disappear
/// from the index, and by the randomised harness, which reached it from seven
/// of eight disjoint seed ranges.
#[test]
fn a_documents_stages_go_when_the_document_does() {
    let db = fixture_empty();
    let doc = db
        .insert_document(&"c".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    db.record_stage(&doc, "chunk", "done").unwrap();
    db.record_stage(&doc, "embed", "done").unwrap();
    assert_eq!(
        db.stage_status(&doc, "chunk").unwrap().as_deref(),
        Some("done")
    );

    db.delete_document(&doc).unwrap();

    assert_eq!(
        db.stage_status(&doc, "chunk").unwrap(),
        None,
        "a checkpoint outlived its document, and content addressing means that \
         hash comes back"
    );
    assert_eq!(db.stage_status(&doc, "embed").unwrap(), None);

    // …and the hash coming back finds nothing waiting for it.
    let again = db
        .insert_document(&"c".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    assert_eq!(db.stage_status(&again, "chunk").unwrap(), None);
}

/// A stage cannot be recorded for content no `document` row names.
///
/// The other side of the cascade, and the constraint the embedding stages
/// inherit: every stage this design has is written after the document exists,
/// and a checkpoint for a document that was never inserted is a checkpoint for
/// nothing. A future stage that wants to check-point earlier inserts the row
/// first rather than removing the foreign key.
#[test]
fn a_stage_for_a_document_that_does_not_exist_is_refused() {
    let db = fixture_empty();
    let err = db
        .record_stage(&"d".repeat(64), "chunk", "done")
        .expect_err("a checkpoint for nothing must not be storable");
    assert!(
        err.to_string().contains("FOREIGN KEY constraint failed"),
        "expected a foreign key violation, got: {err}"
    );
}

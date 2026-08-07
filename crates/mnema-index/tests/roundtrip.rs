use mnema_core::{OnDisk, SourceKind};
use mnema_index::{Db, open, register_vector_extension};

fn fresh(dir: &tempfile::TempDir) -> Db {
    register_vector_extension().unwrap();
    open(&dir.path().join("index.sqlite")).unwrap()
}

#[test]
fn a_fresh_database_migrates_and_reopens() {
    register_vector_extension().expect("extension registers once per process");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");

    let db = open(&path).expect("first open creates and migrates");
    assert_eq!(db.schema_version().unwrap(), mnema_index::SCHEMA_VERSION);
    drop(db);

    // Reopening must be a no-op, not a second migration.
    let db = open(&path).expect("second open succeeds");
    assert_eq!(db.schema_version().unwrap(), mnema_index::SCHEMA_VERSION);
}

#[test]
fn wal_and_foreign_keys_are_on() {
    register_vector_extension().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = open(&dir.path().join("index.sqlite")).unwrap();

    let mode: String = db
        .conn()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");

    let fk: i64 = db
        .conn()
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1);
}

/// Catches the PRAGMA never being issued — the default is FULL (2) — and a
/// value that moves synchronous the wrong way. It does NOT catch a typo in the
/// value string: SQLite falls back to NORMAL for text it does not recognise, so
/// `synchronous='NOTAVALUE'` reads back as 1 exactly like the correct spelling.
#[test]
fn synchronous_is_lowered_from_the_default() {
    register_vector_extension().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = open(&dir.path().join("index.sqlite")).unwrap();

    let synchronous: i64 = db
        .conn()
        .query_row("PRAGMA synchronous", [], |r| r.get(0))
        .unwrap();
    assert_eq!(synchronous, 1, "expected NORMAL (1), got {synchronous}");
}

#[test]
fn the_vector_extension_is_actually_loaded() {
    register_vector_extension().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = open(&dir.path().join("index.sqlite")).unwrap();

    let version: String = db
        .conn()
        .query_row("SELECT vec_version()", [], |r| r.get(0))
        .expect("vec_version() is callable, so the static link worked");
    assert!(version.starts_with("v0.1."), "unexpected: {version}");
}

/// A `path` row remembers which reader made its document, not only how big the
/// file was and when it changed.
///
/// Without these two columns the cheap arm compares a file against the disk and
/// never against the code that read it, so a format whose reader changes hands —
/// `.html` read as text today, by an html reader tomorrow — answers "unchanged"
/// for the life of the index. Nothing logs it, and nothing else in the schema
/// can notice: `INDEX_FORMAT_VERSION` is on `chunk` and on `skipped`, and
/// neither is consulted by that arm.
#[test]
fn a_path_row_remembers_which_reader_made_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let root = db.insert_watched_root("/tmp/root").unwrap();
    let doc = db
        .insert_document(&"a".repeat(64), "text/plain", 10, SourceKind::Document)
        .unwrap();
    db.insert_path(
        root,
        "a.txt",
        &doc,
        OnDisk {
            size_bytes: 10,
            mtime: 1234,
        },
        "text",
        1,
    )
    .unwrap();

    let entry = db.path_entry(root, "a.txt").unwrap().expect("a row");
    assert_eq!(entry.reader, "text");
    assert_eq!(entry.reader_version, 1);
    // Both directions: the other three columns still round-trip. A migration
    // that adds columns by rebuilding the table can silently drop them.
    assert_eq!(entry.document_id, doc);
    assert_eq!(entry.size_bytes, 10);
    assert_eq!(entry.mtime, 1234);
}

/// The two columns carry what the writer said, not a value the reader invented.
///
/// The test above would pass just as well against a `path_entry` that returned
/// `"text"` and `1` from constants, because `"text"` and `1` are also the
/// migration's defaults — the very values the sibling test needs them to be.
/// So the round trip is asserted a second time with a reader that shares no
/// character with the default and a version that is not 1.
#[test]
fn a_path_row_carries_the_reader_it_was_given_and_not_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let root = db.insert_watched_root("/tmp/root").unwrap();
    let doc = db
        .insert_document(&"b".repeat(64), "text/markdown", 3, SourceKind::Document)
        .unwrap();
    db.insert_path(
        root,
        "n.md",
        &doc,
        OnDisk {
            size_bytes: 3,
            mtime: 9,
        },
        "markdown",
        7,
    )
    .unwrap();

    let entry = db.path_entry(root, "n.md").unwrap().expect("a row");
    assert_eq!(entry.reader, "markdown");
    assert_eq!(entry.reader_version, 7);
}

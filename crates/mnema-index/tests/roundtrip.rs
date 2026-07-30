use mnema_index::{open, register_vector_extension};

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

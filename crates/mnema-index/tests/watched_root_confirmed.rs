//! D202: a watched folder added by a person waits for confirmation before any
//! scan reads it; one added any other way is confirmed from the start.

use mnema_index::{Db, open, register_vector_extension};

fn fresh(dir: &tempfile::TempDir) -> Db {
    register_vector_extension().unwrap();
    open(&dir.path().join("index.sqlite")).unwrap()
}

fn confirmed(db: &Db, id: i64) -> bool {
    db.list_watched_roots()
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .expect("the root is listed")
        .confirmed
}

#[test]
fn insert_watched_root_is_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let id = db.insert_watched_root("/tmp/a").unwrap();
    assert!(confirmed(&db, id));
}

#[test]
fn insert_unconfirmed_is_not_confirmed_until_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let id = db.insert_unconfirmed_watched_root("/tmp/a").unwrap();
    assert!(!confirmed(&db, id));
    assert!(db.confirm_watched_root(id).unwrap());
    assert!(confirmed(&db, id));
    assert!(
        db.confirm_watched_root(id).unwrap(),
        "confirming twice still names an existing root"
    );
    assert!(confirmed(&db, id));
}

#[test]
fn confirm_unknown_root_is_false() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    assert!(!db.confirm_watched_root(999).unwrap());
}

#[test]
fn confirming_one_leaves_the_other() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let a = db.insert_unconfirmed_watched_root("/tmp/a").unwrap();
    let b = db.insert_unconfirmed_watched_root("/tmp/b").unwrap();
    db.confirm_watched_root(a).unwrap();
    assert!(confirmed(&db, a));
    assert!(!confirmed(&db, b));
}

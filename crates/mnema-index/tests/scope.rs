//! `search_folder`: the folders the person has switched search off for.
//! Only changes are stored; a missing row means "searched" (spec §4).

use mnema_index::{Db, SCHEMA_VERSION};

mod support;
use support::temp_db;

fn rows(db: &Db) -> i64 {
    db.conn()
        .query_row("SELECT count(*) FROM search_folder", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn setting_a_folder_clears_the_entries_under_it() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    db.set_folder_searched(root, "notes/a", true).unwrap();
    db.set_folder_searched(root, "notes/a/b", false).unwrap();
    db.set_folder_searched(root, "notes2", true).unwrap();

    db.set_folder_searched(root, "notes", false).unwrap();

    assert_eq!(
        db.search_folders(root).unwrap(),
        vec![("notes".to_string(), false), ("notes2".to_string(), true)]
    );
}

#[test]
fn the_root_entry_clears_every_entry_of_its_root_only() {
    let db = temp_db();
    let a = db.insert_watched_root("/a").unwrap();
    let b = db.insert_watched_root("/b").unwrap();
    db.set_folder_searched(a, "notes", false).unwrap();
    db.set_folder_searched(b, "notes", false).unwrap();

    db.set_folder_searched(a, "", false).unwrap();

    assert_eq!(db.search_folders(a).unwrap(), vec![(String::new(), false)]);
    assert_eq!(
        db.search_folders(b).unwrap(),
        vec![("notes".to_string(), false)]
    );
}

#[test]
fn a_removed_root_takes_its_entries_with_it() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    db.set_folder_searched(root, "notes", false).unwrap();
    assert_eq!(rows(&db), 1);

    db.delete_watched_root(root).unwrap();

    assert_eq!(rows(&db), 0);
}

#[test]
fn a_dead_root_id_writes_nothing_and_does_not_fail() {
    let db = temp_db();
    db.set_folder_searched(9999, "x", false).unwrap();
    assert_eq!(rows(&db), 0);
}

#[test]
fn the_migration_is_the_sixth() {
    assert_eq!(SCHEMA_VERSION, 6);
    let db = temp_db();
    assert_eq!(rows(&db), 0, "a fresh database has the table, empty");
}

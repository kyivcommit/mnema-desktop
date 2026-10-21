//! `search_folder`: the folders the person has switched search off for.
//! Only changes are stored; a missing row means "searched" (spec §4).

use mnema_core::{Block, BlockType, Coordinate, Locator, OnDisk, Segment, SourceKind};
use mnema_index::{Db, DocumentStatus, QueryRule, SCHEMA_VERSION};

mod support;
use support::{space_1024, temp_db, unit_vector_1024};

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

#[test]
fn setting_the_same_folder_again_flips_its_entry() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    for path in ["notes", ""] {
        db.set_folder_searched(root, path, false).unwrap();
        db.set_folder_searched(root, path, true).unwrap();
        assert_eq!(
            db.search_folders(root).unwrap(),
            vec![(path.to_string(), true)]
        );
        db.set_folder_searched(root, path, false).unwrap();
    }
}

// ---- the filter inside both search arms (spec §5) ----

/// One indexed document with one chunk holding `text` at `rel` under `root`;
/// returns the chunk id. `n` makes the document id unique.
fn seed(db: &Db, root: i64, n: u32, rel: &str, text: &str) -> i64 {
    let id = format!("{n:064}");
    db.insert_document(&id, "text/plain", 1, SourceKind::Document)
        .unwrap();
    add_path(db, root, &id, rel);
    let page = db.insert_page(&id, 1, "native:txt", None).unwrap();
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
    let chunk = db
        .insert_chunk(
            &id,
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
    db.index_chunk_text(chunk, text).unwrap();
    db.set_document_status(&id, DocumentStatus::Indexed)
        .unwrap();
    chunk
}

fn add_path(db: &Db, root: i64, id: &str, rel: &str) {
    db.insert_path(
        root,
        rel,
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

/// A db with `files` seeded (all containing `zebra`, all with the same
/// vector), and the chunk ids in the same order.
fn index(files: &[&str]) -> (support::TempDb, i64, i64, Vec<i64>) {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    let space = space_1024(&db);
    let chunks: Vec<i64> = files
        .iter()
        .enumerate()
        .map(|(i, rel)| {
            let c = seed(&db, root, i as u32 + 1, rel, "zebra");
            db.insert_vector(space, c, &unit_vector_1024()).unwrap();
            c
        })
        .collect();
    (db, root, space, chunks)
}

/// What each arm finds, as indices into the seeded list, ascending.
fn found(db: &Db, space: i64, chunks: &[i64]) -> [(&'static str, Vec<usize>); 3] {
    let at = |ids: Vec<i64>| -> Vec<usize> {
        let mut v: Vec<usize> = ids
            .iter()
            .map(|id| chunks.iter().position(|c| c == id).unwrap())
            .collect();
        v.sort();
        v
    };
    [
        (
            "lexical",
            at(db
                .search_lexical_with("zebra", QueryRule::AllTerms, 50)
                .unwrap()),
        ),
        (
            "semantic",
            at(db
                .knn_searchable(space, &unit_vector_1024(), 50, None)
                .unwrap()
                .chunks),
        ),
        (
            "semantic restricted",
            at(db
                .knn_searchable(space, &unit_vector_1024(), 50, Some(chunks))
                .unwrap()
                .chunks),
        ),
    ]
}

fn assert_found(db: &Db, space: i64, chunks: &[i64], want: &[usize]) {
    for (arm, got) in found(db, space, chunks) {
        assert_eq!(got, want, "{arm} arm");
    }
}

#[test]
fn an_unsearched_folder_hides_its_files() {
    let (db, root, space, c) = index(&["notes/a.md", "archive/b.md"]);
    assert_found(&db, space, &c, &[0, 1]);
    db.set_folder_searched(root, "notes", false).unwrap();
    assert_found(&db, space, &c, &[1]);
}

#[test]
fn the_nearest_entry_wins() {
    let (db, root, space, c) = index(&["notes/drafts/x.md", "top.md", "notes/y.md"]);
    db.set_folder_searched(root, "", false).unwrap();
    db.set_folder_searched(root, "notes/drafts", true).unwrap();
    assert_found(&db, space, &c, &[0]);
}

#[test]
fn a_folder_is_not_a_prefix_of_its_sibling() {
    let (db, root, space, c) = index(&["notes/a.md", "note/b.md"]);
    db.set_folder_searched(root, "note", false).unwrap();
    assert_found(&db, space, &c, &[0]);
}

#[test]
fn pattern_characters_are_literal() {
    let (db, root, space, c) = index(&["axb/c.md", "a_b/c.md", "100%_done/d.md", "100x_done/d.md"]);
    db.set_folder_searched(root, "a_b", false).unwrap();
    db.set_folder_searched(root, "100%_done", false).unwrap();
    assert_found(&db, space, &c, &[0, 3]);
}

#[test]
fn cyrillic_folders_filter_the_same() {
    let (db, root, space, c) = index(&["Договори/2024/акт.md", "Листи/л.md"]);
    db.set_folder_searched(root, "Договори", false).unwrap();
    assert_found(&db, space, &c, &[1]);
}

#[test]
fn a_root_level_file_follows_the_root_entry() {
    let (db, root, space, c) = index(&["README.md", "docs/a.md"]);
    db.set_folder_searched(root, "", false).unwrap();
    assert_found(&db, space, &c, &[]);
    db.set_folder_searched(root, "docs", true).unwrap();
    assert_found(&db, space, &c, &[1]);
}

#[test]
fn one_searched_copy_is_enough() {
    let (db, root, space, c) = index(&["on/a.md", "off/a.md"]);
    // The second document also lives at `on/copy.md`.
    add_path(&db, root, &format!("{:064}", 2), "on/copy.md");
    db.set_folder_searched(root, "off", false).unwrap();
    assert_found(&db, space, &c, &[0, 1]);
    db.set_folder_searched(root, "on", false).unwrap();
    assert_found(&db, space, &c, &[]);
}

#[test]
fn terms_present_ignores_unsearched_folders() {
    let (db, root, _space, _c) = index(&["off/a.md"]);
    seed(&db, root, 9, "on/b.md", "giraffe");
    db.set_folder_searched(root, "off", false).unwrap();
    assert_eq!(db.terms_present("zebra giraffe").unwrap(), vec!["giraffe"]);
}

#[test]
fn the_embedding_queue_and_coverage_ignore_entries() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    let space = space_1024(&db);
    seed(&db, root, 1, "off/a.md", "zebra");
    seed(&db, root, 2, "on/b.md", "zebra");
    let before = (
        db.chunks_needing_embedding(space, 10).unwrap().len(),
        db.eligible_chunk_count().unwrap(),
    );
    assert_eq!(before, (2, 2));
    db.set_folder_searched(root, "off", false).unwrap();
    let after = (
        db.chunks_needing_embedding(space, 10).unwrap().len(),
        db.eligible_chunk_count().unwrap(),
    );
    assert_eq!(after, before);
}

/// Measurement, not a check: `cargo test -p mnema-index --release --test scope
/// scope_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn scope_cost() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    for i in 0..20_000u32 {
        seed(
            &db,
            root,
            i + 1,
            &format!("dir{}/sub{}/f{i}.md", i % 100, i % 7),
            "zebra",
        );
    }
    let time = |label: &str| {
        let t = std::time::Instant::now();
        for _ in 0..10 {
            db.search_lexical_with("zebra", QueryRule::AllTerms, 50)
                .unwrap();
        }
        println!("{label}: {:?} per search", t.elapsed() / 10);
    };
    time("empty search_folder");
    for i in 0..50 {
        db.set_folder_searched(root, &format!("dir{i}/sub{}", i % 7), i % 2 == 0)
            .unwrap();
    }
    time("50 entries");
}

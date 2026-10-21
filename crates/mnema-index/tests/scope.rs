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
    let wrong: Vec<String> = found(db, space, chunks)
        .into_iter()
        .filter(|(_, got)| got != want)
        .map(|(arm, got)| format!("{arm} arm: got {got:?}"))
        .collect();
    assert!(wrong.is_empty(), "want {want:?}; {wrong:?}");
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
fn the_same_folder_name_in_another_root_is_not_covered() {
    let (db, root_a, space, mut c) = index(&["notes/a.md"]);
    let root_b = db.insert_watched_root("/other").unwrap();
    let b = seed(&db, root_b, 2, "notes/b.md", "zebra");
    db.insert_vector(space, b, &unit_vector_1024()).unwrap();
    c.push(b);
    db.set_folder_searched(root_a, "notes", false).unwrap();
    assert_found(&db, space, &c, &[1]);
}

#[test]
fn a_document_without_a_path_is_never_hidden() {
    let (db, root, space, mut c) = index(&["notes/a.md"]);
    let id = format!("{:064}", 7);
    let orphan = seed(&db, root, 7, "tmp/x.md", "zebra");
    db.conn()
        .execute("DELETE FROM path WHERE document_id = ?1", [&id])
        .unwrap();
    db.insert_vector(space, orphan, &unit_vector_1024())
        .unwrap();
    c.push(orphan);
    assert_found(&db, space, &c, &[0, 1]);
    db.set_folder_searched(root, "elsewhere", false).unwrap();
    assert_found(&db, space, &c, &[0, 1]);
    db.set_folder_searched(root, "notes", false).unwrap();
    assert_found(&db, space, &c, &[1]);
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
    let off = seed(&db, root, 1, "off/a.md", "zebra");
    seed(&db, root, 2, "on/b.md", "zebra");
    db.insert_vector(space, off, &unit_vector_1024()).unwrap();
    let before = (
        db.chunks_needing_embedding(space, 10).unwrap().len(),
        db.eligible_chunk_count().unwrap(),
        db.eligible_embedded_chunk_count(space).unwrap(),
    );
    assert_eq!(before, (1, 2, 1));
    db.set_folder_searched(root, "off", false).unwrap();
    let after = (
        db.chunks_needing_embedding(space, 10).unwrap().len(),
        db.eligible_chunk_count().unwrap(),
        db.eligible_embedded_chunk_count(space).unwrap(),
    );
    assert_eq!(after, before);
}

/// Measurement, not a check: `cargo test -p mnema-index --release --test scope
/// scope_cost -- --ignored --nocapture`. Prints the median of 5 runs, in ms,
/// for lexical (all 20k chunks match / ~100 match) and semantic arms at 0, 1, 5
/// and 50 entries, then the same with a range-shaped per-path condition (an
/// experiment: raw SQL here, not product code).
#[test]
#[ignore]
fn scope_cost() {
    const K: i64 = 30; // mnema_search::CANDIDATES
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    let space = space_1024(&db);
    for i in 0..20_000u32 {
        let text = if i % 200 == 0 {
            "zebra giraffe"
        } else {
            "zebra"
        };
        let c = seed(
            &db,
            root,
            i + 1,
            &format!("dir{}/sub{}/f{i}.md", i % 100, i % 7),
            text,
        );
        db.insert_vector(space, c, &unit_vector_1024()).unwrap();
    }
    let median = |f: &dyn Fn()| -> f64 {
        let mut t: Vec<f64> = (0..5)
            .map(|_| {
                let s = std::time::Instant::now();
                f();
                s.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        t.sort_by(|a, b| a.partial_cmp(b).unwrap());
        t[2]
    };
    let table: String = db
        .conn()
        .query_row("SELECT 'vec_emb_' || ?1", [space], |r| r.get(0))
        .unwrap();
    let blob: Vec<u8> = unit_vector_1024()
        .iter()
        .flat_map(|f| f.to_ne_bytes())
        .collect();
    let range = "COALESCE((SELECT f.searched FROM search_folder f
                 WHERE f.watched_root_id = pa.watched_root_id
                   AND (f.folder_path = '' OR (pa.relative_path > f.folder_path || '/'
                        AND pa.relative_path < f.folder_path || '0'))
                 ORDER BY length(f.folder_path) DESC LIMIT 1), 1) = 1";
    let gate = format!(
        "AND (NOT EXISTS (SELECT 1 FROM search_folder) OR EXISTS (SELECT 1 FROM path pa
          WHERE pa.document_id = d.id AND {range}))"
    );
    let lex_range = |q: &str| {
        let sql = format!(
            "SELECT chunk_fts.rowid FROM chunk_fts JOIN chunk ON chunk.id = chunk_fts.rowid
             JOIN document d ON d.id = chunk.document_id
             WHERE chunk_fts MATCH ?1 AND d.status = 'indexed' {gate}
             ORDER BY rank, chunk_fts.rowid LIMIT ?2"
        );
        let mut st = db.conn().prepare(&sql).unwrap();
        let n = st
            .query_map(rusqlite::params![q, K], |r| r.get::<_, i64>(0))
            .unwrap()
            .count();
        std::hint::black_box(n);
    };
    let sem_range = || {
        let sql = format!(
            "SELECT chunk_id, distance FROM {table} WHERE embedding MATCH ?1 AND k = ?2
             AND chunk_id IN (SELECT c.id FROM chunk c JOIN document d ON d.id = c.document_id
                              WHERE d.status = 'indexed' {gate})
             ORDER BY distance"
        );
        let mut st = db.conn().prepare(&sql).unwrap();
        let n = st
            .query_map(rusqlite::params![blob, K * 4], |r| r.get::<_, i64>(0))
            .unwrap()
            .count();
        std::hint::black_box(n);
    };
    let mut set = 0;
    println!(
        "entries | lex-all | lex-100 | sem | R:lex-all | R:lex-100 | R:sem   (ms, median of 5)"
    );
    for entries in [0, 1, 5, 50] {
        while set < entries {
            db.set_folder_searched(root, &format!("dir{set}/sub{}", set % 7), false)
                .unwrap();
            set += 1;
        }
        let lex = |q: &'static str| {
            let db = &db;
            move || {
                db.search_lexical_with(q, QueryRule::AllTerms, K).unwrap();
            }
        };
        println!(
            "{entries:>7} | {:>7.1} | {:>7.1} | {:>5.1} | {:>9.1} | {:>9.1} | {:>6.1}",
            median(&lex("zebra")),
            median(&lex("giraffe")),
            median(&|| {
                db.knn_searchable(space, &unit_vector_1024(), K, None)
                    .unwrap();
            }),
            median(&|| lex_range("zebra")),
            median(&|| lex_range("giraffe")),
            median(&sem_range),
        );
    }
}

// ---- citation and "is anything searched" (spec §6, §7) ----

#[test]
fn a_citation_prefers_a_searched_path() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    let chunk = seed(&db, root, 1, "a/x.md", "zebra");
    add_path(&db, root, &format!("{:064}", 1), "b/x.md");
    let cited = |db: &Db| db.citation(chunk).unwrap().unwrap().relative_path;
    assert_eq!(cited(&db).as_deref(), Some("a/x.md"));
    db.set_folder_searched(root, "a", false).unwrap();
    assert_eq!(cited(&db).as_deref(), Some("b/x.md"));
}

#[test]
fn nothing_searchable_when_every_root_is_off() {
    let db = temp_db();
    assert!(!db.any_document_searchable().unwrap());
    let r1 = db.insert_watched_root("/r1").unwrap();
    let r2 = db.insert_watched_root("/r2").unwrap();
    seed(&db, r1, 1, "a.md", "zebra");
    seed(&db, r2, 2, "b.md", "zebra");
    assert!(db.any_document_searchable().unwrap());
    db.set_folder_searched(r1, "", false).unwrap();
    assert!(db.any_document_searchable().unwrap());
    db.set_folder_searched(r2, "", false).unwrap();
    assert!(!db.any_document_searchable().unwrap());
    db.set_folder_searched(r2, "", true).unwrap();
    assert!(db.any_document_searchable().unwrap());
}

#[test]
fn a_pathless_indexed_document_counts_as_searchable() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    seed(&db, root, 1, "a.md", "zebra");
    db.conn().execute("DELETE FROM path", []).unwrap();
    db.set_folder_searched(root, "", false).unwrap();
    assert!(db.any_document_searchable().unwrap());
}

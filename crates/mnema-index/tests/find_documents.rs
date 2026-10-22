//! `Db::find_documents`: the file tree's word search. Prefix terms, every
//! indexed document whatever the search folders say, one best chunk each.

use mnema_core::{Block, BlockType, Coordinate, Locator, OnDisk, Segment, SourceKind};
use mnema_index::{Db, DocumentStatus, prepare_for_search};

mod support;
use support::temp_db;

/// One document at `rel` under `root`, one chunk per entry of `texts`, indexed
/// the way the pipeline does it: the original in `chunk.text`, the prepared
/// copy in the lexical index. Returns the chunk ids. `n` makes the id unique.
fn seed(db: &Db, root: i64, n: u32, rel: &str, texts: &[&str]) -> Vec<i64> {
    let id = format!("{n:064}");
    db.insert_document(&id, "text/plain", 1, SourceKind::Document)
        .unwrap();
    db.insert_path(
        root,
        rel,
        &id,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();
    let page = db.insert_page(&id, 1, "native:txt", None).unwrap();
    let chunks = texts
        .iter()
        .enumerate()
        .map(|(ord, text)| {
            let block = db
                .insert_block(
                    page,
                    &Block {
                        block_type: BlockType::Paragraph,
                        reading_order: ord as i64,
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
                    ord as i64,
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
            db.index_chunk_text(chunk, &prepare_for_search(text, SourceKind::Document))
                .unwrap();
            chunk
        })
        .collect();
    db.set_document_status(&id, DocumentStatus::Indexed)
        .unwrap();
    chunks
}

fn ids(db: &Db, query: &str) -> Vec<String> {
    db.find_documents(query, 500)
        .unwrap()
        .expect("the query has terms")
        .documents
        .into_iter()
        .map(|d| d.document_id)
        .collect()
}

fn doc(n: u32) -> String {
    format!("{n:064}")
}

#[test]
fn a_prefix_finds_the_unfinished_word() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    seed(&db, root, 1, "a.md", &["акт звірки взаєморозрахунків"]);
    assert_eq!(ids(&db, "взаємо"), vec![doc(1)]);
}

#[test]
fn every_term_must_be_present() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    seed(&db, root, 1, "a.md", &["акт звірки"]);
    seed(&db, root, 2, "b.md", &["акт приймання"]);
    assert_eq!(ids(&db, "акт зві"), vec![doc(1)]);
}

#[test]
fn an_unsearched_folder_is_still_found() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    seed(&db, root, 1, "archive/b.md", &["zebra"]);
    db.set_folder_searched(root, "archive", false).unwrap();
    assert_eq!(ids(&db, "zebra"), vec![doc(1)]);
}

#[test]
fn a_document_not_indexed_is_not_found() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    seed(&db, root, 1, "a.md", &["zebra"]);
    db.set_document_status(&doc(1), DocumentStatus::Pending)
        .unwrap();
    assert!(ids(&db, "zebra").is_empty());
}

#[test]
fn two_chunks_of_one_document_give_one_document_with_the_better_chunk() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    let chunks = seed(&db, root, 1, "a.md", &["zebra", "zebra zebra zebra"]);
    let found = db.find_documents("zebra", 500).unwrap().unwrap();
    assert_eq!(found.documents.len(), 1);
    assert_eq!(found.documents[0].chunk_id, chunks[1]);
    assert_eq!(found.documents[0].ord, 1);
}

#[test]
fn a_query_without_terms_is_none() {
    let db = temp_db();
    for q in ["", "   ", "—,.!"] {
        assert!(db.find_documents(q, 500).unwrap().is_none(), "{q:?}");
    }
}

#[test]
fn fts_syntax_in_the_query_is_data() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    seed(&db, root, 1, "a.md", &["zebra"]);
    for q in ["\"zeb", "-zeb", "(zeb"] {
        assert_eq!(ids(&db, q), vec![doc(1)], "{q:?}");
    }
    // `OR` is one more word that must be present, not an operator.
    assert!(ids(&db, "zeb OR x").is_empty());
}

#[test]
fn the_limit_truncates() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    for n in 1..=3 {
        seed(&db, root, n, &format!("f{n}.md"), &["zebra"]);
    }
    let two = db.find_documents("zebra", 2).unwrap().unwrap();
    assert_eq!((two.documents.len(), two.truncated), (2, true));
    let three = db.find_documents("zebra", 3).unwrap().unwrap();
    assert_eq!((three.documents.len(), three.truncated), (3, false));
}

#[test]
fn an_apostrophe_and_diacritics_fold_in_the_prefix() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    seed(&db, root, 1, "a.md", &["пам’ять Café"]);
    assert_eq!(ids(&db, "пам'я"), vec![doc(1)]);
    assert_eq!(ids(&db, "cafe"), vec![doc(1)]);
}

#[test]
fn the_text_is_the_original_not_the_prepared_copy() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    seed(&db, root, 1, "a.md", &["Zebra Café"]);
    let found = db.find_documents("zebra", 500).unwrap().unwrap();
    assert_eq!(found.documents[0].text, "Zebra Café");
}

#[test]
fn documents_come_in_relevance_order_not_id_order() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    // The worse match is seeded first, so it has the lower chunk id.
    seed(&db, root, 1, "worse.md", &["zebra"]);
    seed(&db, root, 2, "better.md", &["zebra zebra zebra"]);
    assert_eq!(ids(&db, "zebra"), vec![doc(2), doc(1)]);
}

#[test]
fn equal_matches_come_in_chunk_id_order() {
    let db = temp_db();
    let root = db.insert_watched_root("/r").unwrap();
    seed(&db, root, 1, "a.md", &["zebra"]);
    seed(&db, root, 2, "b.md", &["zebra"]);
    assert_eq!(ids(&db, "zebra"), vec![doc(1), doc(2)]);
}

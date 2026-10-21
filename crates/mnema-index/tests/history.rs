//! `query_history` keeps one row per distinct question, newest first, and
//! holds at most fifty of them. The answers are snapshots, so removing the
//! documents an answer was built from must leave the history as it was.

use mnema_core::{Block, BlockType, Coordinate, Locator, OnDisk, Segment, SourceKind};
use mnema_index::{Db, DocumentStatus};
use rusqlite::params;

mod support;
use support::temp_db;

#[test]
fn recording_then_listing_returns_newest_first() {
    let db = temp_db();
    db.history_record("q1", "{}").unwrap();
    db.history_record("q2", "{}").unwrap();

    let queries: Vec<String> = db
        .history_list()
        .unwrap()
        .into_iter()
        .map(|r| r.query)
        .collect();
    assert_eq!(queries, ["q2", "q1"]);
}

/// A repeated question is one row, moved to the top, carrying the newest
/// answer. Both records land in the same second here, which is the case that
/// matters: an `ON CONFLICT DO UPDATE` keeps the first row's `id`, and with
/// `id DESC` as the tie-break `q1` would sort below `q2` and stay there.
#[test]
fn the_same_query_replaces_and_moves_to_top() {
    let db = temp_db();
    db.history_record("q1", r#""a""#).unwrap();
    db.history_record("q2", r#""b""#).unwrap();
    db.history_record(" q1 ", r#""c""#).unwrap();

    let list = db.history_list().unwrap();
    let queries: Vec<&str> = list.iter().map(|r| r.query.as_str()).collect();
    assert_eq!(
        queries,
        ["q1", "q2"],
        "one row per query, the repeat on top"
    );
    assert_eq!(
        list[0].answer, r#""c""#,
        "the list carries the stored answer JSON, which the list draws a preview from"
    );

    assert_eq!(
        db.history_answer(list[0].id).unwrap(),
        Some(("q1".to_string(), r#""c""#.to_string())),
        "the stored query is trimmed and the answer is the newest one"
    );
}

/// Fifty-one records inside one second: the oldest by `(asked_at, id)` goes,
/// and the newest stays on top. Without the `id` tie-break the trim is free to
/// remove whichever of the same-second rows it reaches first.
#[test]
fn the_fifty_first_record_drops_the_oldest_even_within_one_second() {
    let db = temp_db();
    for i in 0..51 {
        db.history_record(&format!("q{i}"), "{}").unwrap();
    }

    let list = db.history_list().unwrap();
    assert_eq!(list.len(), 50);
    assert_eq!(
        list[0].query, "q50",
        "the newest record must not be trimmed"
    );
    assert!(
        list.iter().all(|r| r.query != "q0"),
        "the oldest record must be the one dropped"
    );
}

/// The clock stepped back while the table was full. Fifty rows are stamped in
/// the future, so a plain `unixepoch()` stamp for the new row would sort it
/// below all of them, and the trim in the same transaction would delete the
/// answer that was just given. The new row must take a stamp no older than the
/// newest existing one.
#[test]
fn a_record_written_after_the_clock_stepped_back_is_kept_and_first() {
    let db = temp_db();
    for i in 0..50 {
        db.conn()
            .execute(
                "INSERT INTO query_history (query, answer, asked_at)
                 VALUES (?1, '{}', unixepoch() + 100000)",
                params![format!("old{i}")],
            )
            .unwrap();
    }

    db.history_record("new", r#""fresh""#).unwrap();

    let list = db.history_list().unwrap();
    assert_eq!(list.len(), 50);
    assert_eq!(
        list[0].query, "new",
        "the record just written must be first"
    );
}

#[test]
fn delete_removes_one_and_reports_whether_it_existed() {
    let db = temp_db();
    db.history_record("q1", r#""a""#).unwrap();
    let id = db.history_list().unwrap()[0].id;

    assert!(db.history_delete(id).unwrap(), "the row was there");
    assert!(!db.history_delete(id).unwrap(), "the row is already gone");
    assert_eq!(db.history_answer(id).unwrap(), None);
    assert!(db.history_list().unwrap().is_empty());
}

/// Removing a root takes the documents its paths named, and the history keeps
/// every row and every answer: the answers are snapshots, and nothing in the
/// table references a document.
/// An id the window may still hold must never name a different question.
/// A plain `INTEGER PRIMARY KEY` hands out `max(id) + 1`, so deleting the
/// newest row and recording another question would give the new one the
/// deleted id — and a stale row's remove button would then delete it.
#[test]
fn a_deleted_newest_id_is_never_given_to_another_question() {
    let db = temp_db();
    db.history_record("q1", "{}").unwrap();
    db.history_record("q2", "{}").unwrap();
    let q2 = db.history_list().unwrap()[0].id;
    assert!(db.history_delete(q2).unwrap());

    db.history_record("q3", "{}").unwrap();
    let q3 = db.history_list().unwrap()[0].id;
    assert_ne!(q3, q2, "q3 reused the id of the deleted q2");

    // The stale delete finds nothing and leaves q3 in place.
    assert!(!db.history_delete(q2).unwrap());
    assert_eq!(
        db.history_answer(q3).unwrap().map(|(q, _)| q).as_deref(),
        Some("q3")
    );
}

#[test]
fn history_survives_removing_every_document() {
    let db = temp_db();
    let root = db.insert_watched_root("/tmp/one").unwrap();
    let doc = index_document_under(&db, root, "a.txt", "unique marker text");

    db.history_record("marker", r#"{"answer":"kept"}"#).unwrap();
    let before = db.history_list().unwrap();

    db.delete_watched_root(root).unwrap();

    assert!(!db.document_exists(&doc).unwrap(), "the document is gone");
    let after = db.history_list().unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, before[0].id);
    assert_eq!(after[0].query, "marker");
    assert_eq!(
        db.history_answer(after[0].id).unwrap(),
        Some(("marker".to_string(), r#"{"answer":"kept"}"#.to_string()))
    );
}

/// One document reachable only through `root`, with a page, block and chunk,
/// built the way `root_removal.rs` builds its own.
fn index_document_under(db: &Db, root: i64, relative_path: &str, text: &str) -> String {
    let doc = "b".repeat(64);
    db.insert_document(&doc, "text/plain", text.len() as i64, SourceKind::Document)
        .unwrap();
    db.insert_path(
        root,
        relative_path,
        &doc,
        OnDisk {
            size_bytes: text.len() as i64,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
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
    db.insert_chunk(
        &doc,
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
    db.set_document_status(&doc, DocumentStatus::Indexed)
        .unwrap();
    doc
}

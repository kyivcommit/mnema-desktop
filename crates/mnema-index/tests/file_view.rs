//! `Db::document_view`: the blocks of one document, whole.
//!
//! The caller is the window's right pane, which paints the file rather than a
//! window around a citation. What it must never get is a shortened list that
//! looks complete — so the limit is reported as a flag, and the command above
//! turns it into a refusal.

use mnema_core::{Block, BlockType, OnDisk, SourceKind};

mod support;

/// One paragraph block — the same shape `tests/citation.rs` builds, because
/// `insert_block` takes the whole `Block` and a test that only cares about the
/// text still has to say what it means by the rest.
fn paragraph(order: i64, text: &str) -> Block {
    Block {
        block_type: BlockType::Paragraph,
        reading_order: order,
        language: None,
        text: text.to_string(),
        line_start: None,
        line_end: None,
    }
}

fn disk() -> OnDisk {
    OnDisk {
        size_bytes: 1,
        mtime: 1,
    }
}

#[test]
fn blocks_come_back_in_page_then_reading_order() {
    let db = support::temp_db();
    let doc = db
        .insert_document(&"a".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    // Written page 2 first, and out of reading order within page 1: insertion
    // order and rowid order both disagree with the answer the caller needs.
    let page_two = db
        .insert_page(&doc, 2, "native:txt", Some("Розділ 2"))
        .unwrap();
    let second_page = db
        .insert_block(page_two, &paragraph(0, "друга сторінка"))
        .unwrap();
    let page_one = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    let second = db.insert_block(page_one, &paragraph(1, "другий")).unwrap();
    let first = db.insert_block(page_one, &paragraph(0, "перший")).unwrap();

    let view = db
        .document_view(&doc, 10)
        .unwrap()
        .expect("the document exists");

    let ids: Vec<i64> = view.blocks.iter().map(|b| b.block_id).collect();
    assert_eq!(
        ids,
        vec![first, second, second_page],
        "blocks must come back in page then reading order, not in insertion order"
    );
    assert_eq!(view.blocks[0].page_no, 1);
    assert_eq!(view.blocks[2].page_no, 2);
    assert_eq!(
        view.blocks[2].section_title.as_deref(),
        Some("Розділ 2"),
        "the page's section title has to reach the pane"
    );
    assert!(!view.truncated);
}

#[test]
fn a_document_without_blocks_is_some_but_empty() {
    let db = support::temp_db();
    let doc = db
        .insert_document(&"b".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();

    let view = db
        .document_view(&doc, 10)
        .unwrap()
        .expect("an existing document is not `None`, whatever it holds");

    assert!(view.blocks.is_empty());
    assert!(!view.truncated);
}

#[test]
fn an_unknown_document_is_none() {
    let db = support::temp_db();
    assert!(
        db.document_view(&"c".repeat(64), 10).unwrap().is_none(),
        "a document that does not exist is `None`, not an empty view — the pane says \
         'no such document' for one and 'nothing to show' for the other"
    );
}

#[test]
fn over_the_limit_reports_truncated() {
    let db = support::temp_db();
    let doc = db
        .insert_document(&"d".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    for order in 0..3 {
        db.insert_block(page, &paragraph(order, "текст")).unwrap();
    }

    let over = db.document_view(&doc, 2).unwrap().unwrap();
    assert!(
        over.truncated,
        "three blocks at a limit of two is more than the caller asked for"
    );
    assert_eq!(
        over.blocks.len(),
        2,
        "the extra row exists only to say there is more, and must not be returned"
    );

    // The other half of what the extra row buys: exactly the limit is not
    // truncated. Without this, `truncated` could be `len == limit` and every
    // full document would refuse.
    let exact = db.document_view(&doc, 3).unwrap().unwrap();
    assert!(!exact.truncated, "exactly the limit is a complete answer");
    assert_eq!(exact.blocks.len(), 3);
}

#[test]
fn kind_is_the_block_type_verbatim() {
    let db = support::temp_db();
    let doc = db
        .insert_document(&"e".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    db.insert_block(
        page,
        &Block {
            block_type: BlockType::Headline,
            ..paragraph(0, "## Заголовок")
        },
    )
    .unwrap();

    let view = db.document_view(&doc, 10).unwrap().unwrap();
    assert_eq!(
        view.blocks[0].kind, "headline",
        "the pane decides what to draw from `block.type`, so the backend must not map it"
    );
    assert_eq!(
        view.blocks[0].text, "## Заголовок",
        "the marker stays in the data; only the display strips it"
    );
}

/// The fallback a caller naming only `documentId` gets: one path is that path,
/// several are no location at all. Guessing one of several reports freshness
/// about a file the person may not be looking at.
#[test]
fn a_documents_only_path_is_named_and_several_are_not() {
    let db = support::temp_db();
    let root = db.insert_watched_root("/roots/a").unwrap();
    let doc = db
        .insert_document(&"f".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();

    assert_eq!(
        db.document_path(&doc).unwrap(),
        None,
        "no path rows is no location, not a location with an empty name"
    );

    db.insert_path(root, "notes/a.txt", &doc, disk(), "text", 1)
        .unwrap();
    assert_eq!(
        db.document_path(&doc).unwrap(),
        Some((root, "notes/a.txt".to_string()))
    );

    db.insert_path(root, "notes/b.txt", &doc, disk(), "text", 1)
        .unwrap();
    assert_eq!(
        db.document_path(&doc).unwrap(),
        None,
        "two copies are not one honest location"
    );
}

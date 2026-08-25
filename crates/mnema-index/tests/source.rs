use mnema_core::{Block, BlockType, Coordinate, Locator, OnDisk, Segment, SourceKind};
use mnema_index::{Db, open, register_vector_extension};

fn fresh(dir: &tempfile::TempDir) -> Db {
    register_vector_extension().unwrap();
    open(&dir.path().join("index.sqlite")).unwrap()
}

fn block(reading_order: i64, text: &str) -> Block {
    Block {
        block_type: BlockType::Paragraph,
        reading_order,
        language: None,
        text: text.to_string(),
        line_start: None,
        line_end: None,
    }
}

// ---------------------------------------------------------------- chunk_anchor

#[test]
fn chunk_anchor_reports_the_page_and_reading_order_range_of_the_chunks_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let doc = db
        .insert_document(&"a".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    let page = db
        .insert_page(&doc, 1, "native:txt", Some("Section One"))
        .unwrap();
    let b1 = db.insert_block(page, &block(1, "перший абзац")).unwrap();
    let b2 = db.insert_block(page, &block(2, "другий абзац")).unwrap();
    let b3 = db.insert_block(page, &block(3, "третій абзац")).unwrap();
    let _ = b1;

    let text2 = "другий абзац";
    let text3 = "третій абзац";
    let n2 = text2.chars().count() as u32;
    let n3 = text3.chars().count() as u32;
    let full_text = format!("{text2}{text3}");
    let locator = Locator {
        spans: vec![
            Segment {
                block_id: b2,
                start: 0,
                end: n2,
                block_start: 0,
            },
            Segment {
                block_id: b3,
                start: n2,
                end: n2 + n3,
                block_start: 0,
            },
        ],
        coordinate: Coordinate::Page { number: 1 },
    };
    let chunk_id = db
        .insert_chunk(&doc, 0, &full_text, &locator, SourceKind::Document)
        .unwrap();

    let anchor = db
        .chunk_anchor(chunk_id)
        .unwrap()
        .expect("the chunk we just inserted");
    assert_eq!(anchor.document_id, doc);
    assert_eq!(anchor.text, full_text);
    assert_eq!(anchor.spans, locator.spans);
    assert_eq!(anchor.section_title.as_deref(), Some("Section One"));
    assert_eq!(anchor.page_no, 1);
    assert_eq!(anchor.first_reading_order, 2);
    assert_eq!(anchor.last_reading_order, 3);

    // Both directions: an id no chunk carries answers None, not the anchor
    // above under a different guise.
    assert!(db.chunk_anchor(chunk_id + 1000).unwrap().is_none());
}

// -------------------------------------------------------- path_occupant, roots

#[test]
fn path_occupant_reports_the_row_as_it_stands() {
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

    let occupant = db
        .path_occupant(root, "a.txt")
        .unwrap()
        .expect("the row we just inserted");
    assert_eq!(occupant.watched_root_id, root);
    assert_eq!(occupant.root_absolute_path, "/tmp/root");
    assert_eq!(occupant.relative_path, "a.txt");
    assert_eq!(occupant.size_bytes, 10);
    assert_eq!(occupant.mtime, 1234);
    assert_eq!(occupant.current_document_id, doc);
}

#[test]
fn path_occupant_is_none_where_no_row_is() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root = db.insert_watched_root("/tmp/root").unwrap();

    // Both directions: a method that always answered `None` would pass the
    // case above's negation too, so this must be checked against a database
    // that actually holds *some* path — just not this one.
    let doc = db
        .insert_document(&"a".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    db.insert_path(
        root,
        "used.txt",
        &doc,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();

    assert!(db.path_occupant(root, "unused.txt").unwrap().is_none());
}

#[test]
fn path_occupant_reports_the_new_document_after_a_repoint() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let root = db.insert_watched_root("/tmp/root").unwrap();
    let original = db
        .insert_document(&"a".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    db.insert_path(
        root,
        "edited.txt",
        &original,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();

    // What `repoint` does: a second content hash, then delete + re-insert the
    // SAME (root, relative_path) onto it.
    let edited = db
        .insert_document(&"b".repeat(64), "text/plain", 2, SourceKind::Document)
        .unwrap();
    db.delete_path(root, "edited.txt").unwrap();
    db.insert_path(
        root,
        "edited.txt",
        &edited,
        OnDisk {
            size_bytes: 2,
            mtime: 2,
        },
        "text",
        1,
    )
    .unwrap();

    let occupant = db
        .path_occupant(root, "edited.txt")
        .unwrap()
        .expect("the repointed row");
    assert_eq!(occupant.current_document_id, edited);
    assert_ne!(occupant.current_document_id, original);
}

#[test]
fn roots_holding_path_returns_every_root() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let root_a = db.insert_watched_root("/tmp/a").unwrap();
    let root_b = db.insert_watched_root("/tmp/b").unwrap();
    let doc = db
        .insert_document(&"a".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    db.insert_path(
        root_a,
        "shared.txt",
        &doc,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();
    db.insert_path(
        root_b,
        "shared.txt",
        &doc,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();

    let mut roots = db.roots_holding_path("shared.txt").unwrap();
    roots.sort();
    let mut expected = vec![root_a, root_b];
    expected.sort();
    assert_eq!(roots, expected);
}

// ----------------------------------------------------------------- reading_window

/// Two pages of four blocks each; the anchor is page 2's first block. A
/// radius of 2 must cross the page boundary and return exactly page 1's last
/// two blocks before it — not merely "at least one", which a window
/// returning a single block would also satisfy.
/// Three pages of three blocks, anchored in the middle, so that **both** sides
/// of the window cross a page boundary and both `has_more` flags are true.
///
/// The shape is load-bearing, and a smaller one hid two live mutants. With an
/// anchor whose before-side and after-side each stay inside one page,
/// `ORDER BY p.page_no DESC, b.reading_order DESC` and the bare
/// `ORDER BY b.reading_order DESC` return the same rows — `reading_order` is
/// unique only per page (`ix_block_page`, `schema.sql:140`), so it is not a
/// document ordinal, and the difference only shows when the candidates span
/// two pages and the values collide across them. Here `p1b3` and `p2b3` both
/// carry `reading_order = 3`, which is what makes the page term load-bearing.
///
/// Both flags are asserted **true** here; the two tests below assert them
/// false. Neither direction alone is coverage: a flag hardcoded either way
/// passes one of the pair.
#[test]
fn reading_window_returns_radius_blocks_each_side_in_document_reading_order() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let doc = db
        .insert_document(&"a".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();

    for page_no in 1..=3 {
        let page = db.insert_page(&doc, page_no, "native:txt", None).unwrap();
        for i in 1..=3 {
            db.insert_block(page, &block(i, &format!("p{page_no}b{i}")))
                .unwrap();
        }
    }

    // Anchor: page 2, reading_order 2. Radius 3 leaves exactly one block over
    // on each side (`p1b1` before, `p3b3` after), so both flags must be true.
    let window = db.reading_window(&doc, 2, 2, 2, 3).unwrap();

    let texts: Vec<&str> = window.blocks.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(
        texts,
        vec!["p1b2", "p1b3", "p2b1", "p2b2", "p2b3", "p3b1", "p3b2"]
    );
    assert!(window.has_more_before, "p1b1 is out of range before");
    assert!(window.has_more_after, "p3b3 is out of range after");

    // The page term is what puts p1b3 before p2b1 despite the larger
    // reading_order; pinned so dropping it from either ORDER BY goes red.
    let pages: Vec<i64> = window.blocks.iter().map(|b| b.page_no).collect();
    assert_eq!(pages, vec![1, 1, 2, 2, 2, 3, 3]);
}

/// Both directions of the flag: an anchor at the very start of the document,
/// with a radius larger than the whole document, must report no more on
/// either side — a window that always claimed `true` would pass the test
/// above alone.
#[test]
fn reading_window_reports_no_more_when_the_document_ends() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let doc = db
        .insert_document(&"a".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    for i in 1..=3 {
        db.insert_block(page, &block(i, &format!("b{i}"))).unwrap();
    }

    let window = db.reading_window(&doc, 1, 1, 1, 10).unwrap();

    assert!(!window.has_more_before);
    assert!(!window.has_more_after);
    let texts: Vec<&str> = window.blocks.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(texts, vec!["b1", "b2", "b3"]);
}

/// Pins the `LIMIT radius + 1` mechanism: a document where the before-side
/// has exactly one block more than the radius asks for must report
/// `has_more_before == true`, and this is red against a hardcoded flag in
/// either direction.
#[test]
fn reading_window_reports_more_before_when_exactly_one_block_is_out_of_range() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let doc = db
        .insert_document(&"a".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    // radius=2 before the anchor (block 4): blocks 1..3 exist before it, one
    // more than the radius can show (2, 3), so block 1 is the "one out of
    // range" that must flip has_more_before to true.
    for i in 1..=5 {
        db.insert_block(page, &block(i, &format!("b{i}"))).unwrap();
    }

    let window = db.reading_window(&doc, 1, 4, 4, 2).unwrap();

    assert!(window.has_more_before);
    // After side: block 5 is the only one after the anchor, well within the
    // radius of 2 — so has_more_after must be false, not merely "not
    // asserted".
    assert!(!window.has_more_after);
    let texts: Vec<&str> = window.blocks.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(texts, vec!["b2", "b3", "b4", "b5"]);
}

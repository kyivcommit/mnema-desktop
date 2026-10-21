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
fn chunk_anchor_reports_the_identity_and_the_blocks_a_chunk_covers() {
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
    // `ord = 4`, not 0: every other assertion on `ChunkAnchor.ord` in the
    // repository used to be `assert_eq!(anchor.ord, 0)`, on a fixture that
    // could not tell "read the column" apart from "return a hardcoded 0" —
    // Every IPC fixture that cites a passage sends `ord: 0` as well.
    // Non-zero here closes that gap at the layer that reads the column
    // directly.
    let chunk_id = db
        .insert_chunk(&doc, 4, &full_text, &locator, SourceKind::Document)
        .unwrap();

    let anchor = db
        .chunk_anchor(chunk_id)
        .unwrap()
        .expect("the chunk we just inserted");
    assert_eq!(anchor.document_id, doc);
    assert_eq!(anchor.ord, 4);
    assert_eq!(anchor.text, full_text);
    assert_eq!(anchor.spans, locator.spans);

    // Both directions: an id no chunk carries answers None, not the anchor
    // above under a different guise.
    assert!(db.chunk_anchor(chunk_id + 1000).unwrap().is_none());
}

// -------------------------------------------------------- path_occupant, roots

#[test]
fn path_occupant_reports_the_row_as_it_stands() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    // Inserted FIRST, so it holds the *lower* id: a root predicate widened
    // from `= ?1` to `<= ?1` then reaches this row instead, which is the
    // mutant nothing caught while every fixture had a single root. It holds
    // the same relative path deliberately — the predicate under test is the
    // root one, so the path must not be what distinguishes them.
    let other_root = db.insert_watched_root("/tmp/other").unwrap();
    let other_doc = db
        .insert_document(&"b".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    db.insert_path(
        other_root,
        "a.txt",
        &other_doc,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();

    let root = db.insert_watched_root("/tmp/root").unwrap();
    assert!(
        other_root < root,
        "the decoy root must sort before the real one"
    );
    let doc = db
        // 10 and 77 differ on purpose: a query reading `document.size_bytes`
        // instead of `path.size_bytes` would pass if the two matched.
        .insert_document(&"a".repeat(64), "text/plain", 10, SourceKind::Document)
        .unwrap();
    db.insert_path(
        root,
        "a.txt",
        &doc,
        OnDisk {
            size_bytes: 77,
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
    assert_eq!(occupant.size_bytes, 77);
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

    // Not sorted here on purpose: the method's own `ORDER BY watched_root_id`
    // is what makes the answer deterministic, and sorting both sides would
    // leave that clause unpinned.
    let roots = db.roots_holding_path("shared.txt").unwrap();
    assert_eq!(roots, vec![root_a, root_b]);
}

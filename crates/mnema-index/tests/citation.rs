use mnema_core::{Block, BlockType, Coordinate, Locator, OnDisk, Segment, SourceKind};
use mnema_index::{
    Citation, Db, DocumentStatus, INDEX_FORMAT_VERSION, open, register_vector_extension,
};
use rusqlite::{Transaction, TransactionBehavior};

mod support;

fn fresh(dir: &tempfile::TempDir) -> Db {
    register_vector_extension().unwrap();
    open(&dir.path().join("index.sqlite")).unwrap()
}

/// One paragraph block. `insert_block` takes the `Block` itself rather than its
/// fields spread out, which is what stops a field being silently dropped on the
/// way to the row — so a test that only cares about the text still has to say
/// what it means by the rest.
fn paragraph(order: i64, text: &str, line_start: Option<u32>, line_end: Option<u32>) -> Block {
    Block {
        block_type: BlockType::Paragraph,
        reading_order: order,
        language: None,
        text: text.to_string(),
        line_start,
        line_end,
    }
}

#[test]
fn a_citation_reads_from_all_four_levels() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let root = db.insert_watched_root("/Volumes/Archive").unwrap();
    let doc = db
        .insert_document(
            "a".repeat(64).as_str(),
            "application/pdf",
            1024,
            SourceKind::Document,
        )
        .unwrap();
    db.insert_path(
        root,
        "contracts/q3.pdf",
        &doc,
        OnDisk {
            size_bytes: 1024,
            mtime: 1_700_000_000,
        },
        "text",
        1,
    )
    .unwrap();
    let page = db
        .insert_page(&doc, 12, "native:pdf", Some("Розділ 3. Умови постачання"))
        .unwrap();
    let block = db
        .insert_block(
            page,
            &Block {
                language: Some("uk".into()),
                ..paragraph(0, "Сторона А зобов'язується передати товар.", None, None)
            },
        )
        .unwrap();
    let chunk = db
        .insert_chunk(
            &doc,
            0,
            "Сторона А зобов'язується передати товар.",
            &Locator {
                spans: vec![Segment {
                    block_id: block,
                    start: 0,
                    end: 40,
                    block_start: 0,
                }],
                coordinate: Coordinate::Page { number: 12 },
            },
            SourceKind::Document,
        )
        .unwrap();

    let c: Citation = db.citation(chunk).unwrap().expect("chunk exists");
    assert_eq!(c.text, "Сторона А зобов'язується передати товар.");
    assert_eq!(
        c.section_title.as_deref(),
        Some("Розділ 3. Умови постачання")
    );
    assert_eq!(c.coordinate.render(), "с. 12");
    assert_eq!(c.relative_path.as_deref(), Some("contracts/q3.pdf"));
}

/// A chunk whose `document_id` disagrees with its block's document would
/// produce a citation naming one file and quoting another — the same section
/// title, the wrong path. That insert-time failure is no longer reachable
/// through `insert_chunk`: `chunk_span_blocks_bi` (a `BEFORE INSERT` trigger
/// added for `char_span`'s sake) already refuses element 0 disagreeing with
/// `document_id`, and it fires before the composite foreign key on
/// `(block_id, document_id)` is even reached, so no insert path still produces
/// `FOREIGN KEY constraint failed`. That schema behaviour is asserted directly
/// in `tests/schema.rs`.
///
/// What the composite key still uniquely owns is the other direction: it
/// exists so a chunk cannot outlive the block it names, and `ON DELETE
/// CASCADE` is what makes that automatic rather than a cleanup step every
/// caller has to remember. Deleting a document should take its whole ladder —
/// page, block, chunk — down with it.
#[test]
fn deleting_a_document_cascades_its_chunks_away() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let doc = db
        .insert_document(
            "a".repeat(64).as_str(),
            "application/pdf",
            1,
            SourceKind::Document,
        )
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:pdf", None).unwrap();
    let block = db
        .insert_block(page, &paragraph(0, "текст документа", None, None))
        .unwrap();
    let chunk = db
        .insert_chunk(
            &doc,
            0,
            "текст документа",
            &Locator {
                spans: vec![Segment {
                    block_id: block,
                    start: 0,
                    end: "текст документа".chars().count() as u32,
                    block_start: 0,
                }],
                coordinate: Coordinate::Page { number: 1 },
            },
            SourceKind::Document,
        )
        .unwrap();

    db.conn()
        .execute("DELETE FROM document WHERE id = ?1", rusqlite::params![doc])
        .unwrap();

    let remaining_chunks: i64 = db
        .conn()
        .query_row(
            "SELECT count(*) FROM chunk WHERE id = ?1",
            rusqlite::params![chunk],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        remaining_chunks, 0,
        "a chunk outlived the document its block belonged to"
    );
    let remaining_blocks: i64 = db
        .conn()
        .query_row(
            "SELECT count(*) FROM block WHERE id = ?1",
            rusqlite::params![block],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining_blocks, 0, "the block itself must go too");
}

/// A block may not claim a document other than its page's either — the rung of
/// the same ladder one level up. Written through raw SQL because
/// `insert_block` reads the document from the page and so cannot express it.
#[test]
fn a_block_cannot_claim_another_document_than_its_page() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let a = db
        .insert_document(
            "a".repeat(64).as_str(),
            "text/plain",
            1,
            SourceKind::Document,
        )
        .unwrap();
    let b = db
        .insert_document(
            "b".repeat(64).as_str(),
            "text/plain",
            1,
            SourceKind::Document,
        )
        .unwrap();
    let page_of_b = db.insert_page(&b, 1, "native:txt", None).unwrap();

    let err = db
        .conn()
        .execute(
            "INSERT INTO block (page_id, document_id, type, reading_order, text)
             VALUES (?1, ?2, 'paragraph', 0, 't')",
            rusqlite::params![page_of_b, a],
        )
        .expect_err("a block must belong to its page's document");
    assert!(
        err.to_string().contains("FOREIGN KEY constraint failed"),
        "expected a foreign key violation, got: {err}"
    );
}

#[test]
fn one_document_can_live_at_several_paths() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let root = db.insert_watched_root("/Volumes/Archive").unwrap();
    let doc = db
        .insert_document(
            "b".repeat(64).as_str(),
            "text/plain",
            10,
            SourceKind::Document,
        )
        .unwrap();
    db.insert_path(
        root,
        "a/note.txt",
        &doc,
        OnDisk {
            size_bytes: 10,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();
    db.insert_path(
        root,
        "b/note.txt",
        &doc,
        OnDisk {
            size_bytes: 10,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();

    // Deleting the recorded copy must not remove a document that still exists
    // on disk under another path — the failure a single path column would cause.
    db.delete_path(root, "a/note.txt").unwrap();
    assert_eq!(db.path_count(&doc).unwrap(), 1);
    assert!(db.document_exists(&doc).unwrap());
}

/// "We do not know where this file is" must not render as a citation to the
/// empty path. Reachable today: a document indexed from inside an archive, or
/// one whose last copy on disk was deleted while the index still holds it.
#[test]
fn a_document_with_no_path_cites_no_path_rather_than_an_empty_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let doc = db
        .insert_document("d".repeat(64).as_str(), "text/plain", 5, SourceKind::Code)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    let block = db
        .insert_block(page, &paragraph(0, "x = 1", Some(1), Some(1)))
        .unwrap();
    let chunk = db
        .insert_chunk(
            &doc,
            0,
            "x = 1",
            &Locator {
                spans: vec![Segment {
                    block_id: block,
                    start: 0,
                    end: 5,
                    block_start: 0,
                }],
                coordinate: Coordinate::Line { start: 1, end: 1 },
            },
            SourceKind::Code,
        )
        .unwrap();

    let c = db.citation(chunk).unwrap().expect("chunk exists");
    assert_eq!(c.relative_path, None);
    assert_eq!(c.text, "x = 1");
}

/// The point of stamping the format version per row is that a reindex is
/// incremental: while it runs, rows written by the old text preparation sit
/// beside rows written by the new one, and each must still say which it is.
/// Round-tripping one row against the constant it was written from would pass
/// just as happily with a single global stamp in `meta`, which is what D14
/// rejects — so this inserts two vintages and asserts they stay distinguishable.
#[test]
fn chunks_of_two_format_versions_coexist_and_keep_their_own_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let older = INDEX_FORMAT_VERSION - 1;
    let doc = db
        .insert_document("c".repeat(64).as_str(), "text/plain", 5, SourceKind::Code)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    let block = db
        .insert_block(page, &paragraph(0, "x = 1", Some(1), Some(1)))
        .unwrap();

    // A row left behind by a previous Mnema. Raw SQL because the write surface
    // only ever stamps the current version — which is exactly as it should be.
    // `char_span` carries the current array shape, one element naming this
    // row's own `block_id`: the schema's CHECK requires that of every row
    // regardless of vintage, and nothing has shipped, so no row in the old
    // scalar shape exists anywhere to keep a fixture faithful to.
    let old_span = serde_json::to_string(&vec![Segment {
        block_id: block,
        start: 0,
        end: 5,
        block_start: 0,
    }])
    .unwrap();
    db.conn()
        .execute(
            "INSERT INTO chunk (document_id, block_id, ord, text, char_span, coordinate,
                                n_chars, content_hash, index_format_version, source_kind)
             VALUES (?1, ?2, 0, 'x = 1', ?4,
                     '{\"kind\":\"line\",\"start\":1,\"end\":1}', 5, 'deadbeef', ?3, 'code')",
            rusqlite::params![doc, block, older, old_span],
        )
        .unwrap();

    // And one written now, by the current pipeline.
    let fresh_chunk = db
        .insert_chunk(
            &doc,
            1,
            "x = 1",
            &Locator {
                spans: vec![Segment {
                    block_id: block,
                    start: 0,
                    end: 5,
                    block_start: 0,
                }],
                coordinate: Coordinate::Line { start: 1, end: 1 },
            },
            SourceKind::Code,
        )
        .unwrap();

    let mut stmt = db
        .conn()
        .prepare("SELECT index_format_version FROM chunk ORDER BY ord")
        .unwrap();
    let versions: Vec<i64> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(
        versions,
        vec![older, INDEX_FORMAT_VERSION],
        "the older row was rewritten, or both rows read one global stamp"
    );

    let v: i64 = db
        .conn()
        .query_row(
            "SELECT index_format_version FROM chunk WHERE id = ?1",
            [fresh_chunk],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(v, INDEX_FORMAT_VERSION);
}

// ---------------------------------------------------------------- Citation

/// `page_no` would answer differently depending on the format: a sheet index
/// for xlsx, a section index for docx — exactly the conflation `Coordinate`
/// exists to stop under D27. There is no page number on the interface at all
/// any more; a citation answers only through its typed coordinate.
#[test]
fn a_citation_answers_only_through_its_typed_coordinate() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let doc = db
        .insert_document("e".repeat(64).as_str(), "text/plain", 1, SourceKind::Code)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    let block = db
        .insert_block(
            page,
            &Block {
                block_type: BlockType::Code,
                ..paragraph(0, "x = 1", Some(1), Some(1))
            },
        )
        .unwrap();
    let chunk = db
        .insert_chunk(
            &doc,
            0,
            "x = 1",
            &Locator {
                spans: vec![Segment {
                    block_id: block,
                    start: 0,
                    end: 5,
                    block_start: 0,
                }],
                coordinate: Coordinate::Line { start: 1, end: 1 },
            },
            SourceKind::Code,
        )
        .unwrap();

    let c = db.citation(chunk).unwrap().expect("chunk exists");
    assert!(matches!(c.coordinate, Coordinate::Line { .. }));
}

// -------------------------------------------------------- atomic chunk write

/// One page, one block, document `"doc-1"` already inserted **and declared
/// finished** — shared setup for the tests below, which care only about
/// `insert_chunk` and `search_lexical`, not about the four-level model itself.
///
/// Finished, because under D61 a search does not answer with a document that is
/// still being written, and `insert_document` leaves one at `pending`. It
/// matters most for the tests here that assert **no** hits: a fixture left
/// `pending` satisfies them whatever `insert_chunk` did, which is the shape of
/// coverage that is not coverage.
struct OnePage {
    db: Db,
    block: i64,
    _dir: tempfile::TempDir,
}

impl std::ops::Deref for OnePage {
    type Target = Db;
    fn deref(&self) -> &Db {
        &self.db
    }
}

impl OnePage {
    fn block(&self) -> i64 {
        self.block
    }
}

fn fixture_one_page() -> OnePage {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    db.insert_document("doc-1", "text/plain", 4, SourceKind::Document)
        .unwrap();
    db.set_document_status("doc-1", DocumentStatus::Indexed)
        .unwrap();
    let page = db.insert_page("doc-1", 1, "native:txt", None).unwrap();
    let block = db
        .insert_block(page, &paragraph(0, "source text", Some(1), Some(1)))
        .unwrap();
    OnePage {
        db,
        block,
        _dir: dir,
    }
}

/// A locator naming the fixture's one block, sized to fit
/// `"Сторона А передає товар."` exactly — the positive case, which must still
/// write both rows in one call.
fn locator_of(db: &OnePage) -> Locator {
    Locator {
        spans: vec![Segment {
            block_id: db.block(),
            start: 0,
            end: "Сторона А передає товар.".chars().count() as u32,
            block_start: 0,
        }],
        coordinate: Coordinate::None,
    }
}

/// The failure `insert_chunk` exists to close: before this, the chunk row and
/// its `chunk_search` row were two public calls, and a chunk left after only
/// the first is stored, citable and embeddable but permanently unfindable by
/// keyword — with no error anywhere. Under D29 the lexical arm is the only
/// private way in, so that silence is the worst outcome the product has.
#[test]
fn a_written_chunk_is_always_findable() {
    let db = fixture_one_page();
    let id = db
        .insert_chunk(
            "doc-1",
            0,
            "Сторона А передає товар.",
            &locator_of(&db),
            SourceKind::Document,
        )
        .unwrap();
    let hits = db.search_lexical("товар", 10).unwrap();
    assert!(
        hits.contains(&id),
        "a chunk that is stored but unsearchable is the worst outcome under D29"
    );
}

/// `validate_locator` is what stands between a wrong offset computed upstream
/// and a citation that quotes past the end of its own text forever — nothing
/// downstream re-derives it from the original.
#[test]
fn a_locator_that_lies_about_its_text_is_refused() {
    let db = fixture_one_page();
    let bad = Locator {
        spans: vec![Segment {
            block_id: db.block(),
            start: 0,
            end: 999,
            block_start: 0,
        }],
        coordinate: Coordinate::Line { start: 1, end: 1 },
    };
    let err = db.insert_chunk("doc-1", 0, "four", &bad, SourceKind::Document);
    assert!(
        err.is_err(),
        "end 999 against 4 characters must not be storable"
    );
}

/// The companion case `validate_locator` exists to catch alongside an out-of-
/// range `end`: two spans that are internally in bounds but disagree with each
/// other about which comes first.
#[test]
fn segments_must_be_ordered_and_not_overlap() {
    let db = fixture_one_page();
    let text = "x".repeat(20);
    let overlapping = Locator {
        spans: vec![
            Segment {
                block_id: db.block(),
                start: 0,
                end: 10,
                block_start: 0,
            },
            Segment {
                block_id: db.block(),
                start: 5,
                end: 20,
                block_start: 0,
            },
        ],
        coordinate: Coordinate::Line { start: 1, end: 2 },
    };
    let err = db.insert_chunk("doc-1", 0, &text, &overlapping, SourceKind::Document);
    assert!(
        err.is_err(),
        "[0..10] and [5..20] overlap and must not be storable"
    );
}

// ------------------------------------------------- locating what was cited

/// A citation nobody can locate in the source is the one failure the whole
/// four-level model exists to prevent, and until now `Citation` carried no way
/// to locate it: `coordinate` says which lines to scroll to, and nothing said
/// which characters of which block to paint. The spans are already stored —
/// `chunk.char_span` — so this reads them back rather than re-deriving them,
/// which is the property that makes a highlight survive a restart.
#[test]
fn a_citation_carries_the_spans_a_highlight_measures_from() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let doc = db
        .insert_document(&"f".repeat(64), "text/plain", 40, SourceKind::Document)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    let first = db
        .insert_block(page, &paragraph(0, "Сторона А", Some(1), Some(1)))
        .unwrap();
    let second = db
        .insert_block(page, &paragraph(1, "передає товар.", Some(3), Some(3)))
        .unwrap();

    // Two blocks joined by the chunker's separator: the chunk's text is
    // "Сторона А\n\nпередає товар.", so the second span starts two characters
    // after the first one ends.
    let text = "Сторона А\n\nпередає товар.";
    let spans = vec![
        Segment {
            block_id: first,
            start: 0,
            end: 9,
            block_start: 0,
        },
        Segment {
            block_id: second,
            start: 11,
            end: 25,
            block_start: 0,
        },
    ];
    let chunk = db
        .insert_chunk(
            &doc,
            0,
            text,
            &Locator {
                spans: spans.clone(),
                coordinate: Coordinate::Line { start: 1, end: 3 },
            },
            SourceKind::Document,
        )
        .unwrap();

    let c = db.citation(chunk).unwrap().expect("chunk exists");
    assert_eq!(
        c.spans, spans,
        "the spans a highlight measures from must survive the round trip"
    );
}

/// `Option`, not `String`. A block id naming no row at all is a bug in whatever
/// produced the id; a block whose text is empty is an ordinary row. Collapsing
/// the two onto `""` would let the first render as a highlight over nothing
/// instead of failing where it can be seen.
#[test]
fn block_text_tells_an_absent_block_from_an_empty_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let doc = db
        .insert_document(&"g".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    let empty = db
        .insert_block(page, &paragraph(0, "", None, None))
        .unwrap();

    assert_eq!(db.block_text(empty).unwrap(), Some(String::new()));
    assert_eq!(
        db.block_text(empty + 10_000).unwrap(),
        None,
        "a block id that names no row is not an empty block"
    );
}

/// `Block` has carried `line_start`/`line_end` since task 1 and the schema has
/// had both columns since task 2, and `insert_block` wrote neither: every row
/// was NULL on both, and no test read them back. A `Coordinate::Line` computed
/// in memory was therefore true until the first re-read from the database and
/// false afterwards — the kind of defect that shows up as a citation pointing
/// at the wrong part of a file the user can see.
#[test]
fn a_blocks_line_numbers_reach_the_row() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let doc = db
        .insert_document(&"h".repeat(64), "text/plain", 1, SourceKind::Document)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    let block = db
        .insert_block(page, &paragraph(0, "текст", Some(12), Some(19)))
        .unwrap();

    let (start, end): (Option<i64>, Option<i64>) = db
        .conn()
        .query_row(
            "SELECT line_start, line_end FROM block WHERE id = ?1",
            [block],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((start, end), (Some(12), Some(19)));

    // And a format that has no lines still writes NULL rather than a zero that
    // would render as line 0.
    let none = db
        .insert_block(page, &paragraph(1, "без рядків", None, None))
        .unwrap();
    let start: Option<i64> = db
        .conn()
        .query_row("SELECT line_start FROM block WHERE id = ?1", [none], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(start, None);
}

// --------------------------------------- a chunk written inside a transaction

/// `insert_chunk` opens its own IMMEDIATE transaction, and SQLite cannot nest
/// one inside another. An orchestrator that writes a whole document under one
/// transaction therefore cannot call it — so the body is reachable through a
/// form that joins the caller's transaction instead of opening its own, and
/// the guarantee is the same one either way: the chunk row and its search row
/// land together or not at all.
#[test]
fn a_chunk_written_inside_an_open_transaction_still_gets_its_search_row() {
    let db = fixture_one_page();
    let id = db
        .transaction(|tx| {
            db.insert_chunk_in(
                tx,
                "doc-1",
                0,
                "Сторона А передає товар.",
                &locator_of(&db),
                SourceKind::Document,
            )
        })
        .unwrap();

    assert!(
        db.search_lexical("товар", 10).unwrap().contains(&id),
        "a chunk written under an ambient transaction must be findable too"
    );
}

/// The other half of the same guarantee: nothing escapes a transaction that
/// was not committed. Without this, `transaction` could be a `BEGIN` nobody
/// rolls back and every test above would stay green.
#[test]
fn a_transaction_that_is_not_committed_leaves_no_chunk_behind() {
    let db = fixture_one_page();
    let outcome: Result<i64, mnema_index::Error> = db.transaction(|tx| {
        db.insert_chunk_in(
            tx,
            "doc-1",
            0,
            "Сторона А передає товар.",
            &locator_of(&db),
            SourceKind::Document,
        )?;
        Err(mnema_index::Error::InvalidLocator("deliberate".into()))
    });
    assert!(outcome.is_err());

    assert!(
        db.search_lexical("товар", 10).unwrap().is_empty(),
        "the search row outlived the transaction that was rolled back"
    );
    let chunks: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM chunk", [], |r| r.get(0))
        .unwrap();
    assert_eq!(chunks, 0, "the chunk row outlived its own transaction");
}

/// `path.size_bytes` and `path.mtime` are what let a second pass over a folder
/// skip a file without opening it, and until now they were written and never
/// read — `schema.sql` calls them "cheap reconciliation without hashing" and
/// nothing reconciled. The row is keyed on the pair `(root, relative_path)`, so
/// a reader that ignores either half would answer for the wrong file.
#[test]
fn a_path_row_reads_back_under_its_own_root_and_name() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let archive = db.insert_watched_root("/Volumes/Archive").unwrap();
    let desktop = db.insert_watched_root("/Users/o/Desktop").unwrap();
    let doc = db
        .insert_document(&"i".repeat(64), "text/plain", 40, SourceKind::Document)
        .unwrap();
    db.insert_path(
        archive,
        "notes/kosto.txt",
        &doc,
        OnDisk {
            size_bytes: 40,
            mtime: 1_700_000_000_123_456_789,
        },
        "text",
        1,
    )
    .unwrap();
    db.insert_path(
        desktop,
        "notes/kosto.txt",
        &doc,
        OnDisk {
            size_bytes: 41,
            mtime: 7,
        },
        "text",
        1,
    )
    .unwrap();

    assert_eq!(
        db.path_entry(archive, "notes/kosto.txt").unwrap(),
        Some(mnema_index::PathEntry {
            document_id: doc.clone(),
            size_bytes: 40,
            mtime: 1_700_000_000_123_456_789,
            reader: "text".to_string(),
            reader_version: 1,
        }),
        "a nanosecond mtime must survive the round trip intact, not be truncated"
    );
    assert_eq!(
        db.path_entry(desktop, "notes/kosto.txt")
            .unwrap()
            .map(|e| e.size_bytes),
        Some(41),
        "the same relative path under another root is a different row"
    );
    assert_eq!(db.path_entry(archive, "notes/other.txt").unwrap(), None);
}

/// Clearing a document's content must reach every level below it — including
/// the lexical index, which is the level that would go on answering.
///
/// `chunk_search` is FTS5 external content: the `chunk_fts` rows are kept in
/// step by triggers on `chunk_search`, not by the foreign key. So the question
/// is whether an `AFTER DELETE` trigger fires when the delete arrives through
/// a *cascade* two levels up rather than as a statement of its own. It does,
/// and this is where that is measured — if it did not, a rebuild would leave
/// the old text findable while every table looked empty, which is the worst
/// shape this defect could take.
///
/// The `path` and `document` rows must survive: their ids are the file's
/// content hash and its place on disk, neither of which a rebuild changes.
#[test]
fn clearing_a_documents_content_empties_the_lexical_index_but_keeps_its_paths() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let root = db.insert_watched_root("/Volumes/Archive").unwrap();
    let doc = db
        .insert_document(&"j".repeat(64), "text/plain", 40, SourceKind::Document)
        .unwrap();
    db.insert_path(
        root,
        "a/kosto.txt",
        &doc,
        OnDisk {
            size_bytes: 40,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();
    db.insert_path(
        root,
        "b/kosto.txt",
        &doc,
        OnDisk {
            size_bytes: 40,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    let block = db
        .insert_block(page, &paragraph(0, "кошторис на ремонт", Some(1), Some(1)))
        .unwrap();
    db.insert_chunk(
        &doc,
        0,
        "кошторис на ремонт",
        &Locator {
            spans: vec![Segment {
                block_id: block,
                start: 0,
                end: 18,
                block_start: 0,
            }],
            coordinate: Coordinate::Line { start: 1, end: 1 },
        },
        SourceKind::Document,
    )
    .unwrap();
    db.set_document_status(&doc, DocumentStatus::Indexed)
        .unwrap();
    assert!(!db.search_lexical("кошторис", 10).unwrap().is_empty());

    db.clear_document_content(&doc).unwrap();

    let count = |sql: &str| -> i64 { db.conn().query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!(count("SELECT count(*) FROM page"), 0);
    assert_eq!(count("SELECT count(*) FROM block"), 0, "blocks cascade");
    assert_eq!(count("SELECT count(*) FROM chunk"), 0, "chunks cascade");
    assert_eq!(
        count("SELECT count(*) FROM chunk_search"),
        0,
        "search rows cascade"
    );
    // `chunk_fts` directly, not through `search_lexical`, and the difference is
    // the whole subject of this test. D61 gave `clear_document_content` a second
    // statement that returns the document to `pending`, and from then on
    // `search_lexical` answers nothing about it whatever the trigger did — so
    // asking the search would be asking the predicate, and the trigger that
    // this test is named for could stop firing without anything going red.
    assert_eq!(
        count("SELECT count(*) FROM chunk_fts"),
        0,
        "the trigger on chunk_search does not fire on a cascade, so a rebuilt \
         document would keep its old text in the lexical index"
    );
    assert!(
        db.search_lexical("кошторис", 10).unwrap().is_empty(),
        "and the search agrees, for both of the two reasons it now has"
    );

    assert!(
        db.document_exists(&doc).unwrap(),
        "the document's id is the file's content hash, which a rebuild does not change"
    );
    assert_eq!(
        db.path_count(&doc).unwrap(),
        2,
        "both copies of the file must keep their place in the index"
    );
}

/// Emptying a document and taking it out of the search are one write or
/// neither. D61.
///
/// The pair is what makes the fix a fix: content gone with the status still
/// `indexed` is exactly the state D61 abolishes, and before this it was
/// reachable by nothing worse than calling the method outside a transaction —
/// the delete would commit on its own and the status write behind it would not.
/// One statement was atomic by itself; two are not, so the method opens a
/// transaction and `clear_document_content_in` is what an orchestrator uses.
///
/// Forced with a trigger that aborts the status write, which is the only way
/// from outside to fail the second of two writes while letting the first run —
/// the same instrument `tests/slice.rs` uses, and the alternative is a
/// fault-injection seam in production code.
///
/// Both directions: the call fails **and** the document is exactly as it was.
/// Either alone is satisfied by a method that does nothing at all.
#[test]
fn emptying_a_document_and_taking_it_out_of_the_search_are_one_write() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let doc = db
        .insert_document(&"m".repeat(64), "text/plain", 40, SourceKind::Document)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    let block = db
        .insert_block(page, &paragraph(0, "кошторис на ремонт", Some(1), Some(1)))
        .unwrap();
    db.insert_chunk(
        &doc,
        0,
        "кошторис на ремонт",
        &Locator {
            spans: vec![Segment {
                block_id: block,
                start: 0,
                end: 18,
                block_start: 0,
            }],
            coordinate: Coordinate::Line { start: 1, end: 1 },
        },
        SourceKind::Document,
    )
    .unwrap();
    db.set_document_status(&doc, DocumentStatus::Indexed)
        .unwrap();

    db.conn()
        .execute_batch(
            "CREATE TRIGGER forced_failure BEFORE UPDATE ON document BEGIN
                 SELECT RAISE(ABORT, 'forced failure');
             END;",
        )
        .unwrap();
    let outcome = db.clear_document_content(&doc);
    db.conn()
        .execute_batch("DROP TRIGGER forced_failure")
        .unwrap();

    assert!(
        outcome.is_err(),
        "the premise is a status write that failed: {outcome:?}"
    );

    let count = |sql: &str| -> i64 { db.conn().query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!(
        count("SELECT count(*) FROM page"),
        1,
        "the delete committed without the status write beside it, which leaves \
         a document with no content still answering searches"
    );
    assert_eq!(count("SELECT count(*) FROM chunk_fts"), 1);
    assert_eq!(
        db.document_status(&doc).unwrap(),
        DocumentStatus::Indexed,
        "nothing happened, so the status is the one the document came in with"
    );
    assert_eq!(
        db.search_lexical("кошторис", 10).unwrap().len(),
        1,
        "and the document is still whole, so it still answers — without this the \
         assertions above are satisfied by a search that returns nothing"
    );
}

/// A transaction from another connection is refused, by both `_in` methods.
///
/// The doc comments used to say this case punished itself — a foreign
/// transaction "would deadlock against this one's write lock". It does not:
/// measured, `clear_document_content_in` with a transaction opened on a second
/// `Db` over the same file returns `Ok` in 407 µs and the write commits with
/// that transaction. Nothing on `self` is touched, so there is nothing to block
/// against, and the pair's atomicity silently becomes somebody else's.
///
/// Both directions, and the second half is what stops this from being satisfied
/// by a method that refuses everything: the same call on this `Db`'s own
/// transaction goes through and writes.
mod a_transaction_from_another_connection {
    use super::*;

    /// Two `Db`s over one file — the arrangement a running walk and a searching
    /// window are already in (`AppState::open_job_index`).
    fn two_connections() -> (tempfile::TempDir, Db, Db, String) {
        let dir = tempfile::tempdir().unwrap();
        register_vector_extension().unwrap();
        let path = dir.path().join("index.sqlite");
        let one = mnema_index::open(&path).unwrap();
        let two = mnema_index::open(&path).unwrap();
        let doc = one
            .insert_document(&"n".repeat(64), "text/plain", 40, SourceKind::Document)
            .unwrap();
        one.insert_page(&doc, 1, "native:txt", None).unwrap();
        (dir, one, two, doc)
    }

    #[test]
    #[should_panic(expected = "the transaction belongs to another connection")]
    fn is_refused_by_clear_document_content_in() {
        let (_d, one, two, doc) = two_connections();
        let tx = Transaction::new_unchecked(two.conn(), TransactionBehavior::Immediate).unwrap();
        let _ = one.clear_document_content_in(&tx, &doc);
    }

    #[test]
    #[should_panic(expected = "the transaction belongs to another connection")]
    fn is_refused_by_insert_chunk_in() {
        let (_d, one, two, doc) = two_connections();
        let page: i64 = one
            .conn()
            .query_row("SELECT id FROM page", [], |r| r.get(0))
            .unwrap();
        let block = one
            .insert_block(page, &paragraph(0, "кошторис", Some(1), Some(1)))
            .unwrap();
        let tx = Transaction::new_unchecked(two.conn(), TransactionBehavior::Immediate).unwrap();
        let _ = one.insert_chunk_in(
            &tx,
            &doc,
            0,
            "кошторис",
            &Locator {
                spans: vec![Segment {
                    block_id: block,
                    start: 0,
                    end: 8,
                    block_start: 0,
                }],
                coordinate: Coordinate::None,
            },
            SourceKind::Document,
        );
    }

    /// The other direction. Without it both tests above are satisfied by an
    /// assertion that fires on every call, which would take the product's own
    /// rebuild with it.
    #[test]
    fn but_this_db_s_own_transaction_goes_through() {
        let (_d, one, _two, doc) = two_connections();
        let page: i64 = one
            .conn()
            .query_row("SELECT id FROM page", [], |r| r.get(0))
            .unwrap();
        let block = one
            .insert_block(page, &paragraph(0, "кошторис", Some(1), Some(1)))
            .unwrap();
        one.transaction(|tx| {
            one.insert_chunk_in(
                tx,
                &doc,
                0,
                "кошторис",
                &Locator {
                    spans: vec![Segment {
                        block_id: block,
                        start: 0,
                        end: 8,
                        block_start: 0,
                    }],
                    coordinate: Coordinate::None,
                },
                SourceKind::Document,
            )?;
            one.clear_document_content_in(tx, &doc)
        })
        .expect("a transaction on this Db's own connection is the ordinary case");

        assert_eq!(
            one.document_status(&doc).unwrap(),
            DocumentStatus::Pending,
            "both writes ran, so the clear's half of the pair landed"
        );
    }
}

/// The reason `clear_document_content` exists rather than a second call to
/// `delete_document`: deleting a document takes every path that names it.
///
/// Not a defect — `path.document_id` is `ON DELETE CASCADE` on purpose, so that
/// removing a document cannot leave a path pointing at nothing. It is pinned
/// here because it is the property that made the obvious rebuild wrong, and
/// nothing else in the suite states it.
#[test]
fn deleting_a_document_takes_every_path_that_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let root = db.insert_watched_root("/Volumes/Archive").unwrap();
    let doc = db
        .insert_document(&"k".repeat(64), "text/plain", 40, SourceKind::Document)
        .unwrap();
    db.insert_path(
        root,
        "a/kosto.txt",
        &doc,
        OnDisk {
            size_bytes: 40,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();
    db.insert_path(
        root,
        "b/kosto.txt",
        &doc,
        OnDisk {
            size_bytes: 40,
            mtime: 1,
        },
        "text",
        1,
    )
    .unwrap();

    db.delete_document(&doc).unwrap();

    assert_eq!(
        db.path_count(&doc).unwrap(),
        0,
        "a rebuild that went through this would lose the other copy's place"
    );
}

// -------------------------------------------------- vectors and rebuilds

/// Fixtures for the two tests below, kept in one place so a helper the next
/// one needs is added here rather than grown a second time inside a test
/// body. `document_with_one_chunk` and `rebuild_with_one_chunk` share their
/// write, on purpose: a rebuild is not a different operation from a first
/// build, only a second call to it against a document id that already
/// exists.
mod fixture {
    use super::*;

    /// A fresh, empty index. `support::TempDb` and not a second copy of it:
    /// `support/mod.rs`'s own doc says it is written there so this is not
    /// grown a second time, and a second copy is exactly what a `TempDb`
    /// local to this module would be.
    pub fn db() -> support::TempDb {
        support::temp_db()
    }

    /// A 1024-wide embedding space — the width `unit_vector_1024` is built
    /// for, and the ordinary one under D95's default model.
    ///
    /// Delegates to `support::space_1024`, which Task 3's upsert/delete
    /// tests (`tests/space.rs`) need too: one definition rather than a
    /// second copy that can drift from this one.
    pub fn space_1024(db: &Db) -> i64 {
        support::space_1024(db)
    }

    /// A document with one page, one block and one chunk holding `text`.
    /// Returns the document id.
    pub fn document_with_one_chunk(db: &Db, text: &str) -> String {
        let doc = db
            .insert_document(
                &"9".repeat(64),
                "text/plain",
                text.len() as i64,
                SourceKind::Document,
            )
            .unwrap();
        write_one_chunk(db, &doc, text);
        doc
    }

    /// What a rebuild does after `clear_document_content`: writes a fresh
    /// page, block and chunk back onto a document id that already exists.
    pub fn rebuild_with_one_chunk(db: &Db, doc: &str, text: &str) {
        write_one_chunk(db, doc, text);
    }

    fn write_one_chunk(db: &Db, doc: &str, text: &str) {
        let page = db.insert_page(doc, 1, "native:txt", None).unwrap();
        let block = db
            .insert_block(page, &paragraph(0, text, None, None))
            .unwrap();
        db.insert_chunk(
            doc,
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
    }

    /// The sole chunk id belonging to `doc` — every fixture above leaves it
    /// with exactly one.
    pub fn only_chunk_id(db: &Db, doc: &str) -> i64 {
        db.conn()
            .query_row(
                "SELECT id FROM chunk WHERE document_id = ?1",
                rusqlite::params![doc],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// A unit vector along axis 0: valid for the cosine space `space_1024`
    /// builds, and — unlike an all-zero vector — one `check_rankable` accepts.
    ///
    /// Delegates to `support::unit_vector_1024`, for the same reason
    /// `space_1024` above does.
    pub fn unit_vector_1024() -> Vec<f32> {
        support::unit_vector_1024()
    }
}

/// A rebuild reuses chunk ids — `chunk.id` is `INTEGER PRIMARY KEY` without
/// `AUTOINCREMENT` — so a vector that outlives the clear does not merely take
/// up room: it names a chunk whose text is now different content, and the row
/// that recorded it as embedded went with the cascade. Search would answer
/// with text the file no longer contains.
#[test]
fn clearing_a_document_takes_its_vectors() {
    let db = fixture::db();
    let space = fixture::space_1024(&db);
    let doc = fixture::document_with_one_chunk(&db, "the original text");
    let chunk = fixture::only_chunk_id(&db, &doc);

    db.insert_vector(space, chunk, &fixture::unit_vector_1024())
        .expect("insert");
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 1);

    db.clear_document_content(&doc).expect("clear");

    assert_eq!(
        db.embedded_chunk_count(space).expect("count"),
        0,
        "the vector outlived the chunk it embeds"
    );
}

/// The first test asks whether the vector is gone. This one asks the question
/// that makes it matter: after a rebuild hands the freed id to a different
/// chunk, does anything of the old embedding attach to the new text.
#[test]
fn a_reused_chunk_id_gets_no_inherited_vector() {
    let db = fixture::db();
    let space = fixture::space_1024(&db);
    let doc = fixture::document_with_one_chunk(&db, "the original text");
    let first = fixture::only_chunk_id(&db, &doc);
    db.insert_vector(space, first, &fixture::unit_vector_1024())
        .expect("insert");
    // Without this, a version of `embedded_chunk_count` narrowed to
    // `chunk_embedding_state` alone would pass this test whether or not the
    // fix below exists: it would answer 0 before the clear too, and the
    // assertion at the end would be satisfied by a count that was never
    // anything else. This is the same "asks nothing" failure the first test's
    // own assertion above is written to avoid.
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 1);

    db.clear_document_content(&doc).expect("clear");
    fixture::rebuild_with_one_chunk(&db, &doc, "entirely different content");
    let second = fixture::only_chunk_id(&db, &doc);

    assert_eq!(
        second, first,
        "this test is pointless unless the id was reused"
    );
    assert_eq!(db.embedded_chunk_count(space).expect("count"), 0);
}

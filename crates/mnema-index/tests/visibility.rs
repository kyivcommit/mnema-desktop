//! Which documents a search is allowed to answer with.
//!
//! A document is written from the bottom up — pages, blocks, chunks, search
//! rows — and only afterwards does anything record that it is finished. Every
//! chunk written before that moment is already in `chunk_fts`, so without a
//! predicate a search answers with a document that is still being assembled:
//! the sections written so far hit, the sections not yet written return
//! nothing, and the person reading the window concludes the file does not
//! contain them. D61.
//!
//! Every fixture is invented — names, places and numbers that belong to nobody.

use mnema_core::{Block, BlockType, Coordinate, Locator, Segment, SourceKind};
use mnema_index::{Db, DocumentStatus, open, register_vector_extension};

fn open_index() -> (tempfile::TempDir, Db) {
    register_vector_extension().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = open(&dir.path().join("index.sqlite")).unwrap();
    (dir, db)
}

/// One document of one page and one chunk, left at the status
/// `insert_document` writes.
///
/// That status is `pending` (`schema.sql:71`), and it is not a detail of this
/// fixture: it is where a first indexing spends the whole of its write, from
/// the first chunk to the checkpoint. Nothing here calls
/// `set_document_status`, so a test that wants a finished document has to say
/// so, exactly as `ingest_file`'s step 5 does.
///
/// **Two blocks, one chunk**, and the first block is why. `chunk.id` and
/// `block.id` are independent `INTEGER PRIMARY KEY` sequences, and a fixture
/// writing one block per chunk advances them in step — so a search joining
/// `chunk_fts.rowid` to the wrong one of the two reads the right rows by
/// coincidence and every assertion here stays green. A block the chunker did
/// not keep (a running header is the ordinary case) puts the two sequences out
/// of step from the second document onward, which is what makes a wrong join
/// key visible at all.
fn write_document(db: &Db, id: &str, text: &str) -> i64 {
    let doc = db
        .insert_document(id, "text/plain", 1, SourceKind::Document)
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    db.insert_block(
        page,
        &Block {
            block_type: BlockType::PageHeader,
            reading_order: 0,
            language: None,
            text: "Управління земельних ресурсів".to_string(),
            line_start: None,
            line_end: None,
        },
    )
    .unwrap();
    let block = db
        .insert_block(
            page,
            &Block {
                block_type: BlockType::Paragraph,
                reading_order: 1,
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
    .unwrap()
}

/// How many rows of the lexical index `term` matches, asked of `chunk_fts`
/// directly and therefore past every predicate `search_lexical` applies.
///
/// This is what keeps the silence assertions from being one-sided. "No hits"
/// is also what a search that has stopped working returns, and what a document
/// whose chunks were never written returns; this says the text is in the
/// lexical index and the predicate is the only reason it does not come back.
fn rows_in_the_lexical_index(db: &Db, term: &str) -> i64 {
    db.conn()
        .query_row(
            "SELECT count(*) FROM chunk_fts WHERE chunk_fts MATCH ?1",
            [format!("\"{term}\"")],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn a_document_still_being_written_answers_no_search() {
    let (_d, db) = open_index();
    let chunk = write_document(&db, &"a".repeat(64), "Сівозміна на ділянці Вільхівка");

    assert_eq!(
        db.document_status(&"a".repeat(64)).unwrap(),
        DocumentStatus::Pending,
        "the premise is a document mid-write, which is what `pending` means"
    );
    assert_eq!(
        rows_in_the_lexical_index(&db, "вільхівка"),
        1,
        "the premise is that the chunk IS indexed — otherwise the silence below \
         says nothing about the predicate"
    );
    assert!(
        db.search_lexical("Вільхівка", 10).unwrap().is_empty(),
        "a document that has not been declared finished must not answer with \
         the part of itself that happens to be written, chunk {chunk}"
    );
}

#[test]
fn the_same_document_answers_once_it_is_declared_finished() {
    let (_d, db) = open_index();
    let id = "b".repeat(64);
    let chunk = write_document(&db, &id, "Сівозміна на ділянці Вільхівка");
    assert!(db.search_lexical("Вільхівка", 10).unwrap().is_empty());

    db.set_document_status(&id, DocumentStatus::Indexed)
        .unwrap();

    assert_eq!(
        db.search_lexical("Вільхівка", 10).unwrap(),
        vec![chunk],
        "without this half the test above is satisfied by a search that never \
         returns anything at all"
    );
}

/// The predicate is per document, not per index: one document being written
/// must not take its neighbours out of the search with it.
#[test]
fn a_finished_document_is_found_while_another_is_being_written() {
    let (_d, db) = open_index();
    let settled = "c".repeat(64);
    let writing = "d".repeat(64);
    let settled_chunk = write_document(&db, &settled, "Кошторис ремонту, ділянка Вільхівка");
    let writing_chunk = write_document(&db, &writing, "Сівозміна на ділянці Вільхівка");
    db.set_document_status(&settled, DocumentStatus::Indexed)
        .unwrap();

    assert_eq!(
        rows_in_the_lexical_index(&db, "вільхівка"),
        2,
        "both documents carry the word, or the test cannot tell the predicate \
         from the query"
    );
    assert_eq!(
        db.search_lexical("Вільхівка", 10).unwrap(),
        vec![settled_chunk],
        "the finished document must still be found, and the one being written \
         (chunk {writing_chunk}) must not be"
    );
}

/// What the predicate costs, on an index big enough for the answer to mean
/// something. Not an assertion — a measurement, printed:
///
/// ```text
/// cargo test --release -p mnema-index --test visibility -- --ignored --nocapture
/// ```
///
/// `#[ignore]`, because it builds ten thousand chunks and a gate that runs on
/// every commit should not. It is here rather than in a scratch file because
/// the number it prints is an input to a decision — D61 turned "each document
/// drops out of search for the length of its own rebuild" into "a predicate on
/// every search", and the size of that is the owner's to weigh, not something
/// to be asserted once and forgotten.
///
/// Both shapes, because they are not the same question. A selective query
/// reaches the join with a handful of candidate rows; a broad one reaches it
/// with every chunk in the index, which is the worst this predicate can cost
/// and the one worth knowing.
#[test]
#[ignore = "a measurement, not an assertion; ten thousand chunks"]
fn what_the_predicate_costs_on_a_non_empty_index() {
    const DOCUMENTS: usize = 200;
    const CHUNKS_PER_DOCUMENT: usize = 50;
    const RUNS: u32 = 200;

    let (_d, db) = open_index();
    for d in 0..DOCUMENTS {
        let id = format!("{d:064x}");
        let doc = db
            .insert_document(&id, "text/plain", 1, SourceKind::Document)
            .unwrap();
        let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
        db.transaction(|tx| {
            for c in 0..CHUNKS_PER_DOCUMENT {
                // One word every chunk carries and one that names this chunk
                // alone: the two queries below.
                let text = format!("Довідка про стан ділянки, запис {d}ф{c}");
                let block = db.insert_block(
                    page,
                    &Block {
                        block_type: BlockType::Paragraph,
                        reading_order: c as i64,
                        language: None,
                        text: text.clone(),
                        line_start: None,
                        line_end: None,
                    },
                )?;
                db.insert_chunk_in(
                    tx,
                    &doc,
                    c as i64,
                    &text,
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
                )?;
            }
            Ok(())
        })
        .unwrap();
        db.set_document_status(&id, DocumentStatus::Indexed)
            .unwrap();
    }

    // The same statement without the predicate, so the two numbers come from
    // one build, one machine and one index rather than from two runs of
    // different code. This is the query `search_lexical` ran before D61.
    let unfiltered = |expr: &str| {
        let mut stmt = db
            .conn()
            .prepare("SELECT rowid FROM chunk_fts WHERE chunk_fts MATCH ?1 ORDER BY rank LIMIT ?2")
            .unwrap();
        stmt.query_map(rusqlite::params![expr, 20i64], |r| r.get::<_, i64>(0))
            .unwrap()
            .count()
    };

    for (label, query, expr) in [
        ("broad   (matches every chunk)", "Довідка", "\"довідка\""),
        ("selective (matches one chunk)", "17ф31", "\"17ф31\""),
    ] {
        let before = std::time::Instant::now();
        let mut hits = 0;
        for _ in 0..RUNS {
            hits = unfiltered(expr);
        }
        let without = before.elapsed() / RUNS;

        let before = std::time::Instant::now();
        let mut filtered = 0;
        for _ in 0..RUNS {
            filtered = db.search_lexical(query, 20).unwrap().len();
        }
        let with = before.elapsed() / RUNS;

        assert_eq!(hits, filtered, "{label}: the two queries must agree");
        println!(
            "{label}: without {without:?}, with {with:?}  \
             ({DOCUMENTS} documents, {CHUNKS_PER_DOCUMENT} chunks each)"
        );
    }
}

/// `pending` is not the only status that is not `indexed`, and the predicate is
/// written against `indexed` rather than against `pending` for that reason.
///
/// Nothing writes these two today — `ingest_file` records `Indexed` and nothing
/// else (`crates/mnema-ingest/src/lib.rs:598`) — so this pins the column's
/// vocabulary (`schema.sql:71-72`) rather than a live path. A predicate spelled
/// `status <> 'pending'` passes every other test in this file and lets a
/// document whose indexing *failed* answer searches.
#[test]
fn a_failed_or_skipped_document_is_not_searchable_either() {
    for (n, status) in [
        ("e", DocumentStatus::Failed),
        ("f", DocumentStatus::Skipped),
    ] {
        let (_d, db) = open_index();
        let id = n.repeat(64);
        write_document(&db, &id, "Сівозміна на ділянці Вільхівка");
        db.set_document_status(&id, status).unwrap();

        assert_eq!(
            rows_in_the_lexical_index(&db, "вільхівка"),
            1,
            "{status:?}: the chunk is in the lexical index"
        );
        assert!(
            db.search_lexical("Вільхівка", 10).unwrap().is_empty(),
            "{status:?} is not a document a search may answer with"
        );
    }
}

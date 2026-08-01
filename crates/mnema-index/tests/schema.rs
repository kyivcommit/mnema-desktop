//! Tests of the DDL itself: the constraints that make a wrong state
//! unrepresentable, and the wiring that makes them apply at all.
//!
//! `the_migration_set_is_valid` proves only that the schema executes. Every
//! constraint below is invisible to it — a dropped CHECK, a UNIQUE turned plain,
//! a foreign key naming a table that does not exist all migrate cleanly and fail
//! later, at a user's first insert, if at all.

use mnema_core::{Block, BlockType, Segment, SourceKind};
use mnema_index::{Db, open, register_vector_extension};
use rusqlite::params;

fn fresh(dir: &tempfile::TempDir) -> Db {
    register_vector_extension().unwrap();
    open(&dir.path().join("index.sqlite")).unwrap()
}

/// One paragraph block with no line numbers — this file tests the DDL, and
/// none of its constraints is about lines.
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

/// Rejection means SQLITE_CONSTRAINT — "this row is bad" — and never
/// SQLITE_ERROR, which means "the statement is broken". A caller skipping bad
/// rows during a multi-hour indexing run keys on that difference, so it is
/// asserted here for every rejection in the file rather than in one test: the
/// message alone cannot tell the two apart, and a guard that raises the wrong
/// class stops an import that should have continued.
fn assert_rejected(result: rusqlite::Result<usize>, expected: &str, what: &str) {
    let err = result.expect_err(what);
    match &err {
        rusqlite::Error::SqliteFailure(e, _) => assert_eq!(
            e.code,
            rusqlite::ErrorCode::ConstraintViolation,
            "{what}: expected a constraint violation, got {:?}: {err}",
            e.code
        ),
        other => panic!("{what}: expected a constraint violation, got: {other}"),
    }
    assert!(
        err.to_string().contains(expected),
        "{what}: expected {expected:?}, got: {err}"
    );
}

/// SQLite resolves a foreign key's parent only when a row is written: a typo
/// like `REFERENCES documnet(id)` is accepted at CREATE TABLE, survives
/// migration and validation, and first fails as `foreign key mismatch` on a
/// user's machine. `PRAGMA foreign_key_check` does not catch it either — on an
/// empty database it has no rows to complain about and returns nothing. Walking
/// the declarations is what works.
#[test]
fn every_foreign_key_names_a_table_and_column_that_exist() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let conn = db.conn();

    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(!tables.is_empty(), "no tables — the schema did not apply");

    let mut checked = 0usize;
    for table in &tables {
        // (id, seq, parent_table, from_column, to_column, ...)
        let rows: Vec<(String, Option<String>)> = conn
            .prepare(&format!("PRAGMA foreign_key_list(\"{table}\")"))
            .unwrap()
            .query_map([], |r| Ok((r.get(2)?, r.get(4)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        for (parent, to_column) in rows {
            checked += 1;
            assert!(
                tables.contains(&parent),
                "{table} references table {parent:?}, which does not exist"
            );
            // `to` is NULL when the FK targets the parent's primary key
            // implicitly; there is no column name to verify in that case.
            let Some(column) = to_column else { continue };
            let columns: Vec<String> = conn
                .prepare(&format!("PRAGMA table_info(\"{parent}\")"))
                .unwrap()
                .query_map([], |r| r.get(1))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert!(
                columns.contains(&column),
                "{table} references {parent}({column}), which does not exist"
            );
        }
    }
    // 21 is what the schema declares, and it is not the number of `REFERENCES`
    // lines: `PRAGMA foreign_key_list` reports one row per FK *column*, so the
    // two composite keys — block→page and chunk→block, each on
    // `(id, document_id)` — contribute two rows apiece. 19 clauses, 21 rows.
    // The number was obtained by running this loop, not by counting the DDL.
    //
    // `>=`, so adding a constraint does not have to fight a test; the direction
    // that matters is downward. The floor used to be 15 against the same 20,
    // which left a quarter of the schema's referential integrity undefended:
    // five foreign keys were deleted and all 131 tests stayed green. One of
    // them, `chunk_search.chunk_id ON DELETE CASCADE`, is not bookkeeping —
    // without it a deleted chunk keeps its row in `chunk_search` and therefore
    // in `chunk_fts` for ever, so lexical search answers with an id whose
    // `citation()` is `None`. All five are in scripts/mutations/branch-review.sh.
    assert!(
        checked >= 21,
        "only {checked} foreign keys found — constraints have gone missing"
    );
}

/// The three provenances are exclusive states, so a pair cannot be `manual` and
/// `removed` at once. With provenance inside the primary key it could, and
/// "is this tag on?" would have no answer from the rows alone.
#[test]
fn a_document_tag_holds_one_provenance_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let doc = db
        .insert_document(
            "a".repeat(64).as_str(),
            "text/plain",
            1,
            SourceKind::Document,
        )
        .unwrap();
    db.conn()
        .execute("INSERT INTO tag (id, name) VALUES (1, 'contracts')", [])
        .unwrap();
    db.conn()
        .execute(
            "INSERT INTO document_tag VALUES (?1, 1, 'manual')",
            params![doc],
        )
        .unwrap();

    assert_rejected(
        db.conn().execute(
            "INSERT INTO document_tag VALUES (?1, 1, 'removed')",
            params![doc],
        ),
        "UNIQUE constraint failed",
        "a second provenance for the same (document, tag)",
    );
    assert_rejected(
        db.conn().execute(
            "INSERT INTO document_tag VALUES (?1, 1, 'banana')",
            params![doc],
        ),
        "CHECK constraint failed",
        "an unknown provenance",
    );
}

/// A path prefix is relative to a watched root, so it means nothing without one.
/// A rule ignores either a subtree or a tag, never both in the same row.
#[test]
fn an_ignore_rule_is_rooted_and_names_exactly_one_thing() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let root = db.insert_watched_root("/Volumes/Archive").unwrap();
    db.conn()
        .execute("INSERT INTO tag (id, name) VALUES (1, 'secret')", [])
        .unwrap();

    assert_rejected(
        db.conn().execute(
            "INSERT INTO ignore_rule (watched_root_id, path_prefix, tag_id)
             VALUES (NULL, 'secret/', NULL)",
            [],
        ),
        "CHECK constraint failed",
        "a path prefix relative to no root",
    );
    assert_rejected(
        db.conn().execute(
            "INSERT INTO ignore_rule (watched_root_id, path_prefix, tag_id)
             VALUES (?1, 'secret/', 1)",
            params![root],
        ),
        "CHECK constraint failed",
        "a rule that is both a path rule and a tag rule",
    );
    assert_rejected(
        db.conn().execute(
            "INSERT INTO ignore_rule (watched_root_id, path_prefix, tag_id)
             VALUES (?1, NULL, NULL)",
            params![root],
        ),
        "CHECK constraint failed",
        "a rule that ignores nothing",
    );

    // The two legitimate shapes.
    db.conn()
        .execute(
            "INSERT INTO ignore_rule (watched_root_id, path_prefix, tag_id)
             VALUES (?1, 'secret/', NULL)",
            params![root],
        )
        .expect("a rooted path rule");
    db.conn()
        .execute(
            "INSERT INTO ignore_rule (watched_root_id, path_prefix, tag_id)
             VALUES (NULL, NULL, 1)",
            [],
        )
        .expect("a tag rule needs no root");
}

/// Reading order is what reconstructs a page; two blocks in one slot make the
/// reconstruction arbitrary.
#[test]
fn two_blocks_cannot_share_a_reading_order_slot() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let doc = db
        .insert_document(
            "a".repeat(64).as_str(),
            "text/plain",
            1,
            SourceKind::Document,
        )
        .unwrap();
    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    db.insert_block(page, &paragraph(0, "first")).unwrap();

    let err = db
        .insert_block(page, &paragraph(0, "second"))
        .expect_err("two blocks must not share (page, reading_order)");
    assert!(
        err.to_string().contains("UNIQUE constraint failed"),
        "expected a uniqueness violation, got: {err}"
    );
}

/// A vec0 table corrupts silently on RENAME, so its name must never contain
/// anything that could later change — a model name above all. The name derives
/// from the space's immutable id, and the schema enforces it rather than
/// trusting the code that writes it.
#[test]
fn a_vector_table_is_named_after_its_space_id_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    db.conn()
        .execute(
            "INSERT INTO model_config (id, name, provider, embed_model, dim)
             VALUES (1, 'default', 'voyage', 'voyage-3', 1024)",
            [],
        )
        .unwrap();

    let insert = "INSERT INTO embedding_space
        (id, model_config_id, dim, index_format_version, chunker_hash, vec_table, state)
        VALUES (?1, 1, 1024, 1, 'abc', ?2, 'building')";

    assert_rejected(
        db.conn().execute(insert, params![3, "vec_voyage_3_1024"]),
        "CHECK constraint failed",
        "a vector table named after its model",
    );
    assert_rejected(
        db.conn().execute(insert, params![4, "vec_emb_0004"]),
        "CHECK constraint failed",
        "a zero-padded name, which does not match the id it derives from",
    );
    db.conn()
        .execute(insert, params![3, "vec_emb_3"])
        .expect("the derived name");

    // And the id may be left to SQLite: the rowid is assigned before the CHECK
    // is evaluated, so the constraint holds for an omitted id too.
    db.conn()
        .execute(
            "INSERT INTO embedding_space
                (model_config_id, dim, index_format_version, chunker_hash, vec_table, state)
             VALUES (1, 1024, 2, 'abc', 'vec_emb_4', 'building')",
            [],
        )
        .expect("an autoincrement id still satisfies the CHECK");
}

/// `source_kind`, `provenance` and `embedding_space.state` are all constrained;
/// these two were left open, so `status = 'banana'` was a valid document.
#[test]
fn status_and_embedding_state_reject_values_outside_their_lists() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let doc = db
        .insert_document(
            "a".repeat(64).as_str(),
            "text/plain",
            1,
            SourceKind::Document,
        )
        .unwrap();
    assert_rejected(
        db.conn().execute(
            "UPDATE document SET status = 'banana' WHERE id = ?1",
            params![doc],
        ),
        "CHECK constraint failed",
        "an unknown document status",
    );
    db.conn()
        .execute(
            "UPDATE document SET status = 'indexed' WHERE id = ?1",
            params![doc],
        )
        .expect("a known status");

    let page = db.insert_page(&doc, 1, "native:txt", None).unwrap();
    let block = db.insert_block(page, &paragraph(0, "x")).unwrap();
    let chunk = db
        .insert_chunk(
            &doc,
            0,
            "x",
            &mnema_core::Locator {
                spans: vec![Segment {
                    block_id: block,
                    start: 0,
                    end: 1,
                    block_start: 0,
                }],
                coordinate: mnema_core::Coordinate::None,
            },
            SourceKind::Code,
        )
        .unwrap();
    db.conn()
        .execute(
            "INSERT INTO model_config (id, name, provider, embed_model, dim)
             VALUES (1, 'default', 'voyage', 'voyage-3', 1024)",
            [],
        )
        .unwrap();
    db.conn()
        .execute(
            "INSERT INTO embedding_space
                (id, model_config_id, dim, index_format_version, chunker_hash, vec_table, state)
             VALUES (1, 1, 1024, 1, 'abc', 'vec_emb_1', 'building')",
            [],
        )
        .unwrap();

    assert_rejected(
        db.conn().execute(
            "INSERT INTO chunk_embedding_state VALUES (1, ?1, 'h', 7, 0)",
            params![chunk],
        ),
        "CHECK constraint failed",
        "an embedding state outside 0..=2",
    );
    db.conn()
        .execute(
            "INSERT INTO chunk_embedding_state VALUES (1, ?1, 'h', 0, 0)",
            params![chunk],
        )
        .expect("pending is a real state");
}

// ------------------------------------------------------------------ char_span

/// A chunk may draw its text from several blocks. The first stays in
/// `chunk.block_id`, where the composite foreign key reaches it; blocks 2..n are
/// integers inside the `char_span` JSON, where nothing does. Every negative case
/// below was a row this schema accepted before the guard existed.
struct TwoDocuments {
    db: Db,
    /// Document A. Its blocks are the ones a well-formed chunk of A may name.
    a: String,
    /// The block a chunk of A declares in `chunk.block_id`.
    a_block: i64,
    /// A second block on the SAME page as `a_block` — the legitimate multi-block
    /// case, which the guard must keep accepting.
    a_block_same_page: i64,
    /// A block of A on a different page. Both blocks belong to A, so the
    /// composite foreign key sees nothing wrong with a chunk naming both.
    a_block_other_page: i64,
    /// A block of document B.
    b_block: i64,
    /// Dropped last, after `db` has closed the file it lives in.
    _dir: tempfile::TempDir,
}

fn fixture_two_documents() -> TwoDocuments {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);

    let a = db
        .insert_document(
            "a".repeat(64).as_str(),
            "application/pdf",
            1,
            SourceKind::Document,
        )
        .unwrap();
    let b = db
        .insert_document(
            "b".repeat(64).as_str(),
            "application/pdf",
            1,
            SourceKind::Document,
        )
        .unwrap();

    let a_page = db.insert_page(&a, 1, "native:pdf", None).unwrap();
    let a_page_2 = db.insert_page(&a, 2, "native:pdf", None).unwrap();
    let b_page = db.insert_page(&b, 1, "native:pdf", None).unwrap();

    let a_block = db
        .insert_block(a_page, &paragraph(0, "перший абзац"))
        .unwrap();
    let a_block_same_page = db
        .insert_block(a_page, &paragraph(1, "другий абзац"))
        .unwrap();
    let a_block_other_page = db
        .insert_block(a_page_2, &paragraph(0, "наступна сторінка"))
        .unwrap();
    let b_block = db
        .insert_block(b_page, &paragraph(0, "текст документа B"))
        .unwrap();

    TwoDocuments {
        db,
        a,
        a_block,
        a_block_same_page,
        a_block_other_page,
        b_block,
        _dir: dir,
    }
}

impl TwoDocuments {
    /// Raw SQL on purpose. `insert_chunk` serialises a `Locator` and so cannot
    /// express a single case below; a guard tested only through it would be
    /// testing the writer, not the table. The constraint has to hold against
    /// whatever reaches the row.
    fn raw_insert_chunk(
        &self,
        document_id: &str,
        block_id: i64,
        ord: i64,
        text: &str,
        char_span: &str,
    ) -> rusqlite::Result<usize> {
        self.db.conn().execute(
            "INSERT INTO chunk (document_id, block_id, ord, text, char_span, coordinate,
                                n_chars, content_hash, index_format_version, source_kind)
             VALUES (?1, ?2, ?3, ?4, ?5, '{\"kind\":\"none\"}', ?6, 'deadbeef', 1, 'document')",
            params![
                document_id,
                block_id,
                ord,
                text,
                char_span,
                text.chars().count() as i64
            ],
        )
    }
}

/// A span in the exact shape the writer produces — `serde_json` over
/// `Vec<Segment>` — so no test can assert against a spelling `insert_chunk`
/// could never emit.
fn spans(segments: &[(i64, u32, u32)]) -> String {
    let v: Vec<Segment> = segments
        .iter()
        .map(|&(block_id, start, end)| Segment {
            block_id,
            start,
            end,
            block_start: 0,
        })
        .collect();
    serde_json::to_string(&v).unwrap()
}

/// The failure the whole four-level model exists to prevent, one level below
/// where the foreign keys can see it: a chunk of A quoting a block of B. The
/// composite key checks `chunk.block_id` and nothing else, so the second element
/// of the span crosses the document boundary unopposed — and a highlight
/// computed from it opens the wrong file.
#[test]
fn the_schema_refuses_a_span_naming_a_foreign_block() {
    let f = fixture_two_documents();
    assert_rejected(
        f.raw_insert_chunk(
            &f.a,
            f.a_block,
            0,
            "test",
            &spans(&[(f.a_block, 0, 4), (f.b_block, 4, 8)]),
        ),
        "char_span names a block outside",
        "a chunk must not reach a block of another document",
    );
}

/// Both blocks here belong to A, so no foreign key is violated and no code path
/// notices. But a chunk carries one coordinate, and a span crossing onto page 2
/// makes that page number a guess about half its own text.
#[test]
fn the_schema_refuses_a_span_crossing_a_page() {
    let f = fixture_two_documents();
    assert_rejected(
        f.raw_insert_chunk(
            &f.a,
            f.a_block,
            0,
            "test",
            &spans(&[(f.a_block, 0, 4), (f.a_block_other_page, 4, 8)]),
        ),
        "char_span names a block outside",
        "a chunk must not span two pages",
    );
}

/// `citation()` joins through `chunk.block_id`; a highlight measures from
/// `char_span[0].block_start`. When the two name different blocks the offsets
/// are read against text the citation never shows, and only one of the two is
/// under a foreign key.
///
/// The trigger cannot catch this one: both blocks are on A's page 1, so every
/// id in the span is legitimate. It is the CHECK's own case, which is why the
/// two guards are not redundant.
#[test]
fn the_schema_refuses_a_first_element_disagreeing_with_block_id() {
    let f = fixture_two_documents();
    assert_rejected(
        f.raw_insert_chunk(
            &f.a,
            f.a_block,
            0,
            "test",
            &spans(&[(f.a_block_same_page, 0, 4)]),
        ),
        "CHECK constraint failed",
        "char_span[0] must be the block the chunk declares",
    );
}

/// Rejected as a constraint violation rather than as `malformed JSON`. The
/// distinction is not cosmetic: SQLite reports malformed JSON as SQLITE_ERROR,
/// the class that means the statement is broken, while a caller sorting bad rows
/// from a broken database keys on SQLITE_CONSTRAINT. A `BEFORE INSERT` trigger
/// runs ahead of the CHECK, so `json_each(new.char_span)` reaches this string
/// first and raises the wrong class unless the trigger guards itself.
#[test]
fn the_schema_refuses_a_span_that_is_not_json() {
    let f = fixture_two_documents();
    assert_rejected(
        f.raw_insert_chunk(&f.a, f.a_block, 0, "test", "not json"),
        "CHECK constraint failed",
        "char_span must be JSON",
    );
}

/// A chunk whose span names no block at all cannot be highlighted, and
/// `block_id` alone cannot stand in for it — the column says where the chunk
/// starts, the span says how much of each block it took.
#[test]
fn the_schema_refuses_an_empty_span_array() {
    let f = fixture_two_documents();
    assert_rejected(
        f.raw_insert_chunk(&f.a, f.a_block, 0, "test", "[]"),
        "CHECK constraint failed",
        "a chunk must come from at least one block",
    );
}

/// Measured, not imagined: the first draft of this guard accepted both spans
/// below. `json_extract` returns NULL for a missing key, `NULL NOT IN (…)` is
/// NULL rather than true, and SQLite counts a CHECK that evaluates to NULL as
/// satisfied — so an element carrying no `block_id` slipped past the CHECK and
/// the trigger at once. A span is only guarded if every element of it names a
/// block that exists.
#[test]
fn the_schema_refuses_a_span_element_that_names_no_block() {
    let f = fixture_two_documents();

    assert_rejected(
        f.raw_insert_chunk(
            &f.a,
            f.a_block,
            0,
            "test",
            r#"[{"start":0,"end":4,"block_start":0}]"#,
        ),
        "char_span names a block outside",
        "the first element must name a block",
    );

    let later = format!(
        r#"[{{"block_id":{},"start":0,"end":4,"block_start":0}},{{"start":4,"end":8,"block_start":0}}]"#,
        f.a_block
    );
    assert_rejected(
        f.raw_insert_chunk(&f.a, f.a_block, 1, "test", &later),
        "char_span names a block outside",
        "a later element must name a block too",
    );
}

/// `json_valid` is not enough on its own: `json_valid('["hello"]')` is 1, so the
/// trigger used to iterate and then raise `malformed JSON` — SQLITE_ERROR — out
/// of `json_extract('hello', …)`. The same class of defect as the one it was
/// added to fix, one level down: valid JSON, invalid element.
///
/// Testing the type of every element, before extracting from it, is what keeps
/// these inside SQLITE_CONSTRAINT. `assert_rejected` is what pins the class.
#[test]
fn the_schema_refuses_a_span_element_that_is_not_an_object() {
    let f = fixture_two_documents();
    for (ord, span) in [r#"["hello"]"#, "[42]", "[null]", "[[1,2]]"]
        .iter()
        .enumerate()
    {
        assert_rejected(
            f.raw_insert_chunk(&f.a, f.a_block, ord as i64, "test", span),
            "char_span names a block outside",
            span,
        );
    }
}

/// `char_span` holding a bare object or a scalar is not "a span reaching the
/// wrong page" — it is not a span at all, and the error has to say so. The
/// trigger stands down for anything that is not a JSON array and lets the CHECK,
/// which is the constraint that actually holds this invariant, report it.
///
/// The first of the two is verbatim the literal `citation.rs:256` carries today.
#[test]
fn a_span_that_is_not_an_array_is_reported_as_such() {
    let f = fixture_two_documents();
    assert_rejected(
        f.raw_insert_chunk(&f.a, f.a_block, 0, "test", r#"{"start":0,"end":5}"#),
        "CHECK constraint failed",
        "a JSON object is not a span",
    );
    assert_rejected(
        f.raw_insert_chunk(&f.a, f.a_block, 1, "test", "123"),
        "CHECK constraint failed",
        "a bare scalar is not a span",
    );
}

/// The positive case, and the reason the guard is a trigger rather than a
/// foreign key: a chunk spanning several blocks of one page is exactly what
/// `char_span` was widened to hold. A constraint that rejects this has broken
/// the feature it was written to protect, so this must pass both before and
/// after the guard exists.
#[test]
fn the_schema_accepts_two_blocks_of_the_same_page() {
    let f = fixture_two_documents();
    f.raw_insert_chunk(
        &f.a,
        f.a_block,
        0,
        "перший абзац другий абзац",
        &spans(&[(f.a_block, 0, 12), (f.a_block_same_page, 13, 25)]),
    )
    .expect("a chunk may span two blocks of one page");
}

/// `block.type` and `page.text_source` were the last open string columns in the
/// document ladder, and both are read back as vocabularies: the interface groups
/// by block type, and `text_source` decides whether a page's text is trustworthy
/// enough to cite. An open column turns a writer's typo into a row that no query
/// ever matches again, and nothing reports it.
#[test]
fn block_type_and_text_source_reject_values_outside_their_vocabularies() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let doc = db
        .insert_document(
            "a".repeat(64).as_str(),
            "text/plain",
            1,
            SourceKind::Document,
        )
        .unwrap();

    let err = db
        .insert_page(&doc, 1, "guessed", None)
        .expect_err("text_source must say how the text was obtained");
    assert!(
        err.to_string().contains("CHECK constraint failed"),
        "expected a CHECK violation, got: {err}"
    );

    let page = db.insert_page(&doc, 1, "native:pdf", None).unwrap();
    // OCR is not implemented in v1, but the vocabulary already admits it: the
    // constraint is on the shape `family:detail`, so adding an engine later
    // costs no migration.
    db.insert_page(&doc, 2, "ocr:tesseract", None)
        .expect("an ocr source is a legal shape");

    // Raw SQL, because `insert_block` now takes a typed `BlockType` and cannot
    // express a type outside the vocabulary at all — which is the improvement,
    // not a reason to stop asking whether the column still guards a row written
    // around that writer.
    let err = db
        .conn()
        .execute(
            "INSERT INTO block (page_id, document_id, type, reading_order, text)
             VALUES (?1, (SELECT document_id FROM page WHERE id = ?1), 'banana', 0, 'x')",
            params![page],
        )
        .expect_err("a block type outside the vocabulary");
    assert!(
        err.to_string().contains("CHECK constraint failed"),
        "expected a CHECK violation, got: {err}"
    );

    // `code` is the eighth type, and ours alone: the server's structurizer has
    // seven and never sees a source file.
    db.insert_block(
        page,
        &Block {
            block_type: BlockType::Code,
            ..paragraph(0, "let x = 1;")
        },
    )
    .expect("code is a block type only this product produces");
}

/// `text_source` names a family, and the family is lower case.
///
/// The CHECK used `LIKE`, which is case-insensitive for ASCII, so `Native:pdf`
/// and `OCR:y` passed — values no reader produces and no query grouping by
/// family would ever match again. `GLOB` is the one-word fix and it was free
/// only while nothing had shipped and this file was still edited in place at
/// `SCHEMA_VERSION` 1.
#[test]
fn a_text_source_family_is_lower_case_or_it_is_not_a_family() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh(&dir);
    let doc = db
        .insert_document(&"a".repeat(64), "application/pdf", 1, SourceKind::Document)
        .unwrap();

    for (page_no, accepted) in [
        ("native:pdf", true),
        ("ocr:tesseract", true),
        ("Native:pdf", false),
        ("NATIVE:x", false),
        ("OcR:y", false),
        ("nativ:z", false),
    ]
    .iter()
    .enumerate()
    .map(|(i, (s, ok))| ((i + 1) as i64, (*s, *ok)))
    {
        let (source, ok) = accepted;
        let outcome = db.insert_page(&doc, page_no, source, None);
        assert_eq!(
            outcome.is_ok(),
            ok,
            "{source:?} should {} have been accepted",
            if ok { "" } else { "not" }
        );
    }
}

//! The read-side half of the chunk-id-reuse defect: `chunk.id` is
//! `INTEGER PRIMARY KEY` without `AUTOINCREMENT`, so a rebuild can hand a
//! deleted chunk's id to a chunk holding different text. `Db::read_snapshot`
//! closes it by holding every read `search` runs to one commit — these
//! tests are the oracle `crates/mnema-index/tests/contention.rs:84-115`
//! already established, run against `mnema_search::search` itself, with a
//! real second connection committing while the first is still inside its
//! snapshot.

use mnema_core::{Block, BlockType, Coordinate, Locator, Segment, SourceKind};
use mnema_index::{Db, DocumentStatus, QueryRule, open, register_vector_extension};
use mnema_search::{Arms, ContentQuery, FusionRule};

fn fresh(dir: &std::path::Path) -> Db {
    register_vector_extension().expect("register the vector extension");
    open(&dir.join("index.sqlite")).expect("open the index")
}

/// A document with one page, one block and one chunk holding `text`, its
/// status `indexed` so the lexical arm can find it. Returns the document id.
fn document_with_one_chunk(db: &Db, id: &str, text: &str) -> String {
    let doc = db
        .insert_document(id, "text/plain", text.len() as i64, SourceKind::Document)
        .expect("document");
    write_one_chunk(db, &doc, text);
    db.set_document_status(&doc, DocumentStatus::Indexed)
        .expect("status");
    doc
}

/// What a rebuild does after `clear_document_content`: writes a fresh page,
/// block and chunk back onto a document id that already exists, and marks
/// it searchable again.
fn rebuild_with_one_chunk(db: &Db, doc: &str, text: &str) {
    write_one_chunk(db, doc, text);
    db.set_document_status(doc, DocumentStatus::Indexed)
        .expect("status");
}

fn write_one_chunk(db: &Db, doc: &str, text: &str) {
    let page = db.insert_page(doc, 1, "native:txt", None).expect("page");
    let block = db
        .insert_block(
            page,
            &Block {
                block_type: BlockType::Paragraph,
                reading_order: 0,
                language: Some("uk".into()),
                text: text.to_string(),
                line_start: None,
                line_end: None,
            },
        )
        .expect("block");
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
    .expect("chunk");
}

fn only_chunk_id(db: &Db, doc: &str) -> i64 {
    db.conn()
        .query_row("SELECT id FROM chunk WHERE document_id = ?1", [doc], |r| {
            r.get(0)
        })
        .expect("the document holds exactly one chunk")
}

fn unit_vector_1024() -> Vec<f32> {
    let mut v = vec![0.0; 1024];
    v[0] = 1.0;
    v
}

/// T1 (design §7, the content arm): a second connection rebuilds the
/// document `first`'s chunk belongs to — same id, different text — while
/// the read snapshot search runs inside is still open. The citation the
/// answer carries, resolved after `search` returns (`bridge.rs`'s own
/// shape), must be the snapshot's own text, never the rebuild's.
#[test]
fn a_rebuild_inside_the_snapshot_does_not_reach_the_content_arms_citation() {
    let dir = tempfile::tempdir().expect("temp dir");
    let reader = fresh(dir.path());
    let writer = open(&dir.path().join("index.sqlite")).expect("second connection");

    let space = reader
        .adopt_embedding_model("baai/bge-m3", 1024, "credential-ref", "chunker-v1")
        .expect("adopt")
        .space_id;
    let doc = document_with_one_chunk(&reader, &"1".repeat(64), "text A");
    let first = only_chunk_id(&reader, &doc);
    reader
        .upsert_vector(space, first, &unit_vector_1024())
        .expect("vector");

    let texts = reader
        .read_snapshot(|db| {
            let content = ContentQuery {
                space_id: space,
                vector: unit_vector_1024(),
            };
            let found = mnema_search::search(
                db,
                Some(content),
                "irrelevant",
                Arms {
                    text: false,
                    content: true,
                },
                QueryRule::AnyTerm,
                FusionRule::ContentOnly,
                20,
            )?;
            assert_eq!(found.chunks, vec![first]);

            // The second connection's commit lands here: after the content
            // arm ran its own `knn` + liveness check, before the citation
            // loop that resolves `found.chunks` into `Hit`s (`bridge.rs`,
            // after `search` returns).
            writer.clear_document_content(&doc).expect("clear");
            rebuild_with_one_chunk(&writer, &doc, "text B");
            let second = only_chunk_id(&writer, &doc);
            assert_eq!(second, first, "pointless unless the id was reused");

            let mut texts = Vec::new();
            for id in &found.chunks {
                texts.push(db.citation(*id)?.map(|c| c.text));
            }
            Ok(texts)
        })
        .expect("the snapshot closes cleanly");

    assert_eq!(
        texts,
        vec![Some("text A".to_string())],
        "the citation inside the snapshot must be the snapshot's own text"
    );
}

/// T2 (design §7, the widest gap): the same race, but the id comes from
/// `search_lexical_with` — the arm that is on by default with no key
/// (D29) — and the second connection's commit lands between the lexical
/// arm running (inside `search`) and the citation lookup that resolves the
/// answer's text (`bridge.rs`, after `search` returns).
#[test]
fn a_rebuild_inside_the_snapshot_does_not_reach_the_text_arms_citation() {
    let dir = tempfile::tempdir().expect("temp dir");
    let reader = fresh(dir.path());
    let writer = open(&dir.path().join("index.sqlite")).expect("second connection");

    let doc = document_with_one_chunk(&reader, &"2".repeat(64), "унікальний маркер тут");
    let first = only_chunk_id(&reader, &doc);

    let texts = reader
        .read_snapshot(|db| {
            let found = mnema_search::search(
                db,
                None,
                "маркер",
                Arms {
                    text: true,
                    content: false,
                },
                QueryRule::AnyTerm,
                FusionRule::TextOnly,
                20,
            )?;
            assert_eq!(found.chunks, vec![first]);

            writer.clear_document_content(&doc).expect("clear");
            rebuild_with_one_chunk(&writer, &doc, "інший текст замість маркера");
            let second = only_chunk_id(&writer, &doc);
            assert_eq!(second, first, "pointless unless the id was reused");

            let mut texts = Vec::new();
            for id in &found.chunks {
                texts.push(db.citation(*id)?.map(|c| c.text));
            }
            Ok(texts)
        })
        .expect("the snapshot closes cleanly");

    assert_eq!(
        texts,
        vec![Some("унікальний маркер тут".to_string())],
        "the citation inside the snapshot must be the snapshot's own text"
    );
}

/// T5 (design §7, §6.2): `content_arm_answered`'s `embedded` and `total`
/// come from two sequential reads — the same shape `read_snapshot` was
/// written for (`crates/mnema-index/tests/contention.rs:84-115`). A second
/// connection deletes one of the two embedded documents between them, still
/// inside the snapshot — the direction that would leave `embedded > total`
/// if the read straddled it — deleting vectors before the document, the
/// ordering `forget_if_unnamed` relies on.
#[test]
fn embedded_never_outruns_total_across_a_commit_inside_the_snapshot() {
    let dir = tempfile::tempdir().expect("temp dir");
    let reader = fresh(dir.path());
    let writer = open(&dir.path().join("index.sqlite")).expect("second connection");

    let space = reader
        .adopt_embedding_model("baai/bge-m3", 1024, "credential-ref", "chunker-v1")
        .expect("adopt")
        .space_id;
    let kept = document_with_one_chunk(&reader, &"3".repeat(64), "kept");
    let kept_chunk = only_chunk_id(&reader, &kept);
    reader
        .upsert_vector(space, kept_chunk, &unit_vector_1024())
        .expect("vector");
    let doomed = document_with_one_chunk(&reader, &"4".repeat(64), "doomed");
    let doomed_chunk = only_chunk_id(&reader, &doomed);
    reader
        .upsert_vector(space, doomed_chunk, &unit_vector_1024())
        .expect("vector");

    let (embedded, total) = reader
        .read_snapshot(|db| {
            let embedded = db.embedded_chunk_count(space)?;

            writer
                .delete_vectors_for_document(&doomed)
                .expect("vectors");
            writer.delete_document(&doomed).expect("document");

            let total = db.chunk_count()?;
            Ok((embedded, total))
        })
        .expect("the snapshot closes cleanly");

    assert!(
        embedded <= total,
        "embedded ({embedded}) outran total ({total}) across a commit inside the snapshot"
    );
    assert_eq!(
        (embedded, total),
        (2, 2),
        "the snapshot must not see the second connection's commit at all"
    );
}

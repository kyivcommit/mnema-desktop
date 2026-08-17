use mnema_core::{Block, BlockType, Coordinate, Locator, Segment, SourceKind};
use mnema_index::DocumentStatus;

mod support;

/// Codex round 3, Finding 7 (design §1.2, T2). The lexical arm already
/// filters `document.status = 'indexed'` (`search.rs:118,142`); before this
/// fix the content arm filtered only "does `citation` still find it" —
/// existence, not status — so a chunk from a `Pending`, `Failed` or
/// `Skipped` document could reach the content arm's answer while the
/// lexical arm refused it. Both arms are asked the same query over the
/// same corpus here, and their answers about that corpus must name the
/// same chunks.
#[test]
fn the_content_arm_and_the_lexical_arm_answer_about_the_same_documents() {
    for hidden in [
        DocumentStatus::Pending,
        DocumentStatus::Failed,
        DocumentStatus::Skipped,
    ] {
        let f = support::indexed_space();
        let space =
            f.db.active_space()
                .expect("active space read")
                .expect("a space is active");

        // A second document, embedded near the query vector and carrying
        // matching text, then hidden behind `hidden`.
        let doc =
            f.db.insert_document(&"d".repeat(64), "text/plain", 64, SourceKind::Document)
                .expect("hidden document");
        let page =
            f.db.insert_page(&doc, 1, "native:txt", None)
                .expect("hidden page");
        let block =
            f.db.insert_block(
                page,
                &Block {
                    block_type: BlockType::Paragraph,
                    reading_order: 0,
                    language: Some("uk".into()),
                    text: "ремонт даху, схований чанк".to_string(),
                    line_start: None,
                    line_end: None,
                },
            )
            .expect("hidden block");
        let hidden_chunk =
            f.db.insert_chunk(
                &doc,
                0,
                "ремонт даху, схований чанк",
                &Locator {
                    spans: vec![Segment {
                        block_id: block,
                        start: 0,
                        end: 26,
                        block_start: 0,
                    }],
                    coordinate: Coordinate::None,
                },
                SourceKind::Document,
            )
            .expect("hidden chunk");
        // The exact vector `indexed_space`'s chunk 0 was embedded on, so
        // the content arm ranks it first alongside chunk 0 — a real tie,
        // not merely "somewhere in range".
        let query_vector = support::vector_matching(&f, f.chunk_ids[0]);
        f.db.upsert_vector(space, hidden_chunk, &query_vector)
            .expect("hidden vector");
        f.db.set_document_status(&doc, hidden).expect("hide it");

        let content = mnema_search::ContentQuery {
            space_id: space,
            vector: query_vector,
        };
        let found = mnema_search::search(
            &f.db,
            Some(content),
            "ремонт даху",
            true,
            mnema_index::QueryRule::AnyTerm,
            mnema_search::FusionRule::TextOnly,
            20,
        )
        .unwrap();

        let content_chunks: Vec<i64> = match found.content {
            mnema_search::ContentArm::Answered { chunks, .. } => chunks,
            other => panic!("expected an answer for {hidden:?}, got {other:?}"),
        };
        let lexical_chunks: Vec<i64> = match found.text {
            mnema_search::TextArm::Answered { chunks } => chunks,
            other => panic!("expected an answer for {hidden:?}, got {other:?}"),
        };
        let mut content_sorted = content_chunks.clone();
        content_sorted.sort();
        let mut lexical_sorted = lexical_chunks.clone();
        lexical_sorted.sort();
        assert_eq!(
            content_sorted, lexical_sorted,
            "content and lexical disagreed about the {hidden:?} document: \
             content {content_chunks:?}, lexical {lexical_chunks:?}"
        );
        assert!(
            !content_chunks.contains(&hidden_chunk),
            "the content arm returned a chunk from a {hidden:?} document: {content_chunks:?}"
        );
    }
}

/// The text arm shows its own answer even when the content arm has no
/// input. A search that failed whole because one arm had nothing to run on
/// would turn a missing key into "your documents do not say this". `search`
/// no longer resolves *why* the content arm has nothing — that happens
/// before any snapshot opens now, in whoever calls this — so it answers
/// `Off`, not `NotConfigured`.
#[test]
fn the_text_arm_still_answers_when_the_content_arm_has_no_input() {
    let f = support::indexed_space();
    let found = mnema_search::search(
        &f.db,
        None,
        "ремонт даху",
        true,
        mnema_index::QueryRule::AnyTerm,
        mnema_search::FusionRule::Rrf,
        20,
    )
    .unwrap();

    assert!(!found.chunks.is_empty(), "the text arm had answers to give");
    assert!(matches!(found.text, mnema_search::TextArm::Answered { .. }));
    assert_eq!(found.content, mnema_search::ContentArm::Off);
}

/// An arm that was not asked is `Off`, which is neither an empty answer nor a
/// failure — and its chunks take no part in the fusion.
#[test]
fn an_arm_that_is_off_contributes_nothing_and_says_so() {
    let f = support::indexed_space();
    let found = mnema_search::search(
        &f.db,
        None,
        "ремонт даху",
        false,
        mnema_index::QueryRule::AnyTerm,
        mnema_search::FusionRule::TextOnly,
        20,
    )
    .unwrap();

    assert_eq!(found.text, mnema_search::TextArm::Off);
    assert!(
        found.chunks.is_empty(),
        "TextOnly over an off text arm is empty"
    );
}

/// Both arms off is answered, not refused: the invariant that one must be on
/// belongs to the window, and a core that panicked here would make the window's
/// job harder rather than safer.
#[test]
fn both_arms_off_is_an_empty_answer_with_both_states_named() {
    let f = support::indexed_space();
    let found = mnema_search::search(
        &f.db,
        None,
        "ремонт даху",
        false,
        mnema_index::QueryRule::AnyTerm,
        mnema_search::FusionRule::Rrf,
        20,
    )
    .unwrap();

    assert!(found.chunks.is_empty());
    assert_eq!(found.text, mnema_search::TextArm::Off);
    assert_eq!(found.content, mnema_search::ContentArm::Off);
}

/// The capstone property: with both arms real and on, the content arm's
/// chunks are not dropped before fusing. `"покрівля"` matches only chunk 1
/// lexically, so any other chunk in `found.chunks` must have come from the
/// content arm — a mutant that fused against an empty content list instead
/// of the real one would lose it.
///
/// The vector is given directly rather than through a mock provider:
/// `search` no longer embeds anything itself, so there is no network call
/// here to answer.
#[test]
fn the_content_arms_chunks_survive_into_the_fused_list() {
    let f = support::indexed_space();
    let space =
        f.db.active_space()
            .expect("active space read")
            .expect("a space is active");
    let content = mnema_search::ContentQuery {
        space_id: space,
        vector: support::vector_matching(&f, f.chunk_ids[1]),
    };

    let found = mnema_search::search(
        &f.db,
        Some(content),
        "покрівля",
        true,
        mnema_index::QueryRule::AnyTerm,
        mnema_search::FusionRule::Rrf,
        20,
    )
    .unwrap();

    assert!(matches!(
        found.content,
        mnema_search::ContentArm::Answered { .. }
    ));
    assert!(
        found.chunks.contains(&f.chunk_ids[0]),
        "chunk 0's text never matches \"покрівля\"; only the content arm \
         could have placed it in the fused list"
    );
}

/// Each arm is queried to `CANDIDATES`, deeper than the caller's `limit` —
/// otherwise a fusion rule would have nothing beyond `limit` to reorder.
/// The arm's own answer is checked directly, by count rather than order, so
/// this does not depend on how ties in `rank` or `distance` are broken.
#[test]
fn the_text_arm_is_asked_deeper_than_the_final_limit() {
    let f = support::indexed_space();
    let found = mnema_search::search(
        &f.db,
        None,
        "ремонт даху",
        true,
        mnema_index::QueryRule::AnyTerm,
        mnema_search::FusionRule::TextOnly,
        2,
    )
    .unwrap();

    match found.text {
        mnema_search::TextArm::Answered { chunks } => {
            assert_eq!(
                chunks.len(),
                f.chunk_ids.len(),
                "the arm's own answer must hold every match, not just `limit`"
            );
        }
        other => panic!("expected an answer, got {other:?}"),
    }
    assert_eq!(found.chunks.len(), 2, "fuse still cuts to `limit` after");
}

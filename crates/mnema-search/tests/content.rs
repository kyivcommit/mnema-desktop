use mnema_core::{Block, BlockType, Coordinate, Locator, Segment, SourceKind};
use mnema_search::{ContentArm, Missing, Provider};

mod support;

/// Codex review on PR #10: `Db::knn` answers straight from the vector
/// table, with no join back to `chunk` — a vector can outlive the chunk it
/// embeds, through `insert_vector`/`upsert_vector` writing an id no `chunk`
/// row backs (`Db::chunk_count`'s own doc). A neighbour ranked ahead of a
/// live chunk but with no row left in `chunk` must not appear in the
/// answer, and must not push a live chunk out of it either.
#[test]
fn an_orphaned_neighbour_is_skipped_without_shortening_the_answer() {
    let f = support::indexed_space();
    let space =
        f.db.active_space()
            .expect("active space read")
            .expect("a space is active");

    let victim_doc =
        f.db.insert_document(
            &"b".repeat(64),
            "text/plain",
            64,
            mnema_core::SourceKind::Document,
        )
        .expect("victim document");
    let page =
        f.db.insert_page(&victim_doc, 1, "native:txt", None)
            .expect("victim page");
    let block =
        f.db.insert_block(
            page,
            &mnema_core::Block {
                block_type: mnema_core::BlockType::Paragraph,
                reading_order: 0,
                language: Some("uk".into()),
                text: "ремонт даху, чанк-жертва".to_string(),
                line_start: None,
                line_end: None,
            },
        )
        .expect("victim block");
    let victim_chunk =
        f.db.insert_chunk(
            &victim_doc,
            0,
            "ремонт даху, чанк-жертва",
            &mnema_core::Locator {
                spans: vec![mnema_core::Segment {
                    block_id: block,
                    start: 0,
                    end: 24,
                    block_start: 0,
                }],
                coordinate: mnema_core::Coordinate::None,
            },
            mnema_core::SourceKind::Document,
        )
        .expect("victim chunk");

    let mut victim_vector = vec![0.0f32; 1024];
    victim_vector[3] = 1.0;
    f.db.upsert_vector(space, victim_chunk, &victim_vector)
        .expect("victim vector");

    // Raw SQL, not `Db::delete_document`: round-3's fix made that method
    // sweep a document's vectors itself (`delete_document_sweeps_its_own_
    // vectors`, `mnema-index/tests/space.rs`), so it no longer leaves this
    // orphan behind. What is reconstructed here directly is the residual
    // gap `Db::chunk_count`'s own doc still names: `insert_vector`/
    // `upsert_vector` writing a `chunk_id` no `chunk` row backs, held by a
    // test rather than a type. `vec0` cannot hold a foreign key to `chunk`
    // (G7.0 §5.7), so the vector survives the document's deletion — an
    // orphan ranked first, since the query below asks for exactly this
    // vector.
    f.db.conn()
        .execute("DELETE FROM document WHERE id = ?1", [victim_doc.as_str()])
        .expect("delete the victim document");

    let row: Vec<String> = victim_vector.iter().map(|v| v.to_string()).collect();
    let mock =
        mnema_mock_provider::MockServer::new(vec![mnema_mock_provider::Reply::ok(&format!(
            r#"{{"data":[{{"embedding":[{}],"index":0}}]}}"#,
            row.join(",")
        ))]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };

    let arm = mnema_search::content_arm(&f.db, Some(provider), "ремонт даху", 3);

    match arm {
        mnema_search::ContentArm::Answered { chunks, .. } => {
            assert_eq!(
                chunks.len(),
                3,
                "an orphaned neighbour shortened the answer below the \
                 three live chunks the space holds: {chunks:?}"
            );
            assert!(
                !chunks.contains(&victim_chunk),
                "the orphaned chunk id reached the answer: {chunks:?}"
            );
            let mut got = chunks.clone();
            got.sort();
            let mut want = f.chunk_ids.clone();
            want.sort();
            assert_eq!(got, want, "not all three live chunks were returned");
        }
        other => panic!("expected an answer, got {other:?}"),
    }
}

/// The other half of the same fix: telling a database error apart from a
/// chunk `knn_searchable`'s eligibility subquery is entitled to skip in
/// silence. `chunk`, not `page` — the eligibility subquery reads `chunk`
/// and `document` directly and never touches `page`, so this is the table
/// whose absence must reach here as `Failed` after the round-3 fix that
/// replaced the citation-lookup loop with that subquery.
#[test]
fn a_citation_lookup_that_fails_makes_the_arm_failed_not_merely_short() {
    let f = support::indexed_space();
    let mock = support::mock_returning_vector_near(&f, f.chunk_ids[0]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };
    f.db.conn()
        .execute_batch("ALTER TABLE chunk RENAME TO chunk_hidden_for_test;")
        .expect("rename the chunk table");

    match mnema_search::content_arm(&f.db, Some(provider), "ремонт даху", 3) {
        mnema_search::ContentArm::Failed { .. } => {}
        other => panic!("expected a failure, got {other:?}"),
    }
}

/// The property this pin protects moved with the fix: renaming `page` no
/// longer breaks the content arm at all, since neither `knn_searchable`'s
/// eligibility subquery nor the coverage counts touch it — the failure
/// that used to come from `citation` now belongs to `bridge.rs`'s own
/// citation loop, outside this crate. `Answered` here, not `Failed`, is
/// what proves the property moved rather than merely stopped firing.
#[test]
fn renaming_page_no_longer_breaks_the_content_arm() {
    let f = support::indexed_space();
    let mock = support::mock_returning_vector_near(&f, f.chunk_ids[0]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };
    f.db.conn()
        .execute_batch("ALTER TABLE page RENAME TO page_hidden_for_test;")
        .expect("rename the page table");

    match mnema_search::content_arm(&f.db, Some(provider), "ремонт даху", 3) {
        mnema_search::ContentArm::Answered { .. } => {}
        other => panic!("expected an answer, got {other:?}"),
    }
}

/// M6, final-round review: `Provider` derived `Debug`, which renders every
/// field including `key` — nothing formats a `Provider` with `{:?}` today,
/// but a struct with a bare credential field one derive away from a log line
/// is the shape `mnema-secrets`' own module doc warns against.
#[test]
fn a_providers_debug_rendering_does_not_carry_the_key() {
    let provider = Provider {
        base: "https://api.example.test".to_string(),
        key: "sk-do-not-print-me".to_string(),
    };
    let rendered = format!("{provider:?}");
    assert!(
        !rendered.contains("sk-do-not-print-me"),
        "the key leaked into Debug: {rendered}"
    );
    assert!(
        rendered.contains("[redacted]"),
        "the field should say it withheld something, not simply vanish: {rendered}"
    );
}

/// "Off", "cannot be asked", "asked and failed", and "asked and answered
/// with nothing" are separate facts, and a person shown one list for all of
/// them learns something false about their documents. The type is what
/// keeps them apart; this pins that every variant below is told apart from
/// every other.
#[test]
fn the_content_arms_silences_are_told_apart_by_type() {
    let states = [
        ContentArm::Off,
        ContentArm::NotConfigured(Missing::NoKey),
        ContentArm::NotConfigured(Missing::NoModel),
        ContentArm::Failed {
            reason: "no route to host".to_string(),
        },
        ContentArm::Answered {
            chunks: vec![],
            embedded: 0,
            total: 9,
            reachable: 9,
            inspected: 0,
        },
    ];
    // Each is distinct from every other, including each `NotConfigured`
    // payload and including an `Answered` that answered with nothing.
    for (i, a) in states.iter().enumerate() {
        for (j, b) in states.iter().enumerate() {
            assert_eq!(i == j, a == b, "{a:?} vs {b:?}");
        }
    }
}

/// An arm that answered nothing is not an arm that could not be asked, and the
/// coverage numbers ride on the answer rather than beside it.
#[test]
fn an_empty_answer_still_carries_its_coverage() {
    let arm = ContentArm::Answered {
        chunks: vec![],
        embedded: 3,
        total: 9,
        reachable: 6,
        inspected: 2,
    };
    match arm {
        ContentArm::Answered {
            chunks,
            embedded,
            total,
            reachable,
            inspected,
        } => {
            assert!(chunks.is_empty());
            assert_eq!((embedded, total, reachable, inspected), (3, 9, 6, 2));
        }
        other => panic!("expected an answer, got {other:?}"),
    }
}

/// The whole arm, end to end against a rude little HTTP server: a query
/// becomes a vector, the vector becomes a `knn` answer, and the answer
/// carries how much of the index it could even see.
#[test]
fn the_content_arm_turns_a_query_into_nearest_chunks() {
    let f = support::indexed_space();
    let mock = support::mock_returning_vector_near(&f, f.chunk_ids[1]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };

    let arm = mnema_search::content_arm(&f.db, Some(provider), "ремонт даху", 10);

    match arm {
        mnema_search::ContentArm::Answered {
            chunks,
            embedded,
            total,
            reachable,
            inspected,
        } => {
            assert_eq!(chunks.first(), Some(&f.chunk_ids[1]));
            let all = f.chunk_ids.len() as i64;
            // Every chunk here is both eligible and embedded, so `inspected`
            // — D115①'s honest pool — agrees with the other three marginals
            // rather than trailing them.
            assert_eq!(
                (embedded, total, reachable, inspected),
                (all, all, all, all)
            );
        }
        other => panic!("expected an answer, got {other:?}"),
    }
}

/// No key is not a failure and not an empty answer.
#[test]
fn no_key_is_not_configured_rather_than_failed() {
    let f = support::indexed_space();
    assert_eq!(
        mnema_search::content_arm(&f.db, None, "ремонт даху", 10),
        mnema_search::ContentArm::NotConfigured(mnema_search::Missing::NoKey)
    );
}

/// A half-filled space answers over its half and says so. Without the pair of
/// numbers a person reads "nothing found" as a fact about their documents.
#[test]
fn a_partly_embedded_space_says_how_much_of_the_index_it_saw() {
    let f = support::indexed_space_with_some_vectors_missing();
    let mock = support::mock_returning_vector_near(&f, f.embedded_ids[0]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };

    match mnema_search::content_arm(&f.db, Some(provider), "ремонт даху", 10) {
        mnema_search::ContentArm::Answered {
            embedded, total, ..
        } => {
            assert_eq!(embedded, f.embedded_ids.len() as i64);
            assert_eq!(total, f.chunk_ids.len() as i64);
            assert!(embedded < total, "the fixture must leave chunks unembedded");
        }
        other => panic!("expected an answer, got {other:?}"),
    }
}

/// Round-3 adversarial review, F-A1. Round 3's own fix widened the excluded
/// population from an orphan filter to a status filter, but `embedded`/
/// `total` still counted the whole `chunk` table — so a document mid-rebuild
/// (`Db::clear_document_content` leaves it `Pending`, D61) could hold
/// embedded chunks the arm can no longer reach while `embedded == total`
/// went on claiming full coverage. The exact shape measured: two embedded
/// chunks, one `Indexed`, one `Pending`, `embedded=2, total=2`.
///
/// `reachable` is the fix: [`mnema_index::Db::eligible_chunk_count`], the
/// same predicate the pre-filter itself runs, so `reachable < total` is now
/// what actually signals incomplete coverage — the `embedded == total`
/// branch alone cannot be trusted for that any more.
#[test]
fn a_pending_documents_embedded_chunks_are_not_claimed_as_coverage() {
    let f = support::indexed_space();
    let space =
        f.db.active_space()
            .expect("active space read")
            .expect("a space is active");

    let rebuilding_doc =
        f.db.insert_document(&"c".repeat(64), "text/plain", 64, SourceKind::Document)
            .expect("rebuilding document");
    let page =
        f.db.insert_page(&rebuilding_doc, 1, "native:txt", None)
            .expect("rebuilding page");
    let block =
        f.db.insert_block(
            page,
            &Block {
                block_type: BlockType::Paragraph,
                reading_order: 0,
                language: Some("uk".into()),
                text: "ремонт даху, чанк-перебудова".to_string(),
                line_start: None,
                line_end: None,
            },
        )
        .expect("rebuilding block");
    let rebuilding_chunk =
        f.db.insert_chunk(
            &rebuilding_doc,
            0,
            "ремонт даху, чанк-перебудова",
            &Locator {
                spans: vec![Segment {
                    block_id: block,
                    start: 0,
                    end: 28,
                    block_start: 0,
                }],
                coordinate: Coordinate::None,
            },
            SourceKind::Document,
        )
        .expect("rebuilding chunk");
    // Left at `insert_document`'s own default — `Pending`, exactly what
    // `Db::clear_document_content` leaves at the start of an ordinary
    // rebuild. No `set_document_status` call here is the point.
    let mut rebuilding_vector = vec![0.0f32; 1024];
    rebuilding_vector[5] = 1.0;
    f.db.upsert_vector(space, rebuilding_chunk, &rebuilding_vector)
        .expect("rebuilding vector");

    let row: Vec<String> = rebuilding_vector.iter().map(|v| v.to_string()).collect();
    let mock =
        mnema_mock_provider::MockServer::new(vec![mnema_mock_provider::Reply::ok(&format!(
            r#"{{"data":[{{"embedding":[{}],"index":0}}]}}"#,
            row.join(",")
        ))]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };

    match mnema_search::content_arm(&f.db, Some(provider), "ремонт даху", 10) {
        mnema_search::ContentArm::Answered {
            chunks,
            embedded,
            total,
            reachable,
            inspected,
        } => {
            assert!(
                !chunks.contains(&rebuilding_chunk),
                "a chunk behind a pending document must not reach the answer: {chunks:?}"
            );
            assert_eq!(
                (embedded, total),
                (4, 4),
                "the pending chunk's vector is genuinely embedded and its row genuinely exists"
            );
            assert_eq!(
                reachable, 3,
                "only the three chunks behind the indexed document are reachable"
            );
            assert!(
                reachable < total,
                "coverage must not read as complete while a document is mid-rebuild"
            );
            // D115①: the pending chunk's vector inflates `embedded` but must
            // not inflate `inspected` — the exact overstatement this field
            // exists to remove. Every reachable chunk here is also embedded,
            // so `inspected` agrees with `reachable`, not with `embedded`.
            assert_eq!(
                inspected, 3,
                "the pending chunk's vector must not count toward the inspected pool"
            );
            assert!(
                inspected < embedded,
                "inspected must be the honest pool, strictly below embedded here"
            );
        }
        other => panic!("expected an answer, got {other:?}"),
    }
}

/// The mirror case F-A1 asked for: an embedded vector with no `chunk` row
/// at all — `Db::insert_vector`'s own doc names this as the residual gap
/// [`Db::chunk_count`]'s doc still points to — must still report
/// `embedded > total`, the anomaly `render.js` already has a branch for.
/// `reachable`'s addition is a second, independent fact beside that one,
/// not a replacement for it.
#[test]
fn an_orphaned_vector_still_reports_embedded_above_total() {
    let f = support::indexed_space();
    let space =
        f.db.active_space()
            .expect("active space read")
            .expect("a space is active");

    // No document, no page, no chunk row — a bare write against the vector
    // table, the same shape `Db::insert_vector`'s own doc calls out.
    let mut orphan_vector = vec![0.0f32; 1024];
    orphan_vector[9] = 1.0;
    f.db.insert_vector(space, 999_999, &orphan_vector)
        .expect("orphan vector");

    let row: Vec<String> = orphan_vector.iter().map(|v| v.to_string()).collect();
    let mock =
        mnema_mock_provider::MockServer::new(vec![mnema_mock_provider::Reply::ok(&format!(
            r#"{{"data":[{{"embedding":[{}],"index":0}}]}}"#,
            row.join(",")
        ))]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };

    match mnema_search::content_arm(&f.db, Some(provider), "ремонт даху", 10) {
        mnema_search::ContentArm::Answered {
            chunks,
            embedded,
            total,
            reachable,
            inspected,
        } => {
            assert!(
                !chunks.contains(&999_999),
                "the orphaned id must not reach the answer: {chunks:?}"
            );
            assert_eq!(
                embedded, 4,
                "the orphan vector is still counted as embedded"
            );
            assert_eq!(
                total, 3,
                "no chunk row backs the orphan, so it is not in chunk_count"
            );
            assert_eq!(
                reachable, 3,
                "the three real chunks are still all reachable"
            );
            assert!(
                embedded > total,
                "the pre-existing anomaly must stay visible after this fix"
            );
            // D115①: `ELIGIBLE_CHUNK` joins through the `chunk` table, so an
            // id with no chunk row cannot match it — the orphan is excluded
            // from `inspected` the same way it is already excluded from
            // `total`, and `inspected` agrees with `reachable` here.
            assert_eq!(
                inspected, 3,
                "the orphan vector must not count toward the inspected pool either"
            );
        }
        other => panic!("expected an answer, got {other:?}"),
    }
}

/// A count that cannot be read is not a zero. `0 of 0` is a claim about the
/// index; a failed count is a claim about this build.
#[test]
fn a_coverage_count_that_fails_makes_the_arm_failed_not_empty() {
    let f = support::indexed_space();
    let mock = support::mock_returning_vector_near(&f, f.chunk_ids[0]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };
    f.break_chunk_count();

    match mnema_search::content_arm(&f.db, Some(provider), "ремонт даху", 10) {
        mnema_search::ContentArm::Failed { .. } => {}
        other => panic!("expected a failure, got {other:?}"),
    }
}

/// The two counts fail on separate tables, and `chunk_count` failing is not
/// the only path in: `embedded_chunk_count`'s own table must be caught too.
#[test]
fn an_unreadable_embedded_count_also_makes_the_arm_failed_not_empty() {
    let f = support::indexed_space();
    let mock = support::mock_returning_vector_near(&f, f.chunk_ids[0]);
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };
    f.break_embedded_count();

    match mnema_search::content_arm(&f.db, Some(provider), "ремонт даху", 10) {
        mnema_search::ContentArm::Failed { .. } => {}
        other => panic!("expected a failure, got {other:?}"),
    }
}

/// The discriminating case for "the model comes from the space": the
/// database also knows a decoy model, on record but never embedded into,
/// and the request must still carry the real space's — not the decoy's. A
/// test with only one model on record would pass no matter which one
/// `content_arm` asked for.
#[test]
fn the_content_arm_refuses_a_model_that_is_not_the_spaces() {
    let f = support::indexed_space_with_a_decoy_model();
    let mock = support::mock_recording_requests();
    let provider = mnema_search::Provider {
        base: mock.base().to_string(),
        key: "k".to_string(),
    };

    let _ = mnema_search::content_arm(&f.db, Some(provider), "ремонт даху", 10);

    let body = mock.request_if_any().expect("the arm must have asked");
    assert!(
        body.contains(&f.space_model),
        "asked for the wrong model: {body}"
    );
    assert!(
        !body.contains(support::DECOY_MODEL),
        "asked for the decoy model instead: {body}"
    );
}

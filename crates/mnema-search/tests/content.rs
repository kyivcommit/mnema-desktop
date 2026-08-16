use mnema_search::{ContentArm, Missing, Provider};

mod support;

/// Codex review on PR #10: `Db::knn` answers straight from the vector
/// table, with no join back to `chunk` — a vector can outlive the chunk it
/// embeds (`crates/mnema-index/src/space.rs:539`'s own doc says so, for
/// `delete_document`). A neighbour ranked ahead of a live chunk but with no
/// row left in `chunk` must not appear in the answer, and must not push a
/// live chunk out of it either.
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

    // The chunk row goes; `vec0` cannot hold a foreign key to it (G7.0
    // §5.7), so the vector survives it — an orphan ranked first, since the
    // query below asks for exactly this vector.
    f.db.delete_document(&victim_doc)
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

/// The other half of the same fix: telling a gone chunk apart from a
/// citation lookup that could not answer at all. A database error there is a
/// defect in this build, not a chunk that no longer exists, and swallowing
/// it into "skip this id" would silently shrink the answer for a reason
/// nobody could see.
#[test]
fn a_citation_lookup_that_fails_makes_the_arm_failed_not_merely_short() {
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
        mnema_search::ContentArm::Failed { .. } => {}
        other => panic!("expected a failure, got {other:?}"),
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
    };
    match arm {
        ContentArm::Answered {
            chunks,
            embedded,
            total,
        } => {
            assert!(chunks.is_empty());
            assert_eq!((embedded, total), (3, 9));
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
        } => {
            assert_eq!(chunks.first(), Some(&f.chunk_ids[1]));
            assert_eq!(
                (embedded, total),
                (f.chunk_ids.len() as i64, f.chunk_ids.len() as i64)
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

//! Розбір списку моделей провайдера. Фікстури — справжні відповіді від
//! 2026-08-08; команди, якими їх знято, у Task 1 плану.
//!
//! **The two are not the same kind of artefact, and the difference is now
//! written in each file.** `rerank-2026-08-08.json` is the whole answer: six
//! records and `total_count: 6`. `embeddings-2026-08-08.json` is an excerpt —
//! six of the 33 records that answer carried, kept for the rules they exercise
//! — and its `total_count: 33` is the provider's own number for the full
//! answer, not this file's. Read as a total of what the file holds it is the
//! "count from a limited query" trap this project has already paid for, so the
//! file says which it is in a `_mnema_note` key of ours that nothing reads
//! (whole-branch review, M1). The 33 is not rewritten to 6: it is the measured
//! figure `MIN_CONTEXT_TOKENS`' own doc cites as "12 of the 33".
//!
//! ⚠️ **Every number in the paragraph above is checked**, by
//! [`each_fixture_says_what_it_is_and_its_own_numbers_agree`]. Writing them here
//! and in `_mnema_note` was the fix for a count trap producing two more counts,
//! in prose, held by nothing: a record added or dropped for some later rule
//! would leave both stale in silence, which is the exact shape of the finding
//! they were written to close (whole-branch review, closing check).

use mnema_provider::{
    Error, InputLimit, MIN_CONTEXT_TOKENS, ModelEntry, Price, RecordId, Refusal, Role,
    UnreadableRecord, models_from_json,
};

const EMBEDDINGS: &str = include_str!("fixtures/embeddings-2026-08-08.json");
const RERANK: &str = include_str!("fixtures/rerank-2026-08-08.json");

fn find<'a>(entries: &'a [ModelEntry], id: &str) -> &'a ModelEntry {
    entries
        .iter()
        .find(|e| e.id == id)
        .unwrap_or_else(|| panic!("{id} is missing from the parsed list"))
}

#[test]
fn the_default_model_survives_every_rule_and_keeps_its_price() {
    let catalogue = models_from_json(Role::Embedding, EMBEDDINGS).expect("parses");
    let bge = find(&catalogue.entries, "baai/bge-m3");
    assert_eq!(bge.refusal, None, "the default choice must be selectable");
    assert_eq!(bge.input_limit, InputLimit::Known { tokens: 8194 });
    assert_eq!(bge.price, Price::Known { amount: 0.00000001 });
}

#[test]
fn a_model_that_takes_512_tokens_is_refused_and_says_both_numbers() {
    let catalogue = models_from_json(Role::Embedding, EMBEDDINGS).expect("parses");
    let small = find(&catalogue.entries, "thenlper/gte-base");
    assert_eq!(
        small.refusal,
        Some(Refusal::InputTooSmall {
            limit: 512,
            floor: MIN_CONTEXT_TOKENS
        }),
        "a refusal that does not name the floor cannot be explained to anyone"
    );
}

#[test]
fn a_refused_model_is_still_returned_with_its_refusal_rather_than_hidden_or_laundered() {
    let catalogue = models_from_json(Role::Embedding, EMBEDDINGS).expect("parses");

    // Both directions matter: an implementation that always returns `None` for
    // `refusal` would still pass a presence-only check (review round 1, F1).
    let refused = find(&catalogue.entries, "intfloat/multilingual-e5-large");
    assert_eq!(
        refused.refusal,
        Some(Refusal::InputTooSmall {
            limit: 512,
            floor: MIN_CONTEXT_TOKENS
        }),
        "hiding a model the provider lists sends the user looking for our bug, and showing \
         it with its refusal silently dropped is the same fault wearing a different shape"
    );

    let usable = find(&catalogue.entries, "baai/bge-m3");
    assert_eq!(
        usable.refusal, None,
        "a model that survives every rule must come back with no refusal at all"
    );
}

#[test]
fn the_rerank_list_parses_and_the_input_floor_does_not_apply_to_it() {
    let catalogue = models_from_json(Role::Rerank, RERANK).expect("parses");
    assert_eq!(catalogue.unreadable, 0);
    assert_eq!(
        catalogue.entries.len(),
        6,
        "the fixture holds six, so six must come back"
    );
    assert!(
        catalogue.entries.iter().all(|e| e.refusal.is_none()),
        "MIN_CONTEXT_TOKENS is an embedding-only floor (spec §6.4); rerank has no input \
         it must hold a whole chunk in, so nothing here should be refused for lacking one"
    );

    // None of the six real records states a limit under MIN_CONTEXT_TOKENS, so
    // the assertion above would stay green even if the floor were wrongly
    // applied to rerank too. A synthetic record that WOULD be refused under
    // the embedding rule is what actually proves the rule does not reach here.
    let json = r#"{"data":[{"id":"vendor/small-context-rerank","name":"Small",
        "context_length":100,"architecture":{"output_modalities":["rerank"]}}]}"#;
    let small = models_from_json(Role::Rerank, json).expect("parses");
    assert_eq!(
        find(&small.entries, "vendor/small-context-rerank").refusal,
        None,
        "100 tokens is well under MIN_CONTEXT_TOKENS and would refuse an embedding model, \
         but rerank does not hold a whole chunk in its input, so no floor applies to it"
    );
}

#[test]
fn a_chat_model_that_does_not_write_text_is_refused() {
    let json = r#"{"data":[
        {"id":"vendor/speaks-only","name":"Speaks","context_length":8192,
         "pricing":{"prompt":"0.000001"},
         "architecture":{"input_modalities":["text"],"output_modalities":["audio"]}},
        {"id":"vendor/writes","name":"Writes","context_length":8192,
         "pricing":{"prompt":"0.000001"},
         "architecture":{"input_modalities":["text"],"output_modalities":["text","image"]}}
    ]}"#;
    let catalogue = models_from_json(Role::Chat, json).expect("parses");
    assert_eq!(
        find(&catalogue.entries, "vendor/speaks-only").refusal,
        Some(Refusal::NoTextOutput)
    );
    assert_eq!(find(&catalogue.entries, "vendor/writes").refusal, None);
}

#[test]
fn a_chat_model_with_no_stated_architecture_is_refused_for_not_saying_not_for_not_writing_text() {
    // No `architecture` field at all — as if the provider renamed or dropped
    // it. This must not read as "text is absent", a fact the provider never
    // stated (review round 1, F3).
    let json = r#"{"data":[
        {"id":"vendor/unstated","name":"Unstated","context_length":8192,
         "pricing":{"prompt":"0.000001"}}
    ]}"#;
    let catalogue = models_from_json(Role::Chat, json).expect("parses");
    assert_eq!(
        find(&catalogue.entries, "vendor/unstated").refusal,
        Some(Refusal::NoStatedOutputModalities),
        "a provider that never mentioned architecture never said text was absent either"
    );
}

#[test]
fn a_stated_limit_this_build_cannot_read_is_not_the_same_as_no_limit_at_all() {
    // 8192.0 is a JSON number with a fraction; serde_json stores that as an
    // f64, and Number::as_i64 refuses it even though 8192.0 is a whole number.
    // Before this fix (review round 2, N1) that shape fell into the same
    // `None` an absent field produces, so a limit the provider DID state
    // greyed out with "no limit stated" — a sentence false about the
    // provider.
    //
    // The assertion checks the variant and that `raw` names the number, not
    // the exact string serde_json renders a fraction as: that rendering is
    // serde_json's choice, not this crate's, and pinning it would turn a
    // routine `cargo update` into a false red (review round 3, I4).
    let json = r#"{"data":[{"id":"vendor/fractional-limit","name":"Fractional",
        "context_length":8192.0,"architecture":{"output_modalities":["embeddings"]}}]}"#;
    let catalogue = models_from_json(Role::Embedding, json).expect("parses");
    match &find(&catalogue.entries, "vendor/fractional-limit").refusal {
        Some(Refusal::LimitNotUnderstood { raw }) => {
            assert!(
                raw.contains("8192"),
                "the raw text should name the number that confused this build, got {raw:?}"
            );
        }
        other => panic!(
            "a limit that was stated, just not in a shape this build parses, must not read \
             as though nothing was stated at all: got {other:?}"
        ),
    }
}

#[test]
fn a_stated_limit_as_a_non_numeric_string_is_reported_without_its_json_quotes() {
    // The string arm of `Stated::deserialize` was not exercised anywhere else
    // under `Role::Embedding` — closing that hole (review round 3, I5).
    // `value.to_string()` on a JSON string re-serializes it WITH its quotes
    // ("8k" -> `"8k"`), which would make the reason look different depending
    // on whether the provider stated a number or a string; unlike the
    // fraction above, a plain string's own rendering is fully this crate's
    // choice, so this one IS pinned exactly.
    let json = r#"{"data":[{"id":"vendor/unit-suffixed-limit","name":"Suffixed",
        "context_length":"8k","architecture":{"output_modalities":["embeddings"]}}]}"#;
    let catalogue = models_from_json(Role::Embedding, json).expect("parses");
    assert_eq!(
        find(&catalogue.entries, "vendor/unit-suffixed-limit").refusal,
        Some(Refusal::LimitNotUnderstood {
            raw: "8k".to_string()
        }),
        "a non-integer string must be reported without its JSON quotes"
    );
}

#[test]
fn a_very_long_unreadable_value_is_capped_before_it_becomes_a_label() {
    // Provider text is untrusted and unbounded; a 200-byte (or 200 KB)
    // context_length must not become a label of that size in a model picker
    // (review round 3, I2).
    let long = "9".repeat(200);
    let json = format!(
        r#"{{"data":[{{"id":"vendor/huge-unreadable-limit","name":"Huge",
        "context_length":"{long}","architecture":{{"output_modalities":["embeddings"]}}}}]}}"#
    );
    let catalogue = models_from_json(Role::Embedding, &json).expect("parses");
    match &find(&catalogue.entries, "vendor/huge-unreadable-limit").refusal {
        Some(Refusal::LimitNotUnderstood { raw }) => {
            assert!(
                raw.len() <= 64,
                "a value this build cannot read must be capped before it is stored anywhere, \
                 got {} bytes",
                raw.len()
            );
        }
        other => panic!("expected LimitNotUnderstood, got {other:?}"),
    }
}

#[test]
fn an_unreadable_sibling_refuses_even_next_to_a_readable_number() {
    // The direction review round 1's F5 test never exercised: with one side
    // unreadable, `combined_limit` cannot keep its promise ("the narrower of
    // what the provider stated"), so it must not fall back to the more
    // permissive number just because the other side happened to parse
    // (review round 3, I1). Accepting the readable 32000 here would silently
    // let a chunk through to a model whose real limit — stated, just not in a
    // shape this build reads — might be far smaller.
    let json = r#"{"data":[
        {"id":"vendor/optimistic-with-unreadable-sibling","name":"Optimistic",
         "context_length":32000,"top_provider":{"context_length":4000.0},
         "architecture":{"output_modalities":["embeddings"]}}
    ]}"#;
    let catalogue = models_from_json(Role::Embedding, json).expect("parses");
    assert!(
        matches!(
            find(
                &catalogue.entries,
                "vendor/optimistic-with-unreadable-sibling"
            )
            .refusal,
            Some(Refusal::LimitNotUnderstood { .. })
        ),
        "an unreadable top_provider.context_length must refuse the record, not be silently \
         outvoted by the larger, readable context_length"
    );
}

#[test]
fn an_absent_sibling_still_lets_the_readable_number_through() {
    // The other direction of I1: `Unreadable` must trigger narrowly, on a
    // field that is genuinely unreadable — not merely absent, which is the
    // ordinary shape of every record with no `top_provider` block at all.
    // Otherwise this rule would quietly become "refuse anything without a
    // top_provider block" (review round 3, I1).
    let json = r#"{"data":[
        {"id":"vendor/no-top-provider-at-all","name":"Plain","context_length":32000,
         "architecture":{"output_modalities":["embeddings"]}}
    ]}"#;
    let catalogue = models_from_json(Role::Embedding, json).expect("parses");
    let entry = find(&catalogue.entries, "vendor/no-top-provider-at-all");
    assert_eq!(
        entry.refusal, None,
        "an absent top_provider must not refuse anything"
    );
    assert_eq!(
        entry.input_limit,
        InputLimit::Known { tokens: 32000 },
        "with no top_provider stated at all, the one readable number must still be used"
    );
}

#[test]
fn an_architecture_present_with_no_output_modalities_field_is_not_stated_either() {
    // The line "did the provider say?" belongs to `output_modalities` itself,
    // not to its container. A provider that states `architecture` but renames
    // or drops `output_modalities` must read the same as one that never
    // mentioned `architecture` at all (review round 2, N2) — not as "said,
    // and text was not among it", which `#[serde(default)]` on a bare `Vec`
    // used to produce here.
    let json = r#"{"data":[{"id":"vendor/half-stated","name":"Half",
        "architecture":{"modality":"text->text"}}]}"#;
    let catalogue = models_from_json(Role::Chat, json).expect("parses");
    assert_eq!(
        find(&catalogue.entries, "vendor/half-stated").refusal,
        Some(Refusal::NoStatedOutputModalities),
        "architecture was stated, but output_modalities inside it was not — that must read \
         as unstated, not as stated-and-empty"
    );
}

#[test]
fn an_explicit_null_output_modalities_is_unstated_rather_than_unreadable() {
    // Closes a Minor the same fix uncovers: with output_modalities typed as a
    // bare `Vec<String>`, an explicit JSON null failed to deserialize into it
    // at all and took the whole record down to `unreadable`. `Option<Vec<_>>`
    // reads null as None for free.
    let json = r#"{"data":[{"id":"vendor/nulled","name":"Nulled",
        "architecture":{"output_modalities":null}}]}"#;
    let catalogue = models_from_json(Role::Chat, json).expect("parses");
    assert_eq!(
        catalogue.unreadable, 0,
        "a null output_modalities must not cost the record"
    );
    assert_eq!(
        find(&catalogue.entries, "vendor/nulled").refusal,
        Some(Refusal::NoStatedOutputModalities),
        "an explicit null says the same as an absent field: nothing was stated"
    );
}

#[test]
fn an_unknown_field_is_ignored_and_a_missing_price_is_not_an_error() {
    let json = r#"{"data":[
        {"id":"vendor/new","name":"New","context_length":32000,
         "something_added_next_month":{"deep":[1,2,3]},
         "architecture":{"output_modalities":["text"]}}
    ]}"#;
    let catalogue = models_from_json(Role::Embedding, json).expect("parses");
    assert_eq!(
        catalogue.unreadable, 0,
        "an unknown field must not make the record unreadable"
    );
    assert_eq!(catalogue.entries[0].price, Price::NotStated);
    assert_eq!(catalogue.entries[0].refusal, None);
}

#[test]
fn a_model_with_no_stated_limit_is_refused_rather_than_assumed_generous() {
    let json = r#"{"data":[{"id":"vendor/silent","name":"Silent",
        "architecture":{"output_modalities":["text"]}}]}"#;
    let catalogue = models_from_json(Role::Embedding, json).expect("parses");
    assert_eq!(catalogue.entries[0].refusal, Some(Refusal::NoStatedLimit));
}

#[test]
fn a_record_with_no_usable_id_is_counted_as_unreadable_rather_than_dropped_silently() {
    let json = r#"{"data":[
        {"id":"vendor/first","name":"First","context_length":32000,
         "architecture":{"output_modalities":["text"]}},
        {"id":"vendor/second","name":"Second","context_length":{},
         "architecture":{"output_modalities":["text"]}},
        {"name":"No id at all","context_length":32000,
         "architecture":{"output_modalities":["text"]}}
    ]}"#;
    let catalogue = models_from_json(Role::Chat, json).expect("parses");
    assert_eq!(
        catalogue.entries.len(),
        2,
        "two of the three records have a usable id, so two entries must come back"
    );
    assert_eq!(
        catalogue.unreadable, 1,
        "the record with no id cannot be shown as a model, but it must be counted rather \
         than vanish without a trace"
    );
    assert_eq!(
        find(&catalogue.entries, "vendor/second").input_limit,
        InputLimit::NotUnderstood {
            raw: "{}".to_string()
        },
        "an odd-shaped context_length must only cost that field, not the whole record — and \
         it must cost it as 'stated, and unreadable' rather than as silence"
    );
    assert_eq!(
        catalogue.unreadable_records,
        vec![UnreadableRecord {
            id: RecordId::Absent,
            index: 2
        }],
        "the one record that could not be read at all has no id, so its position is the only \
         identity left to keep"
    );
}

#[test]
fn an_unreadable_record_that_still_states_an_id_is_identified_by_it_not_only_by_position() {
    // Task 2 review, item 4, the other direction: `pricing` here is a string,
    // not the object `Raw::pricing` requires, so the whole record fails to
    // deserialize — but its `id` was read straight off the raw JSON before
    // that failure, so a genuinely readable id must not be thrown away along
    // with the rest of a record that happened to be broken elsewhere.
    let json = r#"{"data":[
        {"id":"vendor/bad-shape","name":"Bad","pricing":"not an object",
         "architecture":{"output_modalities":["text"]}}
    ]}"#;
    let catalogue = models_from_json(Role::Chat, json).expect("parses");
    assert_eq!(
        catalogue.entries.len(),
        0,
        "the record could not be turned into a model"
    );
    assert_eq!(catalogue.unreadable, 1);
    assert_eq!(
        catalogue.unreadable_records,
        vec![UnreadableRecord {
            id: RecordId::Known {
                id: "vendor/bad-shape".to_string()
            },
            index: 0
        }],
        "the id survives even though the record as a whole did not parse"
    );
}

#[test]
fn an_unreadable_record_whose_id_is_present_but_not_a_string_is_told_apart_from_no_id_at_all() {
    // Task 2 review round 1, F4: `{"id":12345}` is not the same fact as "no
    // id key at all". The provider did name something for this record, just
    // not in the one shape `Raw::id: String` accepts — folding it into
    // `RecordId::Absent` would report "record 0 stated no id", which is false
    // about the provider, the same fold review round 2's N1 refused one field
    // over for `context_length`.
    let json = r#"{"data":[
        {"id":12345,"name":"Numeric id","architecture":{"output_modalities":["text"]}}
    ]}"#;
    let catalogue = models_from_json(Role::Chat, json).expect("parses");
    assert_eq!(
        catalogue.entries.len(),
        0,
        "an id that is not a string cannot name a model"
    );
    assert_eq!(catalogue.unreadable, 1);
    match &catalogue.unreadable_records[0].id {
        RecordId::NotAString { raw } => {
            assert_eq!(
                raw, "12345",
                "the id that confused this build should still be nameable"
            )
        }
        other => panic!("expected NotAString, got {other:?}"),
    }
}

#[test]
fn an_html_error_page_a_json_error_envelope_and_a_truncated_body_are_told_apart() {
    // Task 2 review, item 2: three shapes reach `models_from_json` through the
    // network, and each is a different problem for the user. An HTML error
    // page is a proxy or captive portal answering instead of the provider; a
    // JSON error envelope is the provider itself, just not with a model list;
    // a truncated body is a connection that ended before the JSON closed.
    // Before this fix all three fell into the same one sentence, discarding
    // exactly the distinction a bug report needs.
    let html = malformed_reason("<html><body>502 Bad Gateway</body></html>");
    let envelope = malformed_reason(r#"{"error":{"message":"invalid api key"}}"#);
    let truncated = malformed_reason(r#"{"data":[{"id":"vendor/x""#);

    assert_ne!(
        html, envelope,
        "a proxy's HTML page is not the provider's own error envelope"
    );
    assert_ne!(
        html, truncated,
        "a proxy's HTML page is not a body that stopped mid-transfer"
    );
    assert_ne!(
        envelope, truncated,
        "the provider's own error envelope is not a body that stopped mid-transfer"
    );
}

fn malformed_reason(body: &str) -> String {
    match models_from_json(Role::Chat, body) {
        Err(Error::Malformed(reason)) => reason.to_string(),
        other => panic!("expected Malformed, got {other:?}"),
    }
}

#[test]
fn the_narrower_of_the_two_stated_limits_is_the_one_that_counts() {
    // context_length and top_provider.context_length disagree; the smaller is
    // the one that would actually truncate a chunk (review round 1, F5).
    let json = r#"{"data":[
        {"id":"vendor/optimistic-headline","name":"Optimistic","context_length":32000,
         "top_provider":{"context_length":4000},
         "architecture":{"output_modalities":["embeddings"]}}
    ]}"#;
    let catalogue = models_from_json(Role::Embedding, json).expect("parses");
    assert_eq!(
        find(&catalogue.entries, "vendor/optimistic-headline").input_limit,
        InputLimit::Known { tokens: 4000 },
        "the smaller, more pessimistic number must win over the larger headline one"
    );
}

/// What the provider said about the input limit reaches the window for every
/// role, and not only for the one role that refuses over it.
///
/// The fact used to die on the wire for rerank and chat: the limit refusals are
/// `Role::Embedding`'s, so "the provider stated no limit" and "the provider
/// stated one this build cannot read" both arrived as `context_length: None`
/// with no refusal beside them — the same question mark for opposite statements
/// about the provider (I4, deferred by Task 10, on the screen at the first real
/// run).
///
/// **Every role, and `Embedding` is not a formality here** (review round 1,
/// Minor-1). Its two unknown states were held by the refusal alone — no
/// assertion looked at the field — so a build that answered `NotStated` for
/// this role on the grounds that "the refusal says it anyway" stayed green and
/// produced a label whose halves contradict each other: `input limit not stated
/// — unavailable: the provider stated an input limit in a shape this build does
/// not understand ("8k")`. The mutation case beside this test breaks the
/// opposite direction and cannot see that one.
///
/// Both facts under every role, because a build that answered `NotStated` to
/// everything would satisfy half of this, and one that applied the embedding
/// refusals to rerank would satisfy the other half while greying out models
/// that work.
#[test]
fn a_limit_stated_unreadably_is_told_apart_from_no_limit_for_every_role() {
    let unreadable = r#"{"data":[{"id":"vendor/unreadable-limit","name":"Unreadable",
        "context_length":"8k","architecture":{"output_modalities":["text"]}}]}"#;
    let silent = r#"{"data":[{"id":"vendor/silent-limit","name":"Silent",
        "architecture":{"output_modalities":["text"]}}]}"#;

    for role in [Role::Rerank, Role::Chat, Role::Embedding] {
        // The refusal is the embedding role's alone, and it is asserted beside
        // the field rather than instead of it: the point of this test is that
        // the two carry the fact separately.
        let (unreadable_refusal, silent_refusal) = match role {
            Role::Embedding => (
                Some(Refusal::LimitNotUnderstood {
                    raw: "8k".to_string(),
                }),
                Some(Refusal::NoStatedLimit),
            ),
            Role::Rerank | Role::Chat => (None, None),
        };

        let stated = models_from_json(role, unreadable).expect("parses");
        let entry = find(&stated.entries, "vendor/unreadable-limit");
        assert_eq!(
            entry.input_limit,
            InputLimit::NotUnderstood {
                raw: "8k".to_string()
            },
            "{role:?}: a limit the provider DID state must not reach the window as silence"
        );
        assert_eq!(
            entry.refusal, unreadable_refusal,
            "{role:?}: the floor is an embedding rule, and only there"
        );

        let none = models_from_json(role, silent).expect("parses");
        let entry = find(&none.entries, "vendor/silent-limit");
        assert_eq!(
            entry.input_limit,
            InputLimit::NotStated,
            "{role:?}: nothing stated must not read as something stated unreadably"
        );
        assert_eq!(
            entry.refusal, silent_refusal,
            "{role:?}: the floor is an embedding rule, and only there"
        );
    }
}

/// The price arrives in more shapes than "a number or nothing", and the two
/// that are numbers are the pair `Option<f64>` could not keep apart.
///
/// Every case is one the live list produces. The negative is the one that was
/// on the screen: `-1` multiplied by a million and printed as
/// `$-1000000.000 per million tokens` (the acceptance run, item 2).
#[test]
fn the_states_a_price_arrives_in_are_not_folded_into_one_another() {
    let json = r#"{"data":[
        {"id":"vendor/priced","pricing":{"prompt":"0.000000015"},
         "architecture":{"output_modalities":["text"]}},
        {"id":"vendor/zero","pricing":{"prompt":"0"},
         "architecture":{"output_modalities":["text"]}},
        {"id":"vendor/sentinel","pricing":{"prompt":"-1"},
         "architecture":{"output_modalities":["text"]}},
        {"id":"vendor/worded","pricing":{"prompt":"free"},
         "architecture":{"output_modalities":["text"]}},
        {"id":"vendor/structured","pricing":{"prompt":{"per_request":"0.01"}},
         "architecture":{"output_modalities":["text"]}},
        {"id":"vendor/nulled","pricing":{"prompt":null},
         "architecture":{"output_modalities":["text"]}},
        {"id":"vendor/silent","pricing":{"completion":"0"},
         "architecture":{"output_modalities":["text"]}}
    ]}"#;
    let catalogue = models_from_json(Role::Chat, json).expect("parses");
    assert_eq!(
        catalogue.unreadable, 0,
        "no price this build cannot read may cost the whole record"
    );

    for (id, expected) in [
        (
            "vendor/priced",
            Price::Known {
                amount: 0.000000015,
            },
        ),
        // A stated zero is a number the provider sent, and stays one. What it
        // may be rendered as — never "free" — is the window's question, and no
        // window asks it yet: the settings surfaces are PR 7's.
        ("vendor/zero", Price::Known { amount: 0.0 }),
        (
            "vendor/sentinel",
            Price::NotAPrice {
                raw: "-1".to_string(),
            },
        ),
        (
            "vendor/worded",
            Price::Unreadable {
                raw: "free".to_string(),
            },
        ),
        (
            "vendor/structured",
            Price::Unreadable {
                raw: r#"{"per_request":"0.01"}"#.to_string(),
            },
        ),
        ("vendor/nulled", Price::NotStated),
        ("vendor/silent", Price::NotStated),
    ] {
        assert_eq!(
            find(&catalogue.entries, id).price,
            expected,
            "{id} was sorted into the wrong thing to tell a person"
        );
    }
}

/// A price stated as a string `f64::from_str` accepts but arithmetic cannot use.
///
/// `"NaN"` and `"inf"` are not JSON numbers and cannot arrive as one, but this
/// crate reads a price stated as a string — every price in the live list is one
/// — and `parse::<f64>()` accepts both. Before `Price`, either became
/// `Some(f64::NAN)` and the window rendered `$NaN per million tokens`.
#[test]
fn a_price_that_is_not_a_finite_number_is_not_a_price() {
    for stated in ["NaN", "inf", "-inf"] {
        let json = format!(
            r#"{{"data":[{{"id":"vendor/x","pricing":{{"prompt":"{stated}"}},
            "architecture":{{"output_modalities":["text"]}}}}]}}"#
        );
        let catalogue = models_from_json(Role::Chat, &json).expect("parses");
        assert_eq!(
            catalogue.entries[0].price,
            Price::NotAPrice {
                raw: stated.to_string()
            },
            "{stated} parses as an f64 and is not an amount of money"
        );
    }
}

/// Provider text on a price is capped exactly as it is on an input limit.
///
/// The same rule, and it needs its own witness: the cap on the limit is applied
/// by `Stated`'s own deserializer, and nothing there reaches this field.
#[test]
fn an_unreadable_price_is_capped_before_it_becomes_a_label() {
    let long = "x".repeat(200);
    let json = format!(
        r#"{{"data":[{{"id":"vendor/huge-unreadable-price","pricing":{{"prompt":"{long}"}},
        "architecture":{{"output_modalities":["text"]}}}}]}}"#
    );
    let catalogue = models_from_json(Role::Chat, &json).expect("parses");
    match &find(&catalogue.entries, "vendor/huge-unreadable-price").price {
        Price::Unreadable { raw } => assert!(
            raw.len() <= 64,
            "a price this build cannot read must be capped before it is stored anywhere, \
             got {} bytes",
            raw.len()
        ),
        other => panic!("expected an unreadable price, got {other:?}"),
    }
}

/// Every rerank model the provider lists states a price of zero, and none of
/// them is free.
///
/// The fixture is the live answer of 2026-08-08. This pins the fact the
/// window's own sentence is built on: rerank is billed per search, the payload
/// says nothing about that, and a screen reading `"prompt": "0"` as "this
/// costs nothing" would be telling six models' worth of people they will not be
/// charged.
#[test]
fn every_rerank_model_the_provider_lists_states_a_price_of_zero() {
    let catalogue = models_from_json(Role::Rerank, RERANK).expect("parses");
    assert_eq!(catalogue.entries.len(), 6, "the fixture holds six");
    for entry in &catalogue.entries {
        assert_eq!(
            entry.price,
            Price::Known { amount: 0.0 },
            "{} no longer states zero, so the sentence built on this measurement is stale",
            entry.id
        );
    }
}

/// What each fixture claims to be, checked against what it holds.
///
/// The prose this pins lives in three places and none of them could go red:
/// this module's own header, the `_mnema_note` key inside the excerpt, and
/// `MIN_CONTEXT_TOKENS`' "12 of the 33". Adding or dropping a record for some
/// later rule would leave every one of them stale in silence — the count trap
/// this project has paid for repeatedly, produced by the fix for one instance
/// of it (whole-branch review, closing check).
///
/// **The relationship is asserted as well as the numbers**, and that is the
/// half a pair of literals does not give: `records == total_count` is what
/// "the whole answer" means and `records < total_count` is what "an excerpt"
/// means, so a fixture cannot quietly change kind while both of its numbers
/// still look plausible.
#[test]
fn each_fixture_says_what_it_is_and_its_own_numbers_agree() {
    for (name, body, records, total_count, whole_answer) in [
        ("rerank-2026-08-08.json", RERANK, 6, 6, true),
        ("embeddings-2026-08-08.json", EMBEDDINGS, 6, 33, false),
    ] {
        let value: serde_json::Value = serde_json::from_str(body).expect("the fixture is JSON");
        let held = value["data"].as_array().expect("a data list").len();
        let stated = value["total_count"].as_u64().expect("a stated total") as usize;

        assert_eq!(
            held, records,
            "{name} holds {held} records; this module's header and, for the excerpt, its own \
             `_mnema_note` both name the count in prose — update them and this line together"
        );
        assert_eq!(stated, total_count, "{name}'s `total_count` changed");
        assert_eq!(
            held == stated,
            whole_answer,
            "{name} has changed which kind of artefact it is: `records == total_count` is the \
             whole answer, `records < total_count` is an excerpt of one"
        );

        // The note is what tells the person who opens the file, and it belongs
        // to exactly one of the two. On the complete fixture it would be a
        // sentence contradicting the numbers beside it.
        assert_eq!(
            value.get("_mnema_note").is_some(),
            !whole_answer,
            "{name}: an excerpt must say so in the file itself, and a complete answer must not \
             claim to be one"
        );
    }
}

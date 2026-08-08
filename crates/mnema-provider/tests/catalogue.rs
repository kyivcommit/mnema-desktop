//! Розбір списку моделей провайдера. Фікстури — справжні відповіді від
//! 2026-08-08; команди, якими їх знято, у Task 1 плану.

use mnema_provider::{MIN_CONTEXT_TOKENS, ModelEntry, Refusal, Role, models_from_json};

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
    assert_eq!(bge.context_length, Some(8194));
    assert_eq!(bge.price_per_token, Some(0.00000001));
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
    let json = r#"{"data":[{"id":"vendor/fractional-limit","name":"Fractional",
        "context_length":8192.0,"architecture":{"output_modalities":["embeddings"]}}]}"#;
    let catalogue = models_from_json(Role::Embedding, json).expect("parses");
    assert_eq!(
        find(&catalogue.entries, "vendor/fractional-limit").refusal,
        Some(Refusal::LimitNotUnderstood {
            raw: "8192.0".to_string()
        }),
        "a limit that was stated, just not in a shape this build parses, must not read as \
         though nothing was stated at all"
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
    assert_eq!(catalogue.entries[0].price_per_token, None);
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
        find(&catalogue.entries, "vendor/second").context_length,
        None,
        "an odd-shaped context_length must only cost that field, not the whole record"
    );
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
        find(&catalogue.entries, "vendor/optimistic-headline").context_length,
        Some(4000),
        "the smaller, more pessimistic number must win over the larger headline one"
    );
}

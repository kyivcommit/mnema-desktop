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
    let entries = models_from_json(Role::Embedding, EMBEDDINGS).expect("parses");
    let bge = find(&entries, "baai/bge-m3");
    assert_eq!(bge.refusal, None, "the default choice must be selectable");
    assert_eq!(bge.context_length, Some(8194));
    assert_eq!(bge.price_per_token, Some(0.00000001));
}

#[test]
fn a_model_that_takes_512_tokens_is_refused_and_says_both_numbers() {
    let entries = models_from_json(Role::Embedding, EMBEDDINGS).expect("parses");
    let small = find(&entries, "thenlper/gte-base");
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
fn a_refused_model_is_still_returned_rather_than_hidden() {
    let entries = models_from_json(Role::Embedding, EMBEDDINGS).expect("parses");
    assert!(
        entries
            .iter()
            .any(|e| e.id == "intfloat/multilingual-e5-large"),
        "hiding a model the provider lists sends the user looking for our bug"
    );
}

#[test]
fn the_rerank_list_parses_and_nothing_in_it_is_refused() {
    let entries = models_from_json(Role::Rerank, RERANK).expect("parses");
    assert_eq!(
        entries.len(),
        6,
        "the fixture holds six, so six must come back"
    );
    assert!(entries.iter().all(|e| e.refusal.is_none()));
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
    let entries = models_from_json(Role::Chat, json).expect("parses");
    assert_eq!(
        find(&entries, "vendor/speaks-only").refusal,
        Some(Refusal::NoTextOutput)
    );
    assert_eq!(find(&entries, "vendor/writes").refusal, None);
}

#[test]
fn an_unknown_field_is_ignored_and_a_missing_price_is_not_an_error() {
    let json = r#"{"data":[
        {"id":"vendor/new","name":"New","context_length":32000,
         "something_added_next_month":{"deep":[1,2,3]},
         "architecture":{"output_modalities":["text"]}}
    ]}"#;
    let entries = models_from_json(Role::Embedding, json).expect("parses");
    assert_eq!(entries[0].price_per_token, None);
    assert_eq!(entries[0].refusal, None);
}

#[test]
fn a_model_with_no_stated_limit_is_refused_rather_than_assumed_generous() {
    let json = r#"{"data":[{"id":"vendor/silent","name":"Silent",
        "architecture":{"output_modalities":["text"]}}]}"#;
    let entries = models_from_json(Role::Embedding, json).expect("parses");
    assert_eq!(entries[0].refusal, Some(Refusal::NoStatedLimit));
}

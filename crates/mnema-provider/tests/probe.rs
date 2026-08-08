//! Exercises `list_models` against a real socket (`mnema_mock_provider`)
//! instead of calling `models_from_json` directly, so what is tested is the
//! whole path: which URL the role builds, what travels in which header, how a
//! silent server is told from a slow one, and — the two pairs of tests the
//! Task 1 review demanded — that the role a request was built for is the same
//! role its answer is parsed under, and that a 200 with an unusable body says
//! which of three different problems it was.

use std::time::Duration;

use mnema_mock_provider::{MockServer, Reply};
use mnema_provider::{Error, MIN_CONTEXT_TOKENS, Refusal, Role, list_models};

#[test]
fn the_role_decides_the_query_and_the_key_travels_in_a_header() {
    let server = MockServer::new(vec![Reply::ok(r#"{"data":[]}"#)]);
    list_models(
        server.base(),
        Some("test-key-not-a-real-one"),
        Role::Embedding,
    )
    .expect("call");

    let request = server.request();
    assert!(
        request.contains("output_modalities=embeddings"),
        "the embedding list is a different query from the chat list: {request}"
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer test-key-not-a-real-one"),
        "the key must travel in the header, not in the query string: {request}"
    );
}

#[test]
fn the_chat_list_is_asked_without_a_filter() {
    let server = MockServer::new(vec![Reply::ok(r#"{"data":[]}"#)]);
    list_models(server.base(), None, Role::Chat).expect("call");
    assert!(
        !server.request().contains("output_modalities"),
        "filtering chat by output modality drops the models that also draw or speak"
    );
}

#[test]
fn silence_becomes_a_transport_error_rather_than_a_hang() {
    let server = MockServer::new(vec![Reply::slow(60)]);
    let started = std::time::Instant::now();
    let err = list_models(server.base(), None, Role::Rerank).expect_err("must give up");
    assert!(matches!(err, Error::Transport(_)), "got {err:?}");
    assert!(
        started.elapsed() < Duration::from_secs(45),
        "the timeout must be shorter than the user's patience"
    );
}

/// Task 1 review, item 1: `list_models` builds the URL from `Role::query()`
/// and separately hands the body to `models_from_json(role, ..)`. Nothing
/// binds those two uses of `role` to each other — if they ever disagreed, the
/// failure would be silent and total: an embedding list parsed under
/// `Role::Chat` would refuse every record for writing no text, and a chat list
/// parsed under `Role::Embedding` would offer hundreds of models as
/// selectable embedders.
///
/// One record is used across all three roles, chosen so each role reaches a
/// *different* verdict on it: 100 tokens is below `MIN_CONTEXT_TOKENS`, so
/// `Role::Embedding` refuses it for that; its only stated output modality is
/// `embeddings`, so `Role::Chat` refuses it for writing no text; `Role::Rerank`
/// has no input floor and no output-modality rule, so it refuses nothing. A
/// build that used one role for the query and a different, fixed role for the
/// parse would still pass every earlier test — none of them varies the role —
/// but would fail here, on at least one of the three iterations.
#[test]
fn the_query_and_the_parse_use_the_same_role_on_every_call() {
    let body = r#"{"data":[{"id":"vendor/edge-case","name":"Edge","context_length":100,
        "architecture":{"output_modalities":["embeddings"]}}]}"#;

    for role in [Role::Embedding, Role::Chat, Role::Rerank] {
        let server = MockServer::new(vec![Reply::ok(body)]);
        let catalogue = list_models(server.base(), None, role).expect("call");

        let request = server.request();
        match role.query() {
            Some(filter) => assert!(
                request.contains(&format!("output_modalities={filter}")),
                "role {role:?} must ask with its own filter: {request}"
            ),
            None => assert!(
                !request.contains("output_modalities"),
                "role {role:?} must ask without a filter: {request}"
            ),
        }

        let expected_refusal = match role {
            Role::Embedding => Some(Refusal::InputTooSmall {
                limit: 100,
                floor: MIN_CONTEXT_TOKENS,
            }),
            Role::Chat => Some(Refusal::NoTextOutput),
            Role::Rerank => None,
        };
        assert_eq!(
            catalogue.entries[0].refusal, expected_refusal,
            "role {role:?}: the role the query was built for and the role the answer was \
             parsed under must be the same role — the same server reply must not read \
             differently just because the two uses of `role` inside `list_models` disagreed"
        );
    }
}

/// Task 2 review, item 3: `{"data":[]}` parses to an empty catalogue, and that
/// is deliberate, not incidental. `list_models` relays the provider's answer
/// faithfully; deciding whether zero selectable models is actionable belongs
/// to whoever renders the result, which sees `unreadable` too and can tell
/// "the provider genuinely has none for this role" from "something upstream
/// ate them". Turning a well-formed, on-topic answer into an error here would
/// hide it instead of reporting it.
#[test]
fn an_empty_catalogue_is_a_success_for_the_caller_to_interpret_not_a_network_failure() {
    let server = MockServer::new(vec![Reply::ok(r#"{"data":[]}"#)]);
    let catalogue = list_models(server.base(), None, Role::Rerank)
        .expect("a provider stating zero models is not a transport or parse failure");
    assert_eq!(
        catalogue.entries,
        Vec::new(),
        "no records means no entries, not a placeholder"
    );
    assert_eq!(
        catalogue.unreadable, 0,
        "nothing was sent, so nothing failed to parse either"
    );
}

/// Task 2 review, item 2, exercised through the real socket rather than
/// `models_from_json` directly: a captive portal or a misconfigured proxy
/// answers with `200` and an HTML page instead of forwarding to the provider.
/// `http_status_as_error(false)` must not swallow this as a transport error —
/// the body has to actually reach parsing, and parsing has to say it was not
/// JSON at all, not the generic "wrong shape" this crate used to say for
/// every unparseable body alike.
#[test]
fn a_captive_portal_page_reaches_parsing_and_is_named_for_what_it_is() {
    let server = MockServer::new(vec![Reply::ok(
        "<html><body>Sign in to the network</body></html>",
    )]);
    let err = list_models(server.base(), None, Role::Chat).expect_err("not JSON at all");
    match err {
        Error::Malformed(reason) => assert!(
            reason.contains("not JSON"),
            "an HTML page must be named as not-JSON, not folded into a generic parse failure: \
             {reason}"
        ),
        other => panic!("expected Malformed, got {other:?}"),
    }
}

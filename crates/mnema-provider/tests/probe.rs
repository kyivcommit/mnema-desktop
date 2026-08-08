//! Exercises `list_models` against a real socket (`mnema_mock_provider`)
//! instead of calling `models_from_json` directly, so what is tested is the
//! whole path: which URL the role builds, what travels in which header, and —
//! the two pairs of tests the Task 1 review demanded — that the role a
//! request was built for is the same role its answer is parsed under, and
//! that a 200 with an unusable body says which of three different problems it
//! was. The timeout itself (does it fire, is it configured) is a unit test in
//! `src/http.rs` instead of a 30-second integration test here (Task 2 review
//! round 2, G5).

use std::time::{Duration, Instant};

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
    let request_line = request.lines().next().unwrap_or_default();
    assert!(
        request_line.contains("output_modalities=embeddings"),
        "the embedding list is a different query from the chat list: {request}"
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer test-key-not-a-real-one"),
        "the key must travel in the header: {request}"
    );
    // The header assertion above passes whether or not the key ALSO leaked
    // into the query string — it only checks the header is present (Task 2
    // review round 1, cheap correction). The request line is the one place a
    // query-string leak would show up.
    assert!(
        !request_line.contains("test-key-not-a-real-one"),
        "the key must travel only in the header, never in the request line/query string: \
         {request_line}"
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

/// Task 2 review round 1, mock crate: `listener.incoming().zip(replies)` used
/// to simply stop accepting once `replies` ran out, so a test that
/// accidentally made one call too many either hung waiting for a connection
/// nothing would ever accept, or — depending on OS timing — got a fast
/// connection failure that could be mistaken for the test's own expected
/// error. `599` is a status no real provider or proxy sends, so a call past
/// the end of the configured replies now fails in a way that cannot be
/// mistaken for anything else.
#[test]
fn a_request_past_the_configured_replies_fails_loudly_instead_of_hanging() {
    let server = MockServer::new(vec![Reply::ok(r#"{"data":[]}"#)]);
    list_models(server.base(), None, Role::Chat).expect("the one configured reply");
    let err = list_models(server.base(), None, Role::Chat)
        .expect_err("a second call with no reply configured must not succeed");
    assert!(
        matches!(err, Error::Provider { status: 599 }),
        "got {err:?}"
    );
}

/// Task 2 review round 1, F3: before this round, no test in the whole
/// workspace ever sent a non-200 through `list_models`. `Reply::status` was
/// unused; swapping the `Unauthorised` and `RateLimited` arms, or deleting
/// `http_status_as_error(false)` entirely, both left the suite green. This
/// test doubles as the pin the review asked for: with
/// `http_status_as_error(false)` removed, `ureq` treats a non-2xx as a
/// transport error, so `matches!(err, Error::Unauthorised)` — not a looser
/// check that would also accept `Error::Transport` — is what turns red on
/// that mutation (recorded in the report).
#[test]
fn a_401_with_a_key_says_the_key_was_refused() {
    let server = MockServer::new(vec![Reply::status(401, "")]);
    let err = list_models(server.base(), Some("test-key-not-a-real-one"), Role::Chat)
        .expect_err("401 must fail");
    assert!(matches!(err, Error::Unauthorised), "got {err:?}");
}

/// Task 2 review round 1, F2: a 401 on a call that sent no key is not a
/// credential being refused — there was no credential — it is the provider
/// now requiring one for an endpoint this build calls anonymously.
#[test]
fn a_401_without_a_key_says_the_endpoint_now_requires_one() {
    let server = MockServer::new(vec![Reply::status(401, "")]);
    let err = list_models(server.base(), None, Role::Chat).expect_err("401 must fail");
    assert!(matches!(err, Error::KeyRequired), "got {err:?}");
}

/// Task 2 review round 2, G1: a 403 with a key sent means the key is real and
/// simply not permitted to do this — a different fact from `Unauthorised`
/// (the key itself refused) and from `KeyRequired`/`AnonymousBlocked` (no key
/// was sent at all).
#[test]
fn a_403_with_a_key_says_the_key_is_not_permitted() {
    let server = MockServer::new(vec![Reply::status(403, "")]);
    let err = list_models(server.base(), Some("test-key-not-a-real-one"), Role::Chat)
        .expect_err("403 must fail");
    assert!(matches!(err, Error::Forbidden), "got {err:?}");
}

/// Task 2 review round 2, G1: a 403 on a call that sent no key at all does
/// not name an account — this build never offered one to refuse. On a
/// public, key-less endpoint this is most often a proxy or gateway between
/// this machine and the provider, not the provider itself.
#[test]
fn a_403_without_a_key_names_something_between_this_machine_and_the_provider() {
    let server = MockServer::new(vec![Reply::status(403, "")]);
    let err = list_models(server.base(), None, Role::Chat).expect_err("403 must fail");
    assert!(matches!(err, Error::AnonymousBlocked), "got {err:?}");
}

/// Task 2 review round 1, F2: anonymous rate limiting on a public endpoint is
/// real, so the message must not claim a key was involved when none was sent.
#[test]
fn a_429_is_rate_limited_without_naming_a_key() {
    let server = MockServer::new(vec![Reply::status(429, "")]);
    let err = list_models(server.base(), None, Role::Chat).expect_err("429 must fail");
    assert!(matches!(err, Error::RateLimited), "got {err:?}");
    assert!(
        !err.to_string().to_ascii_lowercase().contains("key"),
        "a rate limit with no key sent must not blame one: {err}"
    );
}

#[test]
fn a_500_is_a_provider_error_naming_its_status() {
    let server = MockServer::new(vec![Reply::status(500, "")]);
    let err = list_models(server.base(), None, Role::Chat).expect_err("500 must fail");
    assert!(
        matches!(err, Error::Provider { status: 500 }),
        "got {err:?}"
    );
}

/// Spec review round 1, item A: inherited item 2's third case — a truncated
/// response — was closed at the parser (`models_from_json`'s `Eof` branch)
/// but never reached on the real wire, because a length-delimited body that
/// stops early fails during the body read itself (F1), before parsing ever
/// starts. `Reply::truncated` gives the mock server the ability to produce
/// that shape for real: a `content-length` that promises more bytes than the
/// connection ever sends before closing.
#[test]
fn a_body_that_stops_mid_transfer_is_named_with_its_status_preserved() {
    let server = MockServer::new(vec![Reply::truncated(r#"{"data":[{"id":"vendor/x""#)]);
    let err = list_models(server.base(), None, Role::Chat).expect_err("body never completed");
    match &err {
        Error::BodyUnreadable { status, .. } => {
            assert_eq!(
                *status, 200,
                "the status was read successfully before the body failed"
            )
        }
        other => panic!("expected BodyUnreadable, got {other:?}"),
    }
    // Task 2 review round 2, G2: the same error also fires for
    // `BodyExceedsLimit` and a timeout during the body read, neither of which
    // is the provider's connection stopping. The top-level sentence must stay
    // neutral about the cause and let `detail` carry it, not assert that the
    // body "stopped" — which is only true for this one of the three.
    assert!(
        err.to_string().contains("reading the response body failed"),
        "the top-level sentence must not claim a specific cause: {err}"
    );
}

/// Task 2 review round 2, G4: once the accept loop started answering
/// past-the-end requests with the `599` sentinel, it never ended — `tx` was
/// never dropped, so every mock server's thread and listening port outlived
/// the test that created it, and `request()` with nothing left to report
/// waited out the full 10-second `recv_timeout` before panicking instead of
/// failing right away. A `break` after the sentinel ends the loop and drops
/// `tx`, so a `request()` call with no third request to see now panics
/// almost immediately.
#[test]
fn the_accept_loop_ends_after_answering_past_the_end() {
    let server = MockServer::new(vec![Reply::ok(r#"{"data":[]}"#)]);
    list_models(server.base(), None, Role::Chat).expect("the one configured reply");
    // Past the end: triggers the 599 sentinel and, with the fix, ends the loop.
    let _ = list_models(server.base(), None, Role::Chat);
    // Drain the two requests already made before asking for a third that was
    // never sent — otherwise this would just read the first of those two back.
    server.request();
    server.request();

    let started = Instant::now();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| server.request()));
    assert!(panicked.is_err(), "there is no third request to report");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the accept loop must end once it answers past-the-end, or this call waits out the \
         full 10 s recv_timeout instead of failing right away: took {:?}",
        started.elapsed()
    );
}

/// The `Error` doc comment promises this scan; before this round it only
/// covered the failure paths in Task 1's original plan. Task 2 review round 1
/// (F2) and round 2 (G1) added five more ways to fail, none of them checked
/// against it. Every path reachable from `list_models` today that could
/// plausibly carry the key, called with a real key present wherever a key
/// can be present at all, is checked here in one place.
#[test]
fn no_error_message_ever_contains_the_key() {
    const KEY: &str = "test-key-not-a-real-one";

    let assert_key_absent = |err: Error| {
        assert!(
            !err.to_string().contains(KEY),
            "an error message must never contain the key it was given: {err}"
        );
    };

    for status in [401, 403, 429, 500] {
        let server = MockServer::new(vec![Reply::status(status, "")]);
        assert_key_absent(
            list_models(server.base(), Some(KEY), Role::Chat)
                .expect_err("a non-200 status must fail"),
        );
    }

    let malformed = MockServer::new(vec![Reply::ok("<html></html>")]);
    assert_key_absent(
        list_models(malformed.base(), Some(KEY), Role::Chat).expect_err("not JSON must fail"),
    );

    let truncated = MockServer::new(vec![Reply::truncated(r#"{"data":[{"id":"x""#)]);
    assert_key_absent(
        list_models(truncated.base(), Some(KEY), Role::Chat)
            .expect_err("a truncated body must fail"),
    );

    // A host nothing listens on, rather than a slow one: a real `Transport`
    // error without paying out this crate's 30 s global timeout (G5).
    assert_key_absent(
        list_models("http://127.0.0.1:1", Some(KEY), Role::Chat)
            .expect_err("an unreachable host must fail"),
    );
}

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

use mnema_mock_provider::{MockServer, Reply, two_vectors};
use mnema_provider::{
    Balance, Error, MIN_CONTEXT_TOKENS, Refusal, Role, check_embedding_model, check_key,
    list_models,
};
use unicode_general_category::{GeneralCategory, get_general_category};

/// Shared by every `check_key` test below — hoisted here rather than
/// repeated per test or left local to `no_error_message_ever_contains_the_key`
/// (which used to declare its own copy of the same literal).
const KEY: &str = "test-key-not-a-real-one";

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
        matches!(err, Error::Provider { status: 599, .. }),
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
    assert!(matches!(err, Error::Unauthorised { .. }), "got {err:?}");
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
    assert!(matches!(err, Error::Forbidden { .. }), "got {err:?}");
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
    assert!(matches!(err, Error::RateLimited { .. }), "got {err:?}");
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
        matches!(err, Error::Provider { status: 500, .. }),
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
/// failing right away.
///
/// Round 2's fix was a `break`, which round 3's review caught trading one
/// bug for another: ending the loop meant a *second* surplus request got a
/// connection refusal (`Error::Transport`) instead of the sentinel — exactly
/// the shape the sentinel exists to keep a test from mistaking for something
/// else. The loop now runs forever; only the channel send stops after the
/// first surplus request, which is what this test actually needs to hold.
#[test]
fn request_fails_fast_once_the_first_surplus_request_has_been_reported() {
    let server = MockServer::new(vec![Reply::ok(r#"{"data":[]}"#)]);
    list_models(server.base(), None, Role::Chat).expect("the one configured reply");
    // Past the end: triggers the 599 sentinel and, with the fix, stops the
    // channel send — the accept loop itself keeps running.
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
        "the channel must stop taking requests once the first surplus one is reported, or \
         this call waits out the full 10 s recv_timeout instead of failing right away: took \
         {:?}",
        started.elapsed()
    );
}

/// The other half of Task 2 review round 3's Minor: a *third* request beyond
/// the configured replies must still get the unmistakable `599` sentinel,
/// not a connection refusal that could pass for a real network failure. This
/// is exactly the property round 2's `break` traded away.
#[test]
fn a_second_surplus_request_still_gets_the_sentinel_not_a_connection_refusal() {
    let server = MockServer::new(vec![Reply::ok(r#"{"data":[]}"#)]);
    list_models(server.base(), None, Role::Chat).expect("the one configured reply");
    list_models(server.base(), None, Role::Chat).expect_err("the first surplus request");
    let err = list_models(server.base(), None, Role::Chat).expect_err("the second surplus request");
    assert!(
        matches!(err, Error::Provider { status: 599, .. }),
        "a second surplus request must still get the sentinel, not {err:?}"
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
    let unreachable = list_models("http://127.0.0.1:1", Some(KEY), Role::Chat)
        .expect_err("an unreachable host must fail");
    // Task 2 review round 3, H2: after the 30-second silence test was
    // deleted, nothing in this crate asserted `Error::Transport` at all — the
    // scan above only ever checked for the key's absence, never the variant.
    // If `finish`'s connection-failure branch ever collapsed into something
    // carrying a status, an offline user would read "the provider answered
    // 0", and this scan would stay green regardless.
    assert!(
        matches!(unreachable, Error::Transport(_)),
        "got {unreachable:?}"
    );
    assert_key_absent(unreachable);
}

// --- check_key ---------------------------------------------------------
//
// Task 3: the cheap call at entry that answers "does this key work" before a
// long indexing run starts (spec §4.5). `list_models`'s tests above exercise
// `error_for_status` through the model list; these exercise the same table
// through `/credits`, plus what `check_key` adds on top of it — the
// account balance and the provider's own refusal text.

#[test]
fn a_good_key_comes_back_with_what_is_left_on_the_account() {
    let server = MockServer::new(vec![Reply::ok(
        r#"{"data":{"total_credits":10.0,"total_usage":3.5}}"#,
    )]);
    let check = check_key(server.base(), KEY).expect("accepted");
    assert_eq!(check.balance, Balance::Known { amount: 6.5 });
}

#[test]
fn a_refused_key_is_its_own_answer_and_not_a_generic_failure() {
    let server = MockServer::new(vec![Reply::status(401, r#"{"error":{"message":"nope"}}"#)]);
    let err = check_key(server.base(), KEY).expect_err("refused");
    assert!(matches!(err, Error::Unauthorised { .. }), "got {err:?}");
}

#[test]
fn a_key_check_with_no_stated_balance_still_accepts_the_key() {
    let server = MockServer::new(vec![Reply::ok(r#"{"data":{"unexpected":true}}"#)]);
    let check = check_key(server.base(), KEY).expect("accepted");
    assert_eq!(
        check.balance,
        Balance::NotStated,
        "neither field was stated at all — a fact about the account, not a reason to refuse a \
         working key"
    );
}

#[test]
fn no_failure_path_puts_the_key_into_the_message() {
    // Task 3 review round 1, C1: a provider that rejects a malformed
    // credential commonly echoes it back inside its own error message — the
    // one path into this scan that used to be unguarded, because every case
    // below it was chosen to satisfy the property rather than to break it.
    // Re-cased too (`to_ascii_uppercase`), since a case-sensitive
    // `contains(KEY)` would silently pass a leak that changed case, and a
    // provider is under no obligation to echo a credential back verbatim.
    let failures = vec![
        Reply::status(401, r#"{"error":{"message":"nope"}}"#),
        Reply::status(429, "{}"),
        Reply::status(500, "{}"),
        Reply::ok("{ this is not json"),
        Reply::status(
            401,
            &format!(r#"{{"error":{{"message":"invalid Authorization header: Bearer {KEY}"}}}}"#),
        ),
        Reply::status(
            403,
            &format!(
                r#"{{"error":{{"message":"key {} is not permitted"}}}}"#,
                KEY.to_ascii_uppercase()
            ),
        ),
    ];
    for reply in failures {
        let server = MockServer::new(vec![reply]);
        let err = check_key(server.base(), KEY).expect_err("must fail");
        let rendered = format!("{err} / {err:?}").to_ascii_lowercase();
        assert!(
            !rendered.contains(&KEY.to_ascii_lowercase()),
            "an error message is a log line, and this one carries the key: {rendered}"
        );
    }
}

/// Task 3 review round 1, C1 — the positive half of the property above: not
/// just "the key is absent", but "the provider's explanation still arrives,
/// with the key's own place in it marked rather than silently dropped".
#[test]
fn a_key_echoed_back_by_the_provider_is_redacted_not_dropped() {
    let server = MockServer::new(vec![Reply::status(
        401,
        &format!(r#"{{"error":{{"message":"invalid Authorization header: Bearer {KEY}"}}}}"#),
    )]);
    let err = check_key(server.base(), KEY).expect_err("refused");
    let rendered = err.to_string();
    assert!(
        !rendered
            .to_ascii_lowercase()
            .contains(&KEY.to_ascii_lowercase()),
        "the key must not appear: {rendered}"
    );
    assert!(
        rendered.contains("invalid Authorization header") && rendered.contains("[redacted]"),
        "the rest of the provider's sentence must survive redaction, with the key's place in \
         it marked rather than the whole message being dropped: {rendered}"
    );
}

/// JSON-escapes `c` as a `\uXXXX` sequence, for embedding a control or
/// invisible character into a JSON string literal — the shape a provider's
/// own JSON encoder produces for one, not a hand-broken document a raw byte
/// would be (a raw C0 control byte inside a JSON string is invalid JSON,
/// and `serde_json` would refuse to parse the body at all). The shape this
/// exercises is a *decoded* character reaching `ProviderMessage::new`.
fn json_escape(c: char) -> String {
    format!("\\u{:04x}", c as u32)
}

/// Seven shapes of `key` a provider (or a hand-broken document nobody
/// intended) could produce, used to check the property "the key never
/// reaches a rendered message" against the key itself rather than against a
/// guessed list of provider behaviours (Task 3 review round 2). Each tuple
/// is `(label, the transformed text to embed, the substring that must not
/// survive)`. Shared between the failure-path and success-path leak scans
/// below (Task 3 review round 3, J1) so both exercise the identical set.
fn key_transformations(key: &str) -> Vec<(&'static str, String, String)> {
    let mid = key.len() / 2;
    let fragment = key[mid - 6..mid + 6].to_string();
    vec![
        ("verbatim", key.to_string(), key.to_string()),
        ("re-cased", key.to_ascii_uppercase(), key.to_string()),
        (
            "a C0 control character inserted",
            format!("{}{}{}", &key[..mid], json_escape('\u{1F}'), &key[mid..]),
            key.to_string(),
        ),
        (
            "a zero-width space inserted",
            format!("{}{}{}", &key[..mid], json_escape('\u{200B}'), &key[mid..]),
            key.to_string(),
        ),
        (
            // Task 3 review round 4, K1: the reviewer's measured attack —
            // C0 insertions, spaced so no surviving segment reaches
            // FRAGMENT_LEN, defeated both the exact-substring redaction and
            // the fragment net on the success path, where the sanitiser used
            // to be handed `data.to_string()` — a *re-serialised* form in
            // which a decoded control character comes back as six ordinary
            // ASCII characters, no longer anything `unsafe_for_display`
            // recognises. The single-insertion case above does not catch
            // this: `KEY` is 23 characters, and any one split leaves a run
            // of at least twelve for the fragment net to catch. This case
            // splits at `23 / 3 = 7`, which is four segments and three
            // insertions — the count the label and this comment both had
            // wrong until fix round 5 (L3), against a key they called 24
            // characters long. The substance was right either way: no
            // segment reaches twelve, so the net has nothing to catch.
            "three C0 control characters spaced within the fragment window",
            {
                let c0 = json_escape('\u{1F}');
                let chunk = (key.len() / 3).max(1);
                key.as_bytes()
                    .chunks(chunk)
                    .map(|part| std::str::from_utf8(part).expect("KEY is ASCII"))
                    .collect::<Vec<_>>()
                    .join(&c0)
            },
            key.to_string(),
        ),
        (
            // Task 3 review round 3, J5: the reviewer's measured attack —
            // three U+2060 WORD JOINER insertions, spaced no more than
            // eleven characters apart, rendered the key verbatim past both
            // defences before the strip list moved from a handwritten set
            // to the Cf Unicode general category.
            "three WORD JOINERs spaced within the fragment window",
            {
                let wj = json_escape('\u{2060}');
                let chunk = (key.len() / 4).max(1);
                key.as_bytes()
                    .chunks(chunk)
                    .map(|part| std::str::from_utf8(part).expect("KEY is ASCII"))
                    .collect::<Vec<_>>()
                    .join(&wj)
            },
            key.to_string(),
        ),
        (
            "split across a JSON escape",
            format!(
                "{}{}{}",
                &key[..1],
                json_escape(key.chars().nth(1).expect("key has at least two characters")),
                &key[1 + key.chars().nth(1).unwrap().len_utf8()..]
            ),
            key.to_string(),
        ),
        ("truncated to a fragment", fragment.clone(), fragment),
    ]
}

/// Unicode's `Default_Ignorable_Code_Point` is a DERIVED property, not
/// fully covered by the general categories below — no dependency available
/// to this crate exposes it directly (Task 3 review round 4, K2/K4). Named
/// explicitly, and only here, in a test's own deliberately over-inclusive
/// oracle: U+FE0F VARIATION SELECTOR-16 and U+034F COMBINING GRAPHEME
/// JOINER are already caught by `NonspacingMark` below and listed again
/// only as a direct pin; U+3164 HANGUL FILLER and U+FFA0 HALFWIDTH HANGUL
/// FILLER are `General_Category=Lo` (letters) and need to be named, since
/// stripping all letters would defeat the point. Being wider than strictly
/// necessary is exactly what a test oracle should be — unlike production's
/// own strip list, which stays a category rather than a list on purpose
/// (K4/round 3's J5) precisely because *production* cannot afford to be
/// wrong in the other direction.
fn is_unsafe_for_test_oracle(c: char) -> bool {
    if matches!(
        get_general_category(c),
        GeneralCategory::Control
            | GeneralCategory::Format
            | GeneralCategory::LineSeparator
            | GeneralCategory::ParagraphSeparator
            | GeneralCategory::NonspacingMark
    ) {
        return true;
    }
    matches!(c, '\u{034F}' | '\u{3164}' | '\u{FFA0}' | '\u{FE0F}')
}

/// The leak scans' own definition of "safe to render", independent of
/// production (Task 3 review round 4, K2). Round 3's fix for J4 called
/// production's own sanitiser here instead — the reviewer named that the
/// weaker half of the fix: it stopped the test and production drifting
/// apart, but it also meant the test narrowed whenever production narrowed,
/// which is the exact failure the derivation was meant to prevent. An
/// oracle must not share a defect with the thing it checks — confirmed by
/// K1 below, where an actual production gap left this exact style of check
/// unable to tell a real leak from a merely-obfuscated substring.
///
/// No length cap either, unlike production's `MAX_MESSAGE_LEN`: that cap is
/// a display choice, not a security boundary, and reusing it here made a
/// leaked key positioned past it in a long message invisible to this exact
/// check — measured: the "was the key redacted at all" mutation is caught
/// for provider messages of 34, 84 and 144 bytes, and not caught from
/// roughly 180 bytes upward, inside the length this crate documents as an
/// ordinary explanation.
fn strip_for_test_oracle(text: &str) -> String {
    text.chars()
        .filter(|c| !is_unsafe_for_test_oracle(*c))
        .collect()
}

/// Whether a `key_transformations` entry represents the *whole* key,
/// obfuscated some way — expected outcome: `redact_key` finds it, and the
/// rendered text shows the `[redacted]` placeholder — versus a genuine
/// *fragment* of it, which the fragment net withholds entirely instead.
/// Only "truncated to a fragment" is the second kind.
fn expects_full_key_redaction(label: &str) -> bool {
    label != "truncated to a fragment"
}

/// Confirms a defence actually fired, rather than the substring check above
/// merely failing to match by accident (Task 3 review round 4, K1: a key
/// broken up by literal escape text — not an invisible character, six
/// ordinary ASCII characters — defeats an exact-substring "is the key
/// present" check without `redact_key` or the fragment net having done
/// anything about it at all).
fn assert_a_defence_fired(label: &str, rendered: &str) {
    let defended = if expects_full_key_redaction(label) {
        rendered.contains("[redacted]")
    } else {
        rendered.contains("Withheld")
    };
    assert!(
        defended,
        "transformation {label:?} must show a visible defence fired, not merely fail to match \
         a substring check: {rendered}"
    );
}

/// Task 3 review round 2: six hand-picked bodies (the previous version of
/// this test) can only ever prove the property against six specific shapes
/// — the shape that actually broke it, a C0 control character sitting
/// inside the echoed key, was not among them, and was exactly the shape
/// nobody would think to hand-pick. Looping over `key_transformations`
/// checks the property against the key itself instead.
#[test]
fn no_transformation_of_the_key_reaches_a_failure_message() {
    for (label, transformed, needle) in key_transformations(KEY) {
        let body = format!(r#"{{"error":{{"message":"invalid credential: {transformed}"}}}}"#);
        let server = MockServer::new(vec![Reply::status(401, &body)]);
        let err = check_key(server.base(), KEY).expect_err("must fail");
        let rendered = format!("{err} / {err:?}");
        let visually = strip_for_test_oracle(&rendered).to_ascii_lowercase();
        assert!(
            !visually.contains(&needle.to_ascii_lowercase()),
            "transformation {label:?} must not leak, even to a reader who cannot see an \
             invisible character: {rendered}"
        );
        assert_a_defence_fired(label, &rendered);
    }
}

/// Task 3 review round 3, J1: `Balance::Unreadable`'s `raw` field carries
/// provider bytes to the screen on the SUCCESS path — a 200 whose balance
/// could not be read. Every leak scan before this one only ever exercised a
/// *failure* path. The sanitising call in `balance_from`
/// (`ProviderMessage::from_provider_text`) is already correct, but nothing
/// held it: replacing it with a raw pass-through left the whole suite
/// green.
#[test]
fn no_transformation_of_the_key_reaches_a_successful_balance() {
    for (label, transformed, needle) in key_transformations(KEY) {
        let body = format!(
            r#"{{"data":{{"total_credits":"quota exhausted for key: {transformed}","total_usage":0}}}}"#
        );
        let server = MockServer::new(vec![Reply::ok(&body)]);
        let check = check_key(server.base(), KEY).expect("a 200 means the key works");
        let rendered = format!("{:?}", check.balance);
        let visually = strip_for_test_oracle(&rendered).to_ascii_lowercase();
        assert!(
            !visually.contains(&needle.to_ascii_lowercase()),
            "transformation {label:?} must not leak on the success path: {rendered}"
        );
        assert_a_defence_fired(label, &rendered);
    }
}

/// Task 3 review round 4, Critical 1 (fix round 5, L1): the shared
/// transformation set above embeds the key in a *string* leaf, always — both
/// loops build `total_credits` as one string of fixed shape, so a `data`
/// whose leaf is an array or an object never reaches the summary builder at
/// all. That branch rendered its value straight through `Value`'s own
/// `Display`, outside the sanitiser: the key came back verbatim, with no
/// redaction marker anywhere, on a body round 3 had redacted. Kept out of
/// `key_transformations` on purpose — that set cannot express "the key sits
/// one level down" without distorting the seven cases that share its one
/// string.
///
/// The fourth case names the shape that is a leaf without being a *value*:
/// an object's own field name is provider bytes too, and reaches the screen
/// the same way its value does.
#[test]
fn a_key_inside_a_non_string_leaf_is_sanitised_too() {
    let bodies = [
        (
            "an object in total_credits",
            format!(
                r#"{{"data":{{"total_credits":{{"note":"key {KEY} is exhausted"}},"total_usage":0}}}}"#
            ),
        ),
        (
            "an array in total_credits",
            format!(r#"{{"data":{{"total_credits":["key {KEY} is exhausted"],"total_usage":0}}}}"#),
        ),
        (
            "an object in total_usage",
            format!(
                r#"{{"data":{{"total_credits":10.0,"total_usage":{{"note":"key {KEY} is exhausted"}}}}}}"#
            ),
        ),
        (
            "the key as an object's own field name",
            format!(r#"{{"data":{{"total_credits":{{"{KEY}":1}},"total_usage":0}}}}"#),
        ),
    ];
    for (label, body) in bodies {
        let server = MockServer::new(vec![Reply::ok(&body)]);
        let check = check_key(server.base(), KEY).expect("a 200 means the key works");
        assert!(
            matches!(check.balance, Balance::Unreadable { .. }),
            "{label}: a leaf this build cannot read as a number must still reach the summary: \
             {:?}",
            check.balance
        );
        let rendered = format!("{:?}", check.balance);
        let visually = strip_for_test_oracle(&rendered).to_ascii_lowercase();
        assert!(
            !visually.contains(&KEY.to_ascii_lowercase()),
            "{label}: a non-string leaf must not carry the key to the screen: {rendered}"
        );
        assert_a_defence_fired(label, &rendered);
    }
}

/// The other direction of the same recursion (fix round 5, L1): a
/// *fragment* of the key surviving inside a nested leaf must withhold the
/// whole summary, not just the leaf it sat in — the rule the two top-level
/// fields already keep between themselves, now reaching however deep the
/// leaf sits. The test above would stay green if a nested `Withheld` were
/// rendered as its own raw text, because a fragment is not the whole key
/// and its `contains(KEY)` check would not match it.
#[test]
fn a_key_fragment_inside_a_nested_leaf_withholds_the_whole_summary() {
    let fragment = &KEY[4..16]; // twelve characters, taken from the key
    let body = format!(
        r#"{{"data":{{"total_credits":{{"note":"key {fragment} is exhausted"}},"total_usage":0}}}}"#
    );
    let server = MockServer::new(vec![Reply::ok(&body)]);
    let check = check_key(server.base(), KEY).expect("a 200 means the key works");
    let rendered = format!("{:?}", check.balance);
    assert!(
        rendered.contains("Withheld"),
        "a fragment one level down must withhold the whole summary, not only its own leaf: \
         {rendered}"
    );
    assert!(
        !strip_for_test_oracle(&rendered)
            .to_ascii_lowercase()
            .contains(&fragment.to_ascii_lowercase()),
        "the fragment itself must not survive anywhere in the summary: {rendered}"
    );
}

/// Task 3 review, item 1: a plain `#[serde(default)] Option<f64>` field only
/// falls back when the *key* is absent. `total_credits` stated as `"$10.00"`
/// is present, so a naive field would fail to deserialize it, taking the
/// *whole* body down and turning a working key into a parse error — the
/// user reads "the credits answer is not the object this code expects" for
/// an account that is perfectly fine. `check_key` must still accept the key
/// and simply not know the balance.
#[test]
fn a_credits_field_in_a_shape_this_build_cannot_read_still_accepts_the_key() {
    let server = MockServer::new(vec![Reply::ok(
        r#"{"data":{"total_credits":"$10.00","total_usage":3.5}}"#,
    )]);
    let check = check_key(server.base(), KEY).expect("a working key must still be accepted");
    assert!(
        matches!(check.balance, Balance::Unreadable { .. }),
        "a balance stated in a shape this build cannot read is this build's own defect, not a \
         reason to fail the whole key check: {:?}",
        check.balance
    );
}

/// Task 3 review, item 2: `Error::BodyUnreadable` bypasses the status table
/// by design (Task 2 review round 2, G3), because it carries the status
/// precisely so a caller does not lose it. This screen's only job is "does
/// the key work", so a 401 whose body was cut off must give the same
/// verdict a clean 401 would — not "reading the response body failed" for a
/// key that was, in fact, refused.
#[test]
fn a_body_that_never_finishes_on_a_401_still_says_the_key_was_refused() {
    let server = MockServer::new(vec![Reply::truncated_status(401, r#"{"error":"#)]);
    let err = check_key(server.base(), KEY).expect_err("refused");
    assert!(matches!(err, Error::Unauthorised { .. }), "got {err:?}");
}

/// Task 3 review, item 4: `error_for_status` cannot carry the response body,
/// so the provider's own explanation used to die on the floor — the screen
/// said only "the key was refused", true and useless for a revoked key. The
/// real case named in review: a 401 whose body names the reason.
#[test]
fn a_refused_key_carries_the_providers_own_explanation() {
    let server = MockServer::new(vec![Reply::status(
        401,
        r#"{"error":{"message":"This key was disabled on 2026-08-01"}}"#,
    )]);
    let err = check_key(server.base(), KEY).expect_err("refused");
    let rendered = err.to_string();
    assert!(
        rendered.contains("This key was disabled on 2026-08-01"),
        "the provider's own explanation must reach the message, not just \"the key was \
         refused\": {rendered}"
    );
}

/// Task 3 review, item 5: `Unauthorised` is load-bearing here — it is the
/// whole point of this screen — and nothing pinned its wording before this,
/// unlike the two 403 messages hedged after review (see
/// `a_403_with_a_key_says_the_key_is_not_permitted` and its sibling, above).
/// A property, not a literal sentence, the same style
/// `a_429_is_rate_limited_without_naming_a_key` and
/// `a_body_that_stops_mid_transfer_is_named_with_its_status_preserved` above
/// already use.
#[test]
fn a_refused_key_says_it_was_refused() {
    let server = MockServer::new(vec![Reply::status(401, "")]);
    let err = check_key(server.base(), KEY).expect_err("refused");
    assert!(
        err.to_string().contains("refused"),
        "a refused key's message must say so: {err}"
    );
}

/// Task 3 review round 1, I3: `data: null` is valid JSON and a 200 — the
/// provider accepted the key, and simply did not (or could not) state a
/// balance in this reply. That must not fail the whole call the way it used
/// to, over a shape problem one level below the fact this screen answers.
#[test]
fn a_null_data_on_a_successful_status_still_accepts_the_key() {
    let server = MockServer::new(vec![Reply::ok(r#"{"data":null}"#)]);
    let check = check_key(server.base(), KEY).expect("a 200 means the key works");
    assert_eq!(check.balance, Balance::EnvelopeNotUnderstood);
}

/// Task 3 review round 1, I3, the other example named in review: an envelope
/// whose `data` key is missing or renamed entirely — not just a field
/// inside it — must read the same as `data: null` above, not as a reason to
/// fail the call.
#[test]
fn a_missing_data_key_on_a_successful_status_still_accepts_the_key() {
    let server = MockServer::new(vec![Reply::ok(r#"{"credits":{"total_credits":10.0}}"#)]);
    let check = check_key(server.base(), KEY).expect("a 200 means the key works");
    assert_eq!(check.balance, Balance::EnvelopeNotUnderstood);
}

/// Task 3 review round 1, Minor: `f64::from_str` accepts `"NaN"` and
/// `"inf"`/`"infinity"` in any case, reachable here because the provider
/// stated the balance as a JSON *string* — a number token this extreme
/// (`1e999`) is not used below because `serde_json` itself refuses to parse
/// it at all ("number out of range"), which already fails the whole body as
/// `Malformed` and never reaches `Stated`; a string is the one shape that
/// gets a non-finite value past JSON syntax and into `f64::from_str`. Both
/// cases must read as "stated in a shape this build cannot use", the same as
/// any other unreadable value — not as a balance that was successfully
/// read, which would also make `KeyCheck`'s derived `PartialEq` stop being
/// reflexive for a `NaN` balance (`NaN != NaN`).
#[test]
fn a_non_finite_balance_is_unreadable_not_a_number() {
    for body in [
        r#"{"data":{"total_credits":"NaN","total_usage":0}}"#,
        r#"{"data":{"total_credits":"inf","total_usage":0}}"#,
    ] {
        let server = MockServer::new(vec![Reply::ok(body)]);
        let check = check_key(server.base(), KEY).expect("a working key must still be accepted");
        assert!(
            matches!(check.balance, Balance::Unreadable { .. }),
            "a non-finite balance must read as unreadable, not as a number: body {body}, got \
             {:?}",
            check.balance
        );
    }
}

/// Task 3 review round 1, Minor: the first cut at attaching the provider's
/// explanation only ever checked it against `Unauthorised`, so a 403 —
/// which can carry the exact same `{"error":{"message":…}}` shape as a 401
/// — dropped the one sentence that would have told the user what to do.
#[test]
fn a_forbidden_key_also_carries_the_providers_own_explanation() {
    let server = MockServer::new(vec![Reply::status(
        403,
        r#"{"error":{"message":"this key is scoped to a different organisation"}}"#,
    )]);
    let err = check_key(server.base(), KEY).expect_err("forbidden");
    let rendered = err.to_string();
    assert!(
        rendered.contains("this key is scoped to a different organisation"),
        "a 403's explanation must reach the message too, not only a 401's: {rendered}"
    );
}

/// Task 3 review round 1, Minor: `BodyUnreadable` is only worth trading away
/// for a status `error_for_status` turns into a *specific* verdict for a key
/// check (401/403/429). A 500 gains nothing from that trade — `Provider`
/// is already the generic fallback whether or not the body could be read —
/// so it must keep `detail`, which names *why* the read failed (a size cap
/// or a timeout, say), rather than being silently discarded.
#[test]
fn a_body_that_never_finishes_on_a_500_keeps_its_detail() {
    let server = MockServer::new(vec![Reply::truncated_status(500, r#"{"error":"#)]);
    let err = check_key(server.base(), KEY).expect_err("must fail");
    match &err {
        Error::BodyUnreadable { status, detail } => {
            assert_eq!(*status, 500, "got {err:?}");
            assert!(
                !detail.is_empty(),
                "detail must not be thrown away: {err:?}"
            );
        }
        other => panic!("expected BodyUnreadable to survive a 500, got {other:?}"),
    }
}

// --- check_embedding_model ---------------------------------------------
//
// Task 4: the second of the two calls at entry (spec §2.6). `check_key`
// above answers "does this key work"; this one answers "will this model fill
// an index with numbers that mean something". Every case below is a refusal
// this build makes before a three-hour indexing run, not after it.

#[test]
fn the_dimension_is_measured_from_the_answer() {
    let server = MockServer::new(vec![Reply::ok(&two_vectors(1024))]);
    let check = check_embedding_model(server.base(), KEY, "baai/bge-m3").expect("usable");
    assert_eq!(check.dim, 1024, "nobody types this number; it is measured");
}

#[test]
fn two_texts_are_sent_in_one_request() {
    let server = MockServer::new(vec![Reply::ok(&two_vectors(8))]);
    check_embedding_model(server.base(), KEY, "m").expect("usable");
    let request = server.request();
    let body = request.rsplit("\r\n\r\n").next().expect("a body");
    let parsed: serde_json::Value = serde_json::from_str(body).expect("the body is JSON");
    assert_eq!(
        parsed["input"].as_array().map(Vec::len),
        Some(2),
        "one text cannot reveal an averaging model: {body}"
    );
    assert_eq!(parsed["model"], "m");
}

#[test]
fn a_model_that_averages_a_batch_is_refused_by_name() {
    // Google's embedder, measured 2026-07-25 (skeleton §6.2): several texts in
    // one request come back as ONE averaged vector.
    let one_vector = r#"{"data":[{"embedding":[1.0,0.0,0.0,0.0],"index":0}]}"#;
    let server = MockServer::new(vec![Reply::ok(one_vector)]);
    let err = check_embedding_model(server.base(), KEY, "averages/things").expect_err("refused");
    assert!(matches!(err, Error::AveragedBatch), "got {err:?}");
}

/// The brief asserted `Error::AveragedBatch` here; fix round 1, item 4 gave
/// this case its own error. A model that answers with a constant, or ignores
/// the second input, returned two vectors and averaged nothing — the refusal
/// is right either way, the stated cause was not.
#[test]
fn two_identical_vectors_for_two_different_texts_are_refused_too() {
    let same = r#"{"data":[{"embedding":[1.0,0.0],"index":0},{"embedding":[1.0,0.0],"index":1}]}"#;
    let server = MockServer::new(vec![Reply::ok(same)]);
    let err = check_embedding_model(server.base(), KEY, "constant/model").expect_err("refused");
    assert!(
        matches!(err, Error::IdenticalVectors),
        "counting the vectors is not enough: a model can return two copies of one answer, and \
         that is not the same fact as averaging a batch — got {err:?}"
    );
    assert!(
        !err.to_string().contains("averag"),
        "the sentence must not name a mechanism this build did not observe: {err}"
    );
}

#[test]
fn a_missing_model_says_so_instead_of_blaming_the_key() {
    let server = MockServer::new(vec![Reply::status(404, r#"{"error":{"message":"no"}}"#)]);
    let err = check_embedding_model(server.base(), KEY, "vendor/gone").expect_err("refused");
    match err {
        Error::NoSuchModel { model } => assert_eq!(model.to_string(), "vendor/gone"),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn an_empty_vector_is_refused() {
    let server = MockServer::new(vec![Reply::ok(
        r#"{"data":[{"embedding":[],"index":0},{"embedding":[],"index":1}]}"#,
    )]);
    let err = check_embedding_model(server.base(), KEY, "m").expect_err("refused");
    assert!(matches!(err, Error::EmptyVector), "got {err:?}");
}

#[test]
fn a_vector_that_is_not_quite_normalised_is_accepted_and_its_norm_reported() {
    // `codestral-embed-2505` measured at 0.9946 (skeleton §6.2). The space uses
    // cosine (`crates/mnema-index/src/space.rs:12`), which normalises on its
    // own, so this is information — not a reason to refuse.
    let body = r#"{"data":[{"embedding":[0.6,0.6],"index":0},{"embedding":[0.0,0.9],"index":1}]}"#;
    let server = MockServer::new(vec![Reply::ok(body)]);
    let check = check_embedding_model(server.base(), KEY, "m").expect("accepted");
    assert!(
        (check.norm - 0.8485).abs() < 0.001,
        "norm was {}",
        check.norm
    );
}

/// Controller's order, item 4: `data.len() != 2` is one-sided, and zero rows
/// satisfy it. Refusing an empty answer as `AveragedBatch` states a fact
/// about the provider — "this model returns one averaged vector for a batch"
/// — that is false about an answer with no vector in it at all. Only one row
/// is the measured Google case.
#[test]
fn no_vectors_at_all_is_not_a_claim_that_the_model_averages_a_batch() {
    let server = MockServer::new(vec![Reply::ok(r#"{"data":[]}"#)]);
    let err = check_embedding_model(server.base(), KEY, "m").expect_err("refused");
    match err {
        Error::Malformed(reason) => assert!(
            reason.contains("no vectors at all"),
            "an empty answer must be named for what it is, not as averaging: {reason}"
        ),
        other => panic!("expected Malformed, got {other:?}"),
    }
}

/// The other side of the same one-sided test: more rows than texts sent is
/// equally not an averaging model. Three vectors for two texts is a shape
/// this build does not understand, and saying "it averages a batch" would be
/// false in the opposite direction.
#[test]
fn more_vectors_than_texts_is_not_a_claim_that_the_model_averages_a_batch() {
    let three = r#"{"data":[{"embedding":[1.0,0.0],"index":0},{"embedding":[0.0,1.0],"index":1},
        {"embedding":[0.5,0.5],"index":2}]}"#;
    let server = MockServer::new(vec![Reply::ok(three)]);
    let err = check_embedding_model(server.base(), KEY, "m").expect_err("refused");
    match err {
        Error::Malformed(reason) => assert!(
            reason.contains("more vectors"),
            "more rows than texts must be named for what it is, not as averaging: {reason}"
        ),
        other => panic!("expected Malformed, got {other:?}"),
    }
}

/// Controller's order, item 2: hand-rolling the status table — which the
/// brief did — compiles, passes a variant check, and silently drops the
/// provider's own explanation on every failure. A revoked key on
/// `/embeddings` would print "the key was refused" and throw away the
/// sentence saying why. Both halves are asserted here: the right variant AND
/// the provider's sentence, for every status `error_for_status` gives this
/// call a `reason` field to carry.
#[test]
fn every_refusing_status_keeps_both_its_verdict_and_the_providers_own_sentence() {
    for status in [401, 403, 429, 500] {
        let sentence = format!("the provider's own sentence about {status}");
        let body = format!(r#"{{"error":{{"message":"{sentence}"}}}}"#);
        let server = MockServer::new(vec![Reply::status(status, &body)]);
        let err = check_embedding_model(server.base(), KEY, "m").expect_err("refused");
        let right_variant = match status {
            401 => matches!(err, Error::Unauthorised { .. }),
            403 => matches!(err, Error::Forbidden { .. }),
            429 => matches!(err, Error::RateLimited { .. }),
            _ => matches!(err, Error::Provider { status: 500, .. }),
        };
        assert!(right_variant, "status {status}: got {err:?}");
        assert!(
            err.to_string().contains(&sentence),
            "status {status}: the provider's own explanation is the one sentence that says what \
             to do, and a hand-rolled status table drops it: {err}"
        );
    }
}

/// The same scan `no_transformation_of_the_key_reaches_a_failure_message`
/// runs over `check_key`, now over the second call — a new family of failure
/// paths that renders a provider's body, and nothing held it to the rule
/// before this. `assert_a_defence_fired` is the positive half: not "the key
/// is absent" (which a call that never rendered anything satisfies) but "the
/// defence I am testing is the one that acted".
#[test]
fn no_transformation_of_the_key_reaches_a_model_check_failure() {
    for (label, transformed, needle) in key_transformations(KEY) {
        let body = format!(r#"{{"error":{{"message":"invalid credential: {transformed}"}}}}"#);
        let server = MockServer::new(vec![Reply::status(401, &body)]);
        let err = check_embedding_model(server.base(), KEY, "m").expect_err("must fail");
        let rendered = format!("{err} / {err:?}");
        let visually = strip_for_test_oracle(&rendered).to_ascii_lowercase();
        assert!(
            !visually.contains(&needle.to_ascii_lowercase()),
            "transformation {label:?} must not leak from the model check, even to a reader who \
             cannot see an invisible character: {rendered}"
        );
        assert_a_defence_fired(label, &rendered);
    }
}

/// The key travels in a header on the POST path too. `post_json` had no
/// caller at all until this task (`src/http.rs`), so nothing had ever checked
/// where the key ends up on a request with a body — and a URL is the one part
/// of a request a proxy writes to its own log without being asked.
#[test]
fn the_model_check_posts_to_the_embeddings_endpoint_with_the_key_only_in_a_header() {
    let server = MockServer::new(vec![Reply::ok(&two_vectors(4))]);
    check_embedding_model(server.base(), KEY, "vendor/m").expect("usable");
    let request = server.request();
    let request_line = request.lines().next().unwrap_or_default();
    assert!(
        request_line.starts_with("POST /embeddings "),
        "the model check is a POST to /embeddings: {request_line}"
    );
    assert!(
        request.to_ascii_lowercase().contains(&format!(
            "authorization: bearer {}",
            KEY.to_ascii_lowercase()
        )),
        "the key must travel in the header: {request}"
    );
    assert!(
        !request_line.contains(KEY),
        "the key must travel only in the header, never in the request line/query string: \
         {request_line}"
    );
}

/// The three shapes of unreadable 200, told apart the same way
/// `models_from_json` tells them apart for the model list: a captive portal
/// page is not JSON at all, a body that stops before the JSON closes is a
/// truncation, and anything else valid but foreign is simply not this shape.
/// One `Malformed` text for all three would be the difference between "check
/// your network" and "check your account" thrown away.
///
/// The third case used to be `{"error":{"message":"quota"}}`; fix round 1,
/// item 2 moved that body out of `Malformed` entirely — see the test below —
/// so the `Data` branch is pinned here with a body that carries no message to
/// keep.
#[test]
fn a_200_this_build_cannot_read_is_named_for_which_of_three_problems_it_is() {
    let cases = [
        (
            "<html><body>Sign in to the network</body></html>",
            "not JSON",
        ),
        (r#"{"data":[{"embedding":[1.0"#, "stopped in the middle"),
        (
            r#"{"object":"list","usage":{"total_tokens":8}}"#,
            "not the shape",
        ),
    ];
    for (body, expected) in cases {
        let server = MockServer::new(vec![Reply::ok(body)]);
        let err = check_embedding_model(server.base(), KEY, "m").expect_err("unreadable");
        match err {
            Error::Malformed(reason) => assert!(
                reason.contains(expected),
                "body {body:?} must be named as {expected:?}, got {reason:?}"
            ),
            other => panic!("body {body:?}: expected Malformed, got {other:?}"),
        }
    }
}

/// Fix round 1, item 2 (review finding 2). A gateway — or the provider itself
/// — answering `200` with its own error envelope used to read as "JSON, but
/// not the shape this code expects": true, and it threw away the only sentence
/// that says what to do. The same defect the status path had before
/// `attach_reason`, on the 200 path instead.
#[test]
fn a_200_carrying_the_providers_error_envelope_keeps_the_providers_sentence() {
    let server = MockServer::new(vec![Reply::ok(
        r#"{"error":{"message":"quota exceeded for this account"}}"#,
    )]);
    let err = check_embedding_model(server.base(), KEY, "m").expect_err("refused");
    assert!(
        matches!(err, Error::ErrorInsteadOfEmbeddings { .. }),
        "got {err:?}"
    );
    assert!(
        err.to_string().contains("quota exceeded for this account"),
        "the provider's own sentence is the difference between \"check your network\" and \
         \"check your account\": {err}"
    );
}

/// The other side of that branch: the envelope shape is what buys the
/// provider's sentence, and a body with no readable message must stay
/// `Malformed`, which is the fact that was true about it all along.
#[test]
fn a_200_error_envelope_with_no_readable_message_stays_a_shape_problem() {
    for body in [r#"{"error":{}}"#, r#"{"error":{"message":""}}"#] {
        let server = MockServer::new(vec![Reply::ok(body)]);
        let err = check_embedding_model(server.base(), KEY, "m").expect_err("unreadable");
        assert!(
            matches!(err, Error::Malformed(_)),
            "body {body:?}: got {err:?}"
        );
    }
}

/// Fix round 1, item 3 (review finding 3). The model id is not the user's own
/// text: it is copied verbatim out of the provider's body
/// (`catalogue.rs:412`) and the user only *picks* it from the list. Rendering
/// it through a plain format string is how a newline inside it cuts a log line
/// in half and lets provider text pass for a separate entry.
#[test]
fn a_model_id_is_sanitised_before_it_reaches_the_message() {
    let server = MockServer::new(vec![Reply::status(404, "{}")]);
    let err = check_embedding_model(
        server.base(),
        KEY,
        "vendor/gone\nWARN this line was never logged by this build",
    )
    .expect_err("refused");
    let rendered = format!("{err} / {err:?}");
    assert!(
        !rendered.contains('\n'),
        "a newline inside a model id must not reach a rendered message: {rendered:?}"
    );
    assert!(
        rendered.contains("vendor/gone"),
        "the rest of the id must survive, not be dropped whole: {rendered}"
    );
}

/// The same scan the failure path already runs, through the one value that
/// reaches a message without coming from a response body. The key is applied
/// *decoded* here rather than as a JSON escape, because that is the shape a
/// model id arrives in: `models_from_json` has already parsed it out of the
/// provider's JSON by the time anyone can select it.
#[test]
fn no_transformation_of_the_key_reaches_a_message_through_the_model_id() {
    let mid = KEY.len() / 2;
    let fragment = KEY[mid - 6..mid + 6].to_string();
    let cases = [
        ("verbatim", KEY.to_string(), KEY.to_string()),
        ("re-cased", KEY.to_ascii_uppercase(), KEY.to_string()),
        (
            "a decoded C0 control character inserted",
            format!("{}\u{1F}{}", &KEY[..mid], &KEY[mid..]),
            KEY.to_string(),
        ),
        (
            "a decoded word joiner inserted",
            format!("{}\u{2060}{}", &KEY[..mid], &KEY[mid..]),
            KEY.to_string(),
        ),
        ("truncated to a fragment", fragment.clone(), fragment),
    ];
    for (label, transformed, needle) in cases {
        let server = MockServer::new(vec![Reply::status(404, "{}")]);
        let err = check_embedding_model(server.base(), KEY, &format!("vendor/{transformed}"))
            .expect_err("refused");
        let rendered = format!("{err} / {err:?}");
        let visually = strip_for_test_oracle(&rendered).to_ascii_lowercase();
        assert!(
            !visually.contains(&needle.to_ascii_lowercase()),
            "transformation {label:?} must not leak through the model id, even to a reader who \
             cannot see an invisible character: {rendered}"
        );
        assert_a_defence_fired(label, &rendered);
    }
}

/// The half of that scan a Debug-shape check cannot see. `ProviderMessage`'s
/// own `Display` is written for an explanation, and substituting it into
/// "no model named {}" produces a sentence about an explanation on an error
/// that is about a name — two facts, one message, in the fix for two facts,
/// one message. The Debug form says `Withheld` either way, so only the
/// rendered sentence can hold this.
#[test]
fn a_withheld_model_id_does_not_borrow_the_sentence_written_for_an_explanation() {
    let fragment = &KEY[4..16]; // twelve characters, taken from the key
    let server = MockServer::new(vec![Reply::status(404, "{}")]);
    let err = check_embedding_model(server.base(), KEY, &format!("vendor/{fragment}"))
        .expect_err("refused");
    let rendered = err.to_string();
    assert!(
        !rendered.contains("explanation"),
        "a model id withheld is a name that could not be shown, not an explanation: {rendered}"
    );
    assert!(
        rendered.contains("name itself could not be shown safely"),
        "the sentence must say what actually happened: {rendered}"
    );
}

/// Fix round 1, item 5 (review finding, Minor 1). Both components are ordinary
/// finite `f32` values and ordinary JSON numbers, so the component check waves
/// them through; the true length is 4.24e38, which does not fit in the `f32`
/// this call reports it in, and `as f32` produces `inf`. `check_rankable`
/// (`crates/mnema-index/src/space.rs:405`) refuses such a vector at insert
/// time, so accepting it here is precisely the failure three hours into
/// indexing that this call exists to prevent.
#[test]
fn a_vector_whose_length_does_not_fit_the_reported_type_is_refused() {
    let body =
        r#"{"data":[{"embedding":[3.0e38,3.0e38],"index":0},{"embedding":[0.0,1.0],"index":1}]}"#;
    let server = MockServer::new(vec![Reply::ok(body)]);
    let err = check_embedding_model(server.base(), KEY, "m").expect_err("refused");
    match err {
        Error::Malformed(reason) => assert!(
            reason.contains("longer than this build can measure"),
            "got {reason:?}"
        ),
        other => panic!("expected Malformed, got {other:?}"),
    }
}

/// Two vectors of different widths state no dimensionality at all. Taking the
/// first one's length would put a number into `model_config.dim` that the
/// answer did not support — and `create_space` then pins the whole index to
/// it (`crates/mnema-index/src/space.rs:47-75`).
#[test]
fn two_vectors_of_different_widths_state_no_dimensionality() {
    let body =
        r#"{"data":[{"embedding":[1.0,0.0],"index":0},{"embedding":[0.0,1.0,0.0],"index":1}]}"#;
    let server = MockServer::new(vec![Reply::ok(body)]);
    let err = check_embedding_model(server.base(), KEY, "m").expect_err("refused");
    match err {
        Error::Malformed(reason) => assert!(reason.contains("different widths"), "got {reason:?}"),
        other => panic!("expected Malformed, got {other:?}"),
    }
}

/// JSON has no `NaN` or `Infinity` literal, but `1e39` is an ordinary JSON
/// number that becomes `f32::INFINITY` on the way into a `Vec<f32>`. Nothing
/// downstream would notice: the row count is right, the widths match, the two
/// vectors differ, and the check would hand back a dimensionality measured
/// from a vector whose numbers are not numbers. The same distinction
/// `Stated::finite_or_unreadable` already draws for a balance, one screen up.
#[test]
fn a_vector_component_that_is_not_a_finite_number_is_refused() {
    let body = r#"{"data":[{"embedding":[1e39,0.0],"index":0},{"embedding":[0.0,1.0],"index":1}]}"#;
    let server = MockServer::new(vec![Reply::ok(body)]);
    let err = check_embedding_model(server.base(), KEY, "m").expect_err("refused");
    match err {
        Error::Malformed(reason) => assert!(
            reason.contains("finite"),
            "an infinite component must be named, not measured: {reason:?}"
        ),
        other => panic!("expected Malformed, got {other:?}"),
    }
}

/// The trade `check_key` makes for a body-read failure (Task 3 review, item
/// 2) transfers to the three statuses where the status alone is the whole
/// verdict: a 401 whose body was merely cut off still means the key was
/// refused.
#[test]
fn a_body_that_never_finishes_on_a_401_still_says_the_key_was_refused_on_the_model_check() {
    let server = MockServer::new(vec![Reply::truncated_status(401, r#"{"error":"#)]);
    let err = check_embedding_model(server.base(), KEY, "m").expect_err("refused");
    assert!(matches!(err, Error::Unauthorised { .. }), "got {err:?}");
}

/// 404 is on that list too, and it is the one status where this call's own
/// table — not `error_for_status` — holds the verdict. The model is not there
/// whether or not the sentence explaining that arrived intact.
#[test]
fn a_body_that_never_finishes_on_a_404_still_names_the_model() {
    let server = MockServer::new(vec![Reply::truncated_status(404, r#"{"error":"#)]);
    let err = check_embedding_model(server.base(), KEY, "vendor/gone").expect_err("refused");
    match err {
        Error::NoSuchModel { model } => assert_eq!(model.to_string(), "vendor/gone"),
        other => panic!("got {other:?}"),
    }
}

/// Where this call parts company with `check_key` (inheritance item 5). A 200
/// whose body could not be read is not a check that passed with a detail
/// missing: on this endpoint the body IS the answer, and this build learned
/// nothing at all about the model. It must stay a failure carrying the reason
/// the read failed, not become a verdict of any kind.
#[test]
fn a_200_whose_body_never_finishes_is_a_failed_check_not_a_passed_one() {
    let server = MockServer::new(vec![Reply::truncated(r#"{"data":[{"embedding":[1.0"#)]);
    let err = check_embedding_model(server.base(), KEY, "m").expect_err("nothing was measured");
    match &err {
        Error::BodyUnreadable { status, detail } => {
            assert_eq!(*status, 200, "got {err:?}");
            assert!(
                !detail.is_empty(),
                "detail must not be thrown away: {err:?}"
            );
        }
        other => panic!("expected BodyUnreadable to survive a 200, got {other:?}"),
    }
}

/// The other bound on that guard, the same one `check_key` keeps: a 500 gains
/// nothing from trading `BodyUnreadable`'s `detail` — which names *why* the
/// read failed — for the generic `Provider { status }` this crate already
/// gives a 500 whose body it read just fine.
#[test]
fn a_body_that_never_finishes_on_a_500_keeps_its_detail_on_the_model_check_too() {
    let server = MockServer::new(vec![Reply::truncated_status(500, r#"{"error":"#)]);
    let err = check_embedding_model(server.base(), KEY, "m").expect_err("must fail");
    match &err {
        Error::BodyUnreadable { status, detail } => {
            assert_eq!(*status, 500, "got {err:?}");
            assert!(
                !detail.is_empty(),
                "detail must not be thrown away: {err:?}"
            );
        }
        other => panic!("expected BodyUnreadable to survive a 500, got {other:?}"),
    }
}

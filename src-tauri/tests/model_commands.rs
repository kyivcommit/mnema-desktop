//! The key crosses three boundaries — window, credential store, provider — and
//! must not cross a fourth into the database.

/// Beside `support/mod.rs` rather than inside it — see that file's own header
/// for why a shared module cannot hold something only one binary uses.
#[path = "support/fixture.rs"]
mod fixture;
mod support;

use fixture::Fixture;
use mnema_desktop::error::Error;
use mnema_desktop::models::{
    ExistingVectors, IndexRead, IndexSettings, KeyRemoval, KeyState, KeyStoreFailure,
    UnreadableCause, forget_key, key_present, model_settings, provider_models, set_chat_model,
    set_embedding_model, set_key, set_rerank_model,
};

/// The index half, or a failure naming what the index said instead.
///
/// A helper rather than a `match` at each call site: written out four times, one
/// of them ends up an `if let` that simply does nothing when the index was never
/// read — an assertion satisfied by the state it exists to exclude.
fn read_index(index: &IndexSettings) -> &IndexRead {
    match index {
        IndexSettings::Read(read) => read,
        IndexSettings::Unreadable { cause, reason } => {
            panic!("the index was expected to be readable here; it said {cause:?}: {reason}")
        }
    }
}

/// Synthetic, and shaped so it cannot be mistaken for a provider key: no `sk-`
/// prefix, no base64 tail, and it says what it is. If this string is ever found
/// in a database or a keychain, it came from here.
const KEY: &str = "test-key-not-a-real-one-0123456789";

/// A second one, so a test can tell "the key that was already there" from "some
/// key is there".
const KEY_ALREADY_ENTERED: &str = "test-key-not-a-real-one-abcdefghij";

/// The model the fixture's provider answers an embedding check for. The same
/// name `Fixture::adopt_default_model` uses; the mock ignores it, the index does
/// not.
const MODEL: &str = "baai/bge-m3";

/// The listing the mock answers with: one usable model and one at 512 tokens.
const TWO_EMBEDDERS: &str = r#"{"data":[
  {"id":"baai/bge-m3","name":"BGE M3","context_length":8194,
   "pricing":{"prompt":"0.00000001"},"architecture":{"output_modalities":["embeddings"]}},
  {"id":"thenlper/gte-base","name":"GTE base","context_length":512,
   "pricing":{"prompt":"0.000000005"},"architecture":{"output_modalities":["embeddings"]}}
]}"#;

#[test]
fn a_key_is_checked_before_it_is_stored() {
    let fx = Fixture::with_provider_rejecting_the_key();

    let outcome = set_key(fx.state(), "wrong-key".into());

    assert!(outcome.is_err(), "a refused key must not be accepted");
    assert_eq!(
        mnema_secrets::load(fx.credential_ref()).expect("read the store"),
        None,
        "a key that does not work, stored anyway, makes the app believe it is configured"
    );
}

#[test]
fn an_empty_key_is_refused_here_rather_than_being_sent_and_reported_as_a_verdict_on_a_key() {
    // The first thing the first real run of the application found. Pressing
    // "Check and save" with nothing typed sent a request carrying an empty
    // bearer token, and the provider's "Missing Authentication header" reached
    // the window as "the key was not saved: provider: the key was refused:
    // Missing Authentication header". Nobody had typed a key, so nothing had
    // been refused.
    //
    // Two claims, checked separately because they fail separately: the message
    // is not a verdict about a key, and no request left the machine to produce
    // one.
    let fx = Fixture::with_provider_rejecting_the_key();

    let refusal = set_key(fx.state(), String::new()).expect_err("an empty key is not a key");

    assert!(
        matches!(refusal, Error::EmptyKey),
        "an empty box must not arrive as anything the provider decided: {refusal:?}"
    );
    // The variant is not the whole guarantee: this type crosses the IPC as its
    // `Display` string, so the sentence is what a person actually reads.
    let said = refusal.to_string();
    assert!(
        !said.contains("refused") && !said.contains("provider:"),
        "the sentence still reports a refusal nobody made: {said}"
    );
    assert!(
        fx.provider_request().is_none(),
        "a request left the machine for a key nobody typed"
    );
    assert_eq!(
        mnema_secrets::load(fx.credential_ref()).expect("read the store"),
        None,
        "an empty string must not be stored as this installation's key"
    );

    // The other direction, and the reason this test can claim anything at all
    // about the line above: a real key DOES reach the provider through this
    // same fixture, so "no request arrived" is a fact about the guard and not
    // about a mock nobody could have reached. It is the fixture's one 401, so
    // the call is refused — which is beside the point here.
    set_key(fx.state(), KEY.into()).expect_err("the fixture's provider refuses every key");
    assert!(
        fx.provider_request().is_some(),
        "no request arrived for a key that was typed either, so the assertion above proves \
         nothing about the empty one"
    );
}

/// The line `set_key`'s doc draws deliberately, and had no witness for a round:
/// **empty is refused here, blank is decided by the provider.**
///
/// `if key.trim().is_empty()` passes every other test in this file and every
/// mutation case, and it states about somebody who typed spaces that they typed
/// nothing — which this build did not observe. The other direction is worse
/// still: trimming before the check would store a credential different from the
/// one a person entered.
///
/// The provider's verdict on a blank key is not this test's business — the
/// fixture answers 401 to everything — only that it is the provider's to give.
#[test]
fn a_key_of_spaces_is_decided_by_the_provider_rather_than_called_nothing() {
    let fx = Fixture::with_provider_rejecting_the_key();

    let outcome = set_key(fx.state(), "   ".into()).expect_err("this fixture refuses every key");

    assert!(
        !matches!(outcome, Error::EmptyKey),
        "spaces are something a person typed, and calling them nothing states what this build \
         did not see: {outcome:?}"
    );
    assert!(
        fx.provider_request().is_some(),
        "the request never left, so the refusal was ours and the doc comment is describing a \
         build that does not exist"
    );
}

#[test]
fn a_refusal_leaves_the_key_that_was_already_working() {
    // The best property of "check, then store", and the one the test above
    // cannot see: it starts from an empty store, so it is satisfied both by a
    // refusal that stored nothing and by a refusal that first deleted what was
    // there. Reordering to forget → check → store keeps it green and destroys a
    // working key on every mistyped attempt at a new one.
    let fx = Fixture::with_provider_rejecting_the_key();
    mnema_secrets::store(fx.credential_ref(), KEY_ALREADY_ENTERED).expect("a key is already there");

    set_key(fx.state(), KEY.into()).expect_err("a refused key must not be accepted");

    assert_eq!(
        mnema_secrets::load(fx.credential_ref())
            .expect("read the store")
            .as_deref(),
        Some(KEY_ALREADY_ENTERED),
        "a refusal must leave the key that was already working exactly where it was"
    );
}

#[test]
fn a_provider_that_refused_and_one_that_never_answered_are_two_shapes() {
    // Same consequence — the key was not saved — and opposite next actions: a
    // refused key needs a different key, an unreachable provider needs the same
    // key again later. One shape for both sends someone with a working
    // credential off to find another one while their network is down. The
    // `Display` strings already differed; what did not was the shape, so
    // nothing above this layer could branch without matching on text.
    let refusing = Fixture::with_provider_rejecting_the_key();
    let refusal = set_key(refusing.state(), KEY.into()).expect_err("the provider refused");
    assert!(
        matches!(refusal, Error::Provider(_)),
        "a provider that answered must not arrive as one that did not: {refusal:?}"
    );

    let silent = Fixture::with_no_provider_listening();
    let unreachable = set_key(silent.state(), KEY.into()).expect_err("nobody answered");
    assert!(
        matches!(unreachable, Error::ProviderUnreachable { .. }),
        "a provider that never answered must not arrive as one that refused: {unreachable:?}"
    );

    // Neither stored anything. Without this the two shapes could differ while
    // the thing that matters about both — the key was not saved — did not hold.
    for (case, fx) in [("refused", &refusing), ("unreachable", &silent)] {
        assert_eq!(
            mnema_secrets::load(fx.credential_ref()).expect("read the store"),
            None,
            "{case}: nothing may be stored when the key was never checked"
        );
    }
}

#[test]
fn an_absent_key_is_an_answer_rather_than_a_failure() {
    // Three different facts about a credential arrive at this layer: nobody has
    // entered one, the store would not open, something else deleted the entry.
    // Only the first is a normal state of the application, and it is the one a
    // window draws a "sign in" panel for. `key_present` keeps it apart in its
    // shape — absence is `Ok(false)`, and only a store that failed is an `Err`
    // — because a single message for both puts "no key has been entered" in
    // front of someone whose keychain is merely locked, and sends them to
    // re-enter a key they already have.
    let fx = Fixture::with_provider_rejecting_the_key();

    assert!(!key_present(fx.state()).expect("asking about an absent key is not a failure"));
}

#[test]
fn the_key_never_reaches_the_database_file() {
    let fx = Fixture::with_provider_accepting_everything();
    fx.open_index();

    set_key(fx.state(), KEY.into()).expect("accepted");
    fx.adopt_default_model();

    // Half one of the positive control. Every assertion below is an assertion
    // of absence, and absence is what a key that was never stored anywhere
    // looks like too — a credential store that quietly did nothing, a `set_key`
    // that returned before writing. This says the key exists to be found.
    assert_eq!(
        mnema_secrets::load(fx.credential_ref())
            .expect("read the store")
            .as_deref(),
        Some(KEY),
        "the store must hold the key this test is about to fail to find on disk"
    );

    // Every file the database is made of, not `index.sqlite` alone. The index
    // runs in WAL mode (`crates/mnema-index/src/open.rs:115`), so a value
    // written moments ago is in `index.sqlite-wal` until a checkpoint moves it,
    // and a scan of the main file would report an absence while the key sat on
    // disk one filename over.
    let scanned = fx.files_on_disk();
    let listing: Vec<String> = scanned
        .iter()
        .map(|f| f.path.display().to_string())
        .collect();

    assert!(
        scanned.iter().any(|f| f.path == fx.index_path()),
        "the scan did not find the database itself, so it is not reading what it \
         claims to be reading: {listing:?}"
    );
    // Half two. The loop below passes over an empty file just as happily as
    // over a configured index; this says the bytes being searched are the bytes
    // of a database that holds THIS installation's configuration. The reference
    // is the right witness because it is the one thing the design deliberately
    // does put in the database in the key's place.
    assert!(
        scanned
            .iter()
            .any(|f| f.holds(fx.credential_ref().as_bytes())),
        "no scanned file holds this installation's credential reference, so the \
         scan is not reading a configured database: {listing:?}"
    );

    for file in &scanned {
        assert!(
            !file.holds(KEY.as_bytes()),
            "{} holds the provider key. The database travels to colleagues (D33); \
             the key must not travel with it",
            file.path.display()
        );
    }
}

#[test]
fn forgetting_a_key_leaves_the_index_alone() {
    let fx = Fixture::with_provider_accepting_everything();
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("accepted");
    fx.adopt_default_model();
    // Both directions: without this, the assertion after `forget_key` is
    // satisfied by a key that was never present.
    assert!(
        key_present(fx.state()).expect("ask the store"),
        "the key must be present before forgetting it means anything"
    );

    let removal = forget_key(fx.state()).expect("forgotten");

    assert_eq!(
        removal,
        KeyRemoval::Removed,
        "there was a key, and it is gone"
    );
    assert!(!key_present(fx.state()).expect("ask the store"));
    assert!(
        fx.state()
            .with_index(|db| db.active_space())
            .expect("read")
            .is_some(),
        "removing the key removes the ability to embed, not what was embedded"
    );
}

/// Pressing "Remove the key" with none stored says so, instead of reporting a
/// removal this application did not make.
///
/// **This is the case the test above structurally cannot see**, and says so in
/// its own words: it asserts the key *is* present first, on purpose, so that
/// its later assertion is not satisfied by a key that was never there. That
/// deliberate setup is why eleven review rounds and 45 mutation cases passed
/// over `forget_key` answering `Ok(())` to two different events, and why the
/// window wrote "the key was removed" to somebody who had entered none
/// (whole-branch review, I1).
///
/// It is a state a person reaches by pressing the same button twice, or by
/// pressing it once in a second window.
#[test]
fn removing_a_key_that_is_not_there_says_so_rather_than_reporting_a_removal() {
    let fx = Fixture::with_provider_accepting_everything();

    assert!(
        !key_present(fx.state()).expect("ask the store"),
        "this fixture starts with an empty store, which is the state under test"
    );

    assert_eq!(
        forget_key(fx.state()).expect("removing nothing is not a failure"),
        KeyRemoval::NothingToRemove,
        "nothing was removed, and a window told otherwise tells a person this application \
         deleted a key they never had"
    );

    // Idempotent, and still honest the second time — the press that produced
    // the defect is the second one on a key that was there.
    set_key(fx.state(), KEY.into()).expect("accepted");
    assert_eq!(
        forget_key(fx.state()).expect("forgotten"),
        KeyRemoval::Removed
    );
    assert_eq!(
        forget_key(fx.state()).expect("removing nothing is not a failure"),
        KeyRemoval::NothingToRemove
    );
}

/// The sibling of the test below, on the one provider failure that is not about
/// the provider's answer — and the path that had nothing at all.
///
/// `http.rs:104` is the only place `Error::Transport` is built, `error.rs:144`
/// carries its payload verbatim into `ProviderUnreachable`, and `Serialize` for
/// `Error` is `serialize_str(&self.to_string())`, so that payload is what
/// crosses the IPC. Between those three lines and the window there was no test:
/// `http.rs`'s own five are about timeouts, trust roots and non-2xx bodies, and
/// `src-tauri/src/error.rs` has no test module.
///
/// **Both halves, and the first one is why this exists.** A message that says
/// only "the provider could not be reached" is a true sentence that helps
/// nobody: a refused connection, an unresolved host and a timeout are three
/// different things to do next, and only ureq's own text tells them apart. The
/// absence half alone would be satisfied by exactly that summary — and by an
/// empty string.
///
/// ⚠️ What this does **not** hold, measured rather than assumed (review round 1,
/// F2): the rule on `http.rs`'s `finish` is "`to_string()`, never `Debug`, and
/// never the request", and only the last clause can fire. Swapping `{e}` for
/// `{e:?}` cannot be made to leak anything — both reachable shapes were run with
/// a key in the `authorization` header, giving `Io(Custom { kind:
/// ConnectionRefused, .. })` and `Http(http::Error(InvalidHeaderValue))`, and at
/// the resolved version `InvalidHeaderValue` is `{ _priv: () }`
/// (`http-1.4.2/src/header/value.rs:29-31`), so the rejected value is not stored
/// in the type at all. The key is absent structurally, not by a rendering
/// policy.
///
/// The clause that **can** fire is "never the request", because
/// `ureq::Error::BadUri(String)` prints the URI it was given
/// (`ureq-3.3.0/src/error.rs`). What holds it is
/// `the_role_decides_the_query_and_the_key_travels_in_a_header`, one crate over,
/// which asserts the request line is clean as well as the header being present.
/// Its `POST` sibling asserts the same thing and **cannot be credited for it**:
/// that test pins the whole request line first, so any way a key could reach one
/// trips the endpoint assertion before the key assertion — see its own doc.
#[test]
fn a_provider_that_never_answered_reaches_the_window_with_why_and_without_the_key() {
    let silent = Fixture::with_no_provider_listening();

    let unreachable = set_key(silent.state(), KEY.into()).expect_err("nobody answered");

    let Error::ProviderUnreachable { detail } = &unreachable else {
        panic!(
            "this test means to look at a provider that never answered; it is looking at \
             something else: {unreachable:?}"
        );
    };
    // The positive half, and both of its assertions are about **ureq's** text
    // rather than an operating system's. `contains("connection refused")` stood
    // here and was wrong: that wording is the OS's, true on macOS and Linux and
    // false on Windows, where `WSAECONNREFUSED` renders as "No connection could
    // be made because the target machine actively refused it". A test that goes
    // red on a platform is not a test that catches a defect, and the product
    // ships there.
    //
    // What is portable is the prefix ureq writes itself —
    // `Error::Io(v) => write!(f, "io: {v}")`, read from the version `Cargo.lock`
    // resolves (`ureq-3.3.0/src/error.rs`). Its presence is what says ureq
    // classified this failure and that the classification survived the trip.
    //
    // ⚠️ The assumption this rests on, named rather than buried: a refused
    // connection reaches `Error::Io` on every platform, because ureq's connector
    // maps a failed `TcpStream::connect` to it. That is read from ureq's source,
    // not run on Windows — the stand exists if it is ever worth confirming, and
    // the failure mode is this line going red there rather than anything silent.
    assert!(
        detail.starts_with("io: "),
        "ureq's own classification of the failure must reach the window: a refused \
         connection, an unresolved host and a timeout are three different things to do \
         next, and this is what tells them apart: {detail}"
    );
    // The other half of the pair, and it is not the same assertion: the first is
    // satisfied by any string ureq classified, this by any string that is not
    // simply this layer's own sentence handed back. Derived from the type rather
    // than written as a literal, so rephrasing `ProviderUnreachable` moves both
    // sides at once instead of reddening here.
    let our_own_words = Error::ProviderUnreachable {
        detail: String::new(),
    }
    .to_string();
    let our_own_words = our_own_words.trim_end_matches(": ");
    assert!(
        !detail.contains(our_own_words),
        "the detail must add something to the sentence that wraps it, and this one is \
         that sentence repeated: {detail}"
    );
    let displayed = unreachable.to_string();
    // And the absence half, on the same three renderings the refusal path uses.
    for (shape, rendering) in [
        ("the Display", displayed.clone()),
        ("the Debug", format!("{unreachable:?}")),
        (
            "the JSON that crosses the IPC",
            serde_json::to_string(&unreachable).expect("an error serialises"),
        ),
    ] {
        assert!(
            !rendering.contains(KEY),
            "{shape} of the unreachable-provider error repeats the key: {rendering}"
        );
    }
}

#[test]
fn nothing_that_crosses_to_the_window_repeats_the_key() {
    // A provider repeating the key back inside its own refusal is measured
    // behaviour, and the reason `mnema-provider` redacts at all. The question
    // here is the next one along: whether this layer puts it back on the way to
    // the window, where a rejected command becomes a log line.
    let refusing = Fixture::with_provider_echoing(KEY);

    let refusal = set_key(refusing.state(), KEY.into()).expect_err("the provider refused");

    assert!(
        refusal.to_string().contains("the key was refused"),
        "this test means to look at a refusal of the key itself; it is looking at \
         something else: {refusal}"
    );
    // Three renderings, two distinct strings today: `impl Serialize for Error`
    // is `serialize_str(&self.to_string())` (`src-tauri/src/error.rs`), so the
    // JSON is the `Display` in quotation marks. Both are listed on purpose —
    // this is what holds them the same, and it goes red the day someone
    // replaces that hand-written impl with a derive that reaches the fields.
    for (shape, rendering) in [
        ("the Display", refusal.to_string()),
        ("the Debug", format!("{refusal:?}")),
        (
            "the JSON that crosses the IPC",
            serde_json::to_string(&refusal).expect("an error serialises"),
        ),
    ] {
        assert!(
            !rendering.contains(KEY),
            "{shape} of the refusal repeats the key: {rendering}"
        );
    }

    // The same question on the accepted path, where provider bytes reach the
    // window inside a command's *result* rather than inside an error: a credits
    // body this build cannot read keeps a summary of what the provider said.
    let unreadable = Fixture::with_provider_stating_credits(&format!(
        r#"{{"data":{{"total_credits":"{KEY}","total_usage":1.0}}}}"#
    ));

    let status = set_key(unreadable.state(), KEY.into()).expect("a 200 is accepted");

    let json = serde_json::to_string(&status).expect("a key status serialises");
    assert!(
        json.contains("unreadable"),
        "this half means to look at a balance the build could not read; it is \
         looking at something else: {json}"
    );
    assert!(
        !json.contains(KEY),
        "the key status that crosses to the window repeats the key: {json}"
    );

    // And the settings, which are the other command whose *successful* result
    // carries text this module did not write — `KeyState::Unreadable.reason`
    // comes from the credential store itself, the one place a key is. The rule
    // this follows is stated on `KeyStatus`: a type carrying foreign text goes
    // in this scan and not only the error type does.
    let configured = Fixture::with_provider_accepting_everything();
    configured.open_index();
    set_key(configured.state(), KEY.into()).expect("accepted");

    let settings =
        serde_json::to_string(&model_settings(configured.state())).expect("the settings serialise");
    // The positive control. Every assertion of absence below is satisfied by
    // settings about an installation that has no key at all; this says the key
    // exists to be leaked.
    assert!(
        settings.contains(r#""kind":"present""#),
        "this half means to look at settings whose key is there; it is looking at \
         something else: {settings}"
    );
    assert!(
        !settings.contains(KEY),
        "the settings that cross to the window repeat the key: {settings}"
    );
}

#[test]
fn the_list_of_embedding_models_marks_what_cannot_be_chosen() {
    let fx = Fixture::with_provider_listing(TWO_EMBEDDERS);

    let catalogue = provider_models(fx.state(), "embedding".into()).expect("listed");

    assert_eq!(catalogue.entries.len(), 2, "both are listed");
    assert_eq!(
        catalogue.unreadable, 0,
        "nothing in this fixture is unreadable"
    );
    let refused: Vec<_> = catalogue
        .entries
        .iter()
        .filter(|e| e.refusal.is_some())
        .collect();
    assert_eq!(
        refused.len(),
        1,
        "the 512-token model is listed and marked, not dropped"
    );
    assert_eq!(refused[0].id, "thenlper/gte-base");
}

#[test]
fn an_unknown_role_is_rejected_rather_than_treated_as_chat() {
    let fx = Fixture::with_provider_listing(TWO_EMBEDDERS);

    let refusal = provider_models(fx.state(), "embbeding".into())
        .expect_err("a typo in a role must not silently fetch the four-hundred-model chat list");

    // The variant and not `is_err()`. This fixture has a provider behind it, so
    // `is_err()` is satisfied by a network answer nobody asked about — and the
    // fact under test is that the string was refused *before* any list was
    // fetched at all.
    assert!(
        matches!(refusal, Error::UnknownRole(_)),
        "the role was not refused; something else failed: {refusal:?}"
    );
}

/// Both widths, and that is the point: a single case at 1024 is satisfied by a
/// constant, which is exactly the mistake the measurement exists to prevent
/// (spec §2.4 — the same model name answers 1536 or 1024 depending on a
/// parameter).
#[test]
fn the_recorded_dimension_is_the_one_the_provider_answered_with() {
    for width in [1024usize, 1536] {
        let fx = Fixture::with_provider_answering_with_dimension(width);
        fx.open_index();
        set_key(fx.state(), KEY.into()).expect("accepted");

        let adopted =
            set_embedding_model(fx.state(), MODEL.into(), ExistingVectors::Keep).expect("chosen");

        // What the call says it wrote, and then what the database says it holds.
        // Only the second proves a write happened at all; only the first
        // survives a read-back that fails.
        assert_eq!(adopted.model, MODEL);
        assert_eq!(adopted.dim, width as i64);
        assert!(adopted.created, "this index had no space before the call");

        let read = read_index(&adopted.index);
        assert_eq!(read.embedding_model.as_deref(), Some(MODEL));
        assert_eq!(read.embedding_dim, Some(width as i64));
        assert_eq!(read.active_space, Some(adopted.space_id));
        assert_eq!(
            read.embedded_chunks, 0,
            "an active space says which model the index works with, not that anything is embedded"
        );

        // The one field on this type whose name the camelCase rename changes.
        // `model`, `dim` and `created` are single words and would read the same
        // with the attribute gone, so nothing else here can notice its loss —
        // and a window looking for `spaceId` would get `undefined` in silence.
        let wire = serde_json::to_string(&adopted).expect("the adoption serialises");
        assert!(
            wire.contains(r#""spaceId":"#),
            "the adoption would not reach the window under the name it looks for: {wire}"
        );
    }
}

/// Choosing the same model again finds the space rather than minting one, and
/// `created` is what says so.
///
/// Without this the field is asserted `true` in one place and could be a literal
/// `true` there: `assert!(adopted.created)` above is satisfied by a constant.
#[test]
fn choosing_the_same_model_again_reports_a_space_found_rather_than_created() {
    let fx = Fixture::with_provider_answering_embedding_checks(1024, 2);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("accepted");
    let first =
        set_embedding_model(fx.state(), MODEL.into(), ExistingVectors::Keep).expect("chosen");
    assert!(first.created);

    // A second embedding check, so the second call has an answer of its own.
    let second =
        set_embedding_model(fx.state(), MODEL.into(), ExistingVectors::Keep).expect("chosen again");

    assert!(
        !second.created,
        "the same model twice is one space, and the second call found it"
    );
    assert_eq!(
        second.space_id, first.space_id,
        "and it is the space the first call minted"
    );
}

#[test]
fn the_free_slots_change_without_touching_the_index() {
    let fx = Fixture::with_provider_accepting_everything();
    fx.open_index();

    set_chat_model(fx.state(), "vendor/one".into()).expect("set");
    set_chat_model(fx.state(), "vendor/two".into()).expect("set again");
    set_rerank_model(fx.state(), "vendor/rr".into()).expect("set the other slot");

    let settings = model_settings(fx.state());
    let read = read_index(&settings.index);
    // Both slots, and in this order. Asserting only the one just written is what
    // let the twin command go unproven: `set_rerank_model` writing the chat key
    // would silently discard a chat model already chosen, and a test that reads
    // one slot cannot see it.
    assert_eq!(read.rerank_model.as_deref(), Some("vendor/rr"));
    assert_eq!(
        read.chat_model.as_deref(),
        Some("vendor/two"),
        "choosing a rerank model must leave the chat model where it was"
    );
    assert_eq!(
        read.active_space, None,
        "choosing a chat model creates no space"
    );
}

/// The shape the window actually receives, on both arms.
///
/// Two things nothing else here would notice. The `kind` tag: `IndexSettings`
/// is internally tagged and `Read` is a newtype variant, and serde can flatten
/// such a payload beside the tag only while the payload is itself a map — a
/// payload that is not one compiles and then fails at run time, which is the
/// trap `Balance`'s own doc records from the other side. And the camelCase
/// rename, which the IPC needs and which no assertion on a Rust field can see.
#[test]
fn the_settings_reach_the_window_tagged_and_in_camel_case() {
    let fx = Fixture::with_provider_accepting_everything();

    let closed =
        serde_json::to_string(&model_settings(fx.state())).expect("the settings serialise");
    // Three assertions and not one `&&`, in a file about not folding facts
    // together: joined, a failure says the arm is wrong without saying which of
    // the tag, the discriminant and the sentence was missing.
    for expected in [
        r#""kind":"unreadable""#,
        r#""cause":"notOpen""#,
        r#""reason":"#,
    ] {
        assert!(
            closed.contains(expected),
            "the unreadable arm did not carry {expected}: {closed}"
        );
    }

    fx.open_index();
    let open = serde_json::to_string(&model_settings(fx.state())).expect("the settings serialise");
    for expected in [
        r#""key":{"kind":"absent"}"#,
        r#""kind":"read""#,
        r#""embeddingModel":null"#,
        r#""embeddedChunks":0"#,
        r#""totalChunks":0"#,
        r#""activeSpace":null"#,
    ] {
        assert!(
            open.contains(expected),
            "the window would not find {expected} in {open}"
        );
    }
    // The tag and the payload are siblings, not nested: a `Read` arm that
    // serialised as `{"kind":"read","read":{…}}` would satisfy every assertion
    // above and be a different wire format.
    assert!(
        !open.contains(r#""read":{"#),
        "the payload was nested under the variant name instead of flattened beside the tag: {open}"
    );
}

/// The key is measured before the index is touched, and it must survive the
/// index not being there.
///
/// This is the state the settings screen is opened in: `AppState.db` is `None`
/// until the window calls `open_index`, and an index this build cannot open
/// leaves it `None` for the rest of the session. Folded into one message, the
/// screen tells someone who has a key that they have none.
#[test]
fn a_key_that_is_there_survives_an_index_that_is_not() {
    let fx = Fixture::with_provider_accepting_everything();
    // Deliberately no `open_index`.
    set_key(fx.state(), KEY.into()).expect("accepted");

    let settings = model_settings(fx.state());

    assert!(
        matches!(settings.key, KeyState::Present),
        "the key was measured before the index was consulted, and the measurement was thrown \
         away: {:?}",
        settings.key
    );
    match &settings.index {
        // The **discriminant**, not the sentence. Separating these by
        // `reason.contains("index is not open")` is matching on message text,
        // which `crate::error::Error`'s own header names as the thing it exists
        // to avoid — and it was what this test did until `kind` existed.
        IndexSettings::Unreadable { cause, reason } => {
            assert_eq!(*cause, UnreadableCause::NotOpen);
            // And separately: the sentence is carried through verbatim rather
            // than summarised, which is what makes it worth showing. Compared
            // against the typed error rather than against a literal, so a
            // rephrasing moves both sides at once instead of reddening here.
            assert_eq!(reason, &Error::IndexNotOpen.to_string());
        }
        other => panic!("the index is not open, and the answer says it was read: {other:?}"),
    }
}

/// The mirror of the test above, and the half that fix round 1 left undone: a
/// credential store that will not answer must not take the index reading with
/// it.
///
/// It is the worse direction of the two. When the index half was lost, the
/// window still had `key_present` as a second route to the key; there is no
/// second route to the index half at all — `model_settings` is the only command
/// that carries it.
#[test]
fn a_store_that_will_not_answer_does_not_take_the_index_with_it() {
    let fx = Fixture::with_a_credential_store_that_will_not_answer();
    fx.open_index();
    // Something in the index worth losing. Without it, every assertion below is
    // satisfied by an index that had nothing to report.
    set_chat_model(fx.state(), "vendor/two".into()).expect("the index is open and writable");

    let settings = model_settings(fx.state());

    match &settings.key {
        // The discriminant and the sentence, both typed — the shape the index
        // half already had. `!reason.is_empty()` was what stood here, and it is
        // satisfied by any string at all: the mutation that proved the index
        // half carries its sentence verbatim stayed green on this side of the
        // same struct.
        KeyState::Unreadable { cause, reason } => {
            assert_eq!(
                *cause,
                KeyStoreFailure::Defect,
                "an empty credential reference is this build's own defect, not something the \
                 person at the window can unlock or de-duplicate"
            );
            assert_eq!(reason, &mnema_secrets::Error::EmptyReference.to_string());
        }
        other => panic!(
            "this test needs a store that will not answer, and it got one that did: {other:?}"
        ),
    }
    let read = read_index(&settings.index);
    assert_eq!(
        read.chat_model.as_deref(),
        Some("vendor/two"),
        "a credential store that would not answer swallowed the model configuration, which has \
         nothing to do with a keychain"
    );
    assert_eq!(read.active_space, None);
}

#[test]
fn a_command_that_needs_the_key_says_no_key_rather_than_blaming_the_store() {
    // Two facts about a credential reach a command that needs one: nobody has
    // entered a key, and the store would not answer. Only the first is a normal
    // state of the application, and the panel it opens is a sign-in one. One
    // shape for both tells someone whose keychain is locked to type a key they
    // already have.
    let fx = Fixture::with_provider_accepting_everything();
    // Open the index, so `IndexNotOpen` cannot stand in for the answer this
    // test is about; and check the key is absent, so the refusal below is not
    // satisfied by a fixture that quietly had one.
    fx.open_index();
    assert!(
        !key_present(fx.state()).expect("ask the store"),
        "this test is about a command running without a key; there is one"
    );

    let refusal = set_embedding_model(fx.state(), MODEL.into(), ExistingVectors::Keep)
        .expect_err("a model needs a key to check");

    assert!(
        matches!(refusal, Error::NoKey),
        "a store that would not answer and a key nobody entered are two facts: {refusal:?}"
    );
    // And it stopped before writing. Without this, the assertion above is
    // satisfied by a command that recorded the model and then complained.
    assert_eq!(
        fx.state()
            .with_index(|db| db.active_space())
            .expect("read the index"),
        None,
        "a command that could not run recorded a model anyway"
    );
}

/// A second embedding model, so that "change the model" is a real change rather
/// than the re-adoption `choosing_the_same_model_again_reports_a_space_found`
/// covers. The mock ignores the name; the index mints a second configuration and
/// a second space for it, which is what makes the first one an obstacle.
const OTHER_MODEL: &str = "vendor/other-embedder";

/// How many embeddings the tests below put in the way. Small, and never a round
/// number the code could produce by accident: `0` would satisfy every assertion
/// about loss vacuously, and `1` cannot tell "the count" from "a count".
const EMBEDDED: i64 = 3;

/// The dangerous half of this task. A model change that retires a space is a
/// deliberate act; one that retires a space because the caller did not ask for
/// it is data loss with a confirmation dialog somewhere else entirely.
///
/// Every direction is asserted, because each is separately satisfiable by a
/// build that lost the vectors: the refusal itself, the space row, the vector
/// tables on disk, the count inside the space, and the pointer.
#[test]
fn changing_the_model_without_confirmation_leaves_the_space_alone() {
    let fx = Fixture::with_provider_answering_embedding_checks(1024, 2);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("accepted");
    fx.adopt_default_model();
    let old = fx.embed_chunks_in_the_active_space(EMBEDDED);
    let spaces_before = fx.space_ids();
    let tables_before = fx.tables_of_space(old);
    assert!(
        tables_before.len() > 1,
        "a vec0 table brings four shadow tables; without them here the assertion below that \
         they went would be about nothing: {tables_before:?}"
    );

    let refusal = set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Keep)
        .expect_err("it must refuse, not silently drop");

    assert!(
        matches!(
            refusal,
            Error::Index(mnema_index::Error::SpaceNotEmpty { space_id, embedded_chunks })
                if space_id == old && embedded_chunks == EMBEDDED
        ),
        "the refusal has to name the space in the way and what it holds, since that number is \
         what the window puts in front of the person: {refusal:?}"
    );
    assert_eq!(
        fx.space_ids(),
        spaces_before,
        "a refused change removed a space"
    );
    assert_eq!(
        fx.tables_of_space(old),
        tables_before,
        "the row survived a refusal and its tables did not"
    );
    assert_eq!(
        fx.embedded_chunks_in(old),
        EMBEDDED,
        "the space survived a refusal and was emptied"
    );
    assert_eq!(
        fx.active_space(),
        Some(old),
        "a refused change moved the index off the space it refused to leave"
    );
}

/// The other half: asked for plainly, the change happens, and the space it cost
/// is gone in full.
///
/// The third assertion is the one this test is written for. A row deleted with
/// its `vec0` table left behind is a leak nothing reports: the space is not in
/// `embedding_space`, so nothing counts it, nothing lists it, and its vectors
/// and four shadow tables sit on the disk of somebody who was told the old model
/// had been retired.
#[test]
fn a_confirmed_model_change_retires_the_old_space_and_its_tables() {
    let fx = Fixture::with_provider_answering_embedding_checks(1024, 2);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("accepted");
    fx.adopt_default_model();
    let old = fx.embed_chunks_in_the_active_space(EMBEDDED);

    let adopted = set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Discard)
        .expect("a confirmed change");

    assert_ne!(
        Some(old),
        fx.active_space(),
        "the index is still on the space the change was supposed to leave"
    );
    assert_eq!(
        fx.active_space(),
        Some(adopted.space_id),
        "the call named one space and the index is on another"
    );
    assert!(
        !fx.space_ids().contains(&old),
        "the space row is still there: {:?}",
        fx.space_ids()
    );
    assert_eq!(
        fx.tables_of_space(old),
        Vec::<String>::new(),
        "the row went and the vec0 tables stayed — the disk is still full"
    );
    // Both directions. Without this, a build that dropped every space would
    // satisfy everything above.
    assert!(
        fx.space_ids().contains(&adopted.space_id),
        "the new space was not created, so nothing can be embedded into it: {:?}",
        fx.space_ids()
    );
    assert_eq!(
        fx.embedded_chunks_in(adopted.space_id),
        0,
        "the new space is to be counted again from nothing"
    );

    // What it cost, stated by the call rather than left for the window to infer
    // from a number it read before the act.
    assert_eq!(
        adopted.retired,
        vec![mnema_desktop::models::RetiredSpace {
            space_id: old,
            embedded_chunks: EMBEDDED,
        }],
        "the answer does not name what it destroyed"
    );
    // And under the names the window looks for. `spaceId` and `embeddedChunks`
    // are the two the camelCase rename changes, and a window reading a renamed
    // field gets `undefined` in silence — the same defect the `spaceId`
    // assertion in `the_recorded_dimension_is_the_one_the_provider_answered_with`
    // exists for.
    let wire = serde_json::to_string(&adopted).expect("the adoption serialises");
    assert!(
        wire.contains(&format!(
            r#""retired":[{{"spaceId":{old},"embeddedChunks":{EMBEDDED}}}]"#
        )),
        "what was destroyed would not reach the window under the names it reads: {wire}"
    );
}

/// Confirmation is permission to remove what is **in the way**, and re-adopting
/// the model the index is already on has nothing in the way.
///
/// The shortcut this excludes is one line long and reads as the obvious
/// implementation: on confirmation, drop `active_space` and then adopt. It
/// destroys an archive for a call that by contract moves nothing.
///
/// **Not a hypothetical about how a window might behave — the index states it.**
/// `Db::refuse_if_the_move_would_orphan_anything`
/// (`crates/mnema-index/src/space.rs:594-602`) exempts a call whose destination
/// is where the pointer already stands, precisely because it is not a transition
/// and there is nothing for a guard on transitions to decide. So re-adopting the
/// recorded model is a call the index promises will leave the database as it
/// found it, and a confirmation flag turning that into a deletion breaks the
/// promise rather than a convention.
///
/// ⚠️ An earlier version of this comment justified the test by saying this
/// command is how a new API key is recorded. That is false and was corrected in
/// review: the key goes to the OS credential store through `set_key` /
/// `mnema_secrets::store`, and `model_config.credential_ref` holds only the
/// *name* the credential is filed under — a constant this window never changes.
/// Adoption is indeed the one path that writes that column; writing a name is
/// not recording a key.
#[test]
fn a_confirmed_change_to_the_model_the_index_is_already_on_retires_nothing() {
    let fx = Fixture::with_provider_answering_embedding_checks(1024, 2);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("accepted");
    fx.adopt_default_model();
    let space = fx.embed_chunks_in_the_active_space(EMBEDDED);
    let tables_before = fx.tables_of_space(space);

    let adopted = set_embedding_model(fx.state(), MODEL.into(), ExistingVectors::Discard)
        .expect("re-adopting the recorded model is not a move and is allowed");

    // Ordered so the first assertion to fire is the one about loss. Asking
    // `embedded_chunks_in` about a space that has been dropped raises
    // `NoSuchSpace` inside the fixture, which is red as a panic from a helper
    // rather than red as a sentence about what was destroyed — measured, by
    // running exactly the shortcut this test excludes.
    assert!(
        fx.space_ids().contains(&space),
        "the space holding the embeddings was retired by a call that changed no model: {:?}",
        fx.space_ids()
    );
    assert_eq!(
        fx.embedded_chunks_in(space),
        EMBEDDED,
        "the embeddings were thrown away by a call that changed no model"
    );
    assert_eq!(
        fx.tables_of_space(space),
        tables_before,
        "the count survived and the tables behind it did not"
    );
    assert_eq!(
        adopted.retired,
        Vec::new(),
        "confirmation retired a space that was not in the way"
    );
    assert_eq!(adopted.space_id, space, "re-adoption minted a second space");
}

/// The settings carry three different numbers about embeddings, and no two of
/// them may collapse into one.
///
/// **`embedded_chunks_everywhere` is the number the window's confirmation stands
/// on** (review 2). `embedded_chunks` counts the active space; the command
/// retires every space in the way; and a space abandoned by an earlier model
/// change is still there holding whatever it held, because
/// `Db::adopt_embedding_model` mints and repoints and never removes what it
/// moved off. So a button naming the active space's count understates the bill
/// by exactly the spaces it forgot.
///
/// The fixture makes the three numbers **pairwise different** — 3 in the active
/// space, 2 in another, 2 spaces — so that a build reading any one of them in
/// place of another is caught rather than accidentally right. A fixture with
/// equal halves is the shape that lets that through.
///
/// A second space is built through `Db::create_model_config` and
/// `Db::create_space` rather than through a second adoption, because a second
/// adoption is exactly what the index refuses in this state — and that refusal
/// is what these numbers exist to describe.
#[test]
fn the_settings_tell_the_active_space_apart_from_the_whole_index() {
    let fx = Fixture::with_provider_answering_embedding_checks(1024, 1);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("accepted");
    fx.adopt_default_model();
    let active = fx.embed_chunks_in_the_active_space(EMBEDDED);

    let before = model_settings(fx.state());
    let before = read_index(&before.index);
    assert_eq!(before.space_count, 1);
    assert_eq!(
        before.embedded_chunks_everywhere, EMBEDDED,
        "with one space the two counts agree, which is why the second space below is needed"
    );

    // Full, and holding fewer than the active one, so no assertion here can be
    // satisfied by the wrong number.
    const ELSEWHERE: i64 = 2;
    let second = fx
        .state()
        .with_index(|db| {
            let config = db.create_model_config("other", "openrouter", None, OTHER_MODEL, 1024)?;
            let space = db.create_space(config, 1024, &mnema_chunk::chunker_hash())?;
            for chunk in 1..=ELSEWHERE {
                db.insert_vector(space, 1000 + chunk, &vec![0.5f32; 1024])?;
            }
            Ok(space)
        })
        .expect("a second space is created");
    assert_ne!(second, active, "the fixture built one space twice");

    let settings = model_settings(fx.state());
    let after = read_index(&settings.index);
    assert_eq!(
        after.embedded_chunks, EMBEDDED,
        "this one counts the active space alone"
    );
    assert_eq!(
        after.embedded_chunks_everywhere,
        EMBEDDED + ELSEWHERE,
        "and this one counts the index — a confirmed change retires the second space too, and a \
         window told only the first number offers to delete less than it will"
    );
    assert_eq!(
        after.space_count, 2,
        "and this one counts spaces, not either"
    );
    assert_eq!(after.active_space, Some(active), "the pointer did not move");
}

/// Trying a second model and going back leaves two spaces **for the life of the
/// index**, and that is the ordinary state rather than a corner of it.
///
/// This is the regression test for what review round 2 found: the confirmation
/// was gated on there being exactly one space, and after this sequence — three
/// presses on a settings screen, before anything is indexed — there are two,
/// permanently. `Db::adopt_embedding_model` mints and repoints and never removes
/// what it moved off, and the only production caller of `Db::drop_space` is the
/// confirmed change itself, so nothing else can bring the count back down.
///
/// **The number was never the problem, which is the part worth pinning.** An
/// abandoned space is empty and contributes nothing, so
/// `embedded_chunks_everywhere` equals the active space's count here — the state
/// that hid the button is a state in which the button's number was already
/// right. Both are asserted, because the claim is about them agreeing.
#[test]
fn trying_a_second_model_leaves_a_space_behind_and_it_never_goes_away() {
    // Four embedding checks: three adoptions and the refused attempt at the end,
    // which reaches the provider before it reaches the index. One short and that
    // last call gets the mock's `599` sentinel, and the refusal this test is
    // about is replaced by one about a provider — measured, on the first run.
    let fx = Fixture::with_provider_answering_embedding_checks(1024, 4);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("accepted");

    // Three presses on the settings screen, all allowed: nothing is embedded
    // yet, so nothing blocks a switch in either direction.
    let first = set_embedding_model(fx.state(), MODEL.into(), ExistingVectors::Keep)
        .expect("the first model");
    set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Keep)
        .expect("a second model, while nothing is embedded");
    let back = set_embedding_model(fx.state(), MODEL.into(), ExistingVectors::Keep)
        .expect("and back to the first");
    assert_eq!(
        back.space_id, first.space_id,
        "going back found the space the first press minted"
    );

    fx.embed_chunks_in_the_active_space(EMBEDDED);

    let settings = model_settings(fx.state());
    let read = read_index(&settings.index);
    assert_eq!(
        read.space_count, 2,
        "the abandoned space is still there, and nothing but a confirmed change removes it"
    );
    assert_eq!(
        read.embedded_chunks_everywhere, EMBEDDED,
        "and it holds nothing, so the number a confirmation would name is unaffected by it — \
         which is why the count of spaces must not decide whether that confirmation is offered"
    );

    // And the state is exactly the one the whole task exists for: a further
    // change is refused, so without a reachable confirmation there is no way to
    // change the model at all from here.
    let refusal = set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Keep)
        .expect_err("a third model, now that something is embedded");
    assert!(
        matches!(
            refusal,
            Error::Index(mnema_index::Error::SpaceNotEmpty { embedded_chunks, .. })
                if embedded_chunks == EMBEDDED
        ),
        "this test's premise is that the index refuses here: {refusal:?}"
    );
}

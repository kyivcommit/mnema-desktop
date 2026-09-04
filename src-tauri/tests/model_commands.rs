//! The key crosses three boundaries — window, credential store, provider — and
//! must not cross a fourth into the database.

/// Beside `support/mod.rs` rather than inside it — see that file's own header
/// for why a shared module cannot hold something only one binary uses.
#[path = "support/fixture.rs"]
mod fixture;
mod support;

use std::sync::mpsc;
use std::time::Duration;

use fixture::{Fixture, vectors_for};
use mnema_desktop::bridge;
use mnema_desktop::embed_job::start_embed_job;
use mnema_desktop::error::Error;
use mnema_desktop::job::JobEvent;
use mnema_desktop::models::{
    DEFAULT_MODELS, ExistingVectors, IndexRead, IndexSettings, KeyRemoval, KeyState,
    KeyStoreFailure, UnreadableCause, forget_key, key_present, model_settings, provider_models,
    set_chat_model, set_embedding_model, set_key, set_rerank_model,
};
use mnema_mock_provider::Reply;
use serde_json::{Value, json};
use tauri::ipc::Channel;

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

/// The model the fixture's provider answers an embedding check for — **this
/// product's own default**, which is the model an index is on once a key has
/// been entered. The mock ignores the name; the index does not.
///
/// It was a literal of its own until `set_key` began applying `DEFAULT_MODELS`.
/// Left as one, every test below that says "the model the index is already on"
/// would have named a model the index was *not* on, and the tests that assert a
/// space was minted would have been asserting about a second one.
const MODEL: &str = DEFAULT_MODELS.embedding;

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
    // Two embedding checks, not the usual one: `set_key` spends the first
    // adopting the default model, and this test spends the second on the write
    // it is here to watch. One short and the mock answers its `599` sentinel.
    let fx = Fixture::with_provider_accepting_everything_for(2);
    fx.open_index();

    set_key(fx.state(), KEY.into()).expect("accepted");
    fx.adopt_default_model();

    // Adopted here rather than left to the fixture, and that is the whole
    // repair. `adopt_default_model` used to call `set_embedding_model`; once
    // `set_key` began choosing the defaults itself, the helper became a read
    // and an assertion — correct for what it now says, and it took this test's
    // only pass through the write with it. The mutation this test exists to
    // catch replaces the model name with the key in exactly that write, so a
    // test that never reaches the write cannot see it, and did not: the case
    // stood green while the guard it names had stopped running.
    set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Keep)
        .expect("nothing is embedded yet, so keeping the vectors cannot refuse");

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
/// `http.rs:104` is the only place `Error::Transport` is built, `error.rs:401`
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
        // Two checks: the one `set_key`'s own default adoption makes, and this
        // test's own. `OTHER_MODEL` and not `MODEL`, because `MODEL` is now the
        // model that adoption already put the index on — re-choosing it would
        // find that space instead of minting one, and `created` below would be
        // asserting the opposite of what it says.
        let fx = Fixture::with_provider_answering_embedding_checks(width, 2);
        fx.open_index();
        set_key(fx.state(), KEY.into()).expect("accepted");

        let adopted = set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Keep)
            .expect("chosen");

        // What the call says it wrote, and then what the database says it holds.
        // Only the second proves a write happened at all; only the first
        // survives a read-back that fails.
        assert_eq!(adopted.model, OTHER_MODEL);
        assert_eq!(adopted.dim, width as i64);
        assert!(
            adopted.created,
            "this index had no space for this model before the call"
        );

        let read = read_index(&adopted.index);
        assert_eq!(read.embedding_model.as_deref(), Some(OTHER_MODEL));
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
    // Three checks: `set_key`'s own default adoption, and this test's two.
    // `OTHER_MODEL`, because the first call must be the one that MINTS the space
    // — with `MODEL` it would find the one the default adoption already made,
    // and `first.created` would be false before the second call ever ran.
    let fx = Fixture::with_provider_answering_embedding_checks(1024, 3);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("accepted");
    let first =
        set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Keep).expect("chosen");
    assert!(first.created);

    // A third embedding check, so the second call has an answer of its own.
    let second = set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Keep)
        .expect("chosen again");

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
        r#""searchTextArm":true"#,
        r#""searchContentArm":true"#,
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
/// until something opens the index — in the application that is the boot
/// (`boot_index`, called from `.setup`), never the window, which is why this
/// fixture opens nothing and reaches the same `None`. Folded into one message,
/// the screen tells someone who has a key that they have none.
///
/// The `NotOpen` below is still the right expectation *here* and would be the
/// wrong one in the product: a failed boot open is recorded and reaches the
/// window as `ReadFailed`. Nothing in this fixture boots, so nothing recorded
/// anything — which is exactly the mirror
/// `a_failed_boot_open_reaches_the_window_as_read_failed` asserts from the
/// other side, in `tests/commands.rs`.
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
    // Five embedding checks: the one `set_key`'s own default adoption makes,
    // three presses, and the refused attempt at the end, which reaches the
    // provider before it reaches the index. One short and that last call gets
    // the mock's `599` sentinel, and the refusal this test is about is replaced
    // by one about a provider — measured, on the first run.
    let fx = Fixture::with_provider_answering_embedding_checks(1024, 5);
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

// ─────────────────────────────────────────────────────────────────────────────
// The embedding job: the one command that takes the key out of the store, sends
// it to the provider, and writes what comes back into the index.
//
// It is in this file rather than in `tests/commands.rs` because it needs all
// three of the things this file's fixture builds and that one's does not: a
// credential store nobody's real keychain is behind, a provider that answers,
// and an index with a model adopted through the command the window uses.

/// Collects what the webview would receive on a job channel.
///
/// The same helper `tests/commands.rs` has, copied rather than moved into
/// `support/mod.rs` for the reason that file's own header gives: Cargo compiles
/// a shared module into every binary that declares it, so an item only two of
/// the six want is dead code in the other four, and `-D warnings` makes that an
/// error rather than a note.
fn job_channel() -> (Channel<JobEvent>, mpsc::Receiver<Value>) {
    let (tx, rx) = mpsc::channel();
    let channel = Channel::new(move |body| {
        let json: Value = body.deserialize().expect("the job event was not JSON");
        let _ = tx.send(json);
        Ok(())
    });
    (channel, rx)
}

/// Every progress report the window was shown, and the ending — which always
/// arrives, however the job ended, and is what this returns rather than a
/// timeout.
fn events_until_ended(events: &mpsc::Receiver<Value>) -> (Vec<Value>, Value) {
    let mut progress = Vec::new();
    loop {
        match events.recv_timeout(Duration::from_secs(20)) {
            Ok(event) if event["event"] == json!("progress") => {
                progress.push(event["data"].clone())
            }
            Ok(event) if event["event"] == json!("ended") => {
                return (progress, event["data"].clone());
            }
            Ok(other) => panic!("the job sent something that is neither: {other}"),
            Err(_) => panic!("the job never told the window it ended"),
        }
    }
}

/// Waits for the job slot to come free, so a test that starts a second job is
/// not racing the first one's own thread.
fn wait_for_the_slot(fx: &Fixture) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while fx.state().job_is_running() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !fx.state().job_is_running(),
        "the job never released the slot"
    );
}

/// One job at a time is the model, and `AppState::running` is one flag for the
/// whole application — so embedding cannot run beside a folder walk, and this is
/// the command being made to say so rather than being trusted to.
///
/// **Both directions.** A command that refused every call would satisfy the
/// first half on its own, and this is a project that has measured that exact
/// shape nine times in one branch. So the same call is made again once the slot
/// is free, and it has to be accepted.
#[test]
fn embedding_refuses_to_start_while_another_job_runs() {
    let fx = Fixture::with_provider_accepting_everything();
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("the key is accepted");

    // A real job in the slot, taken by the command that takes it — not
    // `claim_job` called by hand, which would prove this refuses a flag rather
    // than that it refuses a job.
    let (probe, _probe_events) = job_channel();
    bridge::start_probe_job(fx.state(), probe).expect("the probe job starts");

    let (channel, _events) = job_channel();
    let refusal = start_embed_job(fx.state(), channel)
        .expect_err("embedding started while a job was already running");
    assert!(
        matches!(refusal, Error::JobAlreadyRunning),
        "the refusal must name the slot, not something else: {refusal:?}"
    );

    bridge::cancel_job(fx.state());
    wait_for_the_slot(&fx);

    let (channel, events) = job_channel();
    start_embed_job(fx.state(), channel)
        .expect("embedding was still refused with nothing else running");
    // Nothing is adopted in this test, so the run ends at once — waited out
    // rather than left running, because the fixture's temporary directory is
    // about to go and the job holds a connection to a database inside it.
    events_until_ended(&events);
}

/// The store is asked **before** the slot is taken, and the refusal a person
/// gets says so.
///
/// It is `walk_job::start_walk_job`'s own rule — every fallible step before
/// `claim_job`, so that a call which was always going to fail never has
/// `job_status` reporting a job that is running — and it has more force here
/// than there. `mnema_secrets::load` can block on a modal authorisation dialog:
/// on macOS the store authorises against the code identity that wrote the
/// credential, and an ad-hoc signature changes with every build
/// (`KeyStoreFailure::Locked`'s own doc comment, measured 2026-08-11). Claiming
/// the slot first would disable Start, and refuse a walk, for as long as that
/// dialog sits unanswered on somebody's screen.
///
/// So the order is observable, and this is what observes it: with a job already
/// running **and** no key, the answer names the key.
#[test]
fn the_key_is_read_before_the_job_slot_is_taken() {
    let fx = Fixture::with_provider_accepting_everything();
    fx.open_index();

    let (probe, _probe_events) = job_channel();
    bridge::start_probe_job(fx.state(), probe).expect("the probe job starts");

    let (channel, _events) = job_channel();
    let refusal =
        start_embed_job(fx.state(), channel).expect_err("a job is running and there is no key");
    assert!(
        matches!(refusal, Error::NoKey),
        "the slot was claimed before the store was asked: {refusal:?}"
    );
    // The sentence, not only the variant: this type crosses the IPC as its
    // `Display` string, and `Error::NoKey`'s own doc comment is about what it
    // tells the person to do next.
    let said = refusal.to_string();
    assert!(
        !said.contains("already running"),
        "a person with no key was told to wait for a job instead: {said}"
    );

    bridge::cancel_job(fx.state());
    wait_for_the_slot(&fx);
}

/// A missing key is a refusal a person can read, not a panic — and the slot is
/// left free, so nothing is blocked by a call that never started anything.
#[test]
fn embedding_without_a_key_is_refused_rather_than_panicking() {
    let fx = Fixture::with_provider_accepting_everything();
    fx.open_index();

    let (channel, _events) = job_channel();
    let refusal = start_embed_job(fx.state(), channel).expect_err("there is no key to embed with");

    assert!(matches!(refusal, Error::NoKey), "{refusal:?}");
    assert!(
        !fx.state().job_is_running(),
        "a refused start left the job slot taken, and nothing can index until a restart"
    );
    assert!(
        fx.provider_request().is_none(),
        "a request left the machine for a key nobody has entered"
    );
}

/// The whole path, from the press to the number on the settings screen: the key
/// comes out of the store, the queue comes out of the index, the vectors go back
/// into it, and what the window is shown agrees with what the database holds.
#[test]
fn a_run_started_from_the_window_embeds_the_queue_and_reports_what_it_did() {
    const CHUNKS: usize = 3;
    let fx = Fixture::with_provider_answering_a_run(vec![Reply::ok(&vectors_for(CHUNKS))]);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("the key is accepted");
    fx.adopt_default_model();
    let space = fx.active_space().expect("a model was adopted");
    fx.write_indexed_chunks(CHUNKS);

    let (channel, events) = job_channel();
    start_embed_job(fx.state(), channel).expect("the embedding job starts");
    let (progress, ending) = events_until_ended(&events);

    assert_eq!(ending["reason"], json!("completed"), "{ending}");
    assert_eq!(ending["done"], json!(CHUNKS), "{ending}");
    assert_eq!(ending["total"], json!(CHUNKS), "{ending}");
    assert_eq!(ending["refused"], json!(0), "{ending}");

    // The bar moved, and the last thing it was shown is the truth rather than
    // whatever the throttle last let through. Both halves: an empty list also
    // satisfies "no report disagreed with the ending".
    assert!(
        !progress.is_empty(),
        "the window was shown no progress at all, so the bar never moved"
    );
    let last = progress.last().expect("the assertion above found one");
    assert_eq!(
        last["done"], ending["done"],
        "the last progress report the window saw is short of the ending: {last}"
    );

    // What the index actually holds, which is the only thing that makes the
    // numbers above worth anything.
    assert_eq!(
        fx.embedded_chunks_in(space),
        CHUNKS as i64,
        "the run reported vectors the index does not have"
    );
    let settings = model_settings(fx.state());
    let read = read_index(&settings.index);
    assert_eq!(read.embedded_chunks, CHUNKS as i64);
    assert_eq!(read.total_chunks, CHUNKS as i64);
    assert_eq!(
        read.failed_chunks, 0,
        "nothing was refused, and the screen must say that rather than nothing"
    );
}

/// **The load-bearing test of this task.** A chunk the provider refuses leaves
/// the embedding queue and is not offered again until its text changes — so
/// vector search stops answering for it while the document still shows it and
/// lexical search still finds it. That is only defensible while the count is in
/// front of a person, and `Db::failed_chunk_count` had no production caller at
/// all before this: the whole no-retry rule stood on a number nothing read.
///
/// The refusal is a `400`, which is one of the three statuses
/// `mnema_embed::speaks_only_about_these_texts` will attribute to a text at all,
/// and the batch is split one text at a time — so the first chunk embeds, which
/// is the corroboration that lets the second one be written down as refused
/// rather than blamed on a provider having a bad minute.
#[test]
fn a_chunk_the_provider_refused_is_counted_where_a_person_can_read_it() {
    const CHUNKS: usize = 2;
    let fx = Fixture::with_provider_answering_a_run(vec![
        // The batch of two, refused as a whole. `embed` is all-or-nothing over
        // the batch it was given, so the answer names no particular text.
        Reply::status(400, r#"{"error":{"message":"input is too long"}}"#),
        // The split: the first text alone succeeds…
        Reply::ok(&vectors_for(1)),
        // …and the second alone is refused, which is now evidence about it
        // rather than about the provider.
        Reply::status(400, r#"{"error":{"message":"input is too long"}}"#),
    ]);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("the key is accepted");
    fx.adopt_default_model();
    let space = fx.active_space().expect("a model was adopted");
    fx.write_indexed_chunks(CHUNKS);

    let (channel, events) = job_channel();
    start_embed_job(fx.state(), channel).expect("the embedding job starts");
    let (_progress, ending) = events_until_ended(&events);

    assert_eq!(
        ending["refused"],
        json!(1),
        "the ending does not say a chunk was given up on: {ending}"
    );
    assert_eq!(ending["done"], json!(1), "{ending}");
    assert_eq!(ending["total"], json!(CHUNKS), "{ending}");

    // The number on the settings screen, which is the one that outlives this
    // run's channel and the one the no-retry rule is justified by.
    let settings = model_settings(fx.state());
    let read = read_index(&settings.index);
    assert_eq!(
        read.failed_chunks, 1,
        "the third number never reached the window, so a chunk left vector search in silence"
    );
    // And it agrees with the database rather than with the run's own tally:
    // `Db::failed_chunk_count` counts a refusal that is still about the chunk's
    // current text, and this asks it through the command the window calls.
    assert_eq!(
        fx.state()
            .with_index(|db| db.failed_chunk_count(space))
            .expect("the index answers"),
        read.failed_chunks,
        "the screen and the database disagree about how many chunks were refused"
    );
    assert_eq!(
        read.embedded_chunks, 1,
        "a refused chunk must not be counted as an embedded one"
    );
    assert_eq!(read.total_chunks, CHUNKS as i64);
}

/// The embedding queue, counted the way the pass itself counts it —
/// `Db::queued_chunk_count`, not `total_chunks − embedded_chunks −
/// failed_chunks` (F4, spec §9.3 amended 2026-09-04). A space carrying one of
/// each state — embedded, failed, and untouched — is what tells the two
/// apart: the subtraction would land on the same integer here by coincidence,
/// since one embedded, one failed and two untouched out of four also happens
/// to be `4 − 1 − 1`. What distinguishes them is `total_chunks` and
/// `failed_chunks` asserted on their own terms below, not only `pending_chunks`.
#[test]
fn pending_chunks_counts_the_queue_a_run_still_owes() {
    let fx = Fixture::with_provider_accepting_everything();
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("the key is accepted");
    fx.adopt_default_model();
    let space = fx.active_space().expect("a model was adopted");
    let ids = fx.write_indexed_chunks(4);

    let dim = {
        let settings = model_settings(fx.state());
        read_index(&settings.index)
            .embedding_dim
            .expect("a model was adopted") as usize
    };
    fx.state()
        .with_index(|db| {
            db.insert_vector(space, ids[0], &vec![1.0_f32; dim])?;
            db.record_embedding_failure(space, ids[1])?;
            Ok(())
        })
        .expect("the index answers");
    // `ids[2]` and `ids[3]` are left untouched: indexed, never embedded, never
    // failed — the two chunks a run still owes.

    let settings = model_settings(fx.state());
    let read = read_index(&settings.index);
    assert_eq!(
        read.pending_chunks, 2,
        "the two untouched chunks are what the queue still owes: {read:?}"
    );
    assert_eq!(read.embedded_chunks, 1);
    assert_eq!(read.failed_chunks, 1);
    assert_eq!(read.total_chunks, 4);
}

/// `0` with `active_space == None` is "the question does not arise", the same
/// way it is for `embedded_chunks` and `failed_chunks` beside it — told apart
/// by `active_space`, never by the zero.
#[test]
fn pending_chunks_is_zero_with_no_active_space() {
    let fx = Fixture::with_provider_accepting_everything();
    fx.open_index();

    let settings = model_settings(fx.state());
    let read = read_index(&settings.index);
    assert_eq!(read.active_space, None, "this test needs no model chosen");
    assert_eq!(read.pending_chunks, 0);
}

/// A person who has entered a key but chosen no model gets a sentence, not a
/// silence and not a panic. The pass reads `meta.active_space` itself and
/// refuses; this is that refusal reaching the window as an ending it can render.
///
/// **The key goes into the store directly, and that is the only way this state
/// can still be built.** `set_key` applies `DEFAULT_MODELS` to every role the
/// index has none for, so a key entered through the command on an open index
/// leaves an embedding model behind it every time. The state is still reachable
/// in the product, and this is exactly its shape: a key entered while the index
/// was not open — a start-up open that failed, and a person who typed their key
/// into the settings screen it left them on — and an index opened later. What
/// `set_key` would otherwise contribute here is a credit check this test does
/// not measure, so nothing is lost by not making it.
#[test]
fn a_run_with_no_model_chosen_ends_with_a_message_saying_so() {
    let fx = Fixture::with_provider_accepting_everything();
    fx.open_index();
    mnema_secrets::store(fx.credential_ref(), KEY).expect("the key goes into the store");
    // Nothing has spoken to the provider yet, which is what makes the assertion
    // at the end — that the job never called out — about this job.
    assert!(
        fx.provider_request().is_none(),
        "something called the provider before the job did, so the check at the end is about it"
    );

    let (channel, events) = job_channel();
    start_embed_job(fx.state(), channel).expect("the job starts, and then fails on its own");
    let (_progress, ending) = events_until_ended(&events);

    assert_eq!(ending["reason"], json!("failed"), "{ending}");
    let message = ending["message"]
        .as_str()
        .expect("a failed job must carry a message the window can render");
    assert!(
        message.contains("no embedding model"),
        "the ending does not say what is missing: {message}"
    );
    assert!(
        fx.provider_request().is_none(),
        "a request left the machine before there was anywhere to put the answer"
    );
}

/// A model change is refused while a job holds the slot, and the configuration
/// is exactly where it was afterwards.
///
/// Deterministic: the probe holds the slot for ten seconds and makes no provider
/// call at all, so there is no race to lose and nothing queued behind anything.
///
/// **The state, not only the variant.** A test that checked `JobAlreadyRunning`
/// and stopped would pass the day somebody changed which error this returns, and
/// would say nothing about what the refusal is for. What it is for is that
/// `meta.active_space` does not move and no space is minted while a pass is
/// reading that pointer.
#[test]
fn a_model_change_is_refused_while_a_job_holds_the_slot() {
    // Two embedding checks, although only one adoption is meant to happen: the
    // second is what the refused change *would* make, and without it a build
    // with the claim removed is stopped by the mock running out of replies
    // rather than by anything in the product. Measured — that is exactly what
    // happened on the first run of this test, and it would have been written up
    // as a guard that works.
    let fx = Fixture::with_provider_answering_embedding_checks(1024, 2);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("the key is accepted");
    fx.adopt_default_model();
    let before = fx.active_space().expect("a model was adopted");
    let spaces_before = fx.space_ids();

    // The two calls the setup above made — the credit check and one embedding
    // check — taken off the queue and asserted rather than discarded, so that
    // the assertion further down is about this refusal and not about a queue
    // that was empty for some other reason.
    for expected in [
        "the credit check `set_key` made",
        "the embedding check the adoption made",
    ] {
        assert!(
            fx.provider_request().is_some(),
            "{expected} never arrived, so the check below proves nothing"
        );
    }

    let (probe, _probe_events) = job_channel();
    bridge::start_probe_job(fx.state(), probe).expect("the probe job starts");

    let outcome = set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Keep);

    // **The state before the variant**, and in that order deliberately: an
    // `expect_err` first would panic on a change that succeeded and this test
    // would never say what the success did to the index, which is the whole
    // subject. What the refusal is for is that `meta.active_space` does not move
    // and no space is minted while a pass is reading that pointer.
    assert_eq!(
        fx.active_space(),
        Some(before),
        "the index was repointed under a running job"
    );
    assert_eq!(
        fx.space_ids(),
        spaces_before,
        "a space was minted while a job was running"
    );

    let refusal = outcome.expect_err("the model was changed while a job was running");
    assert!(
        matches!(refusal, Error::JobAlreadyRunning),
        "the refusal must name the slot: {refusal:?}"
    );
    // And no provider call was made for the refused change: the claim is ahead
    // of the check, so a refusal costs nothing and reaches the person at once
    // rather than after a round trip. The fixture's own two calls — the credit
    // check and one embedding check — are already off the queue by now.
    assert!(
        fx.provider_request().is_none(),
        "a request left the machine for a change that was never going to happen"
    );

    bridge::cancel_job(fx.state());
    wait_for_the_slot(&fx);
}

/// **The property, not the mechanism: a run's vectors are in the space the index
/// points at, and there are none anywhere else.**
///
/// This is what the claim in `set_embedding_model` is for. The mechanism —
/// "the command refuses while a job runs" — is the test above; this one attempts
/// the change against a pass that is genuinely in flight and then asks the index
/// the question that matters, which is where the vectors ended up.
///
/// The run is held open by a provider that answers correctly but late, so the
/// window is two seconds rather than a race.
///
/// ⚠️ **Say what this reaches and what it does not.** Against a build with the
/// claim removed, the timing decides which assertion fires. Both are this fix's
/// own subject matter, and neither is a fixture artefact — the replies below are
/// counted so that the mock never runs out and refuses on the product's behalf:
///
/// - The change reaches the index while the active space is **empty** — the run
///   has not written yet — and is not refused: it repoints, and the run's
///   vectors land in a space nothing points at. The two assertions on
///   `embedded_chunks_everywhere` and `active_space` catch that, and nothing
///   else in this suite would.
/// - The change reaches the index **after** the batch landed, and
///   `Db::refuse_unless_every_other_space_is_empty` refuses it as
///   `SpaceNotEmpty`. The variant assertion catches that, and it is a
///   neighbouring defence — named as one, because it protects the non-empty case
///   only, which is exactly why the empty case needed a guard of its own.
///
/// **Both have been measured with the claim removed, on this machine**, which is
/// worth knowing before anybody trusts one of them: first as
/// `Index(SpaceNotEmpty { space_id: 1, embedded_chunks: 3 })` — the run's three
/// writes landing before the change reached the index — and then, on a later
/// run of the harness, as `the index was repointed while a pass was writing into
/// the space it named`, `left: Some(2), right: Some(1)`. The order the two
/// finish in after the mock's delay expires is not fixed, so this test reddens
/// through whichever it gets, and the pair of assertions is why it reddens
/// either way rather than intermittently.
///
/// The test above is the one that fires on its *state* assertion every time:
/// there the job is a probe, which writes nothing, so nothing can make the space
/// non-empty and nothing but the claim stands between the change and the
/// pointer.
#[test]
fn a_run_leaves_no_vectors_in_a_space_nothing_points_at() {
    const CHUNKS: usize = 3;
    let fx = Fixture::with_provider_answering_a_run(vec![
        // The run's one batch, answered correctly and two seconds late, so the
        // pass is provably still in flight below rather than probably.
        Reply::ok_after(2, &vectors_for(CHUNKS)),
        // And the embedding check the refused change *would* make. Without it,
        // a build with the claim removed is stopped by the mock running out of
        // replies rather than by anything in the product — which is a fixture
        // artefact wearing a guard's clothes, and it is what the first run of
        // the test above actually did.
        Reply::ok(&mnema_mock_provider::two_vectors(1024)),
    ]);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("the key is accepted");
    fx.adopt_default_model();
    let space = fx.active_space().expect("a model was adopted");
    fx.write_indexed_chunks(CHUNKS);

    // The two calls the setup made, off the queue and asserted, so that the wait
    // below is for the run's own request and not for one of theirs.
    for expected in [
        "the credit check `set_key` made",
        "the embedding check the adoption made",
    ] {
        assert!(
            fx.provider_request().is_some(),
            "{expected} never arrived, so the wait below is watching for the wrong request"
        );
    }

    let (channel, events) = job_channel();
    start_embed_job(fx.state(), channel).expect("the embedding job starts");

    // Wait until the mock has **read the run's request**, not merely until the
    // slot is taken. The server answers one connection at a time in the order
    // they arrive, so this is what fixes the order of the two callers below: the
    // run is inside its two-second reply, and anything the change sends queues
    // behind it. Without this the two race to connect, and a build with the
    // claim removed crosses the replies — the change gets the run's batch answer
    // and the run gets the change's. Measured on the first run of this test,
    // where it surfaced as the run ending on the mock's own `599` sentinel: a
    // red that says nothing about a model change.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut asked = false;
    while !asked && std::time::Instant::now() < deadline {
        asked = fx.provider_request().is_some();
        if !asked {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    assert!(
        asked,
        "the run never reached the provider, so nothing below is about a pass in flight"
    );
    assert!(
        fx.state().job_is_running(),
        "the run is not holding the slot, so the refusal below would be about nothing"
    );

    let outcome = set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Keep);

    let (_progress, ending) = events_until_ended(&events);
    assert_eq!(ending["reason"], json!("completed"), "{ending}");

    // The index first, for the reason the test above gives: a change that
    // succeeded has already done its damage, and an `expect_err` here would end
    // the test before anything looked at it.
    assert_eq!(
        fx.active_space(),
        Some(space),
        "the index was repointed while a pass was writing into the space it named"
    );
    let settings = model_settings(fx.state());
    let read = read_index(&settings.index);
    assert_eq!(
        read.embedded_chunks, CHUNKS as i64,
        "the active space does not hold what the run says it embedded"
    );
    // The property itself. `embedded_chunks_everywhere` counts every space the
    // index holds; `embedded_chunks` counts the one it points at. A vector in a
    // space nothing points at is exactly the difference between them, and it is
    // invisible to every other assertion in this file.
    assert_eq!(
        read.embedded_chunks_everywhere, read.embedded_chunks,
        "the index holds vectors outside the space it points at — a run wrote into a space that \
         was repointed away from it, and search will never see them"
    );
    assert_eq!(
        ending["done"],
        json!(read.embedded_chunks),
        "what the window was told and what the active space holds are two different numbers"
    );

    let refusal = outcome.expect_err("the model was changed while an embedding pass was writing");
    assert!(
        matches!(refusal, Error::JobAlreadyRunning),
        "the refusal must name the slot: {refusal:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Task 6 — the default model set. `DEFAULT_MODELS` is the one place the two ids
// are spelled, and this is where the rule around them is held: they are applied
// on the first key, and only to a role the index has no answer for.

/// A chat model somebody has already chosen, so that "it was not overwritten" is
/// a fact about a value nothing in the product could have written.
const CHOSEN_CHAT: &str = "vendor/chat-already-chosen";

/// The first key is what turns an unconfigured index into a working one: every
/// role the window can act on gets the model this product ships with, so that
/// nobody has to choose one before anything at all can happen.
///
/// Read through `model_settings` rather than out of the index by hand, because
/// that is the reply the window is drawn from: a default written somewhere the
/// settings command does not look is a default nobody ever sees.
#[test]
fn the_first_key_chooses_this_products_model_for_every_role_the_index_has_none_for() {
    let fx = Fixture::with_provider_accepting_everything();
    fx.open_index();

    set_key(fx.state(), KEY.into()).expect("the key is accepted");

    let settings = model_settings(fx.state());
    let read = read_index(&settings.index);
    assert_eq!(
        read.chat_model.as_deref(),
        Some(DEFAULT_MODELS.chat),
        "an index with no chat model was left with none"
    );
    assert_eq!(
        read.embedding_model.as_deref(),
        Some(DEFAULT_MODELS.embedding),
        "an index with no embedding model was left with none"
    );
    // The embedding role is not a `meta` row: it is a vector space, and a model
    // name recorded without one is a name search cannot use.
    assert!(
        read.active_space.is_some(),
        "the embedding model was recorded without a space to embed into"
    );
    assert_eq!(
        read.embedding_dim,
        Some(1024),
        "the width must be the one the provider answered with, not one this build assumed"
    );
}

/// The direction a happy-path test forgets, and the one that costs something: a
/// key entered over a configuration somebody has already made must leave it
/// exactly as it was.
///
/// **The fixture builds the state the guard branches on**, which is the whole
/// point of it: an index that already has both roles answered. Without that,
/// "sets both defaults" passes on an implementation that overwrites
/// unconditionally.
///
/// **And the provider has an answer in hand**, which is the second half. With a
/// mock that has nothing left to say, an implementation that overwrites is
/// stopped by the fixture running out of replies rather than by anything in the
/// product — a green that proves the mock's arithmetic. The spare reply is a
/// valid 1024-wide embedding check, so an unconditional build genuinely adopts,
/// mints a second space, and is caught here.
#[test]
fn a_key_entered_over_models_that_are_already_chosen_replaces_neither() {
    let fx = Fixture::with_provider_answering_with_dimension(1024);
    fx.open_index();
    set_chat_model(fx.state(), CHOSEN_CHAT.into()).expect("a chat model is chosen");
    // `OTHER_MODEL` and not `MODEL`: `MODEL` **is** this product's default, so an
    // index already on it is a state a build that overwrites unconditionally
    // leaves looking exactly the same — same model, same space, same count. The
    // one thing that tells the two builds apart is an index on a model the
    // default would have to move it off.
    fx.state()
        .with_index(|db| {
            db.adopt_embedding_model(OTHER_MODEL, 1024, fx.credential_ref(), "chunker-under-test")?;
            Ok(())
        })
        .expect("an embedding model is adopted");
    let space_before = fx.space_ids();

    set_key(fx.state(), KEY.into()).expect("the key is accepted");

    let settings = model_settings(fx.state());
    let read = read_index(&settings.index);
    assert_eq!(
        read.chat_model.as_deref(),
        Some(CHOSEN_CHAT),
        "a chat model somebody chose was replaced by this product's default"
    );
    assert_eq!(
        read.embedding_model.as_deref(),
        Some(OTHER_MODEL),
        "the index was repointed at a model nobody asked for"
    );
    assert_eq!(
        fx.space_ids(),
        space_before,
        "a space was minted for a role that already had an answer"
    );
    // The credit check `set_key` makes is the only request this test expects.
    // Taken off the queue and asserted, so the check after it is about the
    // absence of a *second* call rather than about a queue that was empty for
    // some other reason.
    assert!(
        fx.provider_request().is_some(),
        "the credit check `set_key` makes never arrived, so the check below proves nothing"
    );
    assert!(
        fx.provider_request().is_none(),
        "a request left the machine to choose a model that was already chosen"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// The transition gate: what a confirmed model change costs, measured end to end
// rather than warned about.
//
// A Rust integration test and not a component one, because a component test
// mocks the very layer where the loss happens. `mnema-mock-provider` is what
// makes the whole path — embed, search, retire, re-embed — runnable with no
// network.

/// A query no word of which appears in the seeded text, so a hit for it can
/// only have come from the content arm.
const SEMANTIC_ONLY_QUERY: &str = "vulpine";

/// A word that IS in the seeded text, so a hit for it can come from the lexical
/// arm alone — the control that tells "semantic went dark" apart from "the index
/// broke".
const LEXICAL_QUERY: &str = "number";

/// How many chunks a hit is expected for, spelled once: the fixture writes this
/// many, the run embeds this many, and the searches below match this many.
const SEEDED_CHUNKS: usize = 1;

/// Every hit's chunk id, in the order the fusion put them — what "found by
/// meaning" and "found again" are actually compared on, rather than a count that
/// two different documents would satisfy equally.
fn hit_ids(answer: &bridge::SearchAnswer) -> Vec<i64> {
    answer.hits.iter().map(|h| h.chunk_id).collect()
}

/// How many chunks the content arm itself matched — read off the arm's own
/// report rather than inferred from `hits`, because the fused list cannot say
/// which arm put something in it.
fn content_matched(answer: &bridge::SearchAnswer) -> usize {
    match answer.content {
        bridge::ContentArmReport::Answered { matched, .. } => matched,
        ref other => panic!("the content arm did not answer at all: {other:?}"),
    }
}

/// How many the lexical arm matched, on the same terms.
fn text_matched(answer: &bridge::SearchAnswer) -> usize {
    match answer.text {
        bridge::TextArmReport::Answered { matched } => matched,
        ref other => panic!("the lexical arm did not answer at all: {other:?}"),
    }
}

/// **Discarding the vectors takes semantic search with them, and this is that
/// loss stated as a fact instead of as a warning.**
///
/// `set_embedding_model` with [`ExistingVectors::Discard`] is not a migration:
/// nothing is copied and nothing is re-embedded, and `retrieve` hands the KNN
/// only the *current* `active_space`. So the moment the change lands, every
/// semantic hit in the index is gone — while the lexical arm carries on
/// answering, which is what makes the loss quiet: the launcher keeps replying,
/// and replies worse.
///
/// Four moments, in one run, because no two of them are the same claim:
///
/// 1. a hit for a query only the content arm can answer;
/// 2. that hit gone after the change — and the *lexical* arm still answering,
///    which is what separates "semantic went dark" from "the index broke";
/// 3. `start_embed_job` refilling the new space;
/// 4. the same document findable by meaning again.
///
/// **What makes step 2 a measurement of the fix rather than of the seeding** is
/// step 1 above and `a_change_that_retires_nothing_is_the_same_fixture_taking_the_other_path`
/// below: the first says this fixture DOES produce a semantic hit, so its
/// absence later is caused by something; the second says this fixture's change
/// machinery can also take the non-destructive path, so what causes it is
/// `Discard` meeting a space with vectors in it.
#[test]
fn discarding_the_vectors_takes_semantic_search_with_them_until_the_index_is_refilled() {
    let fx = Fixture::with_provider_answering_a_run(vec![
        // The first pass over the queue.
        Reply::ok(&vectors_for(SEEDED_CHUNKS)),
        // The semantic search before the change.
        Reply::ok(&vectors_for(1)),
        // The embedding check the change itself makes.
        Reply::ok(&mnema_mock_provider::two_vectors(1024)),
        // The same semantic search after it, and the lexical control beside it —
        // the content arm runs on that one too and asks for its own embed.
        Reply::ok(&vectors_for(1)),
        Reply::ok(&vectors_for(1)),
        // The refill, and the semantic search that follows it.
        Reply::ok(&vectors_for(SEEDED_CHUNKS)),
        Reply::ok(&vectors_for(1)),
    ]);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("the key is accepted");
    let doomed_space = fx.active_space().expect("`set_key` chose a model");
    let chunks = fx.write_indexed_chunks(SEEDED_CHUNKS);

    let (channel, events) = job_channel();
    start_embed_job(fx.state(), channel).expect("the first embedding pass starts");
    let (_progress, ending) = events_until_ended(&events);
    assert_eq!(ending["reason"], json!("completed"), "{ending}");

    // 1 — found by meaning, and by nothing else. The lexical arm is asserted to
    // have matched NOTHING for this query, which is what makes the hit below
    // attributable to the content arm rather than to a word that happened to
    // overlap.
    let before = bridge::search(fx.state(), SEMANTIC_ONLY_QUERY.into()).expect("the search runs");
    assert_eq!(
        text_matched(&before),
        0,
        "the lexical arm matched this query, so a hit for it says nothing about meaning"
    );
    assert_eq!(
        content_matched(&before),
        SEEDED_CHUNKS,
        "the content arm found nothing before the change, so its silence afterwards would \
         measure the fixture rather than the change"
    );
    assert_eq!(
        hit_ids(&before),
        chunks,
        "the window was shown something other than the document that was seeded"
    );

    let adopted = set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Discard)
        .expect("the confirmed change is accepted");
    assert_eq!(
        adopted
            .retired
            .iter()
            .map(|r| (r.space_id, r.embedded_chunks))
            .collect::<Vec<_>>(),
        vec![(doomed_space, SEEDED_CHUNKS as i64)],
        "the change did not report retiring the space the vectors were in"
    );

    // 2 — the loss. Both halves: the content arm answers and matches nothing,
    // and the lexical arm still finds the very same document, so this is
    // semantic search going dark and not an index that broke.
    let after = bridge::search(fx.state(), SEMANTIC_ONLY_QUERY.into()).expect("the search runs");
    assert_eq!(
        content_matched(&after),
        0,
        "the content arm still matched something in a space that was just emptied"
    );
    assert_eq!(
        hit_ids(&after),
        Vec::<i64>::new(),
        "the window was still shown a hit for a query only the vectors could answer"
    );
    let lexical = bridge::search(fx.state(), LEXICAL_QUERY.into()).expect("the search runs");
    assert_eq!(
        text_matched(&lexical),
        SEEDED_CHUNKS,
        "the lexical arm stopped answering too, so what happened is not the loss this test \
         is about"
    );
    assert_eq!(
        hit_ids(&lexical),
        chunks,
        "the document itself is gone, not merely its vectors"
    );

    // 3 — the recovery, through the ordinary command: `start_embed_job` takes no
    // root and covers the whole index, so nothing new is needed to offer it.
    let (channel, events) = job_channel();
    start_embed_job(fx.state(), channel).expect("the refill starts");
    let (_progress, ending) = events_until_ended(&events);
    assert_eq!(ending["reason"], json!("completed"), "{ending}");
    assert_eq!(ending["done"], json!(SEEDED_CHUNKS), "{ending}");

    // 4 — findable by meaning again, and in the new space rather than the old.
    let refilled = bridge::search(fx.state(), SEMANTIC_ONLY_QUERY.into()).expect("the search runs");
    assert_eq!(
        content_matched(&refilled),
        SEEDED_CHUNKS,
        "the re-embedding pass reported success and semantic search still finds nothing"
    );
    assert_eq!(hit_ids(&refilled), chunks);
    assert_eq!(
        fx.active_space(),
        Some(adopted.space_id),
        "the refilled space is not the one the index points at"
    );
    assert!(
        !fx.space_ids().contains(&doomed_space),
        "the retired space is still there, so nothing was actually discarded: {:?}",
        fx.space_ids()
    );
}

/// The control the gate above stands on: **the same fixture, taking the
/// non-destructive path.**
///
/// Without it, "the content arm matched nothing after the change" is equally
/// well explained by a fixture whose `set_embedding_model` cannot leave an index
/// searchable at all. Here the active space is empty — the state an index is in
/// before a pass has written anything — and the change is made with
/// [`ExistingVectors::Keep`], the value that refuses rather than destroys. It is
/// accepted, it retires nothing, and the index is left pointing at a space of
/// its own.
#[test]
fn a_change_that_retires_nothing_is_the_same_fixture_taking_the_other_path() {
    let fx = Fixture::with_provider_answering_embedding_checks(1024, 2);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("the key is accepted");
    let first_space = fx.active_space().expect("`set_key` chose a model");
    fx.write_indexed_chunks(SEEDED_CHUNKS);
    assert_eq!(
        fx.embedded_chunks_in(first_space),
        0,
        "this control is about an EMPTY active space, and this one is not empty"
    );

    let adopted = set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Keep)
        .expect("a change that strands nothing is not refused");

    assert_eq!(
        adopted.retired,
        Vec::new(),
        "a change nobody confirmed retired a space"
    );
    assert_eq!(
        fx.space_ids().len(),
        2,
        "the space that was left behind is gone, which `Keep` may not do: {:?}",
        fx.space_ids()
    );
    assert_eq!(
        fx.active_space(),
        Some(adopted.space_id),
        "the index does not point at the space the change adopted"
    );
    assert_ne!(
        adopted.space_id, first_space,
        "the change found the space it started from, so it moved nothing and this control \
         says nothing about a change at all"
    );
}

/// **The failure that leaves the data already destroyed**, forced rather than
/// reasoned about.
///
/// Retiring a space and adopting a model are two transactions and cannot be one
/// (`Db::drop_space` and `Db::adopt_embedding_model` each open their own, and
/// SQLite has no nested `BEGIN`). An ordinary failure of the second, after the
/// first has committed, leaves the vectors gone and `meta.active_space` naming a
/// space that no longer exists.
///
/// **How it is forced, and why this shape rather than a lock or a full disk.**
/// A second connection racing for the write lock is a timing hypothesis, and a
/// full disk is not arrangeable from a test on any machine this suite runs on.
/// What is arrangeable is the next space's own table name: `embedding_space.id`
/// is `AUTOINCREMENT`, so the id `create_space` will use is readable from
/// `sqlite_sequence` before the call, and a table already standing under that
/// name makes `CREATE VIRTUAL TABLE` fail — *after* the retirement has
/// committed, which is the whole point. Nothing about the product is mocked: the
/// command takes exactly the path an I/O error would send it down.
///
/// Two claims, and the second is the one a window has to live with: the
/// rejection names the space and the count that went, and a subsequent
/// `model_settings` reports `Unreadable { cause: ReadFailed }` — because the
/// error itself reaches the webview as a bare sentence and cannot be branched
/// on.
///
/// **And a third, added by review: the way out is run, not described.** The
/// window renders `models_index_recover` in this state — "choosing an embedding
/// model again repairs this" — and until now that sentence was a claim made only
/// by [`set_embedding_model`]'s own doc and by a component test against a mock.
/// Neither could tell whether the real index honours it, and the window sends
/// `Keep`, which is the value that refuses whenever another space is not empty.
/// The last block below performs the recovering act against this index, in the
/// state the wreck actually left.
#[test]
fn an_adoption_that_fails_after_the_retirement_says_what_was_already_destroyed() {
    // Three embedding checks: `set_key`'s default adoption, the change that is
    // supposed to fail, and the recovering act at the end.
    let fx = Fixture::with_provider_answering_embedding_checks(1024, 3);
    fx.open_index();
    set_key(fx.state(), KEY.into()).expect("the key is accepted");
    let doomed_space = fx.active_space().expect("`set_key` chose a model");
    fx.embed_chunks_in_the_active_space(EMBEDDED);

    // The name the next space will be given, read from the counter rather than
    // guessed: a guess that missed would leave the adoption succeeding and this
    // test asserting about an error that never happened.
    let next_id: i64 = fx
        .state()
        .with_index(|db| {
            let seq: i64 = db.conn().query_row(
                "SELECT seq FROM sqlite_sequence WHERE name = 'embedding_space'",
                [],
                |r| r.get(0),
            )?;
            db.conn()
                .execute(&format!("CREATE TABLE vec_emb_{} (x)", seq + 1), [])?;
            Ok(seq + 1)
        })
        .expect("the squatting table is created");

    let refusal = set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Discard)
        .expect_err("the adoption was supposed to fail on the name already taken");

    // The state first, because a rejection that arrived after the drop committed
    // has already done its damage and this test's subject is what it left behind.
    assert!(
        !fx.space_ids().contains(&doomed_space),
        "the retirement did not commit, so this is not the state this test is about: {:?}",
        fx.space_ids()
    );
    assert_eq!(
        fx.state()
            .with_index(|db| db.active_space())
            .expect("the index answers"),
        Some(doomed_space),
        "the pointer moved, so the adoption did not fail where this test needs it to"
    );

    let Error::RetiredThenFailed { retired, .. } = &refusal else {
        panic!("the failure did not carry what it had already destroyed: {refusal:?}");
    };
    assert_eq!(
        retired
            .iter()
            .map(|r| (r.space_id, r.embedded_chunks))
            .collect::<Vec<_>>(),
        vec![(doomed_space, EMBEDDED)],
        "the sentence does not name the space that went, or what it held"
    );
    // The sentence itself, because that is all the window receives: `impl
    // Serialize for Error` sends the `Display` string and nothing else.
    let said = refusal.to_string();
    for expected in [doomed_space.to_string(), EMBEDDED.to_string()] {
        assert!(
            said.contains(&expected),
            "the rejection a person is shown does not name `{expected}`: {said}"
        );
    }

    // And the state the window branches on instead of on that sentence.
    let settings = model_settings(fx.state());
    assert!(
        matches!(
            settings.index,
            IndexSettings::Unreadable {
                cause: UnreadableCause::ReadFailed,
                ..
            }
        ),
        "the wreck does not reach the window as the state it has to branch on: {:?}",
        settings.index
    );
    // Not `NotOpen`, and asserted separately: the index IS open, and telling
    // somebody whose vectors are gone that nothing has been opened yet sends
    // them nowhere.
    assert!(
        matches!(settings.key, KeyState::Present),
        "the key half was taken down with the index half"
    );

    // --- and out of it again, by the act the window offers -------------------
    //
    // The obstacle removed is the squatting table and nothing else: the pointer
    // is still naming a space that is gone, and the refusal `Keep` would
    // otherwise raise is not disarmed here — it enumerates `embedding_space`,
    // and the retired space's row went with the `DROP` that committed.
    fx.state()
        .with_index(|db| {
            db.conn()
                .execute(&format!("DROP TABLE vec_emb_{next_id}"), [])?;
            Ok(())
        })
        .expect("the squatting table is removed");

    // `Keep`, because `Keep` is what the window sends in this state — an index
    // that will not answer states no estimate, so it asks nothing and sends the
    // value that refuses rather than destroys.
    let adopted = set_embedding_model(fx.state(), OTHER_MODEL.into(), ExistingVectors::Keep)
        .expect("the recovering act the window offers was refused by the index");

    assert!(
        adopted.created,
        "the recovery found a space instead of minting one, so the pointer it wrote is the old one"
    );
    assert_eq!(
        fx.state()
            .with_index(|db| db.active_space())
            .expect("the index answers"),
        Some(adopted.space_id),
        "the pointer the index lost was not rewritten to the space just adopted"
    );
    // And the window's own reading of it: the defect sentence is gone, and the
    // model named is the one that was just chosen.
    let recovered = model_settings(fx.state());
    assert_eq!(
        read_index(&recovered.index).embedding_model.as_deref(),
        Some(OTHER_MODEL),
        "the settings screen does not name the model the recovery adopted"
    );
}

/// The default set is applied inside the job slot, and a slot that is taken
/// means it is simply not applied — a default the next key or the next explicit
/// choice will apply anyway.
///
/// **The reason is `embed_job.rs`'s**, the same one written out inside
/// `set_embedding_model`: adoption repoints `meta.active_space`, and a pass that
/// read that pointer when it started goes on writing into the space it started
/// with. A key entered mid-run must not move it.
///
/// **Both directions, and the second is the one a person meets.** A job holding
/// the slot must not make entering a key fail: the key is checked, stored, and
/// reported exactly as it would be with nothing running. Only the model choice
/// waits.
#[test]
fn a_key_entered_while_a_job_runs_stores_the_key_and_moves_no_pointer() {
    // A spare embedding check, so a build that took no slot at all is stopped by
    // the assertions below rather than by the mock running out of replies —
    // which would be a fixture artefact wearing a guard's clothes.
    let fx = Fixture::with_provider_answering_embedding_checks(1024, 1);
    fx.open_index();

    let (probe, _probe_events) = job_channel();
    bridge::start_probe_job(fx.state(), probe).expect("the probe job starts");

    set_key(fx.state(), KEY.into()).expect("a running job must not stop a key being entered");

    assert!(
        mnema_secrets::load(fx.credential_ref())
            .expect("read the store")
            .is_some(),
        "the key was not stored, so a job holding the slot cost the person the thing they came \
         to this screen to do"
    );
    assert_eq!(
        fx.active_space(),
        None,
        "the index was repointed while a pass was reading that pointer"
    );
    assert_eq!(
        fx.space_ids(),
        Vec::<i64>::new(),
        "a space was minted while a job was running"
    );

    bridge::cancel_job(fx.state());
    wait_for_the_slot(&fx);
}

/// The other end of the same rule, and the state `set_key` alone cannot reach:
/// a key stored while no index was open leaves an installation with a key and
/// no models, and nothing ever comes back for it.
///
/// **The fixture is the point.** `set_key` is called against a CLOSED index, so
/// every step of its default pass is silent: `with_index` fails for the chat
/// half, `active_space()` fails for the embedding half. That is exactly what a
/// boot whose open failed leaves behind — the settings screen is reachable, the
/// key is checked, accepted and stored, and nothing else happens. On the next
/// launch there is nothing left to press: the key is already there, so `set_key`
/// will not run again, and the product can neither embed nor answer.
///
/// The boot is run for real rather than the inner function being called, and its
/// thread is joined rather than polled: the claim is that **start-up** applies
/// them, and a test that waited on a timer would report timing.
#[test]
fn a_key_stored_while_the_index_was_shut_gets_its_models_at_the_next_boot() {
    let fx = Fixture::with_provider_accepting_everything();

    set_key(fx.state(), KEY.into()).expect("a closed index must not stop a key being entered");

    // The premise, asserted rather than assumed: nothing was configured, and the
    // window has nothing to show for the key it just accepted.
    assert!(
        matches!(
            model_settings(fx.state()).index,
            IndexSettings::Unreadable {
                cause: UnreadableCause::NotOpen,
                ..
            }
        ),
        "this test's premise is a key entered while no index was open"
    );

    mnema_desktop::boot_index(fx.handle())
        .join()
        .expect("the boot's default pass panicked");

    let settings = model_settings(fx.state());
    let index = read_index(&settings.index);
    assert_eq!(
        index.embedding_model.as_deref(),
        Some(DEFAULT_MODELS.embedding),
        "the boot opened an index with no embedding model, and nothing will ever ask again"
    );
    assert_eq!(
        index.chat_model.as_deref(),
        Some(DEFAULT_MODELS.chat),
        "the boot opened an index with no chat model, and nothing will ever ask again"
    );
}

/// And the direction that costs something if it is wrong: a boot with no key
/// stored asks the provider nothing and chooses nothing. Every launch takes this
/// path until somebody enters a key, so a build that called out here would call
/// out on every start-up of an installation that has never been signed in.
#[test]
fn a_boot_with_no_key_stored_calls_nobody_and_chooses_nothing() {
    let fx = Fixture::with_provider_accepting_everything();

    mnema_desktop::boot_index(fx.handle())
        .join()
        .expect("the boot's default pass panicked");

    assert_eq!(
        fx.provider_request(),
        None,
        "a boot with no key still sent a request to the provider"
    );
    let settings = model_settings(fx.state());
    let index = read_index(&settings.index);
    assert_eq!(
        index.embedding_model, None,
        "a boot with no key chose an embedding model anyway"
    );
    assert_eq!(
        index.chat_model, None,
        "a boot with no key chose a chat model anyway"
    );
}

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
    IndexRead, IndexSettings, KeyState, UnreadableCause, forget_key, key_present, model_settings,
    provider_models, set_chat_model, set_embedding_model, set_key, set_rerank_model,
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

    forget_key(fx.state()).expect("forgotten");

    assert!(!key_present(fx.state()).expect("ask the store"));
    assert!(
        fx.state()
            .with_index(|db| db.active_space())
            .expect("read")
            .is_some(),
        "removing the key removes the ability to embed, not what was embedded"
    );
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

        let adopted = set_embedding_model(fx.state(), MODEL.into()).expect("chosen");

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
    let first = set_embedding_model(fx.state(), MODEL.into()).expect("chosen");
    assert!(first.created);

    // A second embedding check, so the second call has an answer of its own.
    let second = set_embedding_model(fx.state(), MODEL.into()).expect("chosen again");

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
        KeyState::Unreadable { reason } => assert!(
            !reason.is_empty(),
            "a store that would not answer must say something about why"
        ),
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

    let refusal =
        set_embedding_model(fx.state(), MODEL.into()).expect_err("a model needs a key to check");

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

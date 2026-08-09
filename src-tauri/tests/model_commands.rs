//! The key crosses three boundaries — window, credential store, provider — and
//! must not cross a fourth into the database.

/// Beside `support/mod.rs` rather than inside it — see that file's own header
/// for why a shared module cannot hold something only one binary uses.
#[path = "support/fixture.rs"]
mod fixture;
mod support;

use fixture::Fixture;
use mnema_desktop::error::Error;
use mnema_desktop::models::{forget_key, key_present, set_key};

/// Synthetic, and shaped so it cannot be mistaken for a provider key: no `sk-`
/// prefix, no base64 tail, and it says what it is. If this string is ever found
/// in a database or a keychain, it came from here.
const KEY: &str = "test-key-not-a-real-one-0123456789";

/// A second one, so a test can tell "the key that was already there" from "some
/// key is there".
const KEY_ALREADY_ENTERED: &str = "test-key-not-a-real-one-abcdefghij";

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

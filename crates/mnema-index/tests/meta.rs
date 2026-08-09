//! `meta` is a key-value table, and the point of these tests is that the keys
//! are constants rather than literals spread over three crates.

use mnema_index::{Error, META_ACTIVE_SPACE, META_CHAT_MODEL, META_RERANK_MODEL};

mod support;
use support::temp_db;

#[test]
fn a_key_that_was_never_written_is_absent_rather_than_empty() {
    let db = temp_db();
    assert_eq!(db.meta_get(META_CHAT_MODEL).expect("read"), None);
}

#[test]
fn writing_the_same_key_twice_replaces_it() {
    let db = temp_db();
    db.meta_set(META_CHAT_MODEL, "vendor/first").expect("write");
    db.meta_set(META_CHAT_MODEL, "vendor/second")
        .expect("write");
    assert_eq!(
        db.meta_get(META_CHAT_MODEL).expect("read").as_deref(),
        Some("vendor/second"),
        "meta holds one current value per key, not a history"
    );
}

#[test]
fn two_keys_do_not_see_each_other() {
    let db = temp_db();
    db.meta_set(META_CHAT_MODEL, "vendor/chat").expect("write");
    assert_eq!(db.meta_get(META_RERANK_MODEL).expect("read"), None);
}

/// The active space is the one key whose overwrite loses data rather than a
/// diagnosis: the replaced space's vectors stay on disk, unreachable, while
/// search answers from the new one.
///
/// Both directions in one test on purpose. "The refused key is refused" is
/// satisfied by a `meta_set` that refuses everything, and "an ordinary key
/// still writes" by one that refuses nothing, so neither half is evidence
/// without the other.
#[test]
fn the_active_space_is_refused_while_an_ordinary_key_still_writes() {
    let db = temp_db();

    let refused = db
        .meta_set(META_ACTIVE_SPACE, "1")
        .expect_err("the active space may not be set through meta_set");
    assert!(
        matches!(refused, Error::ActiveSpaceNotWritable),
        "a caller that tries must be told which rule stopped it, got {refused:?}"
    );
    assert!(
        refused.to_string().contains(META_ACTIVE_SPACE),
        "the message has to name the key it refused, and reads {refused}"
    );
    assert_eq!(
        db.meta_get(META_ACTIVE_SPACE).expect("read"),
        None,
        "the refusal has to be a refusal, not a write that also returned an error"
    );

    db.meta_set(META_CHAT_MODEL, "vendor/chat")
        .expect("an ordinary key still writes");
    assert_eq!(
        db.meta_get(META_CHAT_MODEL).expect("read").as_deref(),
        Some("vendor/chat"),
        "the guard is about one key, not about writing"
    );
}

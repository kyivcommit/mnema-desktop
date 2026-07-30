//! What happens when something else registered the credential store first.
//!
//! `ensure_default_store` defers to whoever got there first — that deference is
//! what lets the unit tests in `src/lib.rs` run the shipped functions against
//! `keyring_core::mock`. This is the floor under it, and it has to be tested
//! from here rather than from a unit test: under `cfg(test)` the check is
//! deliberately skipped, because the mock the unit tests are built on is the
//! very thing it rejects. An integration test compiles the library without
//! `cfg(test)`, so this runs the real, shipped path.
//!
//! Not `#[ignore]`d, and it needs no keychain: it fails before reaching one.
//!
//! Its own file on purpose. `keyring_core`'s default store is process-global,
//! and `tests/roundtrip.rs` asserts that the registered store is *not* a mock.
//! In one binary the two would fight, and which won would depend on test order —
//! `cargo test -- --include-ignored` would decide it. Separate files are
//! separate binaries, so neither can see the other's store.

use std::sync::{Arc, Mutex};

use keyring_core::{CredentialStore, mock};
use mnema_secrets::{Error, forget, load, store};

/// Synthetic, and shaped so it cannot be mistaken for a provider key.
const SYNTHETIC: &str = "mnema-synthetic-value-not-a-key-dddd";

/// Both tests here register a different default store, and that store is
/// process-global. Without this they would race and the loser would fail for a
/// reason that has nothing to do with what it asserts.
static REGISTERED_STORE: Mutex<()> = Mutex::new(());

#[test]
fn a_store_that_does_not_persist_is_refused_on_every_entry_point() {
    let _sole_user = REGISTERED_STORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // `keyring_core::mock` is not behind a feature flag, so it is compiled into
    // release builds and anything in the process can register it. A key handed
    // to it is not stored at all: it lives in process memory, and keyring_core
    // logs the mock credential's `Debug` — which contains the secret — at debug
    // level on every operation.
    let registered: Arc<CredentialStore> = mock::Store::new().expect("the mock store builds");
    keyring_core::set_default_store(registered);

    let name = "foreign-store";

    for (entry_point, result) in [
        ("store", store(name, SYNTHETIC).map(|()| None)),
        ("load", load(name)),
        ("forget", forget(name).map(|()| None)),
    ] {
        let err = result.expect_err("a non-persistent store must be refused");
        assert!(
            matches!(err, Error::NotPersistent { .. }),
            "{entry_point} accepted a store that does not keep credentials: {err:?}"
        );
        // Naming the entry, and naming the store so the reader can tell which
        // one got registered. The secret is not in either.
        let rendering = format!("{err}{err:?}");
        assert!(
            rendering.contains(name),
            "{entry_point}: the error does not name the entry: {rendering}"
        );
        assert!(
            !rendering.contains(SYNTHETIC),
            "{entry_point}: the error contains the secret: {rendering}"
        );
    }
}

#[test]
fn the_refusal_is_about_persistence_and_not_about_every_store() {
    let _sole_user = REGISTERED_STORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // Guards the test above against being vacuous. If `store`/`load`/`forget`
    // errored for any reason at all — a missing store, a bad reference — the
    // assertions up there would pass while proving nothing about persistence.
    // The mock is the only store this test binary can reach, so the control is
    // the predicate itself: it must accept a store that claims durability.
    //
    // `mnema_secrets` does not export the predicate, so this reproduces the one
    // decision it makes, against a store that reports the disk-backed variant.
    struct Durable(Arc<mock::Store>);

    impl keyring_core::api::CredentialStoreApi for Durable {
        fn vendor(&self) -> String {
            "durable stand-in for a platform store".to_string()
        }
        fn id(&self) -> String {
            self.0.id()
        }
        fn build(
            &self,
            service: &str,
            user: &str,
            mods: Option<&std::collections::HashMap<&str, &str>>,
        ) -> keyring_core::Result<keyring_core::Entry> {
            self.0.build(service, user, mods)
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        /// The one line that differs from the mock, and the whole point.
        fn persistence(&self) -> keyring_core::CredentialPersistence {
            keyring_core::CredentialPersistence::UntilDelete
        }
    }

    let durable: Arc<CredentialStore> =
        Arc::new(Durable(mock::Store::new().expect("the mock store builds")));
    keyring_core::set_default_store(durable);

    let name = "durable-store";
    store(name, SYNTHETIC).expect("a store claiming durability is accepted");
    assert_eq!(
        load(name).expect("and can be read back").as_deref(),
        Some(SYNTHETIC),
        "the refusal above must be about persistence, not about the store being a stand-in"
    );
    forget(name).expect("and can be cleared");
}

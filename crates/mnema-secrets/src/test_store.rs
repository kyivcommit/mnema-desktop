//! A credential store for **other crates'** integration tests, and for nothing
//! else.
//!
//! # Why this has to exist at all
//!
//! The unit tests in `lib.rs` install `keyring_core::mock` and run the shipped
//! `store`/`load`/`forget` against it. A test in another crate cannot: this
//! crate withholds the platform store only under its **own** `cfg(test)` (the
//! `#[cfg(test)]` arm inside `platform_store`), which another crate's test does
//! not set. Such a test therefore reaches the developer's real login keychain —
//! and on a runner with no unlocked keychain and no Secret Service session, it
//! fails for a reason that has nothing to do with what it asserts.
//! `.github/workflows/ci.yml` states the resulting rule in prose: a plain
//! `cargo test --workspace`, on CI or on a developer's machine, must never touch
//! a credential store.
//!
//! [`register`] is how a test keeps that rule while still running the shipped
//! functions rather than a parallel code path that proves nothing.
//!
//! # Why it is behind a feature, and why that gate is load-bearing
//!
//! The store below **claims durability and keeps nothing**. That is precisely
//! the shape `store_is_acceptable` exists to refuse: a key handed to it is gone
//! when the process ends, and `keyring_core` logs a mock credential's `Debug` —
//! which contains the secret — at debug level on every operation. Registered in
//! a shipped build it would report success for a key that is not saved, and put
//! that key where a debug logger writes it.
//!
//! `keyring_core::mock` is not behind a feature and so is compiled into release
//! builds already; what this module adds on top is the false durability claim,
//! and that is what the `test-store` feature keeps out of one. Cargo's resolver
//! does not enable a feature that only a dev-dependency asks for when it is
//! building a normal one, so `cargo build --release` does not compile this file.
//!
//! Its [`vendor`](keyring_core::api::CredentialStoreApi::vendor) string says
//! what it is, so a test that asserts which store is registered — as
//! `tests/roundtrip.rs` does — fails loudly rather than passing against this.
//! What actually keeps the feature out of a shipped graph, rather than merely
//! documenting that it should be, is an assertion on `cargo tree -e normal` in
//! `src-tauri/tests/dependency_boundary.rs`.
//!
//! # The `Debug` hazard is not only a release-build concern
//!
//! `build` below delegates to `keyring_core::mock`, so the credentials handed
//! out are mock credentials, whose `Debug` contains the secret — and
//! `keyring_core` logs that `Debug` on every operation. Inside a test binary
//! that is live, not theoretical. It leaks nothing today for two reasons worth
//! stating rather than assuming: no `log` implementation is installed in these
//! binaries, so the macros compile to no-ops, and every value they would print
//! is a synthetic fixture. A test binary that installs a logger and stores
//! something real breaks both at once.
//!
//! # Not what `tests/foreign_store.rs` uses
//!
//! That file builds its own durable-claiming stand-in and keeps doing so. It is
//! the *control* for `check_persistence`: it must accept a store that claims
//! durability. A control that came from the crate under test would be asserting
//! that this crate's own helper satisfies this crate's own predicate, which is a
//! weaker question than the one it asks.

use std::collections::HashMap;
use std::sync::{Arc, Once};

use keyring_core::api::CredentialStoreApi;
use keyring_core::{CredentialPersistence, CredentialStore, Entry, mock};

/// Makes an in-memory store the process default, once.
///
/// Call it before the first `store`/`load`/`forget` in the test binary.
/// `ensure_default_store` accepts whoever registered first, so a call after the
/// platform store has already been installed changes nothing — which is why
/// this belongs at the top of a fixture's constructor rather than beside the
/// assertion that needs it.
///
/// Process-global and idempotent: the `Once` is what lets every fixture in a
/// binary call it without the last one replacing a store the others are already
/// holding entries in.
pub fn register() {
    static REGISTERED: Once = Once::new();
    REGISTERED.call_once(|| {
        // Nobody may have got here first. `keyring_core::set_default_store`
        // replaces silently and returns nothing, so without this the sequence
        // that undoes the whole arrangement is invisible: a test that reaches a
        // credential before building its fixture installs the *platform* store
        // (`ensure_default_store` builds one when none is registered), touches
        // the real keychain, and is then quietly papered over by the next
        // fixture's call to this function. Nothing would go red — least of all
        // on a developer's macOS machine, where the real keychain answers.
        if let Some(existing) = keyring_core::get_default_store() {
            panic!(
                "a credential store was already registered when the test store went to \
                 install itself: {}. Something in this test binary reached a credential \
                 before the fixture that swaps the store — on a machine with a working \
                 keychain that read or wrote a real one.",
                existing.vendor()
            );
        }
        let store: Arc<CredentialStore> = Arc::new(InMemory(
            mock::Store::new().expect("the in-memory store builds"),
        ));
        keyring_core::set_default_store(store);
    });

    // Checked on every call, not only the first. `Once` makes every later call
    // a no-op, and a no-op is precisely what a caller must not be allowed to
    // read as "the store is mine now". Positive — naming the store that must be
    // there rather than ruling out the one that must not — for the reason
    // `tests/roundtrip.rs` gives for its own vendor assertion: a negative names
    // one impostor and waves the rest through.
    let registered = keyring_core::get_default_store()
        .expect("registering a default store leaves one registered");
    assert_eq!(
        registered.vendor(),
        VENDOR,
        "the registered credential store is not the test one, so anything this binary \
         stores or loads is going somewhere real"
    );
}

/// Says what it is, in one place, because two things depend on the exact
/// string: the assertion in [`register`], and `tests/roundtrip.rs`, which
/// asserts the platform store by name and so fails if this one is ever
/// registered in its binary.
const VENDOR: &str = "mnema-secrets test-store: in memory, keeps nothing beyond this process";

/// `keyring_core::mock` with one method changed.
///
/// Delegating rather than reimplementing keeps the entry semantics — one secret
/// per `(service, reference)` pair, `NoEntry` for one never written — identical
/// to what the unit tests in `lib.rs` run against.
struct InMemory(Arc<mock::Store>);

impl CredentialStoreApi for InMemory {
    fn vendor(&self) -> String {
        VENDOR.to_string()
    }

    fn id(&self) -> String {
        self.0.id()
    }

    fn build(
        &self,
        service: &str,
        user: &str,
        mods: Option<&HashMap<&str, &str>>,
    ) -> keyring_core::Result<Entry> {
        self.0.build(service, user, mods)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// The one line that differs from the mock, and the whole reason this type
    /// exists: `check_persistence` refuses everything but `UntilDelete`, so the
    /// mock alone cannot be used to exercise the shipped path from outside this
    /// crate. The claim is false — see this module's header.
    fn persistence(&self) -> CredentialPersistence {
        CredentialPersistence::UntilDelete
    }
}

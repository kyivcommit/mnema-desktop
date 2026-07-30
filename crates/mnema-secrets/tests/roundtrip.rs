//! The one test that talks to the real OS credential store.
//!
//! It is `#[ignore]`d, and that is a decision rather than a convenience. An
//! unattended runner has no unlocked login keychain and no Secret Service
//! session; a test that reaches for one there either fails for a reason that has
//! nothing to do with this crate, or blocks on an unlock prompt no one will
//! answer. Every behaviour of `store`/`load`/`forget` is covered in
//! `src/lib.rs` against `keyring_core::mock`, running the same code — only the
//! registered store differs. What is left here is the one claim the mock cannot
//! make: that the platform store is wired up and does what it is told.
//!
//! Run it deliberately:
//!
//! ```text
//! cargo test -p mnema-secrets -- --ignored
//! ```
//!
//! macOS will ask for permission the first time the test binary touches the
//! keychain, and will ask again after every rebuild, because the binary it is
//! authorising is a new one.

use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use mnema_secrets::{forget, load, store};

/// Synthetic, and shaped so it cannot be mistaken for a provider key: no `sk-`
/// prefix, no base64 tail, and it says what it is. If this string is ever found
/// in a real keychain, it came from here.
const SYNTHETIC: &str = "mnema-synthetic-value-not-a-key-cccc";

/// The crate whose store this platform must be running against.
///
/// Platform-specific because the claim is. "The registered store is the OS
/// credential store" has a different answer on each, and a check that works
/// everywhere is a check that identifies nothing.
///
/// A `contains` on the crate name rather than the whole vendor string:
/// `keyring_core` asks stores to put their crate URL in it, all three do, and
/// the crate name is the part that identifies the store — the prose around it
/// ("macOS Keychain Store, …") can be reworded without changing which store is
/// registered, and a test that fails on rewording is a test that gets deleted.
/// Read from each store's source: apple-native-keyring-store 1.0.1,
/// windows-native-keyring-store 1.1.0, zbus-secret-service-keyring-store 1.0.0.
#[cfg(target_os = "macos")]
const PLATFORM_STORE_CRATE: &str = "apple-native-keyring-store";
#[cfg(target_os = "windows")]
const PLATFORM_STORE_CRATE: &str = "windows-native-keyring-store";
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
const PLATFORM_STORE_CRATE: &str = "zbus-secret-service-keyring-store";

/// Deletes the reference however the test ends.
///
/// Without this, a failed assertion leaves a credential in the developer's login
/// keychain — and the next run, using a different name, would not clean it up
/// either.
struct Cleanup(String);

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Err(e) = forget(&self.0) {
            // Not a panic: panicking in a `Drop` during an unwinding assertion
            // aborts the process and hides the real failure.
            eprintln!(
                "could not remove the test credential `{}` — delete it by hand: {e}",
                self.0
            );
        }
    }
}

/// A name no other run can collide with, so two concurrent runs cannot delete
/// each other's credential, and a leftover from a crashed run is identifiable.
fn unique_reference() -> String {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970");
    format!("selftest-{}-{}", process::id(), since_epoch.as_nanos())
}

#[test]
#[ignore = "touches the real OS credential store; run explicitly"]
fn a_secret_round_trips_through_the_os_store() {
    let name = unique_reference();
    let _cleanup = Cleanup(name.clone());

    assert_eq!(
        load(&name).unwrap(),
        None,
        "a reference this run invented must not already exist"
    );

    store(&name, SYNTHETIC).unwrap();

    // The assertions below would pass just as happily against an in-memory map.
    // This is the one thing that separates this test from the unit tests: the
    // store that answered them is the platform's.
    //
    // The library's own `check_persistence` does not settle it. That asks about
    // durability, not identity, and a store can report `UntilDelete` without
    // being anywhere near a keychain — `Durable` in tests/foreign_store.rs is
    // exactly such a store, and a whole round trip runs green through it.
    //
    // Nor does asking whether the vendor is *not* the mock, which is what this
    // assertion used to do. A negative names one impostor and waves the rest
    // through: measured, a store reporting `UntilDelete` under any other vendor
    // string passes both that check and the persistence check, and then stores,
    // reads back and forgets entirely in memory. So the assertion is positive.
    let vendor = keyring_core::get_default_store()
        .expect("storing a secret registers a default store")
        .vendor();
    assert!(
        vendor.contains(PLATFORM_STORE_CRATE),
        "this test is meant to exercise the OS credential store, so the registered \
         store must be {PLATFORM_STORE_CRATE}, but it is: {vendor}"
    );

    assert_eq!(load(&name).unwrap().as_deref(), Some(SYNTHETIC));

    forget(&name).unwrap();
    assert_eq!(load(&name).unwrap(), None);

    // Forgetting what is already gone is how the cleanup guard above ends, and
    // how removing a model configuration twice ends.
    forget(&name).unwrap();
}

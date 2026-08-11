//! Provider keys live in the OS credential store and NEVER in the database.
//!
//! The reason is section 9 of the spec: the team workflow copies the database
//! file between machines. A key parked "temporarily" beside the index would
//! travel with it — a working payment credential in someone else's hands.
//! `model_config.credential_ref` holds the NAME below, never the secret.
//!
//! # What this crate is careful about
//!
//! **Nothing here renders a secret.** [`Error`] carries the reference name, a
//! reason this crate wrote itself, and — for the two "the store is unreachable"
//! cases only — the `Display` of the platform error. It never carries a
//! [`keyring_core::Error`], because three of that type's variants hold
//! credential material: `BadEncoding(Vec<u8>)` and `BadDataFormat(Vec<u8>, _)`
//! hold the blob, and `Ambiguous(Vec<Entry>)` holds whole credentials whose
//! `Debug` reaches the stored secret. A `#[from] keyring_core::Error` wrapper
//! with a derived `Debug` prints those bytes on any `.unwrap()`, and the bytes
//! print as a numeric array rather than as text, so a leak looks like noise
//! instead of a key. `an_error_names_the_reference_and_never_the_secret` below
//! asserts both renderings.
//!
//! **A store that does not persist is refused.** `keyring_core::mock` is not
//! behind a feature, so it is compiled into release builds too, and a key handed
//! to it is not stored at all — it sits in process memory until the process
//! ends. Worse, `keyring_core` logs the credential's `Debug` on every operation
//! (`debug!("set password for entry {:?}", self.inner)`, and the same for get
//! and delete). On the shipped macOS path that `Debug` is the service and
//! account only, so it is harmless; the mock's `Debug` carries the secret, which
//! is exactly what the leak test below exploits. A debug-level logger in a build
//! where something registered the mock would therefore write every key to the
//! log. `ensure_default_store` accepts whichever store was registered first, so
//! that deference gets a floor: see `store_is_acceptable`.
//!
//! # Why not the `keyring` crate
//!
//! `keyring` 4.x is a 43-line shim over [`keyring_core`], and its `v1` module
//! installs the platform store into the process-wide default on the first
//! `Entry::new`, unconditionally. That leaves no way to point the tests at
//! `keyring_core::mock`, so every test would have to write into the developer's
//! own login keychain — which is the one thing this crate must not make normal.
//! `keyring`'s own documentation sends applications that "want to control which
//! credential stores they use" to `keyring-core` plus the store crates, and the
//! dependency set is identical either way: `keyring`'s default `v1` feature
//! resolves to exactly the three store crates named in this manifest.

use std::sync::{Arc, Mutex};

use keyring_core::{CredentialStore, Entry};

/// A store for other crates' integration tests. Read its header before enabling
/// the feature — it claims durability and keeps nothing, which is the one shape
/// [`store_is_acceptable`] exists to refuse.
#[cfg(feature = "test-store")]
pub mod test_store;

#[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    )
)))]
compile_error!(
    "mnema-secrets has no OS credential store for this target. There is deliberately \
     no fallback: a provider key that cannot go into the OS store must fail to be \
     stored, not quietly land in a file that travels with the database."
);

/// The service name every Mnema credential is filed under.
///
/// Private on purpose. The pair `(SERVICE, reference)` is this crate's whole
/// addressing scheme, and a caller that can name the service can also write
/// under a reference this crate would never produce.
const SERVICE: &str = "com.mnema.desktop";

/// A failure to reach, read or write the OS credential store.
///
/// Every variant names the reference. None can carry secret material: the
/// fields are the reference, a `&'static str` written here, a count, and the
/// `Display` — never the `Debug` — of a platform error. See the module docs.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An empty reference is refused before it reaches the store.
    ///
    /// Not defensive tidiness. In Keychain Services an empty attribute value is
    /// a *wildcard*, which is why `apple-native-keyring-store` rejects an empty
    /// user outright — so an empty `credential_ref` would match every Mnema
    /// credential in the keychain and hand back some other configuration's key.
    /// The mock store accepts it happily, so without this check the two backends
    /// disagree about a case that silently crosses secrets between configs.
    #[error(
        "a credential reference must not be empty: an empty one is a wildcard in the \
         macOS keychain, so it would match another configuration's key"
    )]
    EmptyReference,

    /// The store could not be reached, or refused to unlock.
    ///
    /// `detail` is the `Display` of the platform error — an OS status on macOS
    /// and Windows, a D-Bus error on Secret Service. It is the only text here
    /// this crate did not write, and it is taken from `Display` rather than
    /// `Debug` precisely because `Debug` on a keyring error can reach the blob.
    #[error("the credential store is unavailable, so `{reference}` could not be reached: {detail}")]
    Unavailable { reference: String, detail: String },

    /// The store answered, but not with a usable secret. The stored bytes, if
    /// any, are dropped here rather than reported.
    #[error("the credential store holds `{reference}` but would not return it: {reason}")]
    Unreadable {
        reference: String,
        reason: &'static str,
    },

    /// More than one credential matches. The matching entries are counted and
    /// dropped — their `Debug` reaches the secrets they hold.
    #[error("the credential store holds {count} credentials named `{reference}`")]
    Ambiguous { reference: String, count: usize },

    /// The store rejected the request. `reason` is built from a keyring error's
    /// attribute *name* and explanation, never from a value.
    #[error("the credential store refused `{reference}`: {reason}")]
    Refused { reference: String, reason: String },

    /// A store is registered, but it does not keep what it is given.
    ///
    /// Refusing is the only safe answer. Accepting would report success for a
    /// key that is gone at the next launch, and — if the store is
    /// `keyring_core::mock` — would put the secret where a debug-level logger
    /// prints it. `vendor` is the store's own self-description, not a value.
    #[error(
        "the registered credential store does not keep credentials, so `{reference}` \
         would not really be stored: {vendor}"
    )]
    NotPersistent { reference: String, vendor: String },
}

impl Error {
    /// Maps a keyring error, keeping the reference and dropping every payload
    /// that could be credential material.
    fn from_keyring(reference: &str, err: keyring_core::Error) -> Self {
        use keyring_core::Error as K;

        let reference = reference.to_string();
        match err {
            K::PlatformFailure(cause) | K::NoStorageAccess(cause) => Error::Unavailable {
                reference,
                detail: cause.to_string(),
            },
            K::NoDefaultStore => Error::Unavailable {
                reference,
                detail: "no credential store is registered in this process".to_string(),
            },
            // Unreachable through this crate's three functions, which turn
            // `NoEntry` into `None` and `Ok(())` before it can get here. It is
            // still mapped rather than ignored, because a wrapped credential can
            // be deleted between being opened and being written.
            K::NoEntry => Error::Unreadable {
                reference,
                reason: "the credential was deleted while it was being used",
            },
            K::BadEncoding(_) => Error::Unreadable {
                reference,
                reason: "the stored value is not valid UTF-8",
            },
            K::BadDataFormat(_, _) => Error::Unreadable {
                reference,
                reason: "the store returned a blob it could not decode",
            },
            K::BadStoreFormat(_) => Error::Unreadable {
                reference,
                reason: "the store's own data is malformed",
            },
            K::Ambiguous(entries) => Error::Ambiguous {
                reference,
                count: entries.len(),
            },
            // Both strings are attribute *names* and explanations in every store
            // this crate links: `Invalid("user", "cannot be empty")`. Audited
            // against apple-native-keyring-store 1.0.1; re-read it on a bump —
            // and re-read it for the other two stores before the first Windows
            // or Linux build, since neither compiles on macOS and neither has
            // been read. The same audit covers `Unavailable.detail` above, which
            // is the `Display` of a platform error and the only other text in
            // this type that this crate did not write.
            K::TooLong(attribute, limit) => Error::Refused {
                reference,
                reason: format!("`{attribute}` is longer than the platform limit of {limit}"),
            },
            K::Invalid(attribute, why) => Error::Refused {
                reference,
                reason: format!("`{attribute}` {why}"),
            },
            K::NotSupportedByStore(why) => Error::Refused {
                reference,
                reason: why,
            },
            // `keyring_core::Error` is `#[non_exhaustive]`, so a minor release
            // can add a variant. Anything unknown is reported without its
            // payload, because an unknown payload cannot be shown to be safe.
            _ => Error::Unreadable {
                reference,
                reason: "the credential store failed in a way this build does not recognise",
            },
        }
    }
}

/// Writes `secret` into the OS credential store under `reference`, replacing
/// whatever was there.
pub fn store(reference: &str, secret: &str) -> Result<(), Error> {
    entry(reference)?
        .set_password(secret)
        .map_err(|e| Error::from_keyring(reference, e))
}

/// Reads the secret filed under `reference`.
///
/// A reference that was never written, or has been forgotten, is `Ok(None)` —
/// the ordinary state of a model configuration whose key the user has not
/// entered yet, not an error.
pub fn load(reference: &str) -> Result<Option<String>, Error> {
    match entry(reference)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(e) => Err(Error::from_keyring(reference, e)),
    }
}

/// What a [`forget`] that succeeded actually did.
///
/// Deleting a credential that is not there is not a failure — the caller asked
/// for it to be gone and it is — but it is not the same event either, and this
/// crate is the only layer that can still tell them apart: the store reports
/// `NoEntry` and nothing above ever sees it. Answering `()` to both let a
/// window tell somebody who had entered no key that this application had just
/// removed one (whole-branch review, I1).
///
/// Asking the store beforehand would be a second measurement that can disagree
/// with the one the deletion actually made — the argument
/// `mnema_desktop::models::set_embedding_model` already writes down for the key
/// it loads. This is the first measurement, reported instead of discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forgotten {
    /// A credential was filed under this reference, and is not any more.
    Removed,
    /// There was none to remove.
    NothingToRemove,
}

/// Deletes the secret filed under `reference`. Deleting one that is not there
/// succeeds — see [`Forgotten`] for why it succeeds with a different answer.
pub fn forget(reference: &str) -> Result<Forgotten, Error> {
    match entry(reference)?.delete_credential() {
        Ok(()) => Ok(Forgotten::Removed),
        Err(keyring_core::Error::NoEntry) => Ok(Forgotten::NothingToRemove),
        Err(e) => Err(Error::from_keyring(reference, e)),
    }
}

fn entry(reference: &str) -> Result<Entry, Error> {
    if reference.is_empty() {
        return Err(Error::EmptyReference);
    }
    ensure_default_store(reference)?;
    Entry::new(SERVICE, reference).map_err(|e| Error::from_keyring(reference, e))
}

/// Serialises the one-time installation below. Not the store's own lock — the
/// stores do their own locking; this only stops two threads racing to install.
static INSTALLING: Mutex<()> = Mutex::new(());

/// Registers the platform store as the process default, once.
///
/// Double-checked, and it checks rather than assumes: whoever set a default
/// store first keeps it. That is what lets the tests below install
/// `keyring_core::mock` and have `store`/`load`/`forget` run against it
/// unchanged, rather than against a separate code path that proves nothing.
///
/// Deferring like that is only safe with a floor under it, because the store
/// that got there first is not necessarily one that stores anything. Every path
/// out of here therefore ends at [`check_persistence`] — including the one where
/// this function installed the store itself, since a future platform store could
/// change what it reports.
fn ensure_default_store(reference: &str) -> Result<(), Error> {
    if let Some(registered) = keyring_core::get_default_store() {
        return check_persistence(reference, &registered);
    }
    let _installing = INSTALLING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(registered) = keyring_core::get_default_store() {
        return check_persistence(reference, &registered);
    }
    let store = platform_store(reference)?;
    check_persistence(reference, &store)?;
    keyring_core::set_default_store(store);
    Ok(())
}

/// Whether a registered store actually keeps what it is given.
///
/// Asks `persistence()` rather than matching on `vendor()`. The property that
/// matters is "a credential put here outlives the process and the machine", not
/// "this is not the one mock we happen to know the name of" — a name test
/// catches `keyring_core::mock` and nothing else, while this catches any store
/// that would lose the key, including ones written after this line.
///
/// `CredentialPersistence` is `#[non_exhaustive]` and its trait default is
/// `UntilDelete`, so this accepts only the disk-backed variant by name and
/// refuses everything else — `EntryOnly` and `ProcessOnly`, which lose the key
/// immediately, `UntilLogout` and `UntilReboot`, which lose it later and more
/// confusingly, `Unspecified`, and any variant a future minor release adds.
fn store_is_acceptable(store: &Arc<CredentialStore>) -> bool {
    matches!(
        store.persistence(),
        keyring_core::CredentialPersistence::UntilDelete
    )
}

fn check_persistence(reference: &str, store: &Arc<CredentialStore>) -> Result<(), Error> {
    // The unit tests in this file register `keyring_core::mock` deliberately,
    // and it is `ProcessOnly` by design, so under `cfg(test)` the check would
    // reject the very thing the tests are built on. It is not skipped anywhere
    // else, and it is not tested here either: `tests/foreign_store.rs` compiles
    // this crate without `cfg(test)`, registers a non-persistent store, and
    // asserts the refusal on the shipped path.
    if cfg!(test) || store_is_acceptable(store) {
        return Ok(());
    }
    Err(Error::NotPersistent {
        reference: reference.to_string(),
        vendor: store.vendor(),
    })
}

fn platform_store(reference: &str) -> Result<Arc<CredentialStore>, Error> {
    // A unit test that reaches this point forgot to install the mock store, and
    // is one call away from writing a credential into the developer's own login
    // keychain — or, in CI, from blocking on an unlock prompt that no one will
    // answer. Fail closed and say which reference did it. Integration tests are
    // compiled without `cfg(test)` and are unaffected: `tests/roundtrip.rs` is
    // meant to reach the real store, and is `#[ignore]`d for that reason.
    #[cfg(test)]
    {
        Err(Error::Unavailable {
            reference: reference.to_string(),
            detail: "a unit test must install keyring_core::mock before touching the store"
                .to_string(),
        })
    }
    #[cfg(not(test))]
    {
        #[cfg(target_os = "macos")]
        let built = apple_native_keyring_store::keychain::Store::new();
        #[cfg(target_os = "windows")]
        let built = windows_native_keyring_store::Store::new();
        #[cfg(all(
            unix,
            not(any(target_os = "macos", target_os = "ios", target_os = "android"))
        ))]
        let built = zbus_secret_service_keyring_store::Store::new();

        match built {
            Ok(store) => Ok(store),
            Err(e) => Err(Error::from_keyring(reference, e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use keyring_core::mock;

    use super::{
        Error, Forgotten, SERVICE, forget, load, platform_store, store, store_is_acceptable,
    };

    /// Synthetic, and shaped so it cannot be mistaken for a provider key: no
    /// `sk-` prefix, no base64 tail, and it says what it is.
    const SYNTHETIC: &str = "mnema-synthetic-value-not-a-key-aaaa";
    /// A second one, so a test can tell "the right secret came back" from "a
    /// secret came back".
    const OTHER_SYNTHETIC: &str = "mnema-synthetic-value-not-a-key-bbbb";

    static MOCK: Once = Once::new();

    /// Installs the in-memory store for this test binary.
    ///
    /// Every test calls this first. A test that forgets does not fall through to
    /// the login keychain — `platform_store` refuses under `cfg(test)` — it
    /// fails with that refusal, which is the point.
    fn mock_store() {
        MOCK.call_once(|| {
            keyring_core::set_default_store(mock::Store::new().expect("the mock store builds"));
        });
    }

    #[test]
    fn a_unit_test_cannot_reach_the_platform_store() {
        mock_store();
        // Reachable directly, with no process-global state involved: building a
        // platform store constructs a struct and touches no keychain, so this
        // reds by returning `Ok` when the guard is removed rather than by
        // writing a credential. An earlier report claimed this could only be
        // tested by unsetting the default store and serialising the module.
        // That was wrong.
        assert!(
            matches!(platform_store("guard"), Err(Error::Unavailable { .. })),
            "cfg(test) must keep the login keychain out of reach of unit tests"
        );
    }

    #[test]
    fn the_store_these_tests_run_against_would_be_refused_in_a_release_build() {
        mock_store();
        // The other half of the guard above, and the reason `check_persistence`
        // exists: the store these tests are built on is exactly the kind the
        // shipped path must not accept. If this ever passes, either the mock
        // started claiming durability or the predicate stopped asking.
        let registered = keyring_core::get_default_store().expect("the mock is registered");
        assert!(
            !store_is_acceptable(&registered),
            "an in-memory store must not be acceptable to the shipped path: {}",
            registered.vendor()
        );
    }

    #[test]
    fn a_secret_round_trips_through_the_store() {
        mock_store();
        let name = "roundtrip";

        store(name, SYNTHETIC).unwrap();

        assert_eq!(load(name).unwrap().as_deref(), Some(SYNTHETIC));
    }

    #[test]
    fn each_reference_holds_its_own_secret() {
        mock_store();
        // The round trip above passes just as well if `reference` is ignored and
        // every key lands in one slot — which is precisely the bug that hands a
        // model configuration someone else's credential. Two references, two
        // different values, read back crossed.
        store("own-secret-alpha", SYNTHETIC).unwrap();
        store("own-secret-beta", OTHER_SYNTHETIC).unwrap();

        assert_eq!(
            load("own-secret-alpha").unwrap().as_deref(),
            Some(SYNTHETIC)
        );
        assert_eq!(
            load("own-secret-beta").unwrap().as_deref(),
            Some(OTHER_SYNTHETIC)
        );
    }

    #[test]
    fn storing_twice_replaces_the_secret() {
        mock_store();
        let name = "replaced";

        store(name, SYNTHETIC).unwrap();
        store(name, OTHER_SYNTHETIC).unwrap();

        // Also the answer to "did the round trip above just echo the argument
        // back": here the argument to the last `store` and the value a cache
        // would be holding are different strings.
        assert_eq!(load(name).unwrap().as_deref(), Some(OTHER_SYNTHETIC));
    }

    #[test]
    fn an_absent_reference_is_none_rather_than_an_error() {
        mock_store();
        // The `Some` half is here on purpose: an implementation where every
        // lookup returns `None` satisfies the assertion below and nothing else.
        store("absent-control", SYNTHETIC).unwrap();
        assert_eq!(load("absent-control").unwrap().as_deref(), Some(SYNTHETIC));

        assert_eq!(load("absent-never-written").unwrap(), None);
    }

    #[test]
    fn forget_removes_only_the_named_reference() {
        mock_store();
        store("forget-dropped", SYNTHETIC).unwrap();
        store("forget-kept", OTHER_SYNTHETIC).unwrap();

        forget("forget-dropped").unwrap();

        assert_eq!(load("forget-dropped").unwrap(), None);
        assert_eq!(
            load("forget-kept").unwrap().as_deref(),
            Some(OTHER_SYNTHETIC),
            "forgetting one reference must not clear the store"
        );
    }

    #[test]
    fn forgetting_a_reference_that_was_never_stored_is_not_an_error() {
        mock_store();
        // Removing a model configuration must not fail because its key was never
        // entered, and re-running a cleanup must not fail on the second pass.
        forget("forget-never-stored").unwrap();
        forget("forget-never-stored").unwrap();
    }

    /// The same call, two events, and this is the only layer that can still see
    /// which one happened: the store says `NoEntry` and nothing above it does.
    /// Both directions, because a `forget` that answered `Removed` to
    /// everything is exactly the build this exists to stop, and one that
    /// answered `NothingToRemove` to everything would satisfy the second half
    /// alone (whole-branch review, I1).
    #[test]
    fn forgetting_says_whether_there_was_anything_to_forget() {
        mock_store();
        store("forget-reported", SYNTHETIC).unwrap();

        assert_eq!(forget("forget-reported").unwrap(), Forgotten::Removed);
        assert_eq!(
            forget("forget-reported").unwrap(),
            Forgotten::NothingToRemove,
            "the second call removed nothing, and a window that is told otherwise \
             tells a person this application deleted a key they never had"
        );
    }

    #[test]
    fn an_empty_reference_is_refused_rather_than_matching_everything() {
        mock_store();
        // The mock accepts an empty user; the macOS keychain treats it as a
        // wildcard. Neither behaviour is one this crate should pass through.
        assert!(matches!(store("", SYNTHETIC), Err(Error::EmptyReference)));
        assert!(matches!(load(""), Err(Error::EmptyReference)));
        assert!(matches!(forget(""), Err(Error::EmptyReference)));
    }

    #[test]
    fn an_error_names_the_reference_and_never_the_secret() {
        mock_store();
        let name = "leak-check";
        store(name, SYNTHETIC).unwrap();

        // The mock lets a test choose the next error the store returns, so the
        // two `keyring_core::Error` variants that structurally carry credential
        // material can be aimed at `load` directly:
        //
        //   BadEncoding(Vec<u8>) — the stored blob, in the error.
        //   Ambiguous(Vec<Entry>) — whole credentials, whose `Debug` reaches the
        //                           secret the mock is holding for `name`.
        //
        // Both are what `#[error("credential store: {0}")] Keyring(#[from] ...)`
        // would print.
        let entry = keyring_core::Entry::new(SERVICE, name).unwrap();
        let cred: &mock::Cred = entry
            .as_any()
            .downcast_ref()
            .expect("the mock store builds mock credentials");

        // A `Vec<u8>` prints as `[109, 110, ...]`, not as text, so searching the
        // rendering for the secret *as a string* is an assertion that can only
        // pass — the leak this test exists for does not look like the secret.
        // Both shapes are checked.
        let as_text = SYNTHETIC.to_string();
        let as_bytes = format!("{:?}", SYNTHETIC.as_bytes());

        for injected in [
            keyring_core::Error::BadEncoding(SYNTHETIC.as_bytes().to_vec()),
            keyring_core::Error::Ambiguous(vec![
                keyring_core::Entry::new(SERVICE, name).expect("the entry opens"),
            ]),
        ] {
            let injected_label = format!("{injected:?}");
            cred.set_error(injected);

            let err = load(name).expect_err("the injected error must reach the caller");

            for (shape, rendering) in [("Display", err.to_string()), ("Debug", format!("{err:?}"))]
            {
                assert!(
                    !rendering.contains(&as_text),
                    "the {shape} of the error for {injected_label} contains the secret \
                     as text: {rendering}"
                );
                assert!(
                    !rendering.contains(&as_bytes),
                    "the {shape} of the error for {injected_label} contains the secret \
                     as bytes: {rendering}"
                );
                assert!(
                    rendering.contains(name),
                    "the {shape} of the error for {injected_label} does not name the \
                     entry it is about: {rendering}"
                );
            }
        }
    }

    #[test]
    fn the_leak_test_would_see_a_leak() {
        // Guards the test above against being vacuous. If `as_bytes` stopped
        // matching how a `Vec<u8>` renders, or the mock stopped putting the
        // secret in its `Debug`, every assertion up there would pass while
        // checking nothing. This asserts the leak is visible in the first place.
        mock_store();
        let name = "leak-check-control";
        store(name, SYNTHETIC).unwrap();

        let leaky = keyring_core::Error::Ambiguous(vec![
            keyring_core::Entry::new(SERVICE, name).expect("the entry opens"),
        ]);
        let rendering = format!("{leaky}{leaky:?}");
        assert!(
            rendering.contains(&format!("{:?}", SYNTHETIC.as_bytes())),
            "a keyring error holding this credential no longer renders the secret, so \
             an_error_names_the_reference_and_never_the_secret is no longer testing \
             anything: {rendering}"
        );
    }
}

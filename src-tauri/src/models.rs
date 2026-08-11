//! Model configuration: the key, the three role choices, and what the window
//! needs to draw the state of them.
//!
//! v1 speaks to OpenRouter alone (spec §2.2), so there is one credential and its
//! reference is a constant rather than a row. The database holds no key and no
//! reference of its own — "is there a key?" is a question for the credential
//! store, and every answer here comes from asking it rather than from a flag
//! this module could get wrong.

use serde::Serialize;
use tauri::State;

use mnema_provider::{Catalogue, Role};

use crate::error::Error;
use crate::state::AppState;

/// The name this product files its one credential under in production. A
/// constant because there is one account in v1; a second provider turns it into
/// a column.
///
/// The value the commands actually use comes from [`AppState::credential_ref`],
/// not from here. That indirection is what lets an integration test write under
/// a reference of its own — see that field for what each of the two test-side
/// guards buys, since neither one alone is enough.
pub const CREDENTIAL_REF: &str = "openrouter";

/// Whether a key has been entered. Asks the store, because that is where the
/// answer is — the database records nothing about it.
#[tauri::command(async)]
pub fn key_present(state: State<'_, AppState>) -> Result<bool, Error> {
    Ok(mnema_secrets::load(state.credential_ref())?.is_some())
}

/// Checks the key, and stores it only if the provider accepted it.
///
/// The order is the whole point: a key that does not work, stored anyway, leaves
/// the application believing it is configured and failing at every call
/// afterwards — three hours into an indexing run, at the earliest place anyone
/// would look.
///
/// **An empty key is refused here and not by the provider** ([`Error::EmptyKey`],
/// where the message this replaces is written out). The refusal is in the
/// command rather than in the window because the window is not the only caller:
/// a guard in `main.js` leaves this command reachable over the IPC with exactly
/// the old result, and it is this side that decides whether a request leaves the
/// machine at all. It is also the only side that can keep the sentence single —
/// the window renders whatever this returns, so a second refusal over there
/// would be a second wording of one fact.
///
/// **Empty, not blank.** A key of spaces is refused by the provider and not
/// here, and the two directions of getting that wrong are not symmetric:
/// trimming would store something other than what a person typed, and calling
/// spaces "nothing was typed" states a fact about them that this build did not
/// observe. `mnema_secrets::entry` draws the same line for a reference, one
/// field over.
#[tauri::command(async)]
pub fn set_key(state: State<'_, AppState>, key: String) -> Result<KeyStatus, Error> {
    if key.is_empty() {
        return Err(Error::EmptyKey);
    }
    let check = mnema_provider::check_key(state.provider_base(), &key)?;
    mnema_secrets::store(state.credential_ref(), &key)?;
    Ok(KeyStatus {
        balance: check.balance,
    })
}

/// Removes the key. What was embedded stays embedded; what stops is embedding
/// anything new and answering a question, because the question must be embedded
/// too (D29).
///
/// **It answers which of the two things happened**, because `Ok(())` was one
/// answer to two events and the window turned it into one sentence: "the key
/// was removed", to somebody who had entered none or whose key a second window
/// had already taken (whole-branch review, I1). It is the same press and the
/// same class as the empty field [`set_key`] refuses — a button that reports an
/// event it did not cause.
///
/// The fact is not measured a second time here. `mnema_secrets::forget` sees
/// `NoEntry` from the store and used to discard it; it reports it now, and this
/// command passes it on. Asking `load` first would be the second measurement
/// [`set_embedding_model`] argues against on the same store.
#[tauri::command(async)]
pub fn forget_key(state: State<'_, AppState>) -> Result<KeyRemoval, Error> {
    Ok(KeyRemoval::of(mnema_secrets::forget(
        state.credential_ref(),
    )?))
}

/// What [`forget_key`] did, on the wire.
///
/// Tagged with `kind`, the convention every union this module sends the window
/// already uses. It is [`mnema_secrets::Forgotten`] one layer out rather than
/// that type re-exported: `mnema-secrets` knows nothing about a window and
/// carries no serde, and the pin that fixes these spellings
/// ([`Self::every_discriminant_the_window_sees_has_its_camel_case_spelling_pinned`])
/// lives in this crate for the reason its own doc gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum KeyRemoval {
    /// There was a key filed under this installation's reference, and there is
    /// not any more.
    Removed,
    /// There was none. **Not a failure** — the caller asked for the key to be
    /// gone and it is — and not the same sentence either.
    NothingToRemove,
}

impl KeyRemoval {
    /// Exhaustive over `mnema_secrets::Forgotten`, with no wildcard, for the
    /// reason [`KeyStoreFailure::of`] gives about that crate's error type: a
    /// third value added over there stops this compiling until somebody decides
    /// what the window says about it.
    fn of(forgotten: mnema_secrets::Forgotten) -> Self {
        match forgotten {
            mnema_secrets::Forgotten::Removed => Self::Removed,
            mnema_secrets::Forgotten::NothingToRemove => Self::NothingToRemove,
        }
    }
}

/// What the window draws after a key is accepted.
///
/// It carries no `present` flag. There was one, set to a literal `true` at the
/// single place this type is built, and a literal is not a measurement: it told
/// the caller nothing that `Ok` had not already told it, it could not be wrong
/// today, and it would be wrong the first time `set_key` grew a path that
/// returns without storing. "Is there a key?" has one answer in this build and
/// it comes from asking the store — [`key_present`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    /// Four states, not "a number or nothing". Two states collapse "the
    /// provider did not mention a balance" into "the provider stated one this
    /// build could not read", and the shortest way to render the collapsed
    /// value is to substitute zero — which puts "0 credits" in front of someone
    /// whose account is funded, and sends them to pay again. Two of the four
    /// states are also bug reports for us rather than facts about the account,
    /// and they are worth telling apart when one arrives.
    ///
    /// It carries provider bytes on the `Unreadable` arm, which is why this
    /// type is in the leak scan and not only the error type: they have been
    /// through the same sanitising pipeline every other provider string in that
    /// crate goes through, and `tests/model_commands.rs` runs that arm rather
    /// than trusting the sentence.
    pub balance: mnema_provider::Balance,
}

/// The key, or the fact that nobody has entered one.
///
/// `pub(crate)`, and deliberately not a command: a `#[tauri::command]` returning
/// a `String` here is precisely how the key would cross to the window, which is
/// the one thing this module exists to prevent.
///
/// The absence of a key is [`Error::NoKey`] and not [`Error::Secrets`] — see
/// that variant for the two opposite things those tell the person at the window
/// to do next. This function is where the split is made, and
/// [`set_embedding_model`] is the caller that makes it reachable.
pub(crate) fn key(state: &AppState) -> Result<String, Error> {
    mnema_secrets::load(state.credential_ref())?.ok_or(Error::NoKey)
}

/// The role the window named, or a refusal.
///
/// Written as a total match with no fallthrough: the alternative that suggests
/// itself — treat anything unrecognised as chat — is the one that answers a
/// question about embedders with the chat catalogue. See [`Error::UnknownRole`].
fn role_from(name: &str) -> Result<Role, Error> {
    match name {
        "embedding" => Ok(Role::Embedding),
        "rerank" => Ok(Role::Rerank),
        "chat" => Ok(Role::Chat),
        other => Err(Error::UnknownRole(other.to_string())),
    }
}

/// The provider's list for one role. No key: this endpoint is public (measured
/// 2026-08-08), which is what lets the choice be shown before an account exists.
///
/// Hands the window the whole [`Catalogue`], not just its entries. The count of
/// records this build could not read — and the ids behind them — exist so the
/// window can say "three models this application could not read" instead of
/// showing a list quietly shorter than the provider's. Unwrapping it here would
/// undo that fix at the last seam before the user. The same goes for an empty
/// but well-formed list, which `mnema_provider::list_models` deliberately
/// returns as a success: whether zero selectable models is worth alarming
/// anybody about is a question for whoever renders it, and it can tell "the
/// provider has none" from "something upstream ate them" only while both
/// numbers are still here.
#[tauri::command(async)]
pub fn provider_models(state: State<'_, AppState>, role: String) -> Result<Catalogue, Error> {
    Ok(mnema_provider::list_models(
        state.provider_base(),
        None,
        role_from(&role)?,
    )?)
}

/// Checks the model, then records it. The dimension written to the index is the
/// one the provider answered with, never one anybody typed: the model list
/// states no width in any field (measured 2026-08-08) and the same model name
/// answers 1536 or 1024 depending on a parameter (spec §2.4), so a width taken
/// from anywhere but the answer is a guess that builds a vector space.
///
/// The credential reference written into `model_config` is
/// [`AppState::credential_ref`] and **not** [`CREDENTIAL_REF`]. They are the
/// same string in the application and different ones under test, which is what
/// keeps a test binary out of the developer's own keychain; writing the constant
/// here would record a name this installation does not use.
///
/// **It answers with the adoption and not with [`ModelSettings`]**, so that "the
/// model was recorded" is a fact on the wire rather than something the window
/// has to infer from a command that returned `Ok`. The first version read the
/// settings back and returned those, which meant a read-back that failed on its
/// own turned a recorded model into a rejected command: the window could not
/// tell "nothing was written" from "written, and reading it back failed", and no
/// wording could have told them apart, because the fact was not in the message.
/// It is not inferred here either — see [`AdoptedModel::created`].
///
/// The store is asked **once**, by `key`. Asking it again for a `key_present`
/// would be a second measurement that can disagree with the one this command
/// actually used: a concurrent `forget_key` would report the key as absent on a
/// call that has just succeeded with it. The window does not need the answer in
/// any case — a success here is a call that had the key, and the other arm is
/// [`Error::NoKey`].
#[tauri::command(async)]
pub fn set_embedding_model(
    state: State<'_, AppState>,
    model: String,
) -> Result<AdoptedModel, Error> {
    let key = key(&state)?;
    let check = mnema_provider::check_embedding_model(state.provider_base(), &key, &model)?;
    let hash = mnema_chunk::chunker_hash();
    let dim = check.dim as i64;
    let adopted = state
        .with_index(|db| db.adopt_embedding_model(&model, dim, state.credential_ref(), &hash))?;
    Ok(AdoptedModel {
        model,
        dim,
        space_id: adopted.space_id,
        created: adopted.created,
        index: index_settings(&state),
    })
}

/// What [`set_embedding_model`] recorded, said rather than implied.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptedModel {
    pub model: String,
    /// The width the provider answered with, which is the width that was
    /// written. Kept beside `index` rather than read out of it: `index` says
    /// what the database holds, this says what this call put there, and the two
    /// disagreeing is information rather than noise. It is also the only carrier
    /// left when `index` is [`IndexSettings::Unreadable`].
    pub dim: i64,
    pub space_id: i64,
    /// Whether this call minted the space or found one. **Stated by the index,
    /// never inferred here** — `Db::adopt_embedding_model` reads it from
    /// `create_space`'s own answer precisely because the neighbouring fact ("the
    /// active space moved") is a different one, and agrees with it everywhere
    /// except on a model chosen, abandoned and chosen again. Deriving it from
    /// `embeddedChunks` would be worse still: that number is identically zero in
    /// this build (D29), so the derivation is wrong in exactly one direction.
    pub created: bool,
    /// The settings as they now stand, or why they could not be read — see
    /// [`IndexSettings`]. A failure here does not take the four fields above
    /// with it, which is the whole point of answering with this type.
    ///
    /// ⚠️ **No test reaches the [`IndexSettings::Unreadable`] arm on *this*
    /// type**, and it is worth knowing that before relying on it. The adoption
    /// two lines up went through the same open index, so by the time this is
    /// built the index has just been written to successfully; what is left is a
    /// lock another thread poisoned in between, which no test here can arrange.
    /// The guarantee is carried by the shape rather than by a run: the four
    /// fields above are not inside `index`, so no reading failure can reach
    /// them. `model_settings` exercises the same arm on the same type
    /// (`a_key_that_is_there_survives_an_index_that_is_not`).
    pub index: IndexSettings,
}

/// Leaves nothing on disk, so it needs no check and no space (spec §2.1).
///
/// No provider call either, and that is the asymmetry with
/// [`set_embedding_model`] rather than an omission: a rerank model that turns
/// out not to exist costs one failed query, while an embedding model that does
/// not is a vector space built at a width nobody can reproduce.
#[tauri::command(async)]
pub fn set_rerank_model(state: State<'_, AppState>, model: String) -> Result<(), Error> {
    state.with_index(|db| db.meta_set(mnema_index::META_RERANK_MODEL, &model))?;
    Ok(())
}

/// Same as [`set_rerank_model`].
#[tauri::command(async)]
pub fn set_chat_model(state: State<'_, AppState>, model: String) -> Result<(), Error> {
    state.with_index(|db| db.meta_set(mnema_index::META_CHAT_MODEL, &model))?;
    Ok(())
}

/// Everything the settings screen draws: the key, and the index.
///
/// **Two halves, because they are two facts and they fail separately.** The key
/// lives in the OS credential store and the rest lives in a database, and the
/// database is not open until the window asks it to be — `AppState::db` is
/// `None` until the first `open_index`, and an index written by a newer Mnema
/// does not open at all, which is a state the application stays in rather than
/// passes through. The settings screen is exactly the screen someone opens then.
///
/// The first version measured the key, then let `with_index` fail, and the
/// measurement died with the call. One message for two facts: the window could
/// not say "your key is there, the database did not open", and an empty state
/// drawn from that failure tells someone who **has** a key that they have none —
/// the sentence [`Error::NoKey`]'s own doc calls forbidden.
///
/// The second version fixed that half and left the other: a store that would not
/// answer still took the whole index reading with it. Both halves are answers
/// now, and the command has no failure path at all — see [`model_settings`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettings {
    /// Measured, never a literal — every arm comes from one
    /// `mnema_secrets::load(...)`. See [`KeyStatus`] for what a second source of
    /// this answer cost when it was tried.
    pub key: KeyState,
    pub index: IndexSettings,
    /// Which credential store this build talks to, so the window can say the one
    /// thing that is true on exactly one of them. See [`Platform`].
    pub platform: Platform,
}

/// The operating system this build was compiled for, as a fact rather than a
/// guess.
///
/// It rides on [`ModelSettings`] because that is the payload the settings screen
/// is drawn from, and the sentence that needs it is drawn there — a constant on
/// every read is cheaper than a second command whose only answer never changes.
///
/// **The window must not work this out for itself.** `navigator.userAgent` is
/// available to it and would be a plausible proxy; this project has measured
/// twice, on two platforms, that a plausible proxy is wrong in the direction a
/// test does not catch. The build knows, so the build says.
///
/// What it is for: on macOS the credential store authorises against the code
/// identity that wrote the credential, and an ad-hoc signature is a hash of the
/// binary, so **every update makes this application a stranger to its own key**
/// and the system asks for the login keychain password once. Nothing of the sort
/// happens on the other two — Secret Service unlocks per session and the Windows
/// Credential Manager per logon, neither per binary — so a sentence explaining it
/// belongs on one platform and would be noise on the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    Mac,
    Windows,
    Linux,
}

impl Platform {
    /// Chosen at compile time, so it cannot disagree with the store that is
    /// actually linked: `mnema_secrets::platform_store` selects its backend from
    /// the same three `cfg`s, and "everything that is not macOS or Windows" is
    /// the arm that crate uses too.
    fn of_this_build() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::Mac
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Self::Linux
        }
    }
}

/// What the credential store said, as a third state rather than as a rejected
/// command.
///
/// **This is deliberately not the shape [`key_present`] has**, and the asymmetry
/// is the point rather than an oversight. `key_present` answers one question, so
/// a store that will not answer it has nothing left to report and `Err` is the
/// whole truth. `model_settings` answers **two**, and the first version of this
/// fix protected only the second: `key_present: mnema_secrets::load(…)?` fired
/// before `index_settings` was ever called, so a locked keychain took
/// `embedding_model`, `embedding_dim`, `active_space`, `embedded_chunks`,
/// `total_chunks`, `rerank_model` and `chat_model` with it — seven facts with no
/// connection to a keychain, on the only command that carries them to the
/// window. It was the fix for that same defect, applied to one half and not the
/// other.
///
/// Reachable, and not by a defect: `mnema_secrets::Error::Unavailable` is a
/// locked keychain on macOS or an absent Secret Service session on Linux,
/// `NotPersistent` and `Ambiguous` are likewise states of the machine.
#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum KeyState {
    /// The store answered, and it holds a key. It does not carry the key, and it
    /// never may.
    Present,
    /// The store answered, and it holds none — the normal state of an
    /// application nobody has signed into, and the one a sign-in panel belongs
    /// to.
    Absent,
    /// The store would not answer. A different thing from `Absent`, and the
    /// distinction [`Error::NoKey`] exists for: telling someone whose keychain
    /// is merely locked that they have entered no key sends them to re-enter one
    /// they already have.
    ///
    /// `cause` for the same reason [`IndexSettings::Unreadable`] has one, and
    /// added a commit later because that fix was made to one half and not the
    /// other. `mnema_secrets::Error` has six variants asking for **three
    /// different things from the person at the window** — unlock the store,
    /// remove a duplicate, send a bug report — and a single sentence flattens a
    /// distinction the credential crate drew with a typed enum one layer up.
    ///
    /// ⚠️ **`reason` is diagnostic text and not the sentence to show.**
    /// `mnema_secrets::Error::Unavailable` interpolates the platform error —
    /// an OS status on macOS and Windows, a D-Bus error on Secret Service — and
    /// a status code put in front of a person is not an action. The window's
    /// sentence comes from `cause`; `reason` belongs wherever a bug report is
    /// pasted. It carries no secret: every variant of that type names the
    /// reference and never the credential, by construction.
    Unreadable {
        cause: KeyStoreFailure,
        reason: String,
    },
}

/// Why the credential store would not answer, grouped by what it leaves the
/// person to do.
///
/// Four values over six error variants, and the grouping is the whole content:
/// two of them are things somebody can go and fix, one is a bug report, and one
/// depends on what the store said. Splitting them further would name causes the
/// window has no different answer for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyStoreFailure {
    /// The store would not answer, and **this value stands for two situations
    /// rather than one**: it is locked, or a confirmation was asked for and not
    /// given. Nothing about the person's configuration is wrong in either.
    ///
    /// Naming both is not a hedge. The platform error arrives already flattened
    /// — every status the store does not recognise becomes one variant — so this
    /// build genuinely cannot tell them apart, and the earlier doc here, which
    /// said only "a locked keychain", was falsified by measurement on
    /// 2026-08-11: macOS reaches this value with a keychain that is not locked
    /// at all, when the authorisation dialog is declined, because that store
    /// authorises against the code identity that wrote the credential and an
    /// ad-hoc signature changes with every build. Linux reaches it both ways,
    /// the second as a dismissed prompt. The window's sentence therefore names
    /// both and claims neither.
    Locked,
    /// More than one credential is filed under this installation's name. The
    /// person removes the duplicate; this build will not guess which of them is
    /// the key.
    Duplicate,
    /// The store was reached and would not hand the credential over. Whether
    /// there is anything to do depends on what it said, which is what `reason`
    /// is for — this is the one value that does not name an action.
    Refused,
    /// A defect in this build rather than a state of the machine: a registered
    /// store that does not keep what it is given, or a credential reference
    /// this build left empty. A bug report, and nothing the person can act on.
    Defect,
}

impl KeyStoreFailure {
    /// Classifies what the credential store answered with.
    ///
    /// **Exhaustive over `mnema_secrets::Error`, with no wildcard**, and that is
    /// the guard rather than the mapping: the type is not `#[non_exhaustive]`,
    /// so a seventh variant added over there stops this compiling until somebody
    /// decides which of the four things it leaves a person to do. It is the
    /// mechanism `job.rs` already uses for `EndReason` and this cycle used for
    /// `Role`, and the reason [`UnreadableCause::of`] cannot have it is written
    /// on that function.
    fn of(e: &mnema_secrets::Error) -> Self {
        use mnema_secrets::Error as E;
        match e {
            E::Unavailable { .. } => Self::Locked,
            E::Ambiguous { .. } => Self::Duplicate,
            E::Unreadable { .. } | E::Refused { .. } => Self::Refused,
            E::NotPersistent { .. } | E::EmptyReference => Self::Defect,
        }
    }
}

/// The index half of [`ModelSettings`]: what the database says, or why it said
/// nothing.
///
/// Tagged with `kind`, the convention `Balance`, `Refusal` and `RecordId`
/// already use and the shape the window's code is written against.
#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum IndexSettings {
    /// A newtype variant holding a struct, rather than a struct variant, so the
    /// fields have a name a caller can pass around — the tests destructure this
    /// once instead of matching seven fields at four call sites. It serialises
    /// identically: an internally tagged newtype variant whose payload is a map
    /// is flattened into that map beside `kind`, which is the constraint
    /// `Balance`'s own doc records from the other side.
    Read(IndexRead),
    /// The index could not be read. `reason` is the same `Display` string a
    /// rejected command would have carried, so nothing is lost by not rejecting;
    /// what is gained is that [`ModelSettings::key`] beside it survives.
    ///
    /// `cause` is here because this doc used to enumerate three causes and offer
    /// no way to tell them apart — and the first test written against it
    /// separated them with `reason.contains("index is not open")`, which is
    /// matching on message text, the failure mode `crate::error::Error`'s own
    /// header says it exists to avoid. A rephrased sentence would have broken a
    /// window silently. `reason` stays verbatim, for showing; `cause` is what
    /// anything branches on.
    ///
    /// Named `cause` rather than `kind` because `kind` is this enum's own tag,
    /// and serde refuses a variant field that collides with it — at compile
    /// time, which is how this name was chosen.
    Unreadable {
        cause: UnreadableCause,
        reason: String,
    },
}

/// Why the index said nothing — the discriminant beside
/// [`IndexSettings::Unreadable`]'s sentence.
///
/// **Two, where the prose named three.** "Never opened" and "opened and failed"
/// are one value here because this layer genuinely cannot tell them apart:
/// `AppState::db` is `None` in both cases, since a failed `open_index` returns
/// before it assigns. The window can separate them, and must — it knows whether
/// it has called `open_index` and what that answered. What it could not do
/// before this field is separate either of them from a read that failed on its
/// own, which is the distinction that decides between "ask the user to open a
/// folder" and "report a bug".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UnreadableCause {
    /// No index is open. Either the window has not asked for one yet — the
    /// ordinary state at start-up — or an open failed and left none; see this
    /// enum's own doc for why those are one value.
    NotOpen,
    /// An index **is** open and reading it failed. Always a defect of this
    /// build rather than a state of the machine, so a window meeting it is
    /// looking at a bug report.
    ReadFailed,
}

impl UnreadableCause {
    /// Classifies what `AppState::with_index` handed back.
    ///
    /// **The three exits are named rather than left to a wildcard**, because
    /// "everything else" is a definition by enumeration and reads as though the
    /// classification had been thought about once. `AppState::with_index` has
    /// exactly three ways out — `StatePoisoned` from the lock, `IndexNotOpen`
    /// from the missing connection, `Index(_)` from the closure — and two of the
    /// three are defects of this build, which is what `ReadFailed` says.
    ///
    /// ⚠️ **The trailing arm is not a compiler guard, and cannot be one from
    /// here.** [`KeyStoreFailure::of`] gets one because it classifies a whole
    /// foreign enum; this classifies a *subset* of [`crate::error::Error`] —
    /// the subset one nine-line function can produce — and no match expresses
    /// "the things `with_index` returns". Matching all of `Error` exhaustively
    /// would add nine arms for variants that cannot arrive here, and a reader
    /// would rightly ask why `NoKey` is being classified. So the obligation is
    /// written where it can actually be broken, on `AppState::with_index`
    /// itself: a fourth way out of that function owes this list a decision.
    /// `a_read_that_failed_is_told_apart_from_an_index_that_is_not_open` pins
    /// the three as they stand.
    fn of(e: &Error) -> Self {
        match e {
            Error::IndexNotOpen => Self::NotOpen,
            Error::StatePoisoned | Error::Index(_) => Self::ReadFailed,
            // Not reachable from `with_index` today. `ReadFailed` and not a
            // panic: a command that classified an unexpected error by aborting
            // would take the window down for a case this reasoning simply did
            // not cover, and "a read was attempted and something went wrong" is
            // the honest reading of any error arriving from a read.
            _ => Self::ReadFailed,
        }
    }
}

/// What an open index says about its model configuration.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRead {
    pub embedding_model: Option<String>,
    pub embedding_dim: Option<i64>,
    pub active_space: Option<i64>,
    /// How many chunks already have a vector in the active space, out of how
    /// many exist. The window shows both: an active space says which model the
    /// index works with, not that anything is embedded yet.
    ///
    /// ⚠️ **Not a fraction, and never to be divided.** Four things follow, and
    /// each is a way the obvious rendering is wrong:
    ///
    /// - `embedded_chunks` counts one space, `total_chunks` counts the whole
    ///   index, so "X of Y" is already an inexact sentence.
    /// - `embedded_chunks` can exceed `total_chunks`. A vector outlives the
    ///   chunk it embeds; `Db::chunk_count` sets out why, and
    ///   `a_vector_outlives_the_chunk_it_embeds` in the index crate's
    ///   `tests/adopt.rs` holds the storage half of it in the gate.
    /// - Both are zero in this build, because nothing embeds yet (D29).
    /// - Zero with `active_space == null` is not "nothing is embedded", it is
    ///   "the question does not arise". Tell them apart by `active_space`, never
    ///   by the zero.
    pub embedded_chunks: i64,
    pub total_chunks: i64,
    pub rerank_model: Option<String>,
    pub chat_model: Option<String>,
}

/// The index half, read or refused — and never an `Err`, which is the whole
/// point: a caller building [`ModelSettings`] must not be able to lose the key
/// half by writing `?` here.
fn index_settings(state: &AppState) -> IndexSettings {
    let read = state.with_index(|db| {
        let active_space = db.active_space()?;
        let (embedding_model, embedding_dim) = match active_space {
            Some(id) => {
                let (model, dim) = db.space_model(id)?;
                (Some(model), Some(dim))
            }
            None => (None, None),
        };
        Ok(IndexSettings::Read(IndexRead {
            embedding_model,
            embedding_dim,
            active_space,
            embedded_chunks: match active_space {
                Some(id) => db.embedded_chunk_count(id)?,
                None => 0,
            },
            total_chunks: db.chunk_count()?,
            rerank_model: db.meta_get(mnema_index::META_RERANK_MODEL)?,
            chat_model: db.meta_get(mnema_index::META_CHAT_MODEL)?,
        }))
    });
    match read {
        Ok(settings) => settings,
        Err(e) => IndexSettings::Unreadable {
            cause: UnreadableCause::of(&e),
            reason: e.to_string(),
        },
    }
}

/// The key half, asked and answered — and never an `Err`, for the same reason
/// [`index_settings`] is not one. Between them they leave [`model_settings`]
/// with no `?` at all, which is what makes "one half cannot swallow the other"
/// a property of the command rather than of half of it.
fn key_state(state: &AppState) -> KeyState {
    // `Ok(Some(_))`, discarding what was loaded: this type carries whether a key
    // is there and never the key.
    match mnema_secrets::load(state.credential_ref()) {
        Ok(Some(_)) => KeyState::Present,
        Ok(None) => KeyState::Absent,
        Err(e) => KeyState::Unreadable {
            cause: KeyStoreFailure::of(&e),
            reason: e.to_string(),
        },
    }
}

/// What the window draws on the settings screen.
///
/// The index half is read inside one closure, so its numbers come from a single
/// connection at a single moment rather than being assembled from several.
///
/// An active space naming a space that is gone arrives as
/// `mnema_index::Error::NoSuchSpace` rather than as "nothing chosen" — see
/// `Db::space_model`, where the argument for that is written down. It reaches
/// the window as [`IndexSettings::Unreadable`] with `kind: ReadFailed`, and no
/// longer takes the key half down with it. Nothing in this build can produce the
/// state: `Db::drop_space` has no caller outside the index crate's own tests.
///
/// **It returns no `Result`, and that is the guarantee rather than a
/// convenience.** Every state of the credential store and every state of the
/// index is a state of the configuration, which is the thing this screen draws;
/// there is nothing left for a rejection to mean. A `Result` here would be a
/// type that is always `Ok` and a place for the next `?` to be written, and this
/// command has now twice been the one where a `?` ate the other half of the
/// answer. `bridge::job_status` is the same shape for the same reason.
#[tauri::command(async)]
pub fn model_settings(state: State<'_, AppState>) -> ModelSettings {
    ModelSettings {
        key: key_state(&state),
        index: index_settings(&state),
        platform: Platform::of_this_build(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every role the provider has is reachable from the window — checked by the
    /// compiler and not by counting to three.
    ///
    /// `role_from` is total over *strings*, and that is a different statement
    /// from total over [`Role`]. A fourth variant added to `Role` tomorrow
    /// compiles fine and is simply unreachable from the window, in silence. The
    /// `match` below has no wildcard arm, so that day it stops compiling
    /// instead — the same "a count is a definition" this cycle already paid for
    /// on the eight commands.
    #[test]
    fn every_role_the_provider_has_is_named_by_a_string_the_window_can_send() {
        let mut seen = Vec::new();
        for name in ["embedding", "rerank", "chat"] {
            let role = role_from(name).expect("a role this build claims to know");
            // Exhaustive on purpose, and the arms are separate so that two
            // strings mapping to one variant would leave a third unvisited.
            match role {
                Role::Embedding => seen.push("embedding"),
                Role::Rerank => seen.push("rerank"),
                Role::Chat => seen.push("chat"),
            }
        }
        assert_eq!(
            seen,
            ["embedding", "rerank", "chat"],
            "the strings and the variants are no longer in step"
        );
    }

    #[test]
    fn a_role_this_build_does_not_know_is_refused_rather_than_defaulted() {
        assert!(matches!(role_from("embbeding"), Err(Error::UnknownRole(_))));
    }

    /// An `IndexRead` with nothing in it, for the tests below that care only
    /// about the tag in front of it.
    fn empty_read() -> IndexRead {
        IndexRead {
            embedding_model: None,
            embedding_dim: None,
            active_space: None,
            embedded_chunks: 0,
            total_chunks: 0,
            rerank_model: None,
            chat_model: None,
        }
    }

    /// The canonical camelCase spelling of every discriminant this module sends
    /// across the IPC, pinned the way `job.rs` pins `EndReason`'s.
    ///
    /// `KeyState`, `KeyStoreFailure`, `IndexSettings`, `UnreadableCause`,
    /// `KeyRemoval` and `Platform` reach the window from here — named rather than counted,
    /// because the list has grown since this test was written and a total is a
    /// definition that goes stale. Not one of them was fastened to anything
    /// when it was: `grep -rn 'readFailed' src-tauri ui` was empty, and a fourth
    /// `KeyState` variant would have arrived in the window as an unhandled
    /// `kind` without reddening a thing. `job.rs` has had this exact mechanism
    /// twice since the walk job.
    ///
    /// **Two independent opinions about each spelling, which is what makes it a
    /// test rather than a restatement.** The `match` arms are written by hand
    /// here; `serde_json::to_value` asks serde. A `#[serde(rename)]` typo is
    /// caught unless the same typo was made twice, in two places, by two people.
    /// And no arm has a wildcard, so a variant added to any of these four stops
    /// this file compiling — which is the part that makes a maintainer look at
    /// the list at all.
    ///
    /// What it cannot do is reach `ui/render.test.js`, where the mirrored lists
    /// for `EndReason` live. There is nothing to mirror into yet — the window
    /// has no renderer for these — and a JS list that nothing asserts against
    /// goes stale while looking authoritative. That half belongs with the
    /// renderer; `ui/render.test.js` already runs in CI, so it has somewhere to
    /// land.
    #[test]
    fn every_discriminant_the_window_sees_has_its_camel_case_spelling_pinned() {
        // Two shapes, and they are read differently on purpose. `KeyState` and
        // `IndexSettings` are internally tagged, so their discriminant is the
        // value of `kind` inside an object; `KeyStoreFailure` and
        // `UnreadableCause` are plain unit enums and serialise as bare strings.
        let tag = |v: serde_json::Value| -> String {
            v["kind"]
                .as_str()
                .expect("an internally tagged enum carries `kind`")
                .to_string()
        };
        let bare = |v: serde_json::Value| -> String {
            v.as_str()
                .expect("a unit-only enum serialises as a string")
                .to_string()
        };

        let key = |state: &KeyState| -> &'static str {
            match state {
                KeyState::Present => "present",
                KeyState::Absent => "absent",
                KeyState::Unreadable { .. } => "unreadable",
            }
        };
        for state in [
            KeyState::Present,
            KeyState::Absent,
            KeyState::Unreadable {
                cause: KeyStoreFailure::Locked,
                reason: String::new(),
            },
        ] {
            assert_eq!(
                tag(serde_json::to_value(&state).unwrap()),
                key(&state),
                "{state:?} serialised differently than this test's own spelling of it"
            );
        }

        let failure = |f: KeyStoreFailure| -> &'static str {
            match f {
                KeyStoreFailure::Locked => "locked",
                KeyStoreFailure::Duplicate => "duplicate",
                KeyStoreFailure::Refused => "refused",
                KeyStoreFailure::Defect => "defect",
            }
        };
        for f in [
            KeyStoreFailure::Locked,
            KeyStoreFailure::Duplicate,
            KeyStoreFailure::Refused,
            KeyStoreFailure::Defect,
        ] {
            assert_eq!(
                bare(serde_json::to_value(f).unwrap()),
                failure(f),
                "{f:?} serialised differently than this test's own spelling of it"
            );
        }

        // All three are listed although this build can only ever send one of
        // them: the window's table is indexed by every spelling, and a rename
        // that only breaks on the platform nobody is compiling today is the
        // shape this cycle already met once, in a resource named for one
        // operating system and required by three.
        let platform = |p: Platform| -> &'static str {
            match p {
                Platform::Mac => "mac",
                Platform::Windows => "windows",
                Platform::Linux => "linux",
            }
        };
        for p in [Platform::Mac, Platform::Windows, Platform::Linux] {
            assert_eq!(
                bare(serde_json::to_value(p).unwrap()),
                platform(p),
                "{p:?} serialised differently than this test's own spelling of it"
            );
        }

        // And that the field arrives at all, which is a different claim from how
        // its values are spelled. The window indexes a table by it; an absent
        // field is `undefined` there, falls through that table's fallback, and
        // draws nothing — silently, on the one platform the sentence exists for.
        let settings = ModelSettings {
            key: KeyState::Absent,
            index: IndexSettings::Unreadable {
                cause: UnreadableCause::NotOpen,
                reason: String::new(),
            },
            platform: Platform::of_this_build(),
        };
        let sent = serde_json::to_value(&settings).unwrap();
        assert!(
            ["mac", "windows", "linux"].contains(&sent["platform"].as_str().unwrap_or_default()),
            "`platform` did not reach the window as one of the three spellings: {sent}"
        );

        let index = |settings: &IndexSettings| -> &'static str {
            match settings {
                IndexSettings::Read(_) => "read",
                IndexSettings::Unreadable { .. } => "unreadable",
            }
        };
        for settings in [
            IndexSettings::Read(empty_read()),
            IndexSettings::Unreadable {
                cause: UnreadableCause::NotOpen,
                reason: String::new(),
            },
        ] {
            assert_eq!(
                tag(serde_json::to_value(&settings).unwrap()),
                index(&settings),
                "{settings:?} serialised differently than this test's own spelling of it"
            );
        }

        let cause = |c: UnreadableCause| -> &'static str {
            match c {
                UnreadableCause::NotOpen => "notOpen",
                UnreadableCause::ReadFailed => "readFailed",
            }
        };
        for c in [UnreadableCause::NotOpen, UnreadableCause::ReadFailed] {
            assert_eq!(
                bare(serde_json::to_value(c).unwrap()),
                cause(c),
                "{c:?} serialised differently than this test's own spelling of it"
            );
        }

        let removal = |r: KeyRemoval| -> &'static str {
            match r {
                KeyRemoval::Removed => "removed",
                KeyRemoval::NothingToRemove => "nothingToRemove",
            }
        };
        for r in [KeyRemoval::Removed, KeyRemoval::NothingToRemove] {
            assert_eq!(
                tag(serde_json::to_value(r).unwrap()),
                removal(r),
                "{r:?} serialised differently than this test's own spelling of it"
            );
        }
    }

    /// Every answer the credential store can give a deletion, kept apart.
    ///
    /// The `match` in [`KeyRemoval::of`] is already exhaustive, so a third
    /// `mnema_secrets::Forgotten` stops the crate compiling. What this adds is
    /// the half an exhaustive match is perfectly happy without: that the two
    /// are actually told apart rather than both mapped to `Removed`, which is
    /// the build the window used to be.
    #[test]
    fn a_deletion_that_removed_a_key_is_told_apart_from_one_that_found_none() {
        assert_eq!(
            KeyRemoval::of(mnema_secrets::Forgotten::Removed),
            KeyRemoval::Removed
        );
        assert_eq!(
            KeyRemoval::of(mnema_secrets::Forgotten::NothingToRemove),
            KeyRemoval::NothingToRemove
        );
    }

    /// The same pin for the unions that reach the window from `mnema-provider`
    /// rather than from here — `Refusal`, `Balance`, `RecordId`, `Price` and
    /// `InputLimit`.
    ///
    /// They are pinned in this crate and not in theirs on purpose. "Reaches the
    /// window" is a fact about `mnema-desktop`: `provider_models` returns a
    /// `Catalogue` carrying `Refusal`, `RecordId`, `Price` and `InputLimit`, and
    /// [`KeyStatus::balance`]
    /// carries `Balance`. `mnema-provider` does not know it has a window, and a
    /// test living there would be pinning a serialisation for no stated reader.
    ///
    /// **The compiler half works across the crate boundary because none of them
    /// is `#[non_exhaustive]`** — checked, the way
    /// [`Self::the_credential_store_failures_are_sorted_by_what_they_ask_of_a_person`]'s
    /// own guard was checked against `mnema_secrets::Error`. A sixth `Refusal`
    /// variant added one crate over stops **this file** compiling, which is the
    /// defect this test exists for: `ui/render.js` looks its `kind` up in a
    /// table and falls back to "this build did not recognise the reason", so
    /// without this the new variant would reach a person as that sentence and
    /// redden nothing anywhere. Precisely: the arms are inside
    /// `#[cfg(test)] mod tests`, so `cargo build` still succeeds and it is
    /// `cargo test` that stops — which is what the gate runs, and is the whole
    /// distance this guarantee travels.
    ///
    /// **It compares the whole serialised value, not the tag** (review round 1,
    /// F6). Reading `kind` alone left every payload field unpinned in the entire
    /// workspace — `Refusal`'s `limit`, `floor` and `raw`, `RecordId`'s `raw` and
    /// `id` — while `ui/render.js` interpolates all of them: `REFUSAL_TEXT`'s
    /// `inputTooSmall` reads `r.limit` and `r.floor`, `RECORD_ID_TEXT`'s `known`
    /// reads `record.id.id`. A `#[serde(rename)]` on one field would have drawn
    /// "input limit **undefined** tokens" under a correct `kind`, with this test
    /// green. The `match` arms also **destructure** the fields rather than
    /// eliding them with `..`, so renaming one in Rust stops this file compiling
    /// for the same reason a new variant does.
    ///
    /// **What it does not reach**, said rather than implied: the window keeps
    /// its own lists (`ui/render.test.js`, `REFUSALS` / `BALANCES` /
    /// `RECORD_IDS`) and asserts its tables against them. Those are a hand-made
    /// copy of the list below, and nothing ties the two languages together —
    /// tying them would need the cross-language artefact D39 withdrew. So a
    /// variant added here still has to be carried across by a person; what this
    /// buys is that the person is *told*, by a build that stops, instead of
    /// finding out from a fallback sentence in front of a user.
    ///
    /// `Balance::Unreadable` takes a [`mnema_provider::ProviderMessage`], whose
    /// `Text` variant is unconstructible outside `probe.rs` — `SanitisedText`
    /// has no public constructor. `Withheld` is a public unit variant and needs
    /// none, so the payload costs nothing here; `ProviderMessage` itself is
    /// therefore not pinned by this test, and the window does not read its
    /// `kind` (`ui/render.js` deliberately does not interpolate `raw`).
    #[test]
    fn every_provider_discriminant_the_window_sees_has_its_camel_case_spelling_pinned() {
        use mnema_provider::{Balance, InputLimit, Price, ProviderMessage, RecordId, Refusal};
        use serde_json::json;

        // Written by hand, and by serde, and compared — the second opinion is
        // what makes this a test rather than a restatement of the attribute.
        // No wildcard arm anywhere below, and no `..` in any pattern.
        //
        // The payload values are deliberately distinct from one another
        // (`limit` is not `floor`, `raw` is not `id`), so a pair of fields
        // swapped by a rename cannot satisfy this by accident.
        let refusal = |r: &Refusal| -> serde_json::Value {
            match r {
                Refusal::InputTooSmall { limit, floor } => {
                    json!({"kind": "inputTooSmall", "limit": limit, "floor": floor})
                }
                Refusal::NoStatedLimit => json!({"kind": "noStatedLimit"}),
                Refusal::LimitNotUnderstood { raw } => {
                    json!({"kind": "limitNotUnderstood", "raw": raw})
                }
                Refusal::NoStatedOutputModalities => json!({"kind": "noStatedOutputModalities"}),
                Refusal::NoTextOutput => json!({"kind": "noTextOutput"}),
            }
        };
        for r in [
            Refusal::InputTooSmall {
                limit: 512,
                floor: 2048,
            },
            Refusal::NoStatedLimit,
            Refusal::LimitNotUnderstood {
                raw: "8192.0".to_string(),
            },
            Refusal::NoStatedOutputModalities,
            Refusal::NoTextOutput,
        ] {
            assert_eq!(
                serde_json::to_value(&r).unwrap(),
                refusal(&r),
                "{r:?} serialised differently than this test's own spelling of it"
            );
        }

        let balance = |b: &Balance| -> serde_json::Value {
            match b {
                Balance::Known { amount } => json!({"kind": "known", "amount": amount}),
                Balance::NotStated => json!({"kind": "notStated"}),
                // `raw` is a `ProviderMessage`, itself tagged. Pinned here as
                // the nested object the window receives rather than looked
                // past: `ui/render.js` deliberately does not interpolate it,
                // and this is what would show if it stopped being an object.
                Balance::Unreadable { raw } => json!({"kind": "unreadable", "raw": raw}),
                Balance::EnvelopeNotUnderstood => json!({"kind": "envelopeNotUnderstood"}),
            }
        };
        for b in [
            Balance::Known { amount: 12.5 },
            Balance::NotStated,
            Balance::Unreadable {
                raw: ProviderMessage::Withheld,
            },
            Balance::EnvelopeNotUnderstood,
        ] {
            assert_eq!(
                serde_json::to_value(&b).unwrap(),
                balance(&b),
                "{b:?} serialised differently than this test's own spelling of it"
            );
        }

        let record = |r: &RecordId| -> serde_json::Value {
            match r {
                RecordId::Absent => json!({"kind": "absent"}),
                RecordId::NotAString { raw } => json!({"kind": "notAString", "raw": raw}),
                RecordId::Known { id } => json!({"kind": "known", "id": id}),
            }
        };
        for r in [
            RecordId::Absent,
            RecordId::NotAString {
                raw: "12345".to_string(),
            },
            RecordId::Known {
                id: "vendor/m".to_string(),
            },
        ] {
            assert_eq!(
                serde_json::to_value(&r).unwrap(),
                record(&r),
                "{r:?} serialised differently than this test's own spelling of it"
            );
        }

        // `amount` is the field the window multiplies by a million, and `raw`
        // is the provider text beside it — a rename of either draws a price of
        // `undefined` under a correct `kind`, which is exactly what F6 found on
        // `Refusal`. The values are distinct from every other payload below so
        // that a pair of fields swapped by a rename cannot satisfy this by
        // accident.
        let price = |p: &Price| -> serde_json::Value {
            match p {
                Price::NotStated => json!({"kind": "notStated"}),
                Price::Known { amount } => json!({"kind": "known", "amount": amount}),
                Price::NotAPrice { raw } => json!({"kind": "notAPrice", "raw": raw}),
                Price::Unreadable { raw } => json!({"kind": "unreadable", "raw": raw}),
            }
        };
        for p in [
            Price::NotStated,
            Price::Known { amount: 1.5e-8 },
            Price::NotAPrice {
                raw: "-1".to_string(),
            },
            Price::Unreadable {
                raw: "free".to_string(),
            },
        ] {
            assert_eq!(
                serde_json::to_value(&p).unwrap(),
                price(&p),
                "{p:?} serialised differently than this test's own spelling of it"
            );
        }

        let limit = |l: &InputLimit| -> serde_json::Value {
            match l {
                InputLimit::NotStated => json!({"kind": "notStated"}),
                InputLimit::Known { tokens } => json!({"kind": "known", "tokens": tokens}),
                InputLimit::NotUnderstood { raw } => json!({"kind": "notUnderstood", "raw": raw}),
            }
        };
        for l in [
            InputLimit::NotStated,
            InputLimit::Known { tokens: 8194 },
            InputLimit::NotUnderstood {
                raw: "8k".to_string(),
            },
        ] {
            assert_eq!(
                serde_json::to_value(&l).unwrap(),
                limit(&l),
                "{l:?} serialised differently than this test's own spelling of it"
            );
        }
    }

    /// The three ways out of `AppState::with_index`, sorted as
    /// [`UnreadableCause::of`] sorts them.
    ///
    /// It pins the classification as it stands; it does **not** notice a fourth
    /// exit being added to `with_index`, which is why that obligation is written
    /// on `with_index` itself. Both directions, because a classifier that
    /// answered `ReadFailed` to everything would satisfy two of these three.
    #[test]
    fn a_read_that_failed_is_told_apart_from_an_index_that_is_not_open() {
        assert_eq!(
            UnreadableCause::of(&Error::IndexNotOpen),
            UnreadableCause::NotOpen
        );
        assert_eq!(
            UnreadableCause::of(&Error::StatePoisoned),
            UnreadableCause::ReadFailed
        );
        assert_eq!(
            UnreadableCause::of(&Error::Index(mnema_index::Error::NoSuchSpace(7))),
            UnreadableCause::ReadFailed,
            "a space the pointer names and the database does not have is a defect of this \
             build, not an index nobody opened"
        );
    }

    /// Every failure the credential store can report, sorted by what it leaves
    /// the person to do.
    ///
    /// The `match` in [`KeyStoreFailure::of`] is already exhaustive, so a
    /// seventh variant of `mnema_secrets::Error` stops the crate compiling.
    /// What this adds is the other half: that the four groups are actually
    /// distinguished rather than all mapped to one value, which an exhaustive
    /// match is perfectly happy to do.
    #[test]
    fn the_credential_store_failures_are_sorted_by_what_they_ask_of_a_person() {
        let reference = "mnema-test-reference".to_string();
        for (error, expected) in [
            (
                mnema_secrets::Error::Unavailable {
                    reference: reference.clone(),
                    detail: "the keychain is locked".into(),
                },
                KeyStoreFailure::Locked,
            ),
            (
                mnema_secrets::Error::Ambiguous {
                    reference: reference.clone(),
                    count: 2,
                },
                KeyStoreFailure::Duplicate,
            ),
            (
                mnema_secrets::Error::Refused {
                    reference: reference.clone(),
                    reason: "no".into(),
                },
                KeyStoreFailure::Refused,
            ),
            (
                mnema_secrets::Error::Unreadable {
                    reference: reference.clone(),
                    reason: "not utf-8",
                },
                KeyStoreFailure::Refused,
            ),
            (
                mnema_secrets::Error::NotPersistent {
                    reference: reference.clone(),
                    vendor: "a store that keeps nothing".into(),
                },
                KeyStoreFailure::Defect,
            ),
            (
                mnema_secrets::Error::EmptyReference,
                KeyStoreFailure::Defect,
            ),
        ] {
            assert_eq!(
                KeyStoreFailure::of(&error),
                expected,
                "`{error}` was sorted into the wrong thing for a person to do"
            );
        }
    }
}

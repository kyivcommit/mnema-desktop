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
#[tauri::command(async)]
pub fn set_key(state: State<'_, AppState>, key: String) -> Result<KeyStatus, Error> {
    let check = mnema_provider::check_key(state.provider_base(), &key)?;
    mnema_secrets::store(state.credential_ref(), &key)?;
    Ok(KeyStatus {
        balance: check.balance,
    })
}

/// Removes the key. What was embedded stays embedded; what stops is embedding
/// anything new and answering a question, because the question must be embedded
/// too (D29).
#[tauri::command(async)]
pub fn forget_key(state: State<'_, AppState>) -> Result<(), Error> {
    Ok(mnema_secrets::forget(state.credential_ref())?)
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
/// **What a failure after the adopt means.** The order is check → record → read
/// back, and the read back can fail on its own — the credential store is asked
/// again for `key_present`. A rejected command then reaches the window carrying
/// the store's message while the model *has* been recorded, so the sentence the
/// window draws must not be "the model was not chosen". Reordering to read the
/// settings first would trade this for something worse: a report of the state
/// before the change.
#[tauri::command(async)]
pub fn set_embedding_model(
    state: State<'_, AppState>,
    model: String,
) -> Result<ModelSettings, Error> {
    let key = key(&state)?;
    let check = mnema_provider::check_embedding_model(state.provider_base(), &key, &model)?;
    let hash = mnema_chunk::chunker_hash();
    state.with_index(|db| {
        db.adopt_embedding_model(&model, check.dim as i64, state.credential_ref(), &hash)
    })?;
    model_settings(state)
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

/// Everything the settings screen draws, read at one instant.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettings {
    /// Measured, never a literal — `mnema_secrets::load(...)?.is_some()`. It is
    /// the same question [`key_present`] answers and it has one source; see
    /// [`KeyStatus`] for what a second one cost when it was tried.
    pub key_present: bool,
    pub embedding_model: Option<String>,
    pub embedding_dim: Option<i64>,
    pub active_space: Option<i64>,
    /// How many chunks already have a vector in the active space, out of how
    /// many exist. The window shows both: an active space says which model the
    /// index works with, not that anything is embedded yet.
    ///
    /// ⚠️ **Not a fraction.** The two are counted over different populations
    /// and the first can exceed the second — a vector outlives the chunk it
    /// embeds, which `Db::chunk_count` sets out in full. Both are zero in this
    /// build, because nothing embeds yet (D29), so nothing here goes red the
    /// day it stops being true.
    pub embedded_chunks: i64,
    pub total_chunks: i64,
    pub rerank_model: Option<String>,
    pub chat_model: Option<String>,
}

/// What the window draws on the settings screen.
///
/// The key is asked of the store and the rest of one open index, and the index
/// half is one closure so the numbers below are read from a single connection at
/// a single moment rather than assembled from several.
///
/// An active space naming a space that is gone arrives as
/// `mnema_index::Error::NoSuchSpace` rather than as "nothing chosen" — see
/// `Db::space_model`, where the argument for that is written down. Nothing in
/// this build can produce that state: `Db::drop_space` has no caller outside the
/// index crate's own tests.
#[tauri::command(async)]
pub fn model_settings(state: State<'_, AppState>) -> Result<ModelSettings, Error> {
    let key_present = mnema_secrets::load(state.credential_ref())?.is_some();
    state.with_index(|db| {
        let active_space = db.active_space()?;
        let (embedding_model, embedding_dim) = match active_space {
            Some(id) => {
                let (model, dim) = db.space_model(id)?;
                (Some(model), Some(dim))
            }
            None => (None, None),
        };
        Ok(ModelSettings {
            key_present,
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
        })
    })
}

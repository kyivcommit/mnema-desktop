//! Model configuration: the key, and what the window needs to draw the state of
//! it.
//!
//! v1 speaks to OpenRouter alone (spec §2.2), so there is one credential and its
//! reference is a constant rather than a row. The database holds no key and no
//! reference of its own — "is there a key?" is a question for the credential
//! store, and every answer here comes from asking it rather than from a flag
//! this module could get wrong.

use serde::Serialize;
use tauri::State;

use crate::error::Error;
use crate::state::AppState;

/// The name this product files its one credential under in production. A
/// constant because there is one account in v1; a second provider turns it into
/// a column.
///
/// The value the commands actually use comes from [`AppState::credential_ref`],
/// not from here. That indirection is what lets an integration test write under
/// a reference of its own: `mnema-secrets` keeps the platform store out of reach
/// only under its own `cfg(test)`, which a test of *this* crate does not set, so
/// a test that used this constant would overwrite the developer's real key.
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
        present: true,
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
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    pub present: bool,
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

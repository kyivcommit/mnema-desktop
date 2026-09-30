//! What the launcher's cloud indicator shows: is the provider set up, and does
//! it answer.
//!
//! The first half is local and cheap, so it is recomputed on EVERY call and
//! never cached; only the network half is. That order is what makes a
//! `forget_key` racing an in-flight check harmless — the next call answers
//! `notConfigured` from local facts whatever the cache holds.

use std::time::{Duration, Instant};

use tauri::State;

use crate::error::Error;
use crate::state::AppState;

/// How long an answer from the provider is believed.
const TTL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Missing {
    Key,
    EmbeddingModel,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProviderStatus {
    NotConfigured { missing: Missing },
    Unreachable { reason: String },
    Ok,
}

/// Whether an answer taken at `cached_at` still stands at `now`. Exactly one
/// TTL old is stale.
pub fn fresh(cached_at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(cached_at) < TTL
}

/// What is missing, from local facts only. The same two facts the launcher's
/// `providerReady` reads: a stored key, and an active embedding space. An index
/// that cannot be read counts as no model.
fn missing(state: &AppState) -> Option<Missing> {
    match mnema_secrets::load(state.credential_ref()) {
        Ok(Some(_)) => {}
        // A store that will not answer is not a key we can use either.
        _ => return Some(Missing::Key),
    }
    match state.with_index(|db| db.active_space()) {
        Ok(Some(_)) => None,
        _ => Some(Missing::EmbeddingModel),
    }
}

#[tauri::command(async)]
pub fn provider_status(state: State<'_, AppState>) -> ProviderStatus {
    if let Some(missing) = missing(&state) {
        return ProviderStatus::NotConfigured { missing };
    }
    if let Some(cached) = state.cached_provider_status(fresh) {
        return cached;
    }
    // Checked against the key as it is now; the cache lock is not held across
    // the request, so a slow `/credits` never blocks `set_key` and friends.
    let status = match mnema_secrets::load(state.credential_ref()) {
        Ok(Some(key)) => match mnema_provider::check_key(state.provider_base(), &key) {
            Ok(_) => ProviderStatus::Ok,
            Err(e) => ProviderStatus::Unreachable {
                reason: Error::from(e).to_string(),
            },
        },
        _ => {
            return ProviderStatus::NotConfigured {
                missing: Missing::Key,
            };
        }
    };
    state.store_provider_status(status.clone());
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_fresh_at_59_seconds_and_stale_at_60() {
        let t0 = Instant::now();
        assert!(fresh(t0, t0 + Duration::from_secs(59)));
        assert!(!fresh(t0, t0 + Duration::from_secs(60)));
    }

    #[test]
    fn a_clock_that_reads_earlier_than_the_answer_is_fresh_not_a_panic() {
        let t0 = Instant::now() + Duration::from_secs(5);
        assert!(fresh(t0, t0 - Duration::from_secs(5)));
    }
}

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

/// The local facts: the key, or the status that stands in for it. The same two
/// facts the launcher's `providerReady` reads: a stored key, and an active
/// embedding space. An index that cannot be read counts as no model.
///
/// A credential store that will not answer is NOT "no key entered": the
/// person may well have one, and sending them to re-enter it is wrong
/// (`KeyState::Unreadable`). It is `Unreachable`, and never cached.
fn local_facts(state: &AppState) -> Result<String, ProviderStatus> {
    let key = match mnema_secrets::load(state.credential_ref()) {
        Ok(Some(key)) => key,
        Ok(None) => return Err(not_configured(Missing::Key)),
        Err(e) => {
            return Err(ProviderStatus::Unreachable {
                reason: Error::from(e).to_string(),
            });
        }
    };
    match state.with_index(|db| db.active_space()) {
        Ok(Some(_)) => Ok(key),
        _ => Err(not_configured(Missing::EmbeddingModel)),
    }
}

fn not_configured(missing: Missing) -> ProviderStatus {
    ProviderStatus::NotConfigured { missing }
}

#[tauri::command(async)]
pub fn provider_status(state: State<'_, AppState>) -> ProviderStatus {
    // Before the first read of anything the answer depends on.
    let epoch = state.provider_status_gen();
    let key = match local_facts(&state) {
        Ok(key) => key,
        Err(status) => return status,
    };
    if let Some(cached) = state.cached_provider_status() {
        return cached;
    }
    // The cache lock is not held across the request, so a slow `/credits`
    // never blocks `set_key` and friends.
    let status = match mnema_provider::check_key(state.provider_base(), &key) {
        Ok(_) => ProviderStatus::Ok,
        Err(e) => ProviderStatus::Unreachable {
            reason: Error::from(e).to_string(),
        },
    };
    state.store_provider_status(epoch, status.clone());
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

#[cfg(test)]
mod wire {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_spelling_the_window_reads_is_pinned() {
        let v = |s: &ProviderStatus| serde_json::to_value(s).unwrap();
        assert_eq!(v(&ProviderStatus::Ok), json!({ "kind": "ok" }));
        assert_eq!(
            v(&ProviderStatus::Unreachable { reason: "r".into() }),
            json!({ "kind": "unreachable", "reason": "r" })
        );
        assert_eq!(
            v(&not_configured(Missing::Key)),
            json!({ "kind": "notConfigured", "missing": "key" })
        );
        assert_eq!(
            v(&not_configured(Missing::EmbeddingModel)),
            json!({ "kind": "notConfigured", "missing": "embeddingModel" })
        );
    }

    #[test]
    fn a_check_in_flight_across_an_invalidation_cannot_put_its_verdict_back() {
        let state = AppState::new("d".into(), "w".into(), "b".into(), "r".into());
        let epoch = state.provider_status_gen();
        state.forget_provider_status();
        state.store_provider_status(
            epoch,
            ProviderStatus::Unreachable {
                reason: "old".into(),
            },
        );
        assert_eq!(state.cached_provider_status(), None);
        // Positive control: a write under the current generation lands.
        state.store_provider_status(state.provider_status_gen(), ProviderStatus::Ok);
        assert_eq!(state.cached_provider_status(), Some(ProviderStatus::Ok));
    }
}

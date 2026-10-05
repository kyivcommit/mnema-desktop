//! What the launcher's cloud indicator shows: is the provider set up, and does
//! it answer.
//!
//! The first half is local and cheap, so it is recomputed on EVERY call and
//! never cached; only an `Ok` from the network half is (an `Unreachable`
//! is asked again on the next call, so the cloud follows the network back). That order is what makes a
//! `forget_key` racing an in-flight check harmless — the next call answers
//! `notConfigured` from local facts whatever the cache holds.

use std::time::{Duration, Instant};

use tauri::State;

use crate::error::Error;
use crate::state::AppState;

/// How long the status probe waits for `/credits`. Short on purpose: a lost
/// network must turn the cloud grey within seconds, not after the provider
/// crate's 30 s global timeout.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// How long an `Ok` from the provider is believed.
const TTL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Missing {
    Key,
    EmbeddingModel,
    /// Mnema is chosen and its two models are not both downloaded.
    LocalModels,
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
fn local_facts(
    state: &AppState,
    choice: crate::provider::ProviderChoice,
) -> Result<crate::provider::Endpoint, ProviderStatus> {
    let endpoint = match state.endpoint_as(choice) {
        Ok(endpoint) => endpoint,
        Err(Error::NoKey) => return Err(not_configured(Missing::Key)),
        Err(e) => {
            return Err(ProviderStatus::Unreachable {
                reason: e.to_string(),
            });
        }
    };
    match state.with_index(|db| db.active_space()) {
        Ok(Some(_)) => Ok(endpoint),
        _ => Err(not_configured(Missing::EmbeddingModel)),
    }
}

/// Under Mnema: both models downloaded, the process answering, a space to
/// embed into. Nothing leaves the machine, so nothing is cached and `/credits`
/// is never asked.
fn local_status(state: &AppState) -> ProviderStatus {
    if !state.local().models_ready() {
        return not_configured(Missing::LocalModels);
    }
    match state.endpoint_as(crate::provider::ProviderChoice::Mnema) {
        Ok(_) => ProviderStatus::Ok,
        Err(e) => ProviderStatus::Unreachable {
            reason: e.to_string(),
        },
    }
}

fn not_configured(missing: Missing) -> ProviderStatus {
    ProviderStatus::NotConfigured { missing }
}

#[tauri::command(async)]
pub fn provider_status(state: State<'_, AppState>) -> ProviderStatus {
    status(&state)
}

/// The command's body, reachable without a `State`.
pub(crate) fn status(state: &AppState) -> ProviderStatus {
    let choice = state.provider_choice();
    if choice == crate::provider::ProviderChoice::Mnema {
        return local_status(state);
    }
    // Before the first read of anything the answer depends on.
    let epoch = state.provider_status_gen();
    let endpoint = match local_facts(state, choice) {
        Ok(endpoint) => endpoint,
        Err(status) => return status,
    };
    if let Some(cached) = state.cached_provider_status() {
        return cached;
    }
    // The cache lock is not held across the request, so a slow `/credits`
    // never blocks `set_key` and friends.
    let status =
        match mnema_provider::check_key_within(&endpoint.base, &endpoint.token, PROBE_TIMEOUT) {
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
    fn an_unreachable_verdict_is_not_cached_but_ok_is() {
        let state = AppState::new("d".into(), "w".into(), "b".into(), "r".into());
        let epoch = state.provider_status_gen();
        state.store_provider_status(epoch, ProviderStatus::Unreachable { reason: "x".into() });
        assert_eq!(state.cached_provider_status(), None);
        state.store_provider_status(epoch, ProviderStatus::Ok);
        assert_eq!(state.cached_provider_status(), Some(ProviderStatus::Ok));
    }

    #[test]
    fn the_probe_gives_up_after_five_seconds_not_the_global_thirty() {
        assert_eq!(PROBE_TIMEOUT, Duration::from_secs(5));
    }

    #[test]
    fn a_check_in_flight_across_an_invalidation_cannot_put_its_verdict_back() {
        let state = AppState::new("d".into(), "w".into(), "b".into(), "r".into());
        let epoch = state.provider_status_gen();
        state.forget_provider_status();
        state.store_provider_status(
            epoch,
            ProviderStatus::Ok, // an Ok: an Unreachable is never stored, so it could not tell
        );
        assert_eq!(state.cached_provider_status(), None);
        // Positive control: a write under the current generation lands.
        state.store_provider_status(state.provider_status_gen(), ProviderStatus::Ok);
        assert_eq!(state.cached_provider_status(), Some(ProviderStatus::Ok));
    }
}

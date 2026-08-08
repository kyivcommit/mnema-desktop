//! The only module in the product that opens a socket.
//!
//! Two rules live here rather than at each call site. **Non-2xx is data, not an
//! error:** `http_status_as_error(false)` is what lets the caller tell 401 from
//! 404 and read what the provider said, instead of a transport error that has
//! lost the status. **One global timeout:** a request that never ends is the
//! failure a desktop application must not have, and 30 s is well past any
//! answer this product waits for.

use std::time::Duration;

use crate::Error;

/// A request that never ends is the failure a desktop application must not
/// have; 30 s is well past any answer this product waits for. Named so a
/// fast unit test can pin the value without waiting it out (Task 2 review
/// round 2, G5) — see `agent_with` and the tests at the bottom of this file.
const GLOBAL_TIMEOUT: Duration = Duration::from_secs(30);

/// This builder and the `GET` path — `get()`, below — were verified against
/// the live endpoint 2026-08-08. That is a measurement recorded in the plan,
/// not a claim this crate's own gate holds: no test here calls the live
/// endpoint, and none should. The `POST` path (`post_json`) has no caller
/// until Task 4, so its own verification arrives with that caller, not here
/// (Task 2 review round 3, H4) — a docstring that claims more than the gate
/// holds is how a later session inherits a false premise.
fn agent() -> ureq::Agent {
    agent_with(GLOBAL_TIMEOUT)
}

/// `agent()` with the timeout as a parameter, so a test can prove the
/// mechanism fires on a timeout of its own choosing instead of paying out
/// this crate's real 30 s (Task 2 review round 2, G5). `agent()` is the only
/// caller in product code; the product's own timeout lives in one place,
/// `GLOBAL_TIMEOUT`.
fn agent_with(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

pub(crate) fn get(base: &str, path: &str, key: Option<&str>) -> Result<(u16, String), Error> {
    let mut request = agent()
        .get(format!("{base}{path}"))
        .header("accept", "application/json");
    if let Some(key) = key {
        request = request.header("authorization", &format!("Bearer {key}"));
    }
    finish(request.call())
}

/// Not called yet — `list_models` only ever `GET`s. It exists now because the
/// probe design in Task 3–4 sends a request body (a real embedding call) over
/// the same agent configuration, and that design depends on `finish` reading a
/// non-2xx body rather than losing it to a transport error.
#[allow(dead_code)]
pub(crate) fn post_json(
    base: &str,
    path: &str,
    key: &str,
    body: &str,
) -> Result<(u16, String), Error> {
    let request = agent()
        .post(format!("{base}{path}"))
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("authorization", &format!("Bearer {key}"));
    finish(request.send(body))
}

/// `to_string()` on the transport error, never `Debug` and never the request:
/// the key is in a header of the request this error came from.
///
/// `body` here is provider bytes, not this crate's own text. Never interpolate
/// it into a `{}` format string anywhere it might reach a log line — a
/// newline inside it would cut the line in half and let provider text
/// impersonate a log entry. `{:?}` only, if it is ever logged at all.
fn finish(
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<(u16, String), Error> {
    let mut response = result.map_err(|e| Error::Transport(e.to_string()))?;
    let status = response.status().as_u16();
    // The status is already known here, and it must not be thrown away just
    // because reading the rest of the connection failed (Task 2 review round
    // 1, F1): `ureq` 3.3.0 errors out of a length-delimited body read rather
    // than returning the partial bytes it got, which is the normal wire shape
    // of a response that stopped mid-transfer. A 200 cut short and a host
    // that was never reachable are different problems, and only `Transport`
    // used to name the second one — this used to collapse both into it.
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| Error::BodyUnreadable {
            status,
            detail: e.to_string(),
        })?;
    Ok((status, body))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use mnema_mock_provider::{MockServer, Reply};

    use super::*;

    /// Reads the configuration back rather than exercising it — this alone
    /// does not prove the timeout fires, only that the value is set (Task 2
    /// review round 2, G5); see the paired test below for that. Costs no
    /// wall time, and goes red if the line is deleted (`global` becomes
    /// `None`) or the value is swapped.
    #[test]
    fn the_agent_is_configured_with_the_global_timeout_and_reads_non_2xx_as_data() {
        let config = agent().config().clone();
        assert_eq!(
            config.timeouts().global,
            Some(GLOBAL_TIMEOUT),
            "the global timeout must be set to the constant this crate promises"
        );
        assert!(
            !config.http_status_as_error(),
            "a non-2xx must arrive as a body to read, not a transport error that has lost \
             the status"
        );
    }

    /// A separate claim from the test above (Task 2 review round 3, H1): that
    /// test only proves the agent is built FROM `GLOBAL_TIMEOUT` — both sides
    /// move together, so mutating the constant itself is invisible to it, and
    /// the mechanism test below picks its own 300 ms and never reads the
    /// constant either. Nothing pinned the constant's own value once the
    /// 30-second integration test (G5) was gone: mutating `GLOBAL_TIMEOUT` to
    /// 100 ms left the whole workspace green, and on a healthy network every
    /// provider response slower than 100 ms would arrive as "the provider
    /// could not be reached". This is the lower bound the deleted test used
    /// to hold.
    #[test]
    fn the_global_timeout_itself_is_a_plausible_wait_for_a_person_at_a_window() {
        assert!(
            GLOBAL_TIMEOUT >= Duration::from_secs(20) && GLOBAL_TIMEOUT <= Duration::from_secs(60),
            "GLOBAL_TIMEOUT must be a plausible wait for a person at a window, got \
             {GLOBAL_TIMEOUT:?}"
        );
    }

    /// Pairs with the test above (Task 2 review round 2, G5): reading the
    /// configuration back is a plausible proxy with no proof the timeout
    /// actually does anything. This picks its own short timeout so the
    /// mechanism can be proven in milliseconds rather than the product's
    /// real 30 s, and pins both bounds the way the integration test used to.
    #[test]
    fn a_short_timeout_actually_fires_rather_than_only_being_configured() {
        let server = MockServer::new(vec![Reply::slow(1)]);
        let started = Instant::now();
        let result = agent_with(Duration::from_millis(300))
            .get(server.base())
            .call();
        let elapsed = started.elapsed();
        assert!(
            result.is_err(),
            "a reply slower than the timeout must fail, not succeed"
        );
        assert!(
            elapsed > Duration::from_millis(250),
            "a timeout much shorter than 300 ms would still pass an upper-bound-only check, \
             took {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "the timeout must actually be short, not merely configured to look short, took \
             {elapsed:?}"
        );
    }

    /// Task 2 review round 2, cheap item: `list_models` turns every non-200
    /// into a typed `Error` and discards the body, but `get`/`post_json`
    /// themselves must not — the probe design in Tasks 3–4 reads exactly the
    /// body of a non-2xx (an account-level error message, say) to show the
    /// user what the provider actually said, which depends on
    /// `http_status_as_error(false)` reaching all the way to the caller of
    /// `get`, not being swallowed inside it.
    #[test]
    fn a_non_2xx_status_still_returns_its_body_to_the_caller() {
        let server = MockServer::new(vec![Reply::status(
            401,
            r#"{"error":{"message":"bad key"}}"#,
        )]);
        let (status, body) =
            get(server.base(), "/models", None).expect("a 401 is not a transport error");
        assert_eq!(status, 401);
        assert!(
            body.contains("bad key"),
            "the body of a non-2xx must reach the caller: {body}"
        );
    }
}

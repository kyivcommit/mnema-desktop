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

/// Verified against the live endpoint 2026-08-08: this builder, these calls,
/// `POST` with a JSON body and a bearer header, and a 401 read as a body.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
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
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| Error::Transport(e.to_string()))?;
    Ok((status, body))
}

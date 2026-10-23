//! One request, one reply, over the application's agent socket. Knows nothing
//! about the index: it frames the request, forwards the reply, and turns a dead
//! or absent app into the error kind a caller can act on.

use mnema_core::agent_wire::{Envelope, ErrKind, MAX_LINE, Reply, Request, VERSION, WireError};
use serde_json::Value;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const OVERRIDE: &str = "MNEMA_AGENT_SOCKET";
const APP_ID: &str = "com.mnema.desktop";
const LAUNCH_WAIT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(200);
/// `rescan` with `wait` may take 600 s on the server side; a little more here.
const READ_TIMEOUT: Duration = Duration::from_secs(660);

pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os(OVERRIDE) {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join("Library/Application Support")
        .join(APP_ID)
        .join("agent.sock")
}

fn err(kind: ErrKind, message: &str) -> WireError {
    WireError {
        kind,
        message: message.to_string(),
    }
}

fn disabled() -> WireError {
    err(
        ErrKind::Disabled,
        "Mnema is running but agent access is off: turn it on in Mnema's settings.",
    )
}

/// The override exists for tests: with it set, the app is never launched.
fn connect(path: &PathBuf) -> Result<UnixStream, WireError> {
    if let Ok(s) = UnixStream::connect(path) {
        return Ok(s);
    }
    let not_running = || {
        err(
            ErrKind::NotRunning,
            "Mnema is not running and could not be started.",
        )
    };
    if std::env::var_os(OVERRIDE).is_some() {
        return Err(not_running());
    }
    let _ = Command::new("open")
        .args(["-g", "-b", APP_ID])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let start = Instant::now();
    while start.elapsed() < LAUNCH_WAIT {
        std::thread::sleep(POLL);
        if let Ok(s) = UnixStream::connect(path) {
            return Ok(s);
        }
    }
    // shortcut: pgrep tells "running" from "not running" by process name, upgrade if the executable is renamed
    let running = Command::new("pgrep")
        .args(["-x", "mnema-desktop"])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    Err(if running { disabled() } else { not_running() })
}

pub fn call(req: &Request) -> Result<Value, WireError> {
    call_on(&socket_path(), req, READ_TIMEOUT)
}

fn call_on(path: &PathBuf, req: &Request, read_timeout: Duration) -> Result<Value, WireError> {
    let mut line = serde_json::to_string(req).expect("a Request always serialises");
    // The server refuses a longer line and closes; our write would then fail
    // with EPIPE and its reply would be lost, so refuse here.
    if line.len() > MAX_LINE {
        return Err(err(
            ErrKind::BadRequest,
            &format!("request longer than {MAX_LINE} bytes"),
        ));
    }
    line.push('\n');
    let mut stream = connect(path)?;
    let internal = |what: &str, e: std::io::Error| err(ErrKind::Internal, &format!("{what}: {e}"));
    let _ = stream.set_read_timeout(Some(read_timeout));
    // A write the app closed on is the same event as a read it closed on.
    if stream.write_all(line.as_bytes()).is_err() {
        return Err(disabled());
    }
    let mut reply = String::new();
    match BufReader::new(stream).read_line(&mut reply) {
        // A hung app is not an app with access off.
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
            return Err(err(ErrKind::Internal, "the app did not answer in time"));
        }
        // The app closes connections when access is turned off.
        Ok(0) | Err(_) if reply.is_empty() => return Err(disabled()),
        Err(e) => return Err(internal("reading the reply", e)),
        Ok(_) => {}
    }
    let env: Envelope = serde_json::from_str(&reply)
        .map_err(|e| err(ErrKind::Internal, &format!("unreadable reply: {e}")))?;
    if env.v != VERSION {
        return Err(err(
            ErrKind::Version,
            "the app speaks another protocol version",
        ));
    }
    match env.reply {
        Reply::Ok(v) => Ok(v),
        Reply::Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mnema_core::agent_wire::Op;
    use std::os::unix::net::UnixListener;

    fn status() -> Request {
        Request {
            v: VERSION,
            client: None,
            op: Op::Status,
        }
    }

    #[test]
    fn an_app_that_never_answers_is_internal_not_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let hold = std::thread::spawn(move || {
            // Accept and keep the stream open without a word.
            let (stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(600));
            drop(stream);
        });
        let e = call_on(&path, &status(), Duration::from_millis(100)).unwrap_err();
        assert_eq!(e.kind, ErrKind::Internal, "{e:?}");
        assert!(e.message.contains("did not answer in time"), "{e:?}");
        hold.join().unwrap();
    }

    #[test]
    fn an_app_that_closes_without_answering_is_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let hold = std::thread::spawn(move || drop(listener.accept().unwrap().0));
        let e = call_on(&path, &status(), Duration::from_secs(5)).unwrap_err();
        assert_eq!(e.kind, ErrKind::Disabled, "{e:?}");
        hold.join().unwrap();
    }
}

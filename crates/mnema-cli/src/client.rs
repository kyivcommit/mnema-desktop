//! One request, one reply, over the application's agent socket. Knows nothing
//! about the index: it frames the request, forwards the reply, and turns a dead
//! or absent app into the error kind a caller can act on.

use mnema_core::agent_wire::{Envelope, ErrKind, MAX_LINE, Reply, Request, VERSION, WireError};
use serde_json::Value;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
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

/// `…/X.app/Contents/MacOS/<exe>` → `…/X.app`, so the CLI starts the copy it
/// ships in rather than whichever one LaunchServices prefers.
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app")
        .then(|| app.to_path_buf())
}

fn launch_app() {
    let bundle = std::env::current_exe()
        .ok()
        .and_then(|e| e.canonicalize().ok())
        .and_then(|e| bundle_of(&e));
    let mut open = Command::new("open");
    open.arg("-g");
    match bundle {
        Some(app) => open.arg(app),
        None => open.args(["-b", APP_ID]),
    };
    let _ = open.stdout(Stdio::null()).stderr(Stdio::null()).status();
}

fn app_running() -> bool {
    // shortcut: pgrep tells "running" from "not running" by process name, upgrade if the executable is renamed
    Command::new("pgrep")
        .args(["-x", "mnema-desktop"])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The override exists for tests: with it set, the app is never launched.
fn connect(path: &Path) -> Result<UnixStream, WireError> {
    if std::env::var_os(OVERRIDE).is_some() {
        return UnixStream::connect(path).map_err(|_| not_running());
    }
    connect_with(path, &app_running, &launch_app, LAUNCH_WAIT)
}

fn not_running() -> WireError {
    err(
        ErrKind::NotRunning,
        "Mnema is not running and could not be started.",
    )
}

/// A running app without a socket has access off: `open` on it would only
/// raise a window, and waiting would not help.
fn connect_with(
    path: &Path,
    running: &dyn Fn() -> bool,
    launch: &dyn Fn(),
    wait: Duration,
) -> Result<UnixStream, WireError> {
    if let Ok(s) = UnixStream::connect(path) {
        return Ok(s);
    }
    if running() {
        return Err(disabled());
    }
    launch();
    let start = Instant::now();
    while start.elapsed() < wait {
        std::thread::sleep(POLL);
        if let Ok(s) = UnixStream::connect(path) {
            return Ok(s);
        }
    }
    Err(if running() { disabled() } else { not_running() })
}

pub fn call(req: &Request) -> Result<Value, WireError> {
    call_on(&socket_path(), req, READ_TIMEOUT)
}

fn call_on(path: &Path, req: &Request, read_timeout: Duration) -> Result<Value, WireError> {
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

    use std::cell::Cell;

    #[test]
    fn a_running_app_without_a_socket_is_disabled_at_once_and_is_not_launched() {
        let dir = tempfile::tempdir().unwrap();
        let launched = Cell::new(false);
        let t = Instant::now();
        let e = connect_with(
            &dir.path().join("none.sock"),
            &|| true,
            &|| launched.set(true),
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(e.kind, ErrKind::Disabled, "{e:?}");
        assert!(!launched.get(), "a running app must not be sent `open`");
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "{:?}",
            t.elapsed()
        );
    }

    #[test]
    fn an_app_that_is_not_running_is_launched_and_then_reported_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let launched = Cell::new(false);
        let e = connect_with(
            &dir.path().join("none.sock"),
            &|| false,
            &|| launched.set(true),
            Duration::from_millis(300),
        )
        .unwrap_err();
        assert!(launched.get());
        assert_eq!(e.kind, ErrKind::NotRunning, "{e:?}");
    }

    #[test]
    fn a_launched_app_that_opens_its_socket_is_connected_to() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let keep = Cell::new(None);
        let ok = connect_with(
            &path,
            &|| false,
            &|| keep.set(Some(UnixListener::bind(&path).unwrap())),
            Duration::from_secs(5),
        );
        assert!(ok.is_ok());
    }

    #[test]
    fn the_bundle_is_derived_from_the_executable_path() {
        let p = |s: &str| bundle_of(Path::new(s));
        assert_eq!(
            p("/Apps/Mnema.app/Contents/MacOS/mnema"),
            Some(PathBuf::from("/Apps/Mnema.app"))
        );
        assert_eq!(p("/usr/local/bin/mnema"), None);
        assert_eq!(p("/Apps/Mnema.app/Contents/Resources/mnema"), None);
        assert_eq!(p("/Apps/Mnema/Contents/MacOS/mnema"), None);
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

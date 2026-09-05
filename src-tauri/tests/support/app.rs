//! The application fixture shared by `tests/commands.rs` and
//! `tests/mask_differential.rs`: a mock-runtime app over a temporary index,
//! the IPC `call`, and a walk driven to its `Ended` event.
//!
//! Pulled in with `#[path = "support/app.rs"]` by the binaries that want it,
//! not declared in `support/mod.rs` — that module is compiled into EVERY
//! binary under `tests/`, and `model_commands.rs` has no use for an app
//! fixture, where it would be dead code under `-D warnings`. Same reason
//! `support/fixture.rs` sits beside `mod.rs` rather than inside it.

use std::sync::mpsc;
use std::time::Duration;

use mnema_desktop::job::JobEvent;
use mnema_desktop::state::AppState;
use mnema_desktop::walk_job;
use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, Channel, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindow, WebviewWindowBuilder};

// This file is compiled into every binary that pulls it in, so an item only
// one of them uses is dead code in the others and `-D warnings` refuses the
// build. Each `#[allow(dead_code)]` below marks an item `commands.rs` uses and
// `mask_differential.rs` does not: the attribute says "another binary wants
// this", never "nobody does".

/// A provider address with nothing behind it. Nothing in the binaries that
/// pull this file calls the provider, and a base that refuses instantly is
/// how a future test that starts to finds out at once rather than by
/// reaching the real one.
pub const NO_PROVIDER: &str = "http://127.0.0.1:1";

/// A credential reference that cannot reach a store at all — the same trick
/// `NO_PROVIDER` uses, one line up.
///
/// Empty is not carelessness: `mnema_secrets::entry` refuses an empty
/// reference before it installs or consults any store —
/// `Error::EmptyReference`. It refuses because of the macOS keychain, where
/// an empty attribute is a wildcard matching another configuration's key.
///
/// Kept only for the two fixtures in `commands.rs` that build `AppState`
/// directly and never call `search` or a model command; `app_in` below wants a
/// real store.
#[allow(dead_code)]
pub const NO_CREDENTIAL: &str = "";

/// An application whose data directory is a temporary one.
///
/// The real one is `app_local_data_dir()`, which under the mock context would
/// resolve inside the developer's own Application Support folder. A test must
/// not write there, which is the reason the directory is resolved once at
/// start-up and held in state rather than derived inside each command.
///
/// **A real, in-memory, empty store**, not `NO_CREDENTIAL`'s unreachable
/// one: `search` now reads it even with no key entered, so this fixture
/// needs "no key" (`Error::NoKey`), not "no store" (`Error::Secrets`).
pub fn app_in(dir: &std::path::Path) -> tauri::App<MockRuntime> {
    mnema_secrets::test_store::register();
    mock_builder()
        .manage(AppState::new(
            dir.to_path_buf(),
            super::support::worker().to_path_buf(),
            NO_PROVIDER.to_string(),
            format!("mnema-desktop-test-{}", dir.display()),
        ))
        .invoke_handler(mnema_desktop::invoke_handler())
        .build(mock_context(noop_assets()))
        .expect("failed to build the mock application")
}

pub fn main_webview(app: &tauri::App<MockRuntime>) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, "main", Default::default())
        .build()
        .expect("failed to build the mock webview")
}

/// The origin the webview actually sends from, which is not one string.
///
/// Tauri serves the embedded assets over `tauri://localhost`, except on Windows
/// and Android, where WebView2 cannot register a custom scheme and Tauri falls
/// back to `http://tauri.localhost` — `tauri-2.11.5/src/manager/mod.rs:339`,
/// which branches on `cfg!(windows)`.
///
/// This matters because the ACL classifies a request by its origin. A request
/// arriving from anything other than the local origin is `ExecutionContext::
/// Remote`, and no capability here grants a remote context, so every command is
/// refused before it runs — reported as `"<cmd> not allowed. Plugin not found"`,
/// which names neither the origin nor the ACL.
///
/// Measured on a Windows 11 stand on 2026-07-29, where the macOS constant these
/// two call sites used to hold turned into **six** failures across
/// `commands.rs`, including one that looked unrelated: `the_commands_that_touch_
/// the_database_leave_the_main_thread` compared a thread id against itself,
/// because a refusal answers inline and never reaches a worker. One constant,
/// six red tests, and no message pointing at it.
pub(crate) fn local_origin() -> &'static str {
    if cfg!(windows) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    }
}

/// Invokes a command the way the webview does. `Err` carries what the webview
/// would receive, which for this shell is always a string.
pub fn call(webview: &WebviewWindow<MockRuntime>, cmd: &str, args: Value) -> Result<Value, Value> {
    tauri::test::get_ipc_response(
        webview,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: local_origin().parse().unwrap(),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize::<Value>().expect("non-JSON response"))
}

/// Collects what the webview would receive on a job channel.
pub fn job_channel() -> (Channel<JobEvent>, mpsc::Receiver<Value>) {
    let (tx, rx) = mpsc::channel();
    let channel = Channel::new(move |body| {
        let json: Value = body.deserialize().expect("the job event was not JSON");
        let _ = tx.send(json);
        Ok(())
    });
    (channel, rx)
}

/// Starts a real walk over `root_id` and blocks until the window would have
/// heard `Ended`, returning its payload for the caller to inspect.
///
/// Calls `walk_job::start_walk_job` directly on `state` rather than through
/// `call`, the same choice `a_started_job_reports_progress_and_a_cancelled_
/// one_stops` already makes for the probe: the argument is a real
/// `tauri::ipc::Channel`, built from a callback this test can read, and the
/// raw-IPC path (`"__CHANNEL__:N"`) has nothing on the other end for that
/// callback to be. IPC *reachability* is a separate question, asked by
/// `the_walk_job_is_reachable_through_the_ipc` in `commands.rs`.
pub fn run_walk_and_capture_ending(app: &tauri::App<MockRuntime>, root_id: i64) -> Value {
    let state = app.state::<AppState>();
    let (channel, events) = job_channel();
    walk_job::start_walk_job(state.clone(), root_id, channel).expect("the walk would not start");

    loop {
        match events.recv_timeout(Duration::from_secs(20)) {
            Ok(event) if event["event"] == json!("ended") => return event["data"].clone(),
            Ok(_) => continue,
            Err(_) => panic!("the walk never told the window it ended"),
        }
    }
}

/// The common case: a walk over a fixture with nothing ambiguous in it must
/// simply finish.
#[allow(dead_code)]
pub fn run_walk_to_completion(app: &tauri::App<MockRuntime>, root_id: i64) {
    let ending = run_walk_and_capture_ending(app, root_id);
    assert_eq!(
        ending["reason"],
        json!("completed"),
        "the walk over the fixture folder did not complete: {ending}"
    );
}

/// What the index actually holds under one root, sorted — the same list
/// reconciliation itself compares a walk against (`Db::paths_under_root`).
///
/// `Ended::removed` is a number and this is the fact behind it: a walk that
/// reported `removed: 1` and a walk that reported `removed: 1` while deleting
/// the wrong row are the same number. Every exclusion test in `commands.rs`
/// asserts on both.
#[allow(dead_code)]
pub fn indexed_paths(app: &tauri::App<MockRuntime>, root_id: i64) -> Vec<String> {
    app.state::<AppState>()
        .with_index(|db| db.paths_under_root(root_id))
        .expect("reading the paths the index holds under the root")
}

/// An application whose extraction worker is a stand-in that reads nothing.
///
/// The stand-in answers the manifest handshake by delegating to the real
/// worker, and answers every file with bytes that are not valid UTF-8, which
/// the pool turns into `Failure::Crash` for every single file — the shape D44
/// was written for and the one `mnema-ingest`'s own
/// `a_worker_that_answers_nothing_useful_stops_the_walk` uses. Delegating the
/// handshake is what makes the fixture mean anything: a stand-in that failed
/// the handshake too would stop the walk there, and a test about what happens
/// to files afterwards would be measuring the handshake instead.
///
/// `scratch` must not be a watched folder — the script is written into it, and
/// `enumerate` would list the script itself as a found file.
#[cfg(unix)]
#[allow(dead_code)]
pub fn app_with_a_worker_that_reads_nothing(
    dir: &std::path::Path,
    scratch: &std::path::Path,
) -> tauri::App<MockRuntime> {
    mnema_secrets::test_store::register();
    let manifest = format!(
        "if [ \"$1\" = \"--manifest\" ]; then\n  exec \"{}\" --manifest\nfi\n",
        super::support::worker().display()
    );
    let worker = write_script(
        scratch,
        "worker-that-reads-nothing",
        &format!("{manifest}while read -r _line; do\n  printf '\\377\\376\\n'\ndone\n"),
    );
    mock_builder()
        .manage(AppState::new(
            dir.to_path_buf(),
            worker,
            NO_PROVIDER.to_string(),
            format!("mnema-desktop-test-{}", dir.display()),
        ))
        .invoke_handler(mnema_desktop::invoke_handler())
        .build(mock_context(noop_assets()))
        .expect("failed to build the mock application")
}

/// Writes an executable stand-in and waits until the kernel will run it.
///
/// Copied from `crates/mnema-ingest/tests/support/mod.rs` rather than shared,
/// for the reason `support::worker` gives at length: another crate's `tests/`
/// directory is not reachable from this one. The retry loop is the measured
/// half — between a `fork` and the `exec` that follows it a child holds a
/// duplicate of every descriptor its parent had open, including this file's, so
/// Linux refuses the script with `ETXTBSY` for as long as that window lasts.
#[cfg(unix)]
#[allow(dead_code)]
fn write_script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

    for _ in 0..400 {
        match Command::new(&path)
            .arg("--manifest")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                let _ = child.wait();
                return path;
            }
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => panic!(
                "the stand-in at {} will not run at all: {e}",
                path.display()
            ),
        }
    }
    panic!(
        "{} was still reported as busy after two seconds of retrying",
        path.display()
    );
}

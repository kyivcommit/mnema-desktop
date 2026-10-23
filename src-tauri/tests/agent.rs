//! The agent handlers, called as `agent::handle` with a real index behind them.

mod support;

#[path = "support/app.rs"]
mod app;
use app::app_in;
use support::scan::{run_scan_watching, scan_to_completion};

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mnema_core::agent_wire::{ErrKind, Op};
use mnema_desktop::agent::{AgentServer, agent_log, handle, socket_path};
use mnema_desktop::scan_state::{Entry, ScanSnapshot};
use mnema_desktop::state::AppState;
use serde_json::Value;
use tauri::Manager;

fn fixture_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("animals.txt"),
        "the quick brown fox jumps over the lazy dog",
    )
    .unwrap();
    dir
}

/// An app with the index open and `dir` watched and scanned.
fn indexed(data: &std::path::Path, dir: &std::path::Path) -> tauri::App<tauri::test::MockRuntime> {
    let app = unfolded(data);
    let state = app.state::<AppState>();
    state
        .with_index(|db| db.insert_watched_root(dir.to_str().unwrap()))
        .unwrap();
    scan_to_completion(app.handle());
    app
}

fn unfolded(data: &std::path::Path) -> tauri::App<tauri::test::MockRuntime> {
    let app = app_in(data);
    app.state::<AppState>().open_index().unwrap();
    app
}

fn search(state: &AppState, query: &str, k: u32) -> Value {
    handle(
        state,
        Op::Search {
            query: query.into(),
            k: Some(k),
        },
    )
    .expect("search failed")
}

fn passage(
    state: &AppState,
    id: &str,
    context: u32,
) -> Result<Value, mnema_core::agent_wire::WireError> {
    handle(
        state,
        Op::Passage {
            id: id.into(),
            context: Some(context),
        },
    )
}

fn first_id(reply: &Value) -> String {
    reply["passages"][0]["id"]
        .as_str()
        .expect("no passage")
        .to_string()
}

#[test]
fn search_answers_verbatim_passages_with_their_place() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = indexed(data.path(), dir.path());
    let state = app.state::<AppState>();
    let reply = search(&state, "fox", 8);
    let p = &reply["passages"][0];
    assert!(p["text"].as_str().unwrap().contains("fox"), "{reply}");
    assert_eq!(p["path"], "animals.txt");
    assert_eq!(
        p["absolutePath"].as_str().unwrap(),
        dir.path().join("animals.txt").display().to_string()
    );
    assert_eq!(p["freshness"], "current");
    assert!(
        p["locator"].as_str().is_some_and(|l| !l.is_empty()),
        "{reply}"
    );
    assert_eq!(reply["empty"], false);
    assert!(reply["arms"]["text"].is_object() && reply["arms"]["content"].is_object());
}

#[test]
fn search_with_k_one_answers_one_passage() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    std::fs::write(dir.path().join("more.txt"), "another fox in another file").unwrap();
    let app = indexed(data.path(), dir.path());
    let state = app.state::<AppState>();
    assert_eq!(
        search(&state, "fox", 8)["passages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        search(&state, "fox", 1)["passages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn search_on_an_empty_index_is_empty_not_an_error() {
    let data = tempfile::tempdir().unwrap();
    let app = unfolded(data.path());
    let reply = search(&app.state::<AppState>(), "fox", 8);
    assert_eq!(reply["passages"].as_array().unwrap().len(), 0, "{reply}");
    assert_eq!(reply["empty"], true);
}

#[test]
fn a_file_changed_after_indexing_is_marked_in_search_and_passage() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = indexed(data.path(), dir.path());
    let state = app.state::<AppState>();
    let id = first_id(&search(&state, "fox", 8));
    std::fs::write(
        dir.path().join("animals.txt"),
        "a different and noticeably longer body of text",
    )
    .unwrap();
    let after = search(&state, "fox", 8);
    assert_eq!(after["passages"][0]["freshness"], "fileChanged", "{after}");
    let p = passage(&state, &id, 2).unwrap();
    assert_eq!(p["freshness"], "fileChanged", "{p}");
}

#[test]
fn passage_of_a_fresh_id_gives_that_text() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = indexed(data.path(), dir.path());
    let state = app.state::<AppState>();
    let found = search(&state, "fox", 8);
    let p = passage(&state, &first_id(&found), 2).unwrap();
    let focused: Vec<&Value> = p["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| b["focus"] == true)
        .collect();
    assert!(!focused.is_empty(), "{p}");
    assert!(focused[0]["text"].as_str().unwrap().contains("fox"));
    assert_eq!(p["path"], "animals.txt");
    assert_eq!(p["freshness"], "current");
}

#[test]
fn passage_after_reindex_is_stale_not_another_place() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = indexed(data.path(), dir.path());
    let state = app.state::<AppState>();
    let id = first_id(&search(&state, "fox", 8));
    std::fs::write(
        dir.path().join("animals.txt"),
        "entirely other words, with a fox still in them",
    )
    .unwrap();
    scan_to_completion(app.handle());
    let e = passage(&state, &id, 2).expect_err("an old id answered");
    assert_eq!(e.kind, ErrKind::StaleId, "{e:?}");
}

#[test]
fn passage_of_an_unknown_chunk_is_not_found() {
    let data = tempfile::tempdir().unwrap();
    let app = unfolded(data.path());
    let e = passage(&app.state::<AppState>(), "999999:deadbeef:0", 2).unwrap_err();
    assert_eq!(e.kind, ErrKind::NotFound, "{e:?}");
}

#[test]
fn passage_context_bounds_the_blocks() {
    let data = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let paragraphs: Vec<String> = (0..7)
        .map(|i| format!("paragraph{i} {}", format!("filler{i} ").repeat(120)))
        .collect();
    std::fs::write(dir.path().join("long.txt"), paragraphs.join("\n\n")).unwrap();
    let app = indexed(data.path(), dir.path());
    let state = app.state::<AppState>();
    let id = first_id(&search(&state, "paragraph3", 1));
    let blocks = |context| {
        let p = passage(&state, &id, context).unwrap();
        let b = p["blocks"].as_array().unwrap().clone();
        let focus = b.iter().filter(|x| x["focus"] == true).count();
        (b.len(), focus)
    };
    let (all0, focus0) = blocks(0);
    assert!(
        focus0 > 0 && focus0 < 7,
        "fixture must focus a part: {focus0}"
    );
    assert_eq!(all0, focus0, "context 0 is the focus alone");
    let (all2, focus2) = blocks(2);
    assert_eq!(focus2, focus0);
    assert!(all2 > focus0 && all2 <= focus0 + 4, "{all2} vs {focus0}");
}

#[test]
fn status_carries_the_scan_state_and_folders() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = indexed(data.path(), dir.path());
    let s = handle(&app.state::<AppState>(), Op::Status).unwrap();
    assert!(s["scan"]["revision"].as_u64().unwrap() > 0, "{s}");
    assert_eq!(s["folders"][0]["path"], dir.path().display().to_string());
    assert_eq!(s["folders"][0]["available"], true);
    drop(dir);
    let s = handle(&app.state::<AppState>(), Op::Status).unwrap();
    assert_eq!(s["folders"][0]["available"], false, "{s}");
}

#[test]
fn rescan_while_running_says_running_not_error() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = unfolded(data.path());
    app.state::<AppState>()
        .with_index(|db| db.insert_watched_root(dir.path().to_str().unwrap()))
        .unwrap();
    let seen: Arc<Mutex<Option<Value>>> = Arc::default();
    let slot = Arc::clone(&seen);
    run_scan_watching(
        app.handle(),
        Entry::Full,
        Duration::from_secs(60),
        move |state, now| {
            let mut slot = slot.lock().unwrap();
            if slot.is_none() && matches!(now.snapshot, ScanSnapshot::Running { .. }) {
                *slot =
                    Some(handle(state, Op::Rescan { wait: None }).expect("busy is not an error"));
            }
        },
    );
    let reply = seen
        .lock()
        .unwrap()
        .clone()
        .expect("the scan never reported Running");
    assert_eq!(reply["started"], false, "{reply}");
    assert_eq!(reply["running"], true, "{reply}");
}

#[test]
fn rescan_wait_returns_the_report() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = indexed(data.path(), dir.path());
    let reply = handle(&app.state::<AppState>(), Op::Rescan { wait: Some(true) }).unwrap();
    assert_eq!(reply["started"], true, "{reply}");
    assert_eq!(reply["running"], false, "{reply}");
    assert!(reply["report"]["reason"].is_string(), "{reply}");
}

// ── The socket ───────────────────────────────────────────────────────────────

/// An app with the index open and the agent server managed, as `.setup` does.
fn served(data: &std::path::Path) -> tauri::App<tauri::test::MockRuntime> {
    let app = unfolded(data);
    app.manage(AgentServer::default());
    app
}

fn set_access(
    app: &tauri::App<tauri::test::MockRuntime>,
    on: bool,
) -> Result<bool, mnema_desktop::error::Error> {
    mnema_desktop::prefs::set_agent_access(
        app.handle().clone(),
        app.state::<AppState>(),
        app.state::<AgentServer>(),
        on,
    )
}

fn access_on(app: &tauri::App<tauri::test::MockRuntime>) -> PathBuf {
    assert!(set_access(app, true).expect("access could not be turned on"));
    socket_path(app.state::<AppState>().data_dir())
}

struct Client {
    w: UnixStream,
    r: BufReader<UnixStream>,
}

impl Client {
    fn connect(path: &std::path::Path) -> Client {
        let w = UnixStream::connect(path).expect("connect");
        w.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let r = BufReader::new(w.try_clone().unwrap());
        Client { w, r }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.w.write_all(bytes).expect("write");
    }

    /// The next reply line, or the error the read ended in.
    fn read(&mut self) -> std::io::Result<Option<Value>> {
        let mut line = String::new();
        match self.r.read_line(&mut line)? {
            0 => Ok(None),
            _ => Ok(Some(serde_json::from_str(&line).expect("a reply is JSON"))),
        }
    }

    fn ask(&mut self, line: &str) -> Value {
        self.send(format!("{line}\n").as_bytes());
        self.read().expect("read").expect("the server closed")
    }

    /// True when the next read ends the way a cut connection does, and false
    /// when it gave data or only timed out.
    fn is_cut(&mut self) -> bool {
        match self.r.fill_buf() {
            Ok(b) => b.is_empty(),
            Err(e) => !matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ),
        }
    }
}

const STATUS: &str = r#"{"v":1,"op":"status"}"#;

#[test]
fn turning_access_on_creates_a_socket_only_the_user_can_open() {
    let data = tempfile::tempdir().unwrap();
    let app = served(data.path());
    let path = access_on(&app);
    let mode = std::fs::metadata(&path)
        .expect("no socket")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert!(mnema_desktop::prefs::agent_access(data.path()));
}

#[test]
fn access_is_off_by_default_and_no_socket_exists() {
    let data = tempfile::tempdir().unwrap();
    let app = served(data.path());
    assert!(!mnema_desktop::prefs::agent_access(data.path()));
    // A boot with the switch never touched must not open anything.
    mnema_desktop::agent::start_if_enabled(app.handle());
    assert!(!socket_path(data.path()).exists());
}

#[test]
fn a_boot_with_access_left_on_opens_the_socket() {
    let data = tempfile::tempdir().unwrap();
    let app = served(data.path());
    mnema_desktop::prefs::write_key(data.path(), "agent_access", Value::Bool(true)).unwrap();
    mnema_desktop::agent::start_if_enabled(app.handle());
    let mut c = Client::connect(&socket_path(data.path()));
    assert_eq!(c.ask(STATUS)["v"], 1);
}

#[test]
fn a_boot_with_access_off_removes_a_socket_a_crash_left() {
    let data = tempfile::tempdir().unwrap();
    let app = served(data.path());
    std::fs::write(socket_path(data.path()), b"left over").unwrap();
    mnema_desktop::agent::start_if_enabled(app.handle());
    assert!(!socket_path(data.path()).exists());
}

#[test]
fn turning_access_off_removes_the_socket_and_cuts_an_open_connection() {
    let data = tempfile::tempdir().unwrap();
    let app = served(data.path());
    let path = access_on(&app);
    let mut c = Client::connect(&path);
    // A reply proves the server has this stream registered.
    assert_eq!(c.ask(STATUS)["v"], 1);
    assert!(!set_access(&app, false).unwrap());
    assert!(!path.exists(), "the socket file survived");
    assert!(c.is_cut(), "the open connection was not cut");
    assert!(!mnema_desktop::prefs::agent_access(data.path()));
}

#[test]
fn a_socket_left_by_a_crash_is_replaced_on_start() {
    let data = tempfile::tempdir().unwrap();
    let app = served(data.path());
    let path = socket_path(data.path());
    std::fs::write(&path, b"left over").unwrap();
    std::fs::write(data.path().join("agent.tmp"), b"left over").unwrap();
    access_on(&app);
    let mut c = Client::connect(&path);
    assert_eq!(c.ask(STATUS)["v"], 1);
}

#[test]
fn an_overlong_line_is_refused_and_the_connection_closed() {
    let data = tempfile::tempdir().unwrap();
    let app = served(data.path());
    let mut c = Client::connect(&access_on(&app));

    // Exactly the limit is a line like any other: it fails as JSON, not as long,
    // and the connection serves the next request.
    c.send(&[b'a'; mnema_core::agent_wire::MAX_LINE]);
    c.send(b"\n");
    let at_limit = c.read().unwrap().unwrap();
    assert_eq!(at_limit["err"]["kind"], "bad_request");
    assert!(
        !at_limit["err"]["message"]
            .as_str()
            .unwrap()
            .contains("longer"),
        "{at_limit}"
    );
    assert_eq!(c.ask(STATUS)["v"], 1);

    // One byte more is refused as long, and nothing further is served.
    c.send(&[b'a'; mnema_core::agent_wire::MAX_LINE + 1]);
    let over = c.read().unwrap().unwrap();
    assert_eq!(over["err"]["kind"], "bad_request");
    assert!(
        over["err"]["message"].as_str().unwrap().contains("longer"),
        "{over}"
    );
    assert!(c.is_cut(), "the connection stayed open");
}

#[test]
fn a_ninth_connection_is_refused() {
    let data = tempfile::tempdir().unwrap();
    let app = served(data.path());
    let path = access_on(&app);
    let mut held: Vec<Client> = (0..8).map(|_| Client::connect(&path)).collect();
    for c in &mut held {
        assert_eq!(c.ask(STATUS)["v"], 1, "one of the first eight was refused");
    }
    let mut ninth = Client::connect(&path);
    let refusal = ninth.read().unwrap().expect("no refusal");
    assert_eq!(refusal["err"]["kind"], "internal");
    assert_eq!(refusal["err"]["message"], "too many connections");
    assert!(ninth.is_cut());
}

#[test]
fn a_socket_path_too_long_is_an_error_not_a_panic() {
    let base = tempfile::tempdir().unwrap();
    // `agent.sock` is 10 bytes and one `/` precedes it; the limit is 103.
    let nested = |total: usize| {
        let fixed = base.path().as_os_str().len() + 1 + 1 + "agent.sock".len();
        let dir = base.path().join("d".repeat(total - fixed));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    };
    let ok = nested(103);
    assert_eq!(socket_path(&ok).as_os_str().len(), 103);
    let app = served(&ok);
    set_access(&app, true).expect("a path at the limit must work");

    let too_long = nested(104);
    let app = served(&too_long);
    let e = set_access(&app, true).expect_err("a path over the limit was accepted");
    assert!(e.to_string().contains("limit"), "{e}");
    assert!(!mnema_desktop::prefs::agent_access(&too_long));
    let way_over = nested(120);
    let app = served(&way_over);
    assert!(set_access(&app, true).is_err());
}

#[test]
fn turning_access_off_during_rescan_wait_cuts_the_client_but_not_the_scan() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = served(data.path());
    app.state::<AppState>()
        .with_index(|db| db.insert_watched_root(dir.path().to_str().unwrap()))
        .unwrap();
    let path = access_on(&app);
    let handle_for_watcher = app.handle().clone();
    let cut: Arc<Mutex<Option<bool>>> = Arc::default();
    let slot = Arc::clone(&cut);
    let (_, ended) = run_scan_watching(
        app.handle(),
        Entry::Full,
        Duration::from_secs(60),
        move |_, now| {
            let mut slot = slot.lock().unwrap();
            if slot.is_some() || !matches!(now.snapshot, ScanSnapshot::Running { .. }) {
                return;
            }
            // The announcing thread is the scan's own and is held here, so the
            // scan cannot end while the request waits on it.
            let mut c = Client::connect(&path);
            c.send(b"{\"v\":1,\"op\":\"rescan\",\"wait\":true}\n");
            std::thread::sleep(Duration::from_millis(300));
            mnema_desktop::prefs::set_agent_access(
                handle_for_watcher.clone(),
                handle_for_watcher.state::<AppState>(),
                handle_for_watcher.state::<AgentServer>(),
                false,
            )
            .unwrap();
            *slot = Some(c.is_cut());
        },
    );
    assert_eq!(
        *cut.lock().unwrap(),
        Some(true),
        "the waiting client was not cut"
    );
    assert!(
        matches!(ended.snapshot, ScanSnapshot::Ended { .. }),
        "the scan did not run to its end"
    );
}

#[test]
fn the_log_keeps_the_last_fifty_newest_first() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    std::fs::write(dir.path().join("more.txt"), "another fox in another file").unwrap();
    let app = indexed(data.path(), dir.path());
    app.manage(AgentServer::default());
    let mut c = Client::connect(&access_on(&app));
    for i in 0..51 {
        // Two passages exist; the last request asks for one of them.
        let k = if i == 50 { 1 } else { 2 };
        let r = c.ask(&format!(
            r#"{{"v":1,"op":"search","query":"fox","k":{k},"client":"c{i}"}}"#
        ));
        assert!(r["ok"].is_object(), "{r}");
    }
    let log = agent_log(app.state::<AgentServer>());
    assert_eq!(log.len(), 50);
    assert_eq!(log[0].client.as_deref(), Some("c50"));
    assert_eq!(log[0].passages, Some(1));
    assert_eq!(log[0].op, "search");
    assert_eq!(log[0].query.as_deref(), Some("fox"));
    assert_eq!(log[1].passages, Some(2));
    assert_eq!(log[49].client.as_deref(), Some("c1"));
    assert!(
        log[0].at.ends_with('Z') && log[0].at.contains('T'),
        "{}",
        log[0].at
    );
}

#[test]
fn a_request_round_trips_over_the_socket() {
    let data = tempfile::tempdir().unwrap();
    let dir = fixture_dir();
    let app = indexed(data.path(), dir.path());
    app.manage(AgentServer::default());
    let mut c = Client::connect(&access_on(&app));
    let over_socket = c.ask(r#"{"v":1,"op":"search","query":"fox","k":8}"#);
    let direct = search(&app.state::<AppState>(), "fox", 8);
    assert_eq!(over_socket["v"], 1);
    assert_eq!(over_socket["ok"], direct);
    assert!(!direct["passages"].as_array().unwrap().is_empty());
}

#[test]
fn a_malformed_request_over_the_socket_is_a_bad_request_and_the_connection_lives() {
    let data = tempfile::tempdir().unwrap();
    let app = served(data.path());
    let mut c = Client::connect(&access_on(&app));
    let r = c.ask("not json");
    assert_eq!(r["err"]["kind"], "bad_request");
    assert_eq!(c.ask(STATUS)["v"], 1);
}

// ── The switch ───────────────────────────────────────────────────────────────

/// A directory whose socket path is `total` bytes.
fn dir_for_socket_path_of(base: &std::path::Path, total: usize) -> PathBuf {
    let fixed = base.as_os_str().len() + 1 + 1 + "agent.sock".len();
    let dir = base.join("d".repeat(total - fixed));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn enabled(app: &tauri::App<tauri::test::MockRuntime>) -> bool {
    mnema_desktop::prefs::agent_access_enabled(app.state::<AgentServer>())
}

#[test]
fn turning_access_off_cuts_the_socket_even_when_the_prefs_write_fails() {
    let data = tempfile::tempdir().unwrap();
    let app = served(data.path());
    let path = access_on(&app);
    let mut c = Client::connect(&path);
    assert_eq!(c.ask(STATUS)["v"], 1);
    std::fs::set_permissions(data.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    let off = set_access(&app, false);
    std::fs::set_permissions(data.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(off.is_err(), "the prefs write was expected to fail here");
    assert!(c.is_cut(), "the connection outlived a failed switch-off");
    assert!(!enabled(&app));
}

#[test]
fn concurrent_toggles_leave_the_prefs_and_the_socket_in_agreement() {
    let data = tempfile::tempdir().unwrap();
    let app = served(data.path());
    let threads: Vec<_> = [true, false]
        .into_iter()
        .map(|on| {
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                for _ in 0..150 {
                    let _ = mnema_desktop::prefs::set_agent_access(
                        handle.clone(),
                        handle.state::<AppState>(),
                        handle.state::<AgentServer>(),
                        on,
                    );
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let stored = mnema_desktop::prefs::agent_access(data.path());
    let listening = UnixStream::connect(socket_path(data.path())).is_ok();
    assert_eq!(stored, listening, "prefs and socket disagree");
    assert_eq!(stored, enabled(&app));
}

#[test]
fn a_boot_that_cannot_open_the_socket_switches_access_off() {
    let base = tempfile::tempdir().unwrap();
    let dir = dir_for_socket_path_of(base.path(), 120);
    let app = served(&dir);
    mnema_desktop::prefs::write_key(&dir, "agent_access", Value::Bool(true)).unwrap();
    mnema_desktop::agent::start_if_enabled(app.handle());
    assert!(!enabled(&app));
    assert!(!mnema_desktop::prefs::agent_access(&dir));
}

#[test]
fn the_switch_reports_the_socket_and_not_the_stored_value() {
    let data = tempfile::tempdir().unwrap();
    let app = served(data.path());
    mnema_desktop::prefs::write_key(data.path(), "agent_access", Value::Bool(true)).unwrap();
    assert!(mnema_desktop::prefs::agent_access(data.path()));
    assert!(!enabled(&app), "stored on, nothing listening");
    access_on(&app);
    assert!(enabled(&app));
}

#[test]
fn a_failed_start_with_a_failed_write_back_reports_the_start() {
    let base = tempfile::tempdir().unwrap();
    let dir = dir_for_socket_path_of(base.path(), 120);
    let app = served(&dir);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let on = set_access(&app, true);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let e = on.expect_err("start cannot succeed on this path");
    assert!(
        e.to_string().contains("limit"),
        "the start error was lost: {e}"
    );
}

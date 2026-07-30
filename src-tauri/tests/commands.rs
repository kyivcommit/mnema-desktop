//! The commands as the webview reaches them: through the IPC, by name, with a
//! JSON body — not as ordinary Rust functions.
//!
//! Calling the functions directly would prove they work and nothing about
//! whether they are registered, whether their arguments survive the camelCase
//! rename, or what an error looks like on the other side.

use std::sync::mpsc;
use std::time::Duration;

use mnema_core::{Block, BlockType, Coordinate, Locator, Segment, SourceKind};
use mnema_desktop::bridge;
use mnema_desktop::job::JobEvent;
use mnema_desktop::state::AppState;
use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, Channel, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindow, WebviewWindowBuilder};

/// An application whose data directory is a temporary one.
///
/// The real one is `app_local_data_dir()`, which under the mock context would
/// resolve inside the developer's own Application Support folder. A test must
/// not write there, which is the reason the directory is resolved once at
/// start-up and held in state rather than derived inside each command.
fn app_in(dir: &std::path::Path) -> tauri::App<MockRuntime> {
    mock_builder()
        .manage(AppState::new(dir.to_path_buf()))
        .invoke_handler(mnema_desktop::invoke_handler())
        .build(mock_context(noop_assets()))
        .expect("failed to build the mock application")
}

fn main_webview(app: &tauri::App<MockRuntime>) -> WebviewWindow<MockRuntime> {
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
/// two call sites used to hold turned into **six** failures across this file,
/// including one that looked unrelated: `the_commands_that_touch_the_database_
/// leave_the_main_thread` compared a thread id against itself, because a refusal
/// answers inline and never reaches a worker. One constant, six red tests, and
/// no message pointing at it.
fn local_origin() -> &'static str {
    if cfg!(windows) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    }
}

/// Invokes a command the way the webview does. `Err` carries what the webview
/// would receive, which for this shell is always a string.
fn call(webview: &WebviewWindow<MockRuntime>, cmd: &str, args: Value) -> Result<Value, Value> {
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

#[test]
fn the_application_puts_the_index_in_the_local_data_directory() {
    // Every other test here hands the state a temporary directory, so none of
    // them can see which directory the application itself picks. This calls what
    // `run()` calls. It reads two paths and writes nothing, so no database is
    // created in the real data directory.
    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("failed to build the mock application");

    mnema_desktop::manage_state(app.handle()).expect("the state would not be managed");

    let expected = app
        .path()
        .app_local_data_dir()
        .expect("no local data directory on this platform");
    assert_eq!(
        app.state::<AppState>().data_dir(),
        expected,
        "the index would not be where the local data directory is"
    );
}

/// Which thread produced the response to a command.
///
/// A blocking command runs inline in `run_invoke_handler`, so its responder fires
/// on the thread that called it. A command Tauri runs as a task answers from a
/// worker. Comparing the two thread ids is the only way from outside to tell
/// which kind a command is.
fn responding_thread(webview: &WebviewWindow<MockRuntime>, cmd: &str) -> std::thread::ThreadId {
    let (tx, rx) = mpsc::channel();
    webview.as_ref().clone().on_message(
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: local_origin().parse().unwrap(),
            body: InvokeBody::Json(json!({ "query": "" })),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
        Box::new(move |_, _, _, _, _| {
            let _ = tx.send(std::thread::current().id());
        }),
    );
    rx.recv_timeout(Duration::from_secs(10))
        .expect("the command never answered")
}

#[test]
fn the_commands_that_touch_the_database_leave_the_main_thread() {
    // A blocking command holds the main thread for as long as it runs. These two
    // take the index mutex, which a multi-hour indexing job also wants, and wait
    // up to BUSY_TIMEOUT for it. On the main thread that is a frozen window —
    // which is the thing the timeout is kept short to avoid, so having both would
    // make the timeout meaningless.
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);
    let here = std::thread::current().id();

    for cmd in ["open_index", "lexical_search"] {
        assert_ne!(
            responding_thread(&webview, cmd),
            here,
            "`{cmd}` answered on the calling thread, so it runs inline on the main one"
        );
    }

    // The counterweight. Without it this test would also pass against a Tauri
    // that ran everything off-thread, and would prove nothing about the two
    // attributes above.
    assert_eq!(
        responding_thread(&webview, "cancel_job"),
        here,
        "`cancel_job` is meant to be blocking; if it is not, this test cannot \
         tell a deliberate attribute from Tauri's default"
    );
}

#[test]
fn opening_the_index_creates_it_in_the_data_directory_and_reports_its_version() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);

    let info = call(&webview, "open_index", json!({})).expect("open_index was rejected");

    let expected = dir.path().join("index.sqlite");
    assert_eq!(info["path"], json!(expected.display().to_string()));
    assert_eq!(
        info["schemaVersion"],
        json!(mnema_index::SCHEMA_VERSION),
        "the webview is told a schema version, and it has to be the real one"
    );
    assert!(
        expected.exists(),
        "open_index answered without a database on disk at {}",
        expected.display()
    );
}

#[test]
fn searching_before_the_index_is_open_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);

    let error = call(&webview, "lexical_search", json!({ "query": "договір" }))
        .expect_err("a search with no index behind it must not succeed");

    assert_eq!(error, json!("the index is not open"));
}

#[test]
fn a_search_through_the_ipc_finds_what_another_connection_wrote() {
    // The point of the shell is two connections on one file: this test writes
    // through one and searches through the other, which is the arrangement a
    // running indexing job and a typing user are in.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");
    let app = app_in(dir.path());
    let webview = main_webview(&app);

    call(&webview, "open_index", json!({})).expect("open_index was rejected");

    let chunk_id = {
        let writer = mnema_index::open(&path).unwrap();
        let doc = writer
            .insert_document(&"d".repeat(64), "text/plain", 1, SourceKind::Document)
            .unwrap();
        let page = writer.insert_page(&doc, 1, "native:txt", None).unwrap();
        let text = "акт звірки взаєморозрахунків";
        let block = writer
            .insert_block(
                page,
                &Block {
                    block_type: BlockType::Paragraph,
                    reading_order: 0,
                    language: None,
                    text: text.to_string(),
                    line_start: Some(1),
                    line_end: Some(1),
                },
            )
            .unwrap();
        writer
            .insert_chunk(
                &doc,
                0,
                text,
                &Locator {
                    spans: vec![Segment {
                        block_id: block,
                        start: 0,
                        end: text.chars().count() as u32,
                        block_start: 0,
                    }],
                    coordinate: Coordinate::None,
                },
                SourceKind::Document,
            )
            .unwrap()
    };

    let hits = call(&webview, "lexical_search", json!({ "query": "звірки" }))
        .expect("lexical_search was rejected");

    assert_eq!(
        hits,
        json!([chunk_id]),
        "the search connection did not see the row the other connection wrote"
    );
}

#[test]
fn an_unknown_command_is_rejected() {
    // The control for every test above. They ask whether a command answered
    // correctly; this asks whether answering correctly means anything, and it
    // would be the one to fail if `call` reported success for everything or
    // failure for everything.
    //
    // The behaviour it names is Tauri's, not this crate's, so no change to this
    // repository can make it red. That is what a control is.
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);

    let error = call(&webview, "open_the_pod_bay_doors", json!({}))
        .expect_err("an unregistered command answered instead of failing");

    let message = error.as_str().unwrap_or_default();
    assert!(
        message.contains("open_the_pod_bay_doors") && message.contains("not found"),
        "an unregistered command should be refused by name; the refusal was {error}"
    );
}

/// Collects what the webview would receive on a job channel.
fn job_channel() -> (Channel<JobEvent>, mpsc::Receiver<Value>) {
    let (tx, rx) = mpsc::channel();
    let channel = Channel::new(move |body| {
        let json: Value = body.deserialize().expect("the job event was not JSON");
        let _ = tx.send(json);
        Ok(())
    });
    (channel, rx)
}

/// Waits for the next `progress` event, ignoring anything else.
fn next_progress(events: &mpsc::Receiver<Value>, within: Duration) -> Option<Value> {
    let deadline = std::time::Instant::now() + within;
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        match events.recv_timeout(left) {
            Ok(event) if event["event"] == json!("progress") => return Some(event["data"].clone()),
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
    None
}

#[test]
fn a_started_job_reports_progress_and_a_cancelled_one_stops() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();

    let (channel, events) = job_channel();
    bridge::start_probe_job(state.clone(), channel).expect("the job would not start");

    let first =
        next_progress(&events, Duration::from_secs(10)).expect("no progress arrived at all");
    assert_eq!(first["done"], json!(1));
    assert_eq!(first["total"], json!(mnema_desktop::job::PROBE_UNITS));
    assert_eq!(first["skipped"], json!(0));
    // `is_u64`, not `get(..).is_some()`: a JSON `null` is also `Some`, so the
    // weaker check passes on exactly the payload it exists to reject.
    assert!(
        first["secondsLeft"].is_u64(),
        "the webview reads `secondsLeft` as a number; the payload was {first}"
    );

    bridge::cancel_job(state.clone());

    // Long enough for a report already on its way to land, and for the job
    // thread to notice the flag and exit.
    std::thread::sleep(Duration::from_millis(900));

    let mut last = None;
    while let Ok(event) = events.try_recv() {
        last = Some(event);
    }
    let last = last.expect("nothing arrived after the cancellation, not even an ending");
    assert_eq!(
        last["event"],
        json!("ended"),
        "the last thing the window heard was not an ending: {last}"
    );
    assert_eq!(last["data"]["reason"], json!("cancelled"));
    assert!(
        last["data"]["done"].as_u64().unwrap() < mnema_desktop::job::PROBE_UNITS,
        "a job cancelled after a quarter of a second claims to have finished: {last}"
    );

    assert!(
        events.recv_timeout(Duration::from_secs(1)).is_err(),
        "progress kept arriving after the job was cancelled"
    );
    assert!(
        !state.job_is_running(),
        "the job slot was never released, so nothing could be indexed again"
    );
}

#[test]
fn a_job_that_panics_still_tells_the_window_it_ended() {
    // The panic is forced from the sink, because the probe itself cannot fail.
    // That is the right place for it anyway: the sink is called from inside the
    // job, so an unwind starting there takes the same path as one starting in
    // pdfium, which is what indexing will be doing here.
    //
    // The "deliberate panic" line on stderr during this test is this test.
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();

    let (tx, events) = mpsc::channel();
    let exploded = std::sync::atomic::AtomicBool::new(false);
    let channel = Channel::new(move |body| {
        if !exploded.swap(true, std::sync::atomic::Ordering::SeqCst) {
            panic!("deliberate panic: this test forces the job to fail");
        }
        let json: Value = body.deserialize().expect("the job event was not JSON");
        let _ = tx.send(json);
        Ok(())
    });

    bridge::start_probe_job(state.clone(), channel).expect("the job would not start");

    let ending = events
        .recv_timeout(Duration::from_secs(10))
        .expect("the job panicked and the window was told nothing — Start stays disabled forever");
    assert_eq!(ending["event"], json!("ended"));
    assert_eq!(
        ending["data"]["reason"],
        json!("failed"),
        "a panic was reported to the user as something they did: {ending}"
    );
    // The panic happened *on* the first send, so the window was shown nothing.
    // `Ended::failed` promises the last count the window saw, and 1 here would
    // mean it promises the last count the job attempted — a different quantity,
    // and one the user has no way to reconcile with what is on screen.
    assert_eq!(
        ending["data"]["done"],
        json!(0),
        "the ending counted a report the window never received: {ending}"
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while state.job_is_running() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !state.job_is_running(),
        "a panicking job kept the slot, so nothing can be indexed again"
    );
}

#[test]
fn the_window_can_ask_whether_a_job_is_running() {
    // What a page that reloaded mid-job has to ask. Its channel belonged to the
    // page that started the job and is gone, so this is its only way to find out
    // whether it should be drawing one.
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);
    let state = app.state::<AppState>();

    assert_eq!(
        call(&webview, "job_status", json!({})).expect("job_status was rejected"),
        json!({ "running": false })
    );

    let (channel, events) = job_channel();
    bridge::start_probe_job(state.clone(), channel).expect("the job would not start");
    next_progress(&events, Duration::from_secs(10)).expect("the job never reported");

    assert_eq!(
        call(&webview, "job_status", json!({})).expect("job_status was rejected"),
        json!({ "running": true }),
        "a page reloading now would draw an idle window over a running job"
    );

    bridge::cancel_job(state.clone());
}

#[test]
fn a_job_started_after_a_cancelled_one_is_not_born_cancelled() {
    // The cancellation flag is process-wide and outlives the job that raised it.
    // If claiming the slot does not clear it, the next job returns on its first
    // iteration having done nothing: in the window, Start disabled, Cancel live,
    // and a bar that never moves.
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();

    let (first_channel, first_events) = job_channel();
    bridge::start_probe_job(state.clone(), first_channel).expect("the first job would not start");
    next_progress(&first_events, Duration::from_secs(10)).expect("the first job never reported");
    bridge::cancel_job(state.clone());

    // Wait for the thread to notice and release the slot.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while state.job_is_running() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !state.job_is_running(),
        "the first job never released the slot"
    );

    let (second_channel, second_events) = job_channel();
    bridge::start_probe_job(state.clone(), second_channel).expect("the second job would not start");

    // Progress specifically, not just any message: a job born cancelled still
    // sends an ending, so "something arrived" would pass here for free.
    let progress = next_progress(&second_events, Duration::from_secs(10))
        .expect("the second job never reported — it inherited the cancellation");
    assert_eq!(progress["done"], json!(1));

    bridge::cancel_job(state.clone());
}

#[test]
fn only_one_job_runs_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();

    let (first_channel, first_events) = job_channel();
    bridge::start_probe_job(state.clone(), first_channel).expect("the first job would not start");

    let (second_channel, second_events) = job_channel();
    let refusal = bridge::start_probe_job(state.clone(), second_channel)
        .expect_err("a second job was allowed to start alongside the first");
    assert_eq!(refusal.to_string(), "a job is already running");

    // Not just an error: the refused job must not have run either.
    next_progress(&first_events, Duration::from_secs(10)).expect("the first job never reported");
    assert!(
        second_events.try_recv().is_err(),
        "the refused job sent something, so it started after all"
    );

    bridge::cancel_job(state.clone());
}

#[test]
fn the_probe_job_is_reachable_through_the_ipc() {
    // The channel a real webview passes is a string of this shape. Nothing
    // receives the messages here — that is what the test above is for. What this
    // one proves is that the command is registered and that its argument arrives
    // under the name the JavaScript side sends it by.
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);

    call(
        &webview,
        "start_probe_job",
        json!({ "onProgress": "__CHANNEL__:7" }),
    )
    .expect("start_probe_job was rejected");

    let error = call(
        &webview,
        "start_probe_job",
        json!({ "on_progress": "__CHANNEL__:8" }),
    )
    .expect_err("the snake_case argument name was accepted");
    assert!(
        error.as_str().unwrap_or_default().contains("onProgress"),
        "the rejection should name the missing argument; it was {error}"
    );

    call(&webview, "cancel_job", json!({})).expect("cancel_job was rejected");
}

/// §6 of the vertical slice: the indexing job holds its own connection.
///
/// `with_index` takes a lock around the window's connection for the length of
/// the call, and an indexing job is hours of writes. On that connection every
/// search the user typed would queue behind them. This is the structural half
/// of the guarantee — that the job is handed a different connection —
/// measured here; that a read on one really does complete while a write is in
/// flight on the other is `crates/mnema-ingest/tests/slice.rs`'s.
#[test]
fn the_indexing_job_is_given_its_own_connection_not_the_windows() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(dir.path().to_path_buf());
    state.open_index().expect("the index opens");

    let job = state.open_job_index().expect("the job gets a connection");

    // A write in flight on the job's connection…
    job.conn().execute_batch("BEGIN IMMEDIATE").unwrap();
    job.insert_watched_root("/Volumes/Archive").unwrap();

    // …and the window still answers, from the last committed state. On one
    // shared connection this read would see the uncommitted row; behind one
    // shared lock it could not run at all.
    let roots = state
        .with_index(|db| {
            db.conn()
                .query_row("SELECT count(*) FROM watched_root", [], |r| {
                    r.get::<_, i64>(0)
                })
                .map_err(mnema_index::Error::from)
        })
        .expect("the window's connection answered");
    assert_eq!(
        roots, 0,
        "the window saw the job's uncommitted row, so the two share a connection"
    );

    job.conn().execute_batch("COMMIT").unwrap();

    // And the same database, which the assertion above cannot tell from a job
    // pointed at a different file — that would also show the window nothing.
    let roots = state
        .with_index(|db| {
            db.conn()
                .query_row("SELECT count(*) FROM watched_root", [], |r| {
                    r.get::<_, i64>(0)
                })
                .map_err(mnema_index::Error::from)
        })
        .expect("the window's connection answered");
    assert_eq!(
        roots, 1,
        "the job's committed row never reached the window, so the two are not \
         connections to one index"
    );
}

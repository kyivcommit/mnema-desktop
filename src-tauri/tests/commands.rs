//! The commands as the webview reaches them: through the IPC, by name, with a
//! JSON body — not as ordinary Rust functions.
//!
//! Calling the functions directly would prove they work and nothing about
//! whether they are registered, whether their arguments survive the camelCase
//! rename, or what an error looks like on the other side.

mod support;

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use mnema_core::{Block, BlockType, Coordinate, Locator, Segment, SourceKind};
use mnema_desktop::bridge;
use mnema_desktop::job::JobEvent;
use mnema_desktop::state::AppState;
use mnema_desktop::walk_job;
use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, Channel, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindow, WebviewWindowBuilder};

/// A provider address with nothing behind it. Nothing in this file calls the
/// provider, and a base that refuses instantly is how a future test that starts
/// to finds out at once rather than by reaching the real one.
const NO_PROVIDER: &str = "http://127.0.0.1:1";

/// A credential reference that cannot reach a store at all — the same trick
/// `NO_PROVIDER` uses, one line up.
///
/// Empty is not carelessness: `mnema_secrets::entry` refuses an empty
/// reference before it installs or consults any store —
/// `Error::EmptyReference`. It refuses because of the macOS keychain, where
/// an empty attribute is a wildcard matching another configuration's key.
///
/// Kept only for the two fixtures that build `AppState` directly and never
/// call `search` or a model command; `app_in` below wants a real store.
const NO_CREDENTIAL: &str = "";

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
fn app_in(dir: &std::path::Path) -> tauri::App<MockRuntime> {
    mnema_secrets::test_store::register();
    mock_builder()
        .manage(AppState::new(
            dir.to_path_buf(),
            support::worker().to_path_buf(),
            NO_PROVIDER.to_string(),
            format!("mnema-desktop-commands-test-{}", dir.display()),
        ))
        .invoke_handler(mnema_desktop::invoke_handler())
        .build(mock_context(noop_assets()))
        .expect("failed to build the mock application")
}

/// A watched folder with one file a search can find. `TempDir` is returned,
/// not its path alone: dropping it deletes the directory, and a caller has to
/// keep it alive for exactly as long as the folder needs to exist, the same
/// discipline `tempfile::tempdir()` itself already asks of every other test
/// here.
fn fixture_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp dir for the fixture folder");
    std::fs::write(
        dir.path().join("animals.txt"),
        "the quick brown fox jumps over the lazy dog",
    )
    .expect("writing the fixture file");
    dir
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
/// `the_walk_job_is_reachable_through_the_ipc` below.
fn run_walk_and_capture_ending(app: &tauri::App<MockRuntime>, root_id: i64) -> Value {
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
fn run_walk_to_completion(app: &tauri::App<MockRuntime>, root_id: i64) {
    let ending = run_walk_and_capture_ending(app, root_id);
    assert_eq!(
        ending["reason"],
        json!("completed"),
        "the walk over the fixture folder did not complete: {ending}"
    );
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

    // `start_walk_job` joins this list rather than the blocking one below:
    // unlike `start_probe_job`, it reads the root's path through
    // `with_index` before it ever spawns a thread. `start_embed_job` joins it
    // for a sharper version of the same reason — it reads the *credential
    // store* before it spawns anything, and on macOS that store can put an
    // authorisation dialog on screen and wait for a person to answer it. The
    // body below is `{"query": ""}` for every command in this loop, which is
    // neither job's shape — the point here is only which thread answers, and a
    // rejection for missing arguments answers from the same place a success
    // would.
    for cmd in ["open_index", "search", "start_walk_job", "start_embed_job"] {
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

    let error = call(&webview, "search", json!({ "query": "договір" }))
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
        let chunk = writer
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
            .unwrap();
        // The last act of an indexing job, and the fixture owes it. Without it
        // this writes a document that is still being assembled, and under D61 a
        // search does not answer with one of those. The subject here is the seam
        // between two connections on one file, not the lifecycle, so the writer
        // finishes what it started rather than stopping one statement short.
        writer
            .set_document_status(&doc, mnema_index::DocumentStatus::Indexed)
            .unwrap();
        chunk
    };

    let answer =
        call(&webview, "search", json!({ "query": "звірки" })).expect("search was rejected");
    let hits = answer["hits"]
        .as_array()
        .expect("search did not return a hits array");

    assert_eq!(
        hits.len(),
        1,
        "the search connection did not see the row the other connection wrote: {hits:?}"
    );
    assert_eq!(hits[0]["chunkId"], json!(chunk_id));
    assert!(hits[0]["text"].as_str().unwrap().contains("звірки"));
    // No `path` row was ever written for this chunk's document — only
    // `document`, `page`, `block` and `chunk` — so `citation` has nothing to
    // join a relative path from, and `None` must cross as `null`, not `""`
    // or an absent key.
    assert_eq!(hits[0]["relativePath"], json!(null));
}

/// Writes a fresh page, block and chunk onto a document id that already
/// exists — what a rebuild does after `clear_document_content`. Returns the
/// chunk id `insert_chunk` produced.
fn rebuild_one_chunk(db: &mnema_index::Db, doc: &str, text: &str) -> i64 {
    let page = db.insert_page(doc, 1, "native:txt", None).unwrap();
    let block = db
        .insert_block(
            page,
            &Block {
                block_type: BlockType::Paragraph,
                reading_order: 0,
                language: None,
                text: text.to_string(),
                line_start: None,
                line_end: None,
            },
        )
        .unwrap();
    let chunk = db
        .insert_chunk(
            doc,
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
        .unwrap();
    db.set_document_status(doc, mnema_index::DocumentStatus::Indexed)
        .unwrap();
    chunk
}

/// Finds `chunk_id` among `answer`'s hits and asserts its citation still
/// carries `word` — shared by the control search below and the raced one,
/// so both check the identical property and a panic from either names the
/// same failure shape.
fn assert_citation_carries(answer: &Value, chunk_id: i64, word: &str) {
    let hits = answer["hits"].as_array().expect("hits array");
    let hit = hits
        .iter()
        .find(|h| h["chunkId"] == json!(chunk_id))
        .unwrap_or_else(|| panic!("the target chunk did not rank into the answer: {hits:?}"));
    let text = hit["text"].as_str().unwrap();
    assert!(
        text.contains(word),
        "the chunk was matched because its text held {word:?}, but the citation read back \
         {text:?}: {hits:?}"
    );
}

/// Round-2 review, F1: nothing pinned that `bridge::search` itself wraps
/// its own work in `Db::read_snapshot`, only that the mechanism works
/// (`crates/mnema-search/tests/snapshot_boundary.rs`) — removing that call
/// left the whole suite green. A real second connection rebuilds the
/// matched document — reused chunk id, a different marker word — while
/// the real `search` IPC command runs. Round-3 re-review measured this
/// test's own catch rate under both build profiles and found it profile-
/// dependent at first; the fixture-size and control comments below cover
/// what changed and why. Full repeat-count numbers in the private report.
#[test]
fn a_rebuild_racing_the_ipc_search_does_not_reach_its_citation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");
    let app = app_in(dir.path());
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).expect("open_index was rejected");
    call(
        &webview,
        "set_search_arms",
        json!({ "text": true, "content": false }),
    )
    .expect("set_search_arms was rejected");

    // Fillers first, target last: `chunk.id` is `INTEGER PRIMARY KEY` without
    // `AUTOINCREMENT` (the defect this whole branch closes), and SQLite only
    // reuses the *highest* freed id — `citation.rs:1301-1325`'s own fixture
    // relies on the identical ordering. Inserted any other way, deleting the
    // target frees an id below the corpus max and the rebuild never reuses
    // it. 6000, not 1500: at 1500 this was 25/25 red under debug but 25/25
    // green under `--release` (the command finished inside the writer's
    // head start once optimised) — widened, now 25/25 both ways, both
    // profiles.
    let writer = mnema_index::open(&path).unwrap();
    for i in 0..6000 {
        let filler_doc = format!("{i:064x}");
        writer
            .insert_document(&filler_doc, "text/plain", 1, SourceKind::Document)
            .unwrap();
        rebuild_one_chunk(&writer, &filler_doc, "спільний термін заповнювач");
    }
    let target_doc = "9".repeat(64);
    writer
        .insert_document(&target_doc, "text/plain", 1, SourceKind::Document)
        .unwrap();
    let unique_word = "маркер";
    // Doubled term frequency for both query terms: inserted last, the
    // target would tie-break to the *bottom* of `ORDER BY rank,
    // chunk_fts.rowid` against 6000 once-each fillers, and never reach
    // `SEARCH_LIMIT`. bm25 rewards term frequency enough to overcome that on
    // its own — measured, not assumed, by the fixture below actually
    // finding it.
    let original_text = format!("спільний спільний термін термін {unique_word}");
    let target_chunk = rebuild_one_chunk(&writer, &target_doc, &original_text);

    // Control, before the writer thread exists to race against: round-2's
    // only observed failure in debug was "the target chunk did not rank
    // into the answer", which an unrelated ranking regression would also
    // produce. This proves the fixture and ranking work on their own, so a
    // failure below can only mean the race reached the citation.
    let control = call(&webview, "search", json!({ "query": "спільний термін" }))
        .expect("control search was rejected");
    assert_citation_carries(&control, target_chunk, unique_word);

    let writer_handle = std::thread::spawn(move || {
        // A short, deliberately generous head start: `matching`'s sort over
        // 6000+ tied rows (see the doc comment above) takes low
        // milliseconds under a debug build and low hundreds of microseconds
        // optimised — both comfortably longer than this, measured by the
        // repeat counts the doc comment above states.
        std::thread::sleep(Duration::from_millis(2));
        writer
            .clear_document_content(&target_doc)
            .expect("clear the target document");
        // The same doubled term frequency as `original_text` above, so the
        // rebuild lands inside the race whether it beats the lexical match
        // query or not: either way the chunk still ranks and is found, and
        // only `unique_word` tells the two texts apart at the citation the
        // search answer carries.
        rebuild_one_chunk(
            &writer,
            &target_doc,
            "спільний спільний термін термін замінник",
        );
    });

    let answer = call(&webview, "search", json!({ "query": "спільний термін" }))
        .expect("search was rejected");
    writer_handle.join().expect("the writer thread panicked");

    assert_citation_carries(&answer, target_chunk, unique_word);
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
    // Gap 1 from the task-12 review: `reason: "failed"` alone cannot tell a
    // missing worker binary from a broken pool from a panic. This is the
    // panic's own text, carried across rather than dropped.
    assert_eq!(
        ending["data"]["message"],
        json!("deliberate panic: this test forces the job to fail"),
        "the panic's own message did not reach the window: {ending}"
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
    let state = AppState::new(
        dir.path().to_path_buf(),
        support::worker().to_path_buf(),
        NO_PROVIDER.to_string(),
        NO_CREDENTIAL.to_string(),
    );
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

/// The window needs a citation, not a chunk id. `mnema-index` already
/// re-exports `Citation` and it is `Serialize` (`write.rs:11`), so this
/// crosses the seam without touching the dependency graph — the seam was
/// simply never crossed.
#[test]
fn search_returns_citations_not_ids() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);

    call(&webview, "open_index", json!({})).expect("open_index was rejected");

    let fixture = fixture_dir();
    let root = call(
        &webview,
        "add_watched_folder",
        json!({ "path": fixture.path().display().to_string() }),
    )
    .expect("add_watched_folder was rejected")
    .as_i64()
    .expect("add_watched_folder did not return an id");

    run_walk_to_completion(&app, root);

    let answer = call(&webview, "search", json!({ "query": "fox" })).expect("search was rejected");
    let hits = answer["hits"]
        .as_array()
        .expect("search did not return a hits array");

    assert!(!hits.is_empty());
    assert!(hits[0]["text"].as_str().unwrap().contains("fox"));
    assert!(hits[0]["relativePath"].is_string());
    // A real count, not a placeholder: `matched: chunks.len()` in
    // `bridge.rs`'s `From<TextArm>` mutated to `matched: 0` must fail this
    // specific assertion, not merely the non-empty check above.
    assert_eq!(
        answer["text"]["matched"],
        json!(1),
        "the one-chunk fixture should be counted, not zeroed: {answer}"
    );
}

/// D106: absent means on, for both arms. `mnema-index`'s own
/// `an_index_that_never_saw_the_toggles_has_both_arms_on` pins that the raw
/// row is absent; this pins what `search` does with that absence, the seam a
/// person actually sees. `content`'s only route past `Off` without a key is
/// `NoKey`, so seeing that discriminant is what tells "the arm ran and found
/// nothing to work with" apart from "the arm was skipped".
#[test]
fn a_fresh_index_with_no_arm_written_answers_with_both_arms_on() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).expect("open_index was rejected");

    let answer = call(&webview, "search", json!({ "query": "fox" })).expect("search was rejected");

    assert_eq!(
        answer["text"]["kind"],
        json!("answered"),
        "an absent text-arm row must run the arm, not skip it: {answer}"
    );
    assert_eq!(
        answer["content"]["kind"],
        json!("noKey"),
        "an absent content-arm row must run the arm — `noKey` proves it tried \
         and only then found no key, `off` would mean it never tried: {answer}"
    );
}

/// `set_search_arms` reached through the real IPC path, the same shape
/// `every_model_command_the_window_calls_is_registered`'s own doc warns a
/// `pub` command can silently miss — and exercises `arm_is_on`'s `"off"`
/// branch, for each key in turn. Never both at once:
/// `set_search_arms_refuses_to_turn_off_both_arms`, right below, is what
/// that combination means now.
#[test]
fn set_search_arms_is_reachable_through_the_ipc_and_off_means_off() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).expect("open_index was rejected");

    call(
        &webview,
        "set_search_arms",
        json!({ "text": false, "content": true }),
    )
    .expect("set_search_arms was rejected");
    let answer = call(&webview, "search", json!({ "query": "fox" })).expect("search was rejected");
    assert_eq!(answer["text"]["kind"], json!("off"));

    call(
        &webview,
        "set_search_arms",
        json!({ "text": true, "content": false }),
    )
    .expect("set_search_arms was rejected");
    let answer = call(&webview, "search", json!({ "query": "fox" })).expect("search was rejected");
    assert_eq!(answer["content"]["kind"], json!("off"));
}

/// D106: two independent toggles, and at least one is always on. Nothing
/// stopped a caller from writing both meta rows `"off"` before this —
/// `arm_is_on` would then read a row that contradicts the sentence nothing
/// here rechecks.
#[test]
fn set_search_arms_refuses_to_turn_off_both_arms() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).expect("open_index was rejected");

    let error = call(
        &webview,
        "set_search_arms",
        json!({ "text": false, "content": false }),
    )
    .expect_err("turning off both search arms was accepted");

    assert_eq!(error, json!("at least one search arm must stay on"));
}

/// `model_settings` reads the saved choice back from the same two meta rows
/// `search` runs against (`arm_is_on`), so a checkbox drawn from
/// `model_settings` and the arm a search actually runs cannot disagree —
/// task 26's review, Important 1: a window that always drew both arms on
/// contradicted its own "is off" sentence the moment either was saved off.
#[test]
fn model_settings_reflects_the_saved_arm_choice() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).expect("open_index was rejected");

    let before = call(&webview, "model_settings", json!({})).expect("model_settings was rejected");
    assert_eq!(before["index"]["searchTextArm"], json!(true));
    assert_eq!(before["index"]["searchContentArm"], json!(true));

    call(
        &webview,
        "set_search_arms",
        json!({ "text": false, "content": true }),
    )
    .expect("set_search_arms was rejected");

    let after = call(&webview, "model_settings", json!({})).expect("model_settings was rejected");
    assert_eq!(after["index"]["searchTextArm"], json!(false));
    assert_eq!(after["index"]["searchContentArm"], json!(true));
}

/// `search` must not ask for a key it will not use. Built with
/// `NO_CREDENTIAL` rather than `app_in`'s reachable store: a `search` that
/// still called `crate::models::key` unconditionally would fail here before
/// ever reading `arms.content`, which is exactly the shape this pins.
#[test]
fn a_text_only_search_does_not_touch_a_credential_store_it_does_not_need() {
    let dir = tempfile::tempdir().unwrap();
    let app = mock_builder()
        .manage(AppState::new(
            dir.path().to_path_buf(),
            support::worker().to_path_buf(),
            NO_PROVIDER.to_string(),
            NO_CREDENTIAL.to_string(),
        ))
        .invoke_handler(mnema_desktop::invoke_handler())
        .build(mock_context(noop_assets()))
        .expect("failed to build the mock application");
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).expect("open_index was rejected");

    call(
        &webview,
        "set_search_arms",
        json!({ "text": true, "content": false }),
    )
    .expect("set_search_arms was rejected");

    let answer = call(&webview, "search", json!({ "query": "fox" }))
        .expect("a text-only search reached a credential store it does not need");

    assert_eq!(answer["text"]["kind"], json!("answered"));
    assert_eq!(answer["content"]["kind"], json!("off"));
}

/// I1, final-round review: a credential store that will not answer at all —
/// not merely "no key" — must not take the whole search down with it. Built
/// with `NO_CREDENTIAL`, the same unreachable store as the test above, but
/// with the content arm on, so `crate::models::key` fails with
/// [`mnema_desktop::error::Error::Secrets`] rather than [`Error::NoKey`].
/// Before this test the `Err(e) => return Err(e)` arm in `search` answered
/// the whole command with that error, so the text arm — which needs no
/// credential store at all — never got to answer either.
#[test]
fn a_broken_credential_store_does_not_take_the_text_arm_down_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let app = mock_builder()
        .manage(AppState::new(
            dir.path().to_path_buf(),
            support::worker().to_path_buf(),
            NO_PROVIDER.to_string(),
            NO_CREDENTIAL.to_string(),
        ))
        .invoke_handler(mnema_desktop::invoke_handler())
        .build(mock_context(noop_assets()))
        .expect("failed to build the mock application");
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).expect("open_index was rejected");

    let fixture = fixture_dir();
    let root = call(
        &webview,
        "add_watched_folder",
        json!({ "path": fixture.path().display().to_string() }),
    )
    .expect("add_watched_folder was rejected")
    .as_i64()
    .expect("add_watched_folder did not return an id");
    run_walk_to_completion(&app, root);

    // Both arms on (the default) — the content arm is what hits the broken
    // store; the text arm is the one that must survive it.
    let answer = call(&webview, "search", json!({ "query": "fox" }))
        .expect("a broken credential store must not fail the whole search");

    assert!(
        !answer["hits"].as_array().unwrap().is_empty(),
        "the text arm did not answer even though it needs no credential store: {answer}"
    );
    assert_eq!(answer["text"]["kind"], json!("answered"));
    assert_eq!(
        answer["content"]["kind"],
        json!("failed"),
        "a real store failure must be reported as failed, not silently read as \
         no key: {answer}"
    );
}

/// The channel a real webview passes is a string of this shape. Nothing
/// receives the messages here — `run_walk_to_completion` above is what
/// proves the walk itself works, by calling the command function directly so
/// its `Channel` has a real callback behind it. What this proves is narrower
/// and just as necessary: that `start_walk_job` is in `invoke_handler!` at
/// all, and that its arguments arrive under the name the JavaScript side
/// sends them by. Neither is implied by the function existing and working
/// when called directly, the same reason `the_probe_job_is_reachable_
/// through_the_ipc` exists alongside the tests that call `start_probe_job`
/// straight from Rust.
#[test]
fn the_walk_job_is_reachable_through_the_ipc() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);

    call(&webview, "open_index", json!({})).expect("open_index was rejected");
    let fixture = fixture_dir();
    let root = call(
        &webview,
        "add_watched_folder",
        json!({ "path": fixture.path().display().to_string() }),
    )
    .expect("add_watched_folder was rejected")
    .as_i64()
    .expect("add_watched_folder did not return an id");

    call(
        &webview,
        "start_walk_job",
        json!({ "rootId": root, "onProgress": "__CHANNEL__:9" }),
    )
    .expect("start_walk_job was rejected");

    let error = call(
        &webview,
        "start_walk_job",
        json!({ "root_id": root, "on_progress": "__CHANNEL__:10" }),
    )
    .expect_err("the snake_case argument names were accepted");
    assert!(
        error.as_str().unwrap_or_default().contains("rootId"),
        "the rejection should name the missing argument; it was {error}"
    );

    // The job started above is real and running over a real (tiny) fixture.
    // Letting it finish before `app` and the temp dirs drop keeps this test
    // from racing its own teardown.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while app.state::<AppState>().job_is_running() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !app.state::<AppState>().job_is_running(),
        "the walk job never released the slot"
    );
}

/// The same narrow question for the embedding job: is it in `invoke_handler!`
/// at all, and does its one argument arrive under the name JavaScript sends it
/// by.
///
/// It is asked here rather than in `tests/model_commands.rs`, where the job's
/// behaviour is tested, because that file calls the command function directly
/// and would stay green through exactly the mistake this catches — a `pub`
/// command that compiles and is simply missing from a macro's list, which
/// warns nowhere and fails only on a screen no gate runs.
///
/// **The call is expected to fail**, and that is what proves it was reached:
/// `app_in`'s store has no key in it, so the command refuses for a reason of
/// its own — `Error::NoKey` — rather than being refused by name before it
/// runs. Nothing is started and no slot is taken, which is why this test
/// needs no teardown of its own.
#[test]
fn the_embed_job_is_reachable_through_the_ipc() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);

    let refusal = call(
        &webview,
        "start_embed_job",
        json!({ "onProgress": "__CHANNEL__:11" }),
    )
    .expect_err("this application has no key entered, so the job cannot start");
    assert_ne!(
        error_text(&refusal),
        not_registered("start_embed_job"),
        "the command the window presses Embed to reach is not in `invoke_handler!`"
    );

    let renamed = call(
        &webview,
        "start_embed_job",
        json!({ "on_progress": "__CHANNEL__:12" }),
    )
    .expect_err("the snake_case argument name was accepted");
    assert!(
        error_text(&renamed).contains("onProgress"),
        "the rejection should name the missing argument; it was {renamed}"
    );

    assert!(
        !app.state::<AppState>().job_is_running(),
        "a call that was refused before it started anything left the job slot taken"
    );
}

/// `remove_watched_folder` is not on this task's list for completeness: it
/// is the first thing that reaches `Db::delete_watched_root` from outside a
/// Rust test, over the full seam — add, walk, remove, search — rather than
/// against a database built by hand. §7.1.1 named the gap that function
/// closes: `path` rows fall away with their root through the schema's own
/// foreign key, but nothing cascaded onward to `document` on its own, which
/// would otherwise keep answering `search` for a folder that no longer
/// exists.
#[test]
fn removing_a_watched_folder_takes_its_documents_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);

    call(&webview, "open_index", json!({})).expect("open_index was rejected");
    let fixture = fixture_dir();
    let root = call(
        &webview,
        "add_watched_folder",
        json!({ "path": fixture.path().display().to_string() }),
    )
    .expect("add_watched_folder was rejected")
    .as_i64()
    .expect("add_watched_folder did not return an id");

    run_walk_to_completion(&app, root);
    let before = call(&webview, "search", json!({ "query": "fox" })).expect("search was rejected");
    assert!(
        !before["hits"].as_array().unwrap().is_empty(),
        "the fixture was never indexed, so removing it proves nothing"
    );

    let removed = call(&webview, "remove_watched_folder", json!({ "rootId": root }))
        .expect("remove_watched_folder was rejected");
    assert_eq!(
        removed,
        json!(1),
        "the fixture's one document was not removed with the only root that named it"
    );

    let after = call(&webview, "search", json!({ "query": "fox" })).expect("search was rejected");
    assert_eq!(
        after["hits"],
        json!([]),
        "a document survived the folder that owned it being removed"
    );
}

/// A dangling symlink is exactly the shape `PreSkipRule::NotAFile` names —
/// `crates/mnema-ingest/src/walk.rs`'s own match arm for it says so in
/// words: "a symlink, a dangling symlink, a FIFO, a socket or a device." It
/// is journalled and the walk continues, which this test leans on twice:
/// once for `skips` to have a row to return, and once for
/// `run_walk_to_completion`'s own assertion that the walk still completes.
#[cfg(unix)]
#[test]
fn skips_reports_what_the_walk_could_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);

    call(&webview, "open_index", json!({})).expect("open_index was rejected");

    let fixture = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(
        fixture.path().join("nowhere"),
        fixture.path().join("broken"),
    )
    .expect("creating the dangling symlink");

    let root = call(
        &webview,
        "add_watched_folder",
        json!({ "path": fixture.path().display().to_string() }),
    )
    .expect("add_watched_folder was rejected")
    .as_i64()
    .expect("add_watched_folder did not return an id");

    run_walk_to_completion(&app, root);

    let skips = call(&webview, "skips", json!({ "rootId": root })).expect("skips was rejected");
    let skips = skips.as_array().expect("skips did not return an array");
    assert_eq!(
        skips.len(),
        1,
        "the dangling symlink was not journalled: {skips:?}"
    );
    assert_eq!(skips[0]["relativePath"], json!("broken"));
}

/// The critical case a review round found: `stopped == Completed` does not
/// mean phase 3 ran. An unreadable subdirectory leaves the walk `Completed`
/// — phase 2 finished everything phase 1 could hand it — but
/// `WalkReport::complete` is `false`, and reconciliation refuses to run on
/// an incomplete walk (`mnema-ingest/src/walk.rs`'s own gate, pinned there by
/// `an_incomplete_walk_deletes_nothing`). Before this round `ended_from_
/// report` never read `complete` at all, so this exact shape reached the
/// window as `{reason: "completed", ...}` — byte-identical to a walk that
/// saw everything, with no way to tell the two apart.
#[cfg(unix)]
#[test]
fn an_unreadable_subdirectory_tells_the_window_reconciliation_did_not_run() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).expect("open_index was rejected");

    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join("kept.txt"), "kept").unwrap();
    let locked = fixture.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("inside.txt"), "secret").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
        .expect("locking the subdirectory");

    // Root reads through any permission bits — the same guard
    // `crates/mnema-ingest/tests/walk.rs::complete_is_false_when_a_
    // subdirectory_could_not_be_read` uses for the identical shape, so this
    // test does not fail for a different reason in a container that runs as
    // root.
    if std::fs::read_dir(&locked).is_ok() {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!(
            "skipped an_unreadable_subdirectory_tells_the_window_reconciliation_did_not_run: \
             running as root, chmod 000 has no effect"
        );
        return;
    }

    let root = call(
        &webview,
        "add_watched_folder",
        json!({ "path": fixture.path().display().to_string() }),
    )
    .expect("add_watched_folder was rejected")
    .as_i64()
    .expect("add_watched_folder did not return an id");

    let ending = run_walk_and_capture_ending(&app, root);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
        .expect("unlocking the subdirectory so the temp dir can be cleaned up");

    assert_eq!(
        ending["reason"],
        json!("completed"),
        "the walk did not even stop cleanly, so this proves nothing about `complete`: {ending}"
    );
    assert_eq!(
        ending["complete"],
        json!(false),
        "an unreadable subdirectory must not report as a walk that saw everything: {ending}"
    );
}

/// The seam the throttle has to survive, not only the predicate that
/// decides it: `job::progress_is_due` is unit-tested directly in `job.rs`,
/// but nothing before this test proved a *real* walk actually consults it
/// rather than forwarding every `WalkProgress` `walk_root` hands it. A
/// one-file fixture cannot see the difference — throttled or not, one file
/// produces at most two events. Thirty can: an unthrottled walk sends one
/// progress event per file handled in phase 2 (plus one before the loop, for
/// phase-1 refusals), so thirty files with none refused make thirty-one:
/// `job::REPORT_INTERVAL`'s own doc comment names exactly this shape. A
/// throttled walk instead sends however many 250 ms windows the whole walk
/// actually spans — bounded by wall-clock time, not by file count, so it
/// stays small on this machine regardless of how many files there are.
#[test]
fn progress_events_are_throttled_and_the_last_one_is_exact() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).expect("open_index was rejected");

    let fixture = tempfile::tempdir().unwrap();
    for i in 0..30 {
        std::fs::write(
            fixture.path().join(format!("file-{i}.txt")),
            format!("file number {i}"),
        )
        .expect("writing a fixture file");
    }
    let root = call(
        &webview,
        "add_watched_folder",
        json!({ "path": fixture.path().display().to_string() }),
    )
    .expect("add_watched_folder was rejected")
    .as_i64()
    .expect("add_watched_folder did not return an id");

    let state = app.state::<AppState>();
    let (channel, events) = job_channel();
    walk_job::start_walk_job(state.clone(), root, channel).expect("the walk would not start");

    let mut progress_events = Vec::new();
    let ending = loop {
        match events.recv_timeout(Duration::from_secs(30)) {
            Ok(event) if event["event"] == json!("progress") => {
                progress_events.push(event["data"].clone());
            }
            Ok(event) if event["event"] == json!("ended") => break event["data"].clone(),
            Ok(_) => continue,
            Err(_) => panic!("the walk never told the window it ended"),
        }
    };

    assert_eq!(
        ending["reason"],
        json!("completed"),
        "the walk over thirty files did not complete: {ending}"
    );
    // Both directions, because the upper bound alone is satisfied by zero and
    // a review measured exactly that: made to send nothing at all, this test
    // passed — `len() < 15` held and the exactness check below skipped itself
    // through its own `if let`. A bar that never moves is not a throttle
    // working well, it is a progress channel that is broken.
    assert!(
        !progress_events.is_empty(),
        "thirty files produced no progress events at all — the bar would never move"
    );
    assert!(
        progress_events.len() < 15,
        "thirty files produced {} progress events — throttling did not \
         meaningfully reduce anything: {progress_events:?}",
        progress_events.len()
    );
    // The exception `job::progress_is_due` always makes: the report that
    // reaches `total` is sent regardless of timing, because a bar that
    // stops one file short of the end looks like a hang. The last event must
    // already show the true final count — not a stale one the throttle
    // happened to let through earlier and then withheld the correction for.
    let last = progress_events
        .last()
        .expect("the emptiness assertion above already established there is one");
    assert_eq!(
        last["done"], ending["done"],
        "the last progress event before Ended did not show the true count: {last}"
    );
}

/// `JobSlot::drop` clears `AppState::running`, and `ui/main.js` re-enables
/// Start inside the handler that receives `Ended` — so the window's own
/// contract is that a second walk can start the instant the first says it
/// is over. Before the slot was dropped ahead of the send, this raced: the
/// slot was still held for however long remained of the spawned thread's
/// body, and a second `start_walk_job` issued in that gap was refused with
/// `a job is already running`, even though the first walk had just been
/// reported finished.
#[test]
fn a_second_walk_can_start_the_instant_the_first_says_it_ended() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");

    let fixture = fixture_dir();
    let root = state
        .with_index(|db| db.insert_watched_root(&fixture.path().display().to_string()))
        .expect("insert_watched_root failed");

    let (first_channel, first_events) = job_channel();
    walk_job::start_walk_job(state.clone(), root, first_channel)
        .expect("the first walk would not start");

    loop {
        match first_events.recv_timeout(Duration::from_secs(20)) {
            Ok(event) if event["event"] == json!("ended") => break,
            Ok(_) => continue,
            Err(_) => panic!("the first walk never told the window it ended"),
        }
    }

    let (second_channel, _second_events) = job_channel();
    walk_job::start_walk_job(state.clone(), root, second_channel)
        .expect("a second walk was refused the instant the first said it ended");
}

/// Gap 1 from the task-12 review, exercised through a real walk rather than
/// only through `Ended::failed` itself. The missing path below is contrived
/// now, not the shape a shipped build takes — `bundle.externalBin` stages a
/// worker into a packaged build today, and `scripts/verify-bundle.sh` is what
/// keeps it there — but the failure this test drives is still real: a bundle
/// that a future change fails to carry the worker into hits this exact code.
/// Before `Ended.message` existed, it reached the window as the single word
/// `"failed"`, indistinguishable from a broken pool or a panic.
#[test]
fn a_missing_worker_binary_reports_why_in_the_message() {
    let dir = tempfile::tempdir().unwrap();
    let app = mock_builder()
        .manage(AppState::new(
            dir.path().to_path_buf(),
            PathBuf::from("/nonexistent/mnema-extract-worker"),
            NO_PROVIDER.to_string(),
            NO_CREDENTIAL.to_string(),
        ))
        .invoke_handler(mnema_desktop::invoke_handler())
        .build(mock_context(noop_assets()))
        .expect("failed to build the mock application");
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");

    let fixture = fixture_dir();
    let root = state
        .with_index(|db| db.insert_watched_root(&fixture.path().display().to_string()))
        .expect("insert_watched_root failed");

    let (channel, events) = job_channel();
    walk_job::start_walk_job(state.clone(), root, channel).expect("the walk would not start");

    let ending = loop {
        match events.recv_timeout(Duration::from_secs(20)) {
            Ok(event) if event["event"] == json!("ended") => break event["data"].clone(),
            Ok(_) => continue,
            Err(_) => panic!("the walk never told the window it ended"),
        }
    };

    assert_eq!(
        ending["reason"],
        json!("failed"),
        "a missing worker binary must stop the walk, not be treated as a per-file skip: {ending}"
    );
    let message = ending["message"]
        .as_str()
        .expect("a failed walk must carry a message the window can render");
    assert!(
        message.contains("nonexistent/mnema-extract-worker"),
        "the message does not name the worker path that could not be started: {message}"
    );
}

/// The capability granting `dialog:allow-open` (`src-tauri/capabilities/
/// default.json`), exercised at the ACL layer rather than by actually
/// opening a dialog: a real `open` call blocks on user interaction, which a
/// test must never do. `mock_context(noop_assets())` cannot answer this
/// question — it hands every app `Resolved::default()`, an empty ACL,
/// regardless of what capability files exist on disk (see that function's
/// own source). `tauri::generate_context!()` is what actually reads
/// `tauri.conf.json` and `capabilities/*.json`, the way `lib.rs::run()`
/// does, so this is the one place in this file that uses it — the pairing
/// with `mock_builder()` below is `tauri::test::mock_builder`'s own
/// documented example, not a novel combination.
fn app_with_real_capabilities() -> tauri::App<MockRuntime> {
    mock_builder()
        .invoke_handler(mnema_desktop::invoke_handler())
        .plugin(tauri_plugin_dialog::init())
        .build(tauri::generate_context!())
        .expect("failed to build the mock application")
}

/// A payload the ACL must let through before it fails for an unrelated
/// reason. `directory` is a `bool` on `OpenDialogOptions`
/// (`tauri-plugin-dialog`'s own `src/commands.rs`), so a string here fails to
/// deserialize. If the capability did not grant `dialog:allow-open`, this
/// call would be refused before argument parsing ever ran, with a message
/// naming the plugin rather than the field — which is exactly the
/// distinction the two tests below turn on, and why neither one has to
/// actually open a dialog to prove its point.
fn malformed_open_dialog_args() -> Value {
    json!({ "options": { "directory": "not a boolean" } })
}

/// D48: the ACL classifies a request by its origin, and Windows serves the
/// webview from a different one than macOS does — `local_origin()`'s own doc
/// comment has the measured history of what hardcoding the wrong constant
/// broke last time. `call()` already routes through it for every command in
/// this file; this test is not exempt just because the command it reaches
/// belongs to a plugin rather than to this crate.
#[test]
fn the_main_window_may_reach_the_folder_picker() {
    let app = app_with_real_capabilities();
    let webview = main_webview(&app);

    let error = call(&webview, "plugin:dialog|open", malformed_open_dialog_args())
        .expect_err("a non-boolean `directory` must not deserialize into `OpenDialogOptions`");
    let message = error.as_str().unwrap_or_default();
    // `!message.contains("not allowed")` is not enough: measured directly,
    // removing `.plugin(tauri_plugin_dialog::init())` from
    // `app_with_real_capabilities` changes the message to a "no such plugin"
    // shape that also happens not to contain "not allowed" — so that weaker
    // assertion passed for a picker that was not reachable at all.
    // `invalid type … expected a boolean` is the one shape that can only be
    // produced by `OpenDialogOptions::deserialize` actually running, which
    // only happens once the ACL has let the call through *and* the plugin is
    // registered to handle it.
    assert!(
        message.contains("invalid type") && message.contains("expected a boolean"),
        "the `main` window did not reach `OpenDialogOptions` deserialization — either the ACL \
         refused it, or the dialog plugin was never registered to answer it: {message}"
    );
}

/// The counterweight to the test above: without it, a capability that
/// granted `dialog:allow-open` to every window regardless of `windows: [
/// "main"]` would pass the positive test for the same reason a capability
/// scoped correctly would, and nothing here would tell the two apart.
#[test]
fn a_window_the_capability_does_not_name_may_not_reach_the_folder_picker() {
    let app = app_with_real_capabilities();
    let webview = WebviewWindowBuilder::new(&app, "other", Default::default())
        .build()
        .expect("failed to build the second mock webview");

    let error = call(&webview, "plugin:dialog|open", malformed_open_dialog_args())
        .expect_err("a window outside the capability's `windows` list reached the dialog plugin");
    let message = error.as_str().unwrap_or_default();
    assert!(
        message.contains("not allowed"),
        "a window the capability does not name should be refused by the ACL, not by argument \
         parsing: {message}"
    );
}

/// What Tauri answers a command name it has no entry for — the whole message,
/// which is what makes a comparison against it discriminate.
///
/// `format!("Command {command} not found")`, built in
/// `tauri-2.11.5/src/webview/mod.rs`. Measured 2026-08-09 by asking for
/// `set_embeding_model`; the control in the test below is what keeps it measured
/// rather than remembered.
fn not_registered(cmd: &str) -> String {
    format!("Command {cmd} not found")
}

/// The lead of Tauri's argument-binding refusal, ``invalid args `{1}` for
/// command `{0}`: {2}`` (`tauri-2.11.5/src/error.rs`). Measured by the second
/// control below, for the same reason as the first.
const INVALID_ARGS: &str = "invalid args";

/// What the webview would receive, insisting it is a string.
///
/// `call`'s own doc two hundred lines up states that for this shell an `Err` is
/// **always** a string. `unwrap_or_default()` here would turn a violation of
/// that into `""`, and every assertion of absence downstream would then pass
/// while checking nothing.
fn error_text(rejected: &Value) -> String {
    rejected
        .as_str()
        .expect("this shell's rejections are strings; `call`'s own doc says so")
        .to_string()
}

/// The eight model commands, enumerated, asked of the list that decides what
/// the window can call.
///
/// A count is a definition, and this file is where the definition is checkable:
/// `generate_handler!` is a macro, so a command function that exists, compiles,
/// is `pub`, and is simply missing from that list produces no warning anywhere
/// — and the window's call fails at run time on a screen nobody runs in a gate.
/// The tests in `model_commands.rs` call these functions directly and would all
/// stay green through exactly that mistake.
///
/// **Each is asked with the arguments it declares**, so a parameter renamed on
/// one side alone fails here too; the second control below is what says those
/// arguments are being bound at all rather than ignored.
///
/// **Most of these calls fail, and that is the point rather than a problem.**
/// This application has no provider behind it (`NO_PROVIDER`), no index open,
/// and no key entered in `app_in`'s store; the question here is only whether
/// the command was reached, and being reached is exactly what lets it fail
/// for a reason of its own.
///
/// `model_settings` is the exception and answers `Ok` even here, because
/// every state of the store and of the index is a state it draws — no key
/// entered arrives as `KeyState::Absent` rather than as a rejection. `Ok`
/// proves registration at least as well as a specific failure does: an
/// unregistered command cannot return one, it is refused by name before it
/// runs. This paragraph said "every call is expected to fail" for one commit
/// after that stopped being true.
#[test]
fn every_model_command_the_window_calls_is_registered() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);

    // Control one: a name nobody registered, one letter away from a real
    // command. Without it, the assertion below is a search for a string that may
    // no longer be the one Tauri uses, and it would pass for every command in
    // the list including the absent ones.
    //
    // The whole message and not a substring of it. `contains("not found")` reads
    // as the same check and is not: it also fires on any *other* refusal that
    // happens to use those two words, and then reports "X is not in
    // invoke_handler" about a command that is — red with the wrong cause named,
    // which is worse than green.
    let missing = call(&webview, "set_embeding_model", json!({ "model": "x" }))
        .expect_err("a command nobody registered was accepted");
    assert_eq!(
        error_text(&missing),
        not_registered("set_embeding_model"),
        "an unregistered command no longer answers with that sentence, so the loop below is \
         comparing against the wrong string and would pass for a command that is not \
         registered either"
    );

    // Control two: the arguments are bound, not ignored, AND the string the loop
    // asserts the absence of is a string this build has seen.
    //
    // Both halves are needed and the first version had neither. `contains("model")`
    // is satisfied by the *command name* — Tauri's format is
    // ``invalid args `{1}` for command `{0}`: {2}`` (`tauri-2.11.5/src/error.rs`),
    // and all eight command names contain `model`. And nothing measured
    // "invalid args" at all, so the day Tauri rephrases it, the loop's second
    // assertion becomes permanently vacuous without going red — the same
    // absence-satisfied-by-nothing that control one exists to stop.
    let unbound = call(&webview, "set_rerank_model", json!({}))
        .expect_err("a command was accepted without the argument it declares");
    let unbound = error_text(&unbound);
    // This control is itself reached through the handler, so it has to say which
    // failure it is looking at before it says anything about arguments.
    // Measured: dropping `set_rerank_model` from `invoke_handler` makes the two
    // assertions below fail with "Tauri no longer says `invalid args`" — red
    // with the wrong cause named, about a command that is simply absent.
    assert_ne!(
        unbound,
        not_registered("set_rerank_model"),
        "this control asks about argument binding and its own command is not registered, so \
         it can say nothing about arguments"
    );
    assert!(
        unbound.contains(INVALID_ARGS),
        "a missing argument no longer answers with `{INVALID_ARGS}`, so the loop below \
         asserts the absence of a string nothing produces: {unbound}"
    );
    assert!(
        unbound.contains("`model`"),
        "the rejection should name the missing argument, in the backticks Tauri puts round \
         it — the bare word is in every one of these command names: {unbound}"
    );

    for (cmd, args) in [
        ("provider_models", json!({ "role": "chat" })),
        ("key_present", json!({})),
        ("set_key", json!({ "key": "test-key-not-a-real-one" })),
        ("forget_key", json!({})),
        // Both spellings of `existingVectors`, because the window sends both and
        // a value this build does not recognise is rejected as `invalid args` —
        // which is the assertion below. Two entries and not one: `keep` alone
        // would leave the destructive spelling unpinned, and a rename of it
        // reaches a person as a change that will not happen rather than as a
        // build that stopped.
        (
            "set_embedding_model",
            json!({ "model": "baai/bge-m3", "existingVectors": "keep" }),
        ),
        (
            "set_embedding_model",
            json!({ "model": "baai/bge-m3", "existingVectors": "discard" }),
        ),
        (
            "set_rerank_model",
            json!({ "model": "baai/bge-reranker-v2-m3" }),
        ),
        (
            "set_chat_model",
            json!({ "model": "anthropic/claude-opus-4" }),
        ),
        ("model_settings", json!({})),
    ] {
        let message = match call(&webview, cmd, args) {
            Ok(_) => continue,
            Err(e) => error_text(&e),
        };
        assert_ne!(
            message,
            not_registered(cmd),
            "{cmd} is not in `invoke_handler`, so the window cannot call it however well \
             the function itself works"
        );
        assert!(
            !message.contains(INVALID_ARGS),
            "{cmd} was reached and would not take the arguments the window sends it: \
             {message}"
        );
    }
}

/// A model change that says nothing about the embeddings already there is
/// refused before the command runs.
///
/// **`ExistingVectors`'s own doc claims this and nothing held it.** The test
/// above sends both spellings *present*, so it is satisfied by a build in which
/// the field is optional — adding `#[serde(default)]` to that enum leaves every
/// assertion in this file green while turning a window's typo into one of two
/// answers, only one of which can be undone. This is the assertion that fails
/// the day somebody adds it.
///
/// Which of the two a default would pick does not matter to this test and is
/// exactly why it asserts the refusal rather than the outcome: the argument is
/// that the choice belongs to the caller, not that the safe branch happens to
/// be the one serde would take.
///
/// It says which failure it is looking at before it says anything about
/// arguments, the same way the controls above do — "the command is not
/// registered" also produces an `Err`, and reading that as a refusal about
/// arguments would be red with the wrong cause named.
#[test]
fn a_model_change_that_says_nothing_about_the_existing_vectors_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);

    let refused = call(
        &webview,
        "set_embedding_model",
        json!({ "model": "baai/bge-m3" }),
    )
    .expect_err("a model change with no answer about the existing vectors was accepted");
    let refused = error_text(&refused);
    assert_ne!(
        refused,
        not_registered("set_embedding_model"),
        "this test asks about argument binding and its own command is not registered, so it \
         can say nothing about arguments"
    );
    assert!(
        refused.contains(INVALID_ARGS),
        "the call was reached and ran, so the decision to destroy or keep embeddings was taken \
         by something other than the caller: {refused}"
    );
    assert!(
        refused.contains("`existingVectors`"),
        "the rejection should name the missing argument, in the backticks Tauri puts round it \
         — the bare word appears in this command's own name in the same message: {refused}"
    );
}

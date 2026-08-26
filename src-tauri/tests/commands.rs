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
use mnema_desktop::models::set_key;
use mnema_desktop::state::AppState;
use mnema_desktop::walk_job;
use mnema_mock_provider::{MockServer, Reply, one_vector};
use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, Channel, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;
use tauri::{Manager, State, WebviewWindow, WebviewWindowBuilder};

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

/// An application whose provider is a real, local mock server rather than
/// [`NO_PROVIDER`] — for the one test in this file that needs a model
/// actually adopted. A fresh in-memory credential store per app, the same
/// guard `support/fixture.rs`'s own doc explains at length: `mnema-secrets`
/// only skips the platform store under its own `cfg(test)`, so an
/// integration test of another crate would otherwise reach a developer's
/// real keychain.
fn app_with_provider(dir: &std::path::Path, base: &str) -> tauri::App<MockRuntime> {
    mnema_secrets::test_store::register();
    mock_builder()
        .manage(AppState::new(
            dir.to_path_buf(),
            support::worker().to_path_buf(),
            base.to_string(),
            format!("mnema-desktop-commands-provider-test-{}", dir.display()),
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

/// Round-3 review, Finding 4. The old oracle asserted an *order*: the
/// citation for the reused chunk id must always read the pre-rebuild text.
/// That is wrong on both sides of the race — a search that loses the race
/// outright sees one coherent *new* state and this oracle failed it anyway,
/// and a writer delayed past the search passed without the race ever being
/// reached at all. The invariant `Db::read_snapshot` actually buys is
/// coherence, not order: whichever state a search's answer reflects, it must
/// be *one* state, never the old chunk's id carrying the new chunk's text
/// because two different statements inside the same command read two
/// different moments.
///
/// Made checkable by conditioning *inclusion* on the same word the citation
/// is checked for, rather than treating them as two independent facts. The
/// query requires `MARKER`, which only the pre-rebuild text holds — so a hit
/// for `target_chunk` can only exist because some statement inside this
/// command's snapshot read the chunk while it still held `MARKER`. If the
/// citation for that same hit does not hold `MARKER`, two different
/// statements read two different moments: the match came from before the
/// rebuild and the citation from after it. Absence of the hit is the other
/// coherent outcome — by the time the snapshot was taken the document no
/// longer matched, whether because the rebuild had already landed or because
/// the read fell inside the gap between `clear_document_content`'s commit
/// and the rebuild's — and `bridge.rs`'s own citation loop already treats a
/// chunk that is simply gone as no error, not a defect.
fn assert_coherent_or_absent(answer: &Value, chunk_id: i64) {
    const MARKER: &str = "маркер";
    let hits = answer["hits"].as_array().expect("hits array");
    let Some(hit) = hits.iter().find(|h| h["chunkId"] == json!(chunk_id)) else {
        return;
    };
    let text = hit["text"].as_str().unwrap();
    assert!(
        text.contains(MARKER),
        "the hit exists only because a statement inside this command matched \
         {MARKER:?}, but its citation does not hold that word — the old \
         chunk's id carrying the new chunk's text: {text:?}"
    );
}

/// A document with one chunk holding `text`, for
/// [`a_rebuild_racing_the_ipc_search_does_not_reach_its_citation`]'s decoys.
fn write_one_document(db: &mnema_index::Db, id: &str, text: &str) -> i64 {
    db.insert_document(id, "text/plain", 1, SourceKind::Document)
        .unwrap();
    rebuild_one_chunk(db, id, text)
}

/// Like [`rebuild_one_chunk`], but at a caller-chosen `ord` and
/// `reading_order`, on a `page` the caller already opened.
///
/// `rebuild_one_chunk` hardcodes `ord = 0`, which cannot build Task 2's
/// intra-document duplicate: two occurrences of identical text need two
/// different `ord`s (`UNIQUE(document_id, ord)`, `schema.sql:168`), and two
/// different blocks to sit in (`ix_block_page` needs distinct
/// `reading_order`s on one page).
fn insert_chunk_at(
    db: &mnema_index::Db,
    doc: &str,
    page: i64,
    ord: i64,
    reading_order: i64,
    text: &str,
) -> i64 {
    let block = db
        .insert_block(
            page,
            &Block {
                block_type: BlockType::Paragraph,
                reading_order,
                language: None,
                text: text.to_string(),
                line_start: None,
                line_end: None,
            },
        )
        .unwrap();
    db.insert_chunk(
        doc,
        ord,
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
}

/// Round-2 review, F1: nothing pinned that `bridge::search` itself wraps
/// its own work in `Db::read_snapshot`, only that the mechanism works
/// (`crates/mnema-search/tests/snapshot_boundary.rs`) — removing that call
/// left the whole suite green. A real second connection rebuilds the
/// matched document — reused chunk id, the query's own required word
/// replaced — while the real `search` IPC command runs. See
/// `assert_coherent_or_absent`'s own doc for why the oracle asks about
/// coherence rather than which text won the race, and the private report
/// for this fixture's measured catch rate.
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

    // `bridge.rs`'s `SEARCH_QUERY_RULE` (`TermsInIndex`) demands every term
    // present anywhere in the index, same as `AllTerms` once all three are:
    // only a decoy or the target ever holds all three, so only they match,
    // and `matching`'s own `ORDER BY rank, chunk_fts.rowid` needs a real
    // sort over all of them — `USE TEMP B-TREE FOR ORDER BY` materialises
    // every matching row before `LIMIT` applies (`search.rs:130-136`), and
    // that is the race's width: 6000 decoys costs low milliseconds, the
    // same order of magnitude as the writer's own head start below,
    // matched against the fillers `citation.rs:1301-1325`'s own fixture
    // already relies on for the identical id-reuse ordering. Doubled term
    // frequency ranks the target ahead of every once-tied decoy, so it is
    // always the sort's own cost that supplies the width, not where in a
    // citation loop the target happens to fall.
    let writer = mnema_index::open(&path).unwrap();
    const MARKER: &str = "маркер";
    const DECOYS: usize = 6000;
    for i in 0..DECOYS {
        write_one_document(
            &writer,
            &format!("{i:064x}"),
            &format!("спільний термін {MARKER}"),
        );
    }
    let target_doc = "9".repeat(64);
    let original_text = format!("спільний спільний термін термін {MARKER} {MARKER}");
    let target_chunk = write_one_document(&writer, &target_doc, &original_text);

    // Control, before the writer thread exists to race against: this proves
    // the fixture and ranking work on their own, so a failure below can
    // only mean the race reached the citation.
    let control = call(
        &webview,
        "search",
        json!({ "query": "спільний термін маркер" }),
    )
    .expect("control search was rejected");
    assert!(
        control["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["chunkId"] == json!(target_chunk)),
        "the fixture on its own must find the target chunk: {control}"
    );
    assert_coherent_or_absent(&control, target_chunk);

    let writer_handle = std::thread::spawn(move || {
        // A short, deliberately generous head start over the search this
        // thread races: the match query's own sort over `DECOYS` tied rows,
        // comment above, gives it room to land inside that sort.
        std::thread::sleep(Duration::from_millis(2));
        writer
            .clear_document_content(&target_doc)
            .expect("clear the target document");
        // No `MARKER` in the rebuilt text: the query the search below asks
        // can no longer match this chunk once this statement has committed,
        // which is what makes a hit whose citation lacks `MARKER` provable
        // incoherence rather than an ordinary, harmless miss.
        rebuild_one_chunk(&writer, &target_doc, "спільний термін замінник");
    });

    let answer = call(
        &webview,
        "search",
        json!({ "query": "спільний термін маркер" }),
    )
    .expect("search was rejected");
    writer_handle.join().expect("the writer thread panicked");

    assert_coherent_or_absent(&answer, target_chunk);
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
/// re-exports `Citation` and it is `Serialize` (its derive in `write.rs`), so this
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

/// Codex round 3, Finding 1 — a regression from the `content_arm` split for
/// D111. Before the split, `content_arm` turned `active_space` and
/// `space_model` errors into `ContentArm::Failed` (`content.rs:183-199`
/// still does, on the single-connection path). After the split,
/// `resolve_content_query` let those errors escape through `?` and reject
/// the whole IPC command, throwing away an already-computable lexical
/// answer. `models.rs:243-258` names how a dangling `meta.active_space` is
/// reachable without a corrupt file: a confirmed model change that commits
/// `drop_space` and then fails adoption. Reproduced here the same way,
/// through `Db::drop_space` directly, and asked through the real `search`
/// command rather than `resolve_content_query` alone — the defect was that
/// the error escaped to the command boundary, so the pin has to sit there
/// too.
#[test]
fn an_index_failure_inside_the_content_arm_stays_local_to_it() {
    const KEY: &str = "test-key-not-a-real-one-round3-f1";
    const MODEL: &str = "baai/bge-m3";
    const DIM: i64 = 1024;
    const CREDITS: &str = r#"{"data":{"total_credits":10.0,"total_usage":1.0}}"#;

    // Round-3 adversarial review, R-3's own broken-case investigation: this
    // test used to go through `models::set_embedding_model` (the checked,
    // network-backed command), the only call to it in this file. Mutating
    // that command's own `existing_vectors` parameter — an unrelated,
    // pre-round-3 mutation case in `scripts/mutations/embedding.sh` — broke
    // this test binary's compilation instead of the case's own target,
    // because `commands.rs` is compiled whole. `Db::adopt_embedding_model`
    // directly needs no network call at all and cannot collide with a
    // mutation of a command this test was never testing.
    let server = MockServer::new(vec![Reply::ok(CREDITS)]);
    let dir = tempfile::tempdir().unwrap();
    let app = app_with_provider(dir.path(), server.base());
    let state = app.state::<AppState>();

    state.open_index().expect("the index opens");
    set_key(state.clone(), KEY.into()).expect("the key is accepted");
    let adopted = state
        .with_index(|db| db.adopt_embedding_model(MODEL, DIM, "credential-ref", "chunker-v1"))
        .expect("the default model is adopted");

    // The shape `models.rs:243-258` names: `drop_space` alone, leaving
    // `meta.active_space` pointing at a space that no longer exists — the
    // debris a confirmed model change leaves if `drop_space` commits and
    // the adoption that follows it does not.
    state
        .with_index(|db| db.drop_space(adopted.space_id))
        .expect("drop the space directly, the way a failed re-adoption would");

    let text_dir = fixture_dir();
    let webview = main_webview(&app);
    let root = call(
        &webview,
        "add_watched_folder",
        json!({ "path": text_dir.path().display().to_string() }),
    )
    .expect("add_watched_folder was rejected")
    .as_i64()
    .expect("add_watched_folder did not return an id");
    run_walk_to_completion(&app, root);

    let answer = call(&webview, "search", json!({ "query": "fox" }))
        .expect("a dangling active_space must not reject the whole command");

    assert_eq!(
        answer["text"]["kind"],
        json!("answered"),
        "the lexical arm must survive an index failure in the content arm: {answer}"
    );
    assert!(
        answer["hits"]
            .as_array()
            .is_some_and(|hits| !hits.is_empty()),
        "the lexical hit must still reach the window: {answer}"
    );
    assert_eq!(
        answer["content"]["kind"],
        json!("failed"),
        "the content arm alone must report the failure: {answer}"
    );
}

/// I5 (spec §5): the one network call the content arm makes must run before
/// any read snapshot opens, so a slow provider cannot block a writer's
/// checkpoint. Proven here without a single sleep. The embed reply is held in
/// flight on a barrier; a probe thread waits until the embed has actually
/// reached the mock, then takes the index lock and only *then* releases the
/// reply. Taking that lock is possible only while the embed runs outside the
/// mutex — move the embed inside the snapshot and the probe blocks on the lock
/// forever, the barrier never releases, and `search` deadlocks. The watchdog
/// turns that deadlock into a loud failure rather than a silent hang.
///
/// Set up like `an_index_failure_inside_the_content_arm_stays_local_to_it` —
/// a real provider, a key, a model adopted — but without dropping the space,
/// so the content arm reaches its one embed instead of failing before it.
#[test]
fn the_content_arm_embeds_the_query_before_it_locks_the_index() {
    const KEY: &str = "test-key-not-a-real-one-i5";
    const MODEL: &str = "baai/bge-m3";
    const DIM: usize = 1024;
    const CREDITS: &str = r#"{"data":{"total_credits":10.0,"total_usage":1.0}}"#;

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let server = MockServer::new(vec![
        // `set_key` checks the key against `/credits` before it stores it.
        Reply::ok(CREDITS),
        // The content arm's one embed, held in flight until the probe releases it.
        Reply::gated(barrier.clone(), &one_vector(DIM)),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let app = app_with_provider(dir.path(), server.base());
    let state = app.state::<AppState>();

    state.open_index().expect("the index opens");
    set_key(state.clone(), KEY.into()).expect("the key is accepted");
    state
        .with_index(|db| {
            db.adopt_embedding_model(MODEL, DIM as i64, "credential-ref", "chunker-v1")
        })
        .expect("the default model is adopted");

    // `set_key`'s own `/credits` request is already queued on the mock; take it
    // so the probe's `server.request()` below waits for the embed and nothing
    // else. The reply order already pairs it with `Reply::ok(CREDITS)`.
    server.request();

    let text_dir = fixture_dir();
    let webview = main_webview(&app);
    let root = call(
        &webview,
        "add_watched_folder",
        json!({ "path": text_dir.path().display().to_string() }),
    )
    .expect("add_watched_folder was rejected")
    .as_i64()
    .expect("add_watched_folder did not return an id");
    run_walk_to_completion(&app, root);

    // The probe runs on its own thread and reaches the managed state through the
    // app handle — `AppState` is neither `Clone` nor `Send`, but `AppHandle`
    // is `Send + 'static`. It waits until the embed request has landed on the
    // mock (so it is provably in flight), then takes the index lock. In correct
    // code the lock is free while the embed is in flight; move the embed inside
    // the snapshot and this `with_index` blocks forever and never reaches the
    // barrier below, so the embed reply is never written and `search` hangs.
    let handle = app.handle().clone();
    let probe_barrier = barrier.clone();
    let probe = std::thread::spawn(move || {
        server.request();
        handle
            .state::<AppState>()
            .with_index(|_db| Ok::<(), mnema_index::Error>(()))
            .expect("the index lock must be free while the query embeds");
        probe_barrier.wait();
    });

    // `search` runs on this thread, so an I5 regression hangs it here. A
    // watchdog turns that hang into a loud, per-run failure. It aborts only on a
    // genuine timeout — a normal panic (say `search` was rejected) drops the
    // sender and the watchdog stands down, leaving the real failure to report.
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if let Err(mpsc::RecvTimeoutError::Timeout) = done_rx.recv_timeout(Duration::from_secs(20))
        {
            eprintln!(
                "I5 regression: `search` deadlocked — the query appears to embed inside the \
                 index mutex rather than before the read snapshot opens"
            );
            std::process::abort();
        }
    });

    let answer = call(&webview, "search", json!({ "query": "fox" })).expect("search was rejected");
    // Stand the watchdog down before joining anything a deadlock would have held.
    let _ = done_tx.send(());
    probe.join().expect("the probe thread panicked");
    watchdog.join().expect("the watchdog thread panicked");

    assert_eq!(
        answer["content"]["kind"],
        json!("answered"),
        "the content arm must have embedded and answered: {answer}"
    );
    assert!(
        answer["hits"].as_array().is_some_and(|h| !h.is_empty()),
        "the fused hits must reach the window: {answer}"
    );
}

/// D115① (part 2): `content["inspected"]` is the honest eligible-and-embedded
/// pool [`mnema_index::Db::eligible_embedded_chunk_count`] computes, not
/// [`mnema_index::Db::embedded_chunk_count`]'s wider count of every vector,
/// ineligible ones included. One end-to-end test rather than a separate Rust
/// unit test plus a render test: either half can pass on its own without the
/// `ContentArm` → `ContentArmReport` → serde passthrough actually being
/// wired, so only the IPC boundary proves the wiring.
///
/// The fixture writes two vectors in the same space: one behind an `Indexed`
/// document (eligible and embedded — `inspected` must count it) and one
/// behind a document [`write_one_document`] left `Indexed` but is downgraded
/// back to `Pending` right after (embedded, not eligible — `embedded` must
/// count it and `inspected` must not). This is the same eligible-vs-embedded
/// split `eligible_embedded_counts_only_chunks_that_are_both`
/// (`mnema-index/tests/space.rs`) pins at the `Db` layer, driven here through
/// the real `search` command instead.
#[test]
fn search_reports_the_inspected_pool_not_the_embedded_count() {
    const KEY: &str = "test-key-not-a-real-one-inspected-pool";
    const MODEL: &str = "baai/bge-m3";
    const DIM: usize = 1024;
    const CREDITS: &str = r#"{"data":{"total_credits":10.0,"total_usage":1.0}}"#;

    let server = MockServer::new(vec![
        Reply::ok(CREDITS),          // set_key's own /credits check
        Reply::ok(&one_vector(DIM)), // search's content-arm query embed
    ]);
    let dir = tempfile::tempdir().unwrap();
    let app = app_with_provider(dir.path(), server.base());
    let state = app.state::<AppState>();

    state.open_index().expect("the index opens");
    set_key(state.clone(), KEY.into()).expect("the key is accepted");

    state
        .with_index(|db| {
            let adopted =
                db.adopt_embedding_model(MODEL, DIM as i64, "credential-ref", "chunker-v1")?;

            // Eligible and embedded: an Indexed document with a vector — the
            // one chunk `inspected` must count.
            let eligible_doc = "e".repeat(64);
            let eligible_chunk =
                write_one_document(db, &eligible_doc, "a real question about foxes");
            db.upsert_vector(adopted.space_id, eligible_chunk, &vec![1.0f32; DIM])?;

            // Embedded, NOT eligible: `write_one_document` leaves its document
            // `Indexed`, so it is downgraded back to `Pending` here — a vector
            // behind a document `search` cannot reach. `embedded_chunk_count`
            // still counts it; `inspected` must not.
            let pending_doc = "d".repeat(64);
            let pending_chunk = write_one_document(db, &pending_doc, "unrelated pending text");
            db.set_document_status(&pending_doc, mnema_index::DocumentStatus::Pending)?;
            db.upsert_vector(adopted.space_id, pending_chunk, &vec![0.5f32; DIM])?;

            Ok(())
        })
        .expect("the fixture is written");

    let webview = main_webview(&app);
    let answer =
        call(&webview, "search", json!({ "query": "real question" })).expect("search was rejected");

    let content = &answer["content"];
    assert_eq!(
        content["kind"],
        json!("answered"),
        "the content arm must have answered: {answer}"
    );
    assert_eq!(
        content["embedded"],
        json!(2),
        "both vectors are counted here, eligible or not: {answer}"
    );
    assert!(
        content["inspected"].is_i64(),
        "content[\"inspected\"] must be present: {answer}"
    );
    let inspected = content["inspected"].as_i64().unwrap();
    assert_eq!(
        inspected, 1,
        "only the eligible, embedded chunk is inspectable: {answer}"
    );
    assert!(
        inspected < content["embedded"].as_i64().unwrap(),
        "inspected must be the honest, narrower pool, not the wider embedded count: {answer}"
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

/// A 200 `/credits` body `check_key` accepts, so `set_key` stores the key
/// instead of failing — the shape `an_index_failure_inside_the_content_arm`
/// and the I5 test already use. Every `ask_*` test below needs it: `set_key`
/// always checks the key against `/credits` before storing it, so that
/// request is the first reply the mock must hold, ahead of the ask's own.
const ASK_CREDITS: &str = r#"{"data":{"total_credits":10.0,"total_usage":1.0}}"#;

/// Saves a chat model the way the window's `set_chat_model` command does —
/// straight into `META_CHAT_MODEL` (`models.rs`), the row `chat_readiness`
/// reads. Called directly rather than through the IPC because `set_chat_model`
/// is `pub` and the point here is only to reach the `Ready` gate, not to prove
/// registration (which `search`'s own registration test already covers).
fn set_chat_model_via(state: &State<'_, AppState>, model: &str) {
    mnema_desktop::models::set_chat_model(state.clone(), model.into())
        .expect("the chat model is saved");
}

/// The private gate (spec §7.2): with no chat model set, `ask` must answer
/// with citations and never call the chat model at all. The content arm is on
/// with a model adopted and a key entered, so the gate is provably not vacuous
/// — retrieval makes exactly one `/embeddings` call, carrying the *query*
/// (not citation bytes, [[assert-both-directions]]), and zero
/// `/chat/completions`. `set_key`'s own `/credits` request is drained first so
/// the request read after the ask is the embed and nothing else.
#[test]
fn ask_without_a_chat_model_returns_citations_only_and_makes_no_chat_call() {
    const KEY: &str = "test-key-ask-citations-only";
    const MODEL: &str = "baai/bge-m3";
    const DIM: usize = 1024;

    // set_key's /credits, then the ask's query embed (content arm on).
    let server = MockServer::new(vec![Reply::ok(ASK_CREDITS), Reply::ok(&one_vector(DIM))]);
    let dir = tempfile::tempdir().unwrap();
    let app = app_with_provider(dir.path(), server.base());
    let state = app.state::<AppState>();
    state.open_index().unwrap();
    set_key(state.clone(), KEY.into()).unwrap();
    state
        .with_index(|db| {
            db.adopt_embedding_model(MODEL, DIM as i64, "credential-ref", "chunker-v1")
        })
        .unwrap();
    // deliberately NO chat model set.

    let text_dir = fixture_dir();
    let webview = main_webview(&app);
    // Content arm explicitly on, not the D106 "absent row means on" default:
    // this is the privacy-gate test, and it proves generation stays off by
    // showing the ask makes the content arm's query embed and no chat call. If
    // the product default ever flipped to off, that embed would vanish and this
    // test would fail on a 10 s `request()` timeout with a misleading message
    // rather than a clear gate signal. Pinning the arm keeps the guarantee the
    // test asserts independent of that default.
    call(
        &webview,
        "set_search_arms",
        json!({ "text": true, "content": true }),
    )
    .expect("set_search_arms was rejected");
    let root = call(
        &webview,
        "add_watched_folder",
        json!({ "path": text_dir.path().display().to_string() }),
    )
    .unwrap()
    .as_i64()
    .unwrap();
    run_walk_to_completion(&app, root);

    // Drain set_key's own /credits so the next request read is the ask's embed.
    let credits = server.request();
    assert!(
        credits.contains("/credits"),
        "the one setup request is the key check: {credits}"
    );

    let answer = call(&webview, "ask", json!({ "query": "fox" })).expect("ask was rejected");
    assert_eq!(
        answer["kind"],
        json!("citationsOnly"),
        "no chat model → citationsOnly: {answer}"
    );
    assert!(
        answer["citations"]
            .as_array()
            .is_some_and(|c| !c.is_empty()),
        "the citations the window draws must not be empty: {answer}"
    );

    // Exactly one further request, and it is the embed — the query, not chat.
    let embed = server.request();
    assert!(
        embed.contains("/embeddings"),
        "the ask's one call must be the query embed: {embed}"
    );
    assert!(
        embed.contains("fox"),
        "the embed body must be the query, not citation bytes: {embed}"
    );
    assert!(
        server.request_if_any().is_none(),
        "no chat request may have been made"
    );
}

/// `Ready` (a model and a key) but retrieval found nothing → `Refused`
/// `{NoCandidates}`, and the chat model is NOT called (`service.py:66-68`).
/// The index is empty, so the text arm finds nothing; the content arm is off,
/// so no query embed competes — the only request on the wire is `set_key`'s
/// `/credits`, which is drained before the no-chat assertion.
#[test]
fn ask_with_a_model_but_no_candidates_refuses_without_calling_chat() {
    const KEY: &str = "test-key-ask-nocandidates";
    // Only set_key's /credits is expected; any request past it is a surplus (599).
    let server = MockServer::new(vec![Reply::ok(ASK_CREDITS)]);
    let dir = tempfile::tempdir().unwrap();
    let app = app_with_provider(dir.path(), server.base());
    let state = app.state::<AppState>();
    state.open_index().unwrap();
    set_key(state.clone(), KEY.into()).unwrap();
    set_chat_model_via(&state, "openai/gpt-4o-mini");

    let webview = main_webview(&app);
    // Content arm off so no query embed runs; the index is empty so the text
    // arm finds nothing — Ready, with zero hits.
    call(
        &webview,
        "set_search_arms",
        json!({ "text": true, "content": false }),
    )
    .unwrap();

    let answer =
        call(&webview, "ask", json!({ "query": "nothing indexed" })).expect("ask was rejected");
    assert_eq!(answer["kind"], json!("refused"), "{answer}");
    assert_eq!(answer["reason"]["kind"], json!("noCandidates"), "{answer}");

    // Drain set_key's own /credits request, then prove chat was never called.
    let credits = server.request();
    assert!(
        credits.contains("/credits"),
        "the one setup request is the key check: {credits}"
    );
    assert!(
        server.request_if_any().is_none(),
        "NoCandidates must not call chat"
    );
}

/// The anchor→citation mapping, and the off-by-one silent lie it exists to
/// prevent (spec §9). `resolve_anchors` guarantees `1 <= n <= passages.len()
/// == hits.len()`, so ordinal `n` maps to `hits[n-1]`. Two documents with
/// identical searchable text tie in BM25 rank, so `matching`'s
/// `ORDER BY rank, chunk_fts.rowid` orders them by chunk id — insertion order
/// (the tie-break `matching_breaks_bm25_ties_by_chunk_id` pins). The content
/// arm is off, so the single-arm RRF preserves that order exactly: the first
/// written document is the first fused hit, the second written the second. The
/// mock completion cites `<c>2</c>`, so the one citation's `chunkId` must be
/// the SECOND document's — asserting the `chunkId`, not merely `anchor == 2`
/// or a count, is the only check an off-by-one that shows the neighbour fails.
#[test]
fn ask_maps_each_anchor_to_the_right_citation_and_generates() {
    const KEY: &str = "test-key-ask-generated";
    const BODY: &str = "quantum entanglement resonance";

    // The chat completion cites the second source.
    let completion = serde_json::json!({
        "choices": [{ "message": { "content": "The second source answers <c>2</c>." } }]
    })
    .to_string();
    let server = MockServer::new(vec![Reply::ok(ASK_CREDITS), Reply::ok(&completion)]);
    let dir = tempfile::tempdir().unwrap();
    let app = app_with_provider(dir.path(), server.base());
    let state = app.state::<AppState>();
    state.open_index().unwrap();
    set_key(state.clone(), KEY.into()).unwrap();
    set_chat_model_via(&state, "openai/gpt-4o-mini");

    let webview = main_webview(&app);
    // Content off so no embed request competes with the chat request.
    call(
        &webview,
        "set_search_arms",
        json!({ "text": true, "content": false }),
    )
    .unwrap();

    // Two documents, identical text: they tie in rank and order by chunk id,
    // so the first written is the first fused hit and the second the second.
    let (_first_chunk, second_chunk) = state
        .with_index(|db| {
            Ok::<_, mnema_index::Error>((
                write_one_document(db, &"a".repeat(64), BODY),
                write_one_document(db, &"b".repeat(64), BODY),
            ))
        })
        .unwrap();

    let answer = call(&webview, "ask", json!({ "query": "quantum entanglement" }))
        .expect("ask was rejected");
    assert_eq!(
        answer["kind"],
        json!("generated"),
        "Ready + hits + a real completion → generated: {answer}"
    );
    let citations = answer["citations"].as_array().expect("citations array");
    assert_eq!(
        citations.len(),
        1,
        "only the cited anchor becomes a citation: {answer}"
    );
    assert_eq!(citations[0]["anchor"], json!(2), "{answer}");
    assert_eq!(
        citations[0]["chunkId"],
        json!(second_chunk),
        "<c>2</c> must resolve to the SECOND fused hit's chunk, not the first \
         — the off-by-one silent lie (spec §9): {answer}"
    );
    // Pins the seam from `retrieve`'s `Hit` construction through `ask`'s
    // `AskCitation` construction: both must carry the chunk's own
    // `document_id`/`ord` through, not a hardcoded or blanked stand-in.
    assert_eq!(
        citations[0]["documentId"],
        json!("b".repeat(64)),
        "documentId must name the SECOND document, not a hardcoded or \
         blanked value smuggled through Hit/AskCitation: {answer}"
    );
    assert_eq!(
        citations[0]["ord"],
        json!(0),
        "ord must be read from the chunk's own row, not hardcoded: {answer}"
    );
}

/// `Ready`, chat was called, and the model answered with nothing → `Refused`
/// `{EmptyCompletion}`, never `Generated{answer:""}` (`service.py:80-82`). One
/// indexed document so retrieval is non-empty and the chat step is reached;
/// the completion is whitespace only, which `mnema_rag::answer` reports as
/// `Ok(None)`.
#[test]
fn ask_with_an_empty_completion_refuses_as_empty_completion() {
    const KEY: &str = "test-key-ask-empty";
    const BODY: &str = "quantum entanglement resonance";

    let completion =
        serde_json::json!({ "choices": [{ "message": { "content": "  \n " } }] }).to_string();
    let server = MockServer::new(vec![Reply::ok(ASK_CREDITS), Reply::ok(&completion)]);
    let dir = tempfile::tempdir().unwrap();
    let app = app_with_provider(dir.path(), server.base());
    let state = app.state::<AppState>();
    state.open_index().unwrap();
    set_key(state.clone(), KEY.into()).unwrap();
    set_chat_model_via(&state, "openai/gpt-4o-mini");

    let webview = main_webview(&app);
    call(
        &webview,
        "set_search_arms",
        json!({ "text": true, "content": false }),
    )
    .unwrap();
    state
        .with_index(|db| Ok::<_, mnema_index::Error>(write_one_document(db, &"c".repeat(64), BODY)))
        .unwrap();

    let answer = call(&webview, "ask", json!({ "query": "quantum entanglement" }))
        .expect("ask was rejected");
    assert_eq!(
        answer["kind"],
        json!("refused"),
        "a blank completion is a refusal, not an empty answer: {answer}"
    );
    assert_eq!(
        answer["reason"]["kind"],
        json!("emptyCompletion"),
        "{answer}"
    );
}

/// Port of `ask.py:17` (`Field(max_length=2048)`): the query is capped at
/// 2048 characters, not bytes — Python `str` length counts code points, so
/// the probe repeats a two-byte character, catching a `len()` (bytes)
/// confused for `chars().count()` (chars). The guard runs before
/// `read_arms`/`retrieve` (spec §12), so a minimal app — no index open, no
/// provider configured — is enough: an over-long query must never reach
/// either. The at-limit half only needs the length rejection to be absent;
/// it may still fail later on `IndexNotOpen`, since no index was opened.
#[test]
fn ask_rejects_a_query_longer_than_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let webview = main_webview(&app);

    let too_long = "я".repeat(2049); // 2049 chars, multi-byte on purpose
    let error = call(&webview, "ask", json!({ "query": too_long }))
        .expect_err("2049 characters must be rejected");
    let message = error.as_str().unwrap().to_lowercase();
    assert!(
        message.contains("too long") || message.contains("2048"),
        "unhelpful message: {error}"
    );

    // The boundary is inclusive: 2048 is allowed (does not error on length).
    let at_limit = "я".repeat(2048);
    let ok = call(&webview, "ask", json!({ "query": at_limit }));
    assert!(
        ok.is_ok() || !ok.unwrap_err().as_str().unwrap().contains("2048"),
        "2048 chars must not be rejected for length"
    );
}

/// The lower half of `ask.py:17`'s `Field(..., min_length=1, max_length=2048)`:
/// a blank question is rejected before any retrieval. The server's `min_length=1`
/// rejects only the empty string; we trim, so a whitespace-only question — as
/// meaningless as an empty one — is rejected too. Why it matters beyond
/// tidiness: with the content arm on, a blank query still reaches
/// `resolve_content_query`, which sends an external, billable `/embeddings`
/// request, and if that returns hits while chat is `Ready`, those passages go
/// to `/chat/completions` — the D115 billable-request mechanism through the new
/// `ask` caller. Here the content arm is on AND a chat model is set, so every
/// reason retrieval and generation WOULD fire; a blank ask that makes no
/// request past `set_key`'s `/credits` proves the guard runs first, not a
/// missing precondition.
#[test]
fn ask_rejects_a_blank_query_before_any_retrieval() {
    const KEY: &str = "test-key-ask-blank";
    const MODEL: &str = "baai/bge-m3";
    const DIM: usize = 1024;

    // Only `set_key`'s `/credits` is queued — no reply for a query embed or a
    // chat call, because the guard must return before either could be made. Any
    // request past `/credits` is a surplus (599), which would fail the test.
    let server = MockServer::new(vec![Reply::ok(ASK_CREDITS)]);
    let dir = tempfile::tempdir().unwrap();
    let app = app_with_provider(dir.path(), server.base());
    let state = app.state::<AppState>();
    state.open_index().unwrap();
    set_key(state.clone(), KEY.into()).unwrap();
    state
        .with_index(|db| {
            db.adopt_embedding_model(MODEL, DIM as i64, "credential-ref", "chunker-v1")
        })
        .unwrap();
    set_chat_model_via(&state, "openai/gpt-4o-mini");

    let webview = main_webview(&app);
    call(
        &webview,
        "set_search_arms",
        json!({ "text": true, "content": true }),
    )
    .expect("set_search_arms was rejected");

    // Drain `set_key`'s own `/credits` so a later request read would be the ask's.
    let credits = server.request();
    assert!(
        credits.contains("/credits"),
        "the one setup request is the key check: {credits}"
    );

    for blank in ["", "   ", " \n\t "] {
        let error = call(&webview, "ask", json!({ "query": blank }))
            .expect_err("a blank question must be rejected");
        assert!(
            error.as_str().unwrap_or_default().contains("blank"),
            "a blank question should be refused as blank; got {error}"
        );
    }

    // The guard returned before retrieval on every blank: no billable query
    // embed, no chat call.
    assert!(
        server.request_if_any().is_none(),
        "a blank ask must make no request — no billable embed, no chat call"
    );
}

/// Mirror of `ask_rejects_a_blank_query_before_any_retrieval` for `search`
/// (spec §2.1, D115②). With the content arm on, a blank query used to reach
/// `resolve_content_query` and its billable `/embeddings`; the guard mirrors
/// `ask`'s, so `search` refuses blank before any retrieval. `search` needs no
/// chat model — it never generates.
#[test]
fn search_rejects_a_blank_query_before_any_retrieval() {
    const KEY: &str = "test-key-search-blank";
    const MODEL: &str = "baai/bge-m3";
    const DIM: usize = 1024;

    // Only `set_key`'s `/credits` is queued; any request past it is a surplus.
    let server = MockServer::new(vec![Reply::ok(ASK_CREDITS)]);
    let dir = tempfile::tempdir().unwrap();
    let app = app_with_provider(dir.path(), server.base());
    let state = app.state::<AppState>();
    state.open_index().unwrap();
    set_key(state.clone(), KEY.into()).unwrap();
    state
        .with_index(|db| {
            db.adopt_embedding_model(MODEL, DIM as i64, "credential-ref", "chunker-v1")
        })
        .unwrap();

    let webview = main_webview(&app);
    call(
        &webview,
        "set_search_arms",
        json!({ "text": true, "content": true }),
    )
    .expect("set_search_arms was rejected");

    // Drain `set_key`'s own `/credits`.
    let credits = server.request();
    assert!(
        credits.contains("/credits"),
        "the one setup request is the key check: {credits}"
    );

    for blank in ["", "   ", " \n\t "] {
        let error = call(&webview, "search", json!({ "query": blank }))
            .expect_err("a blank query must be rejected");
        assert!(
            error.as_str().unwrap_or_default().contains("blank"),
            "a blank query should be refused as blank; got {error}"
        );
    }

    assert!(
        server.request_if_any().is_none(),
        "a blank search must make no request — no billable embed"
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
/// broke last time. The folder picker belongs to the settings window (§9.2),
/// which `capabilities/default.json` now names.
#[test]
fn the_settings_window_may_reach_the_folder_picker() {
    let app = app_with_real_capabilities();
    let webview = WebviewWindowBuilder::new(&app, "settings", Default::default())
        .build()
        .expect("failed to build the settings mock webview");

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
        "the `settings` window did not reach `OpenDialogOptions` deserialization — either the ACL \
         refused it, or the dialog plugin was never registered to answer it: {message}"
    );
}

/// The counterweight to the test above: without it, a capability that
/// granted `dialog:allow-open` to every window regardless of `windows: [
/// "settings"]` would pass the positive test for the same reason a capability
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

#[test]
fn list_tree_enumerates_roots_indexed_files_and_recents() {
    use mnema_core::OnDisk; // SourceKind is already in file scope (commands.rs:14, from mnema_core)
    use mnema_index::DocumentStatus;

    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let (root_a, root_b) = state
        .with_index(|db| {
            let a = db.insert_watched_root("/tmp/alpha")?;
            let b = db.insert_watched_root("/tmp/beta")?;

            let older = "a".repeat(64);
            let newer = "b".repeat(64);
            let pending = "c".repeat(64);
            for id in [&older, &newer, &pending] {
                db.insert_document(id, "text/plain", 1, SourceKind::Document)?;
            }
            db.set_document_status(&older, DocumentStatus::Indexed)?;
            db.set_document_status(&newer, DocumentStatus::Indexed)?;
            // Recency is the chunk/done completion time (`ingest_stage`), not
            // `document.created_at`. `pending` carries a chunk/done stage too —
            // the state a downgraded document is in — and the newest one, so the
            // indexed-only filter (not the INNER JOIN) is what keeps it out of
            // recents.
            for id in [&older, &newer, &pending] {
                db.record_stage(id, "chunk", "done")?;
            }
            db.conn().execute(
                "UPDATE ingest_stage SET updated_at = 1000 WHERE content_hash = ?1 AND stage = 'chunk'",
                [&older],
            )?;
            db.conn().execute(
                "UPDATE ingest_stage SET updated_at = 2000 WHERE content_hash = ?1 AND stage = 'chunk'",
                [&newer],
            )?;
            db.conn().execute(
                "UPDATE ingest_stage SET updated_at = 3000 WHERE content_hash = ?1 AND stage = 'chunk'",
                [&pending],
            )?;
            // Two paths under A, deliberately out of sorted order; the pending doc under B.
            db.insert_path(
                a,
                "notes/old.txt",
                &older,
                OnDisk {
                    size_bytes: 1,
                    mtime: 1,
                },
                "text",
                1,
            )?;
            db.insert_path(
                a,
                "new.txt",
                &newer,
                OnDisk {
                    size_bytes: 1,
                    mtime: 1,
                },
                "text",
                1,
            )?;
            db.insert_path(
                b,
                "draft.txt",
                &pending,
                OnDisk {
                    size_bytes: 1,
                    mtime: 1,
                },
                "text",
                1,
            )?;
            Ok::<_, mnema_index::Error>((a, b))
        })
        .unwrap();

    // Through the IPC, by name — the doctrine (`commands.rs:1-6`): a direct call
    // proves nothing about registration or whether fields survive the camelCase
    // rename. `call` returns `Result<Value, Value>` (`commands.rs:174`).
    let v = call(&webview, "list_tree", json!({})).expect("list_tree was rejected");

    let roots = v["roots"].as_array().unwrap();
    assert_eq!(roots.len(), 2);
    // Roots in add order; basename is the display name; camelCase on the wire.
    assert_eq!(roots[0]["rootId"].as_i64().unwrap(), root_a);
    assert_eq!(roots[0]["name"], "alpha");
    assert_eq!(roots[1]["rootId"].as_i64().unwrap(), root_b);

    // Root A: indexed paths only, sorted ("new.txt" < "notes/old.txt").
    let files_a: Vec<&str> = roots[0]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["relativePath"].as_str().unwrap())
        .collect();
    assert_eq!(files_a, vec!["new.txt", "notes/old.txt"]);
    // Root B: the pending doc is excluded — an empty, still-listed root.
    assert!(roots[1]["files"].as_array().unwrap().is_empty());

    // Recents: newest first, indexed-only (the pending doc is absent).
    let recents = v["recents"].as_array().unwrap();
    let rec: Vec<&str> = recents
        .iter()
        .map(|d| d["relativePath"].as_str().unwrap())
        .collect();
    assert_eq!(rec, vec!["new.txt", "notes/old.txt"]);
    assert_eq!(recents[0]["indexedAt"].as_i64().unwrap(), 2000);
    assert_eq!(recents[0]["rootId"].as_i64().unwrap(), root_a);

    // Wire shape, both directions: snake_case must not leak (guards rename_all).
    assert!(roots[0].get("root_id").is_none());
    assert!(recents[0].get("indexed_at").is_none());
}

/// One indexed document the tree listing returns, written the way the pipeline
/// leaves it: an `Indexed` status, a `chunk`/`done` stage (so
/// [`mnema_index::Db::recent_indexed_documents`]'s INNER JOIN reaches it) and a
/// real path under `root`. The four writes
/// [`list_tree_enumerates_roots_indexed_files_and_recents`] already makes,
/// gathered into one call for the race guard's decoys and its intruder.
fn seed_indexed_file(db: &mnema_index::Db, root: i64, id: &str, relative_path: &str) {
    use mnema_core::OnDisk;
    db.insert_document(id, "text/plain", 1, SourceKind::Document)
        .expect("seed: insert_document");
    db.set_document_status(id, mnema_index::DocumentStatus::Indexed)
        .expect("seed: set_document_status");
    db.record_stage(id, "chunk", "done")
        .expect("seed: record_stage");
    db.insert_path(
        root,
        relative_path,
        id,
        OnDisk {
            size_bytes: 1,
            mtime: 1,
        },
        "text",
        1,
    )
    .expect("seed: insert_path");
}

/// Coherence-only, never false-red — the tree twin of
/// [`assert_coherent_or_absent`]. Every `recents[i]`'s `(rootId,
/// relativePath)` must be one the same listing reports under some
/// `roots[].files`. A coherent listing passes whichever way the race fell — the
/// intruder in both phases or in neither; only a torn read, a recent reaching
/// the listing while its file is absent from every `roots[].files`, fails.
///
/// One direction on purpose: `recents ⊆ files` is the whole invariant
/// `read_snapshot` buys here, and requiring equality would false-red on the
/// ordinary case where `files` holds far more than the `RECENTS_LIMIT` rows
/// `recents` caps at.
fn assert_coherent_recents(v: &Value) {
    let mut files: std::collections::HashSet<(i64, String)> = std::collections::HashSet::new();
    for root in v["roots"].as_array().expect("a roots array") {
        let root_id = root["rootId"].as_i64().expect("a rootId");
        for f in root["files"].as_array().expect("a files array") {
            let rel = f["relativePath"].as_str().expect("a relativePath");
            files.insert((root_id, rel.to_string()));
        }
    }
    for rec in v["recents"].as_array().expect("a recents array") {
        let pair = (
            rec["rootId"].as_i64().expect("a recent rootId"),
            rec["relativePath"]
                .as_str()
                .expect("a recent relativePath")
                .to_string(),
        );
        assert!(
            files.contains(&pair),
            "recents carries {:?} under root {}, absent from every roots[].files — a torn read: \
             list_tree returned a state the index never held: {v}",
            pair.1,
            pair.0
        );
    }
}

/// The P1-1 fix (`src-tauri/src/tree.rs`): `list_tree` reads the whole listing
/// inside one [`mnema_index::Db::read_snapshot`]. The `mnema-index` regression
/// (`the_tree_listing_reads_files_and_recents_from_one_snapshot`) composes the
/// two phases by hand and proves the *snapshot* holds; it pins nothing about
/// whether `list_tree` ITSELF wraps its reads — revert the command to
/// `with_index(build_tree_listing)` (three autocommit reads) and that test, and
/// the whole suite, stays green. The search path closed this exact gap with
/// [`a_rebuild_racing_the_ipc_search_does_not_reach_its_citation`], and this
/// mirrors it for `list_tree`. See the private guard-report for the measured
/// catch rate.
///
/// A second connection commits a full new indexed document — path and
/// chunk/done stage — while the real `list_tree` IPC command runs. Under one
/// snapshot the intruder is in neither the files nor the recents; under
/// autocommit it can reach the recents read (phase 2) while the files read
/// (phase 1) already snapshotted without it, and [`assert_coherent_recents`]
/// fails on that tear.
#[test]
fn a_write_racing_the_ipc_list_tree_cannot_tear_recents_from_its_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");
    let app = app_in(dir.path());
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).expect("open_index was rejected");

    // The window and this writer are two connections on one file — the running
    // indexing job's shape, the same arrangement `a_search_through_the_ipc_
    // finds_what_another_connection_wrote` relies on. The root is committed on
    // the writer; the window sees it across the connection.
    let writer = mnema_index::open(&path).unwrap();
    let root = writer
        .insert_watched_root(&dir.path().display().to_string())
        .unwrap();

    // Widen the race window the way the search fixture uses 6000 decoys: many
    // indexed files under the root make `indexed_files_under_root`'s
    // `ORDER BY relative_path` (phase 1) materialise a temp b-tree over every
    // row before it returns — low milliseconds, the same order as the writer's
    // head start below — and it is the gap between that read and the recents
    // read (phase 2) an autocommit `list_tree` leaves for the writer to land in.
    // One transaction around the seed keeps setup fast; it changes no read cost.
    const DECOYS: usize = 6000;
    writer.conn().execute_batch("BEGIN").unwrap();
    for i in 0..DECOYS {
        seed_indexed_file(
            &writer,
            root,
            &format!("{i:064x}"),
            &format!("decoy/{i:06}.txt"),
        );
    }
    // Rank every decoy below any live `unixepoch()`, so the intruder — whose
    // chunk/done stage stamps the real clock — always reaches recents' top
    // `RECENTS_LIMIT` when phase 2 sees it. Without this the intruder ties the
    // decoys on the one-second `unixepoch()` grid and its place in recents would
    // turn on `d.id`, not on whether the race reached the recents read.
    writer
        .conn()
        .execute(
            "UPDATE ingest_stage SET updated_at = 1 WHERE stage = 'chunk'",
            [],
        )
        .unwrap();
    writer.conn().execute_batch("COMMIT").unwrap();

    // Control, before the writer thread exists to race against: the listing is
    // coherent on its own, so a failure below can only mean the race reached the
    // recents read.
    let control = call(&webview, "list_tree", json!({})).expect("list_tree was rejected");
    assert_coherent_recents(&control);

    let intruder = "f".repeat(64);
    let writer_handle = std::thread::spawn(move || {
        // A short, deliberately generous head start over the listing this thread
        // races: the files read's sort over `DECOYS` rows, comment above, gives
        // the commit room to land after that read's snapshot but before the
        // recents read.
        std::thread::sleep(Duration::from_millis(2));
        // A full new indexed document under the root. Its chunk/done
        // `unixepoch()` puts it at the head of recents; it is absent from any
        // `roots[].files` a phase-1 snapshot took before this commit landed —
        // which is what makes a recent for it, with no matching file, provable
        // incoherence rather than a harmless extra row.
        seed_indexed_file(&writer, root, &intruder, "intruder.txt");
    });

    let v = call(&webview, "list_tree", json!({})).expect("list_tree was rejected");
    writer_handle.join().expect("the writer thread panicked");

    assert_coherent_recents(&v);
}

/// Hazard (1) of PR 5's "what disappears" pass, and what makes
/// `source_around` different from every read before it: `ask` and
/// `source_around` are **two IPC calls seconds apart**. The client holds a
/// `chunkId` from the first and sends it to the second, and no snapshot can
/// span them — they are two transactions by construction. `chunk.id` is
/// `INTEGER PRIMARY KEY` *without* `AUTOINCREMENT` (`schema.sql:149`), so
/// SQLite derives the next id as `max(id) + 1` and hands the ids of deleted
/// rows out again; a rebuild of the most recently indexed document deletes
/// exactly the top of that space. Answering with the new chunk's
/// neighbourhood under the citation the user clicked is precisely "answer
/// with text the file no longer contains", so the command refuses instead.
#[test]
fn source_around_refuses_a_chunk_id_a_rebuild_has_handed_to_other_text() {
    const ORIGINAL: &str = "Ціна оцифрування одного аркуша становить дві гривні.";
    const REBUILT: &str = "Ставка залишається незмінною протягом усього строку.";

    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "7".repeat(64);
    let (before, after) = state
        .with_index(|db| {
            let before = write_one_document(db, &doc, ORIGINAL);
            db.clear_document_content(&doc)?;
            let after = rebuild_one_chunk(db, &doc, REBUILT);
            Ok::<_, mnema_index::Error>((before, after))
        })
        .unwrap();

    // The fixture is the hazard only if SQLite really handed the id back.
    // Asserted loudly rather than skipped with a `return`: a quiet skip
    // leaves the test satisfied by a state it never reached. And on the
    // **chunk** rowid specifically — `block.id` and `chunk.id` are separate
    // rowid spaces, both reused, and `rebuild_one_chunk` mints one of each,
    // so a fixture that compared block ids would pass while proving nothing.
    assert_eq!(
        after, before,
        "SQLite did not hand the chunk rowid back, so this fixture never reached the id-reuse \
         hazard it is named for"
    );

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": before,
            "passageText": ORIGINAL,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(
        v["kind"],
        json!("gone"),
        "the id now carries different text, and the command answered with something other than \
         a refusal: {v}"
    );
    assert_eq!(
        v["reason"]["kind"],
        json!("idReused"),
        "a chunk does carry that id — it is simply not this passage — so the refusal must say \
         which of the two causes it was: {v}"
    );
    // Both directions. A `Gone` that still shipped the new chunk's
    // neighbourhood would satisfy a `kind`-only assertion, and the whole
    // point of the refusal is that no text comes back.
    assert!(
        v.get("blocks").is_none(),
        "a refusal must carry no text at all; this one shipped the other passage's blocks: {v}"
    );
}

/// Task 2's own reproduction of owner-Codex **P1** on PR #22, which the test
/// above cannot reach: that fixture reuses a chunk id inside *one* document,
/// so the TEXT pin alone already refuses it. `chunk.id` is reused across
/// DOCUMENTS just as readily (`schema.sql:149`, no `AUTOINCREMENT`), and two
/// documents whose middle paragraph happens to be byte-identical make the
/// text pin powerless: the reused id's text still matches, so it would let
/// the wrong document's neighbourhood through under the user's citation.
/// `documentId`/`ord` (Task 1) are what the identity pin now compares beside
/// the text.
#[test]
fn source_around_refuses_a_reused_id_whose_text_is_byte_identical() {
    const SHARED: &str = "The identical middle paragraph.";
    const NEIGHBOUR_B_BEFORE: &str = "Before B, only in doc_b.";
    const NEIGHBOUR_B_AFTER: &str = "After B, only in doc_b.";

    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc_a = "a".repeat(64);
    let doc_b = "b".repeat(64);
    let (cited_chunk, cited_ord, reused) = state
        .with_index(|db| {
            let before = write_one_document(db, &doc_a, SHARED);
            let ord = db.citation(before)?.unwrap().ord;
            db.delete_document(&doc_a)?;

            // doc_b's SHARED paragraph is inserted FIRST, so it — not one of
            // its neighbours — is the chunk that receives the id doc_a's
            // deletion just freed: `chunk.id` is `max(rowid) + 1`
            // (`schema.sql:149`), so whichever chunk is written next gets it,
            // regardless of where it sits in reading order. The neighbours
            // exist so a leaked answer has something of doc_b's own to be
            // caught carrying.
            db.insert_document(&doc_b, "text/plain", 1, SourceKind::Document)?;
            let page = db.insert_page(&doc_b, 1, "native:txt", None)?;
            let reused = insert_chunk_at(db, &doc_b, page, 1, 2, SHARED);
            insert_chunk_at(db, &doc_b, page, 0, 1, NEIGHBOUR_B_BEFORE);
            insert_chunk_at(db, &doc_b, page, 2, 3, NEIGHBOUR_B_AFTER);
            db.set_document_status(&doc_b, mnema_index::DocumentStatus::Indexed)?;

            Ok::<_, mnema_index::Error>((before, ord, reused))
        })
        .unwrap();

    // The fixture is the hazard only if the id really came back — asserted
    // loudly, never skipped (the idiom at `commands.rs:2905-2915`).
    assert_eq!(
        reused, cited_chunk,
        "SQLite did not reuse the chunk id, so this fixture proves nothing"
    );

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": cited_chunk,
            "passageText": SHARED,
            "citedDocumentId": doc_a,
            "citedOrd": cited_ord,
            "citedRootId": null,
            "citedRelativePath": null,
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(
        v["kind"],
        json!("gone"),
        "the text is byte-identical, but the reused id now belongs to another document — only \
         the identity pin can refuse this, the text pin cannot: {v}"
    );
    assert_eq!(v["reason"]["kind"], json!("idReused"), "{v}");
    assert!(
        v.get("blocks").is_none(),
        "a Gone answer must carry no text at all: {v}"
    );
    let rendered = v.to_string();
    assert!(
        !rendered.contains(NEIGHBOUR_B_BEFORE) && !rendered.contains(NEIGHBOUR_B_AFTER),
        "the other document's paragraphs must not appear anywhere in the answer: {rendered}"
    );
}

/// Step 5's second red: `documentId` alone cannot see a duplicate INSIDE one
/// document. The same paragraph twice at different `ord`s reuses neither
/// `chunk.id` (`schema.sql:149`) nor `document_id` when the id is handed back
/// to the OTHER occurrence — only `ord` (`UNIQUE(document_id, ord)`,
/// `schema.sql:168`) tells the two apart, and the text is identical too, so
/// the text pin cannot catch this one either.
#[test]
fn source_around_refuses_a_reused_id_within_the_same_document_at_a_different_ord() {
    const SAME: &str = "Boilerplate paragraph repeated twice.";
    const THROWAWAY: &str = "A throwaway paragraph, wasting one id.";

    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "d".repeat(64);
    let (cited_chunk, after) = state
        .with_index(|db| {
            db.insert_document(&doc, "text/plain", 1, SourceKind::Document)?;
            let page = db.insert_page(&doc, 1, "native:txt", None)?;
            insert_chunk_at(db, &doc, page, 0, 1, SAME);
            // The SECOND occurrence — `ord = 1` — is the one the client cites.
            let second = insert_chunk_at(db, &doc, page, 1, 2, SAME);
            db.set_document_status(&doc, mnema_index::DocumentStatus::Indexed)?;

            // A rebuild: everything under `doc` is cleared and rewritten. One
            // throwaway chunk consumes the id the FIRST occurrence held, so
            // the next chunk written — at `ord = 0`, the first occurrence's
            // own ord — is the one that lands on the SECOND occurrence's old
            // (cited) id.
            db.clear_document_content(&doc)?;
            let page2 = db.insert_page(&doc, 1, "native:txt", None)?;
            insert_chunk_at(db, &doc, page2, 9, 1, THROWAWAY);
            let after = insert_chunk_at(db, &doc, page2, 0, 2, SAME);
            db.set_document_status(&doc, mnema_index::DocumentStatus::Indexed)?;

            Ok::<_, mnema_index::Error>((second, after))
        })
        .unwrap();

    // The fixture is the hazard only if the rebuild really landed the cited
    // id on the ord=0 occurrence — asserted loudly, never assumed.
    assert_eq!(
        after, cited_chunk,
        "the rebuild did not hand the cited id to the ord=0 occurrence, so this fixture does \
         not reach the case it is named for"
    );

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": cited_chunk,
            "passageText": SAME,
            "citedDocumentId": doc,
            "citedOrd": 1,
            "citedRootId": null,
            "citedRelativePath": null,
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(
        v["kind"],
        json!("gone"),
        "the document and the text both match, but the ord does not — only ord tells this \
         document's two identical paragraphs apart: {v}"
    );
    assert_eq!(v["reason"]["kind"], json!("idReused"), "{v}");
    assert!(v.get("blocks").is_none(), "{v}");
}

/// The pin is **exact** equality, and this is the test that says so.
///
/// The plan forbids `contains`, trimming and normalising alike, but only the
/// empty-`passageText` case is red against `contains` — a pin rewritten as
/// `a.text.trim() != passage_text.trim()` passed every other test in this
/// file, which the controller's mutation run caught. A rebuild that adds or
/// drops surrounding whitespace produces exactly that state: the chunk at this
/// id is a *different* chunk, and a trimming comparison calls it the same one
/// and then hands back the neighbourhood of the wrong passage.
///
/// **The other direction is
/// [`source_around_admits_a_byte_identical_passage_text`], directly below.**
/// It is what stops this test being satisfied by a pin that refuses
/// everything, and it could not be written under Task 5.2: the excerpt arm was
/// `todo!()` then, and a panic inside a command reaches `call` as a
/// `RecvError` panic rather than a value, so there was no result to assert on.
/// Task 5.3 built the arm and paid the debt; the two tests are one pair and
/// neither is coverage alone.
#[test]
fn source_around_refuses_a_passage_that_differs_only_in_surrounding_whitespace() {
    const STORED: &str = "Ставка залишається незмінною протягом усього строку.";

    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "9".repeat(64);
    let chunk = state
        .with_index(|db| Ok::<_, mnema_index::Error>(write_one_document(db, &doc, STORED)))
        .unwrap();

    let padded = format!("  {STORED}\n");
    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": padded,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(
        v["kind"],
        json!("gone"),
        "the stored text and the echoed passage differ, and only trimming makes them equal — \
         the pin must refuse rather than answer about a chunk it was not asked about: {v}"
    );
    // The cause matters as much as the refusal. A chunk *does* carry this id,
    // so `idReused` is the only honest answer; a regression reporting
    // `noSuchChunk` would satisfy a `kind`-only assertion while the message
    // above told whoever read it the wrong story.
    assert_eq!(
        v["reason"]["kind"],
        json!("idReused"),
        "a chunk carries that id — it is simply not this passage — so the refusal must name \
         which of the two causes it was: {v}"
    );
    assert!(
        v.get("blocks").is_none(),
        "a refusal must carry no text at all: {v}"
    );
}

/// The debt Task 5.2 booked and Task 5.3 owed: the pin's **other** direction.
///
/// Every other test about the pin asserts a refusal, so a pin rewritten as
/// `true` — refuse everything — satisfies all of them and the suite stays
/// green while the command has become useless. This is the only test that can
/// tell that mutant from a correct pin, and the same fixture as the whitespace
/// test above with the padding removed is what makes the pair a pair: one
/// character of difference decides between an excerpt and a refusal.
#[test]
fn source_around_admits_a_byte_identical_passage_text() {
    const STORED: &str = "Ставка залишається незмінною протягом усього строку.";

    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "a".repeat(64);
    let chunk = state
        .with_index(|db| Ok::<_, mnema_index::Error>(write_one_document(db, &doc, STORED)))
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": STORED,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(
        v["kind"],
        json!("excerpt"),
        "the echoed passage is byte-identical to the stored chunk, so the pin must let it \
         through — a pin that refuses everything passes every other test in this file: {v}"
    );
    assert!(
        v.get("reason").is_none(),
        "an excerpt carries no refusal reason: {v}"
    );
    assert_eq!(
        v["blocks"][0]["text"],
        json!(STORED),
        "the excerpt must carry the passage's own paragraph: {v}"
    );
    // The false direction of both flags, which no other test through the IPC
    // asserts: this document is one block, so there is nothing either side. A
    // flag hardcoded `true` passes the happy path above and only this.
    assert_eq!(v["hasMoreBefore"], json!(false), "{v}");
    assert_eq!(v["hasMoreAfter"], json!(false), "{v}");
}

/// The other cause, and without it `GoneReason::NoSuchChunk` is a variant
/// nothing produces. `clear_document_content` cascades the document's pages,
/// blocks and chunks away (`Db::clear_document_content_in`) and a rebuild has not landed
/// yet — the gap a watcher's re-index leaves open between two IPC calls.
#[test]
fn source_around_reports_no_such_chunk_when_nothing_carries_the_id() {
    const ORIGINAL: &str = "Обсяг зібрання становить дванадцять тисяч аркушів.";

    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "8".repeat(64);
    let chunk = state
        .with_index(|db| {
            let chunk = write_one_document(db, &doc, ORIGINAL);
            db.clear_document_content(&doc)?;
            Ok::<_, mnema_index::Error>(chunk)
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": ORIGINAL,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("gone"), "nothing carries that id: {v}");
    assert_eq!(
        v["reason"]["kind"],
        json!("noSuchChunk"),
        "no chunk carries that id at all, which is a different cause from an id handed to other \
         text — collapsing the two loses the only thing that tells a rebuild-in-flight from a \
         reused id: {v}"
    );
    assert!(
        v.get("blocks").is_none(),
        "a refusal must carry no text at all: {v}"
    );
}

/// [assert-both-directions] at the level of the pin itself. A pin written as
/// "the stored text *contains* the passage" would accept `""` and match every
/// chunk in the index — an id-reuse check satisfied by absence. The
/// comparison is exact equality, so an empty passage against a live chunk
/// with non-empty text is a refusal, not an excerpt.
#[test]
fn source_around_refuses_an_empty_passage_text_rather_than_matching_anything() {
    const LIVE: &str = "Загальна ціна обчислюється множенням ставки і кількості аркушів.";

    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "9".repeat(64);
    let chunk = state
        .with_index(|db| Ok::<_, mnema_index::Error>(write_one_document(db, &doc, LIVE)))
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": "",
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(
        v["kind"],
        json!("gone"),
        "an empty passage matched a live chunk, so the pin is satisfied by absence: {v}"
    );
    assert_eq!(
        v["reason"]["kind"],
        json!("idReused"),
        "the chunk is there and its text is not the empty passage, which is the reused-id \
         cause, not the missing-chunk one: {v}"
    );
    assert!(
        v.get("blocks").is_none(),
        "a refusal must carry no text at all: {v}"
    );
}

// ------------------------------------------------- source_around, the excerpt

/// The five paragraphs the launcher's right card paints around a citation,
/// Cyrillic because the product is (`…launcher-mockup.html:289-294`). Index 2
/// is the anchor, so a radius of 1 leaves exactly one paragraph over on each
/// side and both `hasMore` flags must be true.
const PARAGRAPHS: [&str; 5] = [
    "Обсяг зібрання становить дванадцять тисяч аркушів.",
    "Ціна оцифрування одного аркуша становить дві гривні.",
    "Загальна ціна обчислюється множенням ставки і кількості аркушів.",
    "Ставка залишається незмінною протягом усього строку договору.",
    "Сторони узгоджують графік передавання матеріалів окремо.",
];

/// A second document, written **before** the one under test, and it is not
/// decoration: it is what makes `reading_window`'s document term falsifiable
/// through the IPC.
///
/// The `mnema-index` suite learned this the expensive way — with one document
/// in the fixture, `WHERE b.document_id = ?1` and `(… OR 1 = 1)` select the
/// same rows, and the mutant that survives is literally "return another
/// document's paragraphs under the user's citation", the hazard this whole
/// command exists to refuse. Its `reading_order` values collide with the real
/// document's, so a window that forgot the document term interleaves them
/// rather than quietly returning the same list.
///
/// Written first for a second reason: pages inserted first take the low
/// `page.id` values, so the real document's page ids stop coinciding with its
/// page numbers and `p.page_no` swapped for `p.id` stops being invisible.
fn write_decoy_document(db: &mnema_index::Db) {
    let decoy = "d".repeat(64);
    db.insert_document(&decoy, "text/plain", 1, SourceKind::Document)
        .unwrap();
    let page = db.insert_page(&decoy, 1, "native:txt", None).unwrap();
    for i in 1..=6 {
        db.insert_block(
            page,
            &Block {
                block_type: BlockType::Paragraph,
                reading_order: i,
                language: None,
                text: format!("ПІДСТАВНИЙ АБЗАЦ {i}"),
                line_start: None,
                line_end: None,
            },
        )
        .unwrap();
    }
}

/// One page of paragraphs with a chunk pinned to a slice of one of them.
///
/// `block_start` and `n_chars` are **character** offsets, never byte ones —
/// every offset this pipeline emits is (`crates/mnema-chunk/src/view.rs:5-9`),
/// and the paragraphs above are Cyrillic precisely so a byte implementation
/// cannot pass. Returns the chunk id and the chunk's own text: the
/// `passageText` a citation echoes back.
fn write_paragraph_document(
    db: &mnema_index::Db,
    id: &str,
    paragraphs: &[&str],
    anchor_ix: usize,
    block_start: u32,
    n_chars: u32,
) -> (i64, String) {
    db.insert_document(id, "text/plain", 1, SourceKind::Document)
        .unwrap();
    let page = db
        .insert_page(id, 1, "native:txt", Some("Розділ перший"))
        .unwrap();
    let blocks: Vec<i64> = paragraphs
        .iter()
        .enumerate()
        .map(|(i, text)| {
            db.insert_block(
                page,
                &Block {
                    block_type: BlockType::Paragraph,
                    reading_order: i as i64 + 1,
                    language: None,
                    text: (*text).to_string(),
                    line_start: None,
                    line_end: None,
                },
            )
            .unwrap()
        })
        .collect();

    let passage: String = paragraphs[anchor_ix]
        .chars()
        .skip(block_start as usize)
        .take(n_chars as usize)
        .collect();
    let chunk = db
        .insert_chunk(
            id,
            0,
            &passage,
            &Locator {
                spans: vec![Segment {
                    block_id: blocks[anchor_ix],
                    start: 0,
                    end: passage.chars().count() as u32,
                    block_start,
                }],
                coordinate: Coordinate::Page { number: 1 },
            },
            SourceKind::Document,
        )
        .unwrap();
    db.set_document_status(id, mnema_index::DocumentStatus::Indexed)
        .unwrap();
    (chunk, passage)
}

/// The right card's own shape (`…launcher-mockup.html:289-294`): the paragraph
/// before, the passage's paragraph, and the paragraph after.
///
/// Asserted on **content**, not on length — three blocks of the wrong three
/// paragraphs is the failure this command exists to prevent, and a
/// `blocks.len() == 3` assertion cannot tell the two apart. The `hasMore`
/// flags are asserted true here and false in the clamp test below; a flag
/// hardcoded either way passes one of that pair.
///
/// The camelCase assertions are not the wire test (that is Task 5.4's) but
/// they reach the same fact from the first excerpt anything constructs:
/// `rename_all` on an *enum* renames variants only, so without
/// `rename_all_fields` this ships `document_id` and `has_more_before` inside a
/// camelCase payload.
/// The passage is on page 2 of three, and the window crosses both boundaries.
///
/// Every other fixture that reaches `source_around` puts its document on a
/// single page, so the seam `chunk_anchor.page_no` → `reading_window(?2)` was
/// only ever exercised with a page number handed in by a unit test. This is the
/// sixth instance of the cycle's one recurring gap — the fixture not building
/// the state the code branches on — and the one the branch review predicted.
#[test]
fn source_around_crosses_page_boundaries_on_a_real_multi_page_document() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "8".repeat(64);
    let passage = PARAGRAPHS[0].to_string();
    let chunk = state
        .with_index(|db| {
            // The decoy first, so the real document's page ids are nowhere near
            // its page numbers.
            write_decoy_document(db);
            db.insert_document(&doc, "text/plain", 1, SourceKind::Document)?;
            let mut anchor = None;
            for page_no in 1..=3 {
                let page = db.insert_page(&doc, page_no, "native:txt", None)?;
                for i in 1..=2 {
                    let text = format!("p{page_no}b{i}");
                    let block = db.insert_block(
                        page,
                        &Block {
                            block_type: BlockType::Paragraph,
                            reading_order: i,
                            language: None,
                            text: if page_no == 2 && i == 1 {
                                passage.clone()
                            } else {
                                text
                            },
                            line_start: None,
                            line_end: None,
                        },
                    )?;
                    if page_no == 2 && i == 1 {
                        anchor = Some(block);
                    }
                }
            }
            let chunk = db.insert_chunk(
                &doc,
                0,
                &passage,
                &Locator {
                    spans: vec![Segment {
                        block_id: anchor.expect("the anchor block"),
                        start: 0,
                        end: passage.chars().count() as u32,
                        block_start: 0,
                    }],
                    coordinate: Coordinate::Page { number: 2 },
                },
                SourceKind::Document,
            )?;
            db.set_document_status(&doc, mnema_index::DocumentStatus::Indexed)?;
            Ok::<_, mnema_index::Error>(chunk)
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 2,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    // Two blocks back reaches page 1; two forward reaches page 3.
    let pages: Vec<i64> = v["blocks"]
        .as_array()
        .expect("a blocks array")
        .iter()
        .map(|b| b["pageNo"].as_i64().expect("a pageNo"))
        .collect();
    assert_eq!(
        pages,
        vec![1, 1, 2, 2, 3],
        "the window must cross both page boundaries in document reading order: {v}"
    );
    assert_eq!(v["blocks"][2]["text"], json!(passage), "{v}");
    assert_eq!(v["hasMoreBefore"], json!(false), "{v}");
    assert_eq!(v["hasMoreAfter"], json!(true), "p3b2 is beyond: {v}");
}

/// An **asymmetric** window: nothing before the passage, more after it.
///
/// Every other IPC fixture in this file returns the two `hasMore` flags with
/// the same value — `(false, false)`, `(true, true)`, `(true, true)` — so
/// swapping the two fields where the excerpt is assembled survived the whole
/// suite. The index-level test that *is* asymmetric never crosses the mapping
/// in `tree.rs`, so it cannot catch it either.
///
/// The loss is not abstract: the mockup's leading "…" is drawn from these
/// flags, and a swap paints it on the side where there is nothing more.
#[test]
fn source_around_reports_more_after_but_not_before_at_the_start_of_a_document() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "7".repeat(64);
    // Anchor on paragraph 1 — nothing precedes it — with a radius of 1 while
    // three paragraphs follow.
    let (chunk, passage) = state
        .with_index(|db| {
            write_decoy_document(db);
            Ok::<_, mnema_index::Error>(write_paragraph_document(
                db,
                &doc,
                &PARAGRAPHS,
                0,
                0,
                PARAGRAPHS[0].chars().count() as u32,
            ))
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    assert_eq!(
        v["hasMoreBefore"],
        json!(false),
        "the anchor is the document's first block, so there is nothing before it: {v}"
    );
    assert_eq!(
        v["hasMoreAfter"],
        json!(true),
        "paragraphs 3..5 are beyond a radius of 1: {v}"
    );
    let texts: Vec<&str> = v["blocks"]
        .as_array()
        .expect("a blocks array")
        .iter()
        .map(|b| b["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, vec![PARAGRAPHS[0], PARAGRAPHS[1]], "{v}");
}

#[test]
fn source_around_returns_the_paragraphs_around_a_cited_passage() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "1".repeat(64);
    let (chunk, passage) = state
        .with_index(|db| {
            write_decoy_document(db);
            Ok::<_, mnema_index::Error>(write_paragraph_document(
                db,
                &doc,
                &PARAGRAPHS,
                2,
                0,
                PARAGRAPHS[2].chars().count() as u32,
            ))
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    let texts: Vec<&str> = v["blocks"]
        .as_array()
        .expect("a blocks array")
        .iter()
        .map(|b| b["text"].as_str().expect("a block text"))
        .collect();
    assert_eq!(
        texts,
        vec![PARAGRAPHS[1], PARAGRAPHS[2], PARAGRAPHS[3]],
        "the card must paint the passage's own paragraph with one either side, \
         in document reading order: {v}"
    );
    assert_eq!(
        v["hasMoreBefore"],
        json!(true),
        "paragraph 1 is beyond: {v}"
    );
    assert_eq!(v["hasMoreAfter"], json!(true), "paragraph 5 is beyond: {v}");
    assert_eq!(v["documentId"], json!(doc), "{v}");
    assert_eq!(v["sectionTitle"], json!("Розділ перший"), "{v}");

    // camelCase, both directions. Every one of these is snake_case in Rust and
    // would cross unrenamed without `rename_all_fields` on the enum.
    assert!(v.get("document_id").is_none(), "{v}");
    assert!(v.get("has_more_before").is_none(), "{v}");
    assert!(v.get("has_more_after").is_none(), "{v}");
    assert!(v.get("section_title").is_none(), "{v}");
    let block = &v["blocks"][0];
    assert!(block["blockId"].is_i64(), "{v}");
    assert_eq!(block["kind"], json!("paragraph"), "{v}");
    assert_eq!(block["pageNo"], json!(1), "{v}");
    // The VALUE, not merely the type. `readingOrder` is what PR 6 may sort or
    // label by, and nothing else in the tree asserted it: returning `block.id`
    // in its place compiles, leaves the order, the texts and `pageNo` correct,
    // and ships rowids as reading order.
    let orders: Vec<i64> = v["blocks"]
        .as_array()
        .expect("a blocks array")
        .iter()
        .map(|b| b["readingOrder"].as_i64().expect("an integer readingOrder"))
        .collect();
    assert_eq!(
        orders,
        vec![2, 3, 4],
        "the window is paragraphs 2..4 of one page, so these are their reading orders: {v}"
    );
    assert!(block.get("block_id").is_none(), "{v}");
    assert!(block.get("page_no").is_none(), "{v}");
    assert!(block.get("reading_order").is_none(), "{v}");
}

/// A chunk that spans **two** blocks — the state no fixture in this cycle
/// built through the IPC, and two mutants lived in the gap.
///
/// The chunker can end a chunk mid-paragraph and carry it into the next, so
/// `char_span` holds one `Segment` per source block (`mnema-core/src/locator.rs`)
/// and `ChunkAnchor` reports a `first_reading_order`/`last_reading_order`
/// **range**. Every other `source_around` test uses a single-segment chunk,
/// where first == last and one span is all there is — so collapsing the range
/// to its first block, or truncating `spans` to its first element, changed
/// nothing any test could see. Both are real losses: the first drops the
/// paragraph the passage ends in, the second drops a highlight the card is
/// supposed to paint.
///
/// The schema pins both anchor blocks to one page (`chunk_span_blocks_bi`), so
/// this is the widest anchor the index can hold, not an invented one.
#[test]
fn source_around_covers_every_block_a_multi_block_chunk_spans() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    // Blocks 2 and 4 of PARAGRAPHS, joined, with a **whitespace-only block
    // between them**: the chunk starts inside block 2 and runs to the end of
    // block 4, so `chunk_anchor` reports the range 2..4 and the anchor query's
    // own `BETWEEN` covers the blank row in the middle.
    //
    // 🔴 That blank row is why the anchor query carries the whitespace
    // predicate too, and until this fixture nothing exercised it: every other
    // test anchors on a single block, where `BETWEEN 3 AND 3` cannot contain a
    // neighbour of any kind. The chunker skips blank blocks
    // (`mnema-chunk/src/lib.rs`), so a chunk legitimately spans *across* one —
    // and without the predicate the excerpt would show it inside the quotation
    // itself.
    let head: String = PARAGRAPHS[1].chars().skip(17).collect();
    let tail = PARAGRAPHS[3].to_string();
    let passage = format!("{head}{tail}");
    let head_chars = head.chars().count() as u32;
    let tail_chars = tail.chars().count() as u32;

    let doc = "6".repeat(64);
    let chunk = state
        .with_index(|db| {
            write_decoy_document(db);
            db.insert_document(&doc, "text/plain", 1, SourceKind::Document)?;
            let page = db.insert_page(&doc, 1, "native:txt", Some("Розділ перший"))?;
            // PARAGRAPHS, but with the third row replaced by a blank line —
            // what the text reader stores for exactly that input.
            //
            // ⚠️ A tab among the spaces, deliberately: SQLite's one-argument
            // `trim` strips spaces only, so a spaces-only row is excluded even
            // by the broken predicate and this fixture would measure nothing.
            let rows: Vec<&str> = vec![
                PARAGRAPHS[0],
                PARAGRAPHS[1],
                " \t ",
                PARAGRAPHS[3],
                PARAGRAPHS[4],
            ];
            let blocks: Vec<i64> = rows
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    db.insert_block(
                        page,
                        &Block {
                            block_type: BlockType::Paragraph,
                            reading_order: i as i64 + 1,
                            language: None,
                            text: (*text).to_string(),
                            line_start: None,
                            line_end: None,
                        },
                    )
                })
                .collect::<Result<_, _>>()?;
            let chunk = db.insert_chunk(
                &doc,
                0,
                &passage,
                &Locator {
                    spans: vec![
                        Segment {
                            block_id: blocks[1],
                            start: 0,
                            end: head_chars,
                            block_start: 17,
                        },
                        Segment {
                            block_id: blocks[3],
                            start: head_chars,
                            end: head_chars + tail_chars,
                            block_start: 0,
                        },
                    ],
                    coordinate: Coordinate::None,
                },
                SourceKind::Document,
            )?;
            db.set_document_status(&doc, mnema_index::DocumentStatus::Indexed)?;
            Ok::<_, mnema_index::Error>(chunk)
        })
        .unwrap();

    // radius 1: one block either side of the anchor's TWO blocks.
    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");

    // Both anchor blocks are in the window, and so is one block either side.
    // Asserted as an exact list: "contains paragraph 3" is satisfied by a
    // window that dropped paragraph 2, which is the mutant.
    let texts: Vec<&str> = v["blocks"]
        .as_array()
        .expect("blocks")
        .iter()
        .map(|b| b["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        texts,
        vec![PARAGRAPHS[0], PARAGRAPHS[1], PARAGRAPHS[3], PARAGRAPHS[4]],
        "the window must span both of the anchor's blocks and skip the blank row between \
         them — a blank inside the quotation is the anchor query's own predicate: {v}"
    );

    // One span per source block, each measuring into its OWN block.
    let spans = v["spans"].as_array().expect("spans");
    assert_eq!(
        spans.len(),
        2,
        "a chunk over two blocks needs two spans, or the card paints one \
         highlight where the passage has two: {v}"
    );
    assert_eq!(spans[0]["blockStart"], json!(17), "{v}");
    assert_eq!(spans[1]["blockStart"], json!(0), "{v}");
    assert_eq!(spans[0]["end"], json!(head_chars), "{v}");
    assert_eq!(spans[1]["end"], json!(head_chars + tail_chars), "{v}");

    // And the slices they name, in characters, reassemble the passage.
    let slice_of = |ix: usize, block_ix: usize| -> String {
        let sp = &spans[ix];
        let start = sp["blockStart"].as_u64().unwrap() as usize;
        let len = (sp["end"].as_u64().unwrap() - sp["start"].as_u64().unwrap()) as usize;
        v["blocks"][block_ix]["text"]
            .as_str()
            .unwrap()
            .chars()
            .skip(start)
            .take(len)
            .collect()
    };
    assert_eq!(
        format!("{}{}", slice_of(0, 1), slice_of(1, 2)),
        passage,
        "the two spans must reassemble the passage out of their own blocks: {v}"
    );
}

/// 🔴 The span that reaches the wire is measured in **characters**, and this
/// test is red against a byte implementation rather than silently green.
///
/// `&str[a..b]` indexes bytes; every offset this pipeline emits is a character
/// offset — `mnema-chunk` says so in the words of a defect already paid for
/// once, "a byte-offset implementation passes every test written over ASCII
/// and then shows itself as a citation quoting the wrong slice of the first
/// Ukrainian chunk" (`crates/mnema-chunk/src/view.rs:5-9`). The paragraph here
/// is Cyrillic, so every character before the passage is two bytes and the two
/// readings cannot coincide.
///
/// The assertion is not "the number came back": it is that the slice
/// `blockStart` names inside the block's own text **is** the passage. That is
/// what the highlight is painted from, and a number nothing is measured with
/// is not evidence.
#[test]
fn source_around_spans_measure_into_the_block_in_characters() {
    // "Ціна оцифрування " is 17 characters and 32 bytes, so a byte reading of
    // `blockStart` lands in the middle of a letter rather than at "одного".
    const BLOCK_START: u32 = 17;
    const N_CHARS: u32 = 13;

    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "2".repeat(64);
    let (chunk, passage) = state
        .with_index(|db| {
            Ok::<_, mnema_index::Error>(write_paragraph_document(
                db,
                &doc,
                &PARAGRAPHS,
                1,
                BLOCK_START,
                N_CHARS,
            ))
        })
        .unwrap();
    assert_eq!(
        passage, "одного аркуша",
        "the fixture must cut the passage out of the middle of its paragraph, \
         or `blockStart` is zero and measures nothing"
    );
    assert_ne!(
        BLOCK_START as usize,
        PARAGRAPHS[1]
            .char_indices()
            .nth(BLOCK_START as usize)
            .unwrap()
            .0,
        "the character offset must differ from the byte offset, or this test \
         is green against a byte implementation"
    );

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    let span = &v["spans"][0];
    assert_eq!(span["blockStart"], json!(BLOCK_START), "{v}");
    assert_eq!(span["start"], json!(0), "{v}");
    assert_eq!(span["end"], json!(N_CHARS), "{v}");
    // The wire mirror, both directions: `Segment` itself is persisted and
    // crosses as `block_start`, which is why `WireSegment` exists.
    assert!(span.get("block_start").is_none(), "{v}");
    assert!(span.get("block_id").is_none(), "{v}");

    // The span names the anchor's own block, and the slice it names inside
    // that block's text is the passage — in characters.
    let anchor = v["blocks"]
        .as_array()
        .expect("a blocks array")
        .iter()
        .find(|b| b["blockId"] == span["blockId"])
        .expect("the span must name a block the excerpt actually returned");
    let text = anchor["text"].as_str().unwrap();
    let start = span["blockStart"].as_u64().unwrap() as usize;
    let len = (span["end"].as_u64().unwrap() - span["start"].as_u64().unwrap()) as usize;
    let painted: String = text.chars().skip(start).take(len).collect();
    assert_eq!(
        painted, passage,
        "the highlight `blockStart` paints is not the passage: {text:?}"
    );
}

/// A watched root that is a real directory, with a real file in it and a
/// `path` row recorded from a **real** `mnema_walk::stat`.
///
/// Neither number is written by hand, and that is the rule rather than a
/// preference: a fixture that fabricates the value it later asserts on
/// measures nothing, which is exactly how PR 4's wrong-column defect hid
/// behind its own test. `mtime` is **nanoseconds** (`mnema-core/src/lib.rs:22-26`),
/// and `mnema_walk::stat` (`mnema-walk/src/lib.rs:371`) is the single place
/// that conversion is done — the same function the command itself calls, so
/// the fixture and the code under test cannot disagree about the unit.
fn record_real_file(
    db: &mnema_index::Db,
    root_dir: &std::path::Path,
    root_id: i64,
    relative_path: &str,
    document_id: &str,
    contents: &str,
) -> mnema_core::OnDisk {
    let full = root_dir.join(relative_path);
    std::fs::write(&full, contents).expect("the fixture must be able to write its own corpus");
    let disk = mnema_walk::stat(&full).expect("a file just written must be statable");
    db.insert_path(root_id, relative_path, document_id, disk, "text", 1)
        .expect("insert_path");
    disk
}

/// A real corpus directory beside the index, and the watched root row for it.
fn watched_corpus(db: &mnema_index::Db, dir: &std::path::Path) -> (PathBuf, i64) {
    let corpus = dir.join("corpus");
    std::fs::create_dir_all(&corpus).expect("the corpus directory");
    let root = db
        .insert_watched_root(corpus.to_str().expect("a UTF-8 temporary path"))
        .expect("insert_watched_root");
    (corpus, root)
}

/// The ordinary case: the cited path still names this document and the file on
/// disk still measures the way it did when it was indexed.
///
/// The cheap arm's own comparison, not a new one —
/// `recorded.size_bytes == disk.size_bytes && recorded.mtime == disk.mtime`
/// is what `mnema-ingest` already asks (`mnema-ingest/src/lib.rs:295-296`).
#[test]
fn source_around_reports_current_when_the_file_still_matches_what_was_indexed() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "3".repeat(64);
    let (chunk, passage) = state
        .with_index(|db| {
            let (corpus, root) = watched_corpus(db, dir.path());
            let seeded = write_paragraph_document(
                db,
                &doc,
                &PARAGRAPHS,
                2,
                0,
                PARAGRAPHS[2].chars().count() as u32,
            );
            record_real_file(
                db,
                &corpus,
                root,
                "dohov-01.md",
                &doc,
                &PARAGRAPHS.join("\n\n"),
            );
            Ok::<_, mnema_index::Error>(seeded)
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "citedRelativePath": "dohov-01.md",
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    assert_eq!(
        v["freshness"]["kind"],
        json!("current"),
        "the path still names this document and the file has not moved since it was indexed: {v}"
    );
}

/// The document has no `path` row at all — indexed from inside an archive, or
/// its last copy on disk was deleted (`write.rs:76-79`). There is nothing to
/// compare against, and inventing a comparison is how a stale excerpt gets
/// shown as fresh.
///
/// Both directions on the same fixture: the excerpt still carries its text,
/// because "no path" is a statement about provenance, not a refusal.
#[test]
fn source_around_reports_no_path_when_the_document_has_none() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "4".repeat(64);
    let (chunk, passage) = state
        .with_index(|db| {
            Ok::<_, mnema_index::Error>(write_paragraph_document(
                db,
                &doc,
                &PARAGRAPHS,
                2,
                0,
                PARAGRAPHS[2].chars().count() as u32,
            ))
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    assert_eq!(v["freshness"]["kind"], json!("noPath"), "{v}");
    assert_eq!(
        v["blocks"][1]["text"],
        json!(PARAGRAPHS[2]),
        "an unknown provenance is not a refusal — the indexed text still comes back: {v}"
    );
    // The excerpt deliberately carries no `relativePath`: no read method can
    // produce one for it, so the field could only echo the caller's own input
    // back and disagree with nothing.
    assert!(v.get("relativePath").is_none(), "{v}");
    assert!(v.get("relative_path").is_none(), "{v}");
}

/// The `path` row still names this document, but nothing is at that location
/// any more — a delete no walk has caught up with yet. The excerpt is honest
/// about what it cannot measure rather than reporting the last thing it knew.
#[test]
fn source_around_reports_file_missing_when_the_file_is_gone_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "5".repeat(64);
    let (chunk, passage) = state
        .with_index(|db| {
            let (corpus, root) = watched_corpus(db, dir.path());
            let seeded = write_paragraph_document(
                db,
                &doc,
                &PARAGRAPHS,
                2,
                0,
                PARAGRAPHS[2].chars().count() as u32,
            );
            record_real_file(
                db,
                &corpus,
                root,
                "dohov-01.md",
                &doc,
                &PARAGRAPHS.join("\n\n"),
            );
            // Indexed, then deleted. The `path` row survives until a walk
            // notices, which is the state this verdict is for.
            std::fs::remove_file(corpus.join("dohov-01.md")).expect("remove the corpus file");
            Ok::<_, mnema_index::Error>(seeded)
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "citedRelativePath": "dohov-01.md",
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    assert_eq!(
        v["freshness"]["kind"],
        json!("fileMissing"),
        "nothing is at that path, so the file cannot be measured at all: {v}"
    );
    assert_eq!(
        v["blocks"][1]["text"],
        json!(PARAGRAPHS[2]),
        "an unmeasurable file is not a refusal — the indexed text still comes back: {v}"
    );
}

/// One document, a real watched corpus, and the real file the `path` row was
/// recorded from. Returns the chunk id, the passage a citation would echo
/// back, the corpus directory and the `OnDisk` numbers the index now holds —
/// all measured, none written by hand.
fn seed_excerpt_with_file(
    state: &AppState,
    dir: &std::path::Path,
    doc: &str,
) -> (i64, String, PathBuf, mnema_core::OnDisk) {
    state
        .with_index(|db| {
            let (corpus, root) = watched_corpus(db, dir);
            let (chunk, passage) = write_paragraph_document(
                db,
                doc,
                &PARAGRAPHS,
                2,
                0,
                PARAGRAPHS[2].chars().count() as u32,
            );
            let disk = record_real_file(
                db,
                &corpus,
                root,
                "dohov-01.md",
                doc,
                &PARAGRAPHS.join("\n\n"),
            );
            Ok::<_, mnema_index::Error>((chunk, passage, corpus, disk))
        })
        .unwrap()
}

/// Sets a file's modification time to exactly `when`.
///
/// Used to hold **one** of the two recorded numbers still while the other
/// moves. Without it a fixture cannot separate the two halves of
/// `size_bytes == … && mtime == …`: every ordinary edit changes both at once,
/// and a mutant that dropped one operand would survive a fixture that changed
/// both — the classic `&&` mutant, and the reason the two tests below exist
/// as a pair rather than as one "the file changed" test.
fn set_mtime(path: &std::path::Path, when: std::time::SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open for set_times")
        .set_times(std::fs::FileTimes::new().set_modified(when))
        .expect("set_times");
}

/// §12's own case — "якщо файл змінився після індексації … повертати те, що в
/// індексі (або позначку «файл змінився»), а **не** мовчазно інший текст" —
/// and the everyday one: the user edits a file and asks a question a minute
/// later, before any walk has reached it. Invisible to an index-only check,
/// which is why the `stat` exists at all.
///
/// **Only the size moves here.** The mtime is put back to what it was, so this
/// test is red against an implementation that compares mtime alone, and its
/// twin below is red against one that compares size alone. An edit changes
/// both at once, so a single test cannot tell either mutant from a correct
/// comparison.
///
/// Both halves of the §12 sentence are asserted: the marker **and** that the
/// indexed text still comes back. A test asserting only the marker leaves the
/// more important half unpinned.
#[test]
fn source_around_reports_file_changed_when_only_the_size_moved() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "6".repeat(64);
    let (chunk, passage, corpus, recorded) = seed_excerpt_with_file(&state, dir.path(), &doc);

    let full = corpus.join("dohov-01.md");
    let indexed_at = std::fs::metadata(&full).unwrap().modified().unwrap();
    std::fs::write(
        &full,
        format!("{}\n\nДодано абзац.", PARAGRAPHS.join("\n\n")),
    )
    .unwrap();
    set_mtime(&full, indexed_at);

    let now = mnema_walk::stat(&full).expect("the edited file must still be statable");
    assert_ne!(
        now.size_bytes, recorded.size_bytes,
        "the fixture must move the size, or it tests nothing"
    );
    assert_eq!(
        now.mtime, recorded.mtime,
        "the fixture must hold the mtime still, or a mutant that dropped the size half survives it"
    );

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "citedRelativePath": "dohov-01.md",
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    assert_eq!(
        v["freshness"]["kind"],
        json!("fileChanged"),
        "the file is a different size from the one that was indexed: {v}"
    );
    assert_eq!(
        v["blocks"][1]["text"],
        json!(PARAGRAPHS[2]),
        "§12 asks for the indexed text *and* the marker; this dropped the text: {v}"
    );
}

/// The twin of the test above, and the operand it holds still is the other
/// one: the file is rewritten to exactly the same length and only its mtime
/// moves. Red against an implementation comparing size alone.
///
/// A same-length edit is not exotic — a corrected digit, a swapped word — and
/// it is precisely what nanosecond mtimes exist to catch
/// (`mnema-core/src/lib.rs:22-26`). The mtime is set explicitly rather than
/// left to the clock, so the test does not depend on the filesystem's
/// timestamp granularity.
#[test]
fn source_around_reports_file_changed_when_only_the_mtime_moved() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "7".repeat(64);
    let (chunk, passage, corpus, recorded) = seed_excerpt_with_file(&state, dir.path(), &doc);

    let full = corpus.join("dohov-01.md");
    let original = PARAGRAPHS.join("\n\n");
    // One Cyrillic letter for another of the same UTF-8 width, so the file's
    // length does not move and only its mtime can. Asserted below rather than
    // trusted — this fixture's whole job is to isolate one of the two operands
    // `decide_freshness` compares.
    let edited = original.replacen('і', "и", 1);
    assert_eq!(
        edited.len(),
        original.len(),
        "the edit must not change the byte length, or this test cannot tell \
         a moved mtime from a moved size"
    );
    std::fs::write(&full, &edited).unwrap();
    let indexed_at = std::fs::metadata(&full).unwrap().modified().unwrap();
    set_mtime(&full, indexed_at - Duration::from_secs(3600));

    let now = mnema_walk::stat(&full).expect("the edited file must still be statable");
    assert_eq!(
        now.size_bytes, recorded.size_bytes,
        "the fixture must hold the size still, or a mutant that dropped the mtime half survives it"
    );
    assert_ne!(
        now.mtime, recorded.mtime,
        "the fixture must move the mtime, or it tests nothing"
    );

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "citedRelativePath": "dohov-01.md",
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    assert_eq!(
        v["freshness"]["kind"],
        json!("fileChanged"),
        "the file was written after it was indexed, at the same length: {v}"
    );
    assert_eq!(
        v["blocks"][1]["text"],
        json!(PARAGRAPHS[2]),
        "§12 asks for the indexed text *and* the marker; this dropped the text: {v}"
    );
}

/// `Freshness::Reindexed` is reachable, and this fixture is what makes it so.
///
/// A walk of an edited file calls `repoint` — `delete_path` then `insert_path`
/// (`mnema-ingest/src/lib.rs:657`) — and then `forget_if_unnamed`, which
/// deletes the displaced document outright when no path is left naming it
/// (`:685`, `:862`). So the variant exists only while **another copy** keeps
/// the old document alive, which is what `copy.md` is here.
///
/// ⚠️ The surviving copy sits at a **different** `relative_path` on purpose.
/// Put it at the same relative path under a second root and the first
/// resolution branch finds a row that still names the anchor's document, the
/// verdict is `Current`, and `Reindexed` is never reached — a defect in the
/// fixture that would read as a defect in the variant.
///
/// The edited file is written and `insert_path`'s numbers come from a real
/// `stat` of it, so an implementation that skipped the document comparison
/// and went straight to the filesystem would answer `Current` — this test is
/// red against exactly that, rather than only against a missing variant.
#[test]
fn source_around_reports_reindexed_when_the_cited_path_now_names_another_document() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "8".repeat(64);
    let edited_doc = "b".repeat(64);
    let (chunk, passage) = state
        .with_index(|db| {
            let (corpus, root) = watched_corpus(db, dir.path());
            let seeded = write_paragraph_document(
                db,
                &doc,
                &PARAGRAPHS,
                2,
                0,
                PARAGRAPHS[2].chars().count() as u32,
            );
            let original = PARAGRAPHS.join("\n\n");
            record_real_file(db, &corpus, root, "dohov-01.md", &doc, &original);
            // The second copy: same bytes, so with content addressing it is the
            // same document — which is what keeps `doc` alive past the
            // repoint. Its name sorts *before* the cited one, so a query that
            // stopped filtering on the relative path would find this row and
            // answer `Current`.
            record_real_file(db, &corpus, root, "copy.md", &doc, &original);

            // The walk: a new content hash, and the cited location repointed
            // at it. `doc` survives because `copy.md` still names it.
            db.insert_document(&edited_doc, "text/plain", 1, SourceKind::Document)?;
            db.set_document_status(&edited_doc, mnema_index::DocumentStatus::Indexed)?;
            db.delete_path(root, "dohov-01.md")?;
            record_real_file(
                db,
                &corpus,
                root,
                "dohov-01.md",
                &edited_doc,
                &format!("{original}\n\nДодано абзац."),
            );
            Ok::<_, mnema_index::Error>(seeded)
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "citedRelativePath": "dohov-01.md",
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    assert_eq!(
        v["freshness"]["kind"],
        json!("reindexed"),
        "the cited location names a different document now — a walk has already re-indexed it: {v}"
    );
    assert_eq!(
        v["documentId"],
        json!(doc),
        "the excerpt's provenance is the document the blocks came from, not the one the path \
         names now: {v}"
    );
    assert_eq!(
        v["blocks"][1]["text"],
        json!(PARAGRAPHS[2]),
        "what is shown is what was indexed; the marker says so rather than the text vanishing: {v}"
    );
}

/// The other half of the pair, and the case users actually hit: **one** copy,
/// edited. `Reindexed` must NOT be the answer — the chunk is already gone.
///
/// Without this the test above proves only that some fixture reaches the
/// variant, not that the ordinary case is classified correctly.
///
/// ⚠️ `delete_path` + `insert_path` do not remove the chunk by themselves.
/// What removes it is `forget_if_unnamed` calling `delete_document` when no
/// path is left (`mnema-ingest/src/lib.rs:862`) — and this test drives
/// `mnema-index` directly, with `mnema-ingest` not in the loop and that
/// function private, so the fixture calls `delete_document` itself. Omit it
/// and the anchor survives, the path names the new document, and the command
/// answers `Reindexed` — the test would fail against a correct implementation.
#[test]
fn source_around_reports_gone_when_the_single_copy_of_a_file_is_re_indexed() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "9".repeat(64);
    let edited_doc = "c".repeat(64);
    let (chunk, passage) = state
        .with_index(|db| {
            let (corpus, root) = watched_corpus(db, dir.path());
            let seeded = write_paragraph_document(
                db,
                &doc,
                &PARAGRAPHS,
                2,
                0,
                PARAGRAPHS[2].chars().count() as u32,
            );
            let original = PARAGRAPHS.join("\n\n");
            record_real_file(db, &corpus, root, "dohov-01.md", &doc, &original);

            db.insert_document(&edited_doc, "text/plain", 1, SourceKind::Document)?;
            db.set_document_status(&edited_doc, mnema_index::DocumentStatus::Indexed)?;
            db.delete_path(root, "dohov-01.md")?;
            record_real_file(
                db,
                &corpus,
                root,
                "dohov-01.md",
                &edited_doc,
                &format!("{original}\n\nДодано абзац."),
            );
            // Standing in for `forget_if_unnamed`: no path names `doc` any
            // more, so the pipeline deletes it, cascading its pages, blocks
            // and chunks away.
            db.delete_document(&doc)?;
            Ok::<_, mnema_index::Error>(seeded)
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "citedRelativePath": "dohov-01.md",
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(
        v["kind"],
        json!("gone"),
        "the ordinary single-copy edit deletes the displaced document outright, so there is no \
         passage left to be fresh or stale about: {v}"
    );
    assert_eq!(v["reason"]["kind"], json!("noSuchChunk"), "{v}");
    assert!(
        v.get("freshness").is_none(),
        "a refusal carries no freshness verdict — it has nothing to be a verdict about: {v}"
    );
}

/// A radius of zero is a bad argument, not a reason to show the user nothing:
/// it is clamped up to 1 and an excerpt comes back with a paragraph either
/// side.
///
/// Asserted as an **effect** — which paragraphs came back — rather than as
/// "a clamp function was called". Without the clamp the window is the anchor
/// block alone, which is a visibly different answer rather than an error.
#[test]
fn source_around_clamps_a_zero_radius_up_to_one_rather_than_returning_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "e".repeat(64);
    let (chunk, passage) = state
        .with_index(|db| {
            Ok::<_, mnema_index::Error>(write_paragraph_document(
                db,
                &doc,
                &PARAGRAPHS,
                2,
                0,
                PARAGRAPHS[2].chars().count() as u32,
            ))
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 0,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(
        v["kind"],
        json!("excerpt"),
        "a bad radius is not an error: {v}"
    );
    let texts: Vec<&str> = v["blocks"]
        .as_array()
        .expect("a blocks array")
        .iter()
        .map(|b| b["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        texts,
        vec![PARAGRAPHS[1], PARAGRAPHS[2], PARAGRAPHS[3]],
        "radius 0 must be read as 1 — one paragraph either side, not the passage's own alone: {v}"
    );
}

/// The other end: a client must not be able to ask for a whole book. The
/// window is bounded by `MAX_RADIUS`, and the bound is asserted by counting
/// what came back and naming its edges — unclamped, this document would come
/// back entire.
#[test]
fn source_around_clamps_an_enormous_radius_to_a_bounded_window() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let paragraphs: Vec<String> = (1..=60)
        .map(|i| format!("Абзац номер {i} цього договору."))
        .collect();
    let refs: Vec<&str> = paragraphs.iter().map(String::as_str).collect();
    let anchor_ix = 29;

    let doc = "f".repeat(64);
    let (chunk, passage) = state
        .with_index(|db| {
            Ok::<_, mnema_index::Error>(write_paragraph_document(
                db,
                &doc,
                &refs,
                anchor_ix,
                0,
                refs[anchor_ix].chars().count() as u32,
            ))
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 10_000,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    let texts: Vec<&str> = v["blocks"]
        .as_array()
        .expect("a blocks array")
        .iter()
        .map(|b| b["text"].as_str().unwrap())
        .collect();
    // MAX_RADIUS = 20: twenty before, the passage's own, twenty after.
    assert_eq!(
        texts.len(),
        41,
        "the window must be bounded however large a radius is asked for: {} blocks came back",
        texts.len()
    );
    assert_eq!(texts[0], refs[anchor_ix - 20], "the near edge of the clamp");
    assert_eq!(texts[40], refs[anchor_ix + 20], "the far edge of the clamp");
    // Both flags true: the clamp cut the window short on each side, and the
    // response must say so rather than let the card claim it has the whole
    // document.
    assert_eq!(v["hasMoreBefore"], json!(true), "{v}");
    assert_eq!(v["hasMoreAfter"], json!(true), "{v}");
}

/// Two roots share the cited path, and the answer is `noPath` **even though
/// the document could have disambiguated it**. That is the decision, not a
/// shortcut.
///
/// An earlier version of this branch narrowed the candidates by document and
/// answered `current` here. Owner review on PR #22 reproduced what that costs
/// in the case it cannot see: two roots holding the *same* document at one
/// path also answer `noPath`, and editing one copy leaves a single survivor —
/// so the same citation flips to `current`, growing confident exactly when the
/// cited copy may be the stale one. The two situations are shape-identical
/// from the index, so the blunt rule is the honest one.
///
/// What it costs is this test: a verdict that *could* have been right is now
/// withheld. The excerpt is still returned; only the freshness tag degrades to
/// "cannot tell".
#[test]
fn source_around_reports_no_path_when_two_roots_share_the_path_even_if_the_document_differs() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "3".repeat(64);
    let other = "4".repeat(64);
    let (chunk, passage) = state
        .with_index(|db| {
            let seeded = write_paragraph_document(
                db,
                &doc,
                &PARAGRAPHS,
                2,
                0,
                PARAGRAPHS[2].chars().count() as u32,
            );

            // Root A holds the cited document.
            let corpus_a = dir.path().join("corpus-a");
            std::fs::create_dir_all(&corpus_a).unwrap();
            let root_a = db.insert_watched_root(corpus_a.to_str().unwrap())?;
            record_real_file(
                db,
                &corpus_a,
                root_a,
                "dohov-01.md",
                &doc,
                &PARAGRAPHS.join("\n\n"),
            );

            // Root B holds a DIFFERENT file at the same relative path. The
            // document *could* pick root A here — and that is exactly the
            // narrowing owner review removed, because the situation where it
            // guesses wrong looks identical from the index.
            let corpus_b = dir.path().join("corpus-b");
            std::fs::create_dir_all(&corpus_b).unwrap();
            let root_b = db.insert_watched_root(corpus_b.to_str().unwrap())?;
            db.insert_document(&other, "text/plain", 1, SourceKind::Document)?;
            record_real_file(
                db,
                &corpus_b,
                root_b,
                "dohov-01.md",
                &other,
                "зовсім інший текст",
            );

            Ok::<_, mnema_index::Error>(seeded)
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "citedRelativePath": "dohov-01.md",
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    assert_eq!(
        v["freshness"]["kind"],
        json!("noPath"),
        "two rows hold the cited path, so nothing here can say which copy the citation meant — \
         and picking the one whose document matches is exactly the confidence owner review \
         showed is unearned: {v}"
    );
}

/// The cited path exists nowhere: both branches come back empty.
///
/// The everyday shape of it is a row that vanished between the two IPC calls.
/// Nothing covered it — the neighbouring test named "no path" does not build
/// this state, it simply omits `citedRelativePath` — so the empty-fallback
/// arm had no oracle at all.
#[test]
fn source_around_reports_no_path_when_no_row_holds_the_cited_path() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "5".repeat(64);
    let (chunk, passage) = state
        .with_index(|db| {
            let seeded = write_paragraph_document(
                db,
                &doc,
                &PARAGRAPHS,
                2,
                0,
                PARAGRAPHS[2].chars().count() as u32,
            );
            let (corpus, root) = watched_corpus(db, dir.path());
            record_real_file(
                db,
                &corpus,
                root,
                "dohov-01.md",
                &doc,
                &PARAGRAPHS.join("\n\n"),
            );
            Ok::<_, mnema_index::Error>(seeded)
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "citedRelativePath": "nowhere.md",
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    assert_eq!(
        v["freshness"]["kind"],
        json!("noPath"),
        "no row holds that path under any root, so there is nothing to compare against and \
         nothing may be guessed: {v}"
    );
}

/// Two watched roots hold the same file at the same relative path, and a
/// citation carries no root (`bridge.rs:433`) — so there is no honest way to
/// say which of the two the verdict would be about.
///
/// `NoPath`, never a guess. A guessed root produces a **confident verdict
/// about the wrong file**, which is worse than admitting the lookup could not
/// be resolved: the card would draw "актуально" over a passage whose cited
/// copy had been edited an hour ago.
///
/// This fixture is here because the `mnema-index` suite learned the lesson the
/// expensive way — three mutants survived Task 5.1 for no reason other than
/// that every fixture held exactly one document and one root. Nothing else in
/// this file builds two roots holding one path, so without it the whole
/// ambiguity branch is unfalsifiable: `candidates.first()` would pass every
/// other test here.
#[test]
fn source_around_reports_no_path_when_two_roots_hold_the_cited_path() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "0".repeat(64);
    let (chunk, passage) = state
        .with_index(|db| {
            let seeded = write_paragraph_document(
                db,
                &doc,
                &PARAGRAPHS,
                2,
                0,
                PARAGRAPHS[2].chars().count() as u32,
            );
            let original = PARAGRAPHS.join("\n\n");
            for name in ["corpus-a", "corpus-b"] {
                let corpus = dir.path().join(name);
                std::fs::create_dir_all(&corpus).unwrap();
                let root = db.insert_watched_root(corpus.to_str().unwrap())?;
                // Same bytes under both roots: with content addressing that is
                // one document, so both `path` rows legally name it and the
                // first resolution branch returns two candidates.
                record_real_file(db, &corpus, root, "dohov-01.md", &doc, &original);
            }
            Ok::<_, mnema_index::Error>(seeded)
        })
        .unwrap();

    let v = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "citedRelativePath": "dohov-01.md",
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");

    assert_eq!(v["kind"], json!("excerpt"), "{v}");
    assert_eq!(
        v["freshness"]["kind"],
        json!("noPath"),
        "two roots hold this path and the citation names neither, so no verdict about the file \
         is honest — picking one is a confident answer about a file the user may not have \
         cited: {v}"
    );
    assert_eq!(
        v["blocks"][1]["text"],
        json!(PARAGRAPHS[2]),
        "an unresolvable location is not a refusal — the indexed text still comes back: {v}"
    );
}

/// Step 6: a citation minted since Task 1 carries `rootId`, and that is what
/// lets `cited_occupant` resolve an otherwise-ambiguous path without guessing.
/// Both directions on the same two-root fixture: naming the root reaches a
/// real verdict, and the same call without one still gets the honest `NoPath`
/// the test above pins — Task 2 must not have narrowed that fallback.
#[test]
fn source_around_uses_the_cited_root_to_resolve_the_occupant_when_two_roots_hold_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.state::<AppState>();
    state.open_index().expect("the index opens");
    let webview = main_webview(&app);

    let doc = "1".repeat(64);
    let other = "2".repeat(64);
    let (chunk, passage, root_a) = state
        .with_index(|db| {
            let (chunk, passage) = write_paragraph_document(
                db,
                &doc,
                &PARAGRAPHS,
                2,
                0,
                PARAGRAPHS[2].chars().count() as u32,
            );

            let corpus_a = dir.path().join("corpus-a");
            std::fs::create_dir_all(&corpus_a).unwrap();
            let root_a = db.insert_watched_root(corpus_a.to_str().unwrap())?;
            record_real_file(
                db,
                &corpus_a,
                root_a,
                "README.md",
                &doc,
                &PARAGRAPHS.join("\n\n"),
            );

            // A second root, same relative path, a DIFFERENT document — the
            // ambiguity the fallback in `cited_occupant` refuses rather than
            // guess through.
            let corpus_b = dir.path().join("corpus-b");
            std::fs::create_dir_all(&corpus_b).unwrap();
            let root_b = db.insert_watched_root(corpus_b.to_str().unwrap())?;
            db.insert_document(&other, "text/plain", 1, SourceKind::Document)?;
            record_real_file(
                db,
                &corpus_b,
                root_b,
                "README.md",
                &other,
                "інший документ у корені B",
            );

            Ok::<_, mnema_index::Error>((chunk, passage, root_a))
        })
        .unwrap();

    // The cited root names root A directly, so the ambiguity with root B
    // never has to be resolved.
    let named = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "citedRootId": root_a,
            "citedRelativePath": "README.md",
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");
    assert_eq!(named["kind"], json!("excerpt"), "{named}");
    assert_eq!(
        named["freshness"]["kind"],
        json!("current"),
        "the citation names root A, so the file's own root — not the ambiguity with root B — \
         decides the verdict: {named}"
    );

    // The same call, but the citation carries no root at all — the fallback
    // Task 2 must leave exactly as honest as it already was.
    let unnamed = call(
        &webview,
        "source_around",
        json!({
            "chunkId": chunk,
            "passageText": passage,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "citedRootId": null,
            "citedRelativePath": "README.md",
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");
    assert_eq!(unnamed["kind"], json!("excerpt"), "{unnamed}");
    assert_eq!(
        unnamed["freshness"]["kind"],
        json!("noPath"),
        "with no root on the citation, two roots hold this path and nothing may be guessed: \
         {unnamed}"
    );
}

/// The excerpt is coherent with the pin that let it through, or it is a
/// refusal. That is the whole oracle, and it is deliberately not an *order*.
///
/// [`assert_coherent_or_absent`]'s Round-3 reasoning applies verbatim one level
/// up. Which state a racing call reflects is the race's business: a call that
/// loses it outright and answers `Gone` has seen one coherent moment and is not
/// a defect, and demanding a particular winner would false-red on exactly that.
/// What can never be true is an `Excerpt` whose own blocks do not contain the
/// passage it was pinned to — the assertion is conditioned on the same fact
/// that made the response possible, since an excerpt exists *only* because
/// `chunk.text == passageText` at the moment the pin read it, so a window read
/// from that same moment must hold that text. If it does not, two statements
/// inside one command read two different moments and the user is looking at
/// another passage's paragraphs under their own citation.
///
/// ⚠️ **The blocks are compared joined, not one by one.** "Some block contains
/// the passage" is only equivalent for a single-block chunk; a passage spanning
/// two blocks is contained in neither alone, and a future multi-block fixture
/// would silently invert an oracle written the other way.
fn assert_excerpt_holds_its_passage(v: &Value, passage: &str) {
    if v["kind"] != json!("excerpt") {
        return;
    }
    let joined = v["blocks"]
        .as_array()
        .expect("an excerpt carries a blocks array")
        .iter()
        .map(|b| b["text"].as_str().expect("a block text"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains(passage),
        "the excerpt exists only because the pin read {passage:?} as this chunk's text, but the \
         window returned beside it does not contain that passage — two statements in one command \
         read two different moments, and these are another passage's paragraphs: {joined:?}"
    );

    // The second half of coherence, and the text check alone cannot see it.
    // The writer rebuilds the ORIGINAL text every other round, so a torn read
    // can return a *new* block carrying the *same* text: `joined` then contains
    // the passage and this looked green, while `spans` still named the block
    // that was deleted. PR 6 would paint the highlight nowhere, or into the
    // wrong paragraph.
    //
    // Never falsely red: in any coherent slice every span's block is in the
    // window by construction — `reading_window` takes the anchor blocks as
    // `reading_order BETWEEN first AND last`, and those two are the MIN and MAX
    // over exactly the blocks the spans name.
    let block_ids: Vec<i64> = v["blocks"]
        .as_array()
        .expect("an excerpt carries a blocks array")
        .iter()
        .map(|b| b["blockId"].as_i64().expect("a blockId"))
        .collect();
    for span in v["spans"].as_array().expect("an excerpt carries spans") {
        let named = span["blockId"].as_i64().expect("a span blockId");
        assert!(
            block_ids.contains(&named),
            "a span names block {named}, which is not among the blocks returned beside it \
             ({block_ids:?}) — the spans and the window read two different moments"
        );
    }
}

/// Hazard (1) with the rebuild landing *during* the command rather than before
/// it — the last thing the deterministic pin tests cannot reach.
///
/// [`source_around_refuses_a_chunk_id_a_rebuild_has_handed_to_other_text`]
/// proves the pin against a rebuild that has already committed. It says nothing
/// about a rebuild that commits *between* the pin's own `chunk_anchor` read and
/// the `reading_window` read that follows it. Under one
/// [`mnema_index::Db::read_snapshot`] those two statements see one moment and
/// the excerpt carries the paragraphs of the passage the pin let through;
/// without it they can see two, and the answer is the new chunk's
/// neighbourhood under the user's citation. See
/// [`assert_excerpt_holds_its_passage`] for why the oracle is coherence.
///
/// **Two fixture constraints, both found by review before this was written.**
///
/// 1. The writer must not rebuild the *same* text every round. Rebuild with the
///    original and the pin passes every single time, `Gone` never occurs, and
///    the "both outcomes" requirement then fails for a reason that has nothing
///    to do with the code. It alternates instead — the original text, then a
///    replacement unique to its round — so consecutive rounds always differ and
///    both outcomes stay reachable for the whole run.
/// 2. The oracle is valid only for a single-block chunk, and
///    [`rebuild_one_chunk`] mints exactly one block and one single-`Segment`
///    chunk. The joined-blocks comparison keeps that from being a hidden
///    premise.
///
/// **The distribution is recorded, not asserted, and that is deliberate.** This
/// is an unsynchronised thread race: how it falls is machine-dependent, and a
/// fast runner can win or lose every round. So the assertion fails *only* on an
/// incoherent excerpt, never on the distribution — a one-sided run is not a red
/// build, it is a silent loss of coverage, which is why the counts are printed
/// for a person to read into the ledger rather than asserted here
/// ([gate-reached-by-accident]).
#[test]
fn a_rebuild_racing_the_ipc_source_around_never_returns_another_passages_paragraphs() {
    const ORIGINAL: &str = "Ціна оцифрування одного аркуша становить дві гривні.";
    // The reader's calls. Fixed here rather than tuned when it flakes.
    const ROUNDS: usize = 200;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");
    let app = app_in(dir.path());
    let webview = main_webview(&app);
    call(&webview, "open_index", json!({})).expect("open_index was rejected");

    // The window and this writer are two connections on one file — the running
    // indexing job's shape, the same arrangement the search and `list_tree`
    // race fixtures rely on.
    let writer = mnema_index::open(&path).unwrap();
    let doc = "3".repeat(64);
    let target = write_one_document(&writer, &doc, ORIGINAL);

    // The fixture is the hazard only if SQLite hands the chunk rowid back after
    // a rebuild. If it did not, every call below would answer `noSuchChunk`,
    // the excerpt arm would never run, and a green race would mean nothing.
    // Asserted loudly rather than skipped with a `return`.
    writer.clear_document_content(&doc).unwrap();
    let again = rebuild_one_chunk(&writer, &doc, ORIGINAL);
    assert_eq!(
        again, target,
        "SQLite did not hand the chunk rowid back, so this fixture cannot reach the id-reuse \
         hazard it races against"
    );

    // Control, before the writer thread exists to race against: the command
    // answers with a coherent excerpt on its own, so anything below can only be
    // the race.
    let control = call(
        &webview,
        "source_around",
        json!({
            "chunkId": target,
            "passageText": ORIGINAL,
            "citedDocumentId": doc,
            "citedOrd": 0,
            "radius": 1,
        }),
    )
    .expect("source_around was rejected");
    assert_eq!(
        control["kind"],
        json!("excerpt"),
        "the fixture must answer with an excerpt before anything races it: {control}"
    );
    assert_excerpt_holds_its_passage(&control, ORIGINAL);

    // The writer runs until the reader is done rather than for a fixed count:
    // two DB writes are far cheaper than a full IPC round trip, so a
    // fixed-count writer would finish early and leave most of the reader's
    // calls running against a settled index — a green run with no race in it.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer_stop = std::sync::Arc::clone(&stop);
    let writer_doc = doc.clone();
    let writer_handle = std::thread::spawn(move || {
        let mut round = 0usize;
        while !writer_stop.load(std::sync::atomic::Ordering::Relaxed) {
            let text = if round.is_multiple_of(2) {
                ORIGINAL.to_string()
            } else {
                // Unique to the round, so no two consecutive rebuilds write the
                // same text and the pin has something to refuse.
                format!("Ставка залишається незмінною, редакція {round}.")
            };
            writer
                .clear_document_content(&writer_doc)
                .expect("clear the target document");
            rebuild_one_chunk(&writer, &writer_doc, &text);
            round += 1;
        }
        round
    });

    let mut excerpts = 0usize;
    let mut refusals = 0usize;
    // Split, because the two refusals are two different racing states and one
    // number cannot say which was reached: `idReused` means the reader landed
    // on a *rebuilt* chunk, `noSuchChunk` that it landed in the gap between
    // `clear_document_content` and the rebuild. Lumping them is the
    // [two-truths-one-message] shape at the level of the record itself.
    let mut reused = 0usize;
    for _ in 0..ROUNDS {
        let v = call(
            &webview,
            "source_around",
            json!({
                "chunkId": target,
                "passageText": ORIGINAL,
                "citedDocumentId": doc,
                "citedOrd": 0,
                "radius": 1,
            }),
        )
        .expect("source_around was rejected");
        match v["kind"].as_str().expect("a kind tag") {
            "excerpt" => excerpts += 1,
            "gone" => {
                refusals += 1;
                if v["reason"]["kind"] == json!("idReused") {
                    reused += 1;
                }
            }
            other => panic!("source_around answered with an unknown variant {other:?}: {v}"),
        }
        assert_excerpt_holds_its_passage(&v, ORIGINAL);
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let rebuilds = writer_handle.join().expect("the writer thread panicked");

    // The one fact about this run that is NOT machine-dependent, so it is the
    // one thing asserted rather than printed: the writer stops only after all
    // 200 IPC round-trips are done, so zero rebuilds means the writer never
    // ran and these were 200 uncontended calls — a green that proves nothing.
    // The excerpt/gone split genuinely does depend on timing, which is why it
    // is printed for a person to read instead.
    assert!(
        rebuilds > 0,
        "the writer never rebuilt once, so nothing raced and this run is not evidence"
    );

    // Read with `-- --nocapture`. Both counts non-zero is what says the fixture
    // reached a racing state at all; one-sided means the guard is decoration
    // this run, and that goes in the ledger rather than passing as a green.
    eprintln!(
        "source_around race: {ROUNDS} calls -> excerpt={excerpts} gone={refusals} \
         (idReused={reused}, noSuchChunk={}); writer rebuilt {rebuilds} times",
        refusals - reused
    );
}
